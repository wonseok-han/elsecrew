//! Selected permanent erasure of discarded chats' content
//! (docs/proposals/m5-chat-erasure.md). Explicit request only: none -> pending ->
//! redacted (single atomic ledger rewrite) -> erased (after WAL truncation).
use super::*;
use crate::storage::chat_erasure::ChatErasureCommit;

pub(crate) const FIELD: &str = "chat_erasure_v1";
const PREVIEW_CHARS: usize = 160;

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

#[cfg(test)]
thread_local! { static BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
pub(crate) fn simulate_busy(busy: bool) {
    BUSY.with(|value| value.set(busy));
}

const DENIED: &str = "chat erasure requires a discarded, idle chat without unresolved recovery";

/// Permanent property of the stored chat (withdraws a `pending` request).
fn denied() -> ProjectFailure {
    ProjectFailure::recovery(DENIED)
}
fn is_denial(failure: &ProjectFailure) -> bool {
    failure.detail == DENIED
}
/// Transient sidecar state; a `pending` request is kept for a later retry.
fn busy() -> ProjectFailure {
    ProjectFailure::recovery("chat erasure waits for applying, saving or preparation to finish")
}

/// Chats whose content was rewritten (`redacted`/`erased`) stay out of chat lists
/// even after hidden chats are restored. `pending` still has its content and
/// stays visible, so a refused or abandoned request never hides a chat.
pub(crate) fn erasure_recorded(project: &Map<String, Value>, chat_id: &Value) -> bool {
    chat_id.as_str().is_some_and(|id| {
        project.get(FIELD).and_then(|jobs| jobs.get(id)).and_then(|job| job.get("state"))
            .is_some_and(|state| state == "redacted" || state == "erased")
    })
}

fn job_state(project: &Map<String, Value>, chat_id: &str) -> Option<String> {
    project.get(FIELD)?.get(chat_id)?.get("state")?.as_str().map(str::to_owned)
}

impl ProjectManager {
    /// The same blockers as list removal and copy cleanup; any doubt denies.
    fn erasure_busy(&self) -> bool {
        #[cfg(test)]
        if BUSY.with(std::cell::Cell::get) {
            return true;
        }
        self.application_running() || self.preparation_cancel.is_some() || !self.pending_saves.is_empty()
    }

    /// Durable eligibility only. `denied()` is permanent; other errors (storage
    /// reads, invalid team state) are treated as transient by the retry path.
    fn erasure_chat(&self, storage: &Storage, chat_id: &str) -> Result<String, ProjectFailure> {
        if !cfg!(unix) {
            return Err(denied());
        }
        let chat = storage.projection_record("chat", chat_id)?.ok_or_else(denied)?;
        let record = &chat["record"];
        if chat["state"] != "discarded"
            || record["state"] != "discarded"
            || record["chatId"] != chat_id
            || chat["chat_id"] != chat_id
            || crate::orchestration::load(&chat)?.is_some_and(|state| state.status == "running")
            || chat
                .get("save_sessions")
                .is_some_and(|sessions| sessions.as_object().is_none_or(|items| !items.is_empty()))
        {
            return Err(denied());
        }
        let project_id = chat["project_id"].as_str().ok_or_else(denied)?.to_owned();
        let worktree_id = chat["worktree_id"].as_str().ok_or_else(denied)?.to_owned();
        if record["projectId"] != project_id.as_str() || record["worktreeId"] != worktree_id.as_str() {
            return Err(denied());
        }
        let project = registered_project(storage, &project_id)?;
        if project
            .get("application_intent")
            .is_some_and(|value| !value.is_null() && value["state"] != "abandoned")
        {
            return Err(denied());
        }
        if let Some(lock) = storage.projection_record("lock", &format!("lock_{worktree_id}"))? {
            if lock["writer_run_id"] != Value::Null
                || lock["poisoned"] != false
                || lock["recovery_required"] != false
            {
                return Err(denied());
            }
        }
        for run in storage.projection_records("run")? {
            if run["chat_id"] != chat_id && run["record"]["chatId"] != chat_id {
                continue;
            }
            let evidence = if run["source_kind"] == "actualAgent" {
                &run["termination"]
            } else {
                &run["record"]["terminationEvidence"]
            };
            // Termination evidence is always required; a recovery must be resolvedAbandoned
            // (the only resolved protocol state) and never substitutes for that evidence.
            if !matches!(run["state"].as_str(), Some("succeeded" | "failed" | "canceled"))
                || evidence["processesProvenGone"] != true
                || run.get("recovery").is_some_and(|value| !value.is_null() && value["state"] != "resolvedAbandoned")
            {
                return Err(denied());
            }
        }
        Ok(project_id)
    }

    pub(crate) fn chat_erasure_list(&self, storage: &Storage) -> Result<ProjectAction, ProjectFailure> {
        let mut entries = Vec::new();
        for chat in storage.projection_records("chat")? {
            let Some(id) = chat["chat_id"].as_str() else { continue };
            if chat["state"] != "discarded" {
                continue;
            }
            let project_id = chat["project_id"].as_str().ok_or_else(denied)?;
            let project = registered_project(storage, project_id)?;
            if super::project_deletion::is_deleted(&project) || super::project_deletion::deleted_chat(&project, id) {
                continue;
            }
            let state = job_state(&project, id);
            if state.as_deref() == Some("erased") {
                continue;
            }
            let eligible = match state.as_deref() {
                Some("redacted") => cfg!(unix),
                _ => {
                    !self.erasure_busy()
                        && self.erasure_chat(storage, id).is_ok()
                        && storage.chat_erasure_precheck(id, project_id).is_ok()
                }
            };
            let preview: String = chat["record"]["goal"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(PREVIEW_CHARS)
                .collect();
            entries.push(json!({
                "chatId":id,"projectId":project_id,"preview":preview,"eligible":eligible,
                "state":state.unwrap_or_else(|| "none".into()),
            }));
            if entries.len() >= 200 {
                break;
            }
        }
        Ok(ProjectAction {
            result: json!({"schemaVersion":1,"action":"chatErasureList","entries":entries}),
            ..Default::default()
        })
    }

    pub(crate) fn erase_chat_content(
        &mut self,
        storage: &mut Storage,
        chat_id: &str,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let done = || ProjectAction {
            result: json!({"schemaVersion":1,"action":"eraseChatContent","chatId":chat_id,"erased":true}),
            ..Default::default()
        };
        let chat = storage.projection_record("chat", chat_id)?.ok_or_else(denied)?;
        let project_id = chat["project_id"].as_str().ok_or_else(denied)?.to_owned();
        let project = registered_project(storage, &project_id)?;
        let mut state = job_state(&project, chat_id);
        if state.as_deref() == Some("erased") {
            return Ok(done());
        }
        if state.is_none() {
            if self.erasure_busy() {
                return Err(busy());
            }
            if self.erasure_chat(storage, chat_id)? != project_id {
                return Err(denied());
            }
            // Full dry run first: a refused chat leaves no durable request at all.
            storage.chat_erasure_precheck(chat_id, &project_id)?;
            let mut project = project.clone();
            project
                .entry(FIELD)
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(denied)?
                .insert(chat_id.into(), json!({"state":"pending","requestedAt":now_timestamp()}));
            append_object(
                storage,
                "project",
                &project_id,
                &format!("op_chat_erasure_intent_{request_id}"),
                request_id,
                Value::Object(project),
            )?;
            state = Some("pending".into());
        }
        checkpoint(1)?;
        if state.as_deref() == Some("pending") {
            // A pending request whose chat became ineligible (classification,
            // compaction, recovery, ...) is withdrawn instead of lingering.
            if self.erasure_busy() {
                return Err(busy()); // keep `pending`
            }
            let refused = match self.erasure_chat(storage, chat_id) {
                Ok(owner) if owner == project_id => match storage.chat_erasure_precheck(chat_id, &project_id) {
                    Ok(()) => None,
                    Err(error) if crate::storage::chat_erasure::is_erasure_refusal(&error) => Some(error.into()),
                    Err(error) => return Err(error.into()), // transient: keep `pending`
                },
                Ok(_) => Some(denied()),
                Err(error) if is_denial(&error) => Some(error),
                Err(error) => return Err(error), // transient: keep `pending`
            };
            if let Some(error) = refused {
                withdraw_pending(storage, &project_id, chat_id, request_id)?;
                return Err(error);
            }
            let mut project = registered_project(storage, &project_id)?;
            let sequence = project
                .get("last_event_sequence")
                .and_then(Value::as_u64)
                .ok_or_else(denied)?;
            let job = project
                .get_mut(FIELD)
                .and_then(|jobs| jobs.get_mut(chat_id))
                .and_then(Value::as_object_mut)
                .ok_or_else(denied)?;
            job.insert("state".into(), json!("redacted"));
            job.insert("redactedAt".into(), json!(now_timestamp()));
            let committed = storage.redact_chat_content(ChatErasureCommit {
                chat_id: chat_id.to_owned(),
                project_id: project_id.clone(),
                expected_project_sequence: sequence,
                project_operation_id: format!("op_chat_erasure_redact_{request_id}"),
                request_id: request_id.to_owned(),
                project_payload: project,
            });
            match committed {
                Ok(()) => {}
                Err(error) if crate::storage::chat_erasure::is_erasure_refusal(&error) => {
                    withdraw_pending(storage, &project_id, chat_id, request_id)?;
                    return Err(error.into());
                }
                // Transient (I/O, conflicting write): keep `pending` for a retry.
                Err(error) => return Err(error.into()),
            }
            state = Some("redacted".into());
        }
        if state.as_deref() != Some("redacted") {
            return Err(denied());
        }
        checkpoint(2)?;
        // Never report completion while pre-erasure page images may remain in the WAL.
        storage.purge_erased_pages()?;
        checkpoint(3)?;
        let mut project = registered_project(storage, &project_id)?;
        let job = project
            .get_mut(FIELD)
            .and_then(|jobs| jobs.get_mut(chat_id))
            .and_then(Value::as_object_mut)
            .filter(|job| job.get("state") == Some(&json!("redacted")))
            .ok_or_else(denied)?;
        job.insert("state".into(), json!("erased"));
        job.insert("completedAt".into(), json!(now_timestamp()));
        append_object(
            storage,
            "project",
            &project_id,
            &format!("op_chat_erasure_done_{request_id}"),
            request_id,
            Value::Object(project),
        )?;
        Ok(done())
    }
}

/// Remove a `pending` request (content untouched) so the chat is an ordinary
/// discarded chat again. Never applies to `redacted`/`erased`.
fn withdraw_pending(storage: &mut Storage, project_id: &str, chat_id: &str, request_id: &str) -> Result<(), ProjectFailure> {
    let mut project = registered_project(storage, project_id)?;
    if job_state(&project, chat_id).as_deref() != Some("pending") {
        return Err(denied());
    }
    project.get_mut(FIELD).and_then(Value::as_object_mut).ok_or_else(denied)?.remove(chat_id);
    append_object(
        storage,
        "project",
        project_id,
        &format!("op_chat_erasure_withdraw_{request_id}"),
        request_id,
        Value::Object(project),
    )
}

fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
