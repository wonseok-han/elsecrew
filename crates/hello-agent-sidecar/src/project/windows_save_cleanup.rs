//! Normal-save cleanup journal. Recovery resume is separate; no automatic retry
//! after an uncertain removal and no completion based on filename absence alone.
use super::{windows_directory_lease::DirectoryLease, windows_save_guard::SaveGuard, *};
const ANCHOR: &str = ".hello-agent-staging-anchor-v1";
const ANCHOR_BYTES: &[u8] = b"hello-agent-staging-anchor-v1\n";

impl ProjectManager {
    /// Internal explicit recovery entry point. The caller supplies the inspected
    /// lock revision; observations alone are never accepted as authorization.
    #[allow(dead_code)] // Public recovery contract/UI and native gate are pending.
    pub(crate) fn resume_windows_save_cleanup(
        &self,
        storage: &mut Storage,
        address: &WorkspaceAddress,
        save_id: &str,
        expected_lock_revision: u64,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        if self.pending_saves.contains_key(save_id) {
            return Err(ProjectFailure::recovery("save still has an active owner"));
        }
        let active = self.validate_workspace_binding_with_lock(
            storage,
            address,
            "active",
            WorkspaceLockAdmission::SaveRecovery(save_id.to_owned()),
        )?;
        let lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::recovery("recovery lock missing"))?;
        if lock.get("last_event_sequence").and_then(Value::as_u64) != Some(expected_lock_revision) {
            return Err(ProjectFailure::recovery(
                "save changed since recovery inspection",
            ));
        }
        let mut record = lock["evidence"]["windowsSaveGuard"].clone();
        if !matches!(
            record["phase"].as_str(),
            Some(
                "replaceIntent"
                    | "replaced"
                    | "cleanupIntent"
                    | "guardRemoved"
                    | "stagingRemoved"
                    | "cleanupComplete"
            )
        ) || record["durabilityPolicy"] != windows_durability::POLICY
            || (record["phase"] != "replaceIntent"
                && record["directoryDurability"] != windows_durability::FLUSHED)
        {
            return Err(ProjectFailure::recovery(
                "save has no completed replacement receipt",
            ));
        }
        let managed = open_absolute_directory_capability(&self.worktrees_root)?;
        if windows_fs::identity(&managed, true)? != windows_fs::identity(&self.worktrees_dir, true)?
        {
            return Err(ProjectFailure::recovery("managed recovery root moved"));
        }
        let root = self.worktree_capability(&active)?;
        let path = record["path"]
            .as_str()
            .ok_or_else(|| ProjectFailure::recovery("save path missing"))?;
        let (parent, target) = open_parent_capability(&root, path)?;
        let target_pin = DirectoryLease::acquire(&parent, OsStr::new(&target))?;
        let staging_pin = DirectoryLease::acquire(&self.save_staging_dir, OsStr::new(ANCHOR))?;
        self.verify_windows_staging_root()?;
        windows_durability::require_ntfs(&parent)?;
        windows_durability::require_ntfs(&self.save_staging_dir)?;
        if windows_fs::identity(&parent, true)?.recovery_token() != record["parentIdentity"]
            || windows_fs::identity(&self.save_staging_dir, true)?.recovery_token()
                != record["stagingParentIdentity"]
            || windows_fs::identity(&open_real_file_at(&self.save_staging_dir, ANCHOR)?, false)?
                .recovery_token()
                != record["stagingAnchorIdentity"]
        {
            return Err(ProjectFailure::recovery(
                "recovery directory binding differs",
            ));
        }
        let guard =
            windows_save_recovery::internal_name(&record, "guardName", ".hello-agent-save-guard-")?
                .to_owned();
        let staging = windows_save_recovery::staging_name(&record)?.to_owned();
        let source =
            windows_save_recovery::internal_name(&record, "sourceName", ".hello-agent-save-")?;
        require_absent(&root, source)?;
        verify_committed(&parent, &target, &record)?;
        if record["phase"] == "replaceIntent" {
            // Never repeat rename: only the exact replacement already at the
            // destination can advance. Reopen it and obtain a fresh OS flush
            // acknowledgement rather than inferring one from matching bytes.
            require_recorded(&parent, &guard, &record["guardIdentity"])?;
            require_recorded(&self.save_staging_dir, &staging, &record["stagingIdentity"])?;
            flush_recovered_replacement(&parent, &target, &record)?;
            target_pin.validate()?;
            staging_pin.validate()?;
            verify_committed(&parent, &target, &record)?;
            record["phase"] = json!("replaced");
            record["directoryDurability"] = json!(windows_durability::FLUSHED);
            self.write_windows_guard_intent(storage, &active, save_id, request_id, &record)?;
        }
        if record["phase"] == "replaced" {
            // No cleanup authorization existed yet: missing artifacts cannot be
            // interpreted as a previously completed deletion.
            require_recorded(&parent, &guard, &record["guardIdentity"])?;
            require_recorded(&self.save_staging_dir, &staging, &record["stagingIdentity"])?;
            record["phase"] = json!("cleanupIntent");
            self.write_windows_guard_intent(storage, &active, save_id, request_id, &record)?;
        }
        if record["phase"] == "cleanupIntent" {
            remove_or_observe_absent(&parent, &target_pin, &guard, &record["guardIdentity"])?;
            record["phase"] = json!("guardRemoved");
            self.write_windows_guard_intent(storage, &active, save_id, request_id, &record)?;
        }
        require_absent(&parent, &guard)?;
        verify_committed(&parent, &target, &record)?;
        if record["phase"] == "guardRemoved" {
            remove_or_observe_absent(
                &self.save_staging_dir,
                &staging_pin,
                &staging,
                &record["stagingIdentity"],
            )?;
            record["phase"] = json!("stagingRemoved");
            self.write_windows_guard_intent(storage, &active, save_id, request_id, &record)?;
        }
        require_absent(&self.save_staging_dir, &staging)?;
        verify_committed(&parent, &target, &record)?;
        target_pin.validate()?;
        staging_pin.validate()?;
        if record["phase"] == "stagingRemoved" {
            record["phase"] = json!("cleanupComplete");
            self.write_windows_guard_intent(storage, &active, save_id, request_id, &record)?;
        }
        let version = record["sourceVersion"]
            .as_str()
            .ok_or_else(|| ProjectFailure::recovery("save version missing"))?;
        self.persist_save_lock(
            storage,
            &active,
            save_id,
            request_id,
            false,
            Some(version),
            Some(&address.chat_id),
        )?;
        Ok(ProjectAction {
            result: json!({"saveId":save_id,"outcome":"committed","version":version}),
            ..ProjectAction::default()
        })
    }

    pub(super) fn prepare_windows_staging_anchor(
        &self,
        pending: &PendingSave,
    ) -> Result<windows_fs::Identity, ProjectFailure> {
        // The current staging file pins this parent while the permanent anchor
        // is prepared. The anchor keeps it nonempty after the last save cleanup.
        let pin =
            DirectoryLease::acquire(&self.save_staging_dir, OsStr::new(&pending.staging_name))?;
        self.verify_windows_staging_root()?;
        windows_durability::require_ntfs(&self.save_staging_dir)?;
        let mut options = CapOpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No)
            .custom_flags(windows_fs::OPEN_REPARSE_POINT | windows_durability::WRITE_THROUGH);
        match self.save_staging_dir.open_with(ANCHOR, &options) {
            Ok(mut file) => {
                file.write_all(ANCHOR_BYTES)
                    .and_then(|_| file.sync_all())
                    .map_err(|error| {
                        ProjectFailure::recovery(format!(
                            "could not prepare staging anchor: {error}"
                        ))
                    })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(ProjectFailure::recovery(format!(
                    "could not open staging anchor: {error}"
                )))
            }
        }
        let mut anchor = open_real_file_at(&self.save_staging_dir, ANCHOR)?;
        let identity = windows_fs::identity(&anchor, false)?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut anchor)
            .take(ANCHOR_BYTES.len() as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                ProjectFailure::recovery(format!("could not inspect staging anchor: {error}"))
            })?;
        if bytes != ANCHOR_BYTES {
            return Err(ProjectFailure::recovery(
                "existing staging anchor differs; it was not overwritten",
            ));
        }
        pin.validate()?;
        Ok(identity)
    }

    fn verify_windows_staging_root(&self) -> Result<(), ProjectFailure> {
        let current = open_absolute_directory_capability(&self.save_staging_root)?;
        if windows_fs::identity(&current, true)?
            != windows_fs::identity(&self.save_staging_dir, true)?
        {
            return Err(ProjectFailure::recovery("save staging root moved"));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn cleanup_windows_save(
        &self,
        storage: &mut Storage,
        active: &ActiveProject,
        save_id: &str,
        request_id: &str,
        guard: SaveGuard,
        prepared: &mut PreparedTarget,
        pending: &mut PendingSave,
    ) -> Result<DirectoryLease, ProjectFailure> {
        self.verify_windows_save_flush(storage, active, save_id, prepared)?;
        let lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::ambiguous("cleanup lock missing"))?;
        let mut record = lock
            .get("evidence")
            .and_then(|v| v.get("windowsSaveGuard"))
            .filter(|v| v.is_object())
            .cloned()
            .ok_or_else(|| ProjectFailure::ambiguous("cleanup intent missing"))?;
        let root = self.worktree_capability(active)?;
        let (parent, target) = open_parent_capability(&root, &pending.relative_path)?;
        guard.lease.validate()?;
        if windows_fs::identity(&parent, true)?.recovery_token() != record["parentIdentity"]
            || guard.name != record["guardName"]
            || guard.identity.recovery_token() != record["guardIdentity"]
            || pending.staging_name != record["stagingName"]
        {
            return Err(ProjectFailure::ambiguous("cleanup file binding differs"));
        }
        let staging_pin = DirectoryLease::acquire(&self.save_staging_dir, OsStr::new(ANCHOR))?;
        self.verify_windows_staging_root()?;
        if windows_fs::identity(&self.save_staging_dir, true)?.recovery_token()
            != record["stagingParentIdentity"]
            || windows_fs::identity(&open_real_file_at(&self.save_staging_dir, ANCHOR)?, false)?
                .recovery_token()
                != record["stagingAnchorIdentity"]
        {
            return Err(ProjectFailure::ambiguous("cleanup staging binding differs"));
        }
        // The independent guard still holds the hierarchy while the source
        // handle is released and the committed target becomes the new anchor.
        drop(prepared.file.take());
        let target_pin = DirectoryLease::acquire(&parent, OsStr::new(&target))?;
        verify_committed(&parent, &target, &record)?;
        record["phase"] = json!("cleanupIntent");
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)?;
        let guard_name = guard.name.clone();
        drop(guard);
        windows_remove::remove_recorded(
            &target_pin,
            &guard_name,
            record["guardIdentity"]
                .as_str()
                .ok_or_else(|| ProjectFailure::ambiguous("cleanup guard ID missing"))?,
        )?;
        record["phase"] = json!("guardRemoved");
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)?;
        verify_committed(&parent, &target, &record)?;
        drop(pending.file.take());
        windows_remove::remove_recorded(
            &staging_pin,
            &pending.staging_name,
            record["stagingIdentity"]
                .as_str()
                .ok_or_else(|| ProjectFailure::ambiguous("cleanup staging ID missing"))?,
        )?;
        record["phase"] = json!("stagingRemoved");
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)?;
        verify_committed(&parent, &target, &record)?;
        target_pin.validate()?;
        staging_pin.validate()?;
        record["phase"] = json!("cleanupComplete");
        self.write_windows_guard_intent(storage, active, save_id, request_id, &record)?;
        Ok(target_pin)
    }
}

