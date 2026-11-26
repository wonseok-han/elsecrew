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

/// The shared command cases (every review counterexample and 27 more), as
/// the lines a provider would send; placeholders put back.
fn summary_cases() -> Vec<(String, Value)> {
    let cases: Value = serde_json::from_str(include_str!("../../../../../tests/fixtures/m5/secret-mask-cases.json")).unwrap();
    cases["summary"].as_array().unwrap().iter().map(|case| {
        let text = case["text"].as_str().unwrap().replace("{colon}", ":").replace("{dot}", ".").replace("{nl}", "\n").replace("{eq}", "=").replace("{sp}", " ");
        let mut row = json!({"action":"command"});
        if let Some(program) = case["program"].as_str() { row["program"] = json!(program); }
        if let Some(subcommand) = case["subcommand"].as_str() { row["subcommand"] = json!(subcommand); }
        if case["hidden"].as_u64().unwrap() > 0 { row["hiddenArgs"] = case["hidden"].clone(); }
        (text, row)
    }).collect()
}

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

/// 2026-11-26 big Write, PR #19 review B1/I1/I2: a 300 KB Write request
/// waiting for the person. It is read in pages and can be allowed only after
/// every page; its large-input budget comes back on every path: allowed and
/// written, declined, left unanswered when the run ends, or allowed but the
/// provider stopped reading.
#[test]
fn claude_large_tool_input_is_read_whole_before_allowing_and_its_budget_always_returns() {
    let _slots = crate::provider_login_io::LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    claude_native_cases(&[("big-allow", false), ("big-decline", false), ("big-end", false), ("big-send-fail", false)]);
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
            // Every shared command case through the real approval and storage path.
            if case == "auto-read" && durable {
                for (index, (text, _)) in summary_cases().iter().enumerate() {
                    let request = json!({"type":"control_request","request_id":format!("case_{index}"),"request":{
                        "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":format!("tool_case_{index}"),"input":{"command":text,"description":"synthetic case"}}});
                    script.push_str(&format!("printf '%s\\n' {}\nIFS= read -r reply || exit 3\ncase \"$reply\" in *'\"behavior\":\"allow\"'*) ;; *) exit 4 ;; esac\n", quoted(&request)));
                }
            }
        }
        if case.starts_with("big-") {
            let content = "A".repeat(300_000);
            let expected = json!({"type":"control_response","response":{"subtype":"success","request_id":"write_big",
                "response":{"behavior":"allow","updatedInput":{"content":content,"file_path":format!("{cwd}/big.txt")}}}}).to_string();
            let expected_sha = format!("{:x}", sha2::Sha256::digest(expected.as_bytes()));
            script.push_str(&format!(
                "printf '{{\"type\":\"control_request\",\"request_id\":\"write_big\",\"request\":{{\"subtype\":\"can_use_tool\",\"tool_name\":\"Write\",\"tool_use_id\":\"tool_big\",\"input\":{{\"file_path\":\"{cwd}/big.txt\",\"content\":\"'; head -c 300000 /dev/zero | tr '\\0' A; printf '\"}}}}}}\\n'\n"));
            match case {
                "big-allow" => script.push_str(&format!("IFS= read -r reply || exit 3\n[ \"$(printf '%s' \"$reply\" | /usr/bin/shasum -a 256 | /usr/bin/cut -d' ' -f1)\" = '{expected_sha}' ] || exit 4\n")),
                "big-decline" => script.push_str("IFS= read -r reply || exit 3\ncase \"$reply\" in *'\"behavior\":\"deny\"'*) ;; *) exit 4 ;; esac\n"),
                // The provider stops reading its input: the allow reply cannot be written.
                "big-send-fail" => script.push_str("exec 0<&-\n/bin/sleep 2\n"),
                _ => {}
            }
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
            if current["state"] == "running" && matches!(case, "big-allow" | "big-decline" | "big-send-fail") && !approved {
                let binding = json!({"chatId":identity.chat_id,"taskId":identity.task_id,"projectId":identity.project_id,
                    "worktreeId":identity.worktree_id,"workspaceIdentity":identity.workspace_identity,"runId":identity.run_id});
                let list = runtime.approval_request(&mut f.storage, &f.projects, &json!({"action":"list","identity":binding})).unwrap();
                if let Some(first) = list["pending"].as_array().unwrap().first() {
                    let id = first["approvalId"].clone();
                    assert!(elsecrew_core::provider::vendor_observation::claude_approval::large_tool_inputs_held() >= 300_000, "the waiting input is counted");
                    if case == "big-decline" {
                        runtime.approval_request(&mut f.storage, &f.projects, &json!({"action":"decline","identity":binding,"approvalId":id})).unwrap();
                    } else {
                        let first_page = runtime.approval_request(&mut f.storage, &f.projects,
                            &json!({"action":"inspect","identity":binding,"approvalId":id})).unwrap()["inspection"].clone();
                        let pages = first_page["toolInputPages"].as_u64().unwrap();
                        assert!(pages > 1);
                        // Out of order, or allowing before the last page, refuses.
                        assert!(runtime.approval_request(&mut f.storage, &f.projects,
                            &json!({"action":"inspect","identity":binding,"approvalId":id,"page":2})).is_err());
                        assert!(runtime.approval_request(&mut f.storage, &f.projects,
                            &json!({"action":"acceptOnce","identity":binding,"approvalId":id})).is_err());
                        let mut joined = first_page["toolInput"].as_str().unwrap().to_string();
                        for page in 1..pages {
                            assert!(runtime.approval_request(&mut f.storage, &f.projects,
                                &json!({"action":"acceptOnce","identity":binding,"approvalId":id})).is_err(), "page {page} not read yet");
                            let view = runtime.approval_request(&mut f.storage, &f.projects,
                                &json!({"action":"inspect","identity":binding,"approvalId":id,"page":page})).unwrap()["inspection"].clone();
                            assert_eq!(view["toolInputSha256"], first_page["toolInputSha256"]);
                            joined.push_str(view["toolInput"].as_str().unwrap());
                        }
                        // What the person read is exactly what is allowed: same size and SHA-256.
                        assert_eq!(json!(joined.len()), first_page["toolInputBytes"]);
                        assert_eq!(json!(format!("{:x}", sha2::Sha256::digest(joined.as_bytes()))), first_page["toolInputSha256"]);
                        let input: Value = serde_json::from_str(&joined).unwrap();
                        assert_eq!(input["content"].as_str().map(str::len), Some(300_000));
                        runtime.approval_request(&mut f.storage, &f.projects,
                            &json!({"action":"acceptOnce","identity":binding,"approvalId":id})).unwrap();
                    }
                    approved = true;
                }
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
                "bad-init" | "unknown-queue" | "unexplained-denial" | "auto-outside-foreign" | "auto-read-failure" | "big-send-fail" => "failed",
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
        assert_eq!(approved, matches!(case, "allow" | "big-allow" | "big-decline" | "big-send-fail"));
        if case.starts_with("big-") {
            // Written, declined, unanswered at the end, or a failed write: all returned.
            assert_eq!(elsecrew_core::provider::vendor_observation::claude_approval::large_tool_inputs_held(), 0, "{case}: budget returned");
        }
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
            if matches!(case, "success" | "long" | "many-processes" | "organization-guard" | "privacy" | "privacy-with-guard" | "default-effort" | "allow" | "auto" | "auto-outside" | "auto-missing" | "auto-missing-hook" | "auto-read" | "auto-read-mutate" | "auto-read-index-lock" | "big-allow" | "big-decline" | "big-end") {
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
            // The line starts with an assignment: only that a command ran
            // (review PR #11 B2, round 3), with the model's own summary.
            let command = json!({"action":"command","note":"check login with token=••••","id":"t1"});
            let mut ended = command.clone();
            ended["phase"] = json!("done");
            assert_eq!(history[..2], [command, ended], "{case}");
            let cases = if case == "auto-read" { summary_cases() } else { Vec::new() };
            // Each shared case stored as its summary only, in order, with the app's ids.
            for (index, (text, expected)) in cases.iter().enumerate() {
                let mut expected = expected.clone();
                expected["note"] = json!("synthetic case");
                expected["id"] = json!(format!("t{}", index + 2));
                assert_eq!(history[2 + index], expected, "{case}: {text:?}");
            }
            assert_eq!(history[2 + cases.len()..], [json!({"action":"read","target":"index.html","id":format!("t{}", cases.len() + 2)})], "{case}");
            // Read back from the store's files: no secret, no output, no line text.
            let mut needles: Vec<String> = ["FAKEexample0FAKEexample0", "FAKEexampleNoteSecret", "FAKE-OUTPUT-MARKER", "FAKEexampleInlineHeader", "FAKE example pass"].map(String::from).to_vec();
            for (text, _) in &cases {
                needles.extend(text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
                    .filter(|word| word.len() >= 5 && word.to_lowercase().contains("fake")).map(String::from));
            }
            needles.sort();
            needles.dedup();
            assert!(cases.is_empty() || needles.len() >= 50, "{} needles", needles.len());
            super::assert_store_lacks(f._root.path(), &needles.iter().map(String::as_str).collect::<Vec<_>>());
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
