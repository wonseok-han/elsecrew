//! Explicit cleanup of non-current backups; never called by startup or inspection.
//! Historical evidence survives, but the selected archive becomes unrestorable.
use super::*;
use crate::project::required_project;
use crate::storage::ProjectionUpsert;
use serde_json::json;

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::project) enum FailurePoint { Pending, Unlinked }

#[cfg(test)]
thread_local! {
    static FAILURE: std::cell::Cell<Option<FailurePoint>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(in crate::project) fn fail_once_at(point: FailurePoint) {
    FAILURE.with(|slot| slot.set(Some(point)));
}

#[cfg(test)]
fn inject_failure(point: FailurePoint) -> Result<(), ProjectFailure> {
    FAILURE.with(|slot| {
        if slot.get() == Some(point) {
            slot.set(None);
            Err(ProjectFailure::recovery("injected cleanup interruption"))
        } else { Ok(()) }
    })
}

fn lock(manager: &ProjectManager) -> Result<ArchiveLock, ProjectFailure> {
    ArchiveLock::acquire(directory(manager)?, libc::LOCK_EX, "backup cleanup requires idle preparation")
}

fn record(storage: &Storage, project: &str, review: &str) -> Result<Value, ProjectFailure> {
    let owner = required_project(storage, project)?;
    let value = owner
        .get("application_backups")
        .and_then(|v| v.get(review))
        .ok_or_else(invalid)?;
    if value["version"] != 1
        || !matches!(value["state"].as_str(), Some("abandoned" | "ready"))
        || value["header"]["reviewId"] != review
        || value["header"]["binding"]["projectId"] != project
        || !value["name"].as_str().is_some_and(valid_name)
    {
        return Err(invalid());
    }
    let fields = if value["state"] == "ready" {
        let backup = Backup::from_value(&value["backup"])?;
        if value["name"] != backup.name {
            return Err(invalid());
        }
        5
    } else {
        4
    };
    let mut owners = 0;
    // A duplicate filename in any project is ambiguous; no unlink authority.
    for other in storage.projection_records("project")? {
        retention::summarize(&other)?;
        if let Some(backups) = other.get("application_backups") {
            for candidate in backups.as_object().ok_or_else(invalid)?.values() {
                if candidate["name"] == value["name"] {
                    owners += 1;
                }
            }
        }
        if let Some(current) = other.get("application_intent") {
            // Even a completed current application remains a restoration target.
            if current["backup"]["name"] == value["name"] {
                return Err(invalid());
            }
        }
        if let Some(history) = other.get("application_history") {
            for application in history.as_object().ok_or_else(invalid)?.values() {
                if application["backup"]["name"] == value["name"]
                    && !historical_reference_matches(application, value)
                {
                    return Err(invalid());
                }
            }
        }
    }
    if owners != 1 {
        return Err(invalid());
    }
    match value.get("cleanup") {
        None if value.as_object().is_some_and(|v| v.len() == fields) => {}
        Some(_) if value.as_object().is_some_and(|v| v.len() == fields + 1) => {
            recorded_state(value)?;
        }
        _ => return Err(invalid()),
    }
    Ok(value.clone())
}

/// Ledger status only; this does not inspect the archive or grant deletion authority.
pub(super) fn recorded_state(record: &Value) -> Result<&str, ProjectFailure> {
    let Some(cleanup) = record.get("cleanup") else {
        return Ok("none");
    };
    if cleanup.as_object().map(|v| v.len()) != Some(3) || cleanup["version"] != 1 {
        return Err(invalid());
    }
    let state = cleanup["state"]
        .as_str()
        .filter(|s| matches!(*s, "pending" | "deleted"))
        .ok_or_else(invalid)?;
    let identity = cleanup.get("identity").ok_or_else(invalid)?;
    if !identity.is_null() {
        serde_json::from_value::<[i128; 9]>(identity.clone()).map_err(|_| invalid())?;
    }
    Ok(state)
}

fn historical_reference_matches(application: &Value, backup: &Value) -> bool {
    if !matches!(
        application["state"].as_str(),
        Some("applied" | "restored" | "abandoned")
    ) {
        return false;
    }
    let mut original = backup.clone();
    let Some(object) = original.as_object_mut() else {
        return false;
    };
    object.remove("cleanup");
    application["backup"] == original
}

#[cfg(test)]
mod reference_tests {
    use super::*;
    #[test]
    fn only_exact_terminal_history_may_lose_its_archive() {
        let backup =
            json!({"version":1,"state":"ready","name":"backup","header":{"reviewId":"review"}});
        let mut application = json!({"state":"applied","backup":backup});
        assert!(historical_reference_matches(&application, &backup));
        let mut pending = backup.clone();
        pending["cleanup"] = json!({"state":"pending"});
        assert!(historical_reference_matches(&application, &pending));
        application["state"] = json!("applying");
        assert!(!historical_reference_matches(&application, &backup));
        application["state"] = json!("restored");
        application["backup"]["header"]["reviewId"] = json!("other");
        assert!(!historical_reference_matches(&application, &backup));
    }
}

fn identity(dir: &Dir, name: &str) -> Result<Value, ProjectFailure> {
    let path = match dir.symlink_metadata(name) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(_) => return Err(invalid()),
    };
    if !path.is_file() || path.nlink() != 1 {
        return Err(invalid());
    }
    let file = open(dir, name, false)?;
    let actual = file.metadata().map_err(|_| invalid())?;
    if stamp(&actual) != stamp(&path) {
        return Err(invalid());
    }
    serde_json::to_value(stamp(&actual)).map_err(|_| invalid())
}

