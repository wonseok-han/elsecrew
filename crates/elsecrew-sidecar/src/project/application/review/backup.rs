//! Immutable before-image archive candidate; not a published restore point or writer.
use super::*;
use crate::project::application::{open, stamp, MAX_BYTES};
use crate::project::{create_managed_directory, sync_capability_directory};
use cap_std::fs::{Dir, File, MetadataExt};
use std::ffi::CString;
use std::io::{Read, Seek};
use std::os::fd::{AsRawFd, FromRawFd};
use uuid::Uuid;

const FORMAT_VERSION: u32 = 2;
const MAX_ARCHIVE: u64 = MAX_BYTES + 32 * 1024 * 1024;
const RESTORE_BUDGET: u64 = 5 * 1024 * 1024 * 1024;
mod decode;
mod budget_settings;
pub(in crate::project) mod space;
pub(in crate::project) mod cleanup;
pub(in crate::project) mod retention;
pub(in crate::project) mod staging;

pub(in crate::project) struct Backup {
    name: String,
    length: u64,
    digest: [u8; 32],
}

fn valid_name(name: &str) -> bool {
    name.len() == 43
        && name.starts_with("backup_")
        && name.ends_with(".bin")
        && name[7..39]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[allow(dead_code)]
impl Backup {
    pub(in crate::project) fn to_value(&self) -> Value {
        serde_json::json!({"version":2,"name":self.name,"length":self.length,"digest":self.digest})
    }

    // Only trusted ledger metadata may supply this descriptor; not renderer input.
    pub(in crate::project) fn from_value(value: &Value) -> Result<Self, ProjectFailure> {
        let fields = value.as_object().ok_or_else(invalid)?;
        let name = value["name"].as_str().ok_or_else(invalid)?;
        let length = value["length"].as_u64().ok_or_else(invalid)?;
        if fields.len() != 4
            || value["version"] != serde_json::json!(2)
            || !valid_name(name)
            || length == 0
            || length > MAX_ARCHIVE
        {
            return Err(invalid());
        }
        let digest: [u8; 32] =
            serde_json::from_value(value["digest"].clone()).map_err(|_| invalid())?;
        Ok(Self {
            name: name.to_owned(),
            length,
            digest,
        })
    }
}

#[cfg(test)]
impl Backup {
    pub(in crate::project) fn path_for_test(&self, manager: &ProjectManager) -> std::path::PathBuf {
        manager
            .data_root
            .join("application-backups-v1")
            .join(&self.name)
    }
}

struct ArchiveWriter {
    file: File,
    hash: Sha256,
    length: u64,
    limit: u64,
}

impl Write for ArchiveWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.length) {
            return Err(io::Error::other("backup exceeds archive budget"));
        }
        let n = self.file.write(bytes)?;
        self.hash.update(&bytes[..n]);
        self.length += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn invalid() -> ProjectFailure {
    ProjectFailure::recovery("before-image archive is incomplete, changed, or exceeds its budget")
}

fn directory(manager: &ProjectManager) -> Result<Dir, ProjectFailure> {
    manager.verify_managed_paths()?;
    Ok(create_managed_directory(
        &manager.data_root,
        &manager.data_dir,
        "application-backups-v1",
    )?
    .1)
}

// The owner explicitly ends its lock, including on error. Closing a descriptor
// alone does not release flock while a concurrent fork/dup retains the same
// open file description. Construct only after successful acquisition.
struct ArchiveLock(Dir);

impl ArchiveLock {
    fn acquire(dir: Dir, operation: libc::c_int, detail: &'static str) -> Result<Self, ProjectFailure> {
        if unsafe { libc::flock(dir.as_raw_fd(), operation | libc::LOCK_NB) } != 0 {
            return Err(ProjectFailure::recovery(detail));
        }
        Ok(Self(dir))
    }
}

impl std::ops::Deref for ArchiveLock {
    type Target = Dir;
    fn deref(&self) -> &Dir { &self.0 }
}

impl Drop for ArchiveLock {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

// Held through publication/cleanup on the same directory inode.
struct BudgetGuard {
    _directory: ArchiveLock,
    remaining: u64,
}

impl BudgetGuard {
    #[cfg(test)]
    fn acquire(dir: Dir, budget: u64) -> Result<Self, ProjectFailure> {
        Self::acquire_with(dir, || Ok(budget))
    }

