//! Persist only a verification descriptor. Reopening reconstructs and verifies the
//! read-only plan; neither the descriptor nor reopening grants application approval.
use super::*;
use crate::project::required_project;
use crate::storage::ProjectionUpsert;
use serde_json::json;
use uuid::Uuid;

fn snapshot_digest(snapshot: &SourceInventory) -> Result<String, ProjectFailure> {
    let mut sink = ReviewHash {
        hash: Sha256::new(),
        remaining: MAX_REVIEW_BYTES,
    };
    serde_json::to_writer(
        &mut sink,
        &(1u32, snapshot.root_stamp.map(|value| value.to_string())),
    )
    .map_err(|_| invalid())?;
    for (path, entry) in &snapshot.entries {
        serde_json::to_writer(
            &mut sink,
            &(
                path,
                entry.identity,
                entry.kind,
                entry.mode,
                entry.size,
                entry.digest,
            ),
        )
        .map_err(|_| invalid())?;
    }
    Ok(format!("sha256:{:x}", sink.hash.finalize()))
}

fn descriptor(plan: &ReviewPlan, review_id: &str) -> Result<Value, ProjectFailure> {
    Ok(json!({
        "schemaVersion":1, "reviewId":review_id, "state":"prepared",
        "binding":plan.binding, "changesDigest":format!("sha256:{}", plan.digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()),
        "snapshotDigest":snapshot_digest(&plan.snapshot)?,
        "changeCount":plan.changes.len(),
    }))
}

fn invalid() -> ProjectFailure {
    ProjectFailure::dirty("saved review is missing, superseded, or no longer matches its files")
}

#[allow(dead_code)]
pub(in crate::project) fn publish(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    request_id: &str,
) -> Result<Value, ProjectFailure> {
    let plan = prepare(manager, storage, project_id)?;
    let mut project = required_project(storage, project_id)?;
    if snapshot_identity(&manager.validate_active_project(storage, project_id)?)? != plan.binding {
        return Err(invalid());
    }
    let revision = project
        .get("last_event_sequence")
        .and_then(Value::as_u64)
        .ok_or_else(invalid)?;
    let review_id = format!("review_{}", Uuid::new_v4().simple());
    let record = descriptor(&plan, &review_id)?;
    project.insert("application_review".into(), record.clone());
    // Same-transaction CAS prevents overwriting concurrent project metadata.
    storage.upsert_projection_records_atomic_checked(
        vec![ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!("op_{review_id}"),
            request_id: Some(request_id.to_owned()),
            payload: project,
        }],
        &[("project", project_id, revision)],
    )?;
    Ok(record)
}

#[allow(dead_code)]
pub(in crate::project) fn reopen(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    review_id: &str,
) -> Result<ReviewPlan, ProjectFailure> {
    if review_id.len() != 39
        || !review_id.starts_with("review_")
        || !review_id[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid());
    }
    let project = required_project(storage, project_id)?;
    let saved = project.get("application_review").ok_or_else(invalid)?;
    let current = prepare(manager, storage, project_id)?;
    if *saved != descriptor(&current, review_id)? {
        return Err(invalid());
    }
    Ok(current)
}
