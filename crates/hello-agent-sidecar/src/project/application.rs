//! Read-only raw source inventory. Not an apply authorization or atomic snapshot.
use super::ProjectFailure;
use cap_std::fs::{Dir, File, Metadata, MetadataExt};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};

const MAX_ENTRIES: usize = 100_000;
const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 128;
pub(super) mod git_baseline;
pub(super) mod review;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SourceInventory {
    root_stamp: [i128; 9],
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    identity: (u64, u64, u64),
    kind: u8,
    mode: u32,
    size: u64,
    digest: [u8; 32],
}

struct Budget {
    entries: usize,
    bytes: u64,
    paths: usize,
}

fn failure() -> ProjectFailure {
    ProjectFailure::dirty("source inventory changed, is unsupported, or exceeds its budget")
}

// Excludes atime: reads can update it. Identity/ctime checks detect observed replacements,
// but cannot exclude a same-UID concurrent writer between observations.
fn stamp(m: &Metadata) -> [i128; 9] {
    [
        m.dev() as i128,
        m.ino() as i128,
        m.mode() as i128,
        m.nlink() as i128,
        m.len() as i128,
        m.mtime() as i128,
        m.mtime_nsec() as i128,
        m.ctime() as i128,
        m.ctime_nsec() as i128,
    ]
}