    fn acquire_with(dir: Dir, budget: impl FnOnce() -> Result<u64, ProjectFailure>) -> Result<Self, ProjectFailure> {
        let dir = ArchiveLock::acquire(dir, libc::LOCK_EX, "another backup preparation is active")?;
        let used = stored_bytes(&dir)?;
        let budget = budget()?;
        let remaining = budget.checked_sub(used).filter(|v| *v > 0).ok_or_else(|| {
            ProjectFailure::recovery("restore storage budget reached; existing backups preserved")
        })?;
        Ok(Self {
            _directory: dir,
            remaining,
        })
    }
}

fn stored_bytes(dir: &Dir) -> Result<u64, ProjectFailure> {
    let mut used = 0u64;
    for entry in dir.entries().map_err(|_| invalid())? {
        let entry = entry.map_err(|_| invalid())?;
        let meta = dir.symlink_metadata(entry.file_name()).map_err(|_| invalid())?;
        // Include orphaned/abandoned archives. Never follow or discount unknown objects.
        if !meta.is_file() || meta.nlink() != 1 { return Err(invalid()); }
        used = used.checked_add(meta.len()).ok_or_else(invalid)?;
    }
    Ok(used)
}

pub(in crate::project) fn storage_usage(manager: &ProjectManager) -> Result<Value, ProjectFailure> {
    // Same managed directory and cooperative lock as archive creation. This may
    // initialize an empty managed directory, but never changes archive contents.
    let dir = ArchiveLock::acquire(directory(manager)?, libc::LOCK_SH, "backup usage unavailable during preparation")?;
    let used = stored_bytes(&dir)?;
    if used > 9_007_199_254_740_991 { return Err(invalid()); }
    let limit = budget_settings::read(manager)?;
    Ok(serde_json::json!({"usedBytes":used,"limitBytes":limit,"remainingBytes":limit.saturating_sub(used)}))
}

pub(in crate::project) fn set_storage_limit(manager: &ProjectManager, expected: u64, limit: u64) -> Result<(), ProjectFailure> {
    let _lock = ArchiveLock::acquire(directory(manager)?, libc::LOCK_EX, "backup budget change requires idle preparation")?;
    budget_settings::set(manager, expected, limit)
}

// Every intermediate component is opened without following links.
fn parent(root: &Dir, path: &str) -> Result<(Dir, String), ProjectFailure> {
    let mut parts: Vec<_> = path.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return Err(invalid());
    }
    let name = parts.pop().ok_or_else(invalid)?.to_owned();
    let mut dir = root.try_clone().map_err(|_| invalid())?;
    for part in parts {
        dir = Dir::from_std_file(open(&dir, part, true)?.into_std());
    }
    Ok((dir, name))
}

#[allow(dead_code)]
pub(in crate::project) fn create(
    manager: &ProjectManager,
    storage: &Storage,
    target: ReviewTarget<'_>,
    review_id: &str,
) -> Result<Backup, ProjectFailure> {
    let name = format!("backup_{}.bin", Uuid::new_v4().simple());
    create_inner(manager, storage, target, review_id, &name, |_| Ok(()))
}

#[cfg(test)]
pub(in crate::project) fn fail_for_test(
    manager: &ProjectManager,
    storage: &Storage,
    target: ReviewTarget<'_>,
    review_id: &str,
    stage: u8,
) -> Result<Backup, ProjectFailure> {
    let name = format!("backup_{}.bin", Uuid::new_v4().simple());
    create_inner(manager, storage, target, review_id, &name, |at| {
        if at == stage {
            Err(invalid())
        } else {
            Ok(())
        }
    })
}

