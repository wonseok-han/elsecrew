//! Selected, resumable deletion of already discarded app-owned copies.
//! Source files, manifests and the conversation/audit ledger are never deleted here.
use super::*;

#[cfg(test)]
thread_local! { static INTERRUPT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(crate) fn interrupt_at(stage: u8) {
    INTERRUPT.with(|value| value.set(stage));
}
fn checkpoint(stage: u8) -> Result<(), ProjectFailure> {
    #[cfg(test)]
    if INTERRUPT.with(|value| {
        if value.get() == stage {
            value.set(0);
            true
        } else {
            false
        }
    }) {
        return Err(denied());
    }
    let _ = stage;
    Ok(())
}

fn denied() -> ProjectFailure {
    ProjectFailure::recovery(
        "workspace cleanup requires a discarded, idle workspace with verified termination",
    )
}
fn io(error: std::io::Error) -> ProjectFailure {
    ProjectFailure::recovery(format!("workspace cleanup: {error}"))
}

impl ProjectManager {
    fn verify_cleanup_copy(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<(), ProjectFailure> {
        let active = self.validate_workspace_binding_with_lock(
            storage,
            address,
            "abandoned",
            WorkspaceLockAdmission::Cleanup,
        )?;
        self.verify_snapshot_repository(&self.worktree_capability(&active)?, &active)?;
        Ok(())
    }

    fn cleanup_chat(
        &self,
        storage: &Storage,
        chat_id: &str,
    ) -> Result<(Map<String, Value>, WorkspaceAddress), ProjectFailure> {
        if self.application_running()
            || self.preparation_cancel.is_some()
            || !self.pending_saves.is_empty()
        {
            return Err(denied());
        }
        let chat = storage
            .projection_record("chat", chat_id)?
            .ok_or_else(denied)?;
        let record = &chat["record"];
        if chat["state"] != "discarded"
            || record["state"] != "discarded"
            || crate::orchestration::load(&chat)?.is_some_and(|state| state.status == "running")
            || chat
                .get("save_sessions")
                .is_some_and(|sessions| sessions.as_object().is_none_or(|items| !items.is_empty()))
        {
            return Err(denied());
        }
        let field = |key: &str| record[key].as_str().map(str::to_owned).ok_or_else(denied);
        let address = WorkspaceAddress {
            chat_id: field("chatId")?,
            project_id: field("projectId")?,
            worktree_id: field("worktreeId")?,
            workspace_identity: field("workspaceIdentity")?,
        };
        if address.chat_id != chat_id
            || chat["project_id"] != address.project_id
            || chat["worktree_id"] != address.worktree_id
            || !address.worktree_id.starts_with("worktree_")
            || !address
                .worktree_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(denied());
        }
        let project = required_project(storage, &address.project_id)?;
        if project.get("prepared_workspaces").is_some_and(|value| {
            value.as_object().is_none_or(|items| {
                items.values().any(|item| {
                    !matches!(item["bindingState"].as_str(), Some("bound" | "abandoned"))
                })
            })
        }) {
            return Err(denied());
        }
        if project
            .get("application_intent")
            .is_some_and(|value| value["state"] != "abandoned")
        {
            return Err(denied());
        }
        let lock = storage
            .projection_record("lock", &format!("lock_{}", address.worktree_id))?
            .ok_or_else(denied)?;
        if lock["lock_id"] != format!("lock_{}", address.worktree_id)
            || lock["worktree_path"].as_str()
                != Some(
                    canonical_protocol_path_without_existing(
                        &self.worktrees_root.join(&address.worktree_id),
                    )?
                    .as_str(),
                )
            || lock["project_id"] != address.project_id
            || lock["writer_run_id"] != Value::Null
            || lock["poisoned"] != false
            || lock["recovery_required"] != false
        {
            return Err(denied());
        }
        for run in storage.projection_records("run")? {
            if run["chat_id"] == chat_id || run["record"]["worktreeId"] == address.worktree_id {
                let evidence = if run["source_kind"] == "actualAgent" {
                    &run["termination"]
                } else {
                    &run["record"]["terminationEvidence"]
                };
                if !matches!(
                    run["state"].as_str(),
                    Some("succeeded" | "failed" | "canceled")
                ) || evidence["processesProvenGone"] != true
                    || run.get("recovery").is_some_and(|value| {
                        !value.is_null()
                            && !matches!(
                                value["state"].as_str(),
                                Some("resolved" | "resolvedAbandoned")
                            )
                    })
                {
                    return Err(denied());
                }
            }
        }
        // Shared or ambiguous ownership is never authority to erase a directory.
        if storage
            .projection_records("chat")?
            .iter()
            .any(|other| other["worktree_id"] == address.worktree_id && other["chat_id"] != chat_id)
        {
            return Err(denied());
        }
        Ok((chat, address))
    }

    pub(crate) fn workspace_cleanup_list(
        &self,
        storage: &Storage,
    ) -> Result<ProjectAction, ProjectFailure> {
        let mut entries = Vec::new();
        for chat in storage.projection_records("chat")? {
            let Some(id) = chat["chat_id"].as_str() else {
                continue;
            };
            if chat["state"] != "discarded" {
                continue;
            }
            let checked = self.cleanup_chat(storage, id);
            let project_id = chat["project_id"].as_str().ok_or_else(denied)?;
            let project = required_project(storage, project_id)?;
            let job = project
                .get("workspace_cleanup_v1")
                .and_then(|value| value.get(id))
                .unwrap_or(&Value::Null);
            if job["state"] == "deleted" {
                continue;
            }
            let eligible = cfg!(unix)
                && checked.as_ref().is_ok_and(|(_, address)| {
                    job["state"] == "pending" || self.verify_cleanup_copy(storage, address).is_ok()
                });
            entries.push(json!({"chatId":id,"projectId":project_id,"title":chat["record"]["title"],"eligible":eligible,"pending":job["state"] == "pending"}));
            if entries.len() >= 200 {
                break;
            }
        }
        Ok(ProjectAction {
            result: json!({"schemaVersion":1,"action":"workspaceCleanupList","entries":entries}),
            ..Default::default()
        })
    }

    #[cfg(unix)]
    pub(crate) fn cleanup_workspace(
        &mut self,
        storage: &mut Storage,
        chat_id: &str,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let (_, address) = self.cleanup_chat(storage, chat_id)?;
        let mut project = required_project(storage, &address.project_id)?;
        let manifest = read_bounded_json(
            &self.manifests_dir,
            &format!("{}.json", address.worktree_id),
        )?;
        validate_project_payload_v1("worktreeManifest", &manifest).map_err(|_| denied())?;
        if manifest["lifecycle"] != "abandoned"
            || manifest["projectId"] != address.project_id
            || manifest["worktreeId"] != address.worktree_id
            || manifest["workspaceIdentity"] != address.workspace_identity
            || manifest["worktreePath"].as_str()
                != Some(
                    canonical_protocol_path_without_existing(
                        &self.worktrees_root.join(&address.worktree_id),
                    )?
                    .as_str(),
                )
            || manifest["canonicalGitRoot"] != project["canonical_path"]
        {
            return Err(denied());
        }
        let existing = project
            .get("workspace_cleanup_v1")
            .and_then(|v| v.get(chat_id))
            .cloned();
        let quarantine = format!("cleanup_{}", address.worktree_id);
        let identity = if let Some(job) = existing {
            if job["state"] == "deleted" {
                return Ok(ProjectAction {
                    result: json!({"schemaVersion":1,"action":"cleanupWorkspace","chatId":chat_id,"deleted":true}),
                    ..Default::default()
                });
            }
            if job["state"] != "pending"
                || job["worktreeId"] != address.worktree_id
                || job["quarantine"] != quarantine
                || job["manifest"] != manifest
            {
                return Err(denied());
            }
            serde_json::from_value::<(u64, u64)>(job["identity"].clone()).map_err(|_| denied())?
        } else {
            self.verify_cleanup_copy(storage, &address)?;
            let root = open_real_child_dir(&self.worktrees_dir, &address.worktree_id)?;
            let identity = capability_directory_identity(&root)?;
            // Preflight bounds and no-follow traversal before any deletion intent.
            walk(&root, false, 0, &mut 0)?;
            let jobs = project
                .entry("workspace_cleanup_v1")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(denied)?;
            jobs.insert(chat_id.into(),json!({"state":"pending","worktreeId":address.worktree_id,"quarantine":quarantine,"identity":identity,"manifest":manifest}));
            append_object(
                storage,
                "project",
                &address.project_id,
                &format!("op_cleanup_intent_{request_id}"),
                request_id,
                Value::Object(project.clone()),
            )?;
            identity
        };
        checkpoint(1)?;
        self.finish_workspace_discard(&address);
        match self.worktrees_dir.symlink_metadata(&address.worktree_id) {
            Ok(_) => {
                let root = open_real_child_dir(&self.worktrees_dir, &address.worktree_id)?;
                if capability_directory_identity(&root)? != identity
                    || self.worktrees_dir.symlink_metadata(&quarantine).is_ok()
                {
                    return Err(denied());
                }
                self.worktrees_dir
                    .rename(&address.worktree_id, &self.worktrees_dir, &quarantine)
                    .map_err(io)?;
                sync_capability_directory(&self.worktrees_dir)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
        checkpoint(2)?;
        match self.worktrees_dir.symlink_metadata(&quarantine) {
            Ok(_) => {
                let root = open_real_child_dir(&self.worktrees_dir, &quarantine)?;
                if capability_directory_identity(&root)? != identity {
                    return Err(denied());
                }
                walk(&root, true, 0, &mut 0)?;
                // Recheck the name before removing the now-empty directory.
                if capability_directory_identity(&open_real_child_dir(
                    &self.worktrees_dir,
                    &quarantine,
                )?)? != identity
                {
                    return Err(denied());
                }
                self.worktrees_dir.remove_dir(&quarantine).map_err(io)?;
                sync_capability_directory(&self.worktrees_dir)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
        checkpoint(3)?;
        project["workspace_cleanup_v1"][chat_id]["state"] = json!("deleted");
        append_object(
            storage,
            "project",
            &address.project_id,
            &format!("op_cleanup_done_{request_id}"),
            request_id,
            Value::Object(project),
        )?;
        Ok(ProjectAction {
            result: json!({"schemaVersion":1,"action":"cleanupWorkspace","chatId":chat_id,"deleted":true}),
            ..Default::default()
        })
    }
    #[cfg(not(unix))]
    pub(crate) fn cleanup_workspace(
        &mut self,
        _: &mut Storage,
        _: &str,
        _: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        Err(denied())
    }
}

#[cfg(unix)]
fn walk(dir: &Dir, erase: bool, depth: usize, count: &mut usize) -> Result<(), ProjectFailure> {
    if depth > 64 {
        return Err(denied());
    }
    for entry in dir.entries().map_err(io)? {
        let entry = entry.map_err(io)?;
        *count += 1;
        if *count > 200_000 {
            return Err(denied());
        }
        let name = entry.file_name();
        let metadata = dir.symlink_metadata(&name).map_err(io)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let child = open_real_child_dir(dir, name.to_str().ok_or_else(denied)?)?;
            let identity = capability_directory_identity(&child)?;
            walk(&child, erase, depth + 1, count)?;
            if erase {
                if capability_directory_identity(&open_real_child_dir(
                    dir,
                    name.to_str().ok_or_else(denied)?,
                )?)? != identity
                {
                    return Err(denied());
                }
                dir.remove_dir(&name).map_err(io)?;
            }
        } else if metadata.is_file() || metadata.file_type().is_symlink() {
            if erase {
                dir.remove_file(&name).map_err(io)?;
            }
        } else {
            return Err(denied());
        }
    }
    Ok(())
}
