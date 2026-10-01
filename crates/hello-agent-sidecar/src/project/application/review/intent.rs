//! Pre-write application fence. No source writer exists here. A future writer
//! must durably leave `prepared` before its first write, so abandonment stays safe.
use super::*;
use crate::project::{required_project, snapshot_recovery::write_pair};
use confirmation::{Confirmations, PreparedApplication};
use serde_json::json;
use uuid::Uuid;

#[cfg(target_os = "macos")]
pub(in crate::project) mod completion;
#[cfg(target_os = "macos")]
pub(in crate::project) mod execution;
mod observation;
#[cfg(target_os = "macos")]
pub(in crate::project) mod writer;
#[allow(unused_imports)]
pub(in crate::project) use observation::observe_pending_source;

fn invalid() -> ProjectFailure {
    ProjectFailure::recovery("application preparation identity, phase, or lock changed")
}

#[allow(dead_code)]
pub(in crate::project) fn begin(
    manager: &ProjectManager,
    storage: &mut Storage,
    confirmations: &mut Confirmations,
    prepared: PreparedApplication,
    request_id: &str,
) -> Result<Value, ProjectFailure> {
    let (plan, backup) = confirmations.consume_prepared(manager, storage, prepared)?;
    let project_id = plan.binding["projectId"].as_str().ok_or_else(invalid)?;
    let lock_id = format!(
        "lock_{}",
        plan.binding["worktreeId"].as_str().ok_or_else(invalid)?
    );
    let mut project = required_project(storage, project_id)?;
    if project.contains_key("application_intent") {
        // A completed record remains evidence, not authority for another write.
        // Fail closed if its lock has changed since completion.
        let previous = inspect(storage, project_id)?;
        if !matches!(
            previous["state"].as_str(),
            Some("applied" | "restored" | "abandoned")
        ) {
            return Err(invalid());
        }
        let previous_id = previous["applicationId"]
            .as_str()
            .ok_or_else(invalid)?
            .to_owned();
        let history = project
            .entry("application_history")
            .or_insert_with(|| json!({}));
        let history = history.as_object_mut().ok_or_else(invalid)?;
        if history.len() >= 1_000 || history.contains_key(&previous_id) {
            return Err(invalid());
        }
        history.insert(previous_id, previous);
    }
    let mut lock = storage
        .projection_record("lock", &lock_id)?
        .ok_or_else(invalid)?;
    if lock.get("project_id") != Some(&json!(project_id))
        || lock.get("writer_run_id") != Some(&Value::Null)
        || lock.get("recovery_required") != Some(&json!(false))
        || lock.get("poisoned") != Some(&json!(false))
    {
        return Err(invalid());
    }
    let project_revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let mut guards = vec![
        ("project", project_id, project_revision),
        (
            "lock",
            lock_id.as_str(),
            plan.binding["lockRevision"].as_u64().ok_or_else(invalid)?,
        ),
    ];
    if let Some(chat) = plan.binding.get("chatBinding") {
        guards.push((
            "chat",
            chat["chatId"].as_str().ok_or_else(invalid)?,
            chat["revision"].as_u64().ok_or_else(invalid)?,
        ));
    }
    let id = format!("application_{}", Uuid::new_v4().simple());
    let record = json!({"version":1,"applicationId":id,"state":"prepared","binding":plan.binding,"lockId":lock_id,"worktreePath":lock["worktree_path"],"backup":backup});
    project.insert("application_intent".into(), record.clone());
    if serde_json::to_vec(&project).map_err(|_| invalid())?.len() > 32 * 1024 * 1024 {
        return Err(invalid());
    }
    lock.insert("recovery_required".into(), json!(true));
    lock.insert(
        "evidence".into(),
        json!({"phase":"applicationPrepared","applicationId":id}),
    );
    write_pair(
        storage,
        &format!("{id}_prepare"),
        request_id,
        Value::Object(project),
        Value::Object(lock),
        &guards,
    )?;
    Ok(record)
}