fn create_inner(
    manager: &ProjectManager,
    storage: &Storage,
    target: ReviewTarget<'_>,
    review_id: &str,
    name: &str,
    mut check: impl FnMut(u8) -> Result<(), ProjectFailure>,
) -> Result<Backup, ProjectFailure> {
    manager.check_application_preparation()?;
    let plan = persistence::reopen_target(manager, storage, target, review_id)?;
    let (active, _) = target.resolve(manager, storage)?;
    let source = manager.open_source_repository(&active.canonical_git_root)?;
    let baseline = capture_clean_baseline(manager, &source)?;
    receipt::verify_bound(manager, storage, &active, &baseline)?;
    space::plan(manager, &plan)?;
    let budget = BudgetGuard::acquire_with(directory(manager)?, || budget_settings::read(manager))?;
    let dir = &budget._directory;
    let c = CString::new(name).map_err(|_| invalid())?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(invalid());
    }
    let file = File::from_std(unsafe { std::fs::File::from_raw_fd(fd) });
    let result = (|| {
        let mut out = ArchiveWriter {
            file: file.try_clone().map_err(|_| invalid())?,
            hash: Sha256::new(),
            length: 0,
            limit: MAX_ARCHIVE.min(budget.remaining),
        };
        // JSON header lines specify payload byte lengths. Payload may contain newlines.
        serde_json::to_writer(&mut out, &serde_json::json!({"version":2,"reviewId":review_id,"binding":plan.binding,"changesDigest":plan.digest,"count":plan.changes.len()})).map_err(|_| invalid())?;
        out.write_all(b"\n").map_err(|_| invalid())?;
        check(1)?;
        for (path, change) in &plan.changes {
            manager.check_application_preparation()?;
            let entry = baseline.inventory().entries.get(path);
            if change.before.as_ref() != entry.map(Image::from).as_ref() {
                return Err(invalid());
            }
            let metadata = entry.map(|e| serde_json::json!({"kind":e.kind,"mode":e.mode,"size":e.size,"digest":e.digest,"identity":e.identity}));
            let after = change.after.as_ref().map(|e| serde_json::json!({"kind":e.kind,"executable":e.executable,"size":e.size,"digest":e.digest}));
            serde_json::to_writer(
                &mut out,
                &serde_json::json!({"path":path,"before":metadata,"after":after}),
            )
            .map_err(|_| invalid())?;
            out.write_all(b"\n").map_err(|_| invalid())?;
            let Some(entry) = entry else {
                continue;
            };
            let (parent, leaf) = parent(&source._root, path)?;
            let meta = parent.symlink_metadata(&leaf).map_err(|_| invalid())?;
            if (meta.dev(), meta.ino(), meta.nlink()) != entry.identity || meta.mode() != entry.mode
            {
                return Err(invalid());
            }
            let mut hash = Sha256::new();
            match entry.kind {
                1 => {
                    let mut file = open(&parent, &leaf, false)?;
                    if stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?) {
                        return Err(invalid());
                    }
                    let mut remaining = entry.size;
                    let mut buffer = [0u8; 65536];
                    while remaining > 0 {
                        manager.check_application_preparation()?;
                        let limit = remaining.min(buffer.len() as u64) as usize;
                        let n = file.read(&mut buffer[..limit]).map_err(|_| invalid())?;
                        manager.check_application_preparation()?;
                        if n == 0 {
                            return Err(invalid());
                        }
                        out.write_all(&buffer[..n]).map_err(|_| invalid())?;
                        hash.update(&buffer[..n]);
                        remaining -= n as u64;
                    }
                    if file.read(&mut buffer[..1]).map_err(|_| invalid())? != 0
                        || stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?)
                    {
                        return Err(invalid());
                    }
                }
                3 => {
                    if entry.size > 4096 {
                        return Err(invalid());
                    }
                    let mut bytes = [0u8; 4097];
                    let leaf = CString::new(leaf.as_str()).map_err(|_| invalid())?;
                    let n = unsafe {
                        libc::readlinkat(
                            parent.as_raw_fd(),
                            leaf.as_ptr(),
                            bytes.as_mut_ptr().cast(),
                            bytes.len(),
                        )
                    };
                    if n < 0 || n as u64 != entry.size {
                        return Err(invalid());
                    }
                    out.write_all(&bytes[..n as usize]).map_err(|_| invalid())?;
                    hash.update(&bytes[..n as usize]);
                }
                _ => return Err(invalid()),
            }
            if <[u8; 32]>::from(hash.finalize()) != entry.digest
                || stamp(&meta) != stamp(&parent.symlink_metadata(&leaf).map_err(|_| invalid())?)
            {
                return Err(invalid());
            }
        }
        out.file.sync_all().map_err(|_| invalid())?;
        sync_capability_directory(dir)?;
        let backup = Backup {
            name: name.to_owned(),
            length: out.length,
            digest: out.hash.finalize().into(),
        };
        verify(manager, &backup)?;
        check(2)?;
        let current = persistence::reopen_target(manager, storage, target, review_id)?;
        if current.binding != plan.binding
            || current.digest != plan.digest
            || current.snapshot != plan.snapshot
            || capture_clean_baseline(manager, &source)? != baseline
        {
            return Err(invalid());
        }
        manager.check_application_preparation()?;
        Ok(backup)
    })();
    if result.is_err() && manager.check_application_preparation().is_ok() {
        remove_failed(dir, name, &file)?;
    }
    result
}

