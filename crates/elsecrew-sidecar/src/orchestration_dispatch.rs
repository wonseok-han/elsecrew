//! Serial scheduler using exactly the same capability discovery and admission as manual runs.
use crate::orchestration::{self, Workflow};
use crate::*;
use elsecrew_protocol::agent_adapter_v1::{
    validate_agent_adapter_payload_v1, AgentRunStartRequestV1,
};
use serde_json::json;

fn role_report_title(role: &str, chat_record: &Value) -> String {
    // V3 teams carry the user's own role name; template IDs keep Korean labels.
    if let Some(name) = chat_record["roleDefinitions"].as_array().and_then(|definitions| {
        definitions.iter().find(|definition| definition["roleId"] == role)
    }).and_then(|definition| definition["name"].as_str()).map(str::trim).filter(|name| !name.is_empty()) {
        return name.to_owned();
    }
    match role {
        "planning" => "기획".into(),
        "design" => "디자인".into(),
        "review" => "검증".into(),
        "coding" => "개발".into(),
        other => other.into(),
    }
}

/// First line of every app-written report; files without it are never replaced.
const ROLE_REPORT_MARKER: &str = "<!-- elsecrew:role-report -->\n";

/// Plain Markdown: heading, the request the coordinator gave, then the report.
fn role_report_document(role: &str, chat_record: &Value, instruction: &str, run: &Value, text: &str) -> String {
    let title = role_report_title(role, chat_record);
    let goal = chat_record["goal"].as_str().unwrap_or_default();
    let profile = &run["selectedProfile"];
    let runtime = if profile["runtime"] == "codex" { "Codex" } else { "Claude Code" };
    let model = profile["modelSelector"].as_str().unwrap_or_default();
    let updated = run["updatedAt"].as_str().unwrap_or_default();
    let mut document = format!("{ROLE_REPORT_MARKER}# {title} 보고서\n\n- 채팅 목표: {}\n- 작성: {runtime} · {model} · {updated}\n",
        goal.lines().next().unwrap_or_default());
    if !instruction.trim().is_empty() {
        document.push_str(&format!("\n## 총괄의 요청\n\n{}\n", instruction.trim()));
    }
    document.push_str(&format!("\n## {title} 결과\n\n{}\n", text.trim_end()));
    document
}

/// Plain reason plus the fixed cause code, so a stopped team task always
/// says why. Only identifier-shaped codes are shown; never raw details.
fn workflow_failure_message(code: &str) -> String {
    let code = if !code.is_empty() && code.len() <= 64
        && code.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')) { code } else { "unknown" };
    format!("{} (원인 코드: {code})", workflow_failure_reason(code))
}

pub(super) fn failure(error: ProjectFailure) -> SidecarError {
    SidecarError::new("orchestrationBlocked", error.detail)
}
fn workflow_failure_reason(code: &str) -> &'static str {
    match code {
        "providerProfileUnavailable" => "다음 담당을 시작하기 위한 폴더 접근 확인에 실패했어요. 프로젝트와 앱 데이터 폴더의 접근 권한을 확인해 주세요.",
        "providerDisconnected" => "다음 담당의 에이전트가 연결 해제되어 있어요. 설정에서 연결해 주세요.",
        "notAuthenticated" | "authExpired" => "다음 담당의 로그인을 확인하지 못했어요. 설정에서 연결 상태를 확인해 주세요.",
        "notInstalled" | "providerExecutableMissing" => "다음 담당의 에이전트 설치를 확인하지 못했어요.",
        "providerBusy" | "providerLoginRecoveryRequired" => "다음 담당의 연결이 다른 작업에서 사용 중이거나 종료 확인이 필요해요.",
        "exactModelUnavailable" | "unsupportedProfile" => "다음 담당에 저장된 모델을 사용할 수 없어요. 역할 설정의 모델을 확인해 주세요.",
        _ => "담당 에이전트의 실행 준비 또는 결과를 확인하지 못했어요. 연결 상태와 역할 설정을 확인해 주세요.",
    }
}
/// How long a team step keeps asking a provider that is busy with another
/// check, and how often.
const WORKFLOW_BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
const WORKFLOW_BUSY_RETRY: std::time::Duration = std::time::Duration::from_secs(1);

