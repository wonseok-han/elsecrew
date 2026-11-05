//! Durable guard preparation for Windows saves. No automatic cleanup on Drop.
//! Incomplete intents retain the recovery lock, even when creation failed.
use super::windows_directory_lease::DirectoryLease;
use super::*;

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
        source_name: &str,
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
        windows_durability::require_ntfs(parent)?;
        let name = format!(".hello-agent-save-guard-{}.tmp", Uuid::new_v4().simple());
        let mut record = json!({
            "schemaVersion":1,"phase":"preparing","saveId":save_id,
            "worktreeId":active.worktree_id,"workspaceIdentity":active.workspace_identity,
            "path":pending.relative_path,"expectedVersion":pending.expected_version,
            "stagingName":pending.staging_name,"guardName":name,
            "stagingIdentity":windows_fs::identity(&pending.file, false)?.recovery_token(),
            "parentIdentity":parent_id.recovery_token(),"guardIdentity":Value::Null,
            "directoryDurability":"unconfirmed",
            "durabilityPolicy":windows_durability::POLICY,
            "sourceName":source_name,"sourceIdentity":Value::Null,
            "sourceVersion":Value::Null,
        });
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)?;
        pending.recovery_fenced = true;
        // Once the intent exists, preserve staging and evidence for every error.
        let result = (|| {
            let mut options = CapOpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No)
                .custom_flags(windows_fs::OPEN_REPARSE_POINT | windows_durability::WRITE_THROUGH);
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn persist_windows_save_source(
        &self,
        storage: &mut Storage,
        active: &ActiveProject,
        save_id: &str,
        request_id: &str,
        guard: &SaveGuard,
        prepared: &PreparedTarget,
        source: &CapFile,
        version: &str,
    ) -> Result<(), ProjectFailure> {
        guard.lease.validate()?;
        let current_root = self.worktree_capability(active)?;
        if windows_fs::identity(&current_root, true)?
            != windows_fs::identity(&prepared.temp_parent, true)?
        {
            return Err(ProjectFailure::recovery("save source parent changed"));
        }
        let reopened = open_real_file_at(&current_root, &prepared.temp_name)?;
        let identity = windows_fs::identity(source, false)?;
        if windows_fs::identity(&reopened, false)? != identity {
            return Err(ProjectFailure::recovery("save source identity changed"));
        }
        let lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::recovery("save source lock missing"))?;
        let mut record = lock
            .get("evidence")
            .and_then(|value| value.get("windowsSaveGuard"))
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| ProjectFailure::recovery("save guard intent missing"))?;
        if record["saveId"] != save_id
            || record["phase"] != "ready"
            || record["sourceName"] != prepared.temp_name
            || record["guardName"] != guard.name
            || record["guardIdentity"] != guard.identity.recovery_token()
        {
            return Err(ProjectFailure::recovery(
                "save source intent binding changed",
            ));
        }
        record["phase"] = json!("sourceReady");
        record["sourceIdentity"] = json!(identity.recovery_token());
        record["sourceVersion"] = json!(version);
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn replace_windows_save(
        &self,
        storage: &mut Storage,
        active: &ActiveProject,
        save_id: &str,
        request_id: &str,
        guard: &SaveGuard,
        prepared: &mut PreparedTarget,
        expected_version: &str,
        mut pre_revalidation_hook: Option<&mut dyn FnMut()>,
        mut pre_syscall_hook: Option<&mut dyn FnMut()>,
    ) -> Result<(), ProjectFailure> {
        if let Some(hook) = &mut pre_revalidation_hook {
            hook();
        }
        let lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::recovery("save replacement lock missing"))?;
        let mut record = lock
            .get("evidence")
            .and_then(|value| value.get("windowsSaveGuard"))
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| ProjectFailure::recovery("save replacement intent missing"))?;
        let source = prepared
            .file
            .as_ref()
            .ok_or_else(|| ProjectFailure::recovery("save replacement source handle missing"))?;
        let source_id = windows_fs::identity(source, false)?;
        windows_durability::require_ntfs(source)?;
        if record["durabilityPolicy"] != windows_durability::POLICY {
            return Err(ProjectFailure::recovery(
                "save write-through policy is missing",
            ));
        }
        if record["phase"] != "sourceReady"
            || record["worktreeId"] != active.worktree_id
            || record["workspaceIdentity"] != active.workspace_identity
            || record["saveId"] != save_id
            || record["expectedVersion"] != expected_version
            || record["sourceName"] != prepared.temp_name
            || record["sourceIdentity"] != source_id.recovery_token()
            || record["guardName"] != guard.name
            || record["guardIdentity"] != guard.identity.recovery_token()
        {
            return Err(ProjectFailure::recovery("save replacement binding changed"));
        }
        let relative = record["path"]
            .as_str()
            .ok_or_else(|| ProjectFailure::recovery("save destination path missing"))?;
        let destination = PathBuf::from(WORKTREE_DIRECTORY)
            .join(&active.worktree_id)
            .join(relative);
        if destination != prepared.target_relative {
            return Err(ProjectFailure::recovery(
                "save destination intent path changed",
            ));
        }
        let managed = open_absolute_directory_capability(&self.worktrees_root)?;
        if windows_fs::identity(&managed, true)? != windows_fs::identity(&self.worktrees_dir, true)?
        {
            return Err(ProjectFailure::recovery(
                "managed root changed before replacement",
            ));
        }
        let root = self.worktree_capability(active)?;
        if windows_fs::identity(&root, true)? != windows_fs::identity(&prepared.temp_parent, true)?
        {
            return Err(ProjectFailure::recovery(
                "save source root changed before replacement",
            ));
        }
        let (parent, name) = open_parent_capability(&root, relative)?;
        if windows_fs::identity(&parent, true)?.recovery_token() != record["parentIdentity"] {
            return Err(ProjectFailure::recovery("save destination parent changed"));
        }
        let source_name = open_real_file_at(&root, &prepared.temp_name)?;
        if windows_fs::identity(&source_name, false)? != source_id {
            return Err(ProjectFailure::recovery("save source name changed"));
        }
        // Refuse ordinary writers while validating the old contents. Legacy
        // FileRenameInfo requires destination handles closed before replacement;
        // the independent parent/guard lease remains held throughout.
        let mut options = CapOpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .share_mode(5)
            .custom_flags(windows_fs::OPEN_REPARSE_POINT);
        let mut target = parent.open_with(&name, &options).map_err(|error| {
            ProjectFailure::recovery(format!("save target cannot be held: {error}"))
        })?;
        let target_id = windows_fs::identity(&target, false)?;
        let (_, _, current) = read_file_bounded_handle(&mut target, 0)?;
        if current != expected_version {
            return Err(ProjectFailure::recovery(
                "save target changed before replacement",
            ));
        }
        let access = windows_security::copy_access(&target, source)?;
        source.sync_all().map_err(|error| {
            ProjectFailure::recovery(format!("could not flush replacement security: {error}"))
        })?;
        guard.lease.validate()?;
        record["phase"] = json!("replaceIntent");
        record["accessFingerprint"] = json!(access);
        record["previousTargetIdentity"] = json!(target_id.recovery_token());
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)?;
        if let Some(hook) = &mut pre_syscall_hook {
            hook();
        }
        // Recheck the namespace after the test hook. The lease remains held.
        let current_target = open_real_file_at(&parent, &name)?;
        if windows_fs::identity(&current_target, false)? != target_id {
            return Err(ProjectFailure::recovery(
                "save target name changed before syscall",
            ));
        }
        // Optimistic version checking, as in macOS save, is not an atomic
        // comparison against hostile same-user edits. Legacy rename requires
        // destination handles closed; parent/guard and exact source stay pinned.
        if windows_security::fingerprint(&current_target)? != access
            || windows_security::fingerprint(source)? != access
        {
            return Err(ProjectFailure::recovery(
                "save security changed before replacement",
            ));
        }
        drop((current_target, target, source_name));
        windows_replace::replace(source, &guard.lease, &name)?;
        prepared.renamed = true;
        let mut committed = open_real_file_at(&parent, &name)?;
        if windows_fs::identity(&committed, false)? != source_id {
            return Err(ProjectFailure::ambiguous(
                "replacement file identity differs",
            ));
        }
        if windows_security::fingerprint(&committed)? != access {
            return Err(ProjectFailure::ambiguous("replacement security differs"));
        }
        let (_, _, version) = read_file_bounded_handle(&mut committed, 0)?;
        if record["sourceVersion"] != version {
            return Err(ProjectFailure::ambiguous("replacement content differs"));
        }
        source.sync_all().map_err(|error| {
            ProjectFailure::ambiguous(format!("replacement metadata flush failed: {error}"))
        })?;
        record["phase"] = json!("replaced");
        record["directoryDurability"] = json!(windows_durability::FLUSHED);
        // NTFS write-through metadata + flush acknowledgement is not cleanup
        // completion or a hardware power-loss guarantee.
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)
    }

    pub(super) fn verify_windows_save_flush(
        &self,
        storage: &Storage,
        active: &ActiveProject,
        save_id: &str,
        prepared: &PreparedTarget,
    ) -> Result<(), ProjectFailure> {
        let lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::ambiguous("save flush lock missing"))?;
        let record = lock
            .get("evidence")
            .and_then(|value| value.get("windowsSaveGuard"))
            .ok_or_else(|| ProjectFailure::ambiguous("save flush receipt missing"))?;
        let source = prepared
            .file
            .as_ref()
            .ok_or_else(|| ProjectFailure::ambiguous("flushed replacement handle missing"))?;
        if !prepared.renamed
            || record["phase"] != "replaced"
            || record["saveId"] != save_id
            || record["worktreeId"] != active.worktree_id
            || record["workspaceIdentity"] != active.workspace_identity
            || record["durabilityPolicy"] != windows_durability::POLICY
            || record["directoryDurability"] != windows_durability::FLUSHED
            || record["sourceIdentity"] != windows_fs::identity(source, false)?.recovery_token()
        {
            return Err(ProjectFailure::ambiguous(
                "save flush receipt binding differs",
            ));
        }
        Ok(())
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
            Some("sourceReady") => previous.is_some_and(|old| {
                if !old.is_object() {
                    return false;
                }
                let mut expected = old.clone();
                expected["phase"] = json!("sourceReady");
                expected["sourceIdentity"] = guard["sourceIdentity"].clone();
                expected["sourceVersion"] = guard["sourceVersion"].clone();
                old["phase"] == "ready"
                    && expected == *guard
                    && lock.get("recovery_required") == Some(&json!(true))
            }),
            Some("replaceIntent") | Some("replaced") => previous.is_some_and(|old| {
                if !old.is_object() {
                    return false;
                }
                let mut expected = old.clone();
                expected["phase"] = guard["phase"].clone();
                let predecessor = if guard["phase"] == "replaceIntent" {
                    expected["previousTargetIdentity"] = guard["previousTargetIdentity"].clone();
                    expected["accessFingerprint"] = guard["accessFingerprint"].clone();
                    "sourceReady"
                } else {
                    if guard["durabilityPolicy"] != windows_durability::POLICY
                        || guard["directoryDurability"] != windows_durability::FLUSHED
                    {
                        return false;
                    }
                    expected["directoryDurability"] = json!(windows_durability::FLUSHED);
                    "replaceIntent"
                };
                old["phase"] == predecessor
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
