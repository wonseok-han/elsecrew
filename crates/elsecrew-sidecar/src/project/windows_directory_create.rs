//! Create a new directory relative to a retained parent and keep the creating
//! handle. The caller must persist its reservation first and retain failures.
//! This primitive is not a directory application/restore transaction.
//! https://learn.microsoft.com/windows/win32/api/winternl/nf-winternl-ntcreatefile
#![allow(dead_code)]
use super::{
    windows_directory_lease::DirectoryLease, windows_durability, windows_fs, ProjectFailure,
};
use cap_std::fs::{Dir, File};
use std::{
    ffi::c_void,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle},
};

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: *mut c_void,
    name: *mut UnicodeString,
    attributes: u32,
    security: *mut c_void,
    quality: *mut c_void,
}
#[repr(C)]
#[derive(Default)]
struct IoStatusBlock {
    status: usize,
    information: usize,
}
const _: () = assert!(size_of::<UnicodeString>() == 2 * size_of::<usize>());
const _: () = assert!(size_of::<ObjectAttributes>() == 6 * size_of::<usize>());
const _: () = assert!(size_of::<IoStatusBlock>() == 2 * size_of::<usize>());

#[link(name = "ntdll")]
extern "system" {
    fn NtCreateFile(
        handle: *mut *mut c_void,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatusBlock,
        allocation: *const i64,
        file_attributes: u32,
        sharing: u32,
        disposition: u32,
        options: u32,
        ea: *const c_void,
        ea_length: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

/// FILE_CREATE never opens or replaces an existing name. The returned handle
/// owns the newly created object, avoiding a create-by-path/reopen ownership gap.
/// Parent namespace durability still needs the enclosing transaction contract.
pub(super) fn create_new(lease: &DirectoryLease, name: &str) -> Result<File, ProjectFailure> {
    let parent = lease.replacement_parent(name)?;
    windows_durability::require_ntfs(parent)?;
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    if wide.is_empty() || wide.len() > 255 || wide.contains(&0) {
        return Err(ProjectFailure::manifest("invalid new directory leaf"));
    }
    let mut unicode = UnicodeString {
        length: (wide.len() * 2) as u16,
        maximum: (wide.len() * 2) as u16,
        buffer: wide.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut unicode,
        attributes: 0x40, // OBJ_CASE_INSENSITIVE: do not introduce case aliases.
        security: std::ptr::null_mut(),
        quality: std::ptr::null_mut(),
    };
    let mut status = IoStatusBlock::default();
    let mut raw = std::ptr::null_mut();
    lease.validate()?;
    // SAFETY: parent handle and all ABI buffers remain live for this synchronous
    // FILE_CREATE call. The single normal component cannot traverse the parent.
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            0xc011_0000,
            &mut attributes,
            &mut status,
            std::ptr::null(),
            0x10,
            3,
            2,
            0x0020_0023, // DIRECTORY_FILE | WRITE_THROUGH | SYNCHRONOUS_IO_NONALERT | OPEN_REPARSE_POINT
            std::ptr::null(),
            0,
        )
    };
    if result < 0 {
        return Err(ProjectFailure::recovery(format!(
            "could not create reserved directory: {}",
            std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(result) } as i32)
        )));
    }
    if raw.is_null() || raw == (-1isize) as *mut c_void {
        return Err(ProjectFailure::ambiguous(
            "directory creation returned no usable handle",
        ));
    }
    // SAFETY: successful synchronous create transfers exactly one live handle.
    let file = File::from_std(unsafe { std::fs::File::from_raw_handle(raw) });
    if status.information != 2 {
        // FILE_CREATED
        return Err(ProjectFailure::ambiguous(
            "directory creation did not report a new object",
        ));
    }
    windows_fs::identity(&file, true)?;
    lease.validate()?;
    Ok(file)
}

