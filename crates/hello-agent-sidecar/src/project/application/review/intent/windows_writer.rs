//! Windows application moves use two non-replacing renames, never an exchange.
//! Each syscall has a durable intent and a post-flush receipt. No retry or
//! cleanup follows an uncertain result. Staging/scheduler admission is separate.
#![allow(dead_code)]
use super::*;
pub(in crate::project) mod recovery;
use crate::project::{
    open_absolute_directory_capability, open_real_child_dir, open_real_file_at,
    validate_relative_path, windows_directory_lease::DirectoryLease, windows_durability,
    windows_fs, windows_replace, windows_source_inventory::file_observation,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{
    ffi::OsStr,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

fn parent(root: &Dir, path: &str) -> Result<(Dir, String), ProjectFailure> {
    validate_relative_path(path, false)?;
    crate::project::protocol_path::from_native(&format!("C:/{path}"), true)
        .map_err(ProjectFailure::manifest)?;
    let parts: Vec<_> = path.split('/').collect();
    let (leaf, ancestors) = parts.split_last().ok_or_else(invalid)?;
    let mut dir = root.try_clone().map_err(|_| invalid())?;
    for part in ancestors {
        dir = open_real_child_dir(&dir, part)?;
    }
    Ok((dir, leaf.to_string()))
}

fn source_parent(
    manager: &ProjectManager,
    record: &Value,
) -> Result<(Dir, Dir, String), ProjectFailure> {
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
    let (dir, leaf) = parent(
        &source._root,
        record["pendingItem"]["item"]["path"]
            .as_str()
            .ok_or_else(invalid)?,
    )?;
    windows_durability::require_ntfs(&dir)?;
    Ok((source._root, dir, leaf))
}

fn validate(j: &Value) -> Result<(), ProjectFailure> {
    if j.as_object().is_none_or(|o| {
        o.len()
            != 12
                + usize::from(o.contains_key("cancelingBeforeInstall"))
                + usize::from(o.contains_key("canceledUnwritten"))
            || [
                "version",
                "platform",
                "phase",
                "nonce",
                "rootIdentity",
                "parentIdentity",
                "guardEntry",
                "stagedEntry",
                "retainedEntry",
                "installedEntry",
                "detachedEntry",
                "restoredEntry",
            ]
            .iter()
            .any(|key| !o.contains_key(*key))
    }) || j["version"] != 1
        || j["platform"] != "windows"
        || !matches!(
            j["phase"].as_str(),
            Some(
                "reserved"
                    | "staged"
                    | "moveBeforeIntent"
                    | "beforeMoved"
                    | "installIntent"
                    | "cancelStagedIntent"
                    | "cancelReady"
                    | "applied"
                    | "detachAfterIntent"
                    | "afterDetached"
                    | "restoreBeforeIntent"
                    | "restored"
            )
        )
    {
        return Err(invalid());
    }
    if let Some(unwritten) = j.get("canceledUnwritten") {
        if unwritten != &json!(true)
            || j["phase"] != "restored"
            || j.get("cancelingBeforeInstall").is_some()
        {
            return Err(invalid());
        }
    }
    if let Some(canceling) = j.get("cancelingBeforeInstall") {
        if canceling != &json!(true)
            || !matches!(
                j["phase"].as_str(),
                Some(
                    "beforeMoved"
                        | "cancelReady"
                        | "cancelStagedIntent"
                        | "afterDetached"
                        | "restoreBeforeIntent"
                        | "restored"
                )
            )
        {
            return Err(invalid());
        }
    }
    if matches!(
        j["phase"].as_str(),
        Some("cancelStagedIntent" | "cancelReady")
    ) && j["cancelingBeforeInstall"] != true
    {
        return Err(invalid());
    }
    let nonce = j["nonce"].as_str().ok_or_else(invalid)?;
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    // Identities are compared to fresh held handles before any operation.
    for key in ["rootIdentity", "parentIdentity"] {
        let id = &j[key];
        if id.as_object().is_none_or(|o| o.len() != 2)
            || id["volume"].as_u64().is_none()
            || id["file"].as_array().is_none_or(|v| {
                v.len() != 16 || v.iter().any(|b| b.as_u64().is_none_or(|n| n > 255))
            })
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn name(j: &Value, suffix: &str) -> Result<String, ProjectFailure> {
    validate(j)?;
    Ok(format!(
        ".hello-agent-win-{}.{}",
        j["nonce"].as_str().ok_or_else(invalid)?,
        suffix
    ))
}

fn publish(
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    journal: Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    validate_receipts(&journal, &expected["pendingItem"]["item"])?;
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
    if serde_json::to_vec(&project).map_err(|_| invalid())?.len() > MAX_REVIEW_BYTES {
        return Err(invalid());
    }
    storage.upsert_projection_records_atomic_checked(
        vec![crate::storage::ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!(
                "op_{}_item_{}_windows_{phase}",
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

/// Reserve all private names before any staging file exists. Existing journals
/// must enter explicit recovery; they are never overwritten by a fresh attempt.
pub(in crate::project) fn reserve(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    check()?;
    if expected["state"] != "applying"
        || inspect_pending_item(manager, storage, project_id)? != *expected
        || expected["pendingItem"].get("writeJournal").is_some()
    {
        return Err(invalid());
    }
    let observed =
        super::observation::observe_pending_source_checked(manager, storage, project_id, check)?;
    if observed["comparison"] != "matchesBeforeEntry" {
        return Err(invalid());
    }
    let (root, dir, _) = source_parent(manager, expected)?;
    let root_identity = json!(windows_fs::identity(&root, true)?);
    if observed["rootIdentity"] != root_identity {
        return Err(invalid());
    }
    let journal = json!({"version":1,"platform":"windows","phase":"reserved","nonce":Uuid::new_v4().simple().to_string(),
        "rootIdentity":root_identity,"parentIdentity":windows_fs::identity(&dir,true)?,
        "guardEntry":null,"stagedEntry":null,"retainedEntry":null,"installedEntry":null,"detachedEntry":null,"restoredEntry":null});
    check()?;
    publish(storage, project_id, expected, journal, request)
}

/// Build files only after reservation. Partial files remain owned by that
/// reservation on any failure; a later call cannot silently reuse them.
pub(in crate::project) fn stage(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    check()?;
    if expected["state"] != "applying"
        || inspect_pending_item(manager, storage, project_id)? != *expected
    {
        return Err(invalid());
    }
    let mut j = expected["pendingItem"]["writeJournal"].clone();
    validate_receipts(&j, &expected["pendingItem"]["item"])?;
    if j["phase"] != "reserved" {
        return Err(invalid());
    }
    let item = &expected["pendingItem"]["item"];
    if (!item["before"].is_null()
        && item["before"]["observation"]["attributes"]
            .as_u64()
            .is_none_or(|v| v & 0x10 != 0))
        || (!item["after"].is_null() && item["after"]["kind"] != 1)
        || (item["before"].is_null() && item["after"].is_null())
    {
        return Err(invalid());
    }
    let (root, dir, _) = source_parent(manager, expected)?;
    if json!(windows_fs::identity(&root, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&dir, true)?) != j["parentIdentity"]
    {
        return Err(invalid());
    }
    let guard_name = name(&j, "guard")?;
    let mut guard = dir
        .open_with(&guard_name, &windows_replace::source_options())
        .map_err(|_| invalid())?;
    guard
        .write_all(b"hello-agent-windows-application-guard-v1\n")
        .map_err(|_| invalid())?;
    guard.sync_all().map_err(|_| invalid())?;
    let lease = DirectoryLease::acquire(&dir, OsStr::new(&guard_name))?;
    j["guardEntry"] = entry(&mut guard, check)?;
    if !item["after"].is_null() {
        let snapshot = open_absolute_directory_capability(Path::new(
            expected["worktreePath"].as_str().ok_or_else(invalid)?,
        ))?;
        let (input_parent, input_leaf) =
            parent(&snapshot, item["path"].as_str().ok_or_else(invalid)?)?;
        let mut input = open_real_file_at(&input_parent, &input_leaf)?;
        let input_before = entry(&mut input, check)?;
        let image = json!({"kind":1,"executable":false,"size":input_before["observation"]["length"],"digest":input_before["digest"]});
        if image != item["after"] {
            return Err(invalid());
        }
        input.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        let staged_name = name(&j, "staged")?;
        let mut output = dir
            .open_with(&staged_name, &windows_replace::source_options())
            .map_err(|_| invalid())?;
        let mut remaining = item["after"]["size"].as_u64().ok_or_else(invalid)?;
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            check()?;
            let limit = buffer.len().min(remaining as usize);
            let n = input.read(&mut buffer[..limit]).map_err(|_| invalid())?;
            if n == 0 {
                return Err(invalid());
            }
            output.write_all(&buffer[..n]).map_err(|_| invalid())?;
            remaining -= n as u64;
        }
        if !item["before"].is_null() {
            let access: crate::project::windows_security::SavedAccess =
                serde_json::from_value(item["access"].clone()).map_err(|_| invalid())?;
            access.restore_replacement(&output)?;
        }
        output.flush().map_err(|_| invalid())?;
        output.sync_all().map_err(|_| invalid())?;
        let staged = entry(&mut output, check)?;
        if staged["observation"]["length"] != item["after"]["size"]
            || staged["digest"] != item["after"]["digest"]
            || entry(&mut input, check)? != input_before
            || entry(&mut open_real_file_at(&input_parent, &input_leaf)?, check)? != input_before
        {
            return Err(invalid());
        }
        j["stagedEntry"] = staged;
    }
    lease.validate()?;
    check()?;
    j["phase"] = json!("staged");
    publish(storage, project_id, expected, j, request)
}

/// Observe one retained regular-file handle without trusting its pathname.
/// The caller compares the leaf separately while holding its directory lease.
fn entry(
    file: &mut File,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    let before = file_observation(file)?;
    if before.length > 4 * 1024 * 1024 * 1024 {
        return Err(invalid());
    }
    file.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
    let mut hash = Sha256::new();
    let mut remaining = before.length;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        check()?;
        let limit = buffer.len().min(remaining as usize);
        let count = file.read(&mut buffer[..limit]).map_err(|_| invalid())?;
        if count == 0 {
            return Err(invalid());
        }
        remaining -= count as u64;
        hash.update(&buffer[..count]);
    }
    if file.read(&mut buffer[..1]).map_err(|_| invalid())? != 0 || file_observation(file)? != before
    {
        return Err(invalid());
    }
    check()?;
    Ok(json!({"observation":before,"digest":<[u8;32]>::from(hash.finalize())}))
}

fn unchanged_by_move(before: &Value, after: &Value) -> bool {
    // A rename can update ChangeTime. All other captured fields, including full
    // ID, content, creation/write times, attributes and ACL must remain equal.
    let mut comparable = after.clone();
    comparable["observation"]["changed"] = before["observation"]["changed"].clone();
    comparable == *before
}

#[derive(Clone, Copy)]
pub(in crate::project) enum Move {
    PreserveBefore,
    InstallAfter,
    DetachAfter,
    RestoreBefore,
    CancelStaged,
}

/// The caller selects a fixed lifecycle step, never arbitrary source/target
/// names. Intent publication precedes rename; a post-rename error leaves that
/// intent and both private names intact for explicit recovery.
pub(in crate::project) fn move_file(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    step: Move,
    request: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    check()?;
    if expected["state"] != "applying"
        || inspect_pending_item(manager, storage, project_id)? != *expected
    {
        return Err(invalid());
    }
    let mut j = expected["pendingItem"]["writeJournal"].clone();
    validate_receipts(&j, &expected["pendingItem"]["item"])?;
    if matches!(step, Move::CancelStaged) {
        if !matches!(j["phase"].as_str(), Some("beforeMoved" | "cancelReady")) {
            return Err(invalid());
        }
        j["cancelingBeforeInstall"] = json!(true);
    } else if j["cancelingBeforeInstall"] == true && !matches!(step, Move::RestoreBefore) {
        return Err(invalid());
    }
    let (root, dir, leaf) = source_parent(manager, expected)?;
    if json!(windows_fs::identity(&root, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&dir, true)?) != j["parentIdentity"]
    {
        return Err(invalid());
    }
    let guard = name(&j, "guard")?;
    let lease = DirectoryLease::acquire(&dir, OsStr::new(&guard))?;
    if entry(&mut open_real_file_at(&dir, &guard)?, check)? != j["guardEntry"] {
        return Err(invalid());
    }
    let (prior, intent, complete, from, to, expected_entry, receipt) =
        transition(&j, &expected["pendingItem"]["item"], &leaf, step)?;
    if j["phase"] != prior {
        return Err(invalid());
    }
    let item = &expected["pendingItem"]["item"];
    let absent_side = match step {
        Move::PreserveBefore | Move::RestoreBefore => item["before"].is_null(),
        Move::InstallAfter | Move::DetachAfter | Move::CancelStaged => item["after"].is_null(),
    };
    let (other_name, other_entry) = match step {
        Move::PreserveBefore => (name(&j, "staged")?, j["stagedEntry"].clone()),
        Move::InstallAfter | Move::DetachAfter | Move::CancelStaged => {
            (name(&j, "retained")?, j["retainedEntry"].clone())
        }
        Move::RestoreBefore => (name(&j, "detached")?, j["detachedEntry"].clone()),
    };
    let other_required = match step {
        Move::PreserveBefore | Move::RestoreBefore => !item["after"].is_null(),
        Move::InstallAfter | Move::DetachAfter | Move::CancelStaged => !item["before"].is_null(),
    };
    if other_entry.is_null() == other_required {
        return Err(invalid());
    }
    // Hold the other half of the recoverable pair against write/delete while
    // moving this half. Reopening only the moved file cannot prove rollback.
    let mut other = pin_entry(&dir, &other_name, &other_entry, check)?;
    if matches!(step, Move::CancelStaged) {
        absent(&dir, &leaf)?;
    }
    if absent_side {
        if !expected_entry.is_null() {
            return Err(invalid());
        }
        // Creation/deletion still require both relevant names to be absent.
        // Only NotFound counts; permissions and unsupported objects are errors.
        absent(&dir, &from)?;
        absent(&dir, &to)?;
        check()?;
        lease.validate()?;
        verify_pin(&dir, &other_name, &other_entry, &mut other, check)?;
        if matches!(step, Move::CancelStaged) {
            absent(&dir, &leaf)?;
        }
        j["phase"] = json!(complete);
        return publish(storage, project_id, expected, j, request);
    }
    if !expected_entry.is_object() {
        return Err(invalid());
    }
    let mut options = windows_replace::source_options();
    options.create_new(false);
    let mut source = dir.open_with(&from, &options).map_err(|_| invalid())?;
    windows_durability::require_ntfs(&source)?;
    if entry(&mut source, check)? != expected_entry
        || entry(&mut open_real_file_at(&dir, &from)?, check)? != expected_entry
    {
        return Err(invalid());
    }
    j["phase"] = json!(intent);
    let current = publish(storage, project_id, expected, j.clone(), request)?;
    check()?;
    lease.validate()?;
    windows_replace::move_new(&source, &lease, &to)?;
    // Do not acknowledge completion on cancellation or any flush/observation
    // failure. The durable intent remains the recovery boundary.
    source.sync_all().map_err(|_| invalid())?;
    let moved = entry(&mut source, check)?;
    if !unchanged_by_move(&expected_entry, &moved)
        || entry(&mut open_real_file_at(&dir, &to)?, check)? != moved
    {
        return Err(invalid());
    }
    lease.validate()?;
    absent(&dir, &from)?;
    let rebound = open_absolute_directory_capability(Path::new(
        expected["binding"]["canonicalGitRoot"]
            .as_str()
            .ok_or_else(invalid)?,
    ))?;
    let (rebound_parent, _) = parent(
        &rebound,
        expected["pendingItem"]["item"]["path"]
            .as_str()
            .ok_or_else(invalid)?,
    )?;
    if json!(windows_fs::identity(&rebound, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&rebound_parent, true)?) != j["parentIdentity"]
    {
        return Err(invalid());
    }
    verify_pin(&dir, &other_name, &other_entry, &mut other, check)?;
    if matches!(step, Move::CancelStaged) {
        absent(&dir, &leaf)?;
    }
    j[receipt] = moved;
    j["phase"] = json!(complete);
    publish(storage, project_id, &current, j, request)
}

fn absent(dir: &Dir, name: &str) -> Result<(), ProjectFailure> {
    match dir.symlink_metadata(name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(invalid()),
    }
}

fn pin_entry(
    dir: &Dir,
    name: &str,
    expected: &Value,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Option<File>, ProjectFailure> {
    if expected.is_null() {
        absent(dir, name)?;
        return Ok(None);
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .share_mode(1)
        .custom_flags(windows_fs::OPEN_REPARSE_POINT);
    let file = dir.open_with(name, &options).map_err(|_| invalid())?;
    let mut held = Some(file);
    verify_pin(dir, name, expected, &mut held, check)?;
    Ok(held)
}
fn verify_pin(
    dir: &Dir,
    name: &str,
    expected: &Value,
    held: &mut Option<File>,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<(), ProjectFailure> {
    match held {
        None if expected.is_null() => absent(dir, name),
        Some(file) => {
            if entry(file, check)? == *expected
                && entry(&mut open_real_file_at(dir, name)?, check)? == *expected
            {
                Ok(())
            } else {
                Err(invalid())
            }
        }
        _ => Err(invalid()),
    }
}

type Transition = (
    &'static str,
    &'static str,
    &'static str,
    String,
    String,
    Value,
    &'static str,
);
fn transition(
    j: &Value,
    item: &Value,
    leaf: &str,
    step: Move,
) -> Result<Transition, ProjectFailure> {
    Ok(match step {
        Move::PreserveBefore => (
            "staged",
            "moveBeforeIntent",
            "beforeMoved",
            leaf.to_owned(),
            name(&j, "retained")?,
            item["before"].clone(),
            "retainedEntry",
        ),
        Move::InstallAfter => (
            "beforeMoved",
            "installIntent",
            "applied",
            name(&j, "staged")?,
            leaf.to_owned(),
            j["stagedEntry"].clone(),
            "installedEntry",
        ),
        Move::DetachAfter => (
            "applied",
            "detachAfterIntent",
            "afterDetached",
            leaf.to_owned(),
            name(&j, "detached")?,
            j["installedEntry"].clone(),
            "detachedEntry",
        ),
        Move::CancelStaged => (
            if j["phase"] == "cancelReady" {
                "cancelReady"
            } else {
                "beforeMoved"
            },
            "cancelStagedIntent",
            "afterDetached",
            name(j, "staged")?,
            name(j, "detached")?,
            j["stagedEntry"].clone(),
            "detachedEntry",
        ),
        Move::RestoreBefore => (
            "afterDetached",
            "restoreBeforeIntent",
            "restored",
            name(&j, "retained")?,
            leaf.to_owned(),
            j["retainedEntry"].clone(),
            "restoredEntry",
        ),
    })
}

/// Bind stored receipts back to the validated archive item. Durable journal
/// fields are evidence to check, not authority to substitute arbitrary files.
fn validate_receipts(j: &Value, item: &Value) -> Result<(), ProjectFailure> {
    validate(j)?;
    if j["canceledUnwritten"] == true {
        // No source rename occurred. Keep prepared private files and validate
        // their original staged receipts, not a fictitious installed pair.
        if j["restoredEntry"] != item["before"] {
            return Err(invalid());
        }
        let mut staged = j.clone();
        staged
            .as_object_mut()
            .ok_or_else(invalid)?
            .remove("canceledUnwritten");
        staged["phase"] = json!("staged");
        staged["restoredEntry"] = Value::Null;
        return validate_receipts(&staged, item);
    }
    let rank = match j["phase"].as_str() {
        Some("reserved") => 0,
        Some("staged" | "moveBeforeIntent") => 1,
        Some("beforeMoved" | "installIntent" | "cancelStagedIntent" | "cancelReady") => 2,
        Some("applied" | "detachAfterIntent") => 3,
        Some("afterDetached" | "restoreBeforeIntent") => 4,
        Some("restored") => 5,
        _ => return Err(invalid()),
    };
    let canceling = j["cancelingBeforeInstall"] == true;
    let before = !item["before"].is_null();
    let after = !item["after"].is_null();
    for (key, required) in [
        ("guardEntry", rank >= 1),
        ("stagedEntry", rank >= 1 && after),
        ("retainedEntry", rank >= 2 && before),
        ("installedEntry", rank >= 3 && after && !canceling),
        ("detachedEntry", rank >= 4 && after),
        ("restoredEntry", rank >= 5 && before),
    ] {
        if (required && !j[key].is_object()) || (!required && !j[key].is_null()) {
            return Err(invalid());
        }
    }
    if rank >= 1 && after {
        let staged = &j["stagedEntry"];
        let image = json!({"kind":1,"executable":false,"size":staged["observation"]["length"],"digest":staged["digest"]});
        if image != item["after"]
            || (before
                && staged["observation"]["access"] != item["before"]["observation"]["access"])
        {
            return Err(invalid());
        }
    }
    for (required, before, after) in [
        (rank >= 2 && before, &item["before"], &j["retainedEntry"]),
        (
            rank >= 3 && after && !canceling,
            &j["stagedEntry"],
            &j["installedEntry"],
        ),
        (
            rank >= 4 && after,
            &j[if canceling {
                "stagedEntry"
            } else {
                "installedEntry"
            }],
            &j["detachedEntry"],
        ),
        (
            rank >= 5 && before,
            &j["retainedEntry"],
            &j["restoredEntry"],
        ),
    ] {
        if required && !unchanged_by_move(before, after) {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Read-only before-image check for an untouched tail. Archive validation is
/// owned by the batch caller; this never turns matching bytes into write authority.
pub(in crate::project) fn verify_unwritten_item(
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
    validate_relative_path(path, false)?;
    crate::project::protocol_path::from_native(&format!("C:/{path}"), true)
        .map_err(ProjectFailure::manifest)?;
    let parts: Vec<_> = path.split('/').collect();
    if parts.iter().any(|part| part.eq_ignore_ascii_case(".git")) {
        return Err(invalid());
    }
    let (leaf, ancestors) = parts.split_last().ok_or_else(invalid)?;
    let root_id = windows_fs::identity(&source._root, true)?;
    let mut dir = source._root.try_clone().map_err(|_| invalid())?;
    for (depth, part) in ancestors.iter().enumerate() {
        match dir.symlink_metadata(part) {
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && item["before"].is_null() =>
            {
                let rebound = open_absolute_directory_capability(&source.canonical_root)?;
                if windows_fs::identity(&rebound, true)? != root_id {
                    return Err(invalid());
                }
                let mut rebound_parent = rebound;
                for ancestor in &ancestors[..depth] {
                    rebound_parent = open_real_child_dir(&rebound_parent, ancestor)?;
                }
                if windows_fs::identity(&rebound_parent, true)? != windows_fs::identity(&dir, true)?
                {
                    return Err(invalid());
                }
                absent(&rebound_parent, part)?;
                return Ok(());
            }
            Ok(meta) if meta.is_dir() => dir = open_real_child_dir(&dir, part)?,
            _ => return Err(invalid()),
        }
    }
    let parent_id = windows_fs::identity(&dir, true)?;
    let mut held = pin_entry(&dir, leaf, &item["before"], &|| Ok(()))?;
    verify_pin(&dir, leaf, &item["before"], &mut held, &|| Ok(()))?;
    let rebound = open_absolute_directory_capability(&source.canonical_root)?;
    let (rebound_parent, _) = parent(&rebound, path)?;
    if windows_fs::identity(&rebound, true)? != root_id
        || windows_fs::identity(&rebound_parent, true)? != parent_id
    {
        return Err(invalid());
    }
    Ok(())
}

/// A new confirmed session may advance a fresh item through its fixed stages.
/// Failure leaves its last durable journal and must not recreate this owner.
pub(in crate::project) fn write_item_cancellable(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    canceled: &std::sync::atomic::AtomicBool,
) -> Result<Value, ProjectFailure> {
    let check = || {
        if canceled.load(std::sync::atomic::Ordering::Acquire) {
            Err(invalid())
        } else {
            Ok(())
        }
    };
    let reserved = reserve(manager, storage, project_id, expected, request, &check)?;
    let staged = stage(manager, storage, project_id, &reserved, request, &check)?;
    let retained = move_file(
        manager,
        storage,
        project_id,
        &staged,
        Move::PreserveBefore,
        request,
        &check,
    )?;
    move_file(
        manager,
        storage,
        project_id,
        &retained,
        Move::InstallAfter,
        request,
        &check,
    )
}
