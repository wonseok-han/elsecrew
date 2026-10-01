use crate::project::{ProjectFailure, ProjectManager, WorkspaceAddress};
use crate::storage::{ChatBindInput, ChatBindOutcome, ProjectionUpsert, Storage};
use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use hello_agent_core::proctree::{
    identity_of, ProcIdentity, SystemClock, SystemProcTable, TerminationConfig, TreeTracker,
};
use hello_agent_protocol::run_recovery_v1::{
    validate_g5_payload_v1, G5_EVENT_PAGE_MAX_ITEMS_V1, G5_PAGE_MAX_ITEMS_V1,
    G5_RUN_START_CHAT_STATES_V1,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use uuid::Uuid;

const FIXTURE_PROFILES: &[(&str, &str, &str, Option<&str>, &str)] = &[
    ("planning", "codex", "sol", Some("medium"), "readOnly"),
    ("coding", "claude-code", "opus", None, "workspaceWrite"),
    ("review", "codex", "sol", Some("high"), "readOnly"),
];

#[derive(Debug, Default)]
pub struct G5Action {
    pub result: Value,
    pub events: Vec<G5Event>,
}

#[derive(Debug, Clone)]
pub struct G5Event {
    pub stream_id: String,
    pub seq: u64,
    pub event: &'static str,
    pub chat_id: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub payload: Value,
}

struct ActiveFixtureRun {
    child: Child,
    tracker: TreeTracker,
    chat_id: String,
    run_id: String,
    stream_id: String,
    next_wire_seq: u64,
    scenario: String,
}

#[derive(Default)]
pub struct G5Manager {
    active: HashMap<String, ActiveFixtureRun>,
    preparing_to_quit: bool,
}

struct RunCompletion<'a> {
    state: &'a str,
    exit_code: Option<i32>,
    proven: bool,
    tracked: &'a [ProcIdentity],
}

