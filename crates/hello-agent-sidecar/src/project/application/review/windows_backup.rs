//! Windows before-image archive. Kept behind the existing source-write gate.
use super::*;
use crate::project::{
    open_parent_capability, open_real_file_at, windows_backup_storage::BudgetGuard,
    windows_durability, windows_replace, windows_security::SavedAccess,
    windows_source_inventory::file_observation,
};
use cap_std::fs::{Dir, File};
use std::io::Read;
use uuid::Uuid;
#[path = "backup/staging.rs"]
pub(in crate::project) mod staging;
const FORMAT_VERSION: u32 = 3;
const MAX_ARCHIVE: u64 = 4 * 1024 * 1024 * 1024 + 32 * 1024 * 1024;
const MAX_METADATA: usize = 1024 * 1024;
const MAGIC: &[u8] = b"HELLO_AGENT_WINDOWS_BACKUP_1\n";
fn invalid() -> ProjectFailure {
    ProjectFailure::recovery("Windows backup is incomplete, changed, or exceeds its budget")
}
fn valid_name(name: &str) -> bool {
    name.len() == 43
        && name.starts_with("backup_")
        && name.ends_with(".bin")
        && name[7..39]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::project) struct Backup {
    version: u32,
    name: String,
    length: u64,
    digest: [u8; 32],
}
impl Backup {
    pub(in crate::project) fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("fixed backup fields")
    }
    pub(in crate::project) fn from_value(value: &Value) -> Result<Self, ProjectFailure> {
        let result: Self = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        if result.version != FORMAT_VERSION
            || !valid_name(&result.name)
            || result.length == 0
            || result.length > MAX_ARCHIVE
        {
            return Err(invalid());
        }
        Ok(result)
    }
}
struct Output {
    file: File,
    length: u64,
    limit: u64,
    hash: Sha256,
    metadata_left: usize,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.length) {
            return Err(io::Error::other("Windows backup budget exceeded"));
        }
        let count = self.file.write(bytes)?;
        self.hash.update(&bytes[..count]);
        self.length += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