/// The request id of a step's `attempt`-th try: a retried request never
/// reuses a refused one's id.
/// Development builds only: why a team step stopped. The app shows just the
/// code; the detail is what a developer needs. Release builds write nothing.
pub(crate) fn dev_diagnosis(line: &str) {
    #[cfg(debug_assertions)]
    {
        use std::io::Write as _;
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true)
            .open(std::env::temp_dir().join("elsecrew-dev-diagnosis.log"))
        {
            let _ = writeln!(file, "{line}");
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = line;
}

fn attempt_request_id(base: &str, attempt: u32) -> String {
    if attempt == 0 { base.to_owned() } else { format!("{base}_retry{attempt}") }
}

impl Sidecar {
    pub(super) fn handle_agent_request(
        &mut self,
        request: RequestV1,
    ) -> Result<HandledRequest, SidecarError> {
        self.require_provider_connection(&request)?;
        if request.method == "run.startAgent" {
            validate_agent_adapter_payload_v1("runStartRequest", &request.params)
                .map_err(|_| SidecarError::new("invalidParams", "invalid start request"))?;
            let input: AgentRunStartRequestV1 = serde_json::from_value(request.params.clone())
                .map_err(|_| SidecarError::new("invalidParams", "invalid start request"))?;
            match orchestration::begin(&mut self.storage, &input, &request.request_id) {
                Ok(true) => {
                    self.workflow_chats.insert(input.chat_id.clone());
                    self.workflow_requests
                        .insert(request.request_id.clone(), (input.chat_id, "root".into()));
                }
                Ok(false) => {}
                Err(error) => {
                    return Ok(HandledRequest::response(project_error_response(
                        &request, &error,
                    )))
                }
            }
        }
        let request_copy = request.clone();
        let handled = self.handle_agent_request_inner(request);
        if self
            .workflow_requests
            .contains_key(&request_copy.request_id)
        {
            let response = match &handled {
                Ok(value) => value.response.clone(),
                Err(error) => Some(error_response(
                    &request_copy,
                    error.kind(),
                    "requestBlocked",
                    error.detail(),
                )),
            };
            if let Some(response) = response {
                let mut events = Vec::new();
                self.workflow_response(&response, &mut events)?;
                if let Ok(mut value) = handled {
                    value.events.extend(self.g5_events(events));
                    return Ok(value);
                }
            }
        }
        handled
    }

    /// Writes `docs/elsecrew/<roleId>.md` in the chat's working copy through the
    /// app-owned save path. The original project is only changed by an explicit
    /// apply after review.
    fn save_role_report(
        &mut self,
        chat_record: &Value,
        run: &Value,
        role: &str,
        instruction: &str,
        text: &str,
    ) -> Result<(), ProjectFailure> {
        let field = |key: &str| chat_record[key].as_str().map(str::to_owned)
            .ok_or_else(|| ProjectFailure::recovery("chat record omitted its workspace identity"));
        let address = crate::project::WorkspaceAddress {
            chat_id: field("chatId")?,
            project_id: field("projectId")?,
            worktree_id: field("worktreeId")?,
            workspace_identity: field("workspaceIdentity")?,
        };
        let run_id = run["runId"].as_str().unwrap_or_default();
        let document = role_report_document(role, chat_record, instruction, run, text);
        self.projects.workspace_write_report(
            &mut self.storage, &address, "docs/elsecrew", role, ROLE_REPORT_MARKER, &document,
            &format!("report_{run_id}"),
        ).map(|_| ())
    }

    fn commit_team_step(&mut self, chat_record: &Value, message: &str) -> Result<bool, ProjectFailure> {
        let field = |key: &str| chat_record[key].as_str().map(str::to_owned)
            .ok_or_else(|| ProjectFailure::recovery("chat record omitted its workspace identity"));
        let address = crate::project::WorkspaceAddress {
            chat_id: field("chatId")?,
            project_id: field("projectId")?,
            worktree_id: field("worktreeId")?,
            workspace_identity: field("workspaceIdentity")?,
        };
        self.projects.workspace_commit_step(&self.storage, &address, message)
    }

    pub(super) fn cancel_team_task(
        &mut self,
        request: RequestV1,
    ) -> Result<HandledRequest, SidecarError> {
        let result = (|| -> Result<(Value, chat::G5Event), ProjectFailure> {
            let params = require_exact_params(
                &request.params,
                &[
                    "action",
                    "chatId",
                    "taskId",
                    "projectId",
                    "worktreeId",
                    "workspaceIdentity",
                    "triggerMessageId",
                ],
            )
            .map_err(ProjectFailure::manifest)?;
            let chat_id = param_string(params, "chatId").map_err(ProjectFailure::manifest)?;
            let mut payload = self
                .storage
                .projection_record("chat", chat_id)?
                .ok_or_else(|| ProjectFailure::manifest("missing team task"))?;
            for key in [
                "chatId",
                "taskId",
                "projectId",
                "worktreeId",
                "workspaceIdentity",
            ] {
                if payload["record"][key] != params[key] {
                    return Err(ProjectFailure::manifest(
                        "team cancellation address changed",
                    ));
                }
            }
            let mut state = orchestration::load(&payload)?
                .ok_or_else(|| ProjectFailure::manifest("no team task to cancel"))?;
            if params["triggerMessageId"] != state.trigger_id
                || !["running", "canceled"].contains(&state.status.as_str())
            {
                return Err(ProjectFailure::manifest(
                    "team cancellation targets a different or finished task",
                ));
            }
            let mut stopping = false;
            if let Some(run_id) = &state.run_id {
                if self.agent_runtime.has_active_run(run_id) {
                    let field = |key: &str| {
                        payload["record"][key]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned()
                    };
                    self.agent_runtime
                        .request_cancel(&agent_runtime::RunIdentity {
                            chat_id: chat_id.into(),
                            task_id: field("taskId"),
                            project_id: field("projectId"),
                            worktree_id: field("worktreeId"),
                            workspace_identity: field("workspaceIdentity"),
                            run_id: run_id.clone(),
                        })?;
                    stopping = true;
                }
            }
            state.status = "canceled".into();
            orchestration::put(&mut payload, &state)?;
            // Acknowledging a stop is not process-closure evidence. Active runs keep
            // their existing state until the ordinary terminal owner commits proof.
            if !stopping && payload["record"]["state"] != "recoveryRequired" {
                payload["record"]["state"] = json!("canceled");
                payload.insert("state".into(), json!("canceled"));
            }
            self.storage.upsert_projection_record(
                "chat",
                chat_id,
                &format!("op_workflow_cancel_{}", Uuid::new_v4().simple()),
                None,
                payload,
            )?;
            self.workflow_chats.remove(chat_id);
            let event = orchestration::notice(
                &mut self.storage,
                chat_id,
                if stopping {
                    "팀 작업을 중단하고 종료를 확인하고 있어요."
                } else {
                    "팀 작업을 중단했어요. 다음 단계는 실행하지 않아요."
                },
            )?;
            Ok((
                json!({"schemaVersion":1,"action":"cancelTeamTask","chatId":chat_id,"triggerMessageId":state.trigger_id,"status":if stopping {"stopping"}else{"stopped"}}),
                event,
            ))
        })();
        match result {
            Ok((value, event)) => Ok(HandledRequest {
                response: Some(success_response(&request, value)),
                events: self.g5_events(vec![event]),
            }),
            Err(error) => Ok(HandledRequest::response(project_error_response(
                &request, &error,
            ))),
        }
    }

    /// The same idle gate as workspace cleanup: nothing may still be starting,
    /// running or cancelling anywhere while app-owned files are removed.
    pub(super) fn deletion_idle(&self) -> bool {
        !self.g5.cleanup_blocked() && self.workflow_chats.is_empty() && self.agent_runtime.owned_run_ids().is_empty()
            && self.pending_agent_starts.is_empty() && self.pending_agent_cancels.is_empty()
    }

    /// A deleted task (or every task of a deleted project) can leave a run
    /// that stopped without a clean finish, for example when the app quit
    /// mid-run. Settle it the same way as the troubleshooting "discard":
    /// only when every recorded process is provably gone. Then finish the
    /// pending deletions and release provider sign-in holds whose bound run
    /// has durably ended. A hold is tried before and after the files go: its
    /// proof accepts the workspace's manifest or, once that is removed, the
    /// ledger's record of the finished deletion, so neither waits for the
    /// other. Anything unproven stays and is retried on a later call.
    /// Returns how many deletions are still pending.
    pub(super) fn settle_deleted_work(&mut self, request_id: &str) -> Result<usize, ProjectFailure> {
        if self.deletion_idle() && !self.projects.deletion_busy() {
            self.resolve_deleted_recoveries(request_id);
        }
        self.release_settled_fences();
        let pending = self.projects.finish_deletions(&mut self.storage, request_id);
        self.release_settled_fences();
        pending
    }

    /// Releases each provider sign-in hold whose bound run has durably ended
    /// (the same proof as the "recover" action; nothing is inferred from a
    /// missing process alone).
    #[cfg(target_os = "macos")]
    pub(super) fn release_settled_fences(&mut self) {
        for name in ["claude-login.pending", "codex-login.pending"] {
            // A run's own hold (ADE multi-project P5) is always bound to its run.
            let prefix = name.trim_end_matches("-login.pending");
            let run_holds = self.storage.path().parent()
                .and_then(|parent| crate::provider_profile::open_absolute_directory(parent).ok())
                .and_then(|directory| crate::provider_login_process::run_hold_names(&directory, prefix).ok())
                .is_some_and(|names| !names.is_empty());
            if !run_holds && !self.fence_is_run_bound(name) { continue; }
            if name.starts_with("claude") {
                self.claude_login.release_bound_fence(&self.storage, &self.projects);
            } else {
                self.login.release_bound_fence(&self.storage, &self.projects);
            }
        }
    }

    /// Provider sign-in holds exist only where the app runs providers itself.
    #[cfg(not(target_os = "macos"))]
    pub(super) fn release_settled_fences(&mut self) {}

    /// Whether a sign-in hold file exists and names a run. A quick look only,
    /// to skip the common case of no hold; the release re-verifies everything.
    #[cfg(target_os = "macos")]
    fn fence_is_run_bound(&self, name: &str) -> bool {
        let Some(path) = self.storage.path().parent().map(|parent| parent.join(name)) else { return false };
        let Ok(metadata) = std::fs::symlink_metadata(&path) else { return false };
        if !metadata.is_file() || metadata.len() > crate::provider_login_process::HOLD_MAX_BYTES {
            return false;
        }
        // The header and the binding are the first two lines; a long hold's
        // process list after them need not be read for this look.
        use std::io::Read;
        let mut head = Vec::new();
        std::fs::File::open(&path).and_then(|file| file.take(4096).read_to_end(&mut head)).is_ok()
            && String::from_utf8_lossy(&head).lines().nth(1).is_some_and(|line| line.starts_with("run-binding-v1 "))
    }

    fn resolve_deleted_recoveries(&mut self, request_id: &str) {
        let Ok(runs) = self.storage.projection_records("run") else { return };
        let mut index = 0usize;
        for run in runs {
            if run.get("source_kind").and_then(Value::as_str) != Some("actualAgent") { continue; }
            let recovery = match run.get("recovery") {
                Some(recovery) if recovery["state"] == "unresolved" => recovery.clone(),
                _ => continue,
            };
            let (Some(project_id), Some(chat_id)) = (recovery["projectId"].as_str(), recovery["chatId"].as_str()) else { continue };
            let Some(project) = self.storage.projection_record("project", project_id).ok().flatten() else { continue };
            if !(project::project_deletion::is_deleted(&project) || project::project_deletion::deleted_chat(&project, chat_id)) { continue; }
            // Older data: the project view may still sit on this task's copy.
            // Settling would close that copy under an open view; wait.
            if project.get("worktree_id") == Some(&recovery["worktreeId"]) && self.projects.project_view_open(project_id) { continue; }
            index += 1;
            let request = json!({
                "recoveryId":recovery["recoveryId"],"chatId":recovery["chatId"],"taskId":recovery["taskId"],
                "projectId":recovery["projectId"],"worktreeId":recovery["worktreeId"],
                "workspaceIdentity":recovery["workspaceIdentity"],"runId":recovery["runId"],
                "expectedRevision":recovery["revision"],"resolution":"abandon",
            });
            match self.agent_runtime.resolve_recovery(&mut self.storage, &mut self.projects, &request, &format!("{request_id}_settle_{index}")) {
                Ok(result) if result["outcome"] == "resolvedAbandoned" => {}
                Ok(result) => eprintln!("deleted task recovery is still waiting: {}", result["outcome"].as_str().unwrap_or("unknown")),
                Err(error) => eprintln!("deleted task recovery is still waiting: {}", error.kind),
            }
        }
    }

    /// Deleting a task or project is the user's decision. Stop what is still
    /// running in that scope first: a running team stops before its next step
    /// and an active agent run is asked to cancel. Termination proof keeps
    /// arriving through the ordinary run owner afterwards.

    pub(super) fn stop_sessions_for_removal(
        &mut self,
        project_id: &str,
        chat_id: Option<&str>,
    ) -> Result<(), ProjectFailure> {
        let mut first_error = None;
        for chat in self.storage.projection_records("chat")? {
            let Some(id) = chat.get("chat_id").and_then(Value::as_str).map(str::to_owned) else { continue };
            if chat.get("project_id").and_then(Value::as_str) != Some(project_id)
                || chat_id.is_some_and(|wanted| wanted != id)
            {
                continue;
            }
            // One task that cannot be stopped must not leave the others running.
            if let Err(error) = self.stop_one_for_removal(&id) {
                first_error.get_or_insert(error);
            }
            self.workflow_chats.remove(&id);
        }
        first_error.map_or(Ok(()), Err)
    }

    fn stop_one_for_removal(&mut self, id: &str) -> Result<(), ProjectFailure> {
        let Some(mut payload) = self.storage.projection_record("chat", id)? else { return Ok(()) };
        let field = |payload: &Map<String, Value>, key: &str| payload["record"][key].as_str().unwrap_or_default().to_owned();
        let identity = |payload: &Map<String, Value>, run_id: &str| agent_runtime::RunIdentity {
            chat_id: id.to_owned(),
            task_id: field(payload, "taskId"),
            project_id: field(payload, "projectId"),
            worktree_id: field(payload, "worktreeId"),
            workspace_identity: field(payload, "workspaceIdentity"),
            run_id: run_id.to_owned(),
        };
        let mut stopping = false;
        for run in self.storage.projection_records("run")? {
            let Some(run_id) = run.get("run_id").and_then(Value::as_str) else { continue };
            if run.get("chat_id").and_then(Value::as_str) == Some(id) && self.agent_runtime.has_active_run(run_id) {
                // Best effort: a run already winding down may refuse a second
                // cancel, and the team below must still stop.
                stopping |= self.agent_runtime.request_cancel(&identity(&payload, run_id)).is_ok();
            }
        }
        if let Some(mut state) = orchestration::load(&payload)? {
            if state.status == "running" {
                state.status = "canceled".into();
                orchestration::put(&mut payload, &state)?;
                if !stopping && payload["record"]["state"] != "recoveryRequired" {
                    payload["record"]["state"] = json!("canceled");
                    payload.insert("state".into(), json!("canceled"));
                }
                self.storage.upsert_projection_record(
                    "chat",
                    id,
                    &format!("op_workflow_remove_{}", Uuid::new_v4().simple()),
                    None,
                    payload,
                )?;
                orchestration::notice(&mut self.storage, id, "목록에서 지워 팀 작업을 중단했어요.")?;
            }
        }
        Ok(())
    }

    fn workflow_request(&self, id: String, method: &str, params: Value) -> RequestV1 {
        RequestV1 {
            protocol_version: PROTOCOL_VERSION,
            kind: "request".into(),
            sidecar_instance_id: self.ready.sidecar_instance_id.clone(),
            request_id: id,
            method: method.into(),
            params,
        }
    }
    fn workflow_state(&self, chat: &str) -> Result<Workflow, SidecarError> {
        let payload = self
            .storage
            .projection_record("chat", chat)?
            .ok_or_else(|| SidecarError::new("orchestrationBlocked", "missing workflow chat"))?;
        orchestration::load(&payload)
            .map_err(failure)?
            .ok_or_else(|| SidecarError::new("orchestrationBlocked", "missing workflow"))
    }
    fn pause_workflow_reason(
        &mut self, chat: &str, events: &mut Vec<chat::G5Event>, reason: &str,
    ) -> Result<(), SidecarError> {
        let mut state = self.workflow_state(chat)?;
        if state.status != "canceled" {
            state.status = "paused".into();
        }
        orchestration::save(&mut self.storage, chat, &state).map_err(failure)?;
        self.workflow_chats.remove(chat);
        self.workflow_busy.remove(chat);
        events.push(orchestration::notice(&mut self.storage,chat,&format!("팀 작업을 멈췄어요. {reason} 이미 실행한 작업은 자동으로 반복하지 않아요.")).map_err(failure)?);
        Ok(())
    }
    /// Returns true for internal responses: they never escape onto the renderer IPC channel.
    fn workflow_response(
        &mut self,
        response: &ResponseV1,
        events: &mut Vec<chat::G5Event>,
    ) -> Result<bool, SidecarError> {
        let Some((chat, kind)) = self.workflow_requests.remove(&response.request_id) else {
            return Ok(false);
        };
        if !response.ok {
            let code = response.error.as_ref().map(|e| e.code.as_str()).unwrap_or("unknown");
            if let Some(error) = &response.error {
                dev_diagnosis(&format!("team step {kind} failed: {} {}", error.code, error.detail));
            }
            // The provider is only briefly busy with another check (a model
            // list, a sign-in check) or with the previous member's cleanup:
            // ask again shortly instead of stopping the team, for up to
            // WORKFLOW_BUSY_WAIT. A refused start created no run, so the
            // step starts over from the capability check.
            if matches!(kind.as_str(), "capability" | "start") && code == "providerBusy" {
                let now = std::time::Instant::now();
                let (since, _, attempt) = self.workflow_busy.get(&chat).copied().unwrap_or((now, now, 0));
                if now.duration_since(since) < WORKFLOW_BUSY_WAIT {
                    self.workflow_busy.insert(chat.clone(), (since, now + WORKFLOW_BUSY_RETRY, attempt + 1));
                    return Ok(true);
                }
            }
            self.workflow_busy.remove(&chat);
            let reason = workflow_failure_message(code);
            self.pause_workflow_reason(&chat, events, &reason)?;
            return Ok(kind != "root");
        }
        if kind == "start" {
            self.workflow_busy.remove(&chat);
        }
        if kind == "root" || kind == "start" {
            let accepted = response.result.as_ref().ok_or_else(|| {
                SidecarError::new("orchestrationBlocked", "missing accepted team run")
            })?;
            if kind == "root" {
                let run_id = accepted["runId"].as_str().ok_or_else(|| {
                    SidecarError::new("orchestrationBlocked", "missing root run id")
                })?;
                self.workflow_roots.insert(chat.clone(), run_id.into());
            } else {
                let root = self.workflow_roots.get(&chat).ok_or_else(|| {
                    SidecarError::new("orchestrationBlocked", "missing authorized team root")
                })?;
                self.workflow_announcements.push(EventV1 {
                    protocol_version: PROTOCOL_VERSION,
                    kind: "event".into(),
                    sidecar_instance_id: self.ready.sidecar_instance_id.clone(),
                    stream_id: format!("team_accept_{}", Uuid::new_v4().simple()),
                    seq: 1,
                    event: "agent.teamRunAccepted".into(),
                    chat_id: Some(chat.clone()),
                    task_id: accepted["taskId"].as_str().map(str::to_owned),
                    run_id: accepted["runId"].as_str().map(str::to_owned),
                    payload: json!({"rootRunId":root,"accepted":accepted}),
                });
            }
        }
        if kind == "capability" {
            // A busy wait ends only when the start is accepted.
            let attempt = self.workflow_busy.get(&chat).map_or(0, |(_, _, attempt)| *attempt);
            let capability = response.result.as_ref().ok_or_else(|| {
                SidecarError::new("orchestrationBlocked", "missing capability result")
            })?;
            let state = self.workflow_state(&chat)?;
            if state.status != "running" || self.prepared_to_quit {
                return Ok(true);
            }
            if capability["canStart"] != true
                || capability["scope"]["chatId"] != chat
                || capability["scope"]["roleId"] != state.role
            {
                self.pause_workflow_reason(&chat, events, &workflow_failure_message(capability["blockedReason"].as_str().unwrap_or("unknown")))?;
                return Ok(true);
            }
            // A retried start uses its own request id; the refused one is never reused.
            let start_id = attempt_request_id(&state.request_id, attempt);
            let request=self.workflow_request(start_id.clone(),"run.startAgent",json!({"chatId":chat,"roleId":state.role,"inputMessageIds":state.input_ids,"expectedCapabilityFingerprint":capability["fingerprint"]}));
            self.workflow_requests
                .insert(request.request_id.clone(), (chat.clone(), "start".into()));
            match self.handle_agent_request_inner(request) {
                Ok(handled) => {
                    self.workflow_outbound.extend(handled.events);
                    if let Some(response) = handled.response {
                        self.workflow_response(&response, events)?;
                    }
                }
                Err(error) => {
                    dev_diagnosis(&format!("team step start failed: {} {}", error.kind(), error.detail()));
                    self.workflow_requests.remove(&start_id);
                    self.workflow_busy.remove(&chat);
                    self.pause_workflow_reason(&chat, events, &workflow_failure_message(error.kind()))?;
                }
            }
        }
        Ok(kind != "root")
    }
    pub(super) fn poll_workflows(
        &mut self,
        responses: &mut Vec<ResponseV1>,
        events: &mut Vec<chat::G5Event>,
    ) -> Result<(), SidecarError> {
        let mut outward = Vec::new();
        for response in std::mem::take(responses) {
            if !self.workflow_response(&response, events)? {
                outward.push(response);
            }
        }
        *responses = outward;
        if self.prepared_to_quit {
            return Ok(());
        }
        for chat in self.workflow_chats.clone() {
            if self.workflow_requests.values().any(|(id, _)| id == &chat) {
                continue;
            }
            let mut state = self.workflow_state(&chat)?;
            if state.status != "running" {
                self.workflow_chats.remove(&chat);
                self.workflow_busy.remove(&chat);
                continue;
            }
            if let Some(run_id) = state.run_id.clone() {
                if self.agent_runtime.has_active_run(&run_id) {
                    continue;
                }
                let Some(projection) = self.storage.projection_record("run", &run_id)? else {
                    self.pause_workflow_reason(&chat, events, &workflow_failure_message("teamRunMissing"))?;
                    continue;
                };
                let run = &projection["record"];
                if ["preparing", "running", "cancelling"]
                    .contains(&run["state"].as_str().unwrap_or_default())
                {
                    continue;
                }
                // Never advance merely because text was emitted: proof of process closure,
                // matching chat/workspace and the actual-agent namespace are mandatory.
                let chat_record =
                    self.storage.projection_record("chat", &chat)?.unwrap()["record"].clone();
                let bound = [
                    "chatId",
                    "taskId",
                    "projectId",
                    "worktreeId",
                    "workspaceIdentity",
                ]
                .iter()
                .all(|key| run[key] == chat_record[key]);
                if !bound
                    || projection["source_kind"] != "actualAgent"
                    || (run["state"] == "succeeded"
                        && (projection["termination"]["processesProvenGone"] != true
                            || !projection["recovery"].is_null()))
                {
                    self.pause_workflow_reason(&chat, events, &workflow_failure_message("teamRunUnconfirmed"))?;
                    continue;
                }
                let messages = self.storage.chat_message_history(&chat)?;
                let finals: Vec<_> = messages
                    .iter()
                    .filter(|m| {
                        m["runId"] == run_id && m["kind"] == "role" && m["roleId"] == state.role
                    })
                    .collect();
                let text = if finals.len() == 1 {
                    finals[0]["text"].as_str().unwrap_or_default()
                } else {
                    ""
                };
                let finished_role = state.role.clone();
                let finished_instruction = state.instruction.clone();
                // A real worktree records each member's work on the task
                // branch, finished or not; a failed record never stops the team.
                if finished_role != "coordinator" {
                    let summary = format!("{}: {}", role_report_title(&finished_role, &chat_record),
                        // A report's first line is often a Markdown heading.
                        text.lines().map(|line| line.trim().trim_start_matches('#').trim()).find(|line| !line.is_empty()).unwrap_or("작업 기록"));
                    if let Err(error) = self.commit_team_step(&chat_record, &summary) {
                        eprintln!("team step not recorded on the task branch: {}", error.detail);
                    }
                }
                if state.finish(run, text, &chat_record["roleSnapshots"]).is_err() {
                    // Distinguish a stopped/failed member from an unusable report or decision.
                    let code = if run["state"] != "succeeded" { "teamMemberNotSucceeded" }
                        else if finals.len() != 1 { "teamReportMissing" } else { "teamDecisionInvalid" };
                    self.pause_workflow_reason(&chat, events, &workflow_failure_message(code))?;
                    continue;
                }
                // Read-only members (planning, design, review, …) cannot write.
                // Keep their deliverable as a reviewable document in the working copy.
                let mut report_failed = false;
                if finished_role != "coordinator"
                    && run["state"] == "succeeded"
                    && run["selectedProfile"]["permission"] == "readOnly"
                {
                    match self.save_role_report(&chat_record, run, &finished_role, &finished_instruction, text) {
                        Ok(()) => state.changed = true,
                        Err(_) => report_failed = true,
                    }
                }
                orchestration::save(&mut self.storage, &chat, &state).map_err(failure)?;
                if report_failed {
                    events.push(orchestration::notice(&mut self.storage, &chat,
                        "역할 보고서를 파일로 저장하지 못했어요. 보고 내용은 이 대화에 그대로 남아 있어요.").map_err(failure)?);
                }
                let notice = match state.status.as_str() {
                    "completed" if state.limit_reached => "이번 요청에서 쓸 수 있는 팀 실행 횟수를 다 썼어요. 총괄이 맡기려던 다음 일은 아직 시작하지 않았어요. 이어서 하려면 '계속'이라고 보내 주세요.",
                    "completed" => "답변을 마쳤어요.",
                    "canceled" => "팀 작업을 중단했어요.",
                    "paused" => "팀 작업이 멈췄어요. 실행 결과를 확인해 주세요.",
                    _ => "팀 작업을 이어가고 있어요.",
                };
                events.push(
                    orchestration::notice(&mut self.storage, &chat, notice).map_err(failure)?,
                );
                if state.status != "running" {
                    self.workflow_chats.remove(&chat);
                    continue;
                }
            }
            let payload = self.storage.projection_record("chat", &chat)?.unwrap();
            let runtime = payload["record"]["roleSnapshots"]
                .as_array()
                .and_then(|roles| roles.iter().find(|p| p["roleId"] == state.role))
                .map(|p| p["runtime"].clone())
                .unwrap_or(Value::Null);
            // Waiting out a busy provider: ask again only when it is time.
            let attempt = match self.workflow_busy.get(&chat) {
                Some((_, retry_at, _)) if std::time::Instant::now() < *retry_at => continue,
                Some((_, _, attempt)) => *attempt,
                None => 0,
            };
            let id = attempt_request_id(&format!("cap_{}", state.request_id), attempt);
            let request = self.workflow_request(
                id.clone(),
                "agent.getCapabilities",
                json!({"runtime":runtime,"refresh":true,"chatId":chat,"roleId":state.role}),
            );
            self.workflow_requests
                .insert(id.clone(), (chat.clone(), "capability".into()));
            match self.handle_agent_request_inner(request) {
                Ok(handled) => {
                    if let Some(response) = handled.response {
                        self.workflow_response(&response, events)?;
                    }
                }
                Err(error) => {
                    dev_diagnosis(&format!("team step capability failed: {} {}", error.kind(), error.detail()));
                    self.workflow_requests.remove(&id);
                    self.pause_workflow_reason(&chat, events, &workflow_failure_message(error.kind()))?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::agent_runtime::{AgentRuntime, FakeScenario};
    use std::{
        process::Command,
        time::{Duration, Instant},
    };

    fn service() -> (tempfile::TempDir, Sidecar, Value) {
        service_with(None)
    }
    /// `team` replaces the default V2 team with a V3 `(roles, roleDefinitions)` pair.
    fn service_with(team: Option<(Value, Value)>) -> (tempfile::TempDir, Sidecar, Value) {
        service_named(team, "data.sqlite3")
    }
    /// Provider profiles (and so sign-in holds) need the app's real database name.
    fn service_named(team: Option<(Value, Value)>, database: &str) -> (tempfile::TempDir, Sidecar, Value) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("hello.txt"), "original\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "-qm",
                "Initial",
            ],
        ] {
            assert!(Command::new("/usr/bin/git")
                .args(args)
                .current_dir(&source)
                .status()
                .unwrap()
                .success());
        }
        let binary = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("elsecrew-sidecar");
        let runtime = AgentRuntime::fake(binary, FakeScenario::OrchestratorDelegate);
        let ready = SidecarSession::new("orchestration_fixture".into(), AgentRuntime::blocked())
            .ready
            .clone();
        let mut sidecar = Sidecar::initialize(
            root.path().join(database),
            ready,
            runtime,
            &AtomicBool::new(false),
        )
        .unwrap();
        let project = sidecar
            .projects
            .open_project(
                &mut sidecar.storage,
                source.to_str().unwrap(),
                "req_workflow_open",
            )
            .unwrap()
            .result;
        let project_id = project["projectId"].as_str().unwrap();
        sidecar
            .g5
            .default_role_settings(
                &mut sidecar.storage,
                &sidecar.projects,
                project_id,
                "req_workflow_defaults",
            )
            .unwrap();
        let roles:Vec<_>=["coordinator","planning","design","coding","review"].iter().map(|role|json!({"roleId":role,"runtime":"claude-code","modelSelector":"claude-opus-5","reasoningEffort":null,"permission":if *role=="coding"{"workspaceWrite"}else{"readOnly"}})).collect();
        sidecar
            .g5
            .update_role_settings(
                &mut sidecar.storage,
                project_id,
                1,
                team.as_ref().map(|team| team.0.clone()).unwrap_or_else(|| json!(roles)),
                team.as_ref().map(|team| team.1.clone()),
                "req_workflow_roles",
            )
            .unwrap();
        let snapshot = sidecar
            .g5
            .prepare_chat_workspace(
                &mut sidecar.storage,
                &mut sidecar.projects,
                project_id,
                "req_workflow_snapshot",
            )
            .unwrap()
            .result;
        let chat = sidecar
            .g5
            .create_chat(
                &mut sidecar.storage,
                &sidecar.projects,
                snapshot["preparationId"].as_str().unwrap(),
                project_id,
                snapshot["manifest"]["worktreeId"].as_str().unwrap(),
                snapshot["manifest"]["workspaceIdentity"].as_str().unwrap(),
                "Inspect the fixture",
                2,
                "req_workflow_chat",
            )
            .unwrap()
            .result;
        (root, sidecar, chat)
    }
    /// A provider briefly busy with another check (model list, sign-in
    /// check) does not stop the team: the step asks again shortly, and only a
    /// provider that stays busy for the whole wait stops it.
    #[test]
    fn a_briefly_busy_provider_delays_the_team_step_instead_of_stopping_it() {
        let (_root, mut sidecar, chat) = service();
        let id = chat["chatId"].as_str().unwrap();
        let input = AgentRunStartRequestV1 {
            chat_id:id.into(), role_id:"coordinator".into(),
            input_message_ids:sidecar.storage.chat_message_history(id).unwrap().iter().filter_map(|v|v["messageId"].as_str().map(str::to_owned)).collect(),
            expected_capability_fingerprint:"sha256:unused".into(),
        };
        orchestration::begin(&mut sidecar.storage, &input, "busy-test").unwrap();
        let busy = |sidecar: &mut Sidecar, request_id: &str| {
            let request = sidecar.workflow_request(request_id.into(), "agent.getCapabilities", json!({}));
            sidecar.workflow_requests.insert(request.request_id.clone(), (id.into(), "capability".into()));
            let response = error_response(&request, "providerBusy", "requestBlocked", "another provider check currently owns this profile; retry after it finishes");
            let mut events = Vec::new();
            assert!(sidecar.workflow_response(&response, &mut events).unwrap());
            events
        };
        assert!(busy(&mut sidecar, "busy-cap-1").is_empty());
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "running", "still waiting for the provider");
        let (since, retry_at, attempt) = sidecar.workflow_busy[id];
        assert!(retry_at > since && attempt == 1);
        // Busy for the whole wait: the team stops with the usual reason.
        sidecar.workflow_busy.insert(id.into(), (since - WORKFLOW_BUSY_WAIT, retry_at, attempt));
        assert!(!busy(&mut sidecar, "busy-cap-2").is_empty());
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "paused");
        assert!(!sidecar.workflow_busy.contains_key(id));
    }

    /// The next member's start can meet the previous member's cleanup still
    /// holding the profile: the step waits and starts over, never reusing the
    /// refused start's request id.
    #[test]
    fn a_busy_start_waits_and_retries_under_a_new_request_id() {
        let (_root, mut sidecar, chat) = service();
        let id = chat["chatId"].as_str().unwrap();
        let input = AgentRunStartRequestV1 {
            chat_id:id.into(), role_id:"coordinator".into(),
            input_message_ids:sidecar.storage.chat_message_history(id).unwrap().iter().filter_map(|v|v["messageId"].as_str().map(str::to_owned)).collect(),
            expected_capability_fingerprint:"sha256:unused".into(),
        };
        orchestration::begin(&mut sidecar.storage, &input, "busy-start").unwrap();
        let request = sidecar.workflow_request("busy-start".into(), "run.startAgent", json!({}));
        sidecar.workflow_requests.insert(request.request_id.clone(), (id.into(), "start".into()));
        let response = error_response(&request, "providerBusy", "requestBlocked", "another provider check currently owns this profile; retry after it finishes");
        let mut events = Vec::new();
        assert!(sidecar.workflow_response(&response, &mut events).unwrap());
        assert!(events.is_empty(), "no stop notice while waiting");
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "running");
        let (_, _, attempt) = sidecar.workflow_busy[id];
        assert_eq!(attempt, 1);
        // Retries are asked under their own ids, never the refused one.
        assert_eq!(attempt_request_id("step", 0), "step");
        assert_eq!(attempt_request_id("step", 1), "step_retry1");
        assert_ne!(attempt_request_id("step", 2), attempt_request_id("step", 1));
        // Busy for the whole wait: the team stops with the usual reason.
        let (since, retry_at, attempt) = sidecar.workflow_busy[id];
        sidecar.workflow_busy.insert(id.into(), (since - WORKFLOW_BUSY_WAIT, retry_at, attempt));
        let request = sidecar.workflow_request("busy-start_retry1".into(), "run.startAgent", json!({}));
        sidecar.workflow_requests.insert(request.request_id.clone(), (id.into(), "start".into()));
        let response = error_response(&request, "providerBusy", "requestBlocked", "still busy");
        assert!(sidecar.workflow_response(&response, &mut events).unwrap());
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "paused");
        assert!(!sidecar.workflow_busy.contains_key(id));
    }

    #[test]
    fn orchestration_admission_failure_keeps_actionable_reason_without_raw_detail() {
        let (_root, mut sidecar, chat) = service();
        let id = chat["chatId"].as_str().unwrap();
        let input = AgentRunStartRequestV1 {
            chat_id:id.into(), role_id:"coordinator".into(),
            input_message_ids:sidecar.storage.chat_message_history(id).unwrap().iter().filter_map(|v|v["messageId"].as_str().map(str::to_owned)).collect(),
            expected_capability_fingerprint:"sha256:unused".into(),
        };
        orchestration::begin(&mut sidecar.storage, &input, "reason-test").unwrap();
        let request = sidecar.workflow_request("reason-cap".into(), "agent.getCapabilities", json!({}));
        sidecar.workflow_requests.insert(request.request_id.clone(), (id.into(), "capability".into()));
        let response = error_response(&request, "providerProfileUnavailable", "requestBlocked", "secret/path/token");
        let mut events = Vec::new();
        assert!(sidecar.workflow_response(&response, &mut events).unwrap());
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "paused");
        let messages = sidecar.storage.chat_message_history(id).unwrap();
        let text = messages.last().unwrap()["text"].as_str().unwrap();
        assert!(text.contains("폴더 접근"));
        assert!(!text.contains("secret"));
        assert!(sidecar.storage.projection_records("run").unwrap().is_empty());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn explicit_provider_connections_block_root_and_team_member_before_reservation() {
        let (_root,mut sidecar,chat)=service();
        sidecar.agent_runtime=AgentRuntime::native_candidate();
        let before=sidecar.storage.projection_record("chat",chat["chatId"].as_str().unwrap()).unwrap();
        for role in ["coordinator","coding"] {
            let request=sidecar.workflow_request(format!("req_disconnected_{role}"),"run.startAgent",json!({"chatId":chat["chatId"],"roleId":role,"inputMessageIds":[],"expectedCapabilityFingerprint":"unneeded"}));
            let error=if role == "coordinator" {sidecar.handle_agent_request(request).err().unwrap()} else {sidecar.handle_agent_request_inner(request).err().unwrap()};
            assert_eq!(error.kind(),"providerDisconnected");
        }
        for runtime in ["codex","claude-code"] {
            for method in ["agent.getCapabilities","agent.getModelChoices"] {
                let request=sidecar.workflow_request(format!("req_{runtime}_{}",method.replace('.',"_")),method,json!({"runtime":runtime,"refresh":false}));
                assert_eq!(sidecar.handle_agent_request_inner(request).err().unwrap().kind(),"providerDisconnected");
            }
        }
        assert!(sidecar.workflow_chats.is_empty());
        assert!(sidecar.pending_agent_starts.is_empty());
        assert!(sidecar.pending_agent_capabilities.is_empty());
        assert_eq!(sidecar.storage.projection_record("chat",chat["chatId"].as_str().unwrap()).unwrap(),before);
    }
    fn start(sidecar: &mut Sidecar, chat: &Value) -> ResponseV1 {
        start_as(sidecar, chat, "req_workflow_start")
    }
    /// Each request of one conversation has its own ID, as in the app.
    fn start_as(sidecar: &mut Sidecar, chat: &Value, request_id: &str) -> ResponseV1 {
        let id = chat["chatId"].as_str().unwrap();
        let capability = sidecar
            .agent_runtime
            .get_capabilities("claude-code", true)
            .unwrap();
        let ids: Vec<_> = sidecar
            .storage
            .chat_message_history(id)
            .unwrap()
            .iter()
            .map(|m| m["messageId"].clone())
            .collect();
        let request=sidecar.workflow_request(request_id.into(),"run.startAgent",json!({"chatId":id,"roleId":"coordinator","inputMessageIds":ids,"expectedCapabilityFingerprint":capability["fingerprint"]}));
        sidecar
            .handle_agent_request(request)
            .unwrap()
            .response
            .unwrap()
    }
    #[test]
    fn orchestration_dispatch_runs_worker_and_returns_final_without_manual_role_selection() {
        let (root, mut sidecar, chat) = service();
        let response = start(&mut sidecar, &chat);
        assert!(response.ok, "{response:?}");
        let id = chat["chatId"].as_str().unwrap();
        sidecar
            .agent_runtime
            .set_fake_scenario(FakeScenario::OrchestratorWorker);
        let deadline = Instant::now() + Duration::from_secs(30);
        let root_run = response.result.as_ref().unwrap()["runId"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut accepted = std::collections::BTreeSet::from([root_run.clone()]);
        let mut announced = 0;
        loop {
            let (responses, events) = sidecar.poll_runtime_events().unwrap();
            assert!(responses.is_empty(), "internal responses must not escape");
            for event in events {
                if event.event == "agent.teamRunAccepted" {
                    assert_eq!(event.payload["rootRunId"], root_run);
                    assert!(accepted.insert(event.run_id.clone().unwrap()));
                    elsecrew_protocol::decode_jsonl(
                        elsecrew_protocol::encode_jsonl(&EnvelopeV1::Event(event.clone()))
                            .unwrap()
                            .as_bytes(),
                    )
                    .unwrap();
                    announced += 1;
                } else if event.event.starts_with("agent.") {
                    assert!(
                        accepted.contains(event.run_id.as_ref().unwrap()),
                        "child event must follow acceptance"
                    );
                }
            }
            let state = sidecar.workflow_state(id).unwrap();
            if state.role == "coding" && state.run_id.is_some() {
                sidecar
                    .agent_runtime
                    .set_fake_scenario(FakeScenario::OrchestratorFinal);
            }
            if state.status != "running" {
                assert_eq!(state.status, "completed", "{state:?}");
                assert_eq!(state.step, 2);
                break;
            }
            assert!(Instant::now() < deadline, "{state:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            announced, 2,
            "worker and final coordinator both need admission"
        );
        let runs = sidecar.storage.projection_records("run").unwrap();
        assert_eq!(runs.len(), 3);
        let worker = runs
            .iter()
            .find(|run| run["record"]["selectedProfile"]["roleId"] == "coding")
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(
                std::path::Path::new(worker["worktree_path"].as_str().unwrap()).join("hello.txt")
            )
            .unwrap(),
            "updated by the isolated fixture worker\n"
        );
        assert!(runs.iter().all(|run| run["record"]["state"] == "succeeded"));
        assert_eq!(
            sidecar
                .storage
                .projection_record("chat", id)
                .unwrap()
                .unwrap()["record"]["state"],
            "reviewReady"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("source/hello.txt")).unwrap(),
            "original\n"
        );
        let messages = sidecar.storage.chat_message_history(id).unwrap();
        assert!(messages.iter().any(|m| m["roleId"] == "coding"));
        assert!(messages.iter().any(|m| m["roleId"] == "coordinator"
            && m["text"]
                .as_str()
                .is_some_and(|t| t.contains("ready") || t.contains("reported success"))));
    }
    #[test]
    fn read_only_member_report_is_saved_as_a_reviewable_document_in_the_work_copy() {
        let (root, mut sidecar, chat) = service();
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorDelegatePlanning);
        let response = start(&mut sidecar, &chat);
        assert!(response.ok, "{response:?}");
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorPlanner);
        let id = chat["chatId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(id).unwrap();
            if state.role == "planning" && state.run_id.is_some() {
                sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorFinal);
            }
            if state.status != "running" {
                assert_eq!(state.status, "completed", "{state:?}");
                break;
            }
            assert!(Instant::now() < deadline, "{state:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let runs = sidecar.storage.projection_records("run").unwrap();
        let planner = runs.iter()
            .find(|run| run["record"]["selectedProfile"]["roleId"] == "planning")
            .unwrap();
        let worktree = std::path::Path::new(planner["worktree_path"].as_str().unwrap());
        let report = std::fs::read_to_string(worktree.join("docs/elsecrew/planning.md")).unwrap();
        assert!(report.starts_with("<!-- elsecrew:role-report -->\n# 기획 보고서\n"), "{report}");
        assert!(report.contains("Write a short plan for the todo page."));
        assert!(report.contains("3. Keep the list after reload"));
        // The original project is untouched; the report is a reviewable change.
        assert!(!root.path().join("source/docs").exists());
        assert_eq!(
            sidecar.storage.projection_record("chat", id).unwrap().unwrap()["record"]["state"],
            "reviewReady"
        );
        // The coordinator never gets a report file.
        assert!(!worktree.join("docs/elsecrew/coordinator.md").exists());
    }
    use std::os::unix::fs::FileTypeExt;
    #[test]
    fn report_never_blocks_on_a_fifo_or_replaces_a_file_the_app_did_not_write() {
        let (_root, mut sidecar, chat) = service();
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorDelegatePlanning);
        let response = start(&mut sidecar, &chat);
        assert!(response.ok, "{response:?}");
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorPlanner);
        let id = chat["chatId"].as_str().unwrap();
        let root_run = response.result.as_ref().unwrap()["runId"].as_str().unwrap().to_owned();
        let projection = sidecar.storage.projection_record("run", &root_run).unwrap().unwrap();
        let worktree = std::path::PathBuf::from(projection["worktree_path"].as_str().unwrap());
        let reports = worktree.join("docs/elsecrew");
        std::fs::create_dir_all(&reports).unwrap();
        let fifo = std::ffi::CString::new(reports.join("planning.md").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        std::fs::write(reports.join("planning-2.md"), "my own notes\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(id).unwrap();
            if state.role == "planning" && state.run_id.is_some() {
                sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorFinal);
            }
            if state.status != "running" {
                assert_eq!(state.status, "completed", "{state:?}");
                break;
            }
            assert!(Instant::now() < deadline, "{state:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(std::fs::symlink_metadata(reports.join("planning.md")).unwrap().file_type().is_fifo());
        assert_eq!(std::fs::read_to_string(reports.join("planning-2.md")).unwrap(), "my own notes\n");
        let report = std::fs::read_to_string(reports.join("planning-3.md")).unwrap();
        assert!(report.starts_with("<!-- elsecrew:role-report -->\n"), "{report}");
    }
    #[test]
    fn orchestration_notices_use_transport_sequence_independent_of_durable_history() {
        let (_root, mut sidecar, chat) = service();
        let id = chat["chatId"].as_str().unwrap();
        let mut session = ProtocolSessionV1::default();
        session
            .observe(&EnvelopeV1::Event(EventV1 {
                protocol_version: PROTOCOL_VERSION,
                kind: "event".into(),
                sidecar_instance_id: sidecar.ready.sidecar_instance_id.clone(),
                stream_id: "system".into(),
                seq: 1,
                event: "system.ready".into(),
                chat_id: None,
                task_id: None,
                run_id: None,
                payload: serde_json::to_value(&sidecar.ready).unwrap(),
            }))
            .unwrap();
        for expected in 2..=3 {
            let event = orchestration::notice(&mut sidecar.storage, id, "Team progress").unwrap();
            assert_eq!(event.payload["eventSequence"], expected);
            let wire = sidecar.g5_events(vec![event]).pop().unwrap();
            session.observe(&EnvelopeV1::Event(wire)).unwrap();
        }
    }
    #[test]
    fn orchestration_dispatch_direct_answer_does_not_start_any_worker() {
        let (_root, mut sidecar, chat) = service();
        sidecar
            .agent_runtime
            .set_fake_scenario(FakeScenario::OrchestratorFinal);
        assert!(start(&mut sidecar, &chat).ok);
        let id = chat["chatId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while sidecar.workflow_state(id).unwrap().status == "running" {
            sidecar.poll_runtime_events().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "completed");
        assert_eq!(sidecar.storage.projection_records("run").unwrap().len(), 1);
        assert_eq!(
            sidecar
                .storage
                .projection_record("chat", id)
                .unwrap()
                .unwrap()["record"]["state"],
            "waitingForUser"
        );
        assert!(
            !start(&mut sidecar, &chat).ok,
            "status messages must not permit replay of the same user message"
        );
    }
    #[test]
    fn orchestration_followups_after_answer_have_valid_transport_sequences() {
        let (_root, mut sidecar, chat) = service();
        sidecar
            .agent_runtime
            .set_fake_scenario(FakeScenario::OrchestratorFinal);
        assert!(start(&mut sidecar, &chat).ok);
        let id = chat["chatId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while sidecar.workflow_state(id).unwrap().status == "running" {
            sidecar.poll_runtime_events().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut session = ProtocolSessionV1::default();
        session
            .observe(&EnvelopeV1::Event(EventV1 {
                protocol_version: PROTOCOL_VERSION,
                kind: "event".into(),
                sidecar_instance_id: sidecar.ready.sidecar_instance_id.clone(),
                stream_id: "system".into(),
                seq: 1,
                event: "system.ready".into(),
                chat_id: None,
                task_id: None,
                run_id: None,
                payload: serde_json::to_value(&sidecar.ready).unwrap(),
            }))
            .unwrap();
        for index in 0..2 {
            let sequence = sidecar
                .storage
                .chat_message_history(id)
                .unwrap()
                .last()
                .unwrap()["eventSequence"]
                .as_u64()
                .unwrap();
            assert!(sequence > 1);
            let request = json!({"projectId":chat["projectId"],"chatId":chat["chatId"],"taskId":chat["taskId"],
                "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
                "expectedEventSequence":sequence,"clientMessageId":format!("user_{index:032x}"),"text":"Continue"});
            let action = sidecar
                .g5
                .append_user_message(&mut sidecar.storage, &sidecar.projects, &request)
                .unwrap();
            assert_eq!(action.result["eventSequence"], sequence + 1);
            for event in sidecar.g5_events(action.events) {
                session.observe(&EnvelopeV1::Event(event)).unwrap();
            }
            let retry = sidecar
                .g5
                .append_user_message(&mut sidecar.storage, &sidecar.projects, &request)
                .unwrap();
            assert!(retry.events.is_empty());
        }
    }

    #[test]
    fn deleting_a_running_task_stops_its_team_and_run_then_drops_it_from_the_list() {
        let (_root, mut sidecar, chat) = service();
        assert!(start(&mut sidecar, &chat).ok);
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::CancelClean);
        let id = chat["chatId"].as_str().unwrap();
        let project_id = chat["projectId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let run_id = loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(id).unwrap();
            if let Some(run_id) = state.run_id.filter(|run_id| sidecar.agent_runtime.has_active_run(run_id)) {
                break run_id;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        let request = sidecar.workflow_request(
            "req_remove_running".into(),
            "project.catalog",
            json!({"action":"deleteChat","projectId":project_id,"chatId":id,"confirmed":true}),
        );
        let response = sidecar.handle_request(request).unwrap().response.unwrap();
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "canceled");
        assert!(sidecar.g5.list_chats(&sidecar.storage, project_id, None, 100).unwrap()["items"].as_array().unwrap().is_empty());
        // The run ends through its ordinary owner with real termination proof.
        while !sidecar.agent_runtime.owned_run_ids().is_empty() {
            sidecar.poll_runtime_events().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sidecar.storage.projection_record("run", &run_id).unwrap().unwrap()["record"]["state"], "canceled");
    }

    /// Binds a real provider sign-in hold to a run the fake runtime left
    /// needing recovery. No provider, account or model is involved.
    #[cfg(target_os = "macos")]
    fn bind_claude_hold(sidecar: &mut Sidecar, run_id: &str) -> std::path::PathBuf {
        use std::io::Write;
        let profile = crate::provider_claude_profile::ClaudeProfileBinding::from_storage(&sidecar.storage).unwrap();
        let mut owner = crate::provider_login_process::LoginProcess::reserve_claude_owned(profile).unwrap();
        owner.retain_fence_before_launch(run_id).unwrap();
        let binding = owner.bind_run_fence(run_id).unwrap();
        let fence_path = sidecar.storage.path().parent().unwrap().join("claude-login.pending");
        let mut projection = sidecar.storage.projection_record("run", run_id).unwrap().unwrap();
        let mut fence = std::fs::OpenOptions::new().append(true).open(&fence_path).unwrap();
        for process in projection["process_identity_internal"].as_array().unwrap() {
            writeln!(fence, "{} {}", process["pid"], process["start_us"]).unwrap();
        }
        fence.sync_all().unwrap();
        projection.remove("last_event_sequence");
        projection.insert("provider_run_fence".into(), binding.to_value());
        sidecar.storage.upsert_projection_records_atomic(vec![crate::storage::ProjectionUpsert {
            aggregate_type: "run", aggregate_id: run_id.into(), operation_id: "test_bind_hold".into(), request_id: None, payload: projection,
        }]).unwrap();
        owner.finish().unwrap();
        fence_path
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn deleting_a_stopped_task_or_project_settles_it_frees_its_sign_in_hold_and_removes_its_files() {
        for whole_project in [false, true] {
            let (_root, mut sidecar, chat, run_id, fence_path) = stopped_run_with_hold();
            let id = chat["chatId"].as_str().unwrap();
            let project_id = chat["projectId"].as_str().unwrap();
            let worktree_id = chat["worktreeId"].as_str().unwrap();
            let data = sidecar.storage.path().parent().unwrap().to_path_buf();
            let workspace = data.join("worktrees-v1").join(worktree_id);
            let manifest = data.join("worktree-manifests-v1").join(format!("{worktree_id}.json"));
            assert!(workspace.is_dir() && manifest.is_file() && fence_path.is_file());
            // While the task exists, nothing settles it behind the user's back.
            let request = sidecar.workflow_request("req_stopped_idle_finish".into(), "project.catalog", json!({"action":"finishDeletions"}));
            assert!(sidecar.handle_request(request).unwrap().response.unwrap().ok);
            assert_eq!(sidecar.storage.projection_record("run", &run_id).unwrap().unwrap()["recovery"]["state"], "unresolved");
            assert!(fence_path.is_file());
            let params = if whole_project {
                json!({"action":"deleteProject","projectId":project_id,"confirmed":true})
            } else {
                json!({"action":"deleteChat","projectId":project_id,"chatId":id,"confirmed":true})
            };
            let request = sidecar.workflow_request("req_stopped_delete".into(), "project.catalog", params);
            let response = sidecar.handle_request(request).unwrap().response.unwrap();
            assert!(response.ok, "{:?}", response.error);
            let request = sidecar.workflow_request("req_stopped_finish".into(), "project.catalog", json!({"action":"finishDeletions"}));
            let response = sidecar.handle_request(request).unwrap().response.unwrap();
            assert_eq!(response.result.unwrap()["pending"], 0, "{whole_project}");
            let settled = sidecar.storage.projection_record("run", &run_id).unwrap().unwrap();
            assert_eq!(settled["record"]["state"], "canceled", "{whole_project}");
            assert_eq!(settled["recovery"]["state"], "resolvedAbandoned", "{whole_project}");
            assert!(!fence_path.exists(), "the sign-in hold is free again ({whole_project})");
            assert!(!workspace.exists() && !manifest.exists(), "{whole_project}");
            assert!(sidecar.storage.blocker_ids().unwrap().is_empty());
        }
    }

    /// A service whose first run was told to stop but could not prove its end,
    /// with a real sign-in hold bound to that run.
    #[cfg(target_os = "macos")]
    fn stopped_run_with_hold() -> (tempfile::TempDir, Sidecar, Value, String, std::path::PathBuf) {
        let (root, mut sidecar, chat) = service_named(None, elsecrew_protocol::lifecycle_v1::SQLITE_FILE_V1);
        assert!(start(&mut sidecar, &chat).ok);
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::CancelUncertain);
        let id = chat["chatId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let run_id = loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(id).unwrap();
            if let Some(run_id) = state.run_id.filter(|run_id| sidecar.agent_runtime.has_active_run(run_id)) {
                break run_id;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        let field = |key: &str| chat[key].as_str().unwrap().to_owned();
        sidecar.agent_runtime.request_cancel(&agent_runtime::RunIdentity {
            chat_id: field("chatId"), task_id: field("taskId"), project_id: field("projectId"),
            worktree_id: field("worktreeId"), workspace_identity: field("workspaceIdentity"), run_id: run_id.clone(),
        }).unwrap();
        while !sidecar.agent_runtime.owned_run_ids().is_empty() {
            sidecar.poll_runtime_events().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let stopped = sidecar.storage.projection_record("run", &run_id).unwrap().unwrap();
        assert_eq!(stopped["record"]["state"], "recoveryRequired");
        assert_eq!(stopped["recovery"]["state"], "unresolved");
        let fence_path = bind_claude_hold(&mut sidecar, &run_id);
        (root, sidecar, chat, run_id, fence_path)
    }

    /// Quitting interrupts the work; it never resets it. Whether the agent's
    /// processes already ended with the engine, were left running by an engine
    /// that died, or an older app version had marked the run for recovery: the
    /// next start ends the run as canceled, stops what was left running, frees
    /// the sign-in hold, and the same task goes on in the same workspace with
    /// every file the agent had written so far.
    #[test]
    #[cfg(target_os = "macos")]
    fn an_app_closed_mid_run_reopens_with_the_work_kept_and_the_task_usable() {
      use elsecrew_core::proctree::{ProbeResult, ProcIdentity, ProcTable, SystemProcTable};
      for case in ["processes ended", "processes left running", "marked by an older version", "the hold lists a last descendant", "the hold lists hundreds"] {
        let (root, mut sidecar, chat) = service_named(None, elsecrew_protocol::lifecycle_v1::SQLITE_FILE_V1);
        assert!(start(&mut sidecar, &chat).ok);
        // A run that keeps going until it is stopped.
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::CancelClean);
        let id = chat["chatId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let run_id = loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(id).unwrap();
            if let Some(run_id) = state.run_id.filter(|run_id| sidecar.agent_runtime.has_active_run(run_id)) {
                let run = sidecar.storage.projection_record("run", &run_id).unwrap().unwrap();
                if run["process_identity_internal"].as_array().is_some_and(|ids| !ids.is_empty()) {
                    break run_id;
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        let fence_path = bind_claude_hold(&mut sidecar, &run_id);
        assert!(fence_path.is_file());
        let run = sidecar.storage.projection_record("run", &run_id).unwrap().unwrap();
        let mut identities: Vec<_> = run["process_identity_internal"].as_array().unwrap().iter()
            .map(|value| ProcIdentity { pid: value["pid"].as_u64().unwrap() as u32, start_us: value["start_us"].as_i64().unwrap() })
            .collect();
        // An engine that goes down records the last descendants it finds in
        // the hold alone: its ledger is already closed.
        let mut last_descendant = None;
        if case == "the hold lists hundreds" {
            // A long build's hold: hundreds of identities, all long gone.
            use std::io::Write;
            let mut hold = std::fs::OpenOptions::new().append(true).open(&fence_path).unwrap();
            for index in 0..300u32 {
                writeln!(hold, "{} {}", 3_000_000 + index, 1_700_000_000_000_000i64 + index as i64).unwrap();
            }
            hold.sync_all().unwrap();
        }
        if case == "the hold lists a last descendant" {
            use std::io::Write;
            let child = Command::new("/bin/sleep").arg("60").spawn().unwrap();
            let identity = elsecrew_core::proctree::identity_of(child.id()).unwrap();
            let mut hold = std::fs::OpenOptions::new().append(true).open(&fence_path).unwrap();
            writeln!(hold, "{} {}", identity.pid, identity.start_us).unwrap();
            hold.sync_all().unwrap();
            identities.push(identity);
            last_descendant = Some((child, identity));
        }
        // What the agent had written so far.
        let workspace = std::path::PathBuf::from(run["worktree_path"].as_str().unwrap());
        std::fs::write(workspace.join("half-done.txt"), "written before the app closed\n").unwrap();
        let data = sidecar.storage.path().parent().unwrap().to_path_buf();
        let manifest = data.join("worktree-manifests-v1").join(format!("{}.json", chat["worktreeId"].as_str().unwrap()));
        let mut ready = sidecar.ready.clone();
        ready.sidecar_instance_id = "closed_mid_run_reopened".into();
        drop(sidecar);
        let gone = |identity: &ProcIdentity| matches!(SystemProcTable.probe(*identity), ProbeResult::Gone);
        let binary = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("elsecrew-sidecar");
        let open = |ready: &ReadyPayloadV1| Sidecar::initialize(
            root.path().join(elsecrew_protocol::lifecycle_v1::SQLITE_FILE_V1),
            ready.clone(),
            AgentRuntime::fake(binary.clone(), FakeScenario::OrchestratorFinal),
            &AtomicBool::new(false),
        ).unwrap();
        if case == "processes ended" {
            // A closing engine ends its provider processes (`LoginProcess` drop).
            // The fake runtime has no such owner, so end them the same way here.
            for identity in &identities {
                unsafe { libc::kill(identity.pid as i32, libc::SIGKILL) };
            }
            while !identities.iter().all(gone) {
                assert!(Instant::now() < deadline, "agent processes outlived the closed engine");
                std::thread::sleep(Duration::from_millis(10));
            }
        } else {
            // The engine died without stopping its agent.
            assert!(!identities.iter().all(gone));
        }
        if case == "marked by an older version" {
            agent_runtime::SKIP_INTERRUPTED_SETTLEMENT.with(|skip| skip.set(true));
            let marked = open(&ready);
            agent_runtime::SKIP_INTERRUPTED_SETTLEMENT.with(|skip| skip.set(false));
            let run = marked.storage.projection_record("run", &run_id).unwrap().unwrap();
            assert_eq!(run["record"]["state"], "recoveryRequired");
            assert_eq!(run["recovery"]["state"], "unresolved");
            assert_eq!(marked.storage.projection_record("chat", id).unwrap().unwrap()["record"]["state"], "recoveryRequired");
            let stored: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
            assert_eq!(stored["lifecycle"], "recoveryRequired");
            assert!(fence_path.is_file());
            ready.sidecar_instance_id = "closed_mid_run_reopened_again".into();
            drop(marked);
        }
        let mut reopened = open(&ready);
        let run = reopened.storage.projection_record("run", &run_id).unwrap().unwrap();
        assert_eq!(run["record"]["state"], "canceled", "{case}");
        assert!(run["recovery"].is_null(), "{case}");
        assert_eq!(run["termination"]["processesProvenGone"], true, "{case}");
        assert!(identities.iter().all(gone), "what the engine left running is stopped ({case})");
        assert!(!fence_path.exists(), "the sign-in hold is free again ({case})");
        if let Some((mut child, identity)) = last_descendant {
            // Stopped with the rest, and part of the run's record.
            let _ = child.wait();
            assert!(matches!(SystemProcTable.probe(identity), ProbeResult::Gone), "{case}");
        }
        // Proven gone: like a confirmed close, the records keep one identity.
        assert_eq!(run["process_identity_internal"].as_array().unwrap().len(), 1, "{case}");
        assert_eq!(run["termination"]["trackedProcesses"].as_array().unwrap().len(), 1, "{case}");
        assert!(reopened.storage.blocker_ids().unwrap().is_empty(), "{case}");
        reopened.agent_runtime.validate_persisted_runs(&reopened.storage).unwrap();
        // The hold's proof: every process the ledger lists must be in the
        // hold's own record, which may list more (each is probed before release).
        let binding = crate::provider_run_binding::RunFenceBinding::from_value(&run["provider_run_fence"]).unwrap();
        let ledger: std::collections::BTreeSet<ProcIdentity> = run["process_identity_internal"].as_array().unwrap().iter()
            .map(|value| ProcIdentity { pid: value["pid"].as_u64().unwrap() as u32, start_us: value["start_us"].as_i64().unwrap() })
            .collect();
        let proof = |recorded: &std::collections::BTreeSet<ProcIdentity>| agent_runtime::verify_resolved_provider_fence(&reopened.storage, &reopened.projects, &binding, recorded);
        assert!(proof(&ledger).is_ok(), "{case}");
        let mut more = ledger.clone();
        more.insert(ProcIdentity { pid: 3_000_000, start_us: 1_700_000_000_000_000 });
        assert!(proof(&more).is_ok(), "{case}");
        let mut fewer = ledger.clone();
        fewer.pop_first();
        assert!(proof(&fewer).is_err(), "{case}");
        // Nothing was reset: the workspace, its files and the task are as they were.
        let stored: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(stored["lifecycle"], "active", "{case}");
        assert_eq!(std::fs::read_to_string(workspace.join("half-done.txt")).unwrap(), "written before the app closed\n", "{case}");
        let stored_chat = reopened.storage.projection_record("chat", id).unwrap().unwrap();
        assert_eq!(stored_chat["record"]["state"], "canceled", "{case}");
        let history = reopened.storage.chat_message_history(id).unwrap();
        assert!(history.iter().any(|message| message["kind"] == "status"
            && message["text"].as_str().is_some_and(|text| text.contains("작업 공간에 그대로"))), "{case}");
        // The same task continues in the same workspace with a new message.
        let request = reopened.workflow_request("req_continue_message".into(), "chat.appendMessage", json!({
            "projectId":chat["projectId"],"chatId":id,"taskId":chat["taskId"],
            "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
            "expectedEventSequence":stored_chat["durable_event_sequence"],
            "clientMessageId":format!("user_{}", "c".repeat(32)),"text":"Carry on from where it stopped.",
        }));
        let response = reopened.handle_request(request).unwrap().response.unwrap();
        assert!(response.ok, "{case}: {:?}", response.error);
        let response = start_as(&mut reopened, &chat, "req_workflow_continue");
        assert!(response.ok, "{case}: {:?}", response.error);
        let next = reopened.storage.projection_record("chat", id).unwrap().unwrap();
        assert_eq!(next["record"]["state"], "running", "{case}");
        assert_eq!(next["record"]["worktreeId"], chat["worktreeId"], "{case}");
      }
    }

    #[cfg(target_os = "macos")]
    fn resolve_request(sidecar: &Sidecar, run_id: &str) -> Value {
        let recovery = sidecar.storage.projection_record("run", run_id).unwrap().unwrap()["recovery"].clone();
        json!({
            "recoveryId":recovery["recoveryId"],"chatId":recovery["chatId"],"taskId":recovery["taskId"],
            "projectId":recovery["projectId"],"worktreeId":recovery["worktreeId"],
            "workspaceIdentity":recovery["workspaceIdentity"],"runId":recovery["runId"],
            "expectedRevision":recovery["revision"],"resolution":"abandon",
        })
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn discarding_a_stopped_task_frees_its_sign_in_hold_at_once() {
        let (_root, mut sidecar, _chat, run_id, fence_path) = stopped_run_with_hold();
        let params = resolve_request(&sidecar, &run_id);
        let request = sidecar.workflow_request("req_discard_stopped".into(), "recovery.resolveAgent", params);
        let response = sidecar.handle_request(request).unwrap().response.unwrap();
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.result.unwrap()["outcome"], "resolvedAbandoned");
        assert!(!fence_path.exists(), "no separate connection check is needed");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_sign_in_hold_of_a_settled_run_is_freed_when_the_app_starts() {
        let (root, mut sidecar, _chat, run_id, fence_path) = stopped_run_with_hold();
        // Settled without the release that normally follows (as when the app
        // stopped in between, or an older version settled it).
        let params = resolve_request(&sidecar, &run_id);
        let resolved = sidecar.agent_runtime.resolve_recovery(&mut sidecar.storage, &mut sidecar.projects, &params, "req_settle_only").unwrap();
        assert_eq!(resolved["outcome"], "resolvedAbandoned");
        assert!(fence_path.is_file());
        let database = sidecar.storage.path().to_path_buf();
        drop(sidecar);
        let binary = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("elsecrew-sidecar");
        let ready = SidecarSession::new("orchestration_restart".into(), AgentRuntime::blocked()).ready.clone();
        let restarted = Sidecar::initialize(database, ready, AgentRuntime::fake(binary, FakeScenario::OrchestratorDelegate), &AtomicBool::new(false)).unwrap();
        assert!(!fence_path.exists(), "the hold is gone before any provider request");
        assert!(restarted.storage.blocker_ids().unwrap().is_empty());
        drop(root);
    }

    #[test]
    fn orchestration_dispatch_cancel_worker_stops_the_whole_team() {
        let (_root, mut sidecar, chat) = service();
        assert!(start(&mut sidecar, &chat).ok);
        sidecar
            .agent_runtime
            .set_fake_scenario(FakeScenario::CancelClean);
        let id = chat["chatId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let run = loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(id).unwrap();
            if state.role == "coding" {
                if let Some(run_id) = state.run_id {
                    break sidecar
                        .storage
                        .projection_record("run", &run_id)
                        .unwrap()
                        .unwrap()["record"]
                        .clone();
                }
            }
            assert_eq!(state.status, "running");
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut params = serde_json::Map::new();
        for key in [
            "chatId",
            "taskId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
        ] {
            params.insert(key.into(), run[key].clone());
        }
        params.insert("action".into(), json!("cancelTeamTask"));
        params.insert(
            "triggerMessageId".into(),
            json!(sidecar.workflow_state(id).unwrap().trigger_id),
        );
        let request = sidecar.workflow_request(
            "req_workflow_cancel".into(),
            "project.catalog",
            Value::Object(params),
        );
        let response = sidecar.cancel_team_task(request).unwrap().response.unwrap();
        assert!(response.ok);
        assert_eq!(response.result.unwrap()["status"], "stopping");
        assert_ne!(
            sidecar
                .storage
                .projection_record("run", run["runId"].as_str().unwrap())
                .unwrap()
                .unwrap()["record"]["state"],
            "canceled",
            "acknowledgment must not manufacture termination proof"
        );
        while !sidecar.agent_runtime.owned_run_ids().is_empty() {
            sidecar.poll_runtime_events().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "canceled");
        assert_eq!(sidecar.storage.projection_records("run").unwrap().len(), 2);
        assert_eq!(
            sidecar
                .storage
                .projection_record("run", run["runId"].as_str().unwrap())
                .unwrap()
                .unwrap()["record"]["state"],
            "canceled"
        );
    }
    #[test]
    fn orchestration_dispatch_reopen_pauses_queued_work_and_guards_unconsumed_messages() {
        let (root, mut sidecar, chat) = service();
        let id = chat["chatId"].as_str().unwrap();
        let ids: Vec<_> = sidecar
            .storage
            .chat_message_history(id)
            .unwrap()
            .iter()
            .map(|m| m["messageId"].as_str().unwrap().to_owned())
            .collect();
        let input = AgentRunStartRequestV1 {
            chat_id: id.into(),
            role_id: "coordinator".into(),
            input_message_ids: ids,
            expected_capability_fingerprint: "unused".into(),
        };
        orchestration::begin(&mut sidecar.storage, &input, "req_queued").unwrap();
        let append = json!({"chatId":id,"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"clientMessageId":"message_extra","expectedEventSequence":1,"text":"Do something different"});
        assert!(sidecar
            .g5
            .append_user_message(&mut sidecar.storage, &sidecar.projects, &append)
            .is_err());
        assert_eq!(sidecar.storage.chat_message_history(id).unwrap().len(), 1);
        let mut ready = sidecar.ready.clone();
        ready.sidecar_instance_id = "orchestration_reopened".into();
        drop(sidecar);
        let binary = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("elsecrew-sidecar");
        let mut reopened = Sidecar::initialize(
            root.path().join("data.sqlite3"),
            ready,
            AgentRuntime::fake(binary, FakeScenario::OrchestratorFinal),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(reopened.workflow_state(id).unwrap().status, "paused");
        reopened.poll_runtime_events().unwrap();
        assert!(reopened
            .storage
            .projection_records("run")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn orchestration_dispatch_reopen_active_durable_run_ends_it_without_relaunch() {
        let (root, mut sidecar, chat) = service();
        let response = start(&mut sidecar, &chat);
        assert!(response.ok);
        let run_id = response.result.unwrap()["runId"]
            .as_str()
            .unwrap()
            .to_owned();
        let id = chat["chatId"].as_str().unwrap();
        let mut ready = sidecar.ready.clone();
        ready.sidecar_instance_id = "orchestration_crash_reopened".into();
        drop(sidecar);
        let binary = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("elsecrew-sidecar");
        let mut reopened = Sidecar::initialize(
            root.path().join("data.sqlite3"),
            ready,
            AgentRuntime::fake(binary, FakeScenario::OrchestratorFinal),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(reopened.workflow_state(id).unwrap().status, "paused");
        let run = reopened
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        // The crashed engine's processes are gone: the run ended as canceled
        // and the paused team work is not relaunched by itself.
        assert_eq!(run["state"], "canceled");
        assert_eq!(run["record"]["state"], "canceled");
        assert!(run["recovery"].is_null());
        reopened.poll_runtime_events().unwrap();
        assert_eq!(reopened.storage.projection_records("run").unwrap().len(), 1);
    }

    #[test]
    fn orchestration_dispatch_queued_cancel_rejects_stale_target_and_never_launches() {
        let (_root, mut sidecar, chat) = service();
        let id = chat["chatId"].as_str().unwrap();
        let input = AgentRunStartRequestV1 {
            chat_id: id.into(),
            role_id: "coordinator".into(),
            input_message_ids: sidecar
                .storage
                .chat_message_history(id)
                .unwrap()
                .iter()
                .map(|m| m["messageId"].as_str().unwrap().to_owned())
                .collect(),
            expected_capability_fingerprint: "unused".into(),
        };
        orchestration::begin(&mut sidecar.storage, &input, "req_queued_cancel").unwrap();
        sidecar.workflow_chats.insert(id.into());
        let mut params = json!({"action":"cancelTeamTask","chatId":id,"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"triggerMessageId":"stale_message"});
        let stale =
            sidecar.workflow_request("req_stale_cancel".into(), "project.catalog", params.clone());
        assert!(
            !sidecar
                .cancel_team_task(stale)
                .unwrap()
                .response
                .unwrap()
                .ok
        );
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "running");
        params["triggerMessageId"] = json!(input.input_message_ids[0]);
        let request = sidecar.workflow_request("req_queued_stop".into(), "project.catalog", params);
        let stopped = sidecar.cancel_team_task(request).unwrap().response.unwrap();
        assert!(stopped.ok);
        assert_eq!(stopped.result.unwrap()["status"], "stopped");
        let cap =
            sidecar.workflow_request("cap_inflight".into(), "agent.getCapabilities", json!({}));
        sidecar
            .workflow_requests
            .insert(cap.request_id.clone(), (id.into(), "capability".into()));
        let mut responses = vec![success_response(
            &cap,
            json!({"canStart":true,"scope":{"chatId":id,"roleId":"coordinator"}}),
        )];
        sidecar.poll_workflows(&mut responses, &mut vec![]).unwrap();
        assert!(responses.is_empty());
        assert!(sidecar
            .storage
            .projection_records("run")
            .unwrap()
            .is_empty());
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "canceled");
    }

    #[test]
    fn orchestration_dispatch_invalid_decision_pauses_without_a_worker() {
        let (_root, mut sidecar, chat) = service();
        sidecar
            .agent_runtime
            .set_fake_scenario(FakeScenario::CleanReadOnly);
        assert!(start(&mut sidecar, &chat).ok);
        let id = chat["chatId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while sidecar.workflow_state(id).unwrap().status == "running" {
            sidecar.poll_runtime_events().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "paused");
        assert_eq!(sidecar.storage.projection_records("run").unwrap().len(), 1);
        assert!(!start(&mut sidecar, &chat).ok);
    }
    #[test]
    fn orchestration_dispatch_start_failure_never_retries_automatically() {
        let (_root, mut sidecar, chat) = service();
        sidecar
            .agent_runtime
            .set_fake_scenario(FakeScenario::SpawnFailure);
        assert!(!start(&mut sidecar, &chat).ok);
        let id = chat["chatId"].as_str().unwrap();
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "paused");
        let count = sidecar.storage.projection_records("run").unwrap().len();
        for _ in 0..3 {
            sidecar.poll_runtime_events().unwrap();
        }
        assert_eq!(
            sidecar.storage.projection_records("run").unwrap().len(),
            count
        );
    }
    fn custom_team(worker: &str) -> (Value, Value) {
        let profile = |id: &str, permission: &str| json!({"roleId":id,"runtime":"claude-code","modelSelector":"claude-opus-5","reasoningEffort":null,"permission":permission});
        (
            json!([profile("coordinator", "readOnly"), profile("research", "readOnly"), profile(worker, "workspaceWrite")]),
            json!([
                {"roleId":"coordinator","name":"리드","responsibility":"Plans and delegates."},
                {"roleId":"research","name":"Researcher","responsibility":"Investigates libraries and reports sources."},
                {"roleId":worker,"name":"Builder","responsibility":"Edits files in the isolated copy."}
            ]),
        )
    }
    #[test]
    fn orchestration_v3_custom_team_delegates_by_snapshot_and_tracks_writer_permission() {
        let (root, mut sidecar, chat) = service_with(Some(custom_team("builder")));
        assert_eq!(chat["schemaVersion"], 3);
        assert_eq!(chat["roleDefinitions"][2]["name"], "Builder");
        let id = chat["chatId"].as_str().unwrap();
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorDelegateCustom);
        let response = start(&mut sidecar, &chat);
        assert!(response.ok, "{response:?}");
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorWorker);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(id).unwrap();
            if state.role == "builder" && state.run_id.is_some() {
                sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorFinal);
            }
            if state.status != "running" {
                assert_eq!(state.status, "completed", "{state:?}");
                assert!(state.changed, "the workspaceWrite member marks the copy changed");
                break;
            }
            assert!(Instant::now() < deadline, "{state:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let runs = sidecar.storage.projection_records("run").unwrap();
        let worker = runs.iter().find(|run| run["record"]["selectedProfile"]["roleId"] == "builder").unwrap();
        assert_eq!(worker["record"]["stage"], "implementing");
        assert_eq!(worker["record"]["selectedProfile"]["permission"], "workspaceWrite");
        assert_eq!(
            std::fs::read_to_string(std::path::Path::new(worker["worktree_path"].as_str().unwrap()).join("hello.txt")).unwrap(),
            "updated by the isolated fixture worker\n"
        );
        assert_eq!(std::fs::read_to_string(root.path().join("source/hello.txt")).unwrap(), "original\n");
        let stored = sidecar.storage.projection_record("chat", id).unwrap().unwrap();
        assert_eq!(stored["record"]["state"], "reviewReady");
        assert_eq!(stored["record"]["roleDefinitions"], chat["roleDefinitions"]);
        // A user-defined author survives the V13 source-index trigger.
        assert!(sidecar.storage.chat_message_history(id).unwrap().iter().any(|m| m["kind"] == "role" && m["roleId"] == "builder"));
    }
    #[test]
    fn orchestration_v3_prompt_lists_configured_team_names_responsibilities_and_permissions() {
        let (_root, mut sidecar, chat) = service_with(Some(custom_team("builder")));
        let id = chat["chatId"].as_str().unwrap();
        // The coordinator prompt carries the configured team, not the legacy template.
        let ids: Vec<String> = sidecar.storage.chat_message_history(id).unwrap().iter().filter_map(|m| m["messageId"].as_str().map(str::to_owned)).collect();
        let input = AgentRunStartRequestV1 { chat_id: id.into(), role_id: "coordinator".into(), input_message_ids: ids.clone(), expected_capability_fingerprint: "sha256:unused".into() };
        orchestration::begin(&mut sidecar.storage, &input, "prompt-test").unwrap();
        let prompt = orchestration::prompt(&sidecar.storage, id, "coordinator", &ids).unwrap().unwrap();
        for expected in ["\"roleId\":\"research\"", "Investigates libraries", "\"name\":\"Builder\"", "\"permission\":\"workspaceWrite\"", "Only members whose permission is workspaceWrite"] {
            assert!(prompt.contains(expected), "{expected}");
        }
        assert!(!prompt.contains("planning|design|coding|review"));
        assert!(!prompt.contains("Only coding can edit"));
        // After the team data and the conversation, the prompt ends by restating the contract (R4).
        let full = crate::agent_runtime::resolve_prompt(&sidecar.storage, id, &ids, "coordinator").unwrap();
        assert!(full.ends_with(orchestration::CLOSING_REMINDER.as_bytes()));
    }
    #[test]
    fn orchestration_v3_rejects_delegation_to_a_role_outside_the_snapshot() {
        // The fake coordinator delegates to legacy `coding`, which this team does not have.
        let (_root, mut sidecar, chat) = service_with(Some(custom_team("builder")));
        let id = chat["chatId"].as_str().unwrap();
        assert!(start(&mut sidecar, &chat).ok);
        let deadline = Instant::now() + Duration::from_secs(20);
        while sidecar.workflow_state(id).unwrap().status == "running" {
            sidecar.poll_runtime_events().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sidecar.workflow_state(id).unwrap().status, "paused");
        // Asked once more with the team's roleIds (R4); the second miss stops the team.
        let runs = sidecar.storage.projection_records("run").unwrap();
        assert_eq!(runs.len(), 2, "the coordinator ran twice");
        assert!(runs.iter().all(|run| run["record"]["selectedProfile"]["roleId"] == "coordinator"), "no worker was started");
        assert_eq!(sidecar.storage.projection_record("chat", id).unwrap().unwrap()["record"]["state"], "needsAttention");
    }

    // ADE multi-project P2′: a save, merge or discard waits only for its own
    // task, while copying into the user's own folder still waits for every team.

    /// Another task: in the project of `source` (opened, with a team, when new).
    fn another_task(sidecar: &mut Sidecar, source: &std::path::Path, label: &str) -> (Value, std::path::PathBuf) {
        if !source.exists() {
            std::fs::create_dir(source).unwrap();
            std::fs::write(source.join("hello.txt"), "original\n").unwrap();
            for args in [vec!["init", "-q"], vec!["add", "."], vec!["-c", "user.name=Fixture", "-c", "user.email=fixture@example.test", "commit", "-qm", "Initial"]] {
                assert!(Command::new("/usr/bin/git").args(args).current_dir(source).status().unwrap().success());
            }
        }
        let before = sidecar.storage.projection_records("project").unwrap().len();
        let project = sidecar.projects.open_project(&mut sidecar.storage, source.to_str().unwrap(), &format!("req_open_{label}")).unwrap().result;
        let project_id = project["projectId"].as_str().unwrap().to_owned();
        if sidecar.storage.projection_records("project").unwrap().len() > before {
            sidecar.g5.default_role_settings(&mut sidecar.storage, &sidecar.projects, &project_id, &format!("req_defaults_{label}")).unwrap();
            let roles: Vec<_> = ["coordinator", "planning", "design", "coding", "review"].iter().map(|role| json!({"roleId":role,"runtime":"claude-code","modelSelector":"claude-opus-5","reasoningEffort":null,"permission":if *role=="coding"{"workspaceWrite"}else{"readOnly"}})).collect();
            sidecar.g5.update_role_settings(&mut sidecar.storage, &project_id, 1, json!(roles), None, &format!("req_roles_{label}")).unwrap();
        }
        // A Git worktree of its own, as the app makes for a new task.
        let snapshot = sidecar.g5.prepare_chat_workspace_with_baseline(&mut sidecar.storage, &mut sidecar.projects, &project_id, &format!("req_snapshot_{label}"), true).unwrap().result;
        assert!(std::path::Path::new(snapshot["manifest"]["worktreePath"].as_str().unwrap()).join(".git").is_file(), "{snapshot}");
        let chat = sidecar.g5.create_chat(&mut sidecar.storage, &sidecar.projects, snapshot["preparationId"].as_str().unwrap(), &project_id,
            snapshot["manifest"]["worktreeId"].as_str().unwrap(), snapshot["manifest"]["workspaceIdentity"].as_str().unwrap(),
            &format!("Task {label}"), 2, &format!("req_chat_{label}")).unwrap().result;
        (chat, std::path::PathBuf::from(snapshot["manifest"]["worktreePath"].as_str().unwrap()))
    }

    /// Runs a task's team with fake members until its work is ready for review.
    fn run_team_to_review(sidecar: &mut Sidecar, chat: &Value, request_id: &str) {
        let id = chat["chatId"].as_str().unwrap().to_owned();
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorDelegate);
        assert!(start_as(sidecar, chat, request_id).ok);
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorWorker);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(&id).unwrap();
            if state.role == "coding" && state.run_id.is_some() {
                sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorFinal);
            }
            if state.status != "running" { break; }
            assert!(Instant::now() < deadline, "{state:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sidecar.storage.projection_record("chat", &id).unwrap().unwrap()["record"]["state"], "reviewReady");
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorDelegate);
    }

    fn address_of(chat: &Value) -> Value {
        json!({"chatId":chat["chatId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"]})
    }

    fn ask(sidecar: &mut Sidecar, id: &str, method: &str, params: Value) -> ResponseV1 {
        let request = sidecar.workflow_request(id.into(), method, params);
        match sidecar.handle_request(request) {
            Ok(handled) => handled.response.unwrap(),
            Err(error) => panic!("{id}: {} {}", error.kind(), error.detail()),
        }
    }

    fn ask_catalog(sidecar: &mut Sidecar, id: &str, action: &str, chat: &Value, extra: Value) -> ResponseV1 {
        let mut params = address_of(chat);
        params["action"] = json!(action);
        for (key, value) in extra.as_object().unwrap() { params[key] = value.clone(); }
        ask(sidecar, id, "project.catalog", params)
    }

    fn refused_with(response: &ResponseV1) -> Option<&str> {
        (!response.ok).then(|| response.error.as_ref().map(|error| error.code.as_str()).unwrap_or(""))
    }

    fn merge(sidecar: &mut Sidecar, id: &str, chat: &Value) -> ResponseV1 {
        let inspected = ask_catalog(sidecar, &format!("{id}_inspect"), "inspectChatMerge", chat, json!({}));
        assert!(inspected.ok, "{inspected:?}");
        let state = inspected.result.unwrap()["merge"].clone();
        ask_catalog(sidecar, id, "mergeChatWorkspace", chat, json!({"expectedHead":state["head"],"expectedBase":state["baseTip"],"keepConflicts":false,"confirmed":true}))
    }

    fn git_out(dir: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("/usr/bin/git").args(args).current_dir(dir).output().unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    }

    /// While task A's agent keeps writing its workspace, task B of another
    /// repository is saved, merged into its base and looked at for
    /// discarding. Task A's own merge, discard and save wait, and so does
    /// task C of A's repository (one repository, one lock: PR #18 review B1);
    /// copying B into its folder waits for every team. The writer is still
    /// running when B's merge finishes, and afterwards both repositories are
    /// intact, each merge carried only its own files and A's file holds the
    /// writer's last round.
    #[test]
    fn other_tasks_save_merge_and_discard_while_a_team_writes_its_own_workspace() {
        let (root, mut sidecar, _) = service();
        let source = root.path().join("source");
        let (chat_a, _) = another_task(&mut sidecar, &source, "writer");
        let (chat_c, path_c) = another_task(&mut sidecar, &source, "same_repo");
        let other = root.path().join("other");
        let (chat_b, path_b) = another_task(&mut sidecar, &other, "other_project");
        let (chat_d, path_d) = another_task(&mut sidecar, &other, "other_discard");
        assert_eq!(chat_c["projectId"], chat_a["projectId"]);
        assert_ne!(chat_b["projectId"], chat_a["projectId"]);
        std::fs::write(path_c.join("from_c.txt"), "task C\n").unwrap();
        std::fs::write(path_b.join("from_b.txt"), "task B\n").unwrap();

        // D's work is ready for review (only such a task can be discarded).
        run_team_to_review(&mut sidecar, &chat_d, "req_team_d");
        let a = chat_a["chatId"].as_str().unwrap().to_owned();
        assert!(start(&mut sidecar, &chat_a).ok);
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::BusyWriter);
        // Wait until A's developer is writing.
        let deadline = Instant::now() + Duration::from_secs(30);
        let (writer, path_a) = loop {
            sidecar.poll_runtime_events().unwrap();
            let state = sidecar.workflow_state(&a).unwrap();
            if let (true, Some(run)) = (state.role == "coding", state.run_id.clone()) {
                let projection = sidecar.storage.projection_record("run", &run).unwrap().unwrap();
                let path = std::path::PathBuf::from(projection["worktree_path"].as_str().unwrap());
                if sidecar.agent_runtime.has_active_run(&run) && path.join("busy.txt").exists() {
                    break (run, path);
                }
            }
            assert!(Instant::now() < deadline, "{state:?}");
            std::thread::sleep(Duration::from_millis(5));
        };
        sidecar.agent_runtime.set_fake_scenario(FakeScenario::OrchestratorFinal);
        let round = || std::fs::read_to_string(path_a.join("busy.txt")).unwrap_or_default().trim().trim_start_matches("round ").parse::<u32>().unwrap_or(0);
        let first_round = round();

        // A's own writes wait.
        assert_eq!(refused_with(&merge(&mut sidecar, "req_merge_a", &chat_a)), Some("requestBlocked"));
        assert_eq!(refused_with(&ask_catalog(&mut sidecar, "req_inspect_discard_a", "inspectChatDiscard", &chat_a, json!({"taskId":chat_a["taskId"]}))), Some("requestBlocked"));
        let mut save_a = address_of(&chat_a);
        for (key, value) in [("path", json!("hello.txt")), ("expectedVersion", json!("sha256:unused")), ("totalBytes", json!(1)), ("encoding", json!("utf-8"))] { save_a[key] = value; }
        assert_eq!(refused_with(&ask(&mut sidecar, "req_save_a", "workspace.file.save", save_a)), Some("orchestrationBusy"));

        // A's repository is A's: task C waits too.
        assert_eq!(refused_with(&merge(&mut sidecar, "req_merge_c_busy", &chat_c)), Some("requestBlocked"));
        let mut save_c = address_of(&chat_c);
        for (key, value) in [("path", json!("hello.txt")), ("expectedVersion", json!("sha256:unused")), ("totalBytes", json!(1)), ("encoding", json!("utf-8"))] { save_c[key] = value; }
        assert_eq!(refused_with(&ask(&mut sidecar, "req_save_c_busy", "workspace.file.save", save_c)), Some("orchestrationBusy"));
        // Another repository goes on.
        let merged_b = merge(&mut sidecar, "req_merge_b", &chat_b);
        assert!(merged_b.ok, "{merged_b:?}");
        assert_eq!(merged_b.result.as_ref().unwrap()["outcome"], "merged", "{merged_b:?}");
        // Another repository's task is discarded for real.
        let looked = ask_catalog(&mut sidecar, "req_inspect_discard_d", "inspectChatDiscard", &chat_d, json!({"taskId":chat_d["taskId"]}));
        assert!(looked.ok, "{looked:?}");
        let inspection = looked.result.as_ref().unwrap()["inspection"].clone();
        let discarded = ask(&mut sidecar, "req_discard_d", "project.catalog", json!({"action":"discardChatWorkspace","inspection":inspection,"confirmed":true}));
        assert!(discarded.ok, "{discarded:?}");
        // And another's file is saved for real: begin, chunk, commit.
        let before = std::fs::read(path_b.join("hello.txt")).unwrap();
        let version = format!("sha256:{:x}", <sha2::Sha256 as sha2::Digest>::digest(&before));
        let text = "saved while A works\n";
        let mut begin = address_of(&chat_b);
        for (key, value) in [("path", json!("hello.txt")), ("expectedVersion", json!(version)), ("totalBytes", json!(text.len())), ("encoding", json!("utf8"))] { begin[key] = value; }
        let begun = ask(&mut sidecar, "req_save_b", "workspace.file.save", begin);
        assert!(begun.ok, "{begun:?}");
        let save_id = begun.result.as_ref().unwrap()["saveId"].clone();
        let mut chunk = address_of(&chat_b);
        for (key, value) in [("saveId", save_id.clone()), ("seq", json!(1)), ("text", json!(text))] { chunk[key] = value; }
        let chunked = ask(&mut sidecar, "req_save_b_chunk", "workspace.file.save", chunk);
        assert!(chunked.ok, "{chunked:?}");
        let mut commit = address_of(&chat_b);
        for (key, value) in [("saveId", save_id), ("expectedVersion", json!(version)), ("lastSeq", json!(1)), ("totalBytes", json!(text.len()))] { commit[key] = value; }
        let committed = ask(&mut sidecar, "req_save_b_commit", "workspace.file.save", commit);
        assert!(committed.ok, "{committed:?}");
        assert_eq!(std::fs::read_to_string(path_b.join("hello.txt")).unwrap(), text);
        // Copying into the user's own folder still waits for every team.
        let applied = ask(&mut sidecar, "req_apply_b", "project.catalog", json!({"action":"applyChatLatest","chatId":chat_b["chatId"],"projectId":chat_b["projectId"]}));
        assert_eq!(refused_with(&applied), Some("orchestrationBusy"), "{applied:?}");
        // Everything above overlapped A's writer: it wrote on in between.
        let last_round = round();
        assert!(first_round > 0 && last_round > first_round && last_round < 200, "writer rounds {first_round} → {last_round}");
        assert!(sidecar.agent_runtime.has_active_run(&writer), "the writer finished before the other tasks were done");

        let deadline = Instant::now() + Duration::from_secs(30);
        while sidecar.workflow_state(&a).unwrap().status == "running" {
            sidecar.poll_runtime_events().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(path_a.join("busy.txt")).unwrap(), "round 200\n");
        // A idle, C of the same repository merges now.
        assert!(!source.join("from_c.txt").exists());
        let merged_c = merge(&mut sidecar, "req_merge_c", &chat_c);
        assert!(merged_c.ok, "{merged_c:?}");
        assert_eq!(merged_c.result.as_ref().unwrap()["outcome"], "merged", "{merged_c:?}");
        // Each merge carried its own task's file, and only that.
        assert_eq!(std::fs::read_to_string(source.join("from_c.txt")).unwrap(), "task C\n");
        assert_eq!(std::fs::read_to_string(other.join("from_b.txt")).unwrap(), "task B\n");
        assert_eq!(sidecar.storage.projection_record("chat", chat_d["chatId"].as_str().unwrap()).unwrap().unwrap()["record"]["state"], "discarded");
        let _ = &path_d;
        for dir in [&source, &other] {
            assert!(!dir.join("busy.txt").exists());
            assert_eq!(std::fs::read_to_string(dir.join("hello.txt")).unwrap(), "original\n");
            git_out(dir, &["fsck", "--no-progress", "--no-dangling"]);
            assert_eq!(git_out(dir, &["status", "--porcelain"]), "");
        }
        assert!(!git_out(&source, &["log", "--format=%s", "-n", "5"]).is_empty());
        // A, now idle, may merge.
        let merged_a = merge(&mut sidecar, "req_merge_a_after", &chat_a);
        assert!(merged_a.ok, "{merged_a:?}");
        assert_eq!(std::fs::read_to_string(source.join("busy.txt")).unwrap(), "round 200\n");
        git_out(&source, &["fsck", "--no-progress", "--no-dangling"]);
    }

    /// A project folder inside a task's workspace, or holding one, is refused
    /// (also through a link), and so is a workspace folder inside a task's
    /// workspace: a merge or a new task there would write where another
    /// task's agent writes (PR #18 review B1).
    #[test]
    fn a_project_or_workspace_folder_overlapping_a_tasks_workspace_is_refused() {
        let (root, mut sidecar, _) = service();
        let (_, path_a) = another_task(&mut sidecar, &root.path().join("source"), "writer");
        // A repository made inside A's workspace.
        let nested = path_a.join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("x.txt"), "x\n").unwrap();
        for args in [vec!["init", "-q"], vec!["add", "."], vec!["-c", "user.name=Fixture", "-c", "user.email=fixture@example.test", "commit", "-qm", "Initial"]] {
            assert!(Command::new("/usr/bin/git").args(args).current_dir(&nested).status().unwrap().success());
        }
        let opened = sidecar.projects.open_project(&mut sidecar.storage, nested.to_str().unwrap(), "req_open_nested");
        assert!(opened.as_ref().is_err_and(|error| error.detail.contains("overlaps a task's workspace")), "{:?}", opened.map(|action| action.result));
        // The same folder through a link elsewhere.
        let link = root.path().join("link-to-nested");
        std::os::unix::fs::symlink(&nested, &link).unwrap();
        assert!(sidecar.projects.open_project(&mut sidecar.storage, link.to_str().unwrap(), "req_open_link").is_err());
        // A folder holding A's workspace.
        let holder = path_a.parent().unwrap();
        let around = sidecar.projects.refuse_workspace_overlap(&sidecar.storage, holder, "the project folder");
        assert!(around.is_err());
        // A workspace folder inside A's workspace.
        let inner = path_a.join("inner");
        std::fs::create_dir(&inner).unwrap();
        let chosen = sidecar.projects.set_workspace_folder(&sidecar.storage, Some(inner.to_str().unwrap()));
        assert!(chosen.is_err_and(|error| error.detail.contains("inside a task's workspace")));
    }

    /// Nothing is written to a folder that meets a task's workspace before
    /// that is refused: an empty folder gets no Git record, a plain folder no
    /// link file, and a new task's folders are not made (PR #18 re-review B1).
    #[test]
    fn nothing_is_written_before_an_overlapping_folder_is_refused() {
        let (root, mut sidecar, _) = service();
        let (_, path_a) = another_task(&mut sidecar, &root.path().join("source"), "writer");
        let listing = |dir: &std::path::Path| { let mut names: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name()).collect(); names.sort(); names };
        // An empty folder inside A's workspace, opened with the empty-folder yes.
        let empty = path_a.join("empty");
        std::fs::create_dir(&empty).unwrap();
        let response = ask(&mut sidecar, "req_open_empty", "project.open", json!({"selectedPath":empty.to_str().unwrap(),"emptyFolderIdentity":"synthetic"}));
        assert!(!response.ok, "{response:?}");
        assert!(response.error.as_ref().unwrap().detail.contains("overlaps a task's workspace"), "{response:?}");
        assert!(listing(&empty).is_empty(), "nothing written to the empty folder");
        // A plain folder with files, opened with the record yes.
        let plain = path_a.join("plain");
        std::fs::create_dir(&plain).unwrap();
        std::fs::write(plain.join("notes.txt"), "n\n").unwrap();
        let response = ask(&mut sidecar, "req_open_plain", "project.open", json!({"selectedPath":plain.to_str().unwrap(),"folderRecordIdentity":"synthetic"}));
        assert!(!response.ok && response.error.as_ref().unwrap().detail.contains("overlaps a task's workspace"), "{response:?}");
        assert_eq!(listing(&plain), vec![std::ffi::OsString::from("notes.txt")], "no .git link written");
        // A workspace folder saved inside A's workspace (an older setting):
        // a new task there is refused before any folder is made.
        let inside = path_a.join("tasks-here");
        std::fs::create_dir(&inside).unwrap();
        let settings = sidecar.storage.path().parent().unwrap().join("application-settings-v1");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(settings.join("workspace-folder.json"), json!({"path":inside.to_str().unwrap()}).to_string()).unwrap();
        let project_id = sidecar.storage.projection_records("project").unwrap().into_iter()
            .find(|p| p["canonical_path"].as_str().is_some_and(|path| path.ends_with("/source"))).unwrap()["project_id"].as_str().unwrap().to_owned();
        let prepared = sidecar.g5.prepare_chat_workspace_with_baseline(&mut sidecar.storage, &mut sidecar.projects, &project_id, "req_inside", true);
        assert!(prepared.is_err_and(|error| error.detail.contains("overlaps another task's workspace")));
        assert!(listing(&inside).is_empty(), "no project or task folder made");
    }

    /// A workspace folder inside the project itself (`~/elsecrew` as the
    /// project, `~/elsecrew/workspaces` as the folder): the new task is
    /// refused before its project folder is made there (PR #18 review B1,
    /// 3rd round). The same for the app's data and another saved project.
    #[test]
    fn a_task_folder_inside_the_project_itself_is_refused_before_anything_is_made() {
        let (root, mut sidecar, _) = service();
        let source = root.path().join("source");
        let project_id = sidecar.storage.projection_records("project").unwrap().into_iter()
            .find(|p| p["canonical_path"].as_str().is_some_and(|path| path.ends_with("/source"))).unwrap()["project_id"].as_str().unwrap().to_owned();
        let settings = sidecar.storage.path().parent().unwrap().join("application-settings-v1");
        std::fs::create_dir_all(&settings).unwrap();
        let listing = |dir: &std::path::Path| std::fs::read_dir(dir).unwrap().count();
        for (label, home) in [("project", source.join("workspaces")), ("app data", sidecar.storage.path().parent().unwrap().join("tasks-here"))] {
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(settings.join("workspace-folder.json"), json!({"path":std::fs::canonicalize(&home).unwrap().to_str().unwrap()}).to_string()).unwrap();
            let prepared = sidecar.g5.prepare_chat_workspace_with_baseline(&mut sidecar.storage, &mut sidecar.projects, &project_id, &format!("req_inside_{}", label.replace(' ', "_")), true);
            assert!(prepared.is_err_and(|error| error.detail.contains("overlaps the project or the app's data")), "{label}");
            assert_eq!(listing(&home), 0, "{label}: nothing made");
        }
        // Another saved project holding the folder (made with the default folder).
        // (Here the app's data folder holds every test folder too, so either rule answers.)
        std::fs::remove_file(settings.join("workspace-folder.json")).unwrap();
        let (other_chat, _) = another_task(&mut sidecar, &root.path().join("other"), "other");
        let _ = other_chat;
        let home = root.path().join("other").join("tasks");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(settings.join("workspace-folder.json"), json!({"path":std::fs::canonicalize(&home).unwrap().to_str().unwrap()}).to_string()).unwrap();
        let prepared = sidecar.g5.prepare_chat_workspace_with_baseline(&mut sidecar.storage, &mut sidecar.projects, &project_id, "req_inside_other", true);
        assert!(prepared.as_ref().is_err_and(|error| error.detail.contains("overlaps")), "{:?}", prepared.as_ref().map(|_| ()).map_err(|e| e.detail.clone()));
        assert_eq!(listing(&home), 0, "another project: nothing made");
    }

    /// A repository whose .git folder shares another's objects and refs
    /// (`commondir`) is not a repository of its own; names differing only in
    /// case are one folder on macOS (PR #18 re-review B1).
    #[test]
    fn a_shared_common_git_folder_is_refused_and_case_does_not_split_a_folder() {
        let (root, mut sidecar, _) = service();
        let sharing = root.path().join("sharing");
        std::fs::create_dir(&sharing).unwrap();
        std::fs::write(sharing.join("x.txt"), "x\n").unwrap();
        assert!(Command::new("/usr/bin/git").args(["init", "-q"]).current_dir(&sharing).status().unwrap().success());
        std::fs::write(sharing.join(".git/commondir"), format!("{}\n", root.path().join("source/.git").display())).unwrap();
        let opened = sidecar.projects.open_project(&mut sidecar.storage, sharing.to_str().unwrap(), "req_open_sharing");
        assert!(opened.as_ref().is_err_and(|error| error.detail.contains("commondir")), "{:?}", opened.map(|a| a.result).map_err(|e| e.detail));
        if cfg!(target_os = "macos") {
            assert!(crate::project::write_scope_path_within(std::path::Path::new("/Users/Someone/Work/a"), std::path::Path::new("/users/someone/work")));
        }
        assert!(!crate::project::write_scope_path_within(std::path::Path::new("/work/ab"), std::path::Path::new("/work/a")), "whole components only");
    }

    /// Write scopes meet on one Git folder or on folders inside one another.
    #[test]
    fn write_scopes_meet_on_a_shared_repository_or_nested_folders() {
        let (root, mut sidecar, _) = service();
        let (chat_a, _) = another_task(&mut sidecar, &root.path().join("source"), "a");
        let (chat_c, _) = another_task(&mut sidecar, &root.path().join("source"), "c");
        let (chat_b, _) = another_task(&mut sidecar, &root.path().join("other"), "b");
        let scope = |chat: &Value| {
            let address = project::WorkspaceAddress { chat_id: chat["chatId"].as_str().unwrap().into(), project_id: chat["projectId"].as_str().unwrap().into(),
                worktree_id: chat["worktreeId"].as_str().unwrap().into(), workspace_identity: chat["workspaceIdentity"].as_str().unwrap().into() };
            sidecar.projects.task_write_scope(&sidecar.storage, &address).unwrap()
        };
        let (a, c, b) = (scope(&chat_a), scope(&chat_c), scope(&chat_b));
        assert!(a.meets(&c), "one repository, one lock");
        assert!(!a.meets(&b), "separate repositories in separate folders");
        assert!(a.meets(&a));
        let view = sidecar.projects.project_write_scope(&sidecar.storage, chat_a["projectId"].as_str().unwrap()).unwrap();
        assert!(view.meets(&a) && !view.meets(&b));
    }

    /// Something in flight that names no task makes every task busy, as the
    /// earlier app-wide rule did; so does a busy task whose paths cannot be
    /// read.
    #[test]
    fn work_in_flight_without_a_task_blocks_every_task() {
        let (root, mut sidecar, chat_a) = service();
        let (chat_b, path_b) = another_task(&mut sidecar, &root.path().join("other"), "other_project");
        std::fs::write(path_b.join("from_b.txt"), "task B\n").unwrap();
        let odd = sidecar.workflow_request("req_unattributed".into(), "agent.getCapabilities", json!({"runtime":"claude-code","refresh":true,"chatId":7}));
        sidecar.pending_agent_capabilities.insert("req_unattributed".into(), odd);
        assert_eq!(sidecar.busy_chats(), crate::work_scope::BusyChats::Unknown);
        assert_eq!(refused_with(&merge(&mut sidecar, "req_merge_b", &chat_b)), Some("requestBlocked"));
        assert_eq!(refused_with(&ask_catalog(&mut sidecar, "req_discard_a", "inspectChatDiscard", &chat_a, json!({"taskId":chat_a["taskId"]}))), Some("requestBlocked"));
        sidecar.pending_agent_capabilities.clear();
        // A busy task without a readable record meets everything.
        sidecar.workflow_chats.insert("chat_gone".into());
        assert_eq!(refused_with(&merge(&mut sidecar, "req_merge_b_unknown", &chat_b)), Some("requestBlocked"));
        sidecar.workflow_chats.clear();
        // A settings model list (no task) blocks nothing.
        let list = sidecar.workflow_request("req_model_list".into(), "agent.getModelChoices", json!({"runtime":"claude-code","refresh":true,"chatId":null,"roleId":null}));
        sidecar.pending_agent_capabilities.insert("req_model_list".into(), list);
        assert!(sidecar.busy_chats().is_empty());
        let merged = merge(&mut sidecar, "req_merge_b_after", &chat_b);
        assert!(merged.ok, "{merged:?}");
    }

    /// A project's own view: a save waits while a task of that project works,
    /// and a later step of that save is tied to it by its save ID.
    #[test]
    fn a_save_to_a_project_view_waits_only_for_that_projects_tasks() {
        let (root, mut sidecar, chat_a) = service();
        let (chat_b, _) = another_task(&mut sidecar, &root.path().join("other"), "other_project");
        sidecar.workflow_chats.insert(chat_a["chatId"].as_str().unwrap().into());
        let save = |project: &Value| json!({"projectId":project,"path":"hello.txt","expectedVersion":"sha256:unused","totalBytes":1,"encoding":"utf-8"});
        assert_eq!(refused_with(&ask(&mut sidecar, "req_view_a", "file.save", save(&chat_a["projectId"]))), Some("orchestrationBusy"));
        let other = ask(&mut sidecar, "req_view_b", "file.save", save(&chat_b["projectId"]));
        assert_ne!(refused_with(&other), Some("orchestrationBusy"), "{other:?}");
        // An unknown save ID is left to the save's own check.
        let unknown = ask(&mut sidecar, "req_chunk_unknown", "file.save", json!({"saveId":"save_unknown","seq":1,"text":"x"}));
        assert_ne!(refused_with(&unknown), Some("orchestrationBusy"), "{unknown:?}");
        sidecar.workflow_chats.clear();
    }
}