impl G5Manager {
    pub fn recover_unclean_runs(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Result<(), SidecarError> {
        for projection in storage.projection_records("run")? {
            if projection.get("source_kind").and_then(Value::as_str) == Some("actualAgent") {
                continue;
            }
            if projection.get("state").and_then(Value::as_str) != Some("recoveryRequired")
                || projection
                    .get("recovery")
                    .is_some_and(|value| !value.is_null())
            {
                continue;
            }
            let Some(run) = projection.get("record").cloned() else {
                continue;
            };
            let Some(identity) = projection.get("process_identity").cloned() else {
                continue;
            };
            let run_id = run["runId"].as_str().unwrap_or_default();
            let chat_id = run["chatId"].as_str().unwrap_or_default();
            let chat_projection = storage.projection_record("chat", chat_id)?.ok_or_else(|| {
                SidecarError::new("recoveryInvalid", "unclean run chat is missing")
            })?;
            let chat = chat_projection.get("record").cloned().ok_or_else(|| {
                SidecarError::new("recoveryInvalid", "unclean chat record is missing")
            })?;
            let recovery = json!({
                "schemaVersion":1,"recoveryId":format!("recovery_unclean_{run_id}"),
                "chatId":chat["chatId"],"taskId":chat["taskId"],"runId":run_id,
                "projectId":chat["projectId"],"worktreeId":chat["worktreeId"],
                "workspaceIdentity":chat["workspaceIdentity"],"reason":"uncleanShutdown",
                "state":"unresolved","processIdentities":[identity],"revision":1,
                "createdAt":now(),"resolvedAt":null,
            });
            projects
                .workspace_restore_recovery_required(
                    &address_from_chat(&chat)
                        .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?,
                )
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let mut payload = projection.clone();
            payload.remove("last_event_sequence");
            let recovered_run = update_run_state(
                &run,
                "recoveryRequired",
                Some(json!({
                    "outcome":"uncertain","processesProvenGone":false,
                    "exitCode":null,"signal":null,"observedAt":now(),
                })),
            )
            .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            payload.insert("record".to_string(), recovered_run);
            payload.insert("recovery".to_string(), recovery);
            storage.upsert_projection_record(
                "run",
                run_id,
                &format!("op_g5_unclean_recovery_{run_id}"),
                None,
                payload,
            )?;
            let recovered_chat = update_chat_state(&chat, "recoveryRequired")
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            persist_chat_state(
                storage,
                &recovered_chat,
                &format!("op_g5_unclean_chat_{run_id}"),
            )
            .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
        }
        Ok(())
    }

    pub fn default_role_settings(
        &self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
    ) -> Result<Value, ProjectFailure> {
        let mut project = project_record(storage, project_id)?;
        if let Some(settings) = project.get("role_settings") {
            return Ok(settings.clone());
        }
        let settings = settings_value(project_id, 1);
        project.insert("role_settings".to_string(), settings.clone());
        storage.upsert_projection_record(
            "project",
            project_id,
            &format!("op_g5_default_settings_{project_id}"),
            Some(request_id),
            project,
        )?;
        Ok(settings)
    }

    pub fn update_role_settings(
        &self,
        storage: &mut Storage,
        project_id: &str,
        expected_revision: u64,
        roles: Value,
        request_id: &str,
    ) -> Result<Value, ProjectFailure> {
        validate_g5_payload_v1(
            "projectRoleSettingsUpdate",
            &json!({"projectId":project_id,"expectedRevision":expected_revision,"roles":roles}),
        )
        .map_err(|error| ProjectFailure::manifest(format!("invalid role settings: {error}")))?;
        let mut project = project_record(storage, project_id)?;
        let current = project
            .get("role_settings")
            .cloned()
            .unwrap_or_else(|| settings_value(project_id, 1));
        if current.get("revision").and_then(Value::as_u64) != Some(expected_revision) {
            return Err(ProjectFailure::dirty(
                "project role settings changed; reload before saving",
            ));
        }
        let updated = json!({
            "schemaVersion":1,
            "projectId":project_id,
            "revision":expected_revision + 1,
            "roles":roles,
            "updatedAt":now(),
        });
        validate_g5_payload_v1("projectRoleSettings", &updated).map_err(|error| {
            ProjectFailure::manifest(format!(
                "updated role settings violate the contract: {error}"
            ))
        })?;
        project.insert("role_settings".to_string(), updated.clone());
        storage.upsert_projection_record(
            "project",
            project_id,
            &format!("op_g5_settings_update_{request_id}"),
            Some(request_id),
            project,
        )?;
        Ok(updated)
    }

    pub fn prepare_chat_workspace(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        project_id: &str,
        request_id: &str,
    ) -> Result<G5Action, ProjectFailure> {
        self.prepare_chat_workspace_with_baseline(storage, projects, project_id, request_id, false)
    }

    pub fn prepare_chat_workspace_with_baseline(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        project_id: &str,
        request_id: &str,
        application_baseline: bool,
    ) -> Result<G5Action, ProjectFailure> {
        let before = project_record(storage, project_id)?;
        let action = if application_baseline {
            projects.prepare_application_workspace(storage, project_id, request_id)?;
            // Reuse only the just-created active snapshot; normal preparation
            // remains fresh and existing snapshots are never retrofitted.
            projects.create_worktree(storage, project_id, request_id)?
        } else {
            projects.prepare_chat_workspace(storage, project_id, request_id)?
        };
        let manifest = action.result;
        let existing = prepared_workspace_records(storage)?;
        let worktree_id = manifest["worktreeId"].as_str().unwrap_or_default();
        let workspace_identity = manifest["workspaceIdentity"].as_str().unwrap_or_default();
        let chat_bindings = storage.projection_records("chat")?;
        if existing.iter().any(|prepared| {
            prepared["manifest"]["worktreeId"] == worktree_id
                || prepared["manifest"]["workspaceIdentity"] == workspace_identity
        }) || chat_bindings.iter().any(|chat| {
            chat.get("worktree_id").and_then(Value::as_str) == Some(worktree_id)
                || chat.get("workspace_identity").and_then(Value::as_str)
                    == Some(workspace_identity)
        }) {
            projects.abandon_unbound_workspace(
                storage,
                project_id,
                worktree_id,
                workspace_identity,
            )?;
            return Err(ProjectFailure::recovery(
                "fresh workspace identity collided with durable binding history",
            ));
        }
        let preparation_id = unique_preparation_id(&existing)?;
        let created = now();
        let prepared = json!({
            "schemaVersion":1,"preparationId":preparation_id,"manifest":manifest,
            "bindingState":"unbound","boundChatId":null,
            "createdAt":created,"updatedAt":created,
        });
        validate_g5_payload_v1("preparedChatWorkspace", &prepared).map_err(|error| {
            ProjectFailure::manifest(format!("prepared workspace is invalid: {error}"))
        })?;
        let mut project = project_record(storage, project_id)?;
        for field in ["role_settings", "prepared_workspaces", "source_baselines", "application_review", "application_chat_reviews", "application_backups", "application_intent"] {
            if !project.contains_key(field) {
                if let Some(value) = before.get(field).cloned() {
                    project.insert(field.to_string(), value);
                }
            }
        }
        let mut preparations = project
            .remove("prepared_workspaces")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        preparations.insert(preparation_id.clone(), prepared.clone());
        project.insert(
            "prepared_workspaces".to_string(),
            Value::Object(preparations),
        );
        if let Err(error) = storage.upsert_projection_record(
            "project",
            project_id,
            &format!("op_workspace_prepare_chat_{request_id}"),
            Some(request_id),
            project,
        ) {
            return match projects.abandon_unbound_workspace(
                storage,
                project_id,
                worktree_id,
                workspace_identity,
            ) {
                Ok(()) => Err(ProjectFailure::recovery(format!(
                    "prepared workspace persistence failed and its snapshot was abandoned: {error}"
                ))),
                Err(cleanup) => Err(ProjectFailure::ambiguous(format!(
                    "prepared workspace persistence failed and terminal abandonment could not be proven: {error}; cleanup: {}",
                    cleanup.detail
                ))),
            };
        }
        Ok(G5Action {
            result: prepared,
            ..G5Action::default()
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn abandon_prepared_workspace(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        preparation_id: &str,
        project_id: &str,
        worktree_id: &str,
        workspace_identity: &str,
        request_id: &str,
    ) -> Result<G5Action, ProjectFailure> {
        let request = json!({
            "preparationId":preparation_id,"projectId":project_id,
            "worktreeId":worktree_id,"workspaceIdentity":workspace_identity,
        });
        validate_g5_payload_v1("abandonPreparedWorkspaceRequest", &request).map_err(|error| {
            ProjectFailure::manifest(format!("invalid prepared abandon request: {error}"))
        })?;
        let mut project = project_record(storage, project_id)?;
        let mut preparations = project
            .remove("prepared_workspaces")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        let prepared = preparations
            .get(preparation_id)
            .cloned()
            .ok_or_else(|| ProjectFailure::manifest("prepared workspace is unknown"))?;
        let exact = prepared["manifest"]["projectId"] == project_id
            && prepared["manifest"]["worktreeId"] == worktree_id
            && prepared["manifest"]["workspaceIdentity"] == workspace_identity;
        if !exact {
            return Err(ProjectFailure::manifest(
                "prepared workspace abandon identity differs",
            ));
        }
        if prepared["bindingState"] != "unbound" {
            return Err(ProjectFailure::manifest(
                "only an unbound prepared workspace can be abandoned",
            ));
        }
        // Make the filesystem identity terminal before the durable projection. If SQLite fails,
        // the exact request can safely resume because the project method accepts only this same
        // already-abandoned identity; chat.create will reject its non-active manifest meanwhile.
        projects.abandon_unbound_workspace(storage, project_id, worktree_id, workspace_identity)?;
        let mut abandoned = prepared;
        abandoned["bindingState"] = Value::String("abandoned".to_string());
        abandoned["boundChatId"] = Value::Null;
        abandoned["manifest"]["lifecycle"] = Value::String("abandoned".to_string());
        abandoned["updatedAt"] = Value::String(now());
        validate_g5_payload_v1("preparedChatWorkspace", &abandoned).map_err(|error| {
            ProjectFailure::manifest(format!("abandoned preparation is invalid: {error}"))
        })?;
        preparations.insert(preparation_id.to_string(), abandoned);
        project.insert(
            "prepared_workspaces".to_string(),
            Value::Object(preparations),
        );
        storage.upsert_projection_record(
            "project",
            project_id,
            &format!("op_workspace_abandon_prepared_{request_id}"),
            Some(request_id),
            project,
        )?;
        let result = json!({
            "preparationId":preparation_id,"projectId":project_id,
            "worktreeId":worktree_id,"workspaceIdentity":workspace_identity,
            "bindingState":"abandoned","physicallyDeleted":false,
        });
        Ok(G5Action {
            result,
            ..G5Action::default()
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_chat(
        &self,
        storage: &mut Storage,
        projects: &ProjectManager,
        preparation_id: &str,
        project_id: &str,
        worktree_id: &str,
        workspace_identity: &str,
        goal: &str,
        expected_settings_revision: u64,
        request_id: &str,
    ) -> Result<G5Action, ProjectFailure> {
        self.create_chat_inner(
            storage,
            projects,
            preparation_id,
            project_id,
            worktree_id,
            workspace_identity,
            goal,
            expected_settings_revision,
            request_id,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_chat_inner(
        &self,
        storage: &mut Storage,
        projects: &ProjectManager,
        preparation_id: &str,
        project_id: &str,
        worktree_id: &str,
        workspace_identity: &str,
        goal: &str,
        expected_settings_revision: u64,
        request_id: &str,
        forced_chat_id: Option<&str>,
    ) -> Result<G5Action, ProjectFailure> {
        let request = json!({
            "preparationId":preparation_id,"projectId":project_id,"worktreeId":worktree_id,
            "workspaceIdentity":workspace_identity,"goal":goal,
            "expectedSettingsRevision":expected_settings_revision,
        });
        validate_g5_payload_v1("chatCreateRequest", &request)
            .map_err(|error| ProjectFailure::manifest(format!("invalid chat request: {error}")))?;
        let project = project_record(storage, project_id)?;
        let settings = project
            .get("role_settings")
            .cloned()
            .unwrap_or_else(|| settings_value(project_id, 1));
        if settings.get("revision").and_then(Value::as_u64) != Some(expected_settings_revision) {
            return Err(ProjectFailure::dirty(
                "project role settings changed before chat creation",
            ));
        }
        let chat_id = match forced_chat_id {
            Some(chat_id) => chat_id.to_string(),
            None => unique_chat_id(storage)?,
        };
        let task_id = format!("task_{}", Uuid::new_v4().simple());
        let address = WorkspaceAddress {
            chat_id: chat_id.clone(),
            project_id: project_id.to_string(),
            worktree_id: worktree_id.to_string(),
            workspace_identity: workspace_identity.to_string(),
        };
        // The chat row does not exist yet, so verify the snapshot independently first.
        verify_unbound_snapshot(
            projects,
            storage,
            project_id,
            worktree_id,
            workspace_identity,
        )?;
        let created = now();
        let chat = json!({
            "schemaVersion":1,"chatId":chat_id,"taskId":task_id,"projectId":project_id,
            "worktreeId":worktree_id,"workspaceIdentity":workspace_identity,"goal":goal,
            "settingsRevision":expected_settings_revision,"roleSnapshots":settings["roles"].clone(),
            "state":"draft","createdAt":created,"updatedAt":created,
        });
        validate_g5_payload_v1("chatRecord", &chat)
            .map_err(|error| ProjectFailure::manifest(format!("invalid created chat: {error}")))?;
        let message = team_message(&chat, 1, "user", None, None, goal)?;
        let payload = chat_projection(&chat, Some(message.clone()), 1)?;
        let outcome = storage.bind_prepared_chat_atomic(ChatBindInput {
            preparation_id: preparation_id.to_string(),
            project_id: project_id.to_string(),
            worktree_id: worktree_id.to_string(),
            workspace_identity: workspace_identity.to_string(),
            expected_settings_revision,
            chat_id: address.chat_id.clone(),
            request_id: request_id.to_string(),
            chat_payload: payload,
        })?;
        match outcome {
            ChatBindOutcome::Committed => {}
            ChatBindOutcome::SettingsDrift => {
                return Err(ProjectFailure::dirty(
                    "project role settings changed before binding",
                ))
            }
            ChatBindOutcome::PreparationUnknown => {
                return Err(ProjectFailure::manifest("prepared workspace is unknown"))
            }
            ChatBindOutcome::PreparationNotUnbound => {
                return Err(ProjectFailure::manifest(
                    "prepared workspace is no longer unbound",
                ))
            }
            ChatBindOutcome::PreparationMismatch => {
                return Err(ProjectFailure::manifest(
                    "prepared workspace identity differs",
                ))
            }
            ChatBindOutcome::BindingConflict => {
                return Err(ProjectFailure::manifest(
                    "global chat/workspace binding conflicts",
                ))
            }
        }
        Ok(G5Action {
            result: chat.clone(),
            events: vec![chat_event(&chat, 1, message)],
        })
    }

    /// Staged durable follow-up operation. Does not start or interrupt a provider.
    pub fn append_user_message(
        &self,
        storage: &mut Storage,
        projects: &ProjectManager,
        request: &Value,
    ) -> Result<G5Action, ProjectFailure> {
        validate_g5_payload_v1("chatAppendMessageRequest", request)
            .map_err(|error| ProjectFailure::manifest(error.to_string()))?;
        let chat_id = request["chatId"].as_str().unwrap();
        let mut stored = chat_projection_record(storage, chat_id)?;
        let mut chat = stored["record"].clone();
        for field in [
            "projectId",
            "chatId",
            "taskId",
            "worktreeId",
            "workspaceIdentity",
        ] {
            if chat[field] != request[field] {
                return Err(ProjectFailure::manifest("follow-up chat identity differs"));
            }
        }
        let validate_result = |message: &Value| {
            validate_g5_payload_v1(
                "chatAppendMessageTranscript",
                &json!({"request":request,"message":message}),
            )
            .map_err(|error| ProjectFailure::manifest(error.to_string()))
        };
        let message_id = request["clientMessageId"].as_str().unwrap();
        if let Some(message) = storage.retained_chat_message(message_id)? {
            validate_result(&message)?;
            return Ok(G5Action {
                result: message,
                events: vec![],
            });
        }
        if self.preparing_to_quit
            || !matches!(
                chat["state"].as_str(),
                Some(
                    "draft"
                        | "running"
                        | "waitingForUser"
                        | "reviewReady"
                        | "needsAttention"
                        | "canceled"
                )
            )
        {
            return Err(ProjectFailure::recovery(
                "chat does not accept new follow-up messages",
            ));
        }
        projects.verify_workspace(storage, &address_from_chat(&chat)?)?;
        let expected = request["expectedEventSequence"].as_u64().unwrap();
        if stored["durable_event_sequence"].as_u64() != Some(expected) {
            return Err(ProjectFailure::dirty(
                "chat message sequence changed before append",
            ));
        }
        let sequence = expected + 1; // The request validator excludes safe-integer overflow.
        chat["updatedAt"] = Value::String(now());
        let mut message = team_message(
            &chat,
            sequence,
            "user",
            None,
            None,
            request["text"].as_str().unwrap(),
        )?;
        message["messageId"] = request["clientMessageId"].clone();
        validate_result(&message)?;
        // Preserve save sessions and compare the full original row inside the write transaction.
        let expected_projection = stored.clone();
        stored.insert("record".to_string(), chat.clone());
        stored.insert("last_message".to_string(), message.clone());
        stored.insert("durable_event_sequence".to_string(), json!(sequence));
        if !storage.append_followup_checked(
            chat_id,
            &format!("op_followup_{message_id}"),
            &expected_projection,
            stored,
        )? {
            if let Some(message) = storage.retained_chat_message(message_id)? {
                validate_result(&message)?;
                return Ok(G5Action {
                    result: message,
                    events: vec![],
                });
            }
            return Err(ProjectFailure::dirty(
                "chat changed before follow-up commit",
            ));
        }
        Ok(G5Action {
            result: message.clone(),
            events: vec![chat_event(&chat, sequence, message)],
        })
    }

    pub fn list_chats(
        &self,
        storage: &Storage,
        project_id: &str,
        cursor: Option<&str>,
        limit: u64,
    ) -> Result<Value, ProjectFailure> {
        let limit = bounded_limit(limit, G5_PAGE_MAX_ITEMS_V1)?;
        let offset = parse_cursor(cursor)?;
        let mut items = storage
            .projection_records("chat")?
            .into_iter()
            .filter(|record| record.get("project_id").and_then(Value::as_str) == Some(project_id))
            .filter_map(|record| record.get("record").cloned())
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left["createdAt"]
                .as_str()
                .cmp(&right["createdAt"].as_str())
                .then_with(|| left["chatId"].as_str().cmp(&right["chatId"].as_str()))
        });
        page(
            items,
            offset,
            limit,
            |items, next| json!({"projectId":project_id,"items":items,"nextCursor":next}),
        )
    }

    pub fn get_chat(
        &self,
        storage: &Storage,
        chat_id: &str,
        after_event_sequence: u64,
        cursor: Option<&str>,
        limit: u64,
        include_agent_runs: bool,
    ) -> Result<Value, ProjectFailure> {
        let limit = bounded_limit(limit, G5_EVENT_PAGE_MAX_ITEMS_V1)?;
        let offset = parse_cursor(cursor)?;
        let record = chat_projection_record(storage, chat_id)?;
        let chat = record["record"].clone();
        let messages = storage
            .aggregate_event_payloads("chat", chat_id)?
            .into_iter()
            .filter_map(|payload| payload.get("last_message").cloned())
            .filter(|message| !message.is_null())
            .filter(|message| {
                message.get("eventSequence").and_then(Value::as_u64) > Some(after_event_sequence)
            })
            .collect::<Vec<_>>();
        let end = offset.saturating_add(limit).min(messages.len());
        if offset > messages.len() {
            return Err(ProjectFailure::manifest(
                "chat cursor is outside the durable event page",
            ));
        }
        let slice = messages[offset..end].to_vec();
        let next_event = slice
            .last()
            .and_then(|message| message.get("eventSequence"))
            .cloned()
            .unwrap_or(Value::Null);
        let runs = storage
            .projection_records("run")?
            .into_iter()
            .filter(|run| run.get("chat_id").and_then(Value::as_str) == Some(chat_id))
            .filter(|run| run.get("source_kind").and_then(Value::as_str) != Some("actualAgent"))
            .filter_map(|run| run.get("record").cloned())
            .take(G5_EVENT_PAGE_MAX_ITEMS_V1)
            .collect::<Vec<_>>();
        let actual_run_projections = storage
            .projection_records("run")?
            .into_iter()
            .filter(|run| run.get("chat_id").and_then(Value::as_str) == Some(chat_id))
            .filter(|run| run.get("source_kind").and_then(Value::as_str) == Some("actualAgent"))
            .collect::<Vec<_>>();
        let mut result = json!({
            "chat":chat,"afterEventSequence":after_event_sequence,"messages":slice,
            "runs":runs,"nextEventSequence":next_event,
        });
        if include_agent_runs {
            if actual_run_projections.len() > G5_EVENT_PAGE_MAX_ITEMS_V1 {
                return Err(ProjectFailure::recovery(
                    "actual-agent chat run ledger exceeds the bounded detail page",
                ));
            }
            let mut agent_runs = Vec::with_capacity(actual_run_projections.len());
            let mut unresolved_recoveries = Vec::new();
            let mut recovery_required_runs = 0_usize;
            for projection in actual_run_projections {
                let run = projection.get("record").cloned().ok_or_else(|| {
                    ProjectFailure::recovery("actual-agent run record is missing")
                })?;
                if projection.get("run_id") != run.get("runId")
                    || projection.get("chat_id") != run.get("chatId")
                    || run.get("chatId") != result["chat"].get("chatId")
                {
                    return Err(ProjectFailure::recovery(
                        "actual-agent run projection identity differs from its chat",
                    ));
                }
                if run.get("state").and_then(Value::as_str) == Some("recoveryRequired") {
                    recovery_required_runs += 1;
                }
                if let Some(recovery) = projection.get("recovery").filter(|value| !value.is_null())
                {
                    hello_agent_protocol::agent_adapter_v1::validate_agent_adapter_payload_v1(
                        "recoveryRecord",
                        recovery,
                    )
                    .map_err(|error| {
                        ProjectFailure::recovery(format!(
                            "invalid durable actual-agent recovery: {error}"
                        ))
                    })?;
                    for field in [
                        "chatId",
                        "taskId",
                        "projectId",
                        "worktreeId",
                        "workspaceIdentity",
                    ] {
                        if recovery.get(field) != result["chat"].get(field)
                            || recovery.get(field) != run.get(field)
                        {
                            return Err(ProjectFailure::recovery(
                                "actual-agent recovery identity differs from its chat or run",
                            ));
                        }
                    }
                    if recovery.get("runId") != run.get("runId") {
                        return Err(ProjectFailure::recovery(
                            "actual-agent recovery run identity differs",
                        ));
                    }
                    if recovery.get("state").and_then(Value::as_str) == Some("unresolved") {
                        unresolved_recoveries.push(recovery.clone());
                    }
                }
                agent_runs.push(run);
            }
            if unresolved_recoveries.len() > 1 {
                return Err(ProjectFailure::recovery(
                    "actual-agent chat has multiple unresolved recoveries",
                ));
            }
            if recovery_required_runs != unresolved_recoveries.len() {
                return Err(ProjectFailure::recovery(
                    "actual-agent recoveryRequired run and unresolved recovery cardinality differ",
                ));
            }
            result["agentRuns"] = Value::Array(agent_runs);
            result["agentRecovery"] = unresolved_recoveries.pop().unwrap_or(Value::Null);
            hello_agent_protocol::agent_adapter_v1::validate_agent_adapter_payload_v1(
                "agentChatDetailPage",
                &result,
            )
            .map_err(|error| {
                ProjectFailure::manifest(format!("invalid actual-agent chat detail: {error}"))
            })?;
        }
        Ok(result)
    }

    pub fn start_fixture(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        chat_id: &str,
        role_id: &str,
        scenario: &str,
        request_id: &str,
    ) -> Result<G5Action, ProjectFailure> {
        if self.preparing_to_quit {
            return Err(ProjectFailure::recovery("sidecar is preparing to quit"));
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (storage, projects, chat_id, role_id, scenario, request_id);
            return Err(ProjectFailure::recovery(
                "fixture execution is disabled until this platform has a verified process-tree contract",
            ));
        }
        #[cfg(target_os = "macos")]
        {
            self.start_fixture_macos(storage, projects, chat_id, role_id, scenario, request_id)
        }
    }

    #[cfg(target_os = "macos")]
    fn start_fixture_macos(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        chat_id: &str,
        role_id: &str,
        scenario: &str,
        request_id: &str,
    ) -> Result<G5Action, ProjectFailure> {
        let chat_projection = chat_projection_record(storage, chat_id)?;
        let chat = chat_projection["record"].clone();
        validate_g5_payload_v1(
            "fixtureRunStart",
            &json!({"chatId":chat_id,"roleId":role_id,"scenario":scenario}),
        )
        .map_err(|error| ProjectFailure::manifest(format!("invalid fixture run: {error}")))?;
        let state = chat["state"].as_str().unwrap_or_default();
        if !G5_RUN_START_CHAT_STATES_V1.contains(&state) {
            return Err(ProjectFailure::recovery(
                "chat state does not allow a new run",
            ));
        }
        if storage.projection_records("run")?.iter().any(|run| {
            run.get("chat_id").and_then(Value::as_str) == Some(chat_id)
                && run
                    .get("state")
                    .and_then(Value::as_str)
                    .is_some_and(|state| ["preparing", "running", "cancelling"].contains(&state))
        }) {
            return Err(ProjectFailure::recovery(
                "active roles are serialized within one chat workspace",
            ));
        }
        let profile = chat["roleSnapshots"]
            .as_array()
            .and_then(|roles| roles.iter().find(|role| role["roleId"] == role_id))
            .cloned()
            .ok_or_else(|| ProjectFailure::manifest("chat role snapshot is missing"))?;
        if !profile_is_fixture_available(&profile) {
            return Err(ProjectFailure::manifest(
                "the exact snapshotted runtime/model profile is unavailable; no fallback was used",
            ));
        }
        let address = address_from_chat(&chat)?;
        let manifest = projects.verify_workspace_for_run(storage, &address)?;
        let run_id = format!("run_{}", Uuid::new_v4().simple());
        let created = now();
        let stage = match role_id {
            "planning" => "planning",
            "coding" => "implementing",
            "review" => "verifying",
            _ => return Err(ProjectFailure::manifest("unknown fixture role")),
        };
        let run = json!({
            "schemaVersion":1,"runId":run_id,"chatId":chat_id,"taskId":chat["taskId"],
            "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
            "selectedProfile":profile,"fixtureScenario":scenario,"state":"preparing","stage":stage,
            "terminationEvidence":null,"createdAt":created,"updatedAt":created,
        });
        validate_g5_payload_v1("runRecord", &run).map_err(|error| {
            ProjectFailure::manifest(format!("invalid fixture run record: {error}"))
        })?;
        let run_payload = run_projection(&run, &manifest, None)?;
        let mut upserts = vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: format!("op_run_prepare_{request_id}"),
            request_id: Some(request_id.to_string()),
            payload: run_payload,
        }];
        if profile["permission"] == "workspaceWrite" {
            let lock_id = format!("lock_{}", address.worktree_id);
            let mut lock = storage
                .projection_record("lock", &lock_id)?
                .ok_or_else(|| ProjectFailure::manifest("workspace writer lock is missing"))?;
            if lock
                .get("writer_run_id")
                .is_some_and(|value| !value.is_null())
                || lock.get("poisoned").and_then(Value::as_bool) != Some(false)
                || lock.get("recovery_required").and_then(Value::as_bool) != Some(false)
            {
                return Err(ProjectFailure::recovery(
                    "workspace already has a writer lock",
                ));
            }
            lock.insert("writer_run_id".to_string(), Value::String(run_id.clone()));
            lock.insert(
                "evidence".to_string(),
                json!({"phase":"g5WriterHeld","workspace_identity":address.workspace_identity}),
            );
            upserts.push(ProjectionUpsert {
                aggregate_type: "lock",
                aggregate_id: lock_id,
                operation_id: format!("op_writer_lock_{request_id}"),
                request_id: Some(request_id.to_string()),
                payload: lock,
            });
        }
        storage.upsert_projection_records_atomic(upserts)?;

        let executable = std::env::current_exe().map_err(|error| {
            ProjectFailure::recovery(format!("could not resolve fixture executable: {error}"))
        })?;
        let mut command = Command::new(executable);
        command
            .args(["--fixture-child", scenario])
            .current_dir(
                manifest["worktreePath"]
                    .as_str()
                    .ok_or_else(|| ProjectFailure::manifest("workspace path is missing"))?,
            )
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.fail_before_fixture_release(
                    storage,
                    &chat,
                    &run,
                    &manifest,
                    &format!("op_fixture_spawn_failed_{request_id}"),
                )?;
                return Err(ProjectFailure::recovery(format!(
                    "could not start deterministic fixture: {error}"
                )));
            }
        };
        let mut fixture_release = child.stdin.take().ok_or_else(|| {
            ProjectFailure::recovery("fixture startup barrier could not be opened")
        })?;
        let identity = match identity_of(child.id()) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                self.fail_before_fixture_release(
                    storage,
                    &chat,
                    &run,
                    &manifest,
                    &format!("op_fixture_identity_failed_{request_id}"),
                )?;
                return Err(ProjectFailure::recovery(format!(
                    "fixture identity could not be proven before release: {error}"
                )));
            }
        };
        let mut tracker = TreeTracker::new(identity);
        let table = SystemProcTable;
        for _ in 0..5 {
            let _ = tracker.observe(&table);
            std::thread::sleep(Duration::from_millis(10));
        }
        let stream_id = format!("run_stream_{}", Uuid::new_v4().simple());
        let running = update_run_state(&run, "running", None)?;
        let mut running_projection = run_projection(&running, &manifest, None)?;
        running_projection.insert(
            "process_identity".to_string(),
            process_identity_value(identity),
        );
        storage.upsert_projection_record(
            "run",
            &run_id,
            &format!("op_run_running_{run_id}"),
            None,
            running_projection,
        )?;
        let running_chat = update_chat_state(&chat, "running")?;
        persist_chat_state(storage, &running_chat, &format!("op_chat_running_{run_id}"))?;
        let active = ActiveFixtureRun {
            child,
            tracker,
            chat_id: chat_id.to_string(),
            run_id: run_id.clone(),
            stream_id: stream_id.clone(),
            next_wire_seq: 1,
            scenario: scenario.to_string(),
        };
        if fixture_release.write_all(b"run").is_err() {
            drop(fixture_release);
            self.active.insert(run_id.clone(), active);
            let cancelled = self.cancel_run(storage, projects, chat_id, &run_id)?;
            return Err(ProjectFailure::recovery(format!(
                "fixture startup barrier closed before release; cleanup outcome: {}",
                cancelled.result["outcome"].as_str().unwrap_or("unknown")
            )));
        }
        drop(fixture_release);
        self.active.insert(run_id.clone(), active);
        Ok(G5Action {
            result: json!({"run":run,"streamId":stream_id}),
            events: vec![G5Event {
                stream_id,
                seq: 1,
                event: "run.event",
                chat_id: Some(chat_id.to_string()),
                task_id: chat["taskId"].as_str().map(str::to_string),
                run_id: Some(run_id),
                payload: json!({"kind":"status","text":format!("{role_id} fixture started")}),
            }],
        })
    }

    pub fn poll_runs(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Vec<G5Event> {
        let ids = self.active.keys().cloned().collect::<Vec<_>>();
        let mut events = Vec::new();
        for run_id in ids {
            let finished = match self.active.get_mut(&run_id) {
                Some(active) => {
                    let _ = active.tracker.observe(&SystemProcTable);
                    active.child.try_wait()
                }
                None => continue,
            };
            let Ok(Some(status)) = finished else {
                continue;
            };
            let Some(mut active) = self.active.remove(&run_id) else {
                continue;
            };
            let report = active.tracker.terminate(
                &SystemProcTable,
                &SystemClock::default(),
                termination_config(),
            );
            let proven = report.confirmed();
            let state = if status.success() && proven {
                "succeeded"
            } else if proven {
                "failed"
            } else {
                "recoveryRequired"
            };
            let observation = RunCompletion {
                state,
                exit_code: status.code(),
                proven,
                tracked: &report.tracked,
            };
            if let Ok(mut completion) = self.finish_run(storage, projects, &active, observation) {
                active.next_wire_seq += 1;
                completion.seq = active.next_wire_seq;
                events.push(completion);
            }
        }
        events
    }

    pub fn cancel_run(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        chat_id: &str,
        run_id: &str,
    ) -> Result<G5Action, ProjectFailure> {
        let mut active = self
            .active
            .remove(run_id)
            .ok_or_else(|| ProjectFailure::manifest("active run identity is missing"))?;
        if active.chat_id != chat_id {
            self.active.insert(run_id.to_string(), active);
            return Err(ProjectFailure::manifest("run belongs to another chat"));
        }
        let prepared = (|| {
            let run_projection = run_projection_record(storage, run_id)?;
            let run = run_projection["record"].clone();
            let chat = chat_projection_record(storage, chat_id)?["record"].clone();
            let cancelling = update_run_state(&run, "cancelling", None)?;
            persist_run(
                storage,
                &cancelling,
                &run_projection["workspace_manifest"],
                run_projection.get("recovery").cloned(),
                &format!("op_run_cancelling_{run_id}"),
            )?;
            let stopping = update_chat_state(&chat, "stopping")?;
            persist_chat_state(storage, &stopping, &format!("op_chat_stopping_{run_id}"))?;
            Ok::<(), ProjectFailure>(())
        })();
        if let Err(error) = prepared {
            self.active.insert(run_id.to_string(), active);
            return Err(error);
        }
        let report = active.tracker.terminate(
            &SystemProcTable,
            &SystemClock::default(),
            termination_config(),
        );
        let _ = active.child.wait();
        let forced_uncertain = active.scenario == "cancel-uncertain";
        let proven = report.confirmed() && !forced_uncertain;
        let state = if proven {
            "canceled"
        } else {
            "recoveryRequired"
        };
        let event = self.finish_run(
            storage,
            projects,
            &active,
            RunCompletion {
                state,
                exit_code: None,
                proven,
                tracked: &report.tracked,
            },
        )?;
        Ok(G5Action {
            result: json!({"outcome":state,"runId":run_id}),
            events: vec![event],
        })
    }

    pub fn prepare_quit(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Result<Vec<String>, ProjectFailure> {
        self.preparing_to_quit = true;
        let ids = self.active.keys().cloned().collect::<Vec<_>>();
        for run_id in ids {
            let chat_id = self.active[&run_id].chat_id.clone();
            let _ = self.cancel_run(storage, projects, &chat_id, &run_id);
        }
        let blockers = storage.blocker_ids()?;
        Ok(blockers)
    }

    pub fn list_recoveries(
        &self,
        storage: &Storage,
        cursor: Option<&str>,
        limit: u64,
    ) -> Result<Value, ProjectFailure> {
        let limit = bounded_limit(limit, G5_PAGE_MAX_ITEMS_V1)?;
        let offset = parse_cursor(cursor)?;
        let mut records = storage
            .projection_records("run")?
            .into_iter()
            .filter(|run| run.get("source_kind").and_then(Value::as_str) != Some("actualAgent"))
            .filter_map(|run| run.get("recovery").cloned())
            .filter(|recovery| !recovery.is_null())
            .map(|recovery| strip_internal_identity_fields(&recovery))
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left["recoveryId"]
                .as_str()
                .cmp(&right["recoveryId"].as_str())
        });
        page(
            records,
            offset,
            limit,
            |items, next| json!({"items":items,"nextCursor":next}),
        )
    }

    pub fn inspect_recovery(
        &self,
        storage: &Storage,
        recovery_id: &str,
    ) -> Result<Value, ProjectFailure> {
        find_recovery(storage, recovery_id)
            .map(|recovery| strip_internal_identity_fields(&recovery))
    }

    pub fn mark_recovery_resolved(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        recovery_id: &str,
        expected_revision: u64,
        resolution: &str,
        request_id: &str,
    ) -> Result<Value, ProjectFailure> {
        if resolution != "abandon" {
            return Err(ProjectFailure::manifest(
                "G5 recovery resolution is abandon-only",
            ));
        }
        let recovery = find_recovery(storage, recovery_id)?;
        if recovery["revision"].as_u64() != Some(expected_revision) {
            return Err(ProjectFailure::dirty("recovery revision changed"));
        }
        let mut identities = recovery["processIdentities"]
            .as_array()
            .cloned()
            .ok_or_else(|| ProjectFailure::manifest("recovery process identities are missing"))?;
        let mut all_gone = true;
        for identity in &mut identities {
            let pid = identity["pid"].as_u64().unwrap_or(0) as u32;
            let start_us = identity["startTimeMicros"].as_i64().or_else(|| {
                identity["startTime"]
                    .as_str()
                    .and_then(parse_identity_timestamp)
            });
            let gone = start_us.is_some_and(|start_us| {
                matches!(
                    hello_agent_core::proctree::ProcTable::probe(
                        &SystemProcTable,
                        ProcIdentity { pid, start_us }
                    ),
                    hello_agent_core::proctree::ProbeResult::Gone
                )
            });
            identity
                .as_object_mut()
                .expect("identity object")
                .insert("observedGone".to_string(), Value::Bool(gone));
            all_gone &= gone;
        }
        if !all_gone {
            return Err(ProjectFailure::recovery(
                "tracked processes are still present or cannot be proven gone",
            ));
        }
        let run_id = recovery["runId"].as_str().unwrap_or_default();
        let stored_run = run_projection_record(storage, run_id)?;
        let address = WorkspaceAddress {
            chat_id: recovery["chatId"].as_str().unwrap_or_default().to_string(),
            project_id: recovery["projectId"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            worktree_id: recovery["worktreeId"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            workspace_identity: recovery["workspaceIdentity"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        };
        projects.workspace_abandon_recovery(&address)?;
        let mut stored_chat = chat_projection_record(storage, &address.chat_id)?;
        let chat = stored_chat["record"].clone();
        let discarded = update_chat_state(&chat, "discarded")?;
        let mut resolved = recovery.clone();
        resolved["state"] = Value::String("resolvedAbandoned".to_string());
        resolved["revision"] = Value::from(expected_revision + 1);
        resolved["resolvedAt"] = Value::String(now());
        resolved["processIdentities"] = Value::Array(
            identities
                .into_iter()
                .map(|mut identity| {
                    identity
                        .as_object_mut()
                        .map(|object| object.remove("startTimeMicros"));
                    identity
                })
                .collect(),
        );
        validate_g5_payload_v1("recoveryRecord", &resolved).map_err(|error| {
            ProjectFailure::manifest(format!("resolved recovery is invalid: {error}"))
        })?;
        let resolved_run = update_run_state(
            &stored_run["record"],
            "canceled",
            Some(json!({
                "outcome":"canceled","processesProvenGone":true,"exitCode":null,
                "signal":"SIGTERM","observedAt":now(),
            })),
        )?;
        let mut run_payload = run_projection(
            &resolved_run,
            &stored_run["workspace_manifest"],
            Some(resolved.clone()),
        )?;
        if let Some(identity) = stored_run.get("process_identity").cloned() {
            run_payload.insert("process_identity".to_string(), identity);
        }
        stored_chat.remove("last_event_sequence");
        stored_chat.insert("record".to_string(), discarded.clone());
        stored_chat.insert("state".to_string(), discarded["state"].clone());
        let mut upserts = vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run_id.to_string(),
                operation_id: format!("op_recovery_resolved_{request_id}"),
                request_id: Some(request_id.to_string()),
                payload: run_payload,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: address.chat_id.clone(),
                operation_id: format!("op_recovery_chat_{request_id}"),
                request_id: Some(request_id.to_string()),
                payload: stored_chat,
            },
        ];
        if stored_run["record"]["selectedProfile"]["permission"] == "workspaceWrite" {
            let (lock_id, lock) = writer_lock_update_payload(storage, &chat, run_id, false)?;
            upserts.push(ProjectionUpsert {
                aggregate_type: "lock",
                aggregate_id: lock_id,
                operation_id: format!("op_recovery_lock_{request_id}"),
                request_id: Some(request_id.to_string()),
                payload: lock,
            });
        }
        storage.upsert_projection_records_atomic(upserts)?;
        Ok(resolved)
    }

    fn finish_run(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        active: &ActiveFixtureRun,
        completion: RunCompletion<'_>,
    ) -> Result<G5Event, ProjectFailure> {
        let RunCompletion {
            state,
            exit_code,
            proven,
            tracked,
        } = completion;
        let stored_run = run_projection_record(storage, &active.run_id)?;
        let run = stored_run["record"].clone();
        let stored_chat = chat_projection_record(storage, &active.chat_id)?;
        let chat = stored_chat["record"].clone();
        let evidence = json!({
            "outcome":match state {"succeeded"=>"cleanExit","failed"=>"failed","canceled"=>"canceled",_=>"uncertain"},
            "processesProvenGone":proven,"exitCode":exit_code,
            "signal":if state == "canceled" || state == "recoveryRequired" {Value::String("SIGTERM".to_string())} else {Value::Null},
            "observedAt":now(),
        });
        let terminal = update_run_state(&run, state, Some(evidence))?;
        let mut recovery = None;
        if state == "recoveryRequired" {
            projects.workspace_mark_recovery_required(storage, &address_from_chat(&chat)?)?;
            let identities = tracked
                .iter()
                .copied()
                .map(process_identity_value)
                .collect::<Vec<_>>();
            let value = json!({
                "schemaVersion":1,"recoveryId":format!("recovery_{}",Uuid::new_v4().simple()),
                "chatId":chat["chatId"],"taskId":chat["taskId"],"runId":active.run_id,
                "projectId":chat["projectId"],"worktreeId":chat["worktreeId"],
                "workspaceIdentity":chat["workspaceIdentity"],
                "reason":if active.scenario == "cancel-uncertain" {"cancelUncertain"} else {"processIdentityUncertain"},
                "state":"unresolved","processIdentities":identities,"revision":1,
                "createdAt":now(),"resolvedAt":null,
            });
            validate_g5_payload_v1("recoveryRecord", &strip_internal_identity_fields(&value))
                .map_err(|error| ProjectFailure::manifest(format!("invalid recovery: {error}")))?;
            recovery = Some(value);
        }
        let chat_state = match state {
            "succeeded" if run["selectedProfile"]["roleId"] == "planning" => "waitingForUser",
            "succeeded" => "reviewReady",
            "canceled" => "canceled",
            "recoveryRequired" => "recoveryRequired",
            _ => "needsAttention",
        };
        let updated_chat = update_chat_state(&chat, chat_state)?;
        let sequence = stored_chat["durable_event_sequence"].as_u64().unwrap_or(0) + 1;
        let text = match state {
            "succeeded" => format!(
                "{} fixture completed",
                run["selectedProfile"]["roleId"].as_str().unwrap_or("role")
            ),
            "canceled" => {
                "Fixture canceled after all tracked processes were proven gone".to_string()
            }
            "recoveryRequired" => {
                "Fixture termination is uncertain; recovery is required".to_string()
            }
            _ => "Fixture failed after process cleanup".to_string(),
        };
        let message = team_message(
            &updated_chat,
            sequence,
            if state == "succeeded" {
                "role"
            } else {
                "status"
            },
            (state == "succeeded").then(|| {
                run["selectedProfile"]["roleId"]
                    .as_str()
                    .unwrap_or("planning")
            }),
            Some(active.run_id.as_str()),
            &text,
        )?;
        let mut run_payload = run_projection(
            &terminal,
            &stored_run["workspace_manifest"],
            recovery.clone(),
        )?;
        if let Some(identity) = stored_run.get("process_identity").cloned() {
            run_payload.insert("process_identity".to_string(), identity);
        }
        let mut chat_payload = chat_projection(&updated_chat, Some(message.clone()), sequence)?;
        if let Some(sessions) = stored_chat.get("save_sessions").cloned() {
            chat_payload.insert("save_sessions".to_string(), sessions);
        }
        let mut upserts = vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: active.run_id.clone(),
                operation_id: format!("op_run_terminal_{}", active.run_id),
                request_id: None,
                payload: run_payload,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: active.chat_id.clone(),
                operation_id: format!("op_chat_message_{}_{}", active.run_id, sequence),
                request_id: None,
                payload: chat_payload,
            },
        ];
        if run["selectedProfile"]["permission"] == "workspaceWrite" {
            let (lock_id, lock) = writer_lock_update_payload(
                storage,
                &chat,
                &active.run_id,
                state == "recoveryRequired",
            )?;
            upserts.push(ProjectionUpsert {
                aggregate_type: "lock",
                aggregate_id: lock_id,
                operation_id: format!("op_writer_terminal_{}", active.run_id),
                request_id: None,
                payload: lock,
            });
        }
        storage.upsert_projection_records_atomic(upserts)?;
        Ok(G5Event {
            stream_id: active.stream_id.clone(),
            seq: active.next_wire_seq + 1,
            event: "run.finished",
            chat_id: Some(active.chat_id.clone()),
            task_id: chat["taskId"].as_str().map(str::to_string),
            run_id: Some(active.run_id.clone()),
            payload: json!({"run":terminal,"message":message,"recovery":recovery.map(|value| strip_internal_identity_fields(&value))}),
        })
    }

