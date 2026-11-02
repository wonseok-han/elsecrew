//! Windows handle checks. These helpers do not enable the Windows product gate.
//! File identity uses the complete FILE_ID_INFO, including ReFS's 128-bit ID.
//! https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info

#[cfg(windows)]
pub(super) const OPEN_REPARSE_POINT: u32 = 0x0020_0000;

fn local_volume_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(r"\\?\Volume{") else {
        return false;
    };
    let Some((id, _)) = rest.split_once("}\\") else {
        return false;
    };
    id.len() == 36 && uuid::Uuid::parse_str(id).is_ok()
}

#[cfg(windows)]
mod native {
    use super::local_volume_path;
    use crate::project::{open_real_os_child_dir, protocol_path, ProjectFailure};
    use cap_std::{ambient_authority, fs::Dir};
    use std::{
        ffi::c_void,
        os::windows::io::AsRawHandle,
        path::{Component, Path},
    };

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Identity {
        volume: u64,
        file: [u8; 16],
    }

    #[repr(C)]
    #[derive(Default)]
    struct AttributeTag {
        attributes: u32,
        tag: u32,
    }

    const _: () = assert!(std::mem::size_of::<Identity>() == 24);
    const _: () = assert!(std::mem::size_of::<AttributeTag>() == 8);

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandleEx(
            handle: *mut c_void,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
        fn GetFinalPathNameByHandleW(
            handle: *mut c_void,
            buffer: *mut u16,
            size: u32,
            flags: u32,
        ) -> u32;
    }

    fn failure(operation: &str) -> ProjectFailure {
        ProjectFailure::recovery(format!("{operation}: {}", std::io::Error::last_os_error()))
    }

    pub(crate) fn identity<T: AsRawHandle>(
        held: &T,
        directory: bool,
    ) -> Result<Identity, ProjectFailure> {
        let mut tag = AttributeTag::default();
        // SAFETY: held owns a live handle throughout this synchronous call;
        // the output buffer has the documented FILE_ATTRIBUTE_TAG_INFO layout.
        let result = unsafe {
            GetFileInformationByHandleEx(
                held.as_raw_handle(),
                9,
                (&mut tag as *mut AttributeTag).cast(),
                std::mem::size_of::<AttributeTag>() as u32,
            )
        };
        if result == 0 {
            return Err(failure("could not inspect Windows file attributes"));
        }
        const REPARSE_POINT: u32 = 0x400;
        const DIRECTORY: u32 = 0x10;
        if tag.attributes & REPARSE_POINT != 0 || (tag.attributes & DIRECTORY != 0) != directory {
            return Err(ProjectFailure::symlink(
                "Windows handle is a reparse point or wrong object kind",
            ));
        }
        let mut id = Identity::default();
        // SAFETY: the output has FILE_ID_INFO's full 64 + 128-bit layout.
        let result = unsafe {
            GetFileInformationByHandleEx(
                held.as_raw_handle(),
                18,
                (&mut id as *mut Identity).cast(),
                std::mem::size_of::<Identity>() as u32,
            )
        };
        if result == 0 {
            return Err(failure("could not inspect full Windows file identity"));
        }
        if id.file == [0; 16] {
            return Err(ProjectFailure::manifest(
                "Windows filesystem did not supply a usable file identity",
            ));
        }
        Ok(id)
    }