fn flush_recovered_replacement(
    parent: &Dir,
    target: &str,
    record: &Value,
) -> Result<(), ProjectFailure> {
    let mut options = CapOpenOptions::new();
    options
        .read(true)
        .write(true)
        .follow(FollowSymlinks::No)
        .share_mode(1)
        .custom_flags(windows_fs::OPEN_REPARSE_POINT | windows_durability::WRITE_THROUGH);
    let mut file = parent.open_with(target, &options).map_err(|error| {
        ProjectFailure::recovery(format!(
            "could not hold replacement for recovery flush: {error}"
        ))
    })?;
    windows_durability::require_ntfs(&file)?;
    let observation = windows_save_recovery::observe_file(&mut file)?;
    if record["durabilityPolicy"] != windows_durability::POLICY
        || observation["identity"] != record["sourceIdentity"]
        || observation["version"] != record["sourceVersion"]
        || windows_security::fingerprint(&file)? != record["accessFingerprint"]
    {
        return Err(ProjectFailure::recovery(
            "recovery flush target differs from replacement intent",
        ));
    }
    file.sync_all().map_err(|error| {
        ProjectFailure::ambiguous(format!("recovery replacement flush failed: {error}"))
    })
}

fn verify_committed(parent: &Dir, target: &str, record: &Value) -> Result<(), ProjectFailure> {
    let observation = windows_save_recovery::observe(parent, target)?;
    let file = open_real_file_at(parent, target)?;
    if observation["identity"] != record["sourceIdentity"]
        || observation["version"] != record["sourceVersion"]
        || windows_fs::identity(&file, false)?.recovery_token() != record["sourceIdentity"]
        || windows_security::fingerprint(&file)? != record["accessFingerprint"]
    {
        return Err(ProjectFailure::ambiguous(
            "committed file changed during cleanup",
        ));
    }
    Ok(())
}