fn remove_failed(dir: &Dir, name: &str, file: &File) -> Result<(), ProjectFailure> {
    let failure =
        || ProjectFailure::recovery(format!("incomplete backup preserved for recovery: {name}"));
    let held = file.metadata().map_err(|_| failure())?;
    let linked = dir.symlink_metadata(name).map_err(|_| failure())?;
    if !held.is_file() || held.nlink() != 1 || stamp(&held) != stamp(&linked) {
        return Err(failure());
    }
    dir.remove_file(name).map_err(|_| failure())?;
    sync_capability_directory(dir).map_err(|_| failure())
}

// Recovery candidate only. Full current baseline and durable header comparison
// are required before the caller can publish this newly computed descriptor.
fn recover_candidate(
    manager: &ProjectManager,
    name: &str,
    baseline: &SourceInventory,
) -> Result<(Backup, Value), ProjectFailure> {
    if !valid_name(name) {
        return Err(invalid());
    }
    let dir = directory(manager)?;
    let mut file = open(&dir, name, false)?;
    let meta = file.metadata().map_err(|_| invalid())?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.mode() & 0o7777 != 0o600
        || meta.len() == 0
        || meta.len() > MAX_ARCHIVE
    {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        manager.check_application_preparation()?;
        let n = file.read(&mut buffer).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > meta.len() {
            return Err(invalid());
        }
        hash.update(&buffer[..n]);
    }
    file.rewind().map_err(|_| invalid())?;
    let (header, _) = decode::validate_inner(
        &mut std::io::BufReader::new(&mut file), Some(baseline), None, false,
        || manager.check_application_preparation(),
    )?;
    if total != meta.len()
        || stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?)
        || stamp(&meta) != stamp(&dir.symlink_metadata(name).map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    Ok((
        Backup {
            name: name.to_owned(),
            length: total,
            digest: hash.finalize().into(),
        },
        header,
    ))
}

#[allow(dead_code)]
pub(in crate::project) fn verify(
    manager: &ProjectManager,
    backup: &Backup,
) -> Result<Value, ProjectFailure> {
    verify_inner(manager, backup, None, false).map(|(header, _)| header)
}

/// Read a recovery item's exact metadata from a trusted ledger descriptor.
/// This does not grant write authority or classify the current source file.
#[allow(dead_code)]
pub(in crate::project) fn verify_item(
    manager: &ProjectManager,
    backup: &Backup,
    index: u64,
) -> Result<(Value, Value), ProjectFailure> {
    let (header, item) = verify_inner(manager, backup, Some(index), false)?;
    Ok((header, item.ok_or_else(invalid)?))
}

pub(in crate::project) fn verify_items(
    manager: &ProjectManager,
    backup: &Backup,
) -> Result<(Value, Vec<Value>), ProjectFailure> {
    let (header, items) = verify_inner(manager, backup, None, true)?;
    let Some(Value::Array(items)) = items else {
        return Err(invalid());
    };
    Ok((header, items))
}