    pub(crate) fn require_local_volume(held: &Dir) -> Result<(), ProjectFailure> {
        // A GUID volume path is obtained from the held handle, not from a drive
        // letter lookup which could be remapped between checks. UNC is excluded.
        let mut buffer = vec![0u16; 32_768];
        // SAFETY: the borrowed handle and allocated UTF-16 output stay live.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                held.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                1,
            )
        };
        if length == 0 {
            return Err(failure("could not identify held local Windows volume"));
        }
        if length as usize >= buffer.len() {
            return Err(ProjectFailure::manifest(
                "Windows volume path exceeds supported bounds",
            ));
        }
        let path = String::from_utf16(&buffer[..length as usize])
            .map_err(|_| ProjectFailure::manifest("Windows volume path is not valid Unicode"))?;
        if !local_volume_path(&path) {
            return Err(ProjectFailure::symlink(
                "source capability requires a local GUID volume",
            ));
        }
        Ok(())
    }

    pub(crate) fn open_directory(path: &Path) -> Result<Dir, ProjectFailure> {
        let text = path
            .to_str()
            .ok_or_else(|| ProjectFailure::manifest("Windows path is not valid Unicode"))?;
        let normalized =
            protocol_path::from_native(text, true).map_err(ProjectFailure::manifest)?;
        let mut current = Dir::open_ambient_dir(&normalized[..3], ambient_authority())
            .map_err(|_| ProjectFailure::recovery("could not open Windows drive root"))?;
        identity(&current, true)?;
        require_local_volume(&current)?;
        for component in Path::new(&normalized).components() {
            match component {
                Component::Prefix(_) | Component::RootDir => {}
                Component::Normal(name) => {
                    current = open_real_os_child_dir(&current, name)?;
                }
                _ => {
                    return Err(ProjectFailure::symlink(
                        "Windows source path contains a non-normal component",
                    ))
                }
            }
        }
        identity(&current, true)?;
        Ok(current)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn held_directory_identity_survives_clone_and_distinguishes_siblings() {
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir(temp.path().join("one")).unwrap();
            std::fs::create_dir(temp.path().join("two")).unwrap();
            let one = open_directory(&temp.path().join("one").canonicalize().unwrap()).unwrap();
            let two = open_directory(&temp.path().join("two").canonicalize().unwrap()).unwrap();
            assert_eq!(
                identity(&one, true).unwrap(),
                identity(&one.try_clone().unwrap(), true).unwrap()
            );
            assert_ne!(identity(&one, true).unwrap(), identity(&two, true).unwrap());
        }
        fn make_junction(link: &Path, target: &Path) {
            use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt};
            #[link(name = "kernel32")]
            extern "system" {
                fn DeviceIoControl(
                    handle: *mut c_void,
                    code: u32,
                    input: *const c_void,
                    input_size: u32,
                    output: *mut c_void,
                    output_size: u32,
                    returned: *mut u32,
                    overlapped: *mut c_void,
                ) -> i32;
            }
            std::fs::create_dir(link).unwrap();
            let target = target.canonicalize().unwrap();
            let target = target.to_str().unwrap().strip_prefix(r"\\?\").unwrap();
            let substitute: Vec<u16> = std::ffi::OsStr::new(&format!(r"\??\{target}"))
                .encode_wide()
                .collect();
            let bytes = u16::try_from(substitute.len() * 2).unwrap();
            let mut data = Vec::new();
            data.extend_from_slice(&0xa0000003u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
            data.extend_from_slice(&(8u16 + bytes + 4).to_le_bytes());
            for value in [0u16, 0, bytes, bytes + 2, 0] {
                data.extend_from_slice(&value.to_le_bytes());
            }
            for value in substitute.into_iter().chain([0, 0]) {
                data.extend_from_slice(&value.to_le_bytes());
            }
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(0x02000000 | super::super::OPEN_REPARSE_POINT)
                .open(link)
                .unwrap();
            let mut returned = 0;
            // SAFETY: synchronous device control borrows live, bounded buffers;
            // the input is a mount-point REPARSE_DATA_BUFFER for this temp dir.
            let ok = unsafe {
                DeviceIoControl(
                    file.as_raw_handle(),
                    0x000900a4,
                    data.as_ptr().cast(),
                    data.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            };
            assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
        }

        #[test]
        fn junction_component_and_reparse_file_are_rejected() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            std::fs::create_dir(root.join("target")).unwrap();
            std::fs::create_dir(root.join("target/child")).unwrap();
            std::fs::write(root.join("target/file"), b"untouched").unwrap();
            make_junction(&root.join("junction"), &root.join("target"));
            assert!(open_directory(&root.join("junction/child")).is_err());
            std::os::windows::fs::symlink_file(root.join("target/file"), root.join("file-link"))
                .unwrap();
            let held = open_directory(&root).unwrap();
            assert!(crate::project::open_real_file_at(&held, "file-link").is_err());
            assert_eq!(
                std::fs::read(root.join("target/file")).unwrap(),
                b"untouched"
            );
        }

        #[test]
        fn directory_symlink_cannot_redirect_held_traversal() {
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir(temp.path().join("target")).unwrap();
            std::os::windows::fs::symlink_dir(temp.path().join("target"), temp.path().join("link"))
                .unwrap();
            assert!(open_directory(&temp.path().canonicalize().unwrap().join("link")).is_err());
        }
    }
}

#[cfg(windows)]
pub(super) use native::{identity, open_directory, require_local_volume};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn volume_authority_requires_a_complete_guid_path() {
        assert!(local_volume_path(
            r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\folder"
        ));
        for value in [
            r"C:\folder",
            r"\\server\share",
            r"\\?\UNC\server\share",
            r"\\?\Volume{bad}\",
            r"\\?\Volume{12345678-1234-1234-1234-123456789abc}",
        ] {
            assert!(!local_volume_path(value));
        }
    }
}
