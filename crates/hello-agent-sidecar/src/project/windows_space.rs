//! Volume capacity queried through a synchronous handle, never a pathname.
//! https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntqueryvolumeinformationfile

fn available_bytes(total: i64, caller: i64, actual: i64, sectors: u32, bytes: u32) -> Option<u64> {
    if caller < 0 || actual < caller || total < actual || sectors == 0 || bytes == 0 {
        return None;
    }
    (caller as u64)
        .checked_mul(sectors.into())?
        .checked_mul(bytes.into())
}

#[cfg(windows)]
pub(super) fn available(directory: &cap_std::fs::Dir) -> Result<u64, super::ProjectFailure> {
    use super::{windows_fs, ProjectFailure};
    use std::{
        ffi::c_void,
        os::windows::io::{AsRawHandle, FromRawHandle},
    };

    #[repr(C)]
    union Status {
        status: i32,
        _pointer: *mut c_void,
    }
    #[repr(C)]
    struct IoStatus {
        status: Status,
        information: usize,
    }
    #[repr(C)]
    #[derive(Default)]
    struct FullSize {
        total: i64,
        caller: i64,
        actual: i64,
        sectors: u32,
        bytes: u32,
    }
    struct Query {
        io: IoStatus,
        size: FullSize,
    }
    const _: () = assert!(std::mem::size_of::<FullSize>() == 32);

    #[link(name = "kernel32")]
    extern "system" {
        fn ReOpenFile(handle: *mut c_void, access: u32, share: u32, flags: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryVolumeInformationFile(
            handle: *mut c_void,
            status: *mut IoStatus,
            buffer: *mut c_void,
            length: u32,
            class: i32,
        ) -> i32;
    }

    let expected = windows_fs::identity(directory, true)?;
    windows_fs::require_local_volume(directory)?;
    // No OVERLAPPED flag: create our own synchronous file object, independent
    // of how the caller's handle was opened. ReOpenFile uses the held object.
    // SAFETY: directory owns the input handle; constants request generic read,
    // read/write/delete sharing, directory access and no reparse processing.
    let raw = unsafe {
        ReOpenFile(
            directory.as_raw_handle(),
            0x80000000,
            7,
            0x02000000 | windows_fs::OPEN_REPARSE_POINT,
        )
    };
    if raw.is_null() || raw as isize == -1 {
        return Err(ProjectFailure::recovery(format!(
            "could not reopen disk capability: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: successful ReOpenFile returned a new owned handle.
    let held = unsafe { std::fs::File::from_raw_handle(raw) };
    if windows_fs::identity(&held, true)? != expected {
        return Err(ProjectFailure::manifest(
            "disk capability changed during reopen",
        ));
    }
    let mut query = Box::new(Query {
        io: IoStatus {
            status: Status { status: 0x103 },
            information: 0,
        },
        size: FullSize::default(),
    });
    // SAFETY: the buffers have IO_STATUS_BLOCK / FILE_FS_FULL_SIZE_INFORMATION
    // layouts and stay alive until completion, including an unexpected pending
    // result. The synchronous owned handle remains open throughout the query.
    let mut status = unsafe {
        NtQueryVolumeInformationFile(
            held.as_raw_handle(),
            &mut query.io,
            (&mut query.size as *mut FullSize).cast(),
            std::mem::size_of::<FullSize>() as u32,
            7,
        )
    };
    if status == 0x103 {
        // STATUS_PENDING: keep native output storage alive.
        let waited = unsafe { WaitForSingleObject(held.as_raw_handle(), u32::MAX) };
        if waited != 0 || unsafe { query.io.status.status } == 0x103 {
            // Do not release buffers or their handle while a driver might still
            // write to them. This exceptional OS failure cannot grant capacity.
            std::mem::forget(query);
            std::mem::forget(held);
            return Err(ProjectFailure::recovery(
                "disk query completion could not be confirmed",
            ));
        }
        status = unsafe { query.io.status.status };
    }
    if status != 0
        || unsafe { query.io.status.status } != 0
        || query.io.information != std::mem::size_of::<FullSize>()
    {
        return Err(ProjectFailure::recovery(format!(
            "disk query did not return complete capacity information: {status:#x}"
        )));
    }
    if windows_fs::identity(&held, true)? != expected {
        return Err(ProjectFailure::manifest(
            "disk capability changed during query",
        ));
    }
    available_bytes(
        query.size.total,
        query.size.caller,
        query.size.actual,
        query.size.sectors,
        query.size.bytes,
    )
    .ok_or_else(|| ProjectFailure::recovery("disk capacity geometry is invalid or overflows"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_caller_quota_and_rejects_invalid_geometry() {
        assert_eq!(available_bytes(1000, 20, 100, 8, 512), Some(81920));
        assert_eq!(available_bytes(1000, 0, 100, 8, 512), Some(0));
        for args in [
            (10, -1, 5, 1, 512),
            (10, 6, 5, 1, 512),
            (10, 5, 11, 1, 512),
            (10, 5, 5, 0, 512),
            (10, 5, 5, 1, 0),
            (i64::MAX, i64::MAX, i64::MAX, u32::MAX, u32::MAX),
        ] {
            assert_eq!(
                available_bytes(args.0, args.1, args.2, args.3, args.4),
                None
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn measures_owned_directory_without_resolving_its_saved_path() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        std::fs::create_dir(&original).unwrap();
        // cap-std's normal directory open denies rename on Windows. This
        // adversarial fixture deliberately permits deletion/rename, so the
        // query must demonstrate that it uses the held object, not its path.
        use std::os::windows::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(7)
            .custom_flags(0x02000000 | super::super::windows_fs::OPEN_REPARSE_POINT)
            .open(&original)
            .unwrap();
        let dir = cap_std::fs::Dir::from_std_file(file);
        std::fs::rename(&original, temp.path().join("moved")).unwrap();
        std::fs::write(&original, b"not the held directory").unwrap();
        available(&dir).unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"not the held directory");
    }
}
