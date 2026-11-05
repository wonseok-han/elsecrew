//! Durable guard preparation for Windows saves. No automatic cleanup on Drop.
//! Incomplete intents retain the recovery lock, even when creation failed.
use super::windows_directory_lease::DirectoryLease;
use super::*;

#[allow(dead_code)] // Replacement and verified cleanup are the next connection.
pub(super) struct SaveGuard {
    pub(super) lease: DirectoryLease,
    pub(super) name: String,
    pub(super) identity: windows_fs::Identity,
}

impl ProjectManager {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_windows_save_guard(
        &self,
        storage: &mut Storage,
        active: &ActiveProject,
        pending: &mut PendingSave,
        save_id: &str,
        request_id: &str,
        worktree: &Dir,
        parent: &Dir,
        target_name: &str,
    ) -> Result<SaveGuard, ProjectFailure> {
        // Before creating anything, pin the existing target, then rebind the
        // pinned hierarchy to the managed workspace's CURRENT rooted path.
        let preparing = DirectoryLease::acquire(parent, OsStr::new(target_name))?;
        let managed = open_absolute_directory_capability(&self.worktrees_root)?;
        if windows_fs::identity(&managed, true)? != windows_fs::identity(&self.worktrees_dir, true)?
        {
            return Err(ProjectFailure::recovery(
                "managed worktree root moved before save guard",
            ));
        }
        let current_root = self.worktree_capability(active)?;
        let (current_parent, _) = open_parent_capability(&current_root, &pending.relative_path)?;
        let parent_id = windows_fs::identity(parent, true)?;
        if windows_fs::identity(&current_root, true)? != windows_fs::identity(worktree, true)?
            || windows_fs::identity(&current_parent, true)? != parent_id
        {
            return Err(ProjectFailure::recovery(
                "save guard hierarchy changed before preparation",
            ));
        }
        preparing.validate()?;
        let name = format!(".hello-agent-save-guard-{}.tmp", Uuid::new_v4().simple());
        let mut record = json!({
            "schemaVersion":1,"phase":"preparing","saveId":save_id,
            "worktreeId":active.worktree_id,"workspaceIdentity":active.workspace_identity,
            "path":pending.relative_path,"expectedVersion":pending.expected_version,
            "stagingName":pending.staging_name,"guardName":name,
            "parentIdentity":parent_id.recovery_token(),"guardIdentity":Value::Null,
            "directoryDurability":"unconfirmed",
        });
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)?;
        pending.recovery_fenced = true;
        // Once the intent exists, preserve staging and evidence for every error.
        let result = (|| {
            let mut options = CapOpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No);
            let mut file = parent.open_with(&name, &options).map_err(|error| {
                ProjectFailure::recovery(format!("could not create save guard: {error}"))
            })?;
            file.write_all(b"hello-agent-save-guard-v1\n")
                .and_then(|_| file.sync_all())
                .map_err(|error| {
                    ProjectFailure::recovery(format!("could not flush save guard: {error}"))
                })?;
            let identity = windows_fs::identity(&file, false)?;
            let lease = DirectoryLease::acquire(parent, OsStr::new(&name))?;
            let reopened = open_real_file_at(parent, &name)?;
            if windows_fs::identity(&reopened, false)? != identity {
                return Err(ProjectFailure::recovery("save guard identity changed"));
            }
            record["phase"] = json!("ready");
            record["guardIdentity"] = json!(identity.recovery_token());
            self.write_windows_guard_intent(storage, active, save_id, request_id, &record)?;
            // The independent guard is now pinned before the target pin drops.
            drop(preparing);
            Ok(SaveGuard {
                lease,
                name,
                identity,
            })
        })();
        result.map_err(|error: ProjectFailure| {
            ProjectFailure::ambiguous(format!(
                "save guard preparation requires recovery; no automatic cleanup: {}",
                error.detail
            ))
        })
    }

    fn write_windows_guard_intent(
        &self,
        storage: &mut Storage,
        active: &ActiveProject,
        save_id: &str,
        request_id: &str,
        guard: &Value,
    ) -> Result<(), ProjectFailure> {
        let mut lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::recovery("save guard lock is missing"))?;
        if lock.get("project_id") != Some(&json!(active.project_id))
            || lock.get("writer_run_id") != Some(&Value::Null)
            || lock.get("poisoned") != Some(&json!(false))
        {
            return Err(ProjectFailure::recovery("save guard lock binding changed"));
        }
        let previous = lock
            .get("evidence")
            .and_then(|value| value.get("windowsSaveGuard"));
        let valid_phase = match guard["phase"].as_str() {
            Some("preparing") => {
                previous.is_none() && lock.get("recovery_required") == Some(&json!(false))
            }
            Some("ready") => previous.is_some_and(|old| {
                if !old.is_object() {
                    return false;
                }
                let mut expected = old.clone();
                expected["phase"] = json!("ready");
                expected["guardIdentity"] = guard["guardIdentity"].clone();
                old["phase"] == "preparing"
                    && expected == *guard
                    && lock.get("recovery_required") == Some(&json!(true))
            }),
            _ => false,
        };
        if !valid_phase
            || lock.get("worktree_path")
                != Some(&json!(canonical_protocol_path(&active.worktree_path)?))
        {
            return Err(ProjectFailure::recovery(
                "save guard intent transition or workspace changed",
            ));
        }
        lock.insert("recovery_required".into(), json!(true));
        lock.insert(
            "evidence".into(),
            json!({"phase":"saveGuardPreparation", "save_id":save_id,
            "windowsSaveGuard":guard}),
        );
        append_object(
            storage,
            "lock",
            &active.lock_id,
            &format!(
                "op_save_guard_{}_{}",
                guard["phase"].as_str().unwrap_or("invalid"),
                save_id
            ),
            request_id,
            Value::Object(lock),
        )
    }
}
