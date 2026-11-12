//! Basic metadata on the newly created staged file only. Never edits the source.
//! https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_basic_info
use super::*;
use std::{ffi::c_void, os::windows::io::AsRawHandle};

// READONLY, HIDDEN, SYSTEM, ARCHIVE, NORMAL, TEMPORARY, NOT_CONTENT_INDEXED.
// Compression, encryption, reparse/cloud/offline and stream preservation need
// their own contracts; do not silently erase those attributes on replacement.
const BASIC_ATTRIBUTES: u32 = 0x0000_21a7;
#[repr(C)]
#[derive(Default)]
struct BasicInfo {
    created: i64,
    accessed: i64,
    written: i64,
    changed: i64,
    attributes: u32,
}
#[link(name = "kernel32")]
extern "system" {
    fn SetFileInformationByHandle(
        handle: *mut c_void,
        class: i32,
        info: *const c_void,
        bytes: u32,
    ) -> i32;
}
fn desired(before: &Value) -> Result<BasicInfo, ProjectFailure> {
    let created = before["observation"]["created"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(invalid)?;
    let attributes = before["observation"]["attributes"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(invalid)?;
    if attributes == 0
        || attributes & !BASIC_ATTRIBUTES != 0
        || (attributes & 0x80 != 0 && attributes != 0x80)
    {
        return Err(invalid());
    }
    Ok(BasicInfo {
        created,
        attributes,
        ..Default::default()
    })
}
pub(in crate::project) fn supported(before: &Value) -> bool {
    before.is_null() || desired(before).is_ok()
}
pub(super) fn matches(before: &Value, staged: &Value) -> bool {
    before.is_null()
        || (supported(before)
            && before["observation"]["created"] == staged["observation"]["created"]
            && before["observation"]["attributes"] == staged["observation"]["attributes"])
}
/// Called only with the exclusive-created staged handle, after content flush.
/// Preserve creation time and basic flags. New content keeps its new write time;
/// access/change times are not transplanted from the old content.
pub(super) fn preserve(before: &Value, staged: &File) -> Result<(), ProjectFailure> {
    let info = desired(before)?;
    let initial = file_observation(staged)?;
    if json!(initial)["identity"] == before["observation"]["identity"] {
        return Err(invalid());
    }
    // SAFETY: live writable staged handle; ABI-compatible FILE_BASIC_INFO.
    if unsafe {
        SetFileInformationByHandle(
            staged.as_raw_handle(),
            0,
            (&info as *const BasicInfo).cast(),
            std::mem::size_of::<BasicInfo>() as u32,
        )
    } == 0
    {
        return Err(invalid());
    }
    staged.sync_all().map_err(|_| invalid())?;
    let final_observation = file_observation(staged)?;
    if initial.identity_token() != final_observation.identity_token()
        || initial.length != final_observation.length
        || !matches(before, &json!({"observation":final_observation}))
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_metadata_rejects_special_attributes_and_control_times() {
        let mut before = json!({"observation":{"created":12345,"attributes":0x22}});
        assert!(supported(&before));
        for attributes in [0, 0x400, 0x800, 0x4000, 0x10, 0x82, 0x1_0000_0020u64] {
            before["observation"]["attributes"] = json!(attributes);
            assert!(!supported(&before));
        }
        before["observation"]["attributes"] = json!(0x20);
        for created in [0, -1, -2] {
            before["observation"]["created"] = json!(created);
            assert!(!supported(&before));
        }
    }

    #[test]
    fn staged_basic_metadata_preserves_source_and_binds_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        let mut source = dir
            .open_with("source", &windows_replace::source_options())
            .unwrap();
        source.write_all(b"original").unwrap();
        source.sync_all().unwrap();
        let before = entry(&mut source, &|| Ok(())).unwrap();
        let mut staged = dir
            .open_with("staged", &windows_replace::source_options())
            .unwrap();
        staged.write_all(b"edited").unwrap();
        staged.sync_all().unwrap();
        preserve(&before, &staged).unwrap();
        assert!(matches(&before, &entry(&mut staged, &|| Ok(())).unwrap()));
        assert_eq!(before, entry(&mut source, &|| Ok(())).unwrap());
        assert!(preserve(&before, &source).is_err());
        let mut changed = entry(&mut staged, &|| Ok(())).unwrap();
        changed["observation"]["created"] = json!(1);
        assert!(!matches(&before, &changed));
    }
}