    fn fail_before_fixture_release(
        &self,
        storage: &mut Storage,
        chat: &Value,
        run: &Value,
        workspace_manifest: &Value,
        operation_id: &str,
    ) -> Result<(), ProjectFailure> {
        let failed = update_run_state(
            run,
            "failed",
            Some(json!({
                "outcome":"failed","processesProvenGone":true,"exitCode":null,
                "signal":null,"observedAt":now(),
            })),
        )?;
        let needs_attention = update_chat_state(chat, "needsAttention")?;
        let chat_id = chat["chatId"].as_str().unwrap_or_default();
        let mut chat_payload = chat_projection_record(storage, chat_id)?;
        chat_payload.remove("last_event_sequence");
        chat_payload.insert("record".to_string(), needs_attention.clone());
        chat_payload.insert("state".to_string(), needs_attention["state"].clone());
        let mut upserts = vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run["runId"].as_str().unwrap_or_default().to_string(),
                operation_id: operation_id.to_string(),
                request_id: None,
                payload: run_projection(&failed, workspace_manifest, None)?,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: chat_id.to_string(),
                operation_id: format!("{operation_id}_chat"),
                request_id: None,
                payload: chat_payload,
            },
        ];
        if run["selectedProfile"]["permission"] == "workspaceWrite" {
            let lock_id = format!("lock_{}", run["worktreeId"].as_str().unwrap_or_default());
            let mut lock = storage
                .projection_record("lock", &lock_id)?
                .ok_or_else(|| ProjectFailure::manifest("workspace writer lock is missing"))?;
            lock.insert("writer_run_id".to_string(), Value::Null);
            lock.insert(
                "evidence".to_string(),
                json!({"phase":"g5PreReleaseFailed","run_id":run["runId"]}),
            );
            upserts.push(ProjectionUpsert {
                aggregate_type: "lock",
                aggregate_id: lock_id,
                operation_id: format!("{operation_id}_lock"),
                request_id: None,
                payload: lock,
            });
        }
        storage.upsert_projection_records_atomic(upserts)?;
        Ok(())
    }
}

