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

fn denied() -> ProjectFailure {
    ProjectFailure::recovery(
        "chat erasure requires a discarded, idle chat without unresolved recovery",
    )
}

/// Erased chats stay out of chat lists even after hidden chats are restored.
pub(crate) fn erasure_recorded(project: &Map<String, Value>, chat_id: &Value) -> bool {
    chat_id.as_str().is_some_and(|id| project.get(FIELD).and_then(|jobs| jobs.get(id)).is_some())
}

fn job_state(project: &Map<String, Value>, chat_id: &str) -> Option<String> {
    project.get(FIELD)?.get(chat_id)?.get("state")?.as_str().map(str::to_owned)
}

impl ProjectManager {
    /// The same blockers as list removal and copy cleanup; any doubt denies.
    fn erasure_chat(&self, storage: &Storage, chat_id: &str) -> Result<String, ProjectFailure> {
        if !cfg!(unix)
            || self.application_running()
            || self.preparation_cancel.is_some()
            || !self.pending_saves.is_empty()
        {
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
            if !matches!(run["state"].as_str(), Some("succeeded" | "failed" | "canceled"))
                || evidence["processesProvenGone"] != true
                || run.get("recovery").is_some_and(|value| {
                    !value.is_null()
                        && !matches!(value["state"].as_str(), Some("resolved" | "resolvedAbandoned"))
                })
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
            let state = job_state(&project, id);
            if state.as_deref() == Some("erased") {
                continue;
            }
            let eligible = match state.as_deref() {
                Some("redacted") => cfg!(unix),
                _ => {
                    self.erasure_chat(storage, id).is_ok()
                        && storage.chat_erasure_precheck(id).is_ok()
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
            if self.erasure_chat(storage, chat_id)? != project_id {
                return Err(denied());
            }
            storage.chat_erasure_precheck(chat_id)?;
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
            if self.erasure_chat(storage, chat_id)? != project_id {
                return Err(denied());
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
            storage.redact_chat_content(ChatErasureCommit {
                chat_id: chat_id.to_owned(),
                project_id: project_id.clone(),
                expected_project_sequence: sequence,
                project_operation_id: format!("op_chat_erasure_redact_{request_id}"),
                request_id: request_id.to_owned(),
                project_payload: project,
            })?;
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

fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
