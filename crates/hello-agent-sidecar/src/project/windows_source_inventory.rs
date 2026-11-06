//! Read-only Windows source observations for application candidates. This is not
//! a clean-index baseline, write grant, or atomic snapshot. Reparse objects and
//! multiply linked files are rejected; alternate-stream preservation still needs
//! the application writer contract before product admission can be enabled.
use super::{open_real_child_dir, open_real_file_at, windows_fs, windows_security, ProjectFailure};
use cap_std::fs::Dir;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ffi::c_void, io::Read, os::windows::io::AsRawHandle, path::Path};

pub(super) const MAX_ENTRIES: usize = 100_000;
const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub(super) const MAX_PATH_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 128;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub(super) struct Inventory {
    root: Observation,
    pub(super) entries: BTreeMap<String, Entry>,
}
impl Inventory {
    pub(super) fn root_identity(&self) -> windows_fs::Identity {
        self.root.identity
    }
    pub(super) fn write_receipt<W: std::io::Write>(
        &self,
        output: &mut W,
        check: &dyn Fn() -> Result<(), ProjectFailure>,
    ) -> Result<(), ProjectFailure> {
        output.write_all(b"{\"root\":").map_err(|_| failure())?;
        serde_json::to_writer(&mut *output, &self.root).map_err(|_| failure())?;
        output.write_all(b",\"entries\":[").map_err(|_| failure())?;
        for (i, entry) in self.entries.iter().enumerate() {
            check()?;
            if i != 0 {
                output.write_all(b",").map_err(|_| failure())?;
            }
            serde_json::to_writer(&mut *output, &entry).map_err(|_| failure())?;
        }
        output.write_all(b"]}").map_err(|_| failure())
    }
    pub(super) fn regular_content(&self, path: &str) -> Option<(u64, [u8; 32])> {
        let entry = self.entries.get(path)?;
        (entry.observation.attributes & 0x10 == 0)
            .then_some((entry.observation.length, entry.digest))
    }
}
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub(super) struct Entry {
    observation: Observation,
    digest: [u8; 32],
}
impl Entry {
    pub(super) fn observation(&self) -> &Observation {
        &self.observation
    }
    pub(super) fn access_fingerprint(&self) -> &str {
        &self.observation.access
    }
    pub(super) fn kind(&self) -> u8 {
        if self.observation.attributes & 0x10 != 0 {
            2
        } else {
            1
        }
    }
    pub(super) fn content(&self) -> (u64, [u8; 32]) {
        (self.observation.length, self.digest)
    }
}
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    identity: windows_fs::Identity,
    created: i64,
    written: i64,
    changed: i64,
    attributes: u32,
    pub(super) length: u64,
    links: u32,
    access: String,
}
impl Observation {
    pub(super) fn identity_token(&self) -> String {
        self.identity.recovery_token()
    }
}
#[repr(C)]
#[derive(Default)]
struct BasicInfo {
    created: i64,
    accessed: i64,
    written: i64,
    changed: i64,
    attributes: u32,
}
#[repr(C)]
#[derive(Default)]
struct StandardInfo {
    allocated: i64,
    end: i64,
    links: u32,
    deleting: u8,
    directory: u8,
}
const _: () = assert!(std::mem::size_of::<BasicInfo>() == 40);
const _: () = assert!(std::mem::size_of::<StandardInfo>() == 24);
#[link(name = "kernel32")]
extern "system" {
    fn GetFileInformationByHandleEx(
        handle: *mut c_void,
        class: i32,
        info: *mut c_void,
        size: u32,
    ) -> i32;
}
pub(super) fn failure() -> ProjectFailure {
    ProjectFailure::dirty("Windows source inventory changed, is unsupported, or exceeds its budget")
}
fn observe<T: AsRawHandle>(
    held: &T,
    directory: bool,
    access: String,
) -> Result<Observation, ProjectFailure> {
    let identity = windows_fs::identity(held, directory)?;
    let mut basic = BasicInfo::default();
    let mut standard = StandardInfo::default();
    // FILE_BASIC_INFO / FILE_STANDARD_INFO, queried from the retained handle.
    // LastAccessTime is intentionally excluded: observing content can update it.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            held.as_raw_handle(),
            0,
            (&mut basic as *mut BasicInfo).cast(),
            std::mem::size_of::<BasicInfo>() as u32,
        ) != 0
            && GetFileInformationByHandleEx(
                held.as_raw_handle(),
                1,
                (&mut standard as *mut StandardInfo).cast(),
                std::mem::size_of::<StandardInfo>() as u32,
            ) != 0
    };
    if !ok
        || standard.end < 0
        || standard.deleting != 0
        || (standard.directory != 0) != directory
        || (!directory && standard.links != 1)
        || basic.attributes & 0x400 != 0
        || windows_fs::identity(held, directory)? != identity
    {
        return Err(failure());
    }
    Ok(Observation {
        identity,
        created: basic.created,
        written: basic.written,
        changed: basic.changed,
        attributes: basic.attributes,
        length: standard.end as u64,
        links: standard.links,
        access,
    })
}
fn directory_observation(dir: &Dir) -> Result<Observation, ProjectFailure> {
    observe(dir, true, windows_security::directory_fingerprint(dir)?)
}
pub(super) fn file_observation(file: &cap_std::fs::File) -> Result<Observation, ProjectFailure> {
    observe(file, false, windows_security::fingerprint(file)?)
}

