//! Backend-authenticated executable bytes and path identity. Release pins and
//! publisher signatures are distinct admission mechanisms; runtime bytes stay bound.
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
    #[cfg(any(test, target_os = "macos"))]
    signature: Option<SignatureBinding>,
}

#[cfg(any(test, target_os = "macos"))]
type SignatureStamp = (u64, u64, u64, u32, u32, u64, i64, i64, i64, i64);

#[cfg(any(test, target_os = "macos"))]
struct SignatureBinding {
    executable: SignatureStamp,
    ancestors: Vec<(PathBuf, cap_std::fs::Dir, SignatureStamp)>,
}

#[cfg(any(test, target_os = "macos"))]
fn signature_directory(path: &Path) -> Result<cap_std::fs::Dir, SidecarError> {
    if path == Path::new("/") {
        cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|_| failure())
    } else {
        open_absolute_directory(path)
    }
}

impl PinnedExecutable {
    /// Redacted identity of the currently observed file, not a pre-registered
    /// release hash or a version allowlist. Caller supplies a measured version.
    pub(crate) fn public_identity(
        &self,
        cli_version: &str,
    ) -> Result<hello_agent_protocol::agent_adapter_v1::AgentExecutableIdentityV1, SidecarError>
    {
        self.revalidate()?;
        if cli_version.is_empty() || cli_version.len() > 128 {
            return Err(failure());
        }
        let metadata = self.held.metadata().map_err(|_| failure())?;
        let path_digest = format!(
            "sha256:{:x}",
            Sha256::digest(self.path.as_os_str().as_encoded_bytes())
        );
        let mut identity = Sha256::new();
        identity.update(metadata.dev().to_le_bytes());
        identity.update(metadata.ino().to_le_bytes());
        identity.update(self.digest);
        Ok(
            hello_agent_protocol::agent_adapter_v1::AgentExecutableIdentityV1 {
                path_digest,
                file_identity_digest: format!("sha256:{:x}", identity.finalize()),
                cli_version: cli_version.into(),
            },
        )
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn open_codex_signed(path: &Path, helper: bool) -> Result<Self, SidecarError> {
        let requirement = if helper {
            "=anchor apple generic and certificate leaf[subject.OU] = \"2DC432GLL2\" and identifier \"codex-code-mode-host\""
        } else {
            "=anchor apple generic and certificate leaf[subject.OU] = \"2DC432GLL2\" and identifier \"codex\""
        };
        Self::open_authenticated(path, |path| verify_signature(path, requirement))
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn open_claude_signed(path: &Path) -> Result<Self, SidecarError> {
        Self::open_authenticated(path, |path| {
            verify_signature(path,
            "=anchor apple generic and certificate leaf[subject.OU] = \"Q6L2SF6YDW\" and identifier \"com.anthropic.claude-code\"")
        })
    }

    #[cfg(any(test, target_os = "macos"))]
    fn open_authenticated(
        path: &Path,
        authenticate: impl FnOnce(&Path) -> Result<(), SidecarError>,
    ) -> Result<Self, SidecarError> {
        let held = open_file(path)?;
        let original = signature_stamp(&held.metadata().map_err(|_| failure())?);
        let mut ancestors = Vec::new();
        for ancestor in path.parent().ok_or_else(failure)?.ancestors() {
            let dir = signature_directory(ancestor)?;
            let stamp = signature_stamp(&dir.dir_metadata().map_err(|_| failure())?);
            ancestors.push((ancestor.to_owned(), dir, stamp));
        }
        let digest = hash(&held)?;
        authenticate(path)?;
        let value = Self {
            path: path.into(),
            held,
            digest,
            signature: Some(SignatureBinding {
                executable: original,
                ancestors,
            }),
        };
        // Reject replacement or in-place mutation across signature verification.
        value.revalidate()?;
        Ok(value)
    }

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
            #[cfg(any(test, target_os = "macos"))]
            signature: None,
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
        #[cfg(any(test, target_os = "macos"))]
        if let Some(binding) = &self.signature {
            if signature_stamp(&self.held.metadata().map_err(|_| failure())?) != binding.executable
            {
                return Err(failure());
            }
            for (path, dir, initial) in &binding.ancestors {
                let held = signature_stamp(&dir.dir_metadata().map_err(|_| failure())?);
                let linked = signature_stamp(
                    &signature_directory(path)?
                        .dir_metadata()
                        .map_err(|_| failure())?,
                );
                if held != *initial || linked != *initial {
                    return Err(failure());
                }
            }
        }
        Ok(&self.path)
    }
}

#[cfg(any(test, target_os = "macos"))]
fn signature_stamp(meta: &cap_std::fs::Metadata) -> SignatureStamp {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mode(),
        meta.uid(),
        meta.nlink(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}

#[cfg(target_os = "macos")]
fn verify_signature(path: &Path, requirement: &str) -> Result<(), SidecarError> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    // Publisher and identifiers were verified on the previously trusted release.
    // Neither values nor requirement expressions come from renderer/candidate data.
    let mut child = Command::new("/usr/bin/codesign")
        .args([
            "--verify",
            "--strict",
            "--all-architectures",
            "-R",
            requirement,
        ])
        .arg(path)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| failure())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(failure())
                }
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure());
            }
        }
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

    // Other parallel fixtures can change a shared temp ancestor. The production
    // check deliberately rejects that observation; only positive controls retry.
    fn stable_authenticated_fixture(path: &Path) -> PinnedExecutable {
        for _ in 0..32 {
            if let Ok(pin) = PinnedExecutable::open_authenticated(path, |_| Ok(())) {
                if pin.revalidate().is_ok() {
                    return pin;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("no stable ancestor observation for positive fixture");
    }

    #[test]
    fn authenticated_updates_bind_current_bytes_and_reject_failed_or_raced_authentication() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = candidate(&root);
        assert!(PinnedExecutable::open_authenticated(&file, |_| Err(failure())).is_err());
        let first = stable_authenticated_fixture(&file);
        std::fs::write(&file, b"new authenticated release").unwrap();
        assert!(first.revalidate().is_err());
        stable_authenticated_fixture(&file);
        assert!(PinnedExecutable::open_authenticated(&file, |path| {
            std::fs::write(path, b"changed while verifying").unwrap();
            Ok(())
        })
        .is_err());
    }

    #[test]
    fn authentication_rejects_file_and_parent_swap_then_restore() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let file = candidate(&nested);
        let mut reached = false;
        assert!(PinnedExecutable::open_authenticated(&file, |path| {
            reached = true;
            std::fs::rename(path, nested.join("held")).unwrap();
            candidate(&nested);
            std::fs::remove_file(path).unwrap();
            std::fs::rename(nested.join("held"), path).unwrap();
            Ok(())
        })
        .is_err());
        assert!(reached);
        reached = false;
        assert!(PinnedExecutable::open_authenticated(&file, |_| {
            reached = true;
            std::fs::rename(&nested, root.join("held-dir")).unwrap();
            std::fs::create_dir(&nested).unwrap();
            candidate(&nested);
            std::fs::remove_file(nested.join("tool")).unwrap();
            std::fs::remove_dir(&nested).unwrap();
            std::fs::rename(root.join("held-dir"), &nested).unwrap();
            Ok(())
        })
        .is_err());
        assert!(reached);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unsigned_file_cannot_pass_codex_publisher_verification() {
        let temp = tempfile::tempdir().unwrap();
        let file = candidate(&temp.path().canonicalize().unwrap());
        assert!(PinnedExecutable::open_codex_signed(&file, false).is_err());
        assert!(PinnedExecutable::open_codex_signed(&file, true).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "read-only installed publisher-signature verification; never executes provider"]
    fn installed_codex_pair_passes_publisher_verification_without_release_pins() {
        let root = Path::new("/Applications/ChatGPT.app/Contents/Resources");
        let cli = root.join("codex-cli/CodexCLI.app/Contents/MacOS/codex");
        let host = root.join("codex-cli/bin/codex-code-mode-host");
        PinnedExecutable::open_codex_signed(&cli, false)
            .unwrap()
            .revalidate()
            .unwrap();
        PinnedExecutable::open_codex_signed(&host, true)
            .unwrap()
            .revalidate()
            .unwrap();
        assert!(PinnedExecutable::open_codex_signed(&host, false).is_err());
        assert!(PinnedExecutable::open_codex_signed(Path::new("/usr/bin/true"), false).is_err());
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