#[allow(dead_code)]
pub(in crate::project) fn inspect(
    storage: &Storage,
    project_id: &str,
) -> Result<Value, ProjectFailure> {
    let project = required_project(storage, project_id)?;
    let record = project.get("application_intent").ok_or_else(invalid)?;
    let pending = record.get("pendingItem");
    let completed = record.get("completedItems");
    let restored = record.get("restoredItems");
    let unstarted = record.get("unstartedFrom");
    let item_state = matches!(
        record["state"].as_str(),
        Some("applying" | "applied" | "restored" | "abandoned")
    );
    let completed_count = match completed {
        Some(value) => {
            let items = value.as_array().ok_or_else(invalid)?;
            if items.len() > 100_000 || pending.is_none() || !item_state {
                return Err(invalid());
            }
            for (index, item) in items.iter().enumerate() {
                if item.as_object().is_none_or(|v| v.len() != 3)
                    || item["index"].as_u64() != Some(index as u64)
                    || !item["item"].is_object()
                    || item["writeJournal"]["phase"] != "applied"
                {
                    return Err(invalid());
                }
            }
            items.len() as u64
        }
        None => 0,
    };
    if let Some(boundary) = unstarted {
        let boundary = boundary.as_u64().ok_or_else(invalid)?;
        let restored_count = restored.and_then(Value::as_array).map_or(0, Vec::len) as u64;
        if boundary == 0
            || boundary
                >= record["backup"]["header"]["count"]
                    .as_u64()
                    .ok_or_else(invalid)?
            || boundary != completed_count + restored_count + 1
        {
            return Err(invalid());
        }
    }
    if let Some(restored) = restored {
        let items = restored.as_array().ok_or_else(invalid)?;
        if items.is_empty() || items.len() > 100_000 || pending.is_none() || !item_state {
            return Err(invalid());
        }
        for (offset, item) in items.iter().enumerate() {
            if item.as_object().is_none_or(|v| v.len() != 3)
                || item["index"].as_u64() != Some(completed_count + (items.len() - offset) as u64)
                || !item["item"].is_object()
                || item["writeJournal"]["phase"] != "restored"
            {
                return Err(invalid());
            }
        }
    }
    if record.as_object().is_none_or(|v| {
        v.len()
            != 7 + usize::from(pending.is_some())
                + usize::from(completed.is_some())
                + usize::from(restored.is_some())
                + usize::from(unstarted.is_some())
    }) || record["version"] != 1
        || record["binding"]["projectId"] != project_id
    {
        return Err(invalid());
    }
    if let Some(pending) = pending {
        if !item_state
            || pending
                .as_object()
                .is_none_or(|v| v.len() != 2 + usize::from(v.contains_key("writeJournal")))
            || pending["index"].as_u64() != Some(completed_count)
            || !pending["item"].is_object()
        {
            return Err(invalid());
        }
    }
    if record["state"] == "abandoned"
        && pending.is_some()
        && (completed.is_some()
            || restored.is_some()
            || unstarted.is_some()
            || pending.is_some_and(|v| v.get("writeJournal").is_some()))
    {
        return Err(invalid());
    }
    let id = record["applicationId"].as_str().ok_or_else(invalid)?;
    if id.len() != 44
        || !id.starts_with("application_")
        || !id[12..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let lock_id = record["lockId"].as_str().ok_or_else(invalid)?;
    if lock_id
        != format!(
            "lock_{}",
            record["binding"]["worktreeId"]
                .as_str()
                .ok_or_else(invalid)?
        )
    {
        return Err(invalid());
    }
    let lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let phase = match record["state"].as_str() {
        Some("prepared") => "applicationPrepared",
        Some("applying") => "applicationWriting",
        Some("applied") => "applicationApplied",
        Some("restored") => "applicationRestored",
        Some("abandoned") => "applicationAbandoned",
        _ => return Err(invalid()),
    };
    if lock.get("project_id") != Some(&json!(project_id))
        || lock.get("lock_id") != Some(&json!(lock_id))
        || lock.get("worktree_path") != record.get("worktreePath")
        || lock.get("writer_run_id") != Some(&Value::Null)
        || lock.get("poisoned") != Some(&json!(false))
        || lock.get("recovery_required")
            != Some(&json!(matches!(
                record["state"].as_str(),
                Some("prepared" | "applying")
            )))
        || lock.get("evidence") != Some(&json!({"phase":phase,"applicationId":id}))
    {
        return Err(invalid());
    }
    Ok(record.clone())
}

/// Read a single journal candidate from the current applying record, never from
/// renderer-supplied file paths or an unbound archive found on disk.
#[allow(dead_code)]
pub(in crate::project) fn inspect_write_item(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    expected: &Value,
    index: u64,
) -> Result<Value, ProjectFailure> {
    let index = usize::try_from(index).map_err(|_| invalid())?;
    inspect_write_items(manager, storage, project_id, expected)?
        .get(index)
        .cloned()
        .ok_or_else(invalid)
}

fn inspect_write_items(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    expected: &Value,
) -> Result<Vec<Value>, ProjectFailure> {
    if !matches!(
        expected["state"].as_str(),
        Some("applying" | "applied" | "restored")
    ) || inspect(storage, project_id)? != *expected
    {
        return Err(invalid());
    }
    let review_id = expected["backup"]["header"]["reviewId"]
        .as_str()
        .ok_or_else(invalid)?;
    let project = required_project(storage, project_id)?;
    if project
        .get("application_backups")
        .and_then(|v| v.get(review_id))
        != Some(&expected["backup"])
        || expected["backup"]["state"] != "ready"
    {
        return Err(invalid());
    }
    let archive = backup::Backup::from_value(&expected["backup"]["backup"])?;
    let (header, items) = backup::verify_items(manager, &archive)?;
    if header != expected["backup"]["header"] || header["binding"] != expected["binding"] {
        return Err(invalid());
    }
    Ok(items)
}

/// Reserve the first file before a writer may attempt its mutation. No completion
/// or cursor advancement is inferred: until a writer supplies durable ownership
/// and post-write evidence, an existing pending item is only inspectable.
#[allow(dead_code)]
pub(in crate::project) fn record_first_item(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request_id: &str,
) -> Result<Value, ProjectFailure> {
    if expected["state"] != "applying" || expected.get("pendingItem").is_some() {
        return Err(invalid());
    }
    let item = inspect_write_item(manager, storage, project_id, expected, 0)?;
    let id = expected["applicationId"].as_str().ok_or_else(invalid)?;
    let lock_id = expected["lockId"].as_str().ok_or_else(invalid)?;
    let mut project = required_project(storage, project_id)?;
    let lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let project_revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let lock_revision = lock["last_event_sequence"].as_u64().ok_or_else(invalid)?;
    let mut next = expected.clone();
    next["pendingItem"] = json!({"index":0,"item":item});
    project.insert("application_intent".into(), next.clone());
    storage.upsert_projection_records_atomic_checked(
        vec![crate::storage::ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!("op_{id}_item_0_pending"),
            request_id: Some(request_id.to_owned()),
            payload: project,
        }],
        &[
            ("project", project_id, project_revision),
            ("lock", lock_id, lock_revision),
        ],
    )?;
    Ok(next)
}

