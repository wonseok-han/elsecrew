//! Exact-handle deletion for journal-owned temporary files, not a cleanup policy.
//! Caller must persist intent and bind the directory lease to the current managed
//! workspace. Absence after close is an observation, not namespace durability.
#![allow(dead_code)]
use super::{windows_directory_lease::DirectoryLease, windows_fs, ProjectFailure};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{File, OpenOptions, OpenOptionsExt};
use std::{
    ffi::c_void,
    os::windows::io::{AsRawHandle, IntoRawHandle},
};

#[link(name = "kernel32")]
extern "system" {
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn SetFileInformationByHandle(
        handle: *mut c_void,
        class: i32,
        information: *const c_void,
        bytes: u32,
    ) -> i32;
}

/// This function never falls back to a pathname deletion and never retries a
/// missing/replaced file. An independent guard must keep the parent pinned.
pub(super) fn remove_recorded(
    lease: &DirectoryLease,
    name: &str,
    expected_identity: &str,
) -> Result<(), ProjectFailure> {
    remove_recorded_checked(lease, name, expected_identity, &|_| Ok(()))
}

/// Optional policy check runs on the same exclusive handle that is deleted,
/// closing a metadata-observation/reopen gap for explicit archive cleanup.
pub(super) fn remove_recorded_checked(
    lease: &DirectoryLease,
    name: &str,
    expected_identity: &str,
    check: &dyn Fn(&File) -> Result<(), ProjectFailure>,
) -> Result<(), ProjectFailure> {
    // Also rejects guard aliases, paths, streams, reserved names and directories.
    let parent = lease.replacement_parent(name)?;
    super::windows_durability::require_ntfs(parent)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .access_mode(0xc001_0000) // GENERIC_READ | GENERIC_WRITE | DELETE
        .share_mode(0)
        .follow(FollowSymlinks::No)
        .custom_flags(windows_fs::OPEN_REPARSE_POINT | super::windows_durability::WRITE_THROUGH);
    let file = parent.open_with(name, &options).map_err(|error| {
        ProjectFailure::recovery(format!(
            "could not exclusively open recorded cleanup file: {error}"
        ))
    })?;
    if windows_fs::identity(&file, false)?.recovery_token() != expected_identity {
        return Err(ProjectFailure::recovery(
            "cleanup file identity differs from durable record",
        ));
    }
    check(&file)?;
    lease.validate()?;
    // FILE_DISPOSITION_INFO contains one BOOLEAN, not a Win32 four-byte BOOL.
    // No POSIX/ignore-readonly/delete-on-close fallback is used.
    let delete: u8 = 1;
    // SAFETY: the live exclusive handle refers to the ID just checked; the
    // one-byte structure matches FileDispositionInfo and outlives the call.
    let success = unsafe {
        SetFileInformationByHandle(file.as_raw_handle(), 4, (&delete as *const u8).cast(), 1)
    };
    if success == 0 {
        return Err(ProjectFailure::recovery(format!(
            "recorded file deletion was rejected: {}",
            std::io::Error::last_os_error()
        )));
    }
    let raw = file.into_std().into_raw_handle();
    // SAFETY: ownership moved out of File; this is the only close attempt.
    // On failure preserve the recovery fence rather than retry an uncertain handle.
    if unsafe { CloseHandle(raw) } == 0 {
        return Err(ProjectFailure::ambiguous(
            "cleanup disposition set but handle close failed",
        ));
    }
    lease.validate().map_err(|error| {
        ProjectFailure::ambiguous(format!(
            "cleanup was requested but parent revalidation failed: {}",
            error.detail
        ))
    })?;
    match parent.symlink_metadata(name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(ProjectFailure::ambiguous(
            "cleanup was requested but absence is unconfirmed; do not retry by path",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::{ambient_authority, fs::Dir};
    use std::ffi::OsStr;

    #[test]
    fn cleanup_requires_exact_identity_and_preserves_guard_and_foreign_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("guard"), b"keep guard").unwrap();
        std::fs::write(temp.path().join("owned"), b"owned").unwrap();
        std::fs::write(temp.path().join("foreign"), b"foreign").unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        let id = windows_fs::identity(&dir.open("owned").unwrap(), false)
            .unwrap()
            .recovery_token();
        let foreign = windows_fs::identity(&dir.open("foreign").unwrap(), false)
            .unwrap()
            .recovery_token();
        assert_ne!(id, foreign);
        assert!(remove_recorded(&lease, "foreign", &id).is_err());
        assert!(remove_recorded(&lease, "guard", &id).is_err());
        assert!(remove_recorded(&lease, "../owned", &id).is_err());
        let held = dir.open("owned").unwrap();
        assert!(remove_recorded(&lease, "owned", &id).is_err());
        assert_eq!(std::fs::read(temp.path().join("owned")).unwrap(), b"owned");
        drop(held);
        let checked = std::cell::Cell::new(false);
        assert!(remove_recorded_checked(&lease, "owned", &id, &|file| {
            checked.set(true);
            assert_eq!(windows_fs::identity(file, false)?.recovery_token(), id);
            Err(ProjectFailure::recovery("changed consent observation"))
        })
        .is_err());
        assert!(checked.get());
        assert_eq!(std::fs::read(temp.path().join("owned")).unwrap(), b"owned");
        remove_recorded(&lease, "owned", &id).unwrap();
        assert!(!temp.path().join("owned").exists());
        assert_eq!(
            std::fs::read(temp.path().join("foreign")).unwrap(),
            b"foreign"
        );
        assert_eq!(
            std::fs::read(temp.path().join("guard")).unwrap(),
            b"keep guard"
        );
        // Reusing the old name must not grant ownership of its replacement.
        std::fs::rename(temp.path().join("foreign"), temp.path().join("owned")).unwrap();
        assert!(remove_recorded(&lease, "owned", &id).is_err());
        assert_eq!(
            std::fs::read(temp.path().join("owned")).unwrap(),
            b"foreign"
        );
    }
}