fn require_absent(parent: &Dir, name: &str) -> Result<(), ProjectFailure> {
    if windows_save_recovery::observe(parent, name)?["present"] != false {
        return Err(ProjectFailure::recovery(
            "completed cleanup name is occupied",
        ));
    }
    Ok(())
}

fn require_recorded(parent: &Dir, name: &str, identity: &Value) -> Result<(), ProjectFailure> {
    if !identity.is_string()
        || windows_save_recovery::observe(parent, name)?["identity"] != *identity
    {
        return Err(ProjectFailure::recovery(
            "cleanup artifact differs from durable identity",
        ));
    }
    Ok(())
}

/// Only called after the durable deletion intent. Foreign names are preserved.
fn remove_or_observe_absent(
    parent: &Dir,
    pin: &DirectoryLease,
    name: &str,
    identity: &Value,
) -> Result<(), ProjectFailure> {
    let expected = identity
        .as_str()
        .ok_or_else(|| ProjectFailure::recovery("cleanup identity missing"))?;
    let observation = windows_save_recovery::observe(parent, name)?;
    if observation["present"] == false {
        return Ok(());
    }
    if observation["identity"] != *identity {
        return Err(ProjectFailure::recovery(
            "cleanup name was reused by another file",
        ));
    }
    windows_remove::remove_recorded(pin, name, expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_flush_requires_exact_replacement_and_preserves_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        std::fs::write(temp.path().join("guard"), b"guard").unwrap();
        std::fs::write(temp.path().join("target"), b"old").unwrap();
        let pin = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        let mut source = dir
            .open_with("source", &windows_replace::source_options())
            .unwrap();
        source.write_all(b"replacement").unwrap();
        source.sync_all().unwrap();
        let record = json!({
            "durabilityPolicy":windows_durability::POLICY,
            "sourceIdentity":windows_fs::identity(&source, false).unwrap().recovery_token(),
            "sourceVersion":sha256_file_version_v1(b"replacement"),
            "accessFingerprint":windows_security::fingerprint(&source).unwrap(),
        });
        // The old destination cannot be promoted by content/phase assumptions.
        assert!(flush_recovered_replacement(&dir, "target", &record).is_err());
        windows_replace::replace(&source, &pin, "target").unwrap();
        drop(source); // Simulate loss of the original write-through handle.
        flush_recovered_replacement(&dir, "target", &record).unwrap();
        let mut wrong = record.clone();
        wrong["sourceVersion"] = json!(sha256_file_version_v1(b"other"));
        assert!(flush_recovered_replacement(&dir, "target", &wrong).is_err());
        wrong = record.clone();
        wrong["accessFingerprint"] = json!("wrong");
        assert!(flush_recovered_replacement(&dir, "target", &wrong).is_err());
        assert_eq!(
            std::fs::read(temp.path().join("target")).unwrap(),
            b"replacement"
        );
        assert!(!temp.path().join("source").exists());
    }

    #[test]
    fn interrupted_cleanup_accepts_absence_but_preserves_reused_name() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("anchor"), b"anchor").unwrap();
        std::fs::write(temp.path().join("owned"), b"owned").unwrap();
        std::fs::write(temp.path().join("foreign"), b"foreign").unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let pin = DirectoryLease::acquire(&dir, OsStr::new("anchor")).unwrap();
        let identity = json!(windows_fs::identity(&dir.open("owned").unwrap(), false)
            .unwrap()
            .recovery_token());
        require_recorded(&dir, "owned", &identity).unwrap();
        assert!(require_absent(&dir, "owned").is_err());
        remove_or_observe_absent(&dir, &pin, "owned", &identity).unwrap();
        // A restart after deletion but before journal advancement can finish
        // only through the intent-authorized helper, never the pre-intent check.
        assert!(require_recorded(&dir, "owned", &identity).is_err());
        remove_or_observe_absent(&dir, &pin, "owned", &identity).unwrap();
        assert!(remove_or_observe_absent(&dir, &pin, "owned", &Value::Null).is_err());
        std::fs::rename(temp.path().join("foreign"), temp.path().join("owned")).unwrap();
        assert!(remove_or_observe_absent(&dir, &pin, "owned", &identity).is_err());
        assert_eq!(
            std::fs::read(temp.path().join("owned")).unwrap(),
            b"foreign"
        );
        assert_eq!(
            std::fs::read(temp.path().join("anchor")).unwrap(),
            b"anchor"
        );
    }
}
