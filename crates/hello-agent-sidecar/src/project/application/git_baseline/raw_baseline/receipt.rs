//! Immutable local receipt candidate. Its descriptor still needs durable ledger binding.
use super::SourceBaseline;
use crate::project::application::{open, stamp};
use crate::project::{
    canonical_protocol_path_without_existing, create_managed_directory, snapshot_identity,
    sync_capability_directory, ActiveProject, ProjectFailure, ProjectManager,
};
use crate::storage::Storage;
use cap_std::fs::{Dir, File, MetadataExt};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};

const RECEIPT_MAX_BYTES: usize = 64 * 1024 * 1024;

// Publication is only legal during a durably fenced snapshot preparation.
// The descriptor is produced here, never accepted from a renderer or a disk scan.
#[allow(dead_code)]
pub(in crate::project) fn persist_and_bind(
    manager: &ProjectManager,
    storage: &mut Storage,
    active: &ActiveProject,
    baseline: &SourceBaseline,
    request_id: &str,
) -> Result<(), ProjectFailure> {
    let mut project = crate::project::required_project(storage, &active.project_id)?;
    let root = canonical_protocol_path_without_existing(&active.canonical_git_root)?;
    let workspace = canonical_protocol_path_without_existing(&active.worktree_path)?;
    for (key, expected) in [
        ("canonical_path", root.as_str()),
        ("source_identity", active.source_identity.as_str()),
        ("base_commit", active.base_commit.as_str()),
        ("base_tree", active.base_tree.as_str()),
        ("object_format", active.object_format.as_str()),
        ("worktree_path", workspace.as_str()),
    ] {
        if project.get(key).and_then(serde_json::Value::as_str) != Some(expected) {
            return Err(invalid());
        }
    }
    let lock = storage
        .projection_record("lock", &active.lock_id)?
        .ok_or_else(invalid)?;
    if project.get("lifecycle").and_then(serde_json::Value::as_str) != Some("recoveryRequired")
        || project
            .get("worktree_id")
            .and_then(serde_json::Value::as_str)
            != Some(&active.worktree_id)
        || project
            .get("workspace_identity")
            .and_then(serde_json::Value::as_str)
            != Some(&active.workspace_identity)
        || project.get("lock_id").and_then(serde_json::Value::as_str) != Some(&active.lock_id)
        || lock.get("recovery_required") != Some(&json!(true))
        || lock.get("project_id").and_then(serde_json::Value::as_str) != Some(&active.project_id)
        || lock
            .get("worktree_path")
            .and_then(serde_json::Value::as_str)
            != Some(workspace.as_str())
        || lock.get("writer_run_id").is_none_or(|v| !v.is_null())
    {
        return Err(invalid());
    }
    let mut records = match project.remove("source_baselines") {
        None => serde_json::Map::new(),
        Some(serde_json::Value::Object(records)) => records,
        _ => return Err(invalid()),
    };
    if records.contains_key(&active.worktree_id) {
        return Err(invalid());
    }
    let store = ReceiptStore::open(manager)?;
    let receipt = persist(&store, active, baseline)?;
    records.insert(
        active.worktree_id.clone(),
        json!({
            "binding": snapshot_identity(active)?, "receipt": receipt.to_value(),
        }),
    );
    project.insert(
        "source_baselines".into(),
        serde_json::Value::Object(records),
    );
    crate::project::append_object(
        storage,
        "project",
        &active.project_id,
        &format!("op_source_baseline_{}", active.worktree_id),
        request_id,
        serde_json::Value::Object(project),
    )
}

#[allow(dead_code)]
pub(in crate::project) fn verify_bound(
    manager: &ProjectManager,
    storage: &Storage,
    active: &ActiveProject,
    current: &SourceBaseline,
) -> Result<(), ProjectFailure> {
    let project = crate::project::required_project(storage, &active.project_id)?;
    let record = project
        .get("source_baselines")
        .and_then(|v| v.get(&active.worktree_id))
        .ok_or_else(invalid)?;
    if record.as_object().is_none_or(|v| v.len() != 2)
        || record.get("binding") != Some(&snapshot_identity(active)?)
    {
        return Err(invalid());
    }
    let receipt = BaselineReceipt::from_value(record.get("receipt").ok_or_else(invalid)?)?;
    verify(
        &ReceiptStore::open(manager)?,
        active,
        current,
        Some(&receipt),
    )
}