/// Read-only, no-follow rebind of a single directory name under a held parent.
/// Share-delete permits observation beside our existing DELETE-capable handle;
/// this handle grants no rename/delete authority and does not replace its pin.
pub(super) fn open_existing(parent: &Dir, name: &str) -> Result<Dir, ProjectFailure> {
    let file = open_object(parent, name)?;
    windows_fs::identity(&file, true)?;
    Ok(Dir::from_std_file(file.into_std()))
}

/// Rebind a regular file or directory beside a retained rename handle. The
/// caller must compare the full receipt and keep its mutation/guard pins alive.
pub(super) fn open_object(parent: &Dir, name: &str) -> Result<File, ProjectFailure> {
    if name.is_empty() || name.contains(['/', '\\']) {
        return Err(ProjectFailure::manifest(
            "invalid directory observation leaf",
        ));
    }
    super::protocol_path::from_native(&format!("C:/{name}"), true)
        .map_err(ProjectFailure::manifest)?;
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    if wide.len() > 255 || wide.contains(&0) {
        return Err(ProjectFailure::manifest(
            "invalid directory observation leaf",
        ));
    }
    let mut unicode = UnicodeString {
        length: (wide.len() * 2) as u16,
        maximum: (wide.len() * 2) as u16,
        buffer: wide.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut unicode,
        attributes: 0x40,
        security: std::ptr::null_mut(),
        quality: std::ptr::null_mut(),
    };
    let mut status = IoStatusBlock::default();
    let mut raw = std::ptr::null_mut();
    // SAFETY: synchronous FILE_OPEN of one validated component, relative to
    // the retained parent. All buffers remain live; no create/write rights.
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            0x0012_0081, // SYNCHRONIZE | READ_CONTROL | READ_ATTRIBUTES | LIST_DIRECTORY
            &mut attributes,
            &mut status,
            std::ptr::null(),
            0,
            7,           // SHARE_READ | SHARE_WRITE | SHARE_DELETE
            1,           // FILE_OPEN: never create or overwrite.
            0x0020_0020, // SYNCHRONOUS_NONALERT | OPEN_REPARSE_POINT; either object kind
            std::ptr::null(),
            0,
        )
    };
    if result < 0 {
        return Err(ProjectFailure::recovery(format!(
            "could not observe retained directory: {}",
            std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(result) } as i32)
        )));
    }
    if raw.is_null() || raw == (-1isize) as *mut c_void {
        return Err(ProjectFailure::recovery(
            "directory observation returned no handle",
        ));
    }
    // SAFETY: successful synchronous open transfers one owned handle.
    let file = File::from_std(unsafe { std::fs::File::from_raw_handle(raw) });
    if status.information != 1 {
        return Err(ProjectFailure::recovery(
            "directory observation was not FILE_OPENED",
        ));
    }
    let directory = file
        .metadata()
        .map_err(|error| {
            ProjectFailure::recovery(format!("could not inspect rebound object: {error}"))
        })?
        .is_dir();
    windows_fs::identity(&file, directory)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::{ambient_authority, fs::Dir};
    use std::ffi::OsStr;
    #[test]
    fn read_only_rebind_coexists_with_retained_file_rename_handle() {
        use std::io::{Read, Write};
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let mut source = dir
            .open_with("staged", &super::super::windows_replace::source_options())
            .unwrap();
        source.write_all(b"preserved").unwrap();
        source.sync_all().unwrap();
        let identity = windows_fs::identity(&source, false).unwrap();
        let mut rebound = open_object(&dir, "staged").unwrap();
        assert_eq!(identity, windows_fs::identity(&rebound, false).unwrap());
        let mut bytes = Vec::new();
        rebound.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"preserved");
        assert!(rebound.write_all(b"denied").is_err());
        assert!(std::fs::rename(temp.path().join("staged"), temp.path().join("foreign")).is_err());
        assert!(open_existing(&dir, "staged").is_err());
        assert_eq!(
            std::fs::read(temp.path().join("staged")).unwrap(),
            b"preserved"
        );
    }

    /// Exercise the complete rename/rebind sequence without releasing either
    /// mutation handle. This is filesystem coverage, not journal/Git admission.
    #[test]
    fn retained_file_rebind_survives_apply_and_reverse_restore_moves() {
        use super::super::windows_replace::{move_new, source_options};
        use std::io::{Read, Write};
        for (has_before, has_after) in [(true, true), (true, false), (false, true)] {
            let temp = tempfile::tempdir().unwrap();
            let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
            std::fs::write(temp.path().join("guard"), b"guard").unwrap();
            let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
            let create = |name: &str, bytes: &[u8]| {
                let mut file = dir.open_with(name, &source_options()).unwrap();
                file.write_all(bytes).unwrap();
                file.sync_all().unwrap();
                file
            };
            let before = has_before.then(|| create("visible", b"before"));
            let after = has_after.then(|| create("staged", b"after"));
            let observe = |name: &str, held: &File, expected: &[u8]| {
                let mut read = open_object(&dir, name).unwrap();
                assert_eq!(
                    windows_fs::identity(&read, false).unwrap(),
                    windows_fs::identity(held, false).unwrap()
                );
                let mut bytes = Vec::new();
                read.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, expected);
                assert!(read.write_all(b"forbidden").is_err());
                read
            };
            // Keep readers alive across moves too: observation must not become
            // an accidental delete-sharing fence on our own retained source.
            let original_read = before
                .as_ref()
                .map(|file| observe("visible", file, b"before"));
            if let Some(file) = &before {
                move_new(file, &lease, "retained").unwrap();
                observe("retained", file, b"before");
            }
            let installed_read = after.as_ref().map(|file| {
                move_new(file, &lease, "visible").unwrap();
                observe("visible", file, b"after")
            });
            if let Some(file) = &after {
                move_new(file, &lease, "detached").unwrap();
                observe("detached", file, b"after");
            }
            if let Some(file) = &before {
                move_new(file, &lease, "visible").unwrap();
                observe("visible", file, b"before");
            } else {
                assert!(!temp.path().join("visible").exists());
            }
            assert!(!temp.path().join("retained").exists());
            assert!(!temp.path().join("staged").exists());
            assert_eq!(std::fs::read(temp.path().join("guard")).unwrap(), b"guard");
            drop((original_read, installed_read));
        }
    }

    #[test]
    fn retained_directory_create_and_move_never_replace_an_existing_leaf() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("guard"), b"guard").unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        for invalid in ["guard", "../escape", "nested/child", "stream:ads"] {
            assert!(create_new(&lease, invalid).is_err());
        }
        let staged = create_new(&lease, "staged").unwrap();
        let identity = windows_fs::identity(&staged, true).unwrap();
        let observed = open_existing(&dir, "staged").unwrap();
        assert_eq!(identity, windows_fs::identity(&observed, true).unwrap());
        for invalid in ["../escape", "nested/child", "stream:ads", ".", ".."] {
            assert!(open_existing(&dir, invalid).is_err());
        }
        assert!(open_existing(&dir, "missing").is_err());
        assert!(!temp.path().join("missing").exists());
        assert!(open_existing(&dir, "guard").is_err());
        drop(observed);
        assert!(create_new(&lease, "staged").is_err());
        std::fs::create_dir(temp.path().join("occupied")).unwrap();
        std::fs::write(temp.path().join("occupied/foreign"), b"keep").unwrap();
        assert!(
            super::super::windows_replace::move_new_directory(&staged, &lease, "occupied").is_err()
        );
        super::super::windows_replace::move_new_directory(&staged, &lease, "installed").unwrap();
        assert_eq!(identity, windows_fs::identity(&staged, true).unwrap());
        assert!(!temp.path().join("staged").exists());
        assert!(temp.path().join("installed").is_dir());
        assert_eq!(
            std::fs::read(temp.path().join("occupied/foreign")).unwrap(),
            b"keep"
        );
    }
}