fn write_json(output: &mut Output, value: &Value) -> Result<(), ProjectFailure> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() > MAX_METADATA {
        return Err(invalid());
    }
    output.metadata_left = output
        .metadata_left
        .checked_sub(bytes.len() + 4)
        .ok_or_else(invalid)?;
    output
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .map_err(|_| invalid())?;
    output.write_all(&bytes).map_err(|_| invalid())
}
pub(in crate::project) mod space {
    use super::*;
    pub(in crate::project) fn intent(
        manager: &ProjectManager,
        record: &Value,
        restoring: bool,
    ) -> Result<(), ProjectFailure> {
        let backup = Backup::from_value(&record["backup"]["backup"])?;
        let (header, items) = verify_items(manager, &backup)?;
        if header != record["backup"]["header"] || header["binding"] != record["binding"] {
            return Err(invalid());
        }
        let root = crate::project::open_absolute_directory_capability(std::path::Path::new(
            record["binding"]["canonicalGitRoot"]
                .as_str()
                .ok_or_else(invalid)?,
        ))?;
        let mut volumes: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for item in items {
            manager.check_application_preparation()?;
            let path = item["path"].as_str().ok_or_else(invalid)?;
            let mut parent = root.try_clone().map_err(|_| invalid())?;
            let mut components = path.split('/').peekable();
            while let Some(component) = components.next() {
                if components.peek().is_none() {
                    break;
                }
                match parent.symlink_metadata(component) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                    Err(_) => return Err(invalid()),
                    Ok(_) => parent = crate::project::open_real_child_dir(&parent, component)?,
                }
            }
            let guid = crate::project::windows_fs::local_guid_path(&parent)?;
            let volume = guid.split_once("}\\").ok_or_else(invalid)?.0.to_owned();
            let free = crate::project::available_space(&parent)?;
            let bytes = if restoring {
                item["before"]["observation"]["length"]
                    .as_u64()
                    .unwrap_or(0)
            } else {
                item["after"]["size"].as_u64().unwrap_or(0)
            };
            let entry = volumes.entry(volume).or_insert((free, 0));
            entry.0 = entry.0.min(free);
            entry.1 = entry
                .1
                .checked_add(bytes)
                .and_then(|v| v.checked_add(65536))
                .ok_or_else(invalid)?;
        }
        for (free, bytes) in volumes.values() {
            if *free
                < bytes
                    .checked_add(crate::project::SNAPSHOT_DISK_RESERVE_BYTES_V1)
                    .ok_or_else(invalid)?
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub(super) fn plan(manager: &ProjectManager, plan: &ReviewPlan) -> Result<(), ProjectFailure> {
        let mut bytes = 32u64 * 1024 * 1024;
        for change in plan.changes.values() {
            bytes = bytes
                .checked_add(change.before.as_ref().map_or(0, |v| v.size))
                .ok_or_else(invalid)?;
        }
        let required = bytes
            .checked_add(crate::project::SNAPSHOT_DISK_RESERVE_BYTES_V1)
            .ok_or_else(invalid)?;
        if crate::project::available_space(&manager.data_dir)? < required {
            return Err(invalid());
        }
        Ok(())
    }
}
fn create_inner(
    manager: &ProjectManager,
    storage: &Storage,
    target: ReviewTarget<'_>,
    review_id: &str,
    name: &str,
    mut check: impl FnMut(u8) -> Result<(), ProjectFailure>,
) -> Result<Backup, ProjectFailure> {
    if !valid_name(name) {
        return Err(invalid());
    }
    let plan = persistence::reopen_target(manager, storage, target, review_id)?;
    let (active, _) = target.resolve(manager, storage)?;
    let source = manager.open_source_repository(&active.canonical_git_root)?;
    let baseline = capture_clean_baseline(manager, &source)?;
    receipt::verify_bound(manager, storage, &active, &baseline)?;
    space::plan(manager, &plan)?;
    let guard = BudgetGuard::acquire(manager)?;
    windows_durability::require_ntfs(&guard.directory)?;
    let file = guard
        .directory
        .open_with(name, &windows_replace::source_options())
        .map_err(|_| invalid())?;
    let mut output = Output {
        file,
        length: 0,
        limit: MAX_ARCHIVE.min(guard.remaining()),
        hash: Sha256::new(),
        metadata_left: 32 * 1024 * 1024,
    };
    output.write_all(MAGIC).map_err(|_| invalid())?;
    let header = serde_json::json!({"version":FORMAT_VERSION,"reviewId":review_id,"binding":plan.binding,"changesDigest":plan.digest,"count":plan.changes.len()});
    write_json(&mut output, &header)?;
    check(1)?;
    let mut buffer = vec![0u8; 65536];
    for (path, change) in &plan.changes {
        manager.check_application_preparation()?;
        let entry = baseline.inventory().entries.get(path);
        if change.before.as_ref() != entry.map(Image::from).as_ref() {
            return Err(invalid());
        }
        let mut source_file = if let Some(entry) = entry {
            if entry.kind() != 1 {
                return Err(invalid());
            }
            let (parent, leaf) = open_parent_capability(&source._root, path)?;
            let file = open_real_file_at(&parent, &leaf)?;
            if &file_observation(&file)? != entry.observation() {
                return Err(invalid());
            }
            Some((file, parent, leaf))
        } else {
            None
        };
        let access = source_file
            .as_ref()
            .map(|(file, _, _)| SavedAccess::capture(file))
            .transpose()?;
        if let (Some(access), Some(entry)) = (&access, entry) {
            if access.validated_fingerprint()? != entry.access_fingerprint() {
                return Err(invalid());
            }
        }
        let after = change.after.as_ref().map(|v| serde_json::json!({"kind":v.kind,"executable":v.executable,"size":v.size,"digest":v.digest}));
        write_json(
            &mut output,
            &serde_json::json!({"path":path,"before":entry,"access":access,"after":after}),
        )?;
        if let Some((file, parent, leaf)) = &mut source_file {
            let entry = entry.ok_or_else(invalid)?;
            let (mut remaining, expected) = entry.content();
            let mut hash = Sha256::new();
            while remaining != 0 {
                manager.check_application_preparation()?;
                let count = remaining.min(buffer.len() as u64) as usize;
                file.read_exact(&mut buffer[..count])
                    .map_err(|_| invalid())?;
                output.write_all(&buffer[..count]).map_err(|_| invalid())?;
                hash.update(&buffer[..count]);
                remaining -= count as u64;
            }
            let actual: [u8; 32] = hash.finalize().into();
            if actual != expected
                || &file_observation(file)? != entry.observation()
                || &file_observation(&open_real_file_at(parent, leaf)?)? != entry.observation()
            {
                return Err(invalid());
            }
        }
    }
    output.flush().map_err(|_| invalid())?;
    output.file.sync_all().map_err(|_| invalid())?;
    let backup = Backup {
        version: FORMAT_VERSION,
        name: name.into(),
        length: output.length,
        digest: output.hash.finalize().into(),
    };
    drop(output.file);
    if read_archive(
        manager,
        &guard.directory,
        name,
        Some(&backup),
        Some(baseline.inventory()),
        false,
    )?
    .1 != header
    {
        return Err(invalid());
    }
    let current = persistence::reopen_target(manager, storage, target, review_id)?;
    if current.binding != plan.binding
        || current.digest != plan.digest
        || current.snapshot != plan.snapshot
        || capture_clean_baseline(manager, &source)? != baseline
    {
        return Err(invalid());
    }
    check(2)?;
    Ok(backup)
}
struct Input {
    file: File,
    length: u64,
    hash: Sha256,
    metadata_left: usize,
}
impl Read for Input {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = self.file.read(bytes)?;
        self.length = self
            .length
            .checked_add(count as u64)
            .filter(|v| *v <= MAX_ARCHIVE)
            .ok_or_else(|| io::Error::other("archive too large"))?;
        self.hash.update(&bytes[..count]);
        Ok(count)
    }
}
fn read_json(input: &mut Input) -> Result<Value, ProjectFailure> {
    let mut size = [0u8; 4];
    input.read_exact(&mut size).map_err(|_| invalid())?;
    let size = u32::from_le_bytes(size) as usize;
    if size == 0 || size > MAX_METADATA {
        return Err(invalid());
    }
    input.metadata_left = input
        .metadata_left
        .checked_sub(size + 4)
        .ok_or_else(invalid)?;
    let mut bytes = vec![0; size];
    input.read_exact(&mut bytes).map_err(|_| invalid())?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}
