//! Bind an explicitly selected image to the debugger's actual image handle.
//! This is file identity, NOT publisher/installation/DLL trust or admission.
#![allow(dead_code)]
use super::{
    windows_directory_lease::{DirectoryLease, RetainedPin},
    windows_fs, ProjectFailure,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{ffi::OsStr, os::windows::io::AsRawHandle};

pub(super) struct ExecutableLease {
    parent: DirectoryLease,
    image: File,
    identity: windows_fs::Identity,
    path: String,
}
pub(super) type RetainedExecutable = RetainedPin<ExecutableLease>;
impl ExecutableLease {
    /// Caller supplies selected parent authority, never PATH or a repository
    /// executable name. Trust origin must be established separately before use.
    pub(super) fn acquire(parent: &Dir, name: &OsStr) -> Result<Self, ProjectFailure> {
        let pinned_parent = DirectoryLease::acquire(parent, name)?;
        let name = name
            .to_str()
            .ok_or_else(|| ProjectFailure::manifest("image name is not Unicode"))?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .share_mode(1)
            .custom_flags(windows_fs::OPEN_REPARSE_POINT);
        let image = parent.open_with(name, &options).map_err(|error| {
            ProjectFailure::recovery(format!("could not retain selected image: {error}"))
        })?;
        let identity = windows_fs::identity(&image, false)?;
        let path = format!(
            "{}{}{}",
            pinned_parent.path(),
            if pinned_parent.path().ends_with('\\') {
                ""
            } else {
                "\\"
            },
            name
        );
        let lease = Self {
            parent: pinned_parent,
            image,
            identity,
            path,
        };
        lease.validate()?;
        Ok(lease)
    }
    /// OS chain/signature validity only. Does not identify an approved Git publisher.
    pub(super) fn verify_authenticode(&self) -> Result<(), ProjectFailure> {
        self.validate()?;
        super::windows_authenticode::verify(&self.image, &self.path)?;
        self.validate()
    }
    pub(super) fn path(&self) -> &str {
        &self.path
    }
    pub(super) fn validate(&self) -> Result<(), ProjectFailure> {
        self.parent.validate()?;
        if windows_fs::identity(&self.image, false)? != self.identity {
            return Err(ProjectFailure::symlink(
                "selected executable identity changed",
            ));
        }
        Ok(())
    }
    pub(super) fn verify_loaded_image<T: AsRawHandle>(
        &self,
        actual: &T,
    ) -> Result<(), ProjectFailure> {
        self.validate()?;
        if windows_fs::identity(actual, false)? != self.identity {
            return Err(ProjectFailure::symlink(
                "loaded image differs from selected executable",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    #[test]
    fn image_binding_rejects_different_file_and_conflicting_write_access() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("selected.exe"), b"selected").unwrap();
        std::fs::write(temp.path().join("different.exe"), b"different").unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("selected.exe"))
            .unwrap();
        assert!(ExecutableLease::acquire(&dir, OsStr::new("selected.exe")).is_err());
        drop(writer);
        let lease = ExecutableLease::acquire(&dir, OsStr::new("selected.exe")).unwrap();
        let same = std::fs::File::open(temp.path().join("selected.exe")).unwrap();
        let other = std::fs::File::open(temp.path().join("different.exe")).unwrap();
        lease.verify_loaded_image(&same).unwrap();
        assert!(lease.verify_loaded_image(&other).is_err());
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("selected.exe"))
            .is_err());
        assert!(std::fs::rename(
            temp.path().join("selected.exe"),
            temp.path().join("moved.exe")
        )
        .is_err());
        drop(lease);
        std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("selected.exe"))
            .unwrap();
    }
}
