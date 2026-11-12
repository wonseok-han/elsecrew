//! Explicitly end restoration availability without deleting source, archive, or lock.
use super::*;
use crate::storage::ProjectionUpsert;

pub(in crate::project) fn preview(
    storage: &Storage,
    project_id: &str,
    application_id: &str,
) -> Result<Value, ProjectFailure> {
    let record = inspect(storage, project_id)?;
    if record["applicationId"] != application_id
        || !matches!(
            record["state"].as_str(),
            Some("applied" | "restored" | "abandoned")
        )
    {
        return Err(invalid());
    }
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&record).map_err(|_| invalid())?)
    );
    Ok(json!({"applicationId":application_id,"fingerprint":fingerprint,"state":record["state"]}))
}

pub(in crate::project) fn retire(
    storage: &mut Storage,
    project_id: &str,
    application_id: &str,
    fingerprint: &str,
    request: &str,
) -> Result<(), ProjectFailure> {
    if preview(storage, project_id, application_id)?["fingerprint"] != fingerprint {
        return Err(invalid());
    }
    let record = inspect(storage, project_id)?;
    let lock_id = record["lockId"].as_str().ok_or_else(invalid)?;
    let lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let lock_revision = lock["last_event_sequence"].as_u64().ok_or_else(invalid)?;
    let mut project = required_project(storage, project_id)?;
    let revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    if project.get("application_intent") != Some(&record) {
        return Err(invalid());
    }
    let history = project
        .entry("application_history")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(invalid)?;
    if history.len() >= 1000 || history.contains_key(application_id) {
        return Err(invalid());
    }
    history.insert(application_id.to_owned(), record.clone());
    project.remove("application_intent");
    // Guard the unchanged terminal lock; this is not a lock-clear operation.
    storage.upsert_projection_records_atomic_checked(
        vec![ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!("op_{application_id}_retire"),
            request_id: Some(request.to_owned()),
            payload: project,
        }],
        &[
            ("project", project_id, revision),
            ("lock", lock_id, lock_revision),
        ],
    )?;
    Ok(())
}