fn read_archive(
    manager: &ProjectManager,
    directory: &Dir,
    name: &str,
    expected: Option<&Backup>,
    baseline: Option<&SourceInventory>,
    collect: bool,
) -> Result<(Backup, Value, Vec<Value>), ProjectFailure> {
    if !valid_name(name) {
        return Err(invalid());
    }
    let file = open_real_file_at(directory, name)?;
    let before = file_observation(&file)?;
    if before.length == 0 || before.length > MAX_ARCHIVE {
        return Err(invalid());
    }
    let mut input = Input {
        file,
        length: 0,
        hash: Sha256::new(),
        metadata_left: 32 * 1024 * 1024,
    };
    let mut magic = vec![0; MAGIC.len()];
    input.read_exact(&mut magic).map_err(|_| invalid())?;
    if magic != MAGIC {
        return Err(invalid());
    }
    let header = read_json(&mut input)?;
    let count = header["count"]
        .as_u64()
        .filter(|v| *v <= 100_000)
        .ok_or_else(invalid)?;
    if header.as_object().map(|v| v.len()) != Some(5) || header["version"] != FORMAT_VERSION {
        return Err(invalid());
    }
    let mut changes = ReviewHash {
        hash: Sha256::new(),
        remaining: MAX_REVIEW_BYTES,
    };
    serde_json::to_writer(&mut changes, &(1u32, &header["binding"])).map_err(|_| invalid())?;
    let mut items = Vec::new();
    let mut previous = String::new();
    let mut buffer = vec![0u8; 65536];
    for _ in 0..count {
        manager.check_application_preparation()?;
        let record = read_json(&mut input)?;
        if record.as_object().map(|v| v.len()) != Some(4) {
            return Err(invalid());
        }
        let path = record["path"].as_str().ok_or_else(invalid)?;
        crate::project::validate_relative_path(path, false)?;
        if path <= previous.as_str() || path.split('/').any(|v| v.eq_ignore_ascii_case(".git")) {
            return Err(invalid());
        }
        previous = path.into();
        if let Some(baseline) = baseline {
            if serde_json::to_value(baseline.entries.get(path)).map_err(|_| invalid())?
                != record["before"]
            {
                return Err(invalid());
            }
        }
        let after: Option<Image> =
            serde_json::from_value(record["after"].clone()).map_err(|_| invalid())?;
        if after.as_ref().is_some_and(|v| {
            !matches!(v.kind, 1 | 2)
                || v.executable
                || (v.kind == 2 && (v.size != 0 || v.digest != [0; 32]))
        }) {
            return Err(invalid());
        }
        let before_image = if record["before"].is_null() {
            None
        } else {
            Some(Image {
                kind: 1,
                executable: false,
                size: record["before"]["observation"]["length"]
                    .as_u64()
                    .ok_or_else(invalid)?,
                digest: serde_json::from_value(record["before"]["digest"].clone())
                    .map_err(|_| invalid())?,
            })
        };
        if before_image == after
            || (before_image.is_some() && after.as_ref().is_some_and(|v| v.kind != 1))
        {
            return Err(invalid());
        }
        let image = |v: &Option<Image>| {
            v.as_ref().map(|v| serde_json::json!({"kind":v.kind,"executable":v.executable,"size":v.size,"digest":v.digest}))
        };
        serde_json::to_writer(&mut changes, &(path, image(&before_image), image(&after)))
            .map_err(|_| invalid())?;
        if collect {
            items.push(record.clone());
        }
        if record["before"].is_null() {
            if !record["access"].is_null() || record["after"].is_null() {
                return Err(invalid());
            }
            continue;
        }
        let observed = &record["before"]["observation"];
        let size = observed["length"]
            .as_u64()
            .filter(|v| *v <= 4 * 1024 * 1024 * 1024)
            .ok_or_else(invalid)?;
        let attrs = observed["attributes"].as_u64().ok_or_else(invalid)?;
        if attrs & (0x10 | 0x400) != 0 || observed["links"] != 1 {
            return Err(invalid());
        }
        let access: SavedAccess =
            serde_json::from_value(record["access"].clone()).map_err(|_| invalid())?;
        if observed["access"].as_str() != Some(access.validated_fingerprint()?.as_str()) {
            return Err(invalid());
        }
        let expected_digest: [u8; 32] =
            serde_json::from_value(record["before"]["digest"].clone()).map_err(|_| invalid())?;
        let mut remaining = size;
        let mut hash = Sha256::new();
        while remaining != 0 {
            manager.check_application_preparation()?;
            let count = remaining.min(buffer.len() as u64) as usize;
            input
                .read_exact(&mut buffer[..count])
                .map_err(|_| invalid())?;
            hash.update(&buffer[..count]);
            remaining -= count as u64;
        }
        let actual: [u8; 32] = hash.finalize().into();
        if actual != expected_digest {
            return Err(invalid());
        }
    }
    let changes_digest: [u8; 32] = changes.hash.finalize().into();
    if serde_json::to_value(changes_digest).map_err(|_| invalid())? != header["changesDigest"] {
        return Err(invalid());
    }
    if input.read(&mut [0u8; 1]).map_err(|_| invalid())? != 0
        || input.length != before.length
        || file_observation(&input.file)? != before
        || file_observation(&open_real_file_at(directory, name)?)? != before
    {
        return Err(invalid());
    }
    let backup = Backup {
        version: FORMAT_VERSION,
        name: name.into(),
        length: input.length,
        digest: input.hash.finalize().into(),
    };
    if expected.is_some_and(|v| {
        v.name != backup.name || v.length != backup.length || v.digest != backup.digest
    }) {
        return Err(invalid());
    }
    Ok((backup, header, items))
}
fn verify(manager: &ProjectManager, backup: &Backup) -> Result<Value, ProjectFailure> {
    let guard = BudgetGuard::acquire(manager)?;
    Ok(read_archive(
        manager,
        &guard.directory,
        &backup.name,
        Some(backup),
        None,
        false,
    )?
    .1)
}
fn recover_candidate(
    manager: &ProjectManager,
    name: &str,
    baseline: &SourceInventory,
) -> Result<(Backup, Value), ProjectFailure> {
    let guard = BudgetGuard::acquire(manager)?;
    let (backup, header, _) =
        read_archive(manager, &guard.directory, name, None, Some(baseline), false)?;
    Ok((backup, header))
}

pub(in crate::project) fn verify_items(
    manager: &ProjectManager,
    backup: &Backup,
) -> Result<(Value, Vec<Value>), ProjectFailure> {
    let guard = BudgetGuard::acquire(manager)?;
    let (_, header, items) = read_archive(
        manager,
        &guard.directory,
        &backup.name,
        Some(backup),
        None,
        true,
    )?;
    Ok((header, items))
}