pub(super) fn capture(
    root: &Dir,
    native_path: &Path,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Inventory, ProjectFailure> {
    let first = capture_once(root, check)?;
    let second = capture_once(root, check)?;
    let rebound = windows_fs::open_directory(native_path)?;
    if first != second || directory_observation(&rebound)? != first.root {
        return Err(failure());
    }
    check()?;
    Ok(first)
}
fn capture_once(
    root: &Dir,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Inventory, ProjectFailure> {
    struct Frame {
        root: Dir,
        path: String,
        depth: usize,
        before: Observation,
        items: cap_std::fs::ReadDir,
        parent_name: Option<String>,
    }
    fn frame(
        root: Dir,
        path: String,
        depth: usize,
        parent_name: Option<String>,
    ) -> Result<Frame, ProjectFailure> {
        let before = directory_observation(&root)?;
        let items = root.entries().map_err(|_| failure())?;
        Ok(Frame {
            root,
            path,
            depth,
            before,
            items,
            parent_name,
        })
    }
    check()?;
    windows_fs::require_local_volume(root)?;
    let mut inventory = Inventory {
        root: directory_observation(root)?,
        entries: BTreeMap::new(),
    };
    let mut stack = vec![frame(
        root.try_clone().map_err(|_| failure())?,
        String::new(),
        0,
        None,
    )?];
    let (mut entries_left, mut bytes_left, mut paths_left) =
        (MAX_ENTRIES, MAX_BYTES, MAX_PATH_BYTES);
    let mut buffer = vec![0u8; 64 * 1024];
    while let Some(current) = stack.last_mut() {
        check()?;
        let Some(item) = current.items.next() else {
            let finished = stack.pop().ok_or_else(failure)?;
            if directory_observation(&finished.root)? != finished.before {
                return Err(failure());
            }
            if let Some(name) = finished.parent_name {
                let parent = stack.last().ok_or_else(failure)?;
                if directory_observation(&open_real_child_dir(&parent.root, &name)?)?
                    != finished.before
                {
                    return Err(failure());
                }
            }
            continue;
        };
        let name = item
            .map_err(|_| failure())?
            .file_name()
            .into_string()
            .map_err(|_| failure())?;
        // .git is compared by the separate Git baseline, not treated as user data.
        if current.path.is_empty() && name.eq_ignore_ascii_case(".git") {
            continue;
        }
        super::validate_relative_path(&name, false)?;
        if name.contains(['/', '\\']) {
            return Err(failure());
        }
        // Reject Win32 aliases (device names, alternate stream syntax, trailing dot/space).
        super::protocol_path::from_native(&format!("C:/{name}"), true)
            .map_err(ProjectFailure::manifest)?;
        entries_left = entries_left.checked_sub(1).ok_or_else(failure)?;
        let path = format!("{}{name}", current.path);
        if path.len() > 4096 {
            return Err(failure());
        }
        paths_left = paths_left.checked_sub(path.len()).ok_or_else(failure)?;
        let metadata = current
            .root
            .symlink_metadata(&name)
            .map_err(|_| failure())?;
        if metadata.is_dir() {
            if current.depth >= MAX_DEPTH {
                return Err(failure());
            }
            let child = open_real_child_dir(&current.root, &name)?;
            let observation = directory_observation(&child)?;
            let next = frame(child, format!("{path}/"), current.depth + 1, Some(name))?;
            if observation != next.before {
                return Err(failure());
            }
            if inventory
                .entries
                .insert(
                    path.clone(),
                    Entry {
                        observation,
                        digest: [0; 32],
                    },
                )
                .is_some()
            {
                return Err(failure());
            }
            stack.push(next);
        } else {
            let mut file = open_real_file_at(&current.root, &name)?;
            let before = file_observation(&file)?;
            if before.length > bytes_left {
                return Err(failure());
            }
            let mut hash = Sha256::new();
            let mut size = 0u64;
            loop {
                check()?;
                let count = file.read(&mut buffer).map_err(|_| failure())?;
                check()?;
                if count == 0 {
                    break;
                }
                bytes_left = bytes_left.checked_sub(count as u64).ok_or_else(failure)?;
                size = size.checked_add(count as u64).ok_or_else(failure)?;
                hash.update(&buffer[..count]);
            }
            if size != before.length
                || file_observation(&file)? != before
                || file_observation(&open_real_file_at(&current.root, &name)?)? != before
            {
                return Err(failure());
            }
            if inventory
                .entries
                .insert(
                    path,
                    Entry {
                        observation: before,
                        digest: hash.finalize().into(),
                    },
                )
                .is_some()
            {
                return Err(failure());
            }
        }
    }
    if directory_observation(root)? != inventory.root {
        return Err(failure());
    }
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_inventory_detects_content_changes_and_excludes_git_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap();
        std::fs::create_dir(path.join(".git")).unwrap();
        std::fs::create_dir(path.join("nested")).unwrap();
        std::fs::write(path.join("nested/file.txt"), b"before").unwrap();
        let root = windows_fs::open_directory(&path).unwrap();
        let first = capture(&root, &path, &|| Ok(())).unwrap();
        assert_eq!(first.entries.len(), 2);
        std::fs::write(path.join(".git/config"), b"metadata only").unwrap();
        assert_eq!(first, capture(&root, &path, &|| Ok(())).unwrap());
        std::fs::write(path.join("nested/file.txt"), b"after!").unwrap();
        assert_ne!(first, capture(&root, &path, &|| Ok(())).unwrap());
    }
    #[test]
    fn native_inventory_rejects_hardlinks_and_an_unrelated_root_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap();
        std::fs::create_dir(path.join("other")).unwrap();
        std::fs::write(path.join("file.txt"), b"data").unwrap();
        let root = windows_fs::open_directory(&path).unwrap();
        assert!(capture(&root, &path.join("other"), &|| Ok(())).is_err());
        std::fs::hard_link(path.join("file.txt"), path.join("alias.txt")).unwrap();
        assert!(capture(&root, &path, &|| Ok(())).is_err());
    }
    #[test]
    fn native_inventory_cancellation_leaves_source_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap();
        let bytes = vec![7; 256 * 1024];
        std::fs::write(path.join("file.txt"), &bytes).unwrap();
        let root = windows_fs::open_directory(&path).unwrap();
        let calls = std::cell::Cell::new(0usize);
        let check = || {
            calls.set(calls.get() + 1);
            if calls.get() >= 5 {
                Err(ProjectFailure::recovery("test canceled"))
            } else {
                Ok(())
            }
        };
        assert!(capture(&root, &path, &check).is_err());
        assert_eq!(std::fs::read(path.join("file.txt")).unwrap(), bytes);
    }
}
