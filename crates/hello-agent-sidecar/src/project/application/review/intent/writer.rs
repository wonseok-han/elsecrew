//! Internal macOS writer candidate. Not reachable from product IPC until the
//! complete apply/restore flow and independent safety review are finished.
//! External editors must be stopped. On uncertainty preserve both names and lock.
use super::*;
use crate::project::application::{open, stamp};
use crate::project::{open_absolute_directory_capability, sync_capability_directory};
use cap_std::fs::{Dir, File, MetadataExt};
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
mod directories;
mod relocation;

fn parent(root: &Dir, path: &str) -> Result<(Dir, String), ProjectFailure> {
    let mut parts: Vec<_> = path.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || matches!(*p, "." | ".." | ".git"))
    {
        return Err(invalid());
    }
    let leaf = parts.pop().ok_or_else(invalid)?.to_owned();
    let mut dir = root.try_clone().map_err(|_| invalid())?;
    for part in parts {
        dir = Dir::from_std_file(open(&dir, part, true)?.into_std());
    }
    Ok((dir, leaf))
}

fn image(dir: &Dir, name: &str) -> Result<Value, ProjectFailure> {
    let meta = match dir.symlink_metadata(name) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(_) => return Err(invalid()),
    };
    if meta.is_dir() {
        let opened = open(dir, name, true)?;
        if stamp(&meta) != stamp(&opened.metadata().map_err(|_| invalid())?) {
            return Err(invalid());
        }
        // Child creation changes directory link counts and timestamps. Its
        // identity is stable; exact child membership is checked separately.
        return Ok(
            json!({"kind":2,"mode":meta.mode(),"size":0,"digest":vec![0u8;32],"identity":[meta.dev(),meta.ino(),0]}),
        );
    }
    if meta.file_type().is_symlink() && meta.nlink() == 1 {
        let bytes = link_bytes(dir, name)?;
        if stamp(&meta) != stamp(&dir.symlink_metadata(name).map_err(|_| invalid())?) {
            return Err(invalid());
        }
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        return Ok(
            json!({"kind":3,"mode":meta.mode(),"size":bytes.len(),"digest":digest,"identity":[meta.dev(),meta.ino(),meta.nlink()]}),
        );
    }
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > super::super::super::MAX_BYTES {
        return Err(invalid());
    }
    let mut file = open(dir, name, false)?;
    if stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?) {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .filter(|v| *v <= meta.len())
            .ok_or_else(invalid)?;
        hash.update(&buffer[..n]);
    }
    if count != meta.len()
        || stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?)
        || stamp(&meta) != stamp(&dir.symlink_metadata(name).map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    let digest: [u8; 32] = hash.finalize().into();
    Ok(
        json!({"kind":1,"mode":meta.mode(),"size":count,"digest":digest,"identity":[meta.dev(),meta.ino(),meta.nlink()]}),
    )
}

