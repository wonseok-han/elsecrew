//! Recovery for versioned preparations that never crossed the export launch fence.
use super::*;
use crate::storage::ProjectionUpsert;

pub(super) fn write_pair(
    storage: &mut Storage,
    operation: &str,
    request_id: &str,
    project: Value,
    lock: Value,
    expected: &[(&str, &str, u64)],
) -> Result<(), ProjectFailure> {
    let mut records = Vec::new();
    for (kind, id_key, value) in [
        ("project", "project_id", project),
        ("lock", "lock_id", lock),
    ] {
        let payload = value
            .as_object()
            .cloned()
            .ok_or_else(|| ProjectFailure::manifest("snapshot pair must be objects"))?;
        records.push(ProjectionUpsert {
            aggregate_type: kind,
            aggregate_id: value_string(&payload, id_key)?.to_owned(),
            operation_id: format!("op_{operation}_{kind}"),
            request_id: Some(request_id.to_owned()),
            payload,
        });
    }
    storage.upsert_projection_records_atomic_checked(records, expected)?;
    Ok(())
}

impl ProjectManager {
    pub(crate) fn inspect_unstarted_snapshot(
        &self,
        storage: &Storage,
        project_id: &str,
    ) -> Result<Value, ProjectFailure> {
        self.verify_managed_paths()?;
        let project = required_project(storage, project_id)?;
        let worktree_id = value_string(&project, "worktree_id")?;
        let workspace_identity = value_string(&project, "workspace_identity")?;
        let lock_id = value_string(&project, "lock_id")?;
        if worktree_id.len() > 128
            || !worktree_id.starts_with("worktree_")
            || !worktree_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || lock_id != format!("lock_{worktree_id}")
        {
            return Err(ProjectFailure::manifest("invalid preparation identity"));
        }
        let lock = storage
            .projection_record("lock", lock_id)?
            .ok_or_else(|| ProjectFailure::recovery("preparation lock missing"))?;
        let worktree_path =
            canonical_protocol_path_without_existing(&self.worktrees_root.join(worktree_id))?;
        let manifest_path = canonical_protocol_path_without_existing(
            &self.manifests_root.join(format!("{worktree_id}.json")),
        )?;
        let evidence = lock.get("evidence").unwrap_or(&Value::Null);
        if project.get("lifecycle") != Some(&json!("recoveryRequired"))
            || project.get("worktree_path") != Some(&json!(worktree_path))
            || project.get("manifest_path") != Some(&json!(manifest_path))
            || lock.get("project_id") != Some(&json!(project_id))
            || lock.get("worktree_path") != Some(&json!(worktree_path))
            || lock.get("poisoned") != Some(&json!(false))
            || lock.get("recovery_required") != Some(&json!(true))
            || lock.get("writer_run_id") != Some(&Value::Null)
            || evidence["phase"] != "beforeSnapshotExport"
            || evidence["launchFenceVersion"] != 1
            || evidence["workspace_identity"] != workspace_identity
        {
            return Err(ProjectFailure::recovery(
                "snapshot has no proof that export never started",
            ));
        }
        for (dir, name) in [
            (&self.worktrees_dir, worktree_id.to_owned()),
            (&self.manifests_dir, format!("{worktree_id}.json")),
        ] {
            match dir.symlink_metadata(name) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(ProjectFailure::recovery(
                        "unexpected activated snapshot artifact",
                    ))
                }
            }
        }
        for kind in ["chat", "run"] {
            if storage.projection_records(kind)?.iter().any(|record| {
                record.get("worktree_id") == Some(&json!(worktree_id))
                    || record.get("workspace_identity") == Some(&json!(workspace_identity))
            }) {
                return Err(ProjectFailure::recovery(
                    "preparation is already bound to a chat or run",
                ));
            }
        }
        Ok(json!({
            "projectId":project_id, "worktreeId":worktree_id,
            "workspaceIdentity":workspace_identity, "lockId":lock_id,
            "projectRevision":project.get("last_event_sequence").and_then(Value::as_u64).ok_or_else(|| ProjectFailure::manifest("project revision missing"))?,
            "lockRevision":lock.get("last_event_sequence").and_then(Value::as_u64).ok_or_else(|| ProjectFailure::manifest("lock revision missing"))?,
        }))
    }

    pub(crate) fn abandon_unstarted_snapshot(
        &self,
        storage: &mut Storage,
        expected: &Value,
        request_id: &str,
    ) -> Result<(), ProjectFailure> {
        self.abandon_unstarted_snapshot_inner(storage, expected, request_id, None)
    }

    #[cfg(test)]
    pub(super) fn abandon_unstarted_snapshot_with_hook(
        &self,
        storage: &mut Storage,
        expected: &Value,
        request_id: &str,
        hook: &mut dyn FnMut(&mut Storage),
    ) -> Result<(), ProjectFailure> {
        self.abandon_unstarted_snapshot_inner(storage, expected, request_id, Some(hook))
    }

    fn abandon_unstarted_snapshot_inner(
        &self,
        storage: &mut Storage,
        expected: &Value,
        request_id: &str,
        hook: Option<&mut dyn FnMut(&mut Storage)>,
    ) -> Result<(), ProjectFailure> {
        let project_id = expected
            .get("projectId")
            .and_then(Value::as_str)
            .ok_or_else(|| ProjectFailure::manifest("project identity missing"))?;
        // A lost response may be retried only against the exact durable completion.
        let existing = required_project(storage, project_id)?;
        if existing.get("lifecycle") == Some(&json!("abandoned")) {
            if let Some(lock_id) = expected.get("lockId").and_then(Value::as_str) {
                if let Some(lock) = storage.projection_record("lock", lock_id)? {
                    let evidence = lock.get("evidence").unwrap_or(&Value::Null);
                    if existing.get("lock_id") == Some(&json!(lock_id))
                        && existing.get("worktree_id") == expected.get("worktreeId")
                        && existing.get("workspace_identity") == expected.get("workspaceIdentity")
                        && lock.get("project_id") == Some(&json!(project_id))
                        && lock.get("recovery_required") == Some(&json!(false))
                        && lock.get("poisoned") == Some(&json!(false))
                        && lock.get("writer_run_id") == Some(&Value::Null)
                        && evidence["phase"] == "abandonedBeforeSnapshotExport"
                        && evidence.get("inspection") == Some(expected)
                    {
                        return Ok(());
                    }
                }
            }
            return Err(ProjectFailure::dirty(
                "abandoned preparation identity changed",
            ));
        }
        if self.inspect_unstarted_snapshot(storage, project_id)? != *expected {
            return Err(ProjectFailure::dirty(
                "preparation changed after inspection",
            ));
        }
        if let Some(hook) = hook {
            hook(storage);
        }
        let mut project = required_project(storage, project_id)?;
        let lock_id = value_string(&project, "lock_id")?.to_owned();
        let mut lock = storage
            .projection_record("lock", &lock_id)?
            .ok_or_else(|| ProjectFailure::recovery("preparation lock missing"))?;
        project.insert("lifecycle".into(), json!("abandoned"));
        lock.insert("recovery_required".into(), json!(false));
        lock.insert(
            "evidence".into(),
            json!({"phase":"abandonedBeforeSnapshotExport", "inspection":expected}),
        );
        write_pair(
            storage,
            &format!("snapshot_abandon_unstarted_{lock_id}"),
            request_id,
            Value::Object(project),
            Value::Object(lock),
            &[
                (
                    "project",
                    project_id,
                    expected["projectRevision"]
                        .as_u64()
                        .ok_or_else(|| ProjectFailure::manifest("project revision missing"))?,
                ),
                (
                    "lock",
                    &lock_id,
                    expected["lockRevision"]
                        .as_u64()
                        .ok_or_else(|| ProjectFailure::manifest("lock revision missing"))?,
                ),
            ],
        )
    }
}
