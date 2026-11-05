//! Read-only observations for an interrupted Windows save. These observations
//! never authorize deletion, retry, lock clearing, or completion on their own.
use super::*;

impl ProjectManager {
    pub(super) fn validate_windows_save_recovery_lock(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        save_id: &str,
        lock: &Map<String, Value>,
    ) -> Result<(), ProjectFailure> {
        self.verify_durable_save_session(storage, address, save_id)?;
        let evidence = lock.get("evidence").unwrap_or(&Value::Null);
        let intent = &evidence["windowsSaveGuard"];
        validate_file_version(
            intent["expectedVersion"].as_str().ok_or_else(|| {
                ProjectFailure::recovery("save recovery expected version missing")
            })?,
        )?;
        if matches!(
            intent["phase"].as_str(),
            Some(
                "sourceReady"
                    | "replaceIntent"
                    | "replaced"
                    | "cleanupIntent"
                    | "guardRemoved"
                    | "stagingRemoved"
                    | "cleanupComplete"
            )
        ) {
            validate_file_version(intent["sourceVersion"].as_str().ok_or_else(|| {
                ProjectFailure::recovery("save recovery source version missing")
            })?)?;
        }
        if lock
            .get("last_event_sequence")
            .and_then(Value::as_u64)
            .is_none()
        {
            return Err(ProjectFailure::recovery("save recovery revision missing"));
        }
        if lock.get("lock_id") != Some(&json!(format!("lock_{}", address.worktree_id)))
            || lock.get("project_id") != Some(&json!(address.project_id))
            || lock.get("writer_run_id") != Some(&Value::Null)
            || lock.get("poisoned") != Some(&json!(false))
            || lock.get("recovery_required") != Some(&json!(true))
            || lock.get("worktree_path")
                != Some(&json!(canonical_protocol_path_without_existing(
                    &self.worktrees_root.join(&address.worktree_id)
                )?))
            || evidence["save_id"] != save_id
            || intent["schemaVersion"] != 1
            || intent["saveId"] != save_id
            || intent["worktreeId"] != address.worktree_id
            || intent["workspaceIdentity"] != address.workspace_identity
            || !matches!(
                intent["phase"].as_str(),
                Some(
                    "preparing"
                        | "ready"
                        | "sourceReady"
                        | "replaceIntent"
                        | "replaced"
                        | "cleanupIntent"
                        | "guardRemoved"
                        | "stagingRemoved"
                        | "cleanupComplete"
                )
            )
        {
            return Err(ProjectFailure::recovery(
                "Windows save recovery binding differs",
            ));
        }
        let chat = storage
            .projection_record("chat", &address.chat_id)?
            .ok_or_else(|| ProjectFailure::recovery("save recovery chat missing"))?;
        let session = &chat["save_sessions"][save_id];
        if intent["path"] != session["path"]
            || intent["expectedVersion"] != session["expectedVersion"]
        {
            return Err(ProjectFailure::recovery(
                "save recovery session differs from intent",
            ));
        }
        Ok(())
    }

    #[allow(dead_code)] // Recovery UI/explicit actions are a subsequent connection.
    pub(crate) fn inspect_windows_save_recovery(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        save_id: &str,
    ) -> Result<Value, ProjectFailure> {
        if self.pending_saves.contains_key(save_id) {
            return Err(ProjectFailure::recovery(
                "save is still owned by an active session",
            ));
        }
        let active = self.validate_workspace_binding_with_lock(
            storage,
            address,
            "active",
            WorkspaceLockAdmission::SaveRecovery(save_id.to_owned()),
        )?;
        let lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::recovery("save recovery lock missing"))?;
        let intent = &lock["evidence"]["windowsSaveGuard"];
        let managed = open_absolute_directory_capability(&self.worktrees_root)?;
        if windows_fs::identity(&managed, true)? != windows_fs::identity(&self.worktrees_dir, true)?
        {
            return Err(ProjectFailure::recovery("managed recovery root moved"));
        }
        let root = self.worktree_capability(&active)?;
        let path = intent["path"]
            .as_str()
            .ok_or_else(|| ProjectFailure::recovery("save path missing"))?;
        let (parent, target) = open_parent_capability(&root, path)?;
        if windows_fs::identity(&parent, true)?.recovery_token() != intent["parentIdentity"] {
            return Err(ProjectFailure::recovery(
                "save recovery parent identity differs",
            ));
        }
        let guard = observe(
            &parent,
            internal_name(intent, "guardName", ".hello-agent-save-guard-")?,
        )?;
        let source = observe(
            &root,
            internal_name(intent, "sourceName", ".hello-agent-save-")?,
        )?;
        let staging = observe(&self.save_staging_dir, staging_name(intent)?)?;
        let destination = observe(&parent, &target)?;
        let matches = |observation: &Value, identity: &Value| {
            identity.is_string() && observation["identity"] == *identity
        };
        let observed = classify_destination(&destination, intent);
        Ok(json!({
            "chatId":address.chat_id,"projectId":address.project_id,
            "worktreeId":address.worktree_id,"workspaceIdentity":address.workspace_identity,
            "saveId":save_id,"lockRevision":lock.get("last_event_sequence"),
            "phase":intent["phase"],"observation":observed,
            "destinationIdentityMatches":matches(&destination, &intent["previousTargetIdentity"]),
            "destinationIdentityRecorded":intent["previousTargetIdentity"].is_string(),
            "guardIdentityMatches":matches(&guard, &intent["guardIdentity"]),
            "sourceIdentityMatches":matches(&source, &intent["sourceIdentity"]),
            "stagingIdentityMatches":matches(&staging, &intent["stagingIdentity"]),
            "guard":guard,"source":source,"staging":staging,"destination":destination,
            "recordedDirectoryDurability":intent["directoryDurability"],
            "directoryDurability":"unconfirmed","cleanupAllowed":false,"completionAllowed":false,
        }))
    }
}