/// A restart observation, not a retry/write grant. Recheck the entire archive so
/// corrupted pending metadata cannot become a source path or restore authority.
#[allow(dead_code)]
pub(in crate::project) fn inspect_pending_item(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
) -> Result<Value, ProjectFailure> {
    let current = inspect(storage, project_id)?;
    let pending = current.get("pendingItem").ok_or_else(invalid)?;
    let items = inspect_write_items(manager, storage, project_id, &current)?;
    let index =
        usize::try_from(pending["index"].as_u64().ok_or_else(invalid)?).map_err(|_| invalid())?;
    if Some(&pending["item"]) != items.get(index) {
        return Err(invalid());
    }
    if let Some(completed) = current["completedItems"].as_array() {
        for (index, done) in completed.iter().enumerate() {
            if Some(&done["item"]) != items.get(index) {
                return Err(invalid());
            }
        }
    }
    if let Some(restored) = current["restoredItems"].as_array() {
        for done in restored {
            if Some(&done["item"])
                != items.get(
                    usize::try_from(done["index"].as_u64().ok_or_else(invalid)?)
                        .map_err(|_| invalid())?,
                )
            {
                return Err(invalid());
            }
        }
    }
    Ok(current)
}

/// Advance only after a verified applied pair. Preserve the full old journal so
/// subsequent recovery can restore in reverse order; never infer batch success.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(in crate::project) fn advance_regular_item(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect_pending_item(manager, storage, project_id)? != *expected
        || expected["state"] != "applying"
        || expected.get("restoredItems").is_some()
        || expected.get("unstartedFrom").is_some()
        || expected["pendingItem"]["writeJournal"]["phase"] != "applied"
        || writer::inspect_regular_outcome(manager, storage, project_id)?["comparison"]
            != "matchesAppliedPair"
    {
        return Err(invalid());
    }
    let index = expected["pendingItem"]["index"]
        .as_u64()
        .ok_or_else(invalid)?
        .checked_add(1)
        .ok_or_else(invalid)?;
    let item = inspect_write_item(manager, storage, project_id, expected, index)?;
    let mut next = expected.clone();
    let mut completed = match expected.get("completedItems") {
        Some(value) => value.as_array().ok_or_else(invalid)?.clone(),
        None => Vec::new(),
    };
    completed.push(expected["pendingItem"].clone());
    next["completedItems"] = json!(completed);
    next["pendingItem"] = json!({"index":index,"item":item});
    // Bound retained metadata before persisting a larger recovery record.
    if serde_json::to_vec(&next).map_err(|_| invalid())?.len() > 32 * 1024 * 1024 {
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
    project.insert("application_intent".into(), next.clone());
    storage.upsert_projection_records_atomic_checked(
        vec![crate::storage::ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!(
                "op_{}_item_{index}_pending",
                expected["applicationId"].as_str().ok_or_else(invalid)?
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

/// Cancel the untouched tail after a completed prefix, preserving its current
/// applied/restored journal, or move back from a journal-free next item.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(in crate::project) fn cancel_unstarted_tail(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if expected["state"] != "applying"
        || inspect_pending_item(manager, storage, project_id)? != *expected
        || expected.get("restoredItems").is_some()
        || expected.get("unstartedFrom").is_some()
    {
        return Err(invalid());
    }
    let index = expected["pendingItem"]["index"]
        .as_u64()
        .ok_or_else(invalid)?;
    let mut next = expected.clone();
    let boundary = if let Some(journal) = expected["pendingItem"].get("writeJournal") {
        let applied = match journal["phase"].as_str() {
            Some("applied") => true,
            Some("restored") => false,
            _ => return Err(invalid()),
        };
        writer::verify_record_pair(manager, expected, applied)?;
        index.checked_add(1).ok_or_else(invalid)?
    } else {
        let mut completed = expected["completedItems"]
            .as_array()
            .ok_or_else(invalid)?
            .clone();
        next["pendingItem"] = completed.pop().ok_or_else(invalid)?;
        next["completedItems"] = json!(completed);
        index
    };
    if boundary
        >= expected["backup"]["header"]["count"]
            .as_u64()
            .ok_or_else(invalid)?
    {
        return Err(invalid());
    }
    next["unstartedFrom"] = json!(boundary);
    let mut project = required_project(storage, project_id)?;
    let revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let lock_id = expected["lockId"].as_str().ok_or_else(invalid)?;
    let lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let lock_revision = lock["last_event_sequence"].as_u64().ok_or_else(invalid)?;
    project.insert("application_intent".into(), next.clone());
    storage.upsert_projection_records_atomic_checked(
        vec![crate::storage::ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!(
                "op_{}_cancel_tail_{boundary}",
                expected["applicationId"].as_str().ok_or_else(invalid)?
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

/// Move backwards only after explicit restoration of the current item. Keep the
/// restored suffix, including retained after images, for final reconciliation.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(in crate::project) fn previous_regular_item(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect_pending_item(manager, storage, project_id)? != *expected
        || expected["state"] != "applying"
        || expected["pendingItem"]["writeJournal"]["phase"] != "restored"
        || writer::inspect_regular_outcome(manager, storage, project_id)?["comparison"]
            != "matchesUnappliedPair"
    {
        return Err(invalid());
    }
    let mut completed = expected["completedItems"]
        .as_array()
        .ok_or_else(invalid)?
        .clone();
    let previous = completed.pop().ok_or_else(invalid)?;
    let index = previous["index"].as_u64().ok_or_else(invalid)?;
    let mut restored = match expected.get("restoredItems") {
        Some(value) => value.as_array().ok_or_else(invalid)?.clone(),
        None => Vec::new(),
    };
    restored.push(expected["pendingItem"].clone());
    let mut next = expected.clone();
    next["completedItems"] = json!(completed);
    next["restoredItems"] = json!(restored);
    next["pendingItem"] = previous;
    if serde_json::to_vec(&next).map_err(|_| invalid())?.len() > 32 * 1024 * 1024 {
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
    project.insert("application_intent".into(), next.clone());
    storage.upsert_projection_records_atomic_checked(
        vec![crate::storage::ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!(
                "op_{}_item_{index}_restore_selected",
                expected["applicationId"].as_str().ok_or_else(invalid)?
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

/// Persist before the first source mutation. This is a recovery fence, NOT a
/// source-write capability: the eventual writer must separately establish file
/// admission and OS race protection. Replaying this transition never authorizes
/// resuming writes; an applying record requires explicit journal recovery.
#[allow(dead_code)]
pub(in crate::project) fn mark_write_started(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request_id: &str,
) -> Result<Value, ProjectFailure> {
    if expected["state"] != "prepared" || inspect(storage, project_id)? != *expected {
        return Err(invalid());
    }
    let review_id = expected["backup"]["header"]["reviewId"]
        .as_str()
        .ok_or_else(invalid)?;
    let backup = backup::staging::inspect(manager, storage, project_id, review_id)?;
    if backup["state"] != "ready"
        || backup != expected["backup"]
        || backup["header"]["binding"] != expected["binding"]
    {
        return Err(invalid());
    }
    let id = expected["applicationId"].as_str().ok_or_else(invalid)?;
    let lock_id = expected["lockId"].as_str().ok_or_else(invalid)?;
    let mut project = required_project(storage, project_id)?;
    let mut lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let project_revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let lock_revision = lock["last_event_sequence"].as_u64().ok_or_else(invalid)?;
    let mut applying = expected.clone();
    applying["state"] = json!("applying");
    project.insert("application_intent".into(), applying.clone());
    lock.insert(
        "evidence".into(),
        json!({"phase":"applicationWriting","applicationId":id}),
    );
    write_pair(
        storage,
        &format!("{id}_writing"),
        request_id,
        Value::Object(project),
        Value::Object(lock),
        &[
            ("project", project_id, project_revision),
            ("lock", lock_id, lock_revision),
        ],
    )?;
    Ok(applying)
}

#[allow(dead_code)]
pub(in crate::project) fn abandon_unwritten(
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request_id: &str,
) -> Result<Value, ProjectFailure> {
    if expected["state"] != "prepared" {
        return Err(invalid());
    }
    let current = inspect(storage, project_id)?;
    let mut abandoned = expected.clone();
    abandoned["state"] = json!("abandoned");
    if current == abandoned {
        return Ok(current);
    }
    if current != *expected {
        return Err(invalid());
    }
    let id = current["applicationId"].as_str().ok_or_else(invalid)?;
    let lock_id = current["lockId"].as_str().ok_or_else(invalid)?;
    let mut project = required_project(storage, project_id)?;
    let mut lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let project_revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let lock_revision = lock["last_event_sequence"].as_u64().ok_or_else(invalid)?;
    if project.get("application_intent") != Some(&current) {
        return Err(invalid());
    }
    project.insert("application_intent".into(), abandoned.clone());
    lock.insert("recovery_required".into(), json!(false));
    lock.insert(
        "evidence".into(),
        json!({"phase":"applicationAbandoned","applicationId":id}),
    );
    write_pair(
        storage,
        &format!("{id}_abandon"),
        request_id,
        Value::Object(project),
        Value::Object(lock),
        &[
            ("project", project_id, project_revision),
            ("lock", lock_id, lock_revision),
        ],
    )?;
    Ok(abandoned)
}
