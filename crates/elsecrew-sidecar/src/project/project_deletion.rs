//! Deleting a project or a task (docs/proposals/m5-project-deletion.md).
//!
//! The ledger is append-only and replay-verified, so deletion never removes
//! event rows. A deleted project is tombstoned: its catalog path becomes a
//! marker (freeing the real folder path for a fresh project ID), it leaves the
//! catalog, and its app-owned files are removed once every run in it has
//! provably exited. A deleted task is recorded on its project and its
//! workspace is removed the same way. The user's original folder is never
//! touched.
use super::*;

pub(crate) const PROJECT_FIELD: &str = "project_deletion_v1";
pub(crate) const CHAT_FIELD: &str = "chat_deletion_v1";
const BASELINE_DIRECTORY: &str = "source-baselines-v1";

fn denied(reason: &str) -> ProjectFailure {
    ProjectFailure::recovery(format!("deletion is waiting: {reason}"))
}

fn io(error: std::io::Error) -> ProjectFailure {
    ProjectFailure::recovery(format!("deletion: {error}"))
}

fn valid_worktree_id(id: &str) -> bool {
    id.len() <= 128 && id.starts_with("worktree_") && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The marker every tombstoned project carries instead of its folder path.
pub(crate) fn deleted_marker(project_id: &str) -> String {
    format!("deleted:{project_id}")
}

pub(crate) fn is_deleted(project: &Map<String, Value>) -> bool {
    project.contains_key(PROJECT_FIELD)
}

/// Chats a project has deleted; they never appear in lists again.
pub(crate) fn deleted_chat(project: &Map<String, Value>, chat_id: &str) -> bool {
    project.get(CHAT_FIELD).and_then(|value| value.get(chat_id)).is_some()
}

impl ProjectManager {
    fn refuse_while_writing_original(&self, project: &Map<String, Value>) -> Result<(), ProjectFailure> {
        #[cfg(any(unix, windows))]
        if self.application_confirmations.has_pending() {
            return Err(ProjectFailure::recovery("close the application confirmation before deleting"));
        }
        // Only a write into the user's original files stays exclusive; it ends
        // in seconds and must not be cut off.
        if self.application_running()
            || project.get("application_intent").is_some_and(|intent| intent["state"] == "applying")
        {
            return Err(ProjectFailure::recovery("finish or undo the update of the original files first"));
        }
        Ok(())
    }

    /// Every app-owned workspace the project ever made: its snapshots and each
    /// task's copy. Locks are written for all of them.
    fn project_worktrees(&self, storage: &Storage, project_id: &str, project: &Map<String, Value>) -> Result<Vec<String>, ProjectFailure> {
        let mut ids: Vec<String> = Vec::new();
        let mut push = |id: Option<&str>| {
            if let Some(id) = id.filter(|id| valid_worktree_id(id)) {
                if !ids.iter().any(|known| known == id) { ids.push(id.to_owned()); }
            }
        };
        push(project.get("worktree_id").and_then(Value::as_str));
        for lock in storage.projection_records("lock")? {
            if lock.get("project_id").and_then(Value::as_str) == Some(project_id) {
                push(lock.get("lock_id").and_then(Value::as_str).and_then(|id| id.strip_prefix("lock_")));
            }
        }
        for chat in storage.projection_records("chat")? {
            if chat.get("project_id").and_then(Value::as_str) == Some(project_id) {
                push(chat.get("worktree_id").and_then(Value::as_str));
            }
        }
        if let Some(baselines) = project.get("source_baselines").and_then(Value::as_object) {
            for id in baselines.keys() { push(Some(id)); }
        }
        Ok(ids)
    }

    /// Tombstones the project at once; files follow in `finish_deletions`.
    pub(crate) fn delete_project(&mut self, storage: &mut Storage, project_id: &str, request_id: &str) -> Result<ProjectAction, ProjectFailure> {
        let mut project = registered_project(storage, project_id)?;
        if !is_deleted(&project) {
            self.refuse_while_writing_original(&project)?;
            let worktrees = self.project_worktrees(storage, project_id, &project)?;
            project.insert("canonical_path".into(), json!(deleted_marker(project_id)));
            project.insert("catalog_hidden".into(), json!(true));
            project.insert(PROJECT_FIELD.into(), json!({"state":"pending","worktrees":worktrees}));
            append_object(storage, "project", project_id, &format!("op_project_delete_{request_id}"), request_id, Value::Object(project))?;
        }
        self.release_watchers_for(project_id);
        Ok(ProjectAction { result: json!({"schemaVersion":1,"action":"deleteProject","projectId":project_id}), ..Default::default() })
    }

    /// Records the task as deleted; its workspace follows in `finish_deletions`.
    pub(crate) fn delete_chat(&mut self, storage: &mut Storage, project_id: &str, chat_id: &str, request_id: &str) -> Result<ProjectAction, ProjectFailure> {
        let mut project = registered_project(storage, project_id)?;
        if !deleted_chat(&project, chat_id) { self.refuse_while_writing_original(&project)?; }
        let chat = storage.projection_record("chat", chat_id)?
            .filter(|chat| chat.get("project_id").and_then(Value::as_str) == Some(project_id))
            .ok_or_else(|| ProjectFailure::manifest("chat does not belong to project"))?;
        let mut project_closed = false;
        if !deleted_chat(&project, chat_id) {
            let worktree = chat.get("worktree_id").and_then(Value::as_str).filter(|id| valid_worktree_id(id)).map(str::to_owned);
            // The latest task works in the project's own current copy. Close
            // that copy so the project reopens from the original and the
            // deleted task's files can go now (a later open makes a new copy).
            let shares_project_copy = worktree.is_some()
                && project.get("lifecycle").and_then(Value::as_str) == Some("active")
                && project.get("worktree_id").and_then(Value::as_str) == worktree.as_deref();
            let jobs = project.entry(CHAT_FIELD).or_insert_with(|| json!({})).as_object_mut()
                .ok_or_else(|| ProjectFailure::manifest("invalid chat deletion record"))?;
            jobs.insert(chat_id.into(), json!({"state":"pending","worktreeId":worktree}));
            append_object(storage, "project", project_id, &format!("op_chat_delete_{request_id}"), request_id, Value::Object(project))?;
            // Only a quiet copy is closed: closing under a still-running task
            // would break that run's own clean finish. Otherwise the files
            // wait and `finish_deletions` closes the copy later.
            if shares_project_copy && worktree.as_deref().is_some_and(|id| self.worktree_is_quiet(storage, id, &[chat_id.to_owned()]).is_ok()) {
                project_closed = self.close_project(storage, project_id, &format!("{request_id}_close")).is_ok();
            }
        }
        self.watchers.remove(&format!("workspace:{chat_id}"));
        Ok(ProjectAction { result: json!({"schemaVersion":1,"action":"deleteChat","projectId":project_id,"chatId":chat_id,"projectClosed":project_closed}), ..Default::default() })
    }

    fn release_watchers_for(&mut self, project_id: &str) {
        self.watchers.retain(|key, watcher| {
            key != project_id && watcher.workspace.as_ref().is_none_or(|workspace| workspace.project_id != project_id)
        });
    }

    /// A workspace may go only when nothing can still write into it: every run
    /// that used it ended with proof its processes are gone, and its lock holds
    /// no writer, poison or recovery.
    fn worktree_is_quiet(&self, storage: &Storage, worktree_id: &str, chat_ids: &[String]) -> Result<(), ProjectFailure> {
        if let Some(lock) = storage.projection_record("lock", &format!("lock_{worktree_id}"))? {
            if lock["writer_run_id"] != Value::Null || lock["poisoned"] != false || lock["recovery_required"] != false {
                return Err(denied("a workspace still has a writer or needs recovery"));
            }
        }
        for run in storage.projection_records("run")? {
            let in_scope = run["record"]["worktreeId"] == worktree_id
                || run["chat_id"].as_str().is_some_and(|id| chat_ids.iter().any(|chat| chat == id));
            if !in_scope { continue; }
            let evidence = if run["source_kind"] == "actualAgent" { &run["termination"] } else { &run["record"]["terminationEvidence"] };
            if !matches!(run["state"].as_str(), Some("succeeded" | "failed" | "canceled"))
                || evidence["processesProvenGone"] != true
                || run.get("recovery").is_some_and(|value| !value.is_null() && !matches!(value["state"].as_str(), Some("resolved" | "resolvedAbandoned")))
            {
                return Err(denied("a run has not provably exited yet"));
            }
        }
        Ok(())
    }

    /// A deleted task's workspace may still be the project's current snapshot
    /// (the latest task reuses it), another live task's copy, or an unbound
    /// prepared workspace. Those files wait until nothing live refers to them.
    fn is_project_copy(project: &Map<String, Value>, worktree_id: &str) -> bool {
        !is_deleted(project)
            && project.get("lifecycle").and_then(Value::as_str) == Some("active")
            && project.get("worktree_id").and_then(Value::as_str) == Some(worktree_id)
    }

    /// Other live users of a deleted task's workspace: another task that is not
    /// deleted, or an unbound prepared workspace. (The project's own current
    /// copy is handled by `is_project_copy`.)
    fn worktree_still_used(&self, storage: &Storage, project: &Map<String, Value>, worktree_id: &str, chat_id: &str) -> Result<bool, ProjectFailure> {
        // A deleted project removes all of its workspaces itself.
        if is_deleted(project) { return Ok(false); }
        if project.get("prepared_workspaces").and_then(Value::as_object).is_some_and(|items| items.values().any(|item| {
            item["manifest"]["worktreeId"] == worktree_id && !matches!(item["bindingState"].as_str(), Some("bound" | "abandoned"))
        })) { return Ok(true); }
        let project_id = project.get("project_id").and_then(Value::as_str);
        Ok(storage.projection_records("chat")?.iter().any(|other| {
            other.get("worktree_id").and_then(Value::as_str) == Some(worktree_id)
                && other.get("chat_id").and_then(Value::as_str) != Some(chat_id)
                && !(other.get("project_id").and_then(Value::as_str) == project_id
                    && other.get("chat_id").and_then(Value::as_str).is_some_and(|id| deleted_chat(project, id)))
        }))
    }

    #[cfg(unix)]
    fn remove_worktree_files(&mut self, worktree_id: &str) -> Result<(), ProjectFailure> {
        if !valid_worktree_id(worktree_id) { return Err(denied("invalid workspace identity")); }
        let quarantine = format!("delete_{worktree_id}");
        match self.worktrees_dir.symlink_metadata(worktree_id) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                if self.worktrees_dir.symlink_metadata(&quarantine).is_ok() {
                    return Err(denied("an earlier deletion of this workspace is unfinished"));
                }
                self.worktrees_dir.rename(worktree_id, &self.worktrees_dir, &quarantine).map_err(io)?;
                sync_capability_directory(&self.worktrees_dir)?;
            }
            Ok(_) => return Err(denied("workspace is not a plain directory")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(error)),
        }
        match self.worktrees_dir.symlink_metadata(&quarantine) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                let root = open_real_child_dir(&self.worktrees_dir, &quarantine)?;
                let identity = capability_directory_identity(&root)?;
                workspace_cleanup::walk(&root, true, 0, &mut 0)?;
                if capability_directory_identity(&open_real_child_dir(&self.worktrees_dir, &quarantine)?)? != identity {
                    return Err(denied("workspace changed while it was being deleted"));
                }
                self.worktrees_dir.remove_dir(&quarantine).map_err(io)?;
                sync_capability_directory(&self.worktrees_dir)?;
            }
            Ok(_) => return Err(denied("workspace is not a plain directory")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(error)),
        }
        if let Ok(manifest) = read_bounded_json(&self.manifests_dir, &format!("{worktree_id}.json")) {
            self.forget_task_worktree(&manifest);
        }
        remove_plain_file(&self.manifests_dir, &format!("{worktree_id}.json"))?;
        match self.data_dir.symlink_metadata(BASELINE_DIRECTORY) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                let baselines = open_real_child_dir(&self.data_dir, BASELINE_DIRECTORY)?;
                remove_plain_file(&baselines, &format!("{worktree_id}.source-baseline-v1.json"))?;
            }
            Ok(_) => return Err(denied("baseline directory is not a plain directory")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(error)),
        }
        Ok(())
    }
    #[cfg(not(unix))]
    fn remove_worktree_files(&mut self, _: &str) -> Result<(), ProjectFailure> {
        Err(denied("workspace deletion is not available on this platform yet"))
    }

    pub(crate) fn project_view_open(&self, project_id: &str) -> bool {
        self.watchers.contains_key(project_id)
    }

    pub(crate) fn deletion_busy(&self) -> bool {
        self.application_running() || self.preparation_cancel.is_some() || !self.pending_saves.is_empty()
    }

    /// Finishes every pending deletion it can; the rest stay pending for the
    /// next call. Returns how many are still pending.
    pub(crate) fn finish_deletions(&mut self, storage: &mut Storage, request_id: &str) -> Result<usize, ProjectFailure> {
        if self.deletion_busy() { return Ok(self.pending_deletions(storage)?); }
        let mut index = 0usize;
        for project in storage.projection_records("project")? {
            let Some(project_id) = project.get("project_id").and_then(Value::as_str).map(str::to_owned) else { continue };
            if project.get(PROJECT_FIELD).is_some_and(|job| job["state"] == "pending") {
                let worktrees: Vec<String> = project[PROJECT_FIELD]["worktrees"].as_array().into_iter().flatten()
                    .filter_map(|id| id.as_str().map(str::to_owned)).collect();
                let chats: Vec<String> = storage.projection_records("chat")?.iter()
                    .filter(|chat| chat.get("project_id").and_then(Value::as_str) == Some(&project_id))
                    .filter_map(|chat| chat.get("chat_id").and_then(Value::as_str).map(str::to_owned)).collect();
                let quiet = worktrees.iter().try_for_each(|id| self.worktree_is_quiet(storage, id, &chats));
                if quiet.is_ok() && worktrees.iter().try_for_each(|id| self.remove_worktree_files(id)).is_ok() {
                    let mut done = registered_project(storage, &project_id)?;
                    done[PROJECT_FIELD]["state"] = json!("deleted");
                    index += 1;
                    append_object(storage, "project", &project_id, &format!("op_project_deleted_{request_id}_{index}"), request_id, Value::Object(done))?;
                }
                continue;
            }
            let pending: Vec<(String, Option<String>)> = project.get(CHAT_FIELD).and_then(Value::as_object).into_iter().flatten()
                .filter(|(_, job)| job["state"] == "pending")
                .map(|(id, job)| (id.clone(), job["worktreeId"].as_str().map(str::to_owned))).collect();
            for (chat_id, worktree) in pending {
                let removed = match worktree.as_deref() {
                    None => Ok(()),
                    Some(id) if self.worktree_still_used(storage, &project, id, &chat_id)? => Err(denied("the workspace is still in use")),
                    Some(id) => self.worktree_is_quiet(storage, id, std::slice::from_ref(&chat_id)).and_then(|()| {
                        if Self::is_project_copy(&registered_project(storage, &project_id)?, id) {
                            // Still the project's current copy: close it only
                            // while no view holds the project open.
                            if self.watchers.contains_key(&project_id) { return Err(denied("the project is open on this workspace")); }
                            index += 1;
                            self.close_project(storage, &project_id, &format!("{request_id}_close_{index}"))?;
                        }
                        self.remove_worktree_files(id)
                    }),
                };
                if removed.is_ok() {
                    let mut done = registered_project(storage, &project_id)?;
                    done[CHAT_FIELD][chat_id.as_str()]["state"] = json!("deleted");
                    index += 1;
                    append_object(storage, "project", &project_id, &format!("op_chat_deleted_{request_id}_{index}"), request_id, Value::Object(done))?;
                }
            }
        }
        self.pending_deletions(storage)
    }

    /// True when the app itself removed this workspace through a deletion:
    /// the ledger lists it for deletion and nothing of it is left on disk. The
    /// removal only ever starts after every run there has provably ended, so
    /// this stands in for the workspace's own manifest where that is gone for
    /// good (also while another workspace of the same deletion still waits).
    pub(crate) fn workspace_removed_by_deletion(&self, storage: &Storage, address: &WorkspaceAddress) -> Result<bool, ProjectFailure> {
        if !valid_worktree_id(&address.worktree_id) { return Ok(false); }
        let Some(project) = storage.projection_record("project", &address.project_id)? else { return Ok(false) };
        let listed = |state: &Value| state == "pending" || state == "deleted";
        let by_project = project.get(PROJECT_FIELD).is_some_and(|job| {
            listed(&job["state"])
                && job["worktrees"].as_array().is_some_and(|ids| ids.iter().any(|id| id == address.worktree_id.as_str()))
        });
        let by_chat = project.get(CHAT_FIELD).and_then(|jobs| jobs.get(address.chat_id.as_str())).is_some_and(|job| {
            listed(&job["state"]) && job["worktreeId"] == address.worktree_id.as_str()
        });
        if !(by_project || by_chat) { return Ok(false); }
        let gone = |dir: &Dir, name: &str| matches!(dir.symlink_metadata(name), Err(error) if error.kind() == std::io::ErrorKind::NotFound);
        Ok(gone(&self.worktrees_dir, &address.worktree_id)
            && gone(&self.worktrees_dir, &format!("delete_{}", address.worktree_id))
            && gone(&self.manifests_dir, &format!("{}.json", address.worktree_id)))
    }

    pub(crate) fn pending_deletions(&self, storage: &Storage) -> Result<usize, ProjectFailure> {
        let mut count = 0;
        for project in storage.projection_records("project")? {
            if project.get(PROJECT_FIELD).is_some_and(|job| job["state"] == "pending") { count += 1; }
            count += project.get(CHAT_FIELD).and_then(Value::as_object).into_iter().flatten().filter(|(_, job)| job["state"] == "pending").count();
        }
        Ok(count)
    }
}

#[cfg(unix)]
fn remove_plain_file(dir: &Dir, name: &str) -> Result<(), ProjectFailure> {
    match dir.symlink_metadata(name) {
        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => dir.remove_file(name).map_err(io),
        Ok(_) => Err(denied("expected a file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io(error)),
    }
}
