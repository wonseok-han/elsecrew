//! Backend-selected executable bytes and path identity. Never learns a trusted
//! digest from the candidate itself. Signature/version/closure admission is separate.
use crate::{provider_profile::open_absolute_directory, SidecarError};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{File, MetadataExt, OpenOptions, OpenOptionsExt};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 512 * 1024 * 1024;

fn failure() -> SidecarError {
    SidecarError::new(
        "providerExecutableChanged",
        "provider executable could not be verified",
    )
}

pub(crate) struct PinnedExecutable {
    path: PathBuf,
    held: File,
    digest: [u8; 32],
}

impl PinnedExecutable {
    /// Trusted backend supplies the expected digest, not a renderer or manifest
    /// accompanying an untrusted binary. This does not grant execution authority.
    pub(crate) fn open(path: &Path, expected: [u8; 32]) -> Result<Self, SidecarError> {
        let held = open_file(path)?;
        if hash(&held)? != expected {
            return Err(failure());
        }
        let value = Self {
            path: path.into(),
            held,
            digest: expected,
        };
        value.revalidate()?;
        Ok(value)
    }

    /// Point-in-time path-to-handle and content check. Does not protect against
    /// hostile same-UID mutation after returning; never treat this as fd-exec.
    pub(crate) fn revalidate(&self) -> Result<&Path, SidecarError> {
        let current = open_file(&self.path)?;
        let old = self.held.metadata().map_err(|_| failure())?;
        let new = current.metadata().map_err(|_| failure())?;
        if old.dev() != new.dev() || old.ino() != new.ino() || hash(&current)? != self.digest {
            return Err(failure());
        }
        Ok(&self.path)
    }
}

fn open_file(path: &Path) -> Result<File, SidecarError> {
    let parent = open_absolute_directory(path.parent().ok_or_else(failure)?)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let file = parent
        .open_with(path.file_name().ok_or_else(failure)?, &options)
        .map_err(|_| failure())?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.mode() & 0o6022 != 0
        || meta.mode() & 0o111 == 0
        || meta.len() == 0
        || meta.len() > MAX_BYTES
        || (meta.uid() != 0 && meta.uid() != unsafe { libc::geteuid() })
    {
        return Err(failure());
    }
    Ok(file)
}

fn hash(file: &File) -> Result<[u8; 32], SidecarError> {
    let before = file.metadata().map_err(|_| failure())?;
    let mut reader = file;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = reader.read(&mut buffer).map_err(|_| failure())?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_BYTES {
            return Err(failure());
        }
        hasher.update(&buffer[..count]);
    }
    let after = file.metadata().map_err(|_| failure())?;
    if total != before.len()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(failure());
    }
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn candidate(root: &Path) -> PathBuf {
        let file = root.join("tool");
        std::fs::write(&file, b"test executable bytes").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        file
    }
    fn digest() -> [u8; 32] {
        Sha256::digest(b"test executable bytes").into()
    }

    #[test]
    fn matching_pin_revalidates_but_in_place_changes_and_replacements_do_not() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = candidate(&root);
        assert!(PinnedExecutable::open(&file, [0; 32]).is_err());
        let pin = PinnedExecutable::open(&file, digest()).unwrap();
        pin.revalidate().unwrap();
        pin.revalidate().unwrap();
        std::fs::write(&file, b"evil executable bytes").unwrap();
        assert!(pin.revalidate().is_err());
        std::fs::rename(&file, root.join("old")).unwrap();
        candidate(&root);
        assert!(pin.revalidate().is_err());
    }

    #[test]
    fn links_permissions_and_nonfiles_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = candidate(&root);
        symlink(&file, root.join("link")).unwrap();
        assert!(PinnedExecutable::open(&root.join("link"), digest()).is_err());
        symlink(&root, root.join("parent-link")).unwrap();
        assert!(PinnedExecutable::open(&root.join("parent-link/tool"), digest()).is_err());
        assert!(PinnedExecutable::open(&root, digest()).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(PinnedExecutable::open(&file, digest()).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(PinnedExecutable::open(&file, digest()).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::hard_link(&file, root.join("hard-link")).unwrap();
        assert!(PinnedExecutable::open(&file, digest()).is_err());
    }
}