pub fn run_fixture_child(scenario: &str) -> Result<(), SidecarError> {
    if ![
        "planning-success",
        "coding-success",
        "review-success",
        "cancel-clean",
        "cancel-uncertain",
    ]
    .contains(&scenario)
    {
        return Err(SidecarError::new(
            "invalidArguments",
            "unknown fixture scenario",
        ));
    }
    let mut release = [0_u8; 3];
    if std::io::stdin().read_exact(&mut release).is_err() || &release != b"run" {
        return Ok(());
    }
    if scenario.starts_with("cancel-") {
        let executable = std::env::current_exe().map_err(|error| {
            SidecarError::new("fixtureFailure", format!("fixture executable: {error}"))
        })?;
        let mut leaf = Command::new(executable)
            .arg("--fixture-leaf")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                SidecarError::new("fixtureFailure", format!("fixture leaf: {error}"))
            })?;
        let _ = leaf.wait();
    } else {
        std::thread::sleep(Duration::from_millis(120));
    }
    Ok(())
}

pub fn run_fixture_leaf() {
    std::thread::sleep(Duration::from_secs(30));
}

fn settings_value(project_id: &str, revision: u64) -> Value {
    json!({
        "schemaVersion":1,"projectId":project_id,"revision":revision,
        "roles":[
            {"roleId":"planning","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"},
            {"roleId":"coding","runtime":"claude-code","modelSelector":"opus","reasoningEffort":null,"permission":"workspaceWrite"},
            {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"high","permission":"readOnly"}
        ],"updatedAt":now(),
    })
}

