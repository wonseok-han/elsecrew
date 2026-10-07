//! Local pipe peer only: no vendor binary, account, network, or model call.
use super::*;
use crate::agent_runtime::preparation::RunPreparation;
use crate::provider_claude_profile::ClaudeProfileBinding;
use crate::provider_login_process::LoginProcess;
use hello_agent_protocol::agent_adapter_v1::{
    compute_agent_capability_fingerprint_v1, AgentCapabilitySnapshotV1,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn quoted(value: &Value) -> String {
    format!("'{}'", value.to_string().replace('\'', "'\\''"))
}

#[test]
fn claude_native_owner_runs_prompt_output_approval_cancel_and_rejects_bad_init() {
    for case in ["success", "allow", "cancel", "bad-init", "unknown-queue"] {
        let mut f = Fixture::new();
        let snapshot = f.snapshot("req_claude_snapshot");
        let chat = f.create_exact_claude_chat(&snapshot, "goal", "req_claude_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        let mut saved = f
            .storage
            .projection_record("chat", chat_id)
            .unwrap()
            .unwrap();
        saved.get_mut("record").unwrap()["roleSnapshots"][1]["reasoningEffort"] = json!("medium");
        f.storage
            .upsert_projection_record("chat", chat_id, "op_claude_medium", None, saved)
            .unwrap();
        let mut fixture_runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
        let mut capability: AgentCapabilitySnapshotV1 = serde_json::from_value(
            fixture_runtime
                .get_capabilities("claude-code", true)
                .unwrap(),
        )
        .unwrap();
        for model in &mut capability.models {
            model.reasoning_efforts = vec!["medium".into()];
        }
        capability.fingerprint = compute_agent_capability_fingerprint_v1(&capability).unwrap();
        let request = AgentRunStartRequestV1 {
            chat_id: chat_id.into(),
            role_id: "coding".into(),
            input_message_ids: vec![initial_message_id(&f, chat_id)],
            expected_capability_fingerprint: capability.fingerprint.clone(),
        };
        let mut reservation = RunPreparation::resolve(&f.storage, &f.projects, &request)
            .unwrap()
            .reserve(
                &mut f.storage,
                &f.projects,
                &capability,
                "req_claude_reserve",
            )
            .unwrap();
        let run = f.storage.projection_records("run").unwrap()[0]["record"].clone();
        let identity = RunIdentity {
            chat_id: chat_id.into(),
            task_id: run["taskId"].as_str().unwrap().into(),
            project_id: run["projectId"].as_str().unwrap().into(),
            worktree_id: run["worktreeId"].as_str().unwrap().into(),
            workspace_identity: run["workspaceIdentity"].as_str().unwrap().into(),
            run_id: run["runId"].as_str().unwrap().into(),
        };
        let binding = ClaudeProfileBinding::from_storage(&f.storage).unwrap();
        let mut process = LoginProcess::reserve_claude_owned(binding).unwrap();
        let cwd = snapshot["worktreePath"].as_str().unwrap();
        let control = json!({"type":"control_response","response":{"subtype":"success",
            "request_id":format!("init_{}",process.connection_id().simple()),"response":{
                "hooks_applied":true,"current_permission_mode":"default","models":[{
                    "value":"claude-opus-5","resolvedModel":"claude-opus-5","supportsEffort":true,"supportedEffortLevels":["medium"]}]}}});
        let init = json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":cwd,
            "model":if case == "bad-init" {"other"} else {"claude-opus-5"},"permissionMode":"default",
            "tools":["Read","Glob","Grep","Edit","Write","Bash"],"mcp_servers":[],"skills":[],"plugins":[]});
        let approval = json!({"type":"control_request","request_id":"write_once","request":{
            "subtype":"can_use_tool","tool_name":"Write","tool_use_id":"tool_write","input":{"file_path":format!("{cwd}/new.txt"),"content":"hello"}}});
        let assistant = json!({"type":"assistant","uuid":"answer","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m","role":"assistant","content":[{"type":"text","text":"hello"}]}});
        let mut result = json!({"type":"result","uuid":"result","session_id":"s","subtype":"success","is_error":false,"permission_denials":[],"queued_turn_count":0,"result":"hello","errors":[]});
        if case == "unknown-queue" {
            result.as_object_mut().unwrap().remove("queued_turn_count");
        }
        let mut script = format!("IFS= read -r init || exit 1\nprintf '%s\\n' {}\nIFS= read -r prompt || exit 2\nprintf '%s\\n' {}\n", quoted(&control), quoted(&init));
        if case == "allow" {
            script.push_str(&format!("printf '%s\\n' {}\nIFS= read -r reply || exit 3\ncase \"$reply\" in *'\"behavior\":\"allow\"'*) ;; *) exit 4 ;; esac\n", quoted(&approval)));
        }
        if case != "cancel" {
            script.push_str(&format!(
                "printf '%s\\n' {} {}\n",
                quoted(&assistant),
                quoted(&result)
            ));
        }
        script.push_str("IFS= read -r end\n");
        reservation
            .launch_claude_toy(&mut f.storage, &f.projects, &mut process, &script)
            .unwrap();
        let admission = Arc::new(
            reservation
                .claude_admission(&f.storage, &f.projects, &process)
                .unwrap(),
        );
        let tool_admission = admission.clone();
        let mut reserved = Some(reservation);
        let mut owned_process = Some(process);
        let mut runtime = AgentRuntime::native_candidate();
        runtime
            .register_claude_start(
                &f.storage,
                &f.projects,
                &mut reserved,
                &mut owned_process,
                Box::new(move |process| admission.process(process)),
                Box::new(move |process, request| tool_admission.tool(process, request)),
            )
            .unwrap();
        assert!(reserved.is_none() && owned_process.is_none());
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut approved = false;
        let mut canceled = false;
        let mut events = Vec::new();
        while runtime.has_active_run(&identity.run_id) {
            assert!(Instant::now() < deadline, "{case}: owner did not terminate");
            events.extend(runtime.poll(&mut f.storage, &mut f.projects).unwrap());
            let current = f
                .storage
                .projection_record("run", &identity.run_id)
                .unwrap()
                .unwrap();
            if current["state"] == "running" && case == "cancel" && !canceled {
                runtime.request_cancel(&identity).unwrap();
                canceled = true;
            }
            if current["state"] == "running" && case == "allow" && !approved {
                let binding = json!({"chatId":identity.chat_id,"taskId":identity.task_id,"projectId":identity.project_id,
                    "worktreeId":identity.worktree_id,"workspaceIdentity":identity.workspace_identity,"runId":identity.run_id});
                let list = runtime
                    .approval_request(
                        &mut f.storage,
                        &f.projects,
                        &json!({"action":"list","identity":binding}),
                    )
                    .unwrap();
                if let Some(first) = list["pending"].as_array().unwrap().first() {
                    let id = &first["approvalId"];
                    assert!(runtime
                        .approval_request(
                            &mut f.storage,
                            &f.projects,
                            &json!({"action":"acceptOnce","identity":binding,"approvalId":id})
                        )
                        .is_err());
                    runtime
                        .approval_request(
                            &mut f.storage,
                            &f.projects,
                            &json!({"action":"inspect","identity":binding,"approvalId":id}),
                        )
                        .unwrap();
                    runtime
                        .approval_request(
                            &mut f.storage,
                            &f.projects,
                            &json!({"action":"acceptOnce","identity":binding,"approvalId":id}),
                        )
                        .unwrap();
                    approved = true;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let ended = f
            .storage
            .projection_record("run", &identity.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            ended["state"],
            match case {
                "cancel" => "canceled",
                "bad-init" | "unknown-queue" => "failed",
                _ => "succeeded",
            },
            "{case}: {ended:?}"
        );
        assert_eq!(ended["native_transport"]["adapter"], "claude.stream.v2");
        assert!(ended["claude_preflight"].is_object());
        assert_eq!(
            ended
                .get("claude_session_observation")
                .is_some_and(Value::is_object),
            case != "bad-init"
        );
        assert!(
            ended["termination"]["processesProvenGone"]
                .as_bool()
                .unwrap(),
            "{case}"
        );
        assert_eq!(approved, case == "allow");
        let final_text: Vec<_> = ended["public_events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] == "final")
            .map(|event| event["text"].clone())
            .collect();
        assert_eq!(
            final_text,
            if matches!(case, "success" | "allow") {
                vec![json!("hello")]
            } else {
                vec![]
            },
            "{case}"
        );
        assert!(!events.is_empty());
        let mut reusable = LoginProcess::reserve_claude_owned(
            ClaudeProfileBinding::from_storage(&f.storage).unwrap(),
        )
        .unwrap();
        reusable.finish().unwrap();
        let database = f._root.path().join("data/hello-agent.sqlite3");
        drop(f.storage);
        let mut storage = crate::Storage::open(&database).unwrap();
        AgentRuntime::blocked()
            .recover_unclean_runs(&mut storage, &mut f.projects)
            .unwrap();
        assert_eq!(
            storage
                .projection_record("run", &identity.run_id)
                .unwrap()
                .unwrap(),
            ended
        );
    }
}
