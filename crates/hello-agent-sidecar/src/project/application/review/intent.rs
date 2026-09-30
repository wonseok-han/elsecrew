//! Pre-write application fence. No source writer exists here. A future writer
//! must durably leave `prepared` before its first write, so abandonment stays safe.
use super::*;
use crate::project::{required_project, snapshot_recovery::write_pair};
use confirmation::{Confirmations, PreparedApplication};
use serde_json::json;
use uuid::Uuid;

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
    if project
        .get("application_intent")
        .is_some_and(|v| v["state"] != "abandoned")
    {
        return Err(invalid());
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
    if record.as_object().is_none_or(|v| v.len() != 7)
        || record["version"] != 1
        || record["binding"]["projectId"] != project_id
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
        Some("abandoned") => "applicationAbandoned",
        _ => return Err(invalid()),
    };
    if lock.get("project_id") != Some(&json!(project_id))
        || lock.get("lock_id") != Some(&json!(lock_id))
        || lock.get("worktree_path") != record.get("worktreePath")
        || lock.get("writer_run_id") != Some(&Value::Null)
        || lock.get("poisoned") != Some(&json!(false))
        || lock.get("recovery_required") != Some(&json!(record["state"] != "abandoned"))
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
    if expected["state"] != "applying" || inspect(storage, project_id)? != *expected {
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
    let (header, item) = backup::verify_item(manager, &archive, index)?;
    if header != expected["backup"]["header"] || header["binding"] != expected["binding"] {
        return Err(invalid());
    }
    Ok(item)
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