pub(in crate::project) struct ReceiptStore {
    directory: Dir,
}

impl ReceiptStore {
    #[allow(dead_code)]
    pub(in crate::project) fn open(manager: &ProjectManager) -> Result<Self, ProjectFailure> {
        manager.verify_managed_paths()?;
        let (_, directory) =
            create_managed_directory(&manager.data_root, &manager.data_dir, "source-baselines-v1")?;
        Ok(Self { directory })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::project) struct BaselineReceipt {
    digest: String,
    length: u64,
}

impl BaselineReceipt {
    #[allow(dead_code)]
    pub(in crate::project) fn to_value(&self) -> serde_json::Value {
        json!({"schemaVersion":1,"digest":self.digest,"length":self.length})
    }

    #[allow(dead_code)]
    pub(in crate::project) fn from_value(
        value: &serde_json::Value,
    ) -> Result<Self, ProjectFailure> {
        let object = value.as_object().ok_or_else(invalid)?;
        let digest = object
            .get("digest")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(invalid)?;
        let length = object
            .get("length")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(invalid)?;
        let hex = digest.strip_prefix("sha256:").ok_or_else(invalid)?;
        if object.len() != 3
            || object.get("schemaVersion") != Some(&json!(1))
            || hex.len() != 64
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || length == 0
            || length > RECEIPT_MAX_BYTES as u64
        {
            return Err(invalid());
        }
        Ok(Self {
            digest: digest.to_owned(),
            length,
        })
    }
}

fn invalid() -> ProjectFailure {
    ProjectFailure::recovery("source baseline receipt is missing, changed, or not durably bound")
}

fn filename(active: &ActiveProject) -> Result<String, ProjectFailure> {
    if active.worktree_id.is_empty()
        || active.worktree_id.len() > 128
        || !active
            .worktree_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(invalid());
    }
    Ok(format!("{}.source-baseline-v1.json", active.worktree_id))
}

struct LimitedBytes(Vec<u8>);
impl Write for LimitedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > RECEIPT_MAX_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("source receipt exceeds 64 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encode(active: &ActiveProject, baseline: &SourceBaseline) -> Result<Vec<u8>, ProjectFailure> {
    if baseline.source_root != canonical_protocol_path_without_existing(&active.canonical_git_root)?
        || baseline.source_identity != active.source_identity
        || baseline.object_format != active.object_format
        || baseline.index.commit != active.base_commit
        || baseline.index.tree != active.base_tree
    {
        return Err(ProjectFailure::manifest(
            "source baseline belongs to a different snapshot source",
        ));
    }
    let mut output = LimitedBytes(Vec::new());
    output.write_all(b"{\"binding\":").map_err(|_| invalid())?;
    serde_json::to_writer(&mut output, &snapshot_identity(active)?).map_err(|_| invalid())?;
    output
        .write_all(b",\"indexDigest\":")
        .map_err(|_| invalid())?;
    serde_json::to_writer(
        &mut output,
        &baseline
            .index
            .index_digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    )
    .map_err(|_| invalid())?;
    output
        .write_all(b",\"indexEntries\":[")
        .map_err(|_| invalid())?;
    for (i, (path, e)) in baseline.index.entries.iter().enumerate() {
        if i != 0 {
            output.write_all(b",").map_err(|_| invalid())?;
        }
        serde_json::to_writer(&mut output, &(path, &e.mode, &e.oid)).map_err(|_| invalid())?;
    }
    output
        .write_all(b"],\"rawEntries\":[")
        .map_err(|_| invalid())?;
    for (i, (path, e)) in baseline.raw.entries.iter().enumerate() {
        if i != 0 {
            output.write_all(b",").map_err(|_| invalid())?;
        }
        serde_json::to_writer(
            &mut output,
            &json!([
                path,
                e.kind,
                e.mode,
                e.size.to_string(),
                [
                    e.identity.0.to_string(),
                    e.identity.1.to_string(),
                    e.identity.2.to_string()
                ],
                e.digest
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ]),
        )
        .map_err(|_| invalid())?;
    }
    output
        .write_all(b"],\"rootStamp\":")
        .map_err(|_| invalid())?;
    serde_json::to_writer(&mut output, &baseline.raw.root_stamp.map(|v| v.to_string()))
        .map_err(|_| invalid())?;
    output
        .write_all(b",\"schemaVersion\":1}")
        .map_err(|_| invalid())?;
    Ok(output.0)
}

