//! Normal-save cleanup journal. Recovery resume is separate; no automatic retry
//! after an uncertain removal and no completion based on filename absence alone.
use super::{windows_directory_lease::DirectoryLease, windows_save_guard::SaveGuard, *};
const ANCHOR: &str = ".hello-agent-staging-anchor-v1";
const ANCHOR_BYTES: &[u8] = b"hello-agent-staging-anchor-v1\n";

impl ProjectManager {
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
