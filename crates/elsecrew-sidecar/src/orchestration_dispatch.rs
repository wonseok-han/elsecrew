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
            let reason = workflow_failure_message(response.error.as_ref().map(|e| e.code.as_str()).unwrap_or("unknown"));
            self.pause_workflow_reason(&chat, events, &reason)?;
            return Ok(kind != "root");
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
            let request=self.workflow_request(state.request_id.clone(),"run.startAgent",json!({"chatId":chat,"roleId":state.role,"inputMessageIds":state.input_ids,"expectedCapabilityFingerprint":capability["fingerprint"]}));
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
                    self.workflow_requests.remove(&state.request_id);
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
            let id = format!("cap_{}", state.request_id);
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
            root.path().join("data.sqlite3"),
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
        let request=sidecar.workflow_request("req_workflow_start".into(),"run.startAgent",json!({"chatId":id,"roleId":"coordinator","inputMessageIds":ids,"expectedCapabilityFingerprint":capability["fingerprint"]}));
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
        assert!(sidecar
            .projects
            .change_catalog_visibility(
                &mut sidecar.storage,
                chat["projectId"].as_str().unwrap(),
                Some(id),
                "removeChat",
                "req_hidden"
            )
            .is_err());
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
    fn orchestration_dispatch_reopen_active_durable_run_preserves_recovery_requirement() {
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
        assert_eq!(run["state"], "recoveryRequired");
        assert_eq!(run["record"]["state"], "recoveryRequired");
        assert_eq!(run["recovery"]["state"], "unresolved");
        assert_eq!(run["recovery"]["poisoned"], true);
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
        assert_eq!(sidecar.storage.projection_records("run").unwrap().len(), 1, "no worker was started");
    }
}