fn project_record(
    storage: &Storage,
    project_id: &str,
) -> Result<Map<String, Value>, ProjectFailure> {
    storage
        .projection_record("project", project_id)?
        .ok_or_else(|| ProjectFailure::manifest("project projection is missing"))
}

fn prepared_workspace_records(storage: &Storage) -> Result<Vec<Value>, ProjectFailure> {
    Ok(storage
        .projection_records("project")?
        .into_iter()
        .filter_map(|project| project.get("prepared_workspaces").cloned())
        .filter_map(|value| value.as_object().cloned())
        .flat_map(|preparations| preparations.into_values())
        .collect())
}

fn unique_preparation_id(existing: &[Value]) -> Result<String, ProjectFailure> {
    for _ in 0..8 {
        let candidate = format!("preparation_{}", Uuid::new_v4().simple());
        if existing.iter().all(|prepared| {
            prepared.get("preparationId").and_then(Value::as_str) != Some(candidate.as_str())
        }) {
            return Ok(candidate);
        }
    }
    Err(ProjectFailure::recovery(
        "could not allocate a globally unique preparation identity",
    ))
}

fn unique_chat_id(storage: &Storage) -> Result<String, ProjectFailure> {
    let chats = storage.projection_records("chat")?;
    let prepared = prepared_workspace_records(storage)?;
    for _ in 0..8 {
        let candidate = format!("chat_{}", Uuid::new_v4().simple());
        let existing_chat = chats
            .iter()
            .any(|chat| chat.get("chat_id").and_then(Value::as_str) == Some(candidate.as_str()));
        let existing_binding = prepared.iter().any(|record| {
            record.get("boundChatId").and_then(Value::as_str) == Some(candidate.as_str())
        });
        if !existing_chat && !existing_binding {
            return Ok(candidate);
        }
    }
    Err(ProjectFailure::recovery(
        "could not allocate a globally unique chat identity",
    ))
}