#[allow(dead_code)]
pub(in crate::project) fn persist(
    store: &ReceiptStore,
    active: &ActiveProject,
    baseline: &SourceBaseline,
) -> Result<BaselineReceipt, ProjectFailure> {
    let root = &store.directory;
    let name = filename(active)?;
    let bytes = encode(active, baseline)?;
    let receipt = BaselineReceipt {
        digest: format!("sha256:{:x}", Sha256::digest(&bytes)),
        length: bytes.len() as u64,
    };
    let name_c = CString::new(name.as_bytes()).map_err(|_| invalid())?;
    // Exclusive creation deliberately preserves partial/previous receipts. No replacement.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(invalid());
    }
    let mut file = File::from_std(unsafe { std::fs::File::from_raw_fd(fd) });
    file.write_all(&bytes).map_err(|_| invalid())?;
    file.sync_all().map_err(|_| invalid())?;
    let meta = file.metadata().map_err(|_| invalid())?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.mode() & 0o777 != 0o600
        || stamp(&meta) != stamp(&root.symlink_metadata(&name).map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    sync_capability_directory(root)?;
    verify(store, active, baseline, Some(&receipt))?;
    Ok(receipt)
}

#[allow(dead_code)]
pub(in crate::project) fn verify(
    store: &ReceiptStore,
    active: &ActiveProject,
    current: &SourceBaseline,
    recorded: Option<&BaselineReceipt>,
) -> Result<(), ProjectFailure> {
    let root = &store.directory;
    // A file appearing on disk is not evidence that an old snapshot had a baseline.
    let recorded = recorded.ok_or_else(invalid)?;
    if recorded.length > RECEIPT_MAX_BYTES as u64 {
        return Err(invalid());
    }
    let name = filename(active)?;
    let mut file = open(root, &name, false)?;
    let before = file.metadata().map_err(|_| invalid())?;
    if !before.is_file()
        || before.nlink() != 1
        || before.mode() & 0o777 != 0o600
        || before.len() != recorded.length
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(recorded.length + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 != recorded.length
        || stamp(&before) != stamp(&file.metadata().map_err(|_| invalid())?)
        || stamp(&before) != stamp(&root.symlink_metadata(&name).map_err(|_| invalid())?)
        || format!("sha256:{:x}", Sha256::digest(&bytes)) != recorded.digest
        || bytes != encode(active, current)?
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_receipt_stream_stops_at_encoded_byte_limit() {
        // Maximum-length paths with escaping reach the output budget without
        // constructing a Value tree for the entire inventory.
        let path = "\\".repeat(4096);
        let mut output = LimitedBytes(Vec::new());
        let mut rejected = false;
        for _ in 0..100_000 {
            if serde_json::to_writer(&mut output, &(&path, 100644, "f".repeat(64))).is_err() {
                rejected = true;
                break;
            }
        }
        assert!(rejected);
        assert!(output.0.len() <= RECEIPT_MAX_BYTES);
        assert!(output.0.len() > RECEIPT_MAX_BYTES - 8192);
        let before = output.0.len();
        assert!(output.write_all(&[0; 8192]).is_err());
        assert_eq!(output.0.len(), before);
    }
}
