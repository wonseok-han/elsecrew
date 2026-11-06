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
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
    fn GetSecurityDescriptorOwner(
        descriptor: *mut c_void,
        owner: *mut *mut c_void,
        defaulted: *mut i32,
    ) -> i32;
    fn GetSecurityDescriptorGroup(
        descriptor: *mut c_void,
        group: *mut *mut c_void,
        defaulted: *mut i32,
    ) -> i32;
    fn GetSecurityDescriptorDacl(
        descriptor: *mut c_void,
        present: *mut i32,
        dacl: *mut *mut c_void,
        defaulted: *mut i32,
    ) -> i32;
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
        Self::read_handle(file, false)
    }
    fn read_handle<T: AsRawHandle>(file: &T, directory: bool) -> Result<Self, ProjectFailure> {
        windows_fs::identity(file, directory)?;
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
    fn from_text(text: &[u16]) -> Result<Self, ProjectFailure> {
        if text.is_empty() || text.len() >= 131_072 || text.contains(&0) {
            return Err(ProjectFailure::recovery(
                "invalid archived Windows access descriptor",
            ));
        }
        // Own the terminator and keep the bounded input alive during conversion.
        let mut input = text.to_vec();
        input.push(0);
        let (mut raw, mut size) = (ptr::null_mut(), 0u32);
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                input.as_ptr(),
                1,
                &mut raw,
                &mut size,
            )
        };
        let allocation = LocalMemory(raw);
        if ok == 0 || allocation.0.is_null() || size == 0 || size > 1024 * 1024 {
            return Err(ProjectFailure::recovery(
                "could not decode archived Windows access descriptor",
            ));
        }
        let (mut owner, mut group, mut dacl) = (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        let (mut defaulted, mut present, mut control, mut revision) = (0i32, 0i32, 0u16, 0u32);
        // Conversion returned a valid self-relative descriptor. These borrowed
        // component pointers cannot escape the LocalFree owner below.
        let ok = unsafe {
            GetSecurityDescriptorOwner(allocation.0, &mut owner, &mut defaulted) != 0
                && GetSecurityDescriptorGroup(allocation.0, &mut group, &mut defaulted) != 0
                && GetSecurityDescriptorDacl(allocation.0, &mut present, &mut dacl, &mut defaulted)
                    != 0
                && GetSecurityDescriptorControl(allocation.0, &mut control, &mut revision) != 0
        };
        if !ok || owner.is_null() || group.is_null() || present == 0 || dacl.is_null() {
            return Err(ProjectFailure::recovery(
                "archived Windows access descriptor is incomplete",
            ));
        }
        let descriptor = Self {
            allocation,
            owner,
            group,
            dacl,
            protected: control & SE_DACL_PROTECTED != 0,
        };
        if descriptor.normalized()? != text {
            return Err(ProjectFailure::recovery(
                "archived Windows access descriptor is not canonical",
            ));
        }
        Ok(descriptor)
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
    let saved = SavedAccess::capture(previous)?;
    let result = saved.restore_replacement(replacement)?;
    if SavedAccess::capture(previous)? != saved {
        return Err(ProjectFailure::recovery(
            "Windows save security changed during preservation",
        ));
    }
    Ok(result)
}

/// Archive payload only, not permission to mutate a user file. Callers must bind
/// this to a verified backup and pass their newly created replacement handle.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SavedAccess {
    schema_version: u32,
    text: Vec<u16>,
}
impl SavedAccess {
    pub(super) fn capture(file: &File) -> Result<Self, ProjectFailure> {
        let text = Descriptor::read(file)?.normalized()?;
        // Require restorability before accepting an archive payload.
        Descriptor::from_text(&text)?;
        Ok(Self {
            schema_version: 1,
            text,
        })
    }
    pub(super) fn restore_replacement(&self, replacement: &File) -> Result<String, ProjectFailure> {
        if self.schema_version != 1 {
            return Err(ProjectFailure::recovery(
                "unknown Windows access archive version",
            ));
        }
        let original = Descriptor::from_text(&self.text)?;
        let identity = windows_fs::identity(replacement, false)?;
        // Reapplying an already equal inherited ACL can canonicalize it differently.
        if Descriptor::read(replacement)?.normalized()? != self.text {
            let protection = if original.protected {
                0x8000_0000
            } else {
                0x2000_0000
            };
            // All pointers remain borrowed from original through this call.
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
                return Err(failure("restore replacement", code));
            }
        }
        if Descriptor::read(replacement)?.normalized()? != self.text
            || windows_fs::identity(replacement, false)? != identity
        {
            return Err(ProjectFailure::recovery(
                "restored Windows access descriptor differs from archive",
            ));
        }
        Ok(digest(&self.text))
    }
}

pub(super) fn fingerprint(file: &File) -> Result<String, ProjectFailure> {
    Ok(digest(&Descriptor::read(file)?.normalized()?))
}
pub(super) fn directory_fingerprint(
    directory: &cap_std::fs::Dir,
) -> Result<String, ProjectFailure> {
    Ok(digest(
        &Descriptor::read_handle(directory, true)?.normalized()?,
    ))
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
    fn archived_access_roundtrips_before_restoring_a_new_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        std::fs::write(temp.path().join("previous"), b"original").unwrap();
        let previous = super::super::open_real_file_at(&dir, "previous").unwrap();
        let saved = SavedAccess::capture(&previous).unwrap();
        let encoded = serde_json::to_vec(&saved).unwrap();
        let decoded: SavedAccess = serde_json::from_slice(&encoded).unwrap();
        let replacement = dir
            .open_with("replacement", &windows_replace::source_options())
            .unwrap();
        assert_eq!(
            decoded.restore_replacement(&replacement).unwrap(),
            fingerprint(&previous).unwrap()
        );
        assert_eq!(SavedAccess::capture(&replacement).unwrap(), saved);
        assert_eq!(
            std::fs::read(temp.path().join("previous")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn malformed_archived_access_is_rejected_before_replacement_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let replacement = dir
            .open_with("replacement", &windows_replace::source_options())
            .unwrap();
        let before = fingerprint(&replacement).unwrap();
        for saved in [
            SavedAccess {
                schema_version: 2,
                text: vec![65],
            },
            SavedAccess {
                schema_version: 1,
                text: vec![0],
            },
            SavedAccess {
                schema_version: 1,
                text: Vec::new(),
            },
            SavedAccess {
                schema_version: 1,
                text: vec![65; 131_072],
            },
        ] {
            assert!(saved.restore_replacement(&replacement).is_err());
            assert_eq!(fingerprint(&replacement).unwrap(), before);
        }
        assert!(Descriptor::from_text(
            &"O:SYG:SYD:NO_ACCESS_CONTROL"
                .encode_utf16()
                .collect::<Vec<_>>()
        )
        .is_err());
    }

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
