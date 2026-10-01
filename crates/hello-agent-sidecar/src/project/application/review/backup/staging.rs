//! Durable backup preparation, not an application intent or source-writer lock.
use super::*;
use crate::project::required_project;
use crate::storage::ProjectionUpsert;
use serde_json::json;

fn write_record(
    storage: &mut Storage,
    target: ReviewTarget<'_>,
    binding: &Value,
    mut project: serde_json::Map<String, Value>,
    review_id: &str,
    record: Value,
    request_id: &str,
) -> Result<(), ProjectFailure> {
    let revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let phase = record["state"].as_str().ok_or_else(invalid)?.to_owned();
    let records = project
        .entry("application_backups")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(invalid)?;
    records.insert(review_id.to_owned(), record);
    // Bound retained preparation metadata before creating any archive. This is
    // not a total disk-retention policy, which is required before public activation.
    if serde_json::to_vec(&records).map_err(|_| invalid())?.len() > 256 * 1024 {
        return Err(invalid());
    }
    let lock_id = format!(
        "lock_{}",
        binding["worktreeId"].as_str().ok_or_else(invalid)?
    );
    let mut guards = vec![
        ("project", target.project_id(), revision),
        (
            "lock",
            lock_id.as_str(),
            binding["lockRevision"].as_u64().ok_or_else(invalid)?,
        ),
    ];
    if let ReviewTarget::Chat { address, .. } = target {
        guards.push((
            "chat",
            address.chat_id.as_str(),
            binding["chatBinding"]["revision"]
                .as_u64()
                .ok_or_else(invalid)?,
        ));
    }
    storage.upsert_projection_records_atomic_checked(
        vec![ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: target.project_id().to_owned(),
            operation_id: format!("op_backup_{review_id}_{phase}"),
            request_id: Some(request_id.to_owned()),
            payload: project,
        }],
        &guards,
    )?;
    Ok(())
}

#[allow(dead_code)]
pub(in crate::project) fn prepare(
    manager: &ProjectManager,
    storage: &mut Storage,
    target: ReviewTarget<'_>,
    review_id: &str,
    request_id: &str,
) -> Result<Value, ProjectFailure> {
    prepare_inner(manager, storage, target, review_id, request_id, || Ok(()))
}

#[cfg(test)]
pub(in crate::project) fn stop_after_reservation_for_test(
    manager: &ProjectManager,
    storage: &mut Storage,
    target: ReviewTarget<'_>,
    review_id: &str,
) -> Result<Value, ProjectFailure> {
    prepare_inner(
        manager,
        storage,
        target,
        review_id,
        "req_stop_backup",
        || Err(invalid()),
    )
}

fn prepare_inner(
    manager: &ProjectManager,
    storage: &mut Storage,
    target: ReviewTarget<'_>,
    review_id: &str,
    request_id: &str,
    reserved: impl FnOnce() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    let plan = persistence::reopen_target(manager, storage, target, review_id)?;
    let project = required_project(storage, target.project_id())?;
    if let Some(records) = project.get("application_backups") {
        let records = records.as_object().ok_or_else(invalid)?;
        if records.contains_key(review_id)
            || records
                .values()
                .any(|record| !matches!(record["state"].as_str(), Some("ready" | "abandoned")))
        {
            return Err(invalid());
        }
    }
    if target.resolve(manager, storage)?.1 != plan.binding {
        return Err(invalid());
    }
    let name = format!("backup_{}.bin", Uuid::new_v4().simple());
    let header = json!({"version":2,"reviewId":review_id,"binding":plan.binding,"changesDigest":plan.digest,"count":plan.changes.len()});
    let pending = json!({"version":1,"state":"preparing","name":name,"header":header});
    write_record(
        storage,
        target,
        &plan.binding,
        project,
        review_id,
        pending.clone(),
        request_id,
    )?;
    // The exact filename and target are durable before O_EXCL or any payload bytes.
    reserved()?;
    manager.check_application_preparation()?;
    let backup = create_inner(manager, storage, target, review_id, &name, |_| Ok(()))?;
    if verify(manager, &backup)? != header {
        return Err(invalid());
    }
    let current = persistence::reopen_target(manager, storage, target, review_id)?;
    if current.binding != plan.binding
        || current.digest != plan.digest
        || current.snapshot != plan.snapshot
    {
        return Err(invalid());
    }
    let project = required_project(storage, target.project_id())?;
    if project
        .get("application_backups")
        .and_then(|v| v.get(review_id))
        != Some(&pending)
    {
        return Err(invalid());
    }
    let ready =
        json!({"version":1,"state":"ready","name":name,"header":header,"backup":backup.to_value()});
    manager.check_application_preparation()?;
    write_record(
        storage,
        target,
        &plan.binding,
        project,
        review_id,
        ready.clone(),
        request_id,
    )?;
    Ok(ready)
}