fn chat_projection_record(
    storage: &Storage,
    chat_id: &str,
) -> Result<Map<String, Value>, ProjectFailure> {
    storage
        .projection_record("chat", chat_id)?
        .ok_or_else(|| ProjectFailure::manifest("chat projection is missing"))
}

fn run_projection_record(
    storage: &Storage,
    run_id: &str,
) -> Result<Map<String, Value>, ProjectFailure> {
    storage
        .projection_record("run", run_id)?
        .ok_or_else(|| ProjectFailure::manifest("run projection is missing"))
}

fn chat_projection(
    chat: &Value,
    message: Option<Value>,
    sequence: u64,
) -> Result<Map<String, Value>, ProjectFailure> {
    Ok(json!({
        "chat_id":chat["chatId"],"project_id":chat["projectId"],
        "worktree_id":chat["worktreeId"],"workspace_identity":chat["workspaceIdentity"],
        "state":chat["state"],"record":chat,"last_message":message,
        "durable_event_sequence":sequence,
    })
    .as_object()
    .expect("chat projection object")
    .clone())
}

fn run_projection(
    run: &Value,
    manifest: &Value,
    recovery: Option<Value>,
) -> Result<Map<String, Value>, ProjectFailure> {
    Ok(json!({
        "run_id":run["runId"],"chat_id":run["chatId"],"state":run["state"],
        "worktree_path":manifest.get("worktreePath").cloned().unwrap_or(Value::Null),
        "record":run,"workspace_manifest":manifest,"recovery":recovery,
    })
    .as_object()
    .expect("run projection object")
    .clone())
}

