//! Handle-bound owner/group/DACL preservation for replacement files.
//! Does not read SACL/audit policy, enable privileges, or claim complete metadata
//! preservation. Product admission still requires the Windows validation gates.
use super::{windows_fs, ProjectFailure};
use cap_std::fs::File;
use sha2::{Digest, Sha256};
use std::{ffi::c_void, os::windows::io::AsRawHandle, ptr};

const OWNER_GROUP_DACL: u32 = 7;
const SE_FILE_OBJECT: i32 = 1;
const SE_DACL_PROTECTED: u16 = 0x1000;

#[link(name = "advapi32")]
extern "system" {
    fn GetSecurityInfo(
        handle: *mut c_void,
        kind: i32,
        info: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn SetSecurityInfo(
        handle: *mut c_void,
        kind: i32,
        info: u32,
        owner: *mut c_void,
        group: *mut c_void,
        dacl: *mut c_void,
        sacl: *mut c_void,
    ) -> u32;
    fn GetSecurityDescriptorControl(
        descriptor: *mut c_void,
        control: *mut u16,
        revision: *mut u32,
    ) -> i32;
    fn ConvertSecurityDescriptorToStringSecurityDescriptorW(
        descriptor: *mut c_void,
        revision: u32,
        info: u32,
        text: *mut *mut u16,
        length: *mut u32,
    ) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

struct LocalMemory(*mut c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this allocation came from an API requiring LocalFree.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}
struct Descriptor {
    allocation: LocalMemory,
    owner: *mut c_void,
    group: *mut c_void,
    dacl: *mut c_void,
    protected: bool,
}
fn failure(operation: &str, code: u32) -> ProjectFailure {
    ProjectFailure::recovery(format!("Windows save security {operation} failed ({code})"))
}
impl Descriptor {
    fn read(file: &File) -> Result<Self, ProjectFailure> {
        windows_fs::identity(file, false)?;
        let (mut owner, mut group, mut dacl, mut allocation) = (
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        // SAFETY: live file handle and writable outputs; component pointers are
        // owned by the returned allocation and never outlive it.
        let code = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_GROUP_DACL,
                &mut owner,
                &mut group,
                &mut dacl,
                ptr::null_mut(),
                &mut allocation,
            )
        };
        let allocation = LocalMemory(allocation);
        if code != 0 {
            return Err(failure("read", code));
        }
        if allocation.0.is_null() || owner.is_null() || group.is_null() || dacl.is_null() {
            return Err(ProjectFailure::recovery(
                "Windows save security descriptor is incomplete",
            ));
        }
        let (mut control, mut revision) = (0u16, 0u32);
        // SAFETY: successful GetSecurityInfo returned a valid descriptor.
        if unsafe { GetSecurityDescriptorControl(allocation.0, &mut control, &mut revision) } == 0 {
            return Err(ProjectFailure::recovery(
                "Windows save security control is unavailable",
            ));
        }
        Ok(Self {
            allocation,
            owner,
            group,
            dacl,
            protected: control & SE_DACL_PROTECTED != 0,
        })
    }
    fn normalized(&self) -> Result<Vec<u16>, ProjectFailure> {
        let (mut text, mut length) = (ptr::null_mut(), 0u32);
        // SAFETY: descriptor remains owned; API returns its own LocalFree buffer.
        let ok = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                self.allocation.0,
                1,
                OWNER_GROUP_DACL,
                &mut text,
                &mut length,
            )
        };
        let _allocation = LocalMemory(text.cast());
        if ok == 0 || text.is_null() || length == 0 || length > 131_072 {
            return Err(ProjectFailure::recovery(
                "Windows save security representation is unavailable or oversized",
            ));
        }
        // SAFETY: successful API returned length WCHARs in the owned buffer.
        let buffer = unsafe { std::slice::from_raw_parts(text, length as usize) };
        let end = buffer.iter().position(|c| *c == 0).ok_or_else(|| {
            ProjectFailure::recovery("Windows save security text is unterminated")
        })?;
        Ok(buffer[..end].to_vec())
    }
}

/// Mutates only the newly created replacement source, never the old target.
/// Inheritance canonicalization must reproduce exactly the inspected access
/// descriptor; any unsupported owner or inheritance combination fails closed.
pub(super) fn copy_access(previous: &File, replacement: &File) -> Result<String, ProjectFailure> {
    let original = Descriptor::read(previous)?;
    let expected = original.normalized()?;
    let protection = if original.protected {
        0x8000_0000
    } else {
        0x2000_0000
    };
    // SAFETY: all component pointers belong to original, which outlives this
    // synchronous call. Only the retained replacement handle is modified.
    let code = unsafe {
        SetSecurityInfo(
            replacement.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_GROUP_DACL | protection,
            original.owner,
            original.group,
            original.dacl,
            ptr::null_mut(),
        )
    };
    if code != 0 {
        return Err(failure("copy", code));
    }
    if Descriptor::read(replacement)?.normalized()? != expected
        || Descriptor::read(previous)?.normalized()? != expected
    {
        return Err(ProjectFailure::recovery(
            "Windows save security changed during preservation",
        ));
    }
    Ok(digest(&expected))
}

pub(super) fn fingerprint(file: &File) -> Result<String, ProjectFailure> {
    Ok(digest(&Descriptor::read(file)?.normalized()?))
}
fn digest(value: &[u16]) -> String {
    let mut hash = Sha256::new();
    for unit in value {
        hash.update(unit.to_le_bytes());
    }
    format!("windows-access-v1:{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::windows_replace;
    use cap_std::{ambient_authority, fs::Dir};

    #[test]
    fn replacement_preserves_inherited_access_in_same_parent() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let old = dir
            .open_with("old", &windows_replace::source_options())
            .unwrap();
        let new = dir
            .open_with("new", &windows_replace::source_options())
            .unwrap();
        assert!(!Descriptor::read(&old).unwrap().protected);
        let before = fingerprint(&old).unwrap();
        assert_eq!(copy_access(&old, &new).unwrap(), before);
        assert_eq!(fingerprint(&old).unwrap(), before);
        assert_eq!(fingerprint(&new).unwrap(), before);
    }

    #[test]
    fn replacement_preserves_protected_access_descriptor_without_changing_old_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let mut old = dir
            .open_with("old", &windows_replace::source_options())
            .unwrap();
        std::io::Write::write_all(&mut old, b"old bytes").unwrap();
        let new = dir
            .open_with("new", &windows_replace::source_options())
            .unwrap();
        let descriptor = Descriptor::read(&old).unwrap();
        // SAFETY: test-owned file and a live descriptor's DACL. This makes the
        // fixture differ from the replacement's initially inherited security.
        assert_eq!(
            unsafe {
                SetSecurityInfo(
                    old.as_raw_handle(),
                    SE_FILE_OBJECT,
                    4 | 0x8000_0000,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    descriptor.dacl,
                    ptr::null_mut(),
                )
            },
            0
        );
        let before = fingerprint(&old).unwrap();
        assert_ne!(fingerprint(&new).unwrap(), before);
        assert_eq!(copy_access(&old, &new).unwrap(), before);
        assert_eq!(fingerprint(&old).unwrap(), before);
        assert_eq!(fingerprint(&new).unwrap(), before);
        assert_ne!(
            windows_fs::identity(&old, false).unwrap(),
            windows_fs::identity(&new, false).unwrap()
        );
        assert_eq!(
            std::fs::read(temp.path().join("old")).unwrap(),
            b"old bytes"
        );
    }
}
