//! Internal batch lifecycle. Completion requires every bound item, never just
//! the last writer response. No backup/retained-file deletion occurs here.
use super::*;

fn verify_batch(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    record: &Value,
    applied: bool,
) -> Result<(), ProjectFailure> {
    let count = record["backup"]["header"]["count"]
        .as_u64()
        .ok_or_else(invalid)?;
    let index = record["pendingItem"]["index"]
        .as_u64()
        .ok_or_else(invalid)?;
    if count == 0 || count > 100_000 {
        return Err(invalid());
    }
    let other = if applied {
        if index + 1 != count
            || record.get("restoredItems").is_some()
            || record.get("unstartedFrom").is_some()
        {
            return Err(invalid());
        }
        record.get("completedItems")
    } else {
        if index != 0
            || record["completedItems"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        {
            return Err(invalid());
        }
        record.get("restoredItems")
    };
    let empty = Vec::new();
    let items = match other {
        Some(value) => value.as_array().ok_or_else(invalid)?,
        None => &empty,
    };
    let written = record
        .get("unstartedFrom")
        .and_then(Value::as_u64)
        .unwrap_or(count);
    if items.len() as u64 + 1 != written {
        return Err(invalid());
    }
    // Repeat the complete observation to catch changes while walking the batch.
    // Still not an OS exclusion guarantee against an uncooperative same-UID editor.
    let archive_items = if written < count {
        inspect_write_items(manager, storage, project_id, record)?
    } else {
        Vec::new()
    };
    for _ in 0..2 {
        for index in written..count {
            let item = archive_items
                .get(usize::try_from(index).map_err(|_| invalid())?)
                .ok_or_else(invalid)?;
            writer::verify_unwritten_item(manager, record, item)?;
        }
        writer::verify_record_pair(manager, record, applied)?;
        for item in items {
            let mut selected = record.clone();
            selected["pendingItem"] = item.clone();
            writer::verify_record_pair(manager, &selected, applied)?;
        }
    }
    Ok(())
}

fn transition(
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    state: &str,
    phase: &str,
    locked: bool,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect(storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut project = required_project(storage, project_id)?;
    let lock_id = expected["lockId"].as_str().ok_or_else(invalid)?;
    let mut lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let project_revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let lock_revision = lock["last_event_sequence"].as_u64().ok_or_else(invalid)?;
    let id = expected["applicationId"].as_str().ok_or_else(invalid)?;
    let mut next = expected.clone();
    next["state"] = json!(state);
    project.insert("application_intent".into(), next.clone());
    lock.insert("recovery_required".into(), json!(locked));
    lock.insert("evidence".into(), json!({"phase":phase,"applicationId":id}));
    write_pair(
        storage,
        &format!("{id}_{phase}"),
        request,
        Value::Object(project),
        Value::Object(lock),
        &[
            ("project", project_id, project_revision),
            ("lock", lock_id, lock_revision),
        ],
    )?;
    Ok(next)
}

#[allow(dead_code)]
pub(in crate::project) fn finish(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    restored: bool,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if expected["state"] != "applying"
        || inspect_pending_item(manager, storage, project_id)? != *expected
    {
        return Err(invalid());
    }
    verify_batch(manager, storage, project_id, expected, !restored)?;
    transition(
        storage,
        project_id,
        expected,
        if restored { "restored" } else { "applied" },
        if restored {
            "applicationRestored"
        } else {
            "applicationApplied"
        },
        false,
        request,
    )
}

/// Explicit restore admission for a completed batch. Refuse other live/recovery
/// work before reacquiring the same lock; a finished record isn't a write grant.
#[allow(dead_code)]
pub(in crate::project) fn begin_restore(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if expected["state"] != "applied"
        || !storage.blocker_ids()?.is_empty()
        || inspect_pending_item(manager, storage, project_id)? != *expected
    {
        return Err(invalid());
    }
    verify_batch(manager, storage, project_id, expected, true)?;
    transition(
        storage,
        project_id,
        expected,
        "applying",
        "applicationWriting",
        true,
        request,
    )
}