fn persist_chat_state(
    storage: &mut Storage,
    chat: &Value,
    operation: &str,
) -> Result<(), ProjectFailure> {
    let existing = chat_projection_record(storage, chat["chatId"].as_str().unwrap_or_default())?;
    let payload = chat_projection(
        chat,
        None,
        existing["durable_event_sequence"].as_u64().unwrap_or(0),
    )?;
    storage.upsert_projection_record(
        "chat",
        chat["chatId"].as_str().unwrap_or_default(),
        operation,
        None,
        payload,
    )?;
    Ok(())
}

fn persist_run(
    storage: &mut Storage,
    run: &Value,
    manifest: &Value,
    recovery: Option<Value>,
    operation: &str,
) -> Result<(), ProjectFailure> {
    let payload = run_projection(run, manifest, recovery)?;
    storage.upsert_projection_record(
        "run",
        run["runId"].as_str().unwrap_or_default(),
        operation,
        None,
        payload,
    )?;
    Ok(())
}

fn team_message(
    chat: &Value,
    sequence: u64,
    kind: &str,
    role_id: Option<&str>,
    run_id: Option<&str>,
    text: &str,
) -> Result<Value, ProjectFailure> {
    let message = json!({
        "schemaVersion":1,"messageId":format!("message_{}",Uuid::new_v4().simple()),
        "chatId":chat["chatId"],"taskId":chat["taskId"],"worktreeId":chat["worktreeId"],
        "eventSequence":sequence,"kind":kind,"roleId":role_id,"runId":run_id,
        "text":text,"createdAt":now(),
    });
    validate_g5_payload_v1("teamMessage", &message)
        .map_err(|error| ProjectFailure::manifest(format!("invalid public message: {error}")))?;
    Ok(message)
}