fn link_bytes(dir: &Dir, name: &str) -> Result<Vec<u8>, ProjectFailure> {
    let name = CString::new(name).map_err(|_| invalid())?;
    let mut bytes = vec![0u8; 4097];
    let n = unsafe {
        libc::readlinkat(
            dir.as_raw_fd(),
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    if n <= 0 || n as usize >= bytes.len() {
        return Err(invalid());
    }
    bytes.truncate(n as usize);
    Ok(bytes)
}

fn matches_after(actual: &Value, expected: &Value) -> bool {
    if actual.is_null() || expected.is_null() {
        return actual == expected;
    }
    actual["kind"] == expected["kind"]
        && actual["size"] == expected["size"]
        && actual["digest"] == expected["digest"]
        && actual["mode"]
            .as_u64()
            .map(|m| actual["kind"] == 1 && m & 0o100 != 0)
            == expected["executable"].as_bool()
}

pub(super) fn verify_unwritten_item(
    manager: &ProjectManager,
    record: &Value,
    item: &Value,
) -> Result<(), ProjectFailure> {
    let binding = &record["binding"];
    let source = manager.open_source_repository(Path::new(
        binding["canonicalGitRoot"].as_str().ok_or_else(invalid)?,
    ))?;
    if binding["sourceIdentity"] != source.source_identity
        || binding["baseCommit"] != source.base_commit
        || binding["baseTree"] != source.base_tree
        || binding["objectFormat"] != source.object_format
    {
        return Err(invalid());
    }
    let path = item["path"].as_str().ok_or_else(invalid)?;
    let mut parts: Vec<_> = path.split('/').collect();
    if parts.iter().any(|part| {
        part.is_empty() || matches!(*part, "." | "..") || part.eq_ignore_ascii_case(".git")
    }) {
        return Err(invalid());
    }
    let leaf = parts.pop().ok_or_else(invalid)?;
    let mut dir = source._root.try_clone().map_err(|_| invalid())?;
    for part in parts {
        match dir.symlink_metadata(part) {
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && item["before"].is_null() =>
            {
                return Ok(())
            }
            Ok(meta) if meta.is_dir() => {
                dir = Dir::from_std_file(open(&dir, part, true)?.into_std());
            }
            _ => return Err(invalid()),
        }
    }
    if image(&dir, leaf)? != item["before"] {
        return Err(invalid());
    }
    Ok(())
}

// A persisted name is still data: never accept paths or unknown journal shapes.
fn journal(record: &Value) -> Result<&Value, ProjectFailure> {
    validate_journal(&record["pendingItem"]["writeJournal"])
}

fn validate_journal(value: &Value) -> Result<&Value, ProjectFailure> {
    let name = value["retainedName"].as_str().ok_or_else(invalid)?;
    let suffix = name
        .strip_prefix(".hello-agent-apply-")
        .ok_or_else(invalid)?;
    let canceled = value.get("canceledStaging");
    if value
        .as_object()
        .is_none_or(|v| v.len() != 6 + usize::from(canceled.is_some()))
        || canceled.is_some_and(|v| v.as_bool() != Some(true) || value["phase"] != "restored")
        || value["version"] != 1
        || suffix.len() != 32
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || !matches!(
            value["phase"].as_str(),
            Some("reserved" | "staged" | "applied" | "restoring" | "restored")
        )
    {
        return Err(invalid());
    }
    for key in ["rootIdentity", "parentIdentity"] {
        let pair = value[key].as_array().ok_or_else(invalid)?;
        if pair.len() != 2 || pair.iter().any(|v| v.as_u64().is_none()) {
            return Err(invalid());
        }
    }
    Ok(value)
}

fn bound_parent(manager: &ProjectManager, record: &Value) -> Result<(Dir, String), ProjectFailure> {
    bound_parent_in_scope(manager, record, record)
}

fn bound_parent_in_scope(
    manager: &ProjectManager,
    record: &Value,
    scope: &Value,
) -> Result<(Dir, String), ProjectFailure> {
    let journal = journal(record)?;
    let binding = &record["binding"];
    let source = manager.open_source_repository(Path::new(
        binding["canonicalGitRoot"].as_str().ok_or_else(invalid)?,
    ))?;
    if binding["sourceIdentity"] != source.source_identity
        || binding["baseCommit"] != source.base_commit
        || binding["baseTree"] != source.base_tree
        || binding["objectFormat"] != source.object_format
    {
        return Err(invalid());
    }
    let root = source._root.dir_metadata().map_err(|_| invalid())?;
    let (dir, leaf) = parent(
        &source._root,
        &relocation::path(
            scope,
            record["pendingItem"]["item"]["path"]
                .as_str()
                .ok_or_else(invalid)?,
        )?,
    )?;
    let meta = dir.dir_metadata().map_err(|_| invalid())?;
    if journal["rootIdentity"] != json!([root.dev(), root.ino()])
        || journal["parentIdentity"] != json!([meta.dev(), meta.ino()])
    {
        return Err(invalid());
    }
    Ok((dir, leaf))
}

fn pair_matches(
    dir: &Dir,
    leaf: &str,
    record: &Value,
    applied: bool,
) -> Result<bool, ProjectFailure> {
    let j = journal(record)?;
    let old = &record["pendingItem"]["item"]["before"];
    let new = &j["stagedImage"];
    if j["canceledStaging"] == true {
        if applied {
            return Ok(false);
        }
    } else if !matches_after(new, &record["pendingItem"]["item"]["after"]) {
        return Err(invalid());
    }
    let (target, saved) = if applied { (new, old) } else { (old, new) };
    Ok(image(dir, leaf)? == *target
        && image(dir, j["retainedName"].as_str().ok_or_else(invalid)?)? == *saved)
}

pub(super) fn verify_record_pair(
    manager: &ProjectManager,
    record: &Value,
    applied: bool,
) -> Result<(), ProjectFailure> {
    verify_record_pair_in_scope(manager, record, record, applied)
}

pub(super) fn verify_record_pair_in_scope(
    manager: &ProjectManager,
    record: &Value,
    scope: &Value,
    applied: bool,
) -> Result<(), ProjectFailure> {
    let j = journal(record)?;
    if j["phase"] != if applied { "applied" } else { "restored" } {
        return Err(invalid());
    }
    let (dir, leaf) = bound_parent_in_scope(manager, record, scope)?;
    if !pair_matches(&dir, &leaf, record, applied)? {
        return Err(invalid());
    }
    directories::verify(&dir, &leaf, record, scope, applied, !applied)?;
    sync_capability_directory(&dir)?;
    Ok(())
}

/// Read-only restart diagnosis. Does not turn matching bytes into execution permission.
#[allow(dead_code)]
pub(in crate::project) fn inspect_regular_outcome(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
) -> Result<Value, ProjectFailure> {
    let current = inspect_pending_item(manager, storage, project_id)?;
    let j = journal(&current)?;
    let (dir, leaf) = bound_parent(manager, &current)?;
    let comparison = if j["phase"] == "reserved" {
        "incompleteStaging"
    } else if pair_matches(&dir, &leaf, &current, true)? {
        "matchesAppliedPair"
    } else if pair_matches(&dir, &leaf, &current, false)? {
        "matchesUnappliedPair"
    } else {
        "conflict"
    };
    Ok(
        json!({"applicationId":current["applicationId"],"index":current["pendingItem"]["index"],"phase":j["phase"],"comparison":comparison}),
    )
}

/// Cancel an interrupted copy before its durable staged fence. Preserve whatever
/// bounded regular/link image exists; it is evidence, never a valid after image.
pub(super) fn cancel_reserved_staging(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut j = journal(expected)?.clone();
    if j["phase"] != "reserved" || !j["stagedImage"].is_null() {
        return Err(invalid());
    }
    let (dir, leaf) = bound_parent(manager, expected)?;
    if image(&dir, &leaf)? != expected["pendingItem"]["item"]["before"] {
        return Err(invalid());
    }
    let saved = j["retainedName"].as_str().ok_or_else(invalid)?;
    let partial = image(&dir, saved)?;
    sync_capability_directory(&dir)?;
    let (fresh, fresh_leaf) = bound_parent(manager, expected)?;
    if image(&fresh, &fresh_leaf)? != expected["pendingItem"]["item"]["before"]
        || image(&fresh, saved)? != partial
    {
        return Err(invalid());
    }
    j["stagedImage"] = partial;
    j["canceledStaging"] = json!(true);
    j["phase"] = json!("restored");
    publish(storage, project_id, expected, j, request)
}

/// Explicit reconciliation of a staged/restoring pair. An untouched staged pair
/// is canceled as restored, keeping its staged image. Never renames or unlocks.
#[allow(dead_code)]
pub(in crate::project) fn acknowledge_regular_outcome(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut j = journal(expected)?.clone();
    let (dir, leaf) = bound_parent(manager, expected)?;
    let (applied, phase) = match j["phase"].as_str() {
        Some("staged") if pair_matches(&dir, &leaf, expected, true)? => (true, "applied"),
        Some("staged") => (false, "restored"),
        Some("restoring") => (false, "restored"),
        _ => return Err(invalid()),
    };
    if !pair_matches(&dir, &leaf, expected, applied)? {
        return Err(invalid());
    }
    sync_capability_directory(&dir)?;
    let (fresh, _) = bound_parent(manager, expected)?;
    if !pair_matches(&fresh, &leaf, expected, applied)? {
        return Err(invalid());
    }
    j["phase"] = json!(phase);
    publish(storage, project_id, expected, j, request)
}

/// Restore only the app-owned after image. A later edit/replacement is a
/// conflict; both old and new contents remain available for manual resolution.
#[allow(dead_code)]
pub(in crate::project) fn restore_first_regular_item(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut j = journal(expected)?.clone();
    if !matches!(j["phase"].as_str(), Some("applied" | "restoring")) {
        return Err(invalid());
    }
    let (dir, leaf) = bound_parent(manager, expected)?;
    if !pair_matches(&dir, &leaf, expected, true)? {
        return Err(invalid());
    }
    directories::verify(&dir, &leaf, expected, expected, true, true)?;
    let current = if j["phase"] == "applied" {
        j["phase"] = json!("restoring");
        publish(storage, project_id, expected, j.clone(), request)?
    } else {
        // Explicit recovery may retry only the exact still-applied pair. Keep
        // the original durable restoring intent rather than posting it twice.
        expected.clone()
    };
    // Reopen the path after durable intent; don't write to a detached old parent.
    let (dir, leaf) = bound_parent(manager, &current)?;
    if !pair_matches(&dir, &leaf, &current, true)? {
        return Err(invalid());
    }
    directories::verify(&dir, &leaf, &current, &current, true, true)?;
    let target = CString::new(leaf.as_str()).map_err(|_| invalid())?;
    let saved =
        CString::new(j["retainedName"].as_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
    let item = &current["pendingItem"]["item"];
    let status = unsafe {
        if item["before"].is_null() {
            libc::renameatx_np(
                dir.as_raw_fd(),
                target.as_ptr(),
                dir.as_raw_fd(),
                saved.as_ptr(),
                libc::RENAME_EXCL,
            )
        } else {
            libc::renameatx_np(
                dir.as_raw_fd(),
                saved.as_ptr(),
                dir.as_raw_fd(),
                target.as_ptr(),
                if item["after"].is_null() {
                    libc::RENAME_EXCL
                } else {
                    libc::RENAME_SWAP
                },
            )
        }
    };
    if status != 0 {
        return Err(invalid());
    }
    sync_capability_directory(&dir)?;
    if !pair_matches(&dir, &leaf, &current, false)? {
        return Err(invalid());
    }
    let (fresh, _) = bound_parent(manager, &current)?;
    if !pair_matches(&fresh, &leaf, &current, false)? {
        return Err(invalid());
    }
    j["phase"] = json!("restored");
    publish(storage, project_id, &current, j, request)
}

fn publish(
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    journal: Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if expected["state"] != "applying" || inspect(storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut project = required_project(storage, project_id)?;
    let revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let lock_id = expected["lockId"].as_str().ok_or_else(invalid)?;
    let lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let lock_revision = lock["last_event_sequence"].as_u64().ok_or_else(invalid)?;
    let phase = journal["phase"].as_str().ok_or_else(invalid)?.to_owned();
    let mut next = expected.clone();
    next["pendingItem"]["writeJournal"] = journal;
    project.insert("application_intent".into(), next.clone());
    storage.upsert_projection_records_atomic_checked(
        vec![crate::storage::ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!(
                "op_{}_item_{}_{phase}",
                expected["applicationId"].as_str().ok_or_else(invalid)?,
                expected["pendingItem"]["index"]
                    .as_u64()
                    .ok_or_else(invalid)?
            ),
            request_id: Some(request.to_owned()),
            payload: project,
        }],
        &[
            ("project", project_id, revision),
            ("lock", lock_id, lock_revision),
        ],
    )?;
    Ok(next)
}

/// One-shot execution only. Existing journals require explicit recovery, never
/// a blind retry. A failed CAS after filesystem mutation keeps the staged journal.
#[allow(dead_code)]
pub(in crate::project) fn write_first_regular_item(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    write_item_cancellable(
        manager,
        storage,
        project_id,
        expected,
        request,
        &AtomicBool::new(false),
    )
}

pub(super) fn write_item_cancellable(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    canceled: &AtomicBool,
) -> Result<Value, ProjectFailure> {
    check_canceled(canceled)?;
    if inspect_pending_item(manager, storage, project_id)? != *expected
        || expected["pendingItem"].get("writeJournal").is_some()
    {
        return Err(invalid());
    }
    let item = &expected["pendingItem"]["item"];
    for side in ["before", "after"] {
        if !item[side].is_null() && !matches!(item[side]["kind"].as_u64(), Some(1..=3)) {
            return Err(invalid());
        }
    }
    if item["before"]["kind"] == 2 || (item["after"]["kind"] == 2 && !item["before"].is_null()) {
        return Err(invalid());
    }
    if item["before"].is_null() && item["after"].is_null() {
        return Err(invalid());
    }
    let observation = observe_pending_source(manager, storage, project_id)?;
    if observation["comparison"] != "matchesBeforeEntry" {
        return Err(invalid());
    }
    let root_path = Path::new(
        expected["binding"]["canonicalGitRoot"]
            .as_str()
            .ok_or_else(invalid)?,
    );
    let root = open_absolute_directory_capability(root_path)?;
    let root_identity = root.dir_metadata().map_err(|_| invalid())?;
    if observation["rootIdentity"] != json!([root_identity.dev(), root_identity.ino()]) {
        return Err(invalid());
    }
    let path = item["path"].as_str().ok_or_else(invalid)?;
    let (dir, leaf) = parent(&root, path)?;
    let parent_identity = dir.dir_metadata().map_err(|_| invalid())?;
    let retained = format!(".hello-agent-apply-{}", Uuid::new_v4().simple());
    let mut journal = json!({"version":1,"phase":"reserved","retainedName":retained,
        "rootIdentity":[root_identity.dev(),root_identity.ino()],
        "parentIdentity":[parent_identity.dev(),parent_identity.ino()],"stagedImage":null});
    let mut current = publish(storage, project_id, expected, journal.clone(), request)?;
    if !item["after"].is_null() {
        let snapshot = open_absolute_directory_capability(Path::new(
            expected["worktreePath"].as_str().ok_or_else(invalid)?,
        ))?;
        let (input_dir, input_leaf) = parent(&snapshot, path)?;
        let input_image = image(&input_dir, &input_leaf)?;
        if !matches_after(&input_image, &item["after"]) {
            return Err(invalid());
        }
        if item["after"]["kind"] == 2 {
            dir.create_dir(&retained).map_err(|_| invalid())?;
            let staged_dir = open(&dir, &retained, true)?;
            if unsafe { libc::fchmod(staged_dir.as_raw_fd(), 0o755) } != 0 {
                return Err(invalid());
            }
            staged_dir.sync_all().map_err(|_| invalid())?;
        } else if item["after"]["kind"] == 3 {
            let bytes = link_bytes(&input_dir, &input_leaf)?;
            let target = CString::new(bytes).map_err(|_| invalid())?;
            let name = CString::new(retained.as_str()).map_err(|_| invalid())?;
            // symlinkat creates a new directory entry and never follows its target.
            if unsafe { libc::symlinkat(target.as_ptr(), dir.as_raw_fd(), name.as_ptr()) } != 0 {
                return Err(invalid());
            }
        } else {
            let mut input = open(&input_dir, &input_leaf, false)?;
            let name = CString::new(retained.as_str()).map_err(|_| invalid())?;
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(invalid());
            }
            let mut output = File::from_std(unsafe { std::fs::File::from_raw_fd(fd) });
            let size = item["after"]["size"].as_u64().ok_or_else(invalid)?;
            let copied = copy_cancellable(&mut (&mut input).take(size + 1), &mut output, canceled)?;
            let mode = if item["before"].is_null() {
                0o644
            } else {
                item["before"]["mode"].as_u64().ok_or_else(invalid)? & 0o777
            };
            let mode = if item["after"]["executable"] == true {
                mode | 0o100
            } else {
                mode & !0o100
            };
            if copied != size
                || unsafe { libc::fchmod(output.as_raw_fd(), mode as libc::mode_t) } != 0
            {
                return Err(invalid());
            }
            output.flush().map_err(|_| invalid())?;
            output.sync_all().map_err(|_| invalid())?;
        }
        let staged = image(&dir, &retained)?;
        if !matches_after(&staged, &item["after"]) || image(&input_dir, &input_leaf)? != input_image
        {
            return Err(invalid());
        }
        journal["stagedImage"] = staged;
    }
    sync_capability_directory(&dir)?;
    check_canceled(canceled)?;
    journal["phase"] = json!("staged");
    current = publish(storage, project_id, &current, journal.clone(), request)?;
    let reopened = open_absolute_directory_capability(root_path)?;
    let (linked_parent, _) = parent(&reopened, path)?;
    let linked_root = reopened.dir_metadata().map_err(|_| invalid())?;
    let linked = linked_parent.dir_metadata().map_err(|_| invalid())?;
    if (linked_root.dev(), linked_root.ino()) != (root_identity.dev(), root_identity.ino())
        || (linked.dev(), linked.ino()) != (parent_identity.dev(), parent_identity.ino())
        || image(&dir, &leaf)? != item["before"]
        || image(&dir, &retained)? != journal["stagedImage"]
    {
        return Err(invalid());
    }
    let target = CString::new(leaf.as_str()).map_err(|_| invalid())?;
    let saved = CString::new(retained.as_str()).map_err(|_| invalid())?;
    directories::verify(&dir, &leaf, &current, &current, false, false)?;
    check_canceled(canceled)?;
    let status = unsafe {
        if item["after"].is_null() {
            libc::renameatx_np(
                dir.as_raw_fd(),
                target.as_ptr(),
                dir.as_raw_fd(),
                saved.as_ptr(),
                libc::RENAME_EXCL,
            )
        } else {
            libc::renameatx_np(
                dir.as_raw_fd(),
                saved.as_ptr(),
                dir.as_raw_fd(),
                target.as_ptr(),
                if item["before"].is_null() {
                    libc::RENAME_EXCL
                } else {
                    libc::RENAME_SWAP
                },
            )
        }
    };
    if status != 0 {
        return Err(invalid());
    }
    sync_capability_directory(&dir)?;
    // A post-rename conflict never triggers destructive cleanup or automatic undo.
    let after = image(&dir, &leaf)?;
    let before = image(&dir, &retained)?;
    if after != journal["stagedImage"] || before != item["before"] {
        return Err(invalid());
    }
    let check = parent(&open_absolute_directory_capability(root_path)?, path)?
        .0
        .dir_metadata()
        .map_err(|_| invalid())?;
    if (check.dev(), check.ino()) != (parent_identity.dev(), parent_identity.ino()) {
        return Err(invalid());
    }
    journal["phase"] = json!("applied");
    publish(storage, project_id, &current, journal, request)
}

fn check_canceled(canceled: &AtomicBool) -> Result<(), ProjectFailure> {
    if canceled.load(Ordering::Acquire) {
        return Err(ProjectFailure::recovery(
            "application canceled; preserve staging for explicit recovery",
        ));
    }
    Ok(())
}

fn copy_cancellable(
    input: &mut impl Read,
    output: &mut impl Write,
    canceled: &AtomicBool,
) -> Result<u64, ProjectFailure> {
    let mut buffer = [0u8; 65536];
    let mut count = 0u64;
    loop {
        check_canceled(canceled)?;
        let n = match input.read(&mut buffer) {
            Ok(n) => n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(invalid()),
        };
        check_canceled(canceled)?;
        if n == 0 {
            return Ok(count);
        }
        output.write_all(&buffer[..n]).map_err(|_| invalid())?;
        count = count.checked_add(n as u64).ok_or_else(invalid)?;
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[test]
    fn cancellation_after_read_never_writes_that_chunk() {
        struct CancelingReader<'a>(&'a AtomicBool);
        impl Read for CancelingReader<'_> {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                bytes[0] = 42;
                self.0.store(true, Ordering::Release);
                Ok(1)
            }
        }
        let canceled = AtomicBool::new(false);
        let mut output = Vec::new();
        assert!(copy_cancellable(&mut CancelingReader(&canceled), &mut output, &canceled).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn normal_copy_keeps_exact_bytes_across_chunks() {
        let bytes = vec![13u8; 131_073];
        let mut output = Vec::new();
        assert_eq!(
            copy_cancellable(&mut bytes.as_slice(), &mut output, &AtomicBool::new(false)).unwrap(),
            bytes.len() as u64
        );
        assert_eq!(output, bytes);
    }
}
