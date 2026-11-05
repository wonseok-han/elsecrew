//! Handle-relative replacement primitive, not a complete save transaction.
//! The caller must own the durable intent, version check, temp identity, and
//! post-rename durability/recovery. Product save admission remains disabled.
#![allow(dead_code)]
use super::{windows_directory_lease::DirectoryLease, windows_fs, ProjectFailure};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{File, OpenOptions, OpenOptionsExt};
use std::{
    ffi::c_void,
    mem::{offset_of, size_of},
    os::windows::io::AsRawHandle,
};

/// Create the replacement source once and retain this handle until the rename.
/// No second writer or deleter is admitted while the source is prepared.
pub(super) fn source_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No)
        .access_mode(0xc00d0000) // GENERIC_READ | GENERIC_WRITE | DELETE | WRITE_DAC | WRITE_OWNER
        .share_mode(1)
        .custom_flags(windows_fs::OPEN_REPARSE_POINT | super::windows_durability::WRITE_THROUGH);
    options
}

#[repr(C)]
struct RenameInfo {
    replace: u32,
    root: *mut c_void,
    name_bytes: u32,
    name: [u16; 1],
}

// IO_STATUS_BLOCK contains a pointer-sized status/pointer union and ULONG_PTR.
#[repr(C)]
#[derive(Default)]
struct IoStatusBlock {
    status: usize,
    information: usize,
}

const _: () = assert!(offset_of!(RenameInfo, root) == size_of::<usize>());
const _: () = assert!(offset_of!(RenameInfo, name_bytes) == 2 * size_of::<usize>());
const _: () = assert!(offset_of!(RenameInfo, name) == 2 * size_of::<usize>() + 4);
const _: () = assert!(size_of::<IoStatusBlock>() == 2 * size_of::<usize>());

#[link(name = "ntdll")]
extern "system" {
    fn NtSetInformationFile(
        handle: *mut c_void,
        status: *mut IoStatusBlock,
        information: *const c_void,
        bytes: u32,
        class: i32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

/// Does not open a source pathname or resolve a multi-component destination.
/// No POSIX/ignore-readonly flags and no delete/copy fallback are used.
pub(super) fn replace(
    source: &File,
    destination: &DirectoryLease,
    name: &str,
) -> Result<(), ProjectFailure> {
    let parent = destination.replacement_parent(name)?;
    windows_fs::identity(source, false)?;
    let wide: Vec<u16> = name.encode_utf16().collect();
    if wide.is_empty() || wide.len() > 255 || wide.contains(&0) {
        return Err(ProjectFailure::manifest("invalid replacement leaf name"));
    }
    // Word allocation provides the native pointer alignment. Include room for
    // a terminator although FileNameLength describes only actual UTF-16 bytes.
    let length = size_of::<RenameInfo>() + (wide.len() + 1) * 2;
    let mut words = vec![0usize; length.div_ceil(size_of::<usize>())];
    let info = words.as_mut_ptr().cast::<RenameInfo>();
    // SAFETY: aligned allocation is larger than the fixed header and name;
    // both handles and the allocation outlive this synchronous call.
    unsafe {
        (*info).replace = 1;
        (*info).root = parent.as_raw_handle();
        (*info).name_bytes = (wide.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            wide.as_ptr(),
            words
                .as_mut_ptr()
                .cast::<u8>()
                .add(offset_of!(RenameInfo, name))
                .cast::<u16>(),
            wide.len(),
        );
    }
    destination.validate()?;
    // Native FileRenameInformation = 10 supports the held RootDirectory. The
    // Win32 FileRenameInfo wrapper rejected this relative form with error 87.
    // source_options creates a synchronous file (no FILE_FLAG_OVERLAPPED), so
    // the native call completes before this stack-owned IO_STATUS_BLOCK expires.
    let mut completion = IoStatusBlock::default();
    let result = unsafe {
        NtSetInformationFile(
            source.as_raw_handle(),
            &mut completion,
            words.as_ptr().cast(),
            length as u32,
            10,
        )
    };
    if result < 0 {
        let code = unsafe { RtlNtStatusToDosError(result) };
        return Err(ProjectFailure::recovery(format!(
            "Windows handle-relative replace failed: {} (NTSTATUS {result:#x})",
            std::io::Error::from_raw_os_error(code as i32)
        )));
    }
    // Success here is rename only; never clear a durable save lock here.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::{ambient_authority, fs::Dir};
    use std::{ffi::OsStr, os::windows::fs::OpenOptionsExt};

    #[test]
    fn replacement_is_relative_to_pinned_parent_and_preserves_guard() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("destination");
        std::fs::create_dir(&parent).unwrap();
        std::fs::write(parent.join("guard"), b"guard").unwrap();
        std::fs::write(parent.join("file.txt"), b"before").unwrap();
        let staging = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let dir = Dir::open_ambient_dir(&parent, ambient_authority()).unwrap();
        let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        let mut source = staging.open_with("staged", &source_options()).unwrap();
        std::io::Write::write_all(&mut source, b"after").unwrap();
        source.sync_all().unwrap();
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("staged"))
            .is_err());
        assert!(std::fs::remove_file(temp.path().join("staged")).is_err());
        assert!(std::fs::rename(temp.path().join("staged"), temp.path().join("moved")).is_err());
        let identity = windows_fs::identity(&source, false).unwrap();
        for name in ["guard", "../escape", "sub/file", "file:stream", "NUL"] {
            assert!(replace(&source, &lease, name).is_err());
            assert_eq!(std::fs::read(parent.join("file.txt")).unwrap(), b"before");
        }
        let held_target = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(parent.join("file.txt"))
            .unwrap();
        assert!(replace(&source, &lease, "file.txt").is_err());
        assert_eq!(std::fs::read(parent.join("file.txt")).unwrap(), b"before");
        assert!(temp.path().join("staged").exists());
        drop(held_target);
        std::fs::write(parent.join("Å.txt"), b"unicode guard").unwrap();
        let unicode_guard = DirectoryLease::acquire(&dir, OsStr::new("Å.txt")).unwrap();
        // Prove the two names refer to the same object on this test filesystem.
        let upper = dir.open("Å.txt").unwrap();
        let lower = dir.open("å.txt").unwrap();
        assert_eq!(
            windows_fs::identity(&upper, false).unwrap(),
            windows_fs::identity(&lower, false).unwrap()
        );
        assert!(unicode_guard.replacement_parent("å.txt").is_err());
        assert!(replace(&source, &unicode_guard, "å.txt").is_err());
        assert_eq!(
            std::fs::read(parent.join("Å.txt")).unwrap(),
            b"unicode guard"
        );
        drop((upper, lower, unicode_guard));
        replace(&source, &lease, "file.txt").unwrap();
        let written = dir.open("file.txt").unwrap();
        assert_eq!(windows_fs::identity(&written, false).unwrap(), identity);
        assert_eq!(std::fs::read(parent.join("file.txt")).unwrap(), b"after");
        assert_eq!(std::fs::read(parent.join("guard")).unwrap(), b"guard");
        assert!(!temp.path().join("staged").exists());
    }
}
