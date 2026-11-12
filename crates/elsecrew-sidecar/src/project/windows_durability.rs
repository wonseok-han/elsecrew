//! NTFS-specific write-through policy for save metadata. This records an OS
//! flush acknowledgement, not a guarantee about arbitrary disk-controller caches.
use super::ProjectFailure;
use std::{ffi::c_void, os::windows::io::AsRawHandle, ptr};

pub(super) const WRITE_THROUGH: u32 = 0x8000_0000;
pub(super) const POLICY: &str = "ntfsWriteThroughV1";
pub(super) const FLUSHED: &str = "ntfsWriteThroughFlushed";

#[link(name = "kernel32")]
extern "system" {
    fn GetVolumeInformationByHandleW(
        handle: *mut c_void,
        volume_name: *mut u16,
        volume_name_size: u32,
        serial: *mut u32,
        max_component: *mut u32,
        flags: *mut u32,
        filesystem: *mut u16,
        filesystem_size: u32,
    ) -> i32;
}

/// Query the filesystem through the already held object; no drive-letter lookup
/// or elevated volume flush. The documented rename metadata policy is NTFS-only.
pub(super) fn require_ntfs<T: AsRawHandle>(object: &T) -> Result<(), ProjectFailure> {
    let mut name = [0u16; 32];
    // SAFETY: a live file/directory handle and a correctly sized writable buffer.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            object.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(ProjectFailure::recovery(format!(
            "could not identify save filesystem: {}",
            std::io::Error::last_os_error()
        )));
    }
    let end = name
        .iter()
        .position(|unit| *unit == 0)
        .ok_or_else(|| ProjectFailure::recovery("save filesystem name is unterminated"))?;
    if name[..end] != [b'N' as u16, b'T' as u16, b'F' as u16, b'S' as u16] {
        return Err(ProjectFailure::recovery(
            "Windows durable save currently requires local NTFS",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{windows_directory_lease::DirectoryLease, windows_replace};
    use cap_std::{ambient_authority, fs::Dir};
    use std::ffi::OsStr;

    #[test]
    fn ntfs_write_through_source_can_be_flushed_after_handle_relative_rename() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        require_ntfs(&dir).unwrap();
        std::fs::write(temp.path().join("guard"), b"guard").unwrap();
        std::fs::write(temp.path().join("target"), b"old").unwrap();
        let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        let mut source = dir
            .open_with("source", &windows_replace::source_options())
            .unwrap();
        require_ntfs(&source).unwrap();
        std::io::Write::write_all(&mut source, b"new").unwrap();
        source.sync_all().unwrap();
        windows_replace::replace(&source, &lease, "target").unwrap();
        source.sync_all().unwrap();
        assert_eq!(std::fs::read(temp.path().join("target")).unwrap(), b"new");
        assert!(!temp.path().join("source").exists());
        // This is API/namespace integration coverage, not a power-loss test.
    }
}