fn verify_inner(
    manager: &ProjectManager,
    backup: &Backup,
    index: Option<u64>,
    collect_all: bool,
) -> Result<(Value, Option<Value>), ProjectFailure> {
    let dir = directory(manager)?;
    let mut file = open(&dir, &backup.name, false)?;
    let meta = file.metadata().map_err(|_| invalid())?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.mode() & 0o7777 != 0o600
        || meta.len() != backup.length
        || backup.length > MAX_ARCHIVE
    {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut length = 0;
    loop {
        manager.check_application_preparation()?;
        let n = file.read(&mut buffer).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        length += n as u64;
        if length > backup.length {
            return Err(invalid());
        }
        hash.update(&buffer[..n]);
    }
    if length != backup.length
        || <[u8; 32]>::from(hash.finalize()) != backup.digest
        || stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?)
        || stamp(&meta) != stamp(&dir.symlink_metadata(&backup.name).map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    file.rewind().map_err(|_| invalid())?;
    let (header, item) = decode::validate_inner(
        &mut std::io::BufReader::new(&mut file), None, index, collect_all,
        || manager.check_application_preparation(),
    )?;
    if stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?)
        || stamp(&meta) != stamp(&dir.symlink_metadata(&backup.name).map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    Ok((header, item))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_budget_fails_before_writing_and_parent_links_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let mut output = ArchiveWriter {
            file: File::from_std(std::fs::File::create(temp.path().join("archive")).unwrap()),
            hash: Sha256::new(),
            length: MAX_ARCHIVE,
            limit: MAX_ARCHIVE,
        };
        assert!(output.write_all(b"x").is_err());
        assert_eq!(output.file.metadata().unwrap().len(), 0);
        let root = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        std::os::unix::fs::symlink("/tmp", temp.path().join("escape")).unwrap();
        assert!(parent(&root, "escape/file").is_err());
        for path in ["../outside", "/absolute", "a//b", "./file"] {
            assert!(parent(&root, path).is_err());
        }
        remove_failed(&root, "archive", &output.file).unwrap();
        assert!(!temp.path().join("archive").exists());
        std::fs::write(temp.path().join("archive"), b"replacement").unwrap();
        assert!(remove_failed(&root, "archive", &output.file).is_err());
        assert_eq!(
            std::fs::read(temp.path().join("archive")).unwrap(),
            b"replacement"
        );
    }

    #[test]
    fn backup_lock_ends_with_owner_even_if_descriptor_is_duplicated() {
        let temp = tempfile::tempdir().unwrap();
        let open_dir = || Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        let guard = BudgetGuard::acquire(open_dir(), 10).unwrap();
        let duplicate = guard._directory.try_clone().unwrap();
        assert!(BudgetGuard::acquire(open_dir(), 10).is_err());
        drop(guard);
        assert!(BudgetGuard::acquire(open_dir(), 10).is_ok());
        drop(duplicate);
    }

    #[test]
    fn backup_lock_releases_shared_owner_and_failed_budget_admission() {
        let temp = tempfile::tempdir().unwrap();
        let open_dir = || Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        let reader = ArchiveLock::acquire(open_dir(), libc::LOCK_SH, "busy").unwrap();
        let duplicate = reader.try_clone().unwrap();
        assert!(BudgetGuard::acquire(open_dir(), 10).is_err());
        // Failed acquisition must not unlock another owner's shared lock.
        assert!(BudgetGuard::acquire(open_dir(), 10).is_err());
        drop(reader);
        assert!(BudgetGuard::acquire(open_dir(), 10).is_ok());
        drop(duplicate);

        let dir = open_dir();
        let duplicate = dir.try_clone().unwrap();
        assert!(BudgetGuard::acquire_with(dir, || Err(invalid())).is_err());
        assert!(BudgetGuard::acquire(open_dir(), 10).is_ok());
        drop(duplicate);
    }

    #[test]
    fn restore_budget_counts_orphans_serializes_and_preserves_existing_data() {
        let temp = tempfile::tempdir().unwrap();
        let open_dir = || Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        std::fs::write(temp.path().join("orphan"), b"keep").unwrap();
        let guard = BudgetGuard::acquire(open_dir(), 6).unwrap();
        assert_eq!(guard.remaining, 2);
        assert!(BudgetGuard::acquire(open_dir(), 6).is_err());
        let mut output = ArchiveWriter {
            file: File::from_std(std::fs::File::create(temp.path().join("new")).unwrap()),
            hash: Sha256::new(),
            length: 0,
            limit: guard.remaining,
        };
        output.write_all(b"ok").unwrap();
        assert!(output.write_all(b"x").is_err());
        drop(guard);
        // Inspection still reports occupied bytes when admission has no budget.
        assert_eq!(stored_bytes(&open_dir()).unwrap(), 6);
        assert!(BudgetGuard::acquire(open_dir(), 6).is_err());
        assert!(BudgetGuard::acquire(open_dir(), 5).is_err());
        assert_eq!(std::fs::read(temp.path().join("orphan")).unwrap(), b"keep");
        assert_eq!(std::fs::read(temp.path().join("new")).unwrap(), b"ok");
        assert!(BudgetGuard::acquire(open_dir(), 7).is_ok());
        std::os::unix::fs::symlink("orphan", temp.path().join("link")).unwrap();
        assert!(stored_bytes(&open_dir()).is_err());
        assert!(BudgetGuard::acquire(open_dir(), 100).is_err());
    }
}