fn open(root: &Dir, name: &str, directory: bool) -> Result<File, ProjectFailure> {
    let name = CString::new(name).map_err(|_| failure())?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | libc::O_CLOEXEC
        | if directory { libc::O_DIRECTORY } else { 0 };
    // One component, relative to a held directory. NONBLOCK prevents a FIFO replacement
    // from hanging the reader before the opened file's type can be checked.
    let fd = unsafe { libc::openat(root.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(failure());
    }
    Ok(File::from_std(unsafe { std::fs::File::from_raw_fd(fd) }))
}

// Kept private and unconnected until HEAD/index comparison and durable binding exist.
#[allow(dead_code)]
pub(super) fn capture(root: &Dir) -> Result<SourceInventory, ProjectFailure> {
    let first = capture_once(root)?;
    let second = capture_once(root)?;
    if first != second {
        return Err(failure());
    }
    Ok(first)
}

fn capture_once(root: &Dir) -> Result<SourceInventory, ProjectFailure> {
    let metadata = root.dir_metadata().map_err(|_| failure())?;
    let mut inventory = SourceInventory {
        root_stamp: stamp(&metadata),
        entries: BTreeMap::new(),
    };
    let mut budget = Budget {
        entries: MAX_ENTRIES,
        bytes: MAX_BYTES,
        paths: MAX_PATH_BYTES,
    };
    scan(root, "", 0, &mut inventory.entries, &mut budget)?;
    if stamp(&metadata) != stamp(&root.dir_metadata().map_err(|_| failure())?) {
        return Err(failure());
    }
    Ok(inventory)
}

fn scan(
    root: &Dir,
    prefix: &str,
    depth: usize,
    entries: &mut BTreeMap<String, Entry>,
    budget: &mut Budget,
) -> Result<(), ProjectFailure> {
    if depth > MAX_DEPTH {
        return Err(failure());
    }
    let before = root.dir_metadata().map_err(|_| failure())?;
    for item in root.entries().map_err(|_| failure())? {
        let name = item.map_err(|_| failure())?.file_name();
        let name = name.to_str().ok_or_else(failure)?;
        if prefix.is_empty() && name == ".git" {
            continue;
        }
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(failure());
        }
        budget.entries = budget.entries.checked_sub(1).ok_or_else(failure)?;
        let path = format!("{prefix}{name}");
        if path.len() > 4096 {
            return Err(failure());
        }
        budget.paths = budget.paths.checked_sub(path.len()).ok_or_else(failure)?;
        let meta = root.symlink_metadata(name).map_err(|_| failure())?;
        if !meta.is_dir() && meta.nlink() != 1 {
            return Err(failure());
        }
        let (kind, size, digest) = if meta.file_type().is_symlink() {
            // Read the link itself, including dangling/outside links; never its target.
            let name_c = CString::new(name).map_err(|_| failure())?;
            let mut bytes = vec![0u8; 4097];
            let count = unsafe {
                libc::readlinkat(
                    root.as_raw_fd(),
                    name_c.as_ptr(),
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                )
            };
            if count < 0 || count as usize == bytes.len() {
                return Err(failure());
            }
            budget.bytes = budget.bytes.checked_sub(count as u64).ok_or_else(failure)?;
            (
                3,
                count as u64,
                Sha256::digest(&bytes[..count as usize]).into(),
            )
        } else if meta.is_dir() {
            let opened = open(root, name, true)?;
            if stamp(&meta) != stamp(&opened.metadata().map_err(|_| failure())?) {
                return Err(failure());
            }
            let child = Dir::from_std_file(opened.into_std());
            scan(&child, &format!("{path}/"), depth + 1, entries, budget)?;
            (2, 0, [0; 32])
        } else if meta.is_file() && meta.nlink() == 1 {
            let mut opened = open(root, name, false)?;
            if stamp(&meta) != stamp(&opened.metadata().map_err(|_| failure())?) {
                return Err(failure());
            }
            if meta.len() > budget.bytes {
                return Err(failure());
            }
            let mut hash = Sha256::new();
            let mut bytes = vec![0u8; 64 * 1024];
            let mut size = 0u64;
            loop {
                let count = opened.read(&mut bytes).map_err(|_| failure())?;
                if count == 0 {
                    break;
                }
                budget.bytes = budget.bytes.checked_sub(count as u64).ok_or_else(failure)?;
                size += count as u64;
                hash.update(&bytes[..count]);
            }
            if size != meta.len()
                || stamp(&meta) != stamp(&opened.metadata().map_err(|_| failure())?)
            {
                return Err(failure());
            }
            (1, size, hash.finalize().into())
        } else {
            return Err(failure());
        };
        if stamp(&meta) != stamp(&root.symlink_metadata(name).map_err(|_| failure())?) {
            return Err(failure());
        }
        if entries
            .insert(
                path,
                Entry {
                    identity: (meta.dev(), meta.ino(), meta.nlink()),
                    kind,
                    mode: meta.mode(),
                    size,
                    digest,
                },
            )
            .is_some()
        {
            return Err(failure());
        }
    }
    if stamp(&before) != stamp(&root.dir_metadata().map_err(|_| failure())?) {
        return Err(failure());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn source_inventory_covers_raw_ignored_link_and_mode_changes() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::create_dir(temp.path().join(".git")).unwrap();
        fs::write(temp.path().join(".git/control"), b"excluded").unwrap();
        fs::create_dir(temp.path().join("ignored")).unwrap();
        fs::write(temp.path().join("ignored/raw"), b"first").unwrap();
        symlink("/nonexistent/outside", temp.path().join("link")).unwrap();
        let first = capture(&root).unwrap();
        assert_eq!(first.entries.len(), 3);
        assert_eq!(first.entries["link"].kind, 3);
        assert_eq!(first, capture(&root).unwrap());
        fs::write(temp.path().join(".git/control"), b"changed").unwrap();
        assert_eq!(first, capture(&root).unwrap());
        fs::write(temp.path().join("ignored/raw"), b"other").unwrap();
        let second = capture(&root).unwrap();
        assert_ne!(first, second);
        fs::set_permissions(
            temp.path().join("ignored/raw"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert_ne!(second, capture(&root).unwrap());
        assert_eq!(fs::read(temp.path().join("ignored/raw")).unwrap(), b"other");
        fs::remove_file(temp.path().join("link")).unwrap();
        symlink("different", temp.path().join("link")).unwrap();
        assert_ne!(
            first.entries["link"],
            capture(&root).unwrap().entries["link"]
        );
    }

    #[test]
    fn source_inventory_rejects_shared_special_and_budget_overflow() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::write(temp.path().join("file"), b"data").unwrap();
        for mut budget in [
            Budget {
                entries: 0,
                bytes: 10,
                paths: 100,
            },
            Budget {
                entries: 10,
                bytes: 3,
                paths: 100,
            },
            Budget {
                entries: 10,
                bytes: 10,
                paths: 3,
            },
        ] {
            assert!(scan(&root, "", 0, &mut BTreeMap::new(), &mut budget).is_err());
        }
        fs::hard_link(temp.path().join("file"), temp.path().join("alias")).unwrap();
        assert!(capture(&root).is_err());
        fs::remove_file(temp.path().join("alias")).unwrap();
        let fifo = CString::new(temp.path().join("fifo").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(capture(&root).is_err());
        assert!(!open(&root, "fifo", false)
            .unwrap()
            .metadata()
            .unwrap()
            .is_file());
        assert_eq!(fs::read(temp.path().join("file")).unwrap(), b"data");
    }

    #[test]
    fn source_inventory_keeps_hidden_nested_git_binary_and_empty_directories() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::create_dir_all(temp.path().join("nested/.git")).unwrap();
        fs::create_dir(temp.path().join("empty")).unwrap();
        fs::write(temp.path().join("nested/.git/raw"), [0, 255, 13, 10]).unwrap();
        fs::write(temp.path().join(".hello-agent-save-test.tmp"), b"user file").unwrap();
        let before = root.symlink_metadata("nested/.git/raw").unwrap();
        let inventory = capture(&root).unwrap();
        assert_eq!(inventory.entries.len(), 5);
        assert_eq!(inventory.entries["empty"].kind, 2);
        assert_eq!(
            inventory.entries["nested/.git/raw"].digest,
            <[u8; 32]>::from(Sha256::digest([0, 255, 13, 10]))
        );
        assert!(inventory.entries.contains_key(".hello-agent-save-test.tmp"));
        assert_eq!(
            stamp(&before),
            stamp(&root.symlink_metadata("nested/.git/raw").unwrap())
        );
        assert!(scan(
            &root,
            "",
            MAX_DEPTH + 1,
            &mut BTreeMap::new(),
            &mut Budget {
                entries: MAX_ENTRIES,
                bytes: MAX_BYTES,
                paths: MAX_PATH_BYTES
            }
        )
        .is_err());
    }

    #[test]
    fn source_inventory_depth_limit_does_not_overflow_stack() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let mut path = temp.path().to_path_buf();
        for _ in 0..MAX_DEPTH {
            path.push("d");
            fs::create_dir(&path).unwrap();
        }
        fs::write(path.join("file"), b"leaf").unwrap();
        assert_eq!(capture(&root).unwrap().entries.len(), MAX_DEPTH + 1);
        fs::create_dir(path.join("over")).unwrap();
        assert!(capture(&root).is_err());
    }

    #[test]
    fn source_inventory_does_not_read_link_targets() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("source")).unwrap();
        fs::write(temp.path().join("outside"), b"outside one").unwrap();
        symlink("../outside", temp.path().join("source/link")).unwrap();
        let root = Dir::open_ambient_dir(temp.path().join("source"), ambient_authority()).unwrap();
        let before = capture(&root).unwrap();
        assert!(open(&root, "link", false).is_err());
        fs::write(temp.path().join("outside"), b"outside two").unwrap();
        assert_eq!(before, capture(&root).unwrap());
        // APFS rejects invalid UTF-8 names before the scanner can observe them.
        #[cfg(target_os = "linux")]
        {
            fs::write(
                temp.path()
                    .join("source")
                    .join(std::ffi::OsStr::from_bytes(&[0xff])),
                b"data",
            )
            .unwrap();
            assert!(capture(&root).is_err());
        }
    }

    #[test]
    fn source_inventory_detects_same_bytes_inode_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::write(temp.path().join("file"), b"same").unwrap();
        fs::create_dir(temp.path().join("dir")).unwrap();
        let before = capture(&root).unwrap();
        fs::write(temp.path().join("replacement"), b"same").unwrap();
        fs::rename(temp.path().join("replacement"), temp.path().join("file")).unwrap();
        let after = capture(&root).unwrap();
        assert_eq!(before.entries["file"].digest, after.entries["file"].digest);
        assert_ne!(
            before.entries["file"].identity,
            after.entries["file"].identity
        );
        assert_ne!(before, after);
        fs::create_dir(temp.path().join("replacement-dir")).unwrap();
        fs::remove_dir(temp.path().join("dir")).unwrap();
        fs::rename(temp.path().join("replacement-dir"), temp.path().join("dir")).unwrap();
        let replaced = capture(&root).unwrap();
        assert_ne!(
            after.entries["dir"].identity,
            replaced.entries["dir"].identity
        );
        assert_ne!(after, replaced);
    }

    #[test]
    fn source_inventory_rejects_hardlinked_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        symlink("missing", temp.path().join("link")).unwrap();
        fs::hard_link(temp.path().join("link"), temp.path().join("alias")).unwrap();
        assert_eq!(root.symlink_metadata("link").unwrap().nlink(), 2);
        assert!(capture(&root).is_err());
    }

    #[test]
    fn source_inventory_detects_root_mode_change() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let before = capture(&root).unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o750)).unwrap();
        assert_ne!(before, capture(&root).unwrap());
    }
}