fn preview_inner(
    storage: &Storage,
    dir: &Dir,
    project: &str,
    review: &str,
) -> Result<(Value, Value, String), ProjectFailure> {
    let record = record(storage, project, review)?;
    let identity = identity(dir, record["name"].as_str().ok_or_else(invalid)?)?;
    if record["cleanup"]["state"] == "deleted" && !identity.is_null() {
        return Err(invalid());
    }
    if record["cleanup"]["state"] == "pending"
        && !identity.is_null()
        && identity != record["cleanup"]["identity"]
    {
        return Err(invalid());
    }
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!([project, review, record, identity]))
                .map_err(|_| invalid())?
        )
    );
    Ok((record, identity, fingerprint))
}

pub(in crate::project) fn preview(
    manager: &ProjectManager,
    storage: &Storage,
    project: &str,
    review: &str,
) -> Result<Value, ProjectFailure> {
    let dir = lock(manager)?;
    let (record, identity, fingerprint) = preview_inner(storage, &dir, project, review)?;
    Ok(
        json!({"reviewId":review,"fingerprint":fingerprint,"filePresent":!identity.is_null(),"state":record.get("cleanup").map(|v| v["state"].clone()).unwrap_or(json!("available"))}),
    )
}

fn persist(
    storage: &mut Storage,
    project_id: &str,
    review: &str,
    expected: &Value,
    next: Value,
    request: &str,
) -> Result<(), ProjectFailure> {
    let mut project = required_project(storage, project_id)?;
    if project["application_backups"][review] != *expected {
        return Err(invalid());
    }
    let revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let phase = next["cleanup"]["state"]
        .as_str()
        .ok_or_else(invalid)?
        .to_owned();
    project["application_backups"][review] = next;
    storage.upsert_projection_records_atomic_checked(
        vec![ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!("op_backup_cleanup_{review}_{phase}"),
            request_id: Some(request.to_owned()),
            payload: project,
        }],
        &[("project", project_id, revision)],
    )?;
    Ok(())
}

pub(in crate::project) fn remove(
    manager: &ProjectManager,
    storage: &mut Storage,
    project: &str,
    review: &str,
    fingerprint: &str,
    request: &str,
) -> Result<(), ProjectFailure> {
    let dir = lock(manager)?;
    let (mut current, observed, expected) = preview_inner(storage, &dir, project, review)?;
    if expected != fingerprint {
        return Err(ProjectFailure::recovery(
            "backup cleanup preview changed; inspect again",
        ));
    }
    if current["cleanup"]["state"] == "deleted" {
        return Ok(());
    }
    if current.get("cleanup").is_none() {
        let mut next = current.clone();
        next["cleanup"] = json!({"version":1,"state":"pending","identity":observed});
        persist(storage, project, review, &current, next.clone(), request)?;
        current = next;
    }
    #[cfg(test)]
    inject_failure(FailurePoint::Pending)?;
    // Recheck after durable intent. Missing only completes this explicit attempt;
    // a replacement file is never interpreted as the previously consented file.
    let name = current["name"].as_str().ok_or_else(invalid)?;
    let now = identity(&dir, name)?;
    if !now.is_null() {
        if now != current["cleanup"]["identity"] {
            return Err(invalid());
        }
        dir.remove_file(name).map_err(|_| invalid())?;
    }
    sync_capability_directory(&dir)?;
    #[cfg(test)]
    inject_failure(FailurePoint::Unlinked)?;
    let mut complete = current.clone();
    complete["cleanup"]["state"] = json!("deleted");
    persist(storage, project, review, &current, complete, request)
}