// Read only: a preparing record never becomes ready just because a file exists.
#[allow(dead_code)]
pub(in crate::project) fn inspect(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    review_id: &str,
) -> Result<Value, ProjectFailure> {
    let project = required_project(storage, project_id)?;
    let record = project
        .get("application_backups")
        .and_then(|v| v.get(review_id))
        .ok_or_else(invalid)?;
    if record["version"] != 1
        || record["header"]["reviewId"] != review_id
        || record["header"]["binding"]["projectId"] != project_id
        || !record["name"].as_str().is_some_and(valid_name)
    {
        return Err(invalid());
    }
    match record["state"].as_str() {
        Some("preparing" | "abandoned") if record.as_object().is_some_and(|v| v.len() == 4) => {
            Ok(record.clone())
        }
        Some("ready") if record.as_object().is_some_and(|v| v.len() == 5) => {
            let backup = Backup::from_value(&record["backup"])?;
            if record["name"] != backup.name || verify(manager, &backup)? != record["header"] {
                return Err(invalid());
            }
            Ok(record.clone())
        }
        _ => Err(invalid()),
    }
}

/// Explicit recovery, never invoked by inspection/startup. The entire before
/// metadata is compared to the bound current baseline, not merely the Git image.
#[allow(dead_code)]
pub(in crate::project) fn resume(
    manager: &ProjectManager,
    storage: &mut Storage,
    target: ReviewTarget<'_>,
    review_id: &str,
    request_id: &str,
) -> Result<Value, ProjectFailure> {
    let record = inspect(manager, storage, target.project_id(), review_id)?;
    if !matches!(record["state"].as_str(), Some("preparing" | "ready")) {
        return Err(invalid());
    }
    let plan = persistence::reopen_target(manager, storage, target, review_id)?;
    let expected_header = json!({"version":2,"reviewId":review_id,"binding":plan.binding,"changesDigest":plan.digest,"count":plan.changes.len()});
    if record["header"] != expected_header {
        return Err(invalid());
    }
    if record["state"] == "ready" {
        return Ok(record);
    }
    let (active, _) = target.resolve(manager, storage)?;
    let source = manager.open_source_repository(&active.canonical_git_root)?;
    let baseline = capture_clean_baseline(manager, &source)?;
    receipt::verify_bound(manager, storage, &active, &baseline)?;
    let (backup, decoded) = recover_candidate(
        manager,
        record["name"].as_str().ok_or_else(invalid)?,
        baseline.inventory(),
    )?;
    if decoded != expected_header {
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
    let project = required_project(storage, target.project_id())?;
    if project
        .get("application_backups")
        .and_then(|v| v.get(review_id))
        != Some(&record)
    {
        return Err(invalid());
    }
    let ready = json!({"version":1,"state":"ready","name":backup.name,"header":expected_header,"backup":backup.to_value()});
    write_record(
        storage,
        target,
        &plan.binding,
        project,
        review_id,
        ready.clone(),
        request_id,
    )?;
    Ok(ready)
}

/// Ends only an unfinished backup preparation. No file deletion, source mutation,
/// or application lock release; retained filename remains available for cleanup.
#[allow(dead_code)]
pub(in crate::project) fn abandon(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    review_id: &str,
    expected: &Value,
    request_id: &str,
) -> Result<Value, ProjectFailure> {
    if expected["state"] != "preparing" {
        return Err(invalid());
    }
    let current = inspect(manager, storage, project_id, review_id)?;
    let mut abandoned = expected.clone();
    abandoned["state"] = json!("abandoned");
    if current == abandoned {
        return Ok(current);
    }
    if current != *expected {
        return Err(invalid());
    }
    let mut project = required_project(storage, project_id)?;
    let revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    if project["application_backups"][review_id] != current {
        return Err(invalid());
    }
    project["application_backups"][review_id] = abandoned.clone();
    storage.upsert_projection_records_atomic_checked(
        vec![ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!("op_backup_{review_id}_abandoned"),
            request_id: Some(request_id.to_owned()),
            payload: project,
        }],
        &[("project", project_id, revision)],
    )?;
    Ok(abandoned)
}