fn classify_destination(destination: &Value, intent: &Value) -> &'static str {
    if intent["sourceIdentity"].is_string()
        && intent["sourceVersion"].is_string()
        && destination["identity"] == intent["sourceIdentity"]
        && destination["version"] == intent["sourceVersion"]
    {
        "replacementObserved"
    } else if intent["expectedVersion"].is_string()
        && destination["version"] == intent["expectedVersion"]
    {
        if !intent["previousTargetIdentity"].is_string() {
            "expectedContentOnly"
        } else if destination["identity"] == intent["previousTargetIdentity"] {
            "originalObserved"
        } else {
            "conflictOrMissing"
        }
    } else {
        "conflictOrMissing"
    }
}

pub(super) fn internal_name<'a>(
    intent: &'a Value,
    field: &str,
    prefix: &str,
) -> Result<&'a str, ProjectFailure> {
    let name = intent[field]
        .as_str()
        .ok_or_else(|| ProjectFailure::recovery("save artifact name missing"))?;
    let suffix = name
        .strip_prefix(prefix)
        .and_then(|v| v.strip_suffix(".tmp"))
        .ok_or_else(|| ProjectFailure::recovery("save artifact name is invalid"))?;
    if suffix.len() != 32 || !suffix.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err(ProjectFailure::recovery(
            "save artifact identity is invalid",
        ));
    }
    Ok(name)
}
pub(super) fn staging_name(intent: &Value) -> Result<&str, ProjectFailure> {
    let name = intent["stagingName"]
        .as_str()
        .ok_or_else(|| ProjectFailure::recovery("staging name missing"))?;
    if name != format!("{}.stage", intent["saveId"].as_str().unwrap_or_default())
        || name.contains(['/', '\\', ':'])
        || name.contains("..")
    {
        return Err(ProjectFailure::recovery(
            "staging name differs from save identity",
        ));
    }
    Ok(name)
}
pub(super) fn observe(parent: &Dir, name: &str) -> Result<Value, ProjectFailure> {
    match parent.symlink_metadata(name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"present":false}))
        }
        Err(error) => {
            return Err(ProjectFailure::recovery(format!(
                "could not inspect save artifact: {error}"
            )))
        }
        Ok(_) => {}
    }
    let mut options = CapOpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .share_mode(1)
        .custom_flags(windows_fs::OPEN_REPARSE_POINT);
    let mut file = parent.open_with(name, &options).map_err(|error| {
        ProjectFailure::recovery(format!(
            "save artifact cannot be held for inspection: {error}"
        ))
    })?;
    let identity = windows_fs::identity(&file, false)?.recovery_token();
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    let mut reader = std::io::Read::by_ref(&mut file).take(FILE_MAX_BYTES_V1 + 1);
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            ProjectFailure::recovery(format!("could not read save artifact: {error}"))
        })?;
        if read == 0 {
            break;
        }
        count += read as u64;
        if count > FILE_MAX_BYTES_V1 {
            return Err(ProjectFailure::recovery(
                "save recovery artifact exceeds file limit",
            ));
        }
        hash.update(&buffer[..read]);
    }
    let version = format!("sha256:{:x}", hash.finalize());
    Ok(json!({"present":true,"identity":identity,"version":version,"byteLength":count}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_content_with_different_identity_is_not_original() {
        let intent = json!({"expectedVersion":"old", "previousTargetIdentity":"original",
            "sourceVersion":"new", "sourceIdentity":"staged"});
        assert_eq!(
            classify_destination(&json!({"version":"old","identity":"foreign"}), &intent),
            "conflictOrMissing"
        );
        assert_eq!(
            classify_destination(&json!({"version":"old","identity":"original"}), &intent),
            "originalObserved"
        );
        assert_eq!(
            classify_destination(&json!({"version":"new","identity":"foreign"}), &intent),
            "conflictOrMissing"
        );
        assert_eq!(
            classify_destination(&json!({"version":"new","identity":"staged"}), &intent),
            "replacementObserved"
        );
        assert_eq!(
            classify_destination(
                &json!({"version":"old","identity":"unrecorded"}),
                &json!({"expectedVersion":"old"})
            ),
            "expectedContentOnly"
        );
        assert_eq!(
            classify_destination(&json!({"present":false}), &intent),
            "conflictOrMissing"
        );
    }
    #[test]
    fn oversized_recovery_artifact_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let file = std::fs::File::create(temp.path().join("large")).unwrap();
        file.set_len(FILE_MAX_BYTES_V1 + 1).unwrap();
        drop(file);
        let error = observe(&dir, "large").unwrap_err();
        assert!(error.detail.contains("exceeds file limit"));
    }
}
