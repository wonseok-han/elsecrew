//! Local pipe peer only: no vendor binary, account, network, or model call.
use super::*;
use crate::agent_runtime::preparation::RunPreparation;
use crate::provider_claude_profile::ClaudeProfileBinding;
use crate::provider_login_process::LoginProcess;
use elsecrew_protocol::agent_adapter_v1::{
    compute_agent_capability_fingerprint_v1, AgentCapabilitySnapshotV1,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn quoted(value: &Value) -> String {
    format!("'{}'", value.to_string().replace('\'', "'\\''"))
}

#[test]
fn claude_project_consent_auto_approves_and_still_rejects_outside_files() {
    claude_native_cases(&[("auto", false), ("auto-outside", false), ("auto-outside-foreign", false), ("auto-missing", false), ("auto-missing-hook", false), ("auto-read", false), ("auto-read", true), ("auto-read-failure", true)]);
}

/// Real Claude, after an allowed Bash call, runs child processes and writes
/// into the copy (a new file, its own `.claude/.cc-writes`, `git status`)
/// before asking for the next tool. The next approval must still be delivered.
#[test]
fn claude_auto_read_after_real_bash_side_effects() {
    claude_native_cases(&[("auto-read-mutate", true), ("auto-read-index-lock", true)]);
}

#[test]
fn claude_native_owner_runs_prompt_output_approval_cancel_and_rejects_bad_init() {
    claude_native_cases(&[
        ("success", false),
        ("organization-guard", false),
        ("privacy", false),
        ("privacy-with-guard", true),
        ("default-effort", false),
        ("allow", false),
        ("cancel", false),
        ("bad-init", false),
        ("unknown-queue", false),
        ("unexplained-denial", false),
        ("success", true),
        ("cancel", true),
        ("bad-init", true),
    ]);
}

/// A run that starts far more short-lived processes than a record may list
/// (a build or an install) finishes, keeps small records, and frees its hold.
#[test]
fn claude_run_that_starts_hundreds_of_processes_finishes_and_frees_its_hold() {
    claude_native_cases(&[("many-processes", true)]);
}

#[test]
fn claude_durable_native_three_thousand_outputs() {
    claude_native_cases(&[("long", true)]);
}

fn claude_native_cases(cases: &[(&str, bool)]) {
    for &(case, durable) in cases {
        let mut f = Fixture::with_durable_output(durable);
        if case.starts_with("auto") {
            f.projects.automation_settings(&mut f.storage, &f.project_id,
                Some(crate::project::automation_permissions::AutomationPermissions {files:true,commands:true}), "req_auto_consent").unwrap();
        }
        let snapshot = f.snapshot("req_claude_snapshot");
        let chat = f.create_exact_claude_chat(&snapshot, "goal", "req_claude_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        let mut saved = f
            .storage
            .projection_record("chat", chat_id)
            .unwrap()
            .unwrap();
        saved.get_mut("record").unwrap()["roleSnapshots"][1]["reasoningEffort"] =
            if case == "default-effort" {
                Value::Null
            } else {
                json!("medium")
            };
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
            model.reasoning_efforts = if case == "default-effort" {
                vec![]
            } else {
                vec!["medium".into()]
            };
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
                    "value":"claude-opus-5","resolvedModel":"claude-opus-5","supportsEffort":case != "default-effort","supportedEffortLevels":if case == "default-effort" {json!([])} else {json!(["medium"])} }]}}});
        let mut init = json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":cwd,
            "model":if case == "bad-init" {"other"} else {"claude-opus-5"},"permissionMode":"default",
            "tools":["Read","Glob","Grep","Edit","Write","Bash"],"mcp_servers":[],"skills":[],"plugins":[]});
        if case == "organization-guard" {
            init["plugins"] = json!([{"name":"cc-plugin-sec-default","path":"builtin","source":"cc-plugin-sec-default@builtin"}]);
        }
        if matches!(case, "privacy" | "privacy-with-guard") {
            init["plugins"] = json!([{"name":"cc-plugin-privacy","path":"builtin","source":"cc-plugin-privacy@builtin"}]);
            if case == "privacy-with-guard" {
                init["plugins"].as_array_mut().unwrap().insert(0, json!({"name":"cc-plugin-sec-default","path":"builtin","source":"cc-plugin-sec-default@builtin"}));
            }
        }

        let mut approval = json!({"type":"control_request","request_id":"write_once","request":{
            "subtype":"can_use_tool","tool_name":"Write","tool_use_id":"tool_write","input":{"file_path":format!("{cwd}/new.txt"),"content":"hello"}}});
        if matches!(case, "auto-outside" | "auto-outside-foreign") { approval["request"]["input"]["file_path"] = json!("/outside/new.txt"); }
        // The real incident: a planner reads a file the empty project lacks.
        if case == "auto-missing" {
            approval["request"]["tool_name"] = json!("Read");
            approval["request"]["input"] = json!({"file_path":format!("{cwd}/README.md")});
        }
        // Real Claude checks every tool through the PreToolUse hook first.
        if case == "auto-missing-hook" {
            approval = json!({"type":"control_request","request_id":"hook_once","request":{
                "subtype":"hook_callback","callback_id":elsecrew_core::provider::vendor_observation::claude_approval::PRE_TOOL_CALLBACK,
                "tool_use_id":"tool_read","input":{"session_id":"s","cwd":cwd,"hook_event_name":"PreToolUse",
                    "tool_name":"Read","tool_use_id":"tool_read","tool_input":{"file_path":format!("{cwd}/README.md")}}}});
        }
        if case.starts_with("auto-read") {
            fs::write(std::path::Path::new(cwd).join("index.html"), b"fixture").unwrap();
            approval["request"]["tool_name"] = json!("Read");
            approval["request"]["input"] = json!({"file_path":format!("{cwd}/index.html")});
        }
        let assistant = json!({"type":"assistant","uuid":"answer","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m","role":"assistant","content":[{"type":"text","text":"hello"}]}});
        let mut result = json!({"type":"result","uuid":"result","session_id":"s","subtype":"success","is_error":false,"permission_denials":[],"queued_turn_count":0,"result":"hello","errors":[]});
        if case == "unknown-queue" {
            result.as_object_mut().unwrap().remove("queued_turn_count");
        }
        // Real Claude lists the tool uses that were refused.
        let denied = |ids: &[&str]| json!(ids.iter().map(|id| json!({"tool_name":"Read","tool_use_id":id,"tool_input":{}})).collect::<Vec<_>>());
        match case {
            "auto-outside" | "auto-missing" => result["permission_denials"] = denied(&["tool_write"]),
            "auto-missing-hook" => result["permission_denials"] = denied(&["tool_read"]),
            "unexplained-denial" => result["permission_denials"] = denied(&["tool_other"]),
            "auto-outside-foreign" => result["permission_denials"] = denied(&["tool_write", "tool_other"]),
            _ => {}
        }
        let mut script = format!("IFS= read -r init || exit 1\nprintf '%s\\n' {}\nIFS= read -r prompt || exit 2\nprintf '%s\\n' {}\n", quoted(&control), quoted(&init));
        if case.starts_with("auto-read") {
            // Synthetic secrets (they say FAKE) in the command, its summary and
            // what it printed: stored masked or not at all (review PR #11).
            // Review PR #11 round 2: an inline Authorization header and a quoted
            // curl password (`:` joined at run time so scanners do not read it).
            let command = format!("GITHUB_TOKEN=ghp_FAKEexample0FAKEexample0FAKEexample0 gh auth status && curl --header='Authorization: Bearer FAKEexampleInlineHeader' -u 'fakeuser{}FAKE example pass' https://api.example.com", ":");
            let bash = json!({"type":"control_request","request_id":"bash_once","request":{
                "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"tool_bash","input":{
                    "command":command,
                    "description":"check login with token=FAKEexampleNoteSecret"}}});
            let printed = json!({"type":"user","uuid":"bash-result","session_id":"s","parent_tool_use_id":null,
                "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool_bash","is_error":false,"content":"FAKE-OUTPUT-MARKER printed"}]},
                "tool_use_result":{"stdout":"FAKE-OUTPUT-MARKER printed","stderr":"","interrupted":false}});
            script.push_str(&format!("printf '%s\\n' {}\nIFS= read -r reply || exit 3\ncase \"$reply\" in *'\"behavior\":\"allow\"'*) ;; *) exit 4 ;; esac\nprintf '%s\\n' {}\n", quoted(&bash), quoted(&printed)));
        }
        if case == "auto-read-index-lock" {
            // A concurrent provider `git status` briefly owns the index lock while
            // the next approval is validated.
            script.push_str("( : > .git/index.lock; /bin/sleep 3; /bin/rm -f .git/index.lock ) &\n/bin/sleep 0.3\n");
        }
        if case == "auto-read-mutate" {
            script.push_str("/bin/mkdir -p .claude && : > .claude/.cc-writes && printf 'draft' > draft.html && /usr/bin/git status --porcelain >/dev/null 2>&1; /bin/sleep 1\n");
        }
        if case == "allow" || case.starts_with("auto") {
            // A request outside the boundary is refused on its own and the run goes on.
            let expected = match case {
                "auto-outside" | "auto-outside-foreign" | "auto-missing" => r#""behavior":"deny""#,
                "auto-missing-hook" => r#""permissionDecision":"deny""#,
                _ => r#""behavior":"allow""#,
            };
            script.push_str(&format!("printf '%s\\n' {}\nIFS= read -r reply || exit 3\ncase \"$reply\" in *'{expected}'*) ;; *) exit 4 ;; esac\n", quoted(&approval)));
        }
        if case == "many-processes" {
            // 8 waves of 40 children, each alive long enough to be observed.
            script.push_str("i=0; while [ $i -lt 8 ]; do j=0; while [ $j -lt 40 ]; do /bin/sleep 0.4 & j=$((j+1)); done; wait; i=$((i+1)); done\n");
        }
        if case == "long" {
            let path = f._root.path().join("claude-long-output.jsonl");
            let mut body = String::new();
            for index in 0..2999 {
                let mut frame = assistant.clone();
                frame["uuid"] = json!(format!("answer-{index}"));
                frame["message"]["id"] = json!(format!("message-{index}"));
                body.push_str(&frame.to_string());
                body.push('\n');
            }
            body.push_str(&result.to_string());
            body.push('\n');
            fs::write(&path, body).unwrap();
            script.push_str(&format!("/bin/cat '{}' || exit 5\n", path.to_str().unwrap().replace('\'', "'\\''")));
        } else if case != "cancel" {
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
        let inject_read_failure = case == "auto-read-failure";
        runtime
            .register_claude_start(
                &f.storage,
                &f.projects,
                &mut reserved,
                &mut owned_process,
                Box::new(move |process| admission.process(process)),
                Box::new(move |process, request| {
                    if inject_read_failure && request.tool_name == "Read" {
                        return Err(SidecarError::new("agentApprovalChanged", "PRIVATE token /secret/path"));
                    }
                    tool_admission.tool(process, request)
                }),
            )
            .unwrap();
        assert!(reserved.is_none() && owned_process.is_none());
        let deadline = Instant::now() + Duration::from_secs(if case == "long" { 660 } else { 15 });
        if case.starts_with("auto") { assert!(f.projects.automation_permissions(&f.storage, &f.project_id).unwrap().files, "consent survived workspace preparation"); }
        let mut approved = false;
        let mut canceled = false;
        let mut events = Vec::new();
        while runtime.has_active_run(&identity.run_id) {
            assert!(Instant::now() < deadline, "{case}: owner did not terminate");
            events.extend(runtime.poll(&mut f.storage, &mut f.projects)
                .unwrap_or_else(|error| panic!("{case} durable={durable}: {error:?}")));
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
                    // Viewing a request no longer waiting shows nothing and
                    // answers nothing; answering it again still refuses.
                    let viewed = runtime
                        .approval_request(
                            &mut f.storage,
                            &f.projects,
                            &json!({"action":"inspect","identity":binding,"approvalId":id}),
                        )
                        .unwrap();
                    assert_eq!(viewed["inspection"], Value::Null);
                    assert_eq!(viewed["queued"], Value::Null);
                    assert!(!viewed["pending"].as_array().unwrap().iter().any(|item| &item["approvalId"] == id));
                    for action in ["acceptOnce", "decline"] {
                        assert!(runtime
                            .approval_request(
                                &mut f.storage,
                                &f.projects,
                                &json!({"action":action,"identity":binding,"approvalId":id})
                            )
                            .is_err());
                    }
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
                "bad-init" | "unknown-queue" | "unexplained-denial" | "auto-outside-foreign" | "auto-read-failure" => "failed",
                _ => "succeeded",
            },
            "{case}: {ended:?}"
        );
        if case == "many-processes" {
            // Confirmed close: the records keep the root alone, all proven gone.
            assert_eq!(ended["termination"]["processesProvenGone"], true);
            assert_eq!(ended["termination"]["trackedProcesses"].as_array().unwrap().len(), 1);
            assert_eq!(ended["process_identity_internal"].as_array().unwrap().len(), 1);
            assert_eq!(ended["barrier"]["trackedProcesses"], ended["termination"]["trackedProcesses"]);
        }
        if case == "auto-read-failure" {
            assert_eq!(ended["record"]["failure"], json!({"stage":"approvalDelivery","code":"approvalUnavailable"}));
            assert!(!serde_json::to_string(&ended).unwrap().contains("PRIVATE"));
            assert!(!serde_json::to_string(&ended).unwrap().contains("/secret/path"));
        }
        assert_eq!(ended["native_transport"]["adapter"], "claude.stream.v2");
        assert!(ended["claude_preflight"].is_object());
        if case == "organization-guard" {
            assert_eq!(ended["claude_session_observation"]["observedBuiltinPlugins"], json!(["cc-plugin-sec-default@builtin"]));
        }
        if matches!(case, "privacy" | "privacy-with-guard") {
            let mut expected = vec!["cc-plugin-privacy@builtin"];
            if case == "privacy-with-guard" { expected.push("cc-plugin-sec-default@builtin"); }
            assert_eq!(ended["claude_session_observation"]["observedBuiltinPlugins"], json!(expected));
        }
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
        let public = if durable {
            assert!(ended.get("public_events").is_none());
            assert!(!events.iter().any(|event| event.event == "agent.publicEvent"));
            let binding = serde_json::from_value(json!({
                "chatId":identity.chat_id,"taskId":identity.task_id,"projectId":identity.project_id,
                "worktreeId":identity.worktree_id,"workspaceIdentity":identity.workspace_identity,
                "runId":identity.run_id,"roleId":"coding","capabilityFingerprint":capability.fingerprint
            })).unwrap();
            let head = f.storage.agent_output_head(&binding).unwrap();
            assert!(head.sealed);
            if case == "long" { assert_eq!(head.committed_sequence, 3000); }
            durable_output::read_all(&mut f.storage, &ended["record"])
        } else { ended["public_events"].as_array().unwrap().clone() };
        let final_text: Vec<_> = public
            .iter()
            .filter(|event| event["kind"] == "final")
            .map(|event| event["text"].clone())
            .collect();
        assert_eq!(
            final_text,
            if matches!(case, "success" | "long" | "many-processes" | "organization-guard" | "privacy" | "privacy-with-guard" | "default-effort" | "allow" | "auto" | "auto-outside" | "auto-missing" | "auto-missing-hook" | "auto-read" | "auto-read-mutate" | "auto-read-index-lock") {
                vec![json!("hello")]
            } else {
                vec![]
            },
            "{case}"
        );
        assert!(!events.is_empty());
        if durable && matches!(case, "auto-read" | "auto-read-mutate" | "auto-read-index-lock") {
            // Each allowed tool use leaves a work-history row: the command line
            // itself (masked) with the model's summary as a note (2026-11-26),
            // a file by its workspace path, the app's own ids; how the command
            // ended (no exit code is made up), never what it printed.
            let mut history: Vec<Value> = public.iter().filter(|event| event["kind"] == "status")
                .filter_map(|event| serde_json::from_str::<Value>(event["text"].as_str()?).ok())
                .filter(|row| row.get("action").is_some()).collect();
            assert!(history[1]["ms"].is_u64(), "{case}");
            history[1].as_object_mut().unwrap().remove("ms");
            let shown = "GITHUB_TOKEN=•••• gh auth status && curl --header=•••• -u •••• https://api.example.com";
            let command = json!({"action":"command","target":shown,"command":shown,"note":"check login with token=••••","id":"t1"});
            let mut ended = command.clone();
            ended["phase"] = json!("done");
            assert_eq!(history, vec![command, ended, json!({"action":"read","target":"index.html","id":"t2"})], "{case}");
            // Read back from the store's files: no secret, no output, no provider id in a row.
            super::assert_store_lacks(f._root.path(), &["FAKEexample0FAKEexample0", "FAKEexampleNoteSecret", "FAKE-OUTPUT-MARKER", "FAKEexampleInlineHeader", "FAKE example pass"]);
            assert!(!public.iter().any(|event| event["text"].as_str().is_some_and(|text| text.contains("tool_bash") || text.contains("tool_write"))));
        }
        if case == "long" {
            assert_eq!(public.len(), 3000);
            assert!(public.iter().all(|event| event["text"] == "hello"));
            assert!(events.iter().any(|event| event.event == "agent.outputAvailable" && event.payload["sealed"] == true));
        }
        let mut reusable = LoginProcess::reserve_claude_owned(
            ClaudeProfileBinding::from_storage(&f.storage).unwrap(),
        )
        .unwrap();
        reusable.finish().unwrap();
        let database = f._root.path().join("data/elsecrew.sqlite3");
        drop(f.storage);
        let mut storage = if durable {
            crate::Storage::open_durable_output_for_test(&database).unwrap()
        } else { crate::Storage::open(&database).unwrap() };
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
        if case == "long" {
            assert_eq!(durable_output::read_all(&mut storage, &ended["record"]), public);
        }
    }
}