fn chat_event(chat: &Value, seq: u64, message: Value) -> G5Event {
    G5Event {
        stream_id: format!(
            "chat_ledger_{}",
            chat["chatId"].as_str().unwrap_or_default()
        ),
        seq,
        event: "chat.eventAppended",
        chat_id: chat["chatId"].as_str().map(str::to_string),
        task_id: chat["taskId"].as_str().map(str::to_string),
        run_id: None,
        payload: message,
    }
}

fn address_from_chat(chat: &Value) -> Result<WorkspaceAddress, ProjectFailure> {
    let string = |field: &str| {
        chat[field]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ProjectFailure::manifest(format!("chat {field} is missing")))
    };
    Ok(WorkspaceAddress {
        chat_id: string("chatId")?,
        project_id: string("projectId")?,
        worktree_id: string("worktreeId")?,
        workspace_identity: string("workspaceIdentity")?,
    })
}

fn verify_unbound_snapshot(
    projects: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    worktree_id: &str,
    workspace_identity: &str,
) -> Result<(), ProjectFailure> {
    projects.verify_unbound_workspace(storage, project_id, worktree_id, workspace_identity)
}

fn profile_is_fixture_available(profile: &Value) -> bool {
    FIXTURE_PROFILES
        .iter()
        .any(|(role, runtime, model, reasoning, permission)| {
            profile["roleId"] == *role
                && profile["runtime"] == *runtime
                && profile["modelSelector"] == *model
                && profile["reasoningEffort"].as_str() == *reasoning
                && profile["permission"] == *permission
        })
}

fn update_chat_state(chat: &Value, state: &str) -> Result<Value, ProjectFailure> {
    let mut value = chat.clone();
    value["state"] = Value::String(state.to_string());
    value["updatedAt"] = Value::String(now());
    validate_g5_payload_v1("chatRecord", &value)
        .map_err(|error| ProjectFailure::manifest(format!("invalid chat transition: {error}")))?;
    Ok(value)
}

fn update_run_state(
    run: &Value,
    state: &str,
    evidence: Option<Value>,
) -> Result<Value, ProjectFailure> {
    let mut value = run.clone();
    value["state"] = Value::String(state.to_string());
    value["terminationEvidence"] = evidence.unwrap_or(Value::Null);
    value["updatedAt"] = Value::String(now());
    validate_g5_payload_v1("runRecord", &value)
        .map_err(|error| ProjectFailure::manifest(format!("invalid run transition: {error}")))?;
    Ok(value)
}

fn writer_lock_update_payload(
    storage: &Storage,
    chat: &Value,
    run_id: &str,
    recovery: bool,
) -> Result<(String, Map<String, Value>), ProjectFailure> {
    let lock_id = format!("lock_{}", chat["worktreeId"].as_str().unwrap_or_default());
    let mut lock = storage
        .projection_record("lock", &lock_id)?
        .ok_or_else(|| ProjectFailure::manifest("writer lock is missing"))?;
    lock.insert(
        "writer_run_id".to_string(),
        if recovery {
            Value::String(run_id.to_string())
        } else {
            Value::Null
        },
    );
    lock.insert("poisoned".to_string(), Value::Bool(recovery));
    lock.insert("recovery_required".to_string(), Value::Bool(recovery));
    lock.insert("evidence".to_string(), json!({"phase":if recovery {"g5RecoveryRequired"} else {"g5WriterReleased"},"run_id":run_id}));
    lock.remove("last_event_sequence");
    Ok((lock_id, lock))
}

fn find_recovery(storage: &Storage, recovery_id: &str) -> Result<Value, ProjectFailure> {
    storage
        .projection_records("run")?
        .into_iter()
        .filter(|run| run.get("source_kind").and_then(Value::as_str) != Some("actualAgent"))
        .filter_map(|run| run.get("recovery").cloned())
        .find(|recovery| recovery["recoveryId"] == recovery_id)
        .ok_or_else(|| ProjectFailure::manifest("recovery record is missing"))
}

fn process_identity_value(identity: ProcIdentity) -> Value {
    let seconds = identity.start_us.div_euclid(1_000_000);
    let micros = identity.start_us.rem_euclid(1_000_000) as u32;
    let timestamp = chrono::DateTime::from_timestamp(seconds, micros * 1_000)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(now);
    json!({"pid":identity.pid,"startTime":timestamp,"startTimeMicros":identity.start_us,
        "executablePathDigest":digest_text("hello-agent-sidecar-fixture"),"observedGone":false})
}

fn strip_internal_identity_fields(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(identities) = value
        .get_mut("processIdentities")
        .and_then(Value::as_array_mut)
    {
        for identity in identities {
            identity
                .as_object_mut()
                .map(|object| object.remove("startTimeMicros"));
        }
    }
    value
}

fn parse_identity_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp_micros())
}

fn digest_text(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

fn termination_config() -> TerminationConfig {
    TerminationConfig {
        term_grace_ms: 1_000,
        kill_grace_ms: 1_000,
        poll_interval_ms: 25,
    }
}

fn bounded_limit(limit: u64, maximum: usize) -> Result<usize, ProjectFailure> {
    if limit == 0 || limit > maximum as u64 {
        return Err(ProjectFailure::manifest(
            "page limit is outside its bounded contract",
        ));
    }
    Ok(limit as usize)
}

fn parse_cursor(cursor: Option<&str>) -> Result<usize, ProjectFailure> {
    cursor
        .map(|value| {
            value
                .strip_prefix("offset:")
                .unwrap_or(value)
                .parse::<usize>()
        })
        .transpose()
        .map_err(|_| ProjectFailure::manifest("page cursor is malformed"))
        .map(|value| value.unwrap_or(0))
}

fn page<F>(
    items: Vec<Value>,
    offset: usize,
    limit: usize,
    build: F,
) -> Result<Value, ProjectFailure>
where
    F: FnOnce(Vec<Value>, Value) -> Value,
{
    if offset > items.len() {
        return Err(ProjectFailure::manifest(
            "page cursor is outside the result set",
        ));
    }
    let end = offset.saturating_add(limit).min(items.len());
    let next = if end < items.len() {
        Value::String(format!("offset:{end}"))
    } else {
        Value::Null
    };
    Ok(build(items[offset..end].to_vec(), next))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests;
