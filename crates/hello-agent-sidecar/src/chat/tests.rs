use super::*;
use crate::agent_runtime::{
    AgentRuntime, FakeScenario, RecoveryResolutionFailurePoint, RunIdentity,
};
use hello_agent_core::agent_adapter::MAX_AGENT_EVENT_BYTES;
use hello_agent_protocol::agent_adapter_v1::AgentRunStartRequestV1;
use hello_agent_protocol::project_v1::sha256_file_version_v1;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

struct Fixture {
    _root: TempDir,
    source: std::path::PathBuf,
    storage: Storage,
    projects: ProjectManager,
    chats: G5Manager,
    project_id: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let data = root.path().join("data");
        fs::create_dir_all(source.join("src")).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::write(source.join("src/value.txt"), b"base").unwrap();
        git(&source, &["init", "-q"]);
        git(&source, &["add", "."]);
        git(
            &source,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "-m",
                "fixture",
            ],
        );
        let database = data.join("hello-agent.sqlite3");
        let mut storage = Storage::open(&database).unwrap();
        let mut projects = ProjectManager::new(&database).unwrap();
        let opened = projects
            .open_project(&mut storage, source.to_str().unwrap(), "req_open")
            .unwrap();
        let project_id = opened.result["projectId"].as_str().unwrap().to_string();
        Self {
            _root: root,
            source,
            storage,
            projects,
            chats: G5Manager::default(),
            project_id,
        }
    }

    fn snapshot(&mut self, request: &str) -> Value {
        let prepared = self
            .chats
            .prepare_chat_workspace(
                &mut self.storage,
                &mut self.projects,
                &self.project_id,
                request,
            )
            .unwrap()
            .result;
        let mut snapshot = prepared["manifest"].clone();
        snapshot["preparationId"] = prepared["preparationId"].clone();
        snapshot
    }

    fn create_chat(&mut self, snapshot: &Value, goal: &str, request: &str) -> Value {
        self.chats
            .default_role_settings(&mut self.storage, &self.project_id, request)
            .unwrap();
        self.chats
            .create_chat(
                &mut self.storage,
                &self.projects,
                snapshot["preparationId"].as_str().unwrap(),
                &self.project_id,
                snapshot["worktreeId"].as_str().unwrap(),
                snapshot["workspaceIdentity"].as_str().unwrap(),
                goal,
                1,
                request,
            )
            .unwrap()
            .result
    }

    fn create_exact_claude_chat(&mut self, snapshot: &Value, goal: &str, request: &str) -> Value {
        self.chats
            .default_role_settings(&mut self.storage, &self.project_id, request)
            .unwrap();
        self.chats
            .update_role_settings(
                &mut self.storage,
                &self.project_id,
                1,
                json!([
                    {"roleId":"planning","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"},
                    {"roleId":"coding","runtime":"claude-code","modelSelector":"claude-opus-5","reasoningEffort":null,"permission":"workspaceWrite"},
                    {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"}
                ]),
                &format!("{request}_exact_roles"),
            )
            .unwrap();
        self.chats
            .create_chat(
                &mut self.storage,
                &self.projects,
                snapshot["preparationId"].as_str().unwrap(),
                &self.project_id,
                snapshot["worktreeId"].as_str().unwrap(),
                snapshot["workspaceIdentity"].as_str().unwrap(),
                goal,
                2,
                request,
            )
            .unwrap()
            .result
    }
}

#[test]
fn reserved_run_identity_is_persisted_before_gate_release_and_storage_failure_blocks_payload() {
    use crate::{
        agent_runtime::preparation::RunPreparation, provider_login_process::LoginProcess,
        provider_profile::CodexProfileBinding,
    };
    use std::ffi::OsStr;
    for failure in ["none", "identity", "release-record", "tree-record"] {
        let fail_store = failure == "identity";
        let fail_release_record = failure == "release-record";
        let fail_tree_record = failure == "tree-record";
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_gate_snapshot");
        let chat = fixture.create_exact_claude_chat(&snapshot, "goal", "req_gate_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
        let value = runtime.get_capabilities("claude-code", true).unwrap();
        let request = AgentRunStartRequestV1 {
            chat_id: chat_id.into(),
            role_id: "coding".into(),
            input_message_ids: vec![initial_message_id(&fixture, chat_id)],
            expected_capability_fingerprint: value["fingerprint"].as_str().unwrap().into(),
        };
        let reservation = RunPreparation::resolve(&fixture.storage, &fixture.projects, &request)
            .unwrap()
            .reserve(
                &mut fixture.storage,
                &fixture.projects,
                &serde_json::from_value(value).unwrap(),
                "req_gate_reserve",
            )
            .unwrap();
        let profile = CodexProfileBinding::from_storage(&fixture.storage).unwrap();
        let home = profile.revalidate().unwrap();
        let marker = fixture._root.path().join("payload-ran");
        if fail_store {
            fixture.storage.connection_for_test().execute_batch("CREATE TRIGGER test_reject_identity BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_agent_identity_%' BEGIN SELECT RAISE(ABORT, 'injected identity persistence failure'); END;").unwrap();
        }
        if fail_release_record {
            fixture.storage.connection_for_test().execute_batch("CREATE TRIGGER test_reject_release BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_agent_released_%' BEGIN SELECT RAISE(ABORT, 'injected release persistence failure'); END;").unwrap();
        }
        let mut process = LoginProcess::reserve(&profile).unwrap();
        let mut release_called = false;
        let result = reservation.launch_gated(
            &mut fixture.storage,
            &fixture.projects,
            &mut process,
            || {
                crate::provider_login_gate::command(
                    std::path::Path::new("/bin/sh"),
                    &[
                        OsStr::new("-c"),
                        OsStr::new(
                            "/bin/sleep 30 & printf ran > \"$1\"; printf ready; IFS= read -r hold",
                        ),
                        OsStr::new("toy-vendor"),
                        marker.as_os_str(),
                    ],
                    &home,
                )?
                .spawn()
            },
            |child| {
                assert!(!marker.exists());
                release_called = true;
                crate::provider_login_gate::release(child)
            },
        );
        assert_eq!(result.is_ok(), !fail_store && !fail_release_record);
        assert_eq!(release_called, !fail_store);
        if fail_release_record {
            assert!(process.begin_preflight().is_err());
        }
        if !fail_store {
            let recorded = fixture.storage.projection_records("run").unwrap();
            let root = process.root_for_test();
            assert_eq!(recorded[0]["process_identity_internal"][0]["pid"], root.pid);
            assert_eq!(
                recorded[0]["process_identity_internal"][0]["start_us"],
                root.start_us
            );
            assert_eq!(
                recorded[0]["barrier"]["releaseGranted"],
                !fail_release_record
            );
            assert_eq!(recorded[0]["state"], "preparing");
            // Observe the toy payload without sending any model input.
            use std::sync::atomic::AtomicBool;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !marker.exists() && std::time::Instant::now() < deadline {
                if !fail_release_record {
                    process.poll(&AtomicBool::new(false)).unwrap();
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(marker.exists());
        }
        if !fail_store && !fail_release_record {
            if fail_tree_record {
                fixture.storage.connection_for_test().execute_batch("CREATE TRIGGER test_reject_tree BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_agent_tree_%' BEGIN SELECT RAISE(ABORT, 'injected tree persistence failure'); END;").unwrap();
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                let observed = reservation.observe_process(&mut fixture.storage, &mut process);
                let records = fixture.storage.projection_records("run").unwrap();
                let tracked = records[0]["process_identity_internal"]
                    .as_array()
                    .unwrap()
                    .len();
                if fail_tree_record && observed.is_err() {
                    assert_eq!(tracked, 1);
                    assert!(process.begin_preflight().is_err());
                    break;
                }
                observed.unwrap();
                if !fail_tree_record && tracked > 1 {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "child observation did not reach ledger"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        process.finish().unwrap();
        assert_eq!(marker.exists(), !fail_store);
        if fail_store {
            fixture
                .storage
                .connection_for_test()
                .execute_batch("DROP TRIGGER test_reject_identity;")
                .unwrap();
        }
        if fail_release_record {
            fixture
                .storage
                .connection_for_test()
                .execute_batch("DROP TRIGGER test_reject_release;")
                .unwrap();
        }
        if fail_tree_record {
            fixture
                .storage
                .connection_for_test()
                .execute_batch("DROP TRIGGER test_reject_tree;")
                .unwrap();
        }
        let before = fixture.storage.projection_records("run").unwrap();
        assert_eq!(before[0]["process_identity_internal"].is_null(), fail_store);
        AgentRuntime::blocked()
            .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
            .unwrap();
        let after = fixture.storage.projection_records("run").unwrap();
        assert_eq!(
            after[0]["recovery"]["reason"],
            if fail_store {
                "processIdentityUnavailable"
            } else {
                "uncleanShutdown"
            }
        );
        assert_eq!(
            after[0]["recovery"]["trackedProcesses"]
                .as_array()
                .unwrap()
                .is_empty(),
            fail_store
        );
        assert!(LoginProcess::reserve(&profile).is_ok());
    }
}

#[test]
fn actual_run_preparation_reserves_once_and_is_visible_to_restart_recovery() {
    use crate::agent_runtime::preparation::RunPreparation;
    for role in ["coding", "review"] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_prepared_snapshot");
        let chat = fixture.create_exact_claude_chat(&snapshot, "goal", "req_prepared_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        if role == "review" {
            let mut saved = fixture
                .storage
                .projection_record("chat", chat_id)
                .unwrap()
                .unwrap();
            let review = &mut saved["record"]["roleSnapshots"][2];
            review["runtime"] = json!("claude-code");
            review["modelSelector"] = json!("claude-opus-5");
            review["reasoningEffort"] = Value::Null;
            fixture
                .storage
                .upsert_projection_record("chat", chat_id, "op_test_review_role", None, saved)
                .unwrap();
        }
        let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let request = AgentRunStartRequestV1 {
            chat_id: chat_id.into(),
            role_id: role.into(),
            input_message_ids: vec![initial_message_id(&fixture, chat_id)],
            expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
        };
        let capability = serde_json::from_value(capability).unwrap();
        let first = RunPreparation::resolve(&fixture.storage, &fixture.projects, &request).unwrap();
        let stale = RunPreparation::resolve(&fixture.storage, &fixture.projects, &request).unwrap();
        first
            .reserve(
                &mut fixture.storage,
                &fixture.projects,
                &capability,
                "req_reserve_first",
            )
            .unwrap();
        assert!(stale
            .reserve(
                &mut fixture.storage,
                &fixture.projects,
                &capability,
                "req_reserve_stale"
            )
            .is_err());
        let runs = fixture.storage.projection_records("run").unwrap();
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run["source_kind"], "actualAgent");
        assert_eq!(run["state"], "preparing");
        assert!(run["recovery_id_reservation"].as_str().is_some());
        assert!(run["process_identity_internal"].is_null());
        let lock_id = format!("lock_{}", snapshot["worktreeId"].as_str().unwrap());
        let lock = fixture
            .storage
            .projection_record("lock", &lock_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            lock["writer_run_id"],
            if role == "coding" {
                run["run_id"].clone()
            } else {
                Value::Null
            }
        );
        // No child was spawned. Startup recovery must still discover this reservation.
        AgentRuntime::blocked()
            .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
            .unwrap();
        let recovered = fixture.storage.projection_records("run").unwrap();
        assert_eq!(
            recovered[0]["recovery"]["reason"],
            "processIdentityUnavailable"
        );
        assert_eq!(
            recovered[0]["recovery"]["recoveryId"],
            run["recovery_id_reservation"]
        );
        assert_eq!(recovered[0]["state"], "recoveryRequired");
    }
}

#[test]
fn actual_run_preparation_rejects_foreign_input_and_stale_evidence_without_writes() {
    use crate::agent_runtime::preparation::RunPreparation;
    for case in ["foreign", "fingerprint", "closed"] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_reject_snapshot");
        let other_snapshot = fixture.snapshot("req_foreign_snapshot");
        let other = fixture.create_chat(&other_snapshot, "foreign goal", "req_foreign_chat");
        let chat = fixture.create_exact_claude_chat(&snapshot, "goal", "req_reject_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
        let value = runtime.get_capabilities("claude-code", true).unwrap();
        let request = AgentRunStartRequestV1 {
            chat_id: chat_id.into(),
            role_id: "coding".into(),
            input_message_ids: vec![initial_message_id(
                &fixture,
                if case == "foreign" {
                    other["chatId"].as_str().unwrap()
                } else {
                    chat_id
                },
            )],
            expected_capability_fingerprint: if case == "fingerprint" {
                format!("sha256:{}", "0".repeat(64))
            } else {
                value["fingerprint"].as_str().unwrap().into()
            },
        };
        let before_runs = fixture.storage.projection_records("run").unwrap();
        let prepared = RunPreparation::resolve(&fixture.storage, &fixture.projects, &request);
        if case == "foreign" {
            assert!(prepared.is_err());
        } else {
            let prepared = prepared.unwrap();
            if case == "closed" {
                fixture
                    .projects
                    .workspace_close(&fixture.storage, &address_from_chat(&chat).unwrap())
                    .unwrap();
            }
            let before_locks = fixture.storage.projection_records("lock").unwrap();
            assert!(prepared
                .reserve(
                    &mut fixture.storage,
                    &fixture.projects,
                    &serde_json::from_value(value).unwrap(),
                    "req_reject_reserve"
                )
                .is_err());
            assert_eq!(
                fixture.storage.projection_records("lock").unwrap(),
                before_locks
            );
        }
        assert_eq!(
            fixture.storage.projection_records("run").unwrap(),
            before_runs
        );
    }
}

#[test]
fn text_candidate_public_sink_uses_the_same_snapshot_storage_owner() {
    use crate::provider_login_process::LoginProcess;
    use crate::provider_preflight_session::{run_text_and_close, TextWorkspace};
    use crate::provider_profile::CodexProfileBinding;
    use hello_agent_core::provider::vendor_observation::{
        codex_events::{stream::TurnOutcome, Projection},
        codex_preflight::{exchange::PreflightExchange, ExpectedProfile},
    };
    use std::{process::Stdio, sync::atomic::AtomicBool};
    for failure in [
        "none",
        "sink",
        "observer",
        "idle-success",
        "idle-observer",
        "bound",
        "bound-native",
        "bound-native-gate-cli",
        "bound-native-gate-helper",
        "bound-native-gate-cancel",
        "bound-role",
        "bound-write",
        "bound-input",
        "bound-persist",
        "bound-sink",
        "bound-cleanup",
        "bound-interrupted",
        "bound-multiple",
        "bound-terminal",
        "bound-cancel",
    ] {
        let fail_sink = matches!(failure, "sink" | "bound-sink");
        let success = matches!(
            failure,
            "none"
                | "idle-success"
                | "bound"
                | "bound-native"
                | "bound-interrupted"
                | "bound-multiple"
        );
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_sink_snapshot");
        let chat = fixture.create_chat(&snapshot, "goal", "req_sink_chat");
        let address = address_from_chat(&chat).unwrap();
        let reservation = if failure.starts_with("bound") {
            // Synthetic admitted evidence only, never a real Codex discovery result.
            let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
            let mut capability: hello_agent_protocol::agent_adapter_v1::AgentCapabilitySnapshotV1 =
                serde_json::from_value(runtime.get_capabilities("claude-code", true).unwrap())
                    .unwrap();
            capability.runtime = "codex".into();
            capability.models[0].model_selector = "test-model".into();
            capability.models[0].reasoning_efforts = vec!["medium".into()];
            capability.fingerprint =
                hello_agent_protocol::agent_adapter_v1::compute_agent_capability_fingerprint_v1(
                    &capability,
                )
                .unwrap();
            let mut saved = fixture
                .storage
                .projection_record("chat", &address.chat_id)
                .unwrap()
                .unwrap();
            let role = &mut saved["record"]["roleSnapshots"][2];
            role["runtime"] = json!("codex");
            role["modelSelector"] = json!("test-model");
            role["reasoningEffort"] = json!("medium");
            if failure == "bound-write" {
                role["permission"] = json!("workspaceWrite");
            }
            fixture
                .storage
                .upsert_projection_record("chat", &address.chat_id, "op_bound_role", None, saved)
                .unwrap();
            let request = AgentRunStartRequestV1 {
                chat_id: address.chat_id.clone(),
                role_id: "review".into(),
                input_message_ids: vec![initial_message_id(&fixture, &address.chat_id)],
                expected_capability_fingerprint: capability.fingerprint.clone(),
            };
            Some(
                crate::agent_runtime::preparation::RunPreparation::resolve(
                    &fixture.storage,
                    &fixture.projects,
                    &request,
                )
                .unwrap()
                .reserve(
                    &mut fixture.storage,
                    &fixture.projects,
                    &capability,
                    "req_bound_reserve",
                )
                .unwrap(),
            )
        } else {
            None
        };
        let profile = CodexProfileBinding::from_storage(&fixture.storage).unwrap();
        let home = profile.revalidate().unwrap();
        let settings = json!({"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only","features":{"hooks":false}});
        let messages = [
            json!({"id":1,"result":{"userAgent":"test","codexHome":home,"platformFamily":"unix","platformOs":"macos"}}),
            json!({"id":2,"result":{"config":settings,"layers":[
                {"name":{"type":"sessionFlags"},"config":settings},
                {"name":{"type":"user","file":home.join("config.toml")},"config":{"cli_auth_credentials_store":"file"}},
                {"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}
            ]}}),
            json!({"id":3,"result":{"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}}),
            json!({"id":4,"result":{"data":[{"id":"test-model","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}],"nextCursor":null}}),
            json!({"id":5,"result":{"thread":{"id":"thread-test"},"model":"test-model","modelProvider":"openai","cwd":snapshot["worktreePath"],"instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"}}),
            json!({"id":6,"result":{"turn":{"id":"t1","status":"inProgress","items":[],"error":null}}}),
        ];
        let mut replies: Vec<String> = messages.iter().map(Value::to_string).collect();
        replies[4].push_str(
            "\n{\"method\":\"thread/started\",\"params\":{\"thread\":{\"id\":\"thread-test\"}}}",
        );
        for event in [
            json!({"method":"turn/started","params":{"threadId":"thread-test","turn":{"id":"t1","status":"inProgress","items":[],"error":null}}}),
            json!({"method":"item/started","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"agentMessage","id":"a1","text":""}}}),
            json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-test","turnId":"t1","itemId":"a1","delta":"hello"}}),
            json!({"method":"item/completed","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"agentMessage","id":"a1","text":"hello"}}}),
            json!({"method":"turn/completed","params":{"threadId":"thread-test","turn":{"id":"t1","status":if failure == "bound-interrupted" {"interrupted"} else {"completed"},"items":[],"error":null}}}),
        ] {
            replies[5].push('\n');
            replies[5].push_str(&event.to_string());
        }
        if failure == "bound-multiple" {
            let (preceding, finished) = replies[5].rsplit_once('\n').unwrap();
            let mut expanded = preceding.to_owned();
            for event in [
                json!({"method":"item/started","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"agentMessage","id":"a2","text":""}}}),
                json!({"method":"item/completed","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"agentMessage","id":"a2","text":"world"}}}),
                json!({"method":"item/started","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"plan","id":"p1","text":""}}}),
                json!({"method":"item/completed","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"plan","id":"p1","text":"plan"}}}),
            ] {
                expanded.push('\n');
                expanded.push_str(&event.to_string());
            }
            expanded.push('\n');
            expanded.push_str(finished);
            replies[5] = expanded;
        }
        let flow = PreflightExchange::new(
            &ExpectedProfile {
                model: "test-model",
                reasoning_effort: "medium",
            },
            home.to_str().unwrap(),
        )
        .unwrap();
        let mut process = LoginProcess::reserve(&profile).unwrap();
        let received = fixture._root.path().join("received-protocol");
        let responded = fixture._root.path().join("sent-first-response");
        let captured = fixture._root.path().join("captured-requests");
        let spawn = || {
            Command::new("/bin/sh")
            .args(["-c", "first=yes; for reply in \"$@\"; do IFS= read -r line || exit 2; printf '%s\\n' \"$line\" >> \"$CAPTURED\"; printf received > \"$RECEIVED\"; if test \"$first\" = yes; then if test \"$DELAY\" = yes; then /bin/sleep 0.6; fi; printf sent > \"$RESPONDED\"; fi; printf '%s\\n' \"$reply\"; if test \"$first\" = yes; then IFS= read -r initialized || exit 3; first=no; fi; done; IFS= read -r hold", "snapshot-peer"])
            .args(&replies).env_clear().env("CAPTURED", &captured).env("RECEIVED", &received).env("RESPONDED", &responded).env("DELAY", if failure.starts_with("idle-") {"yes"} else {"no"}).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        };
        if let Some(reservation) = &reservation {
            if failure.starts_with("bound-native") {
                use crate::provider_executable::PinnedExecutable;
                use sha2::{Digest, Sha256};
                use std::os::unix::fs::PermissionsExt;
                let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
                let script = format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nfirst=yes\nfor reply in {}; do\nIFS= read -r line || exit 2\nprintf '%s\\n' \"$line\" >> {}\nprintf received > {}\nprintf '%s\\n' \"$reply\"\nif test \"$first\" = yes; then IFS= read -r initialized || exit 3; first=no; fi\ndone\nIFS= read -r hold\n",
                    quote(fixture._root.path().join("native-argv").to_str().unwrap()),
                    replies.iter().map(|value| quote(value)).collect::<Vec<_>>().join(" "),
                    quote(captured.to_str().unwrap()), quote(received.to_str().unwrap()),
                );
                let native_root = fixture._root.path().canonicalize().unwrap();
                let cli_path = native_root.join("toy-cli");
                let helper_path = native_root.join("toy-helper");
                std::fs::write(&cli_path, &script).unwrap();
                std::fs::write(&helper_path, b"#!/bin/sh\nexit 0\n").unwrap();
                for path in [&cli_path, &helper_path] {
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
                }
                let cli =
                    PinnedExecutable::open(&cli_path, Sha256::digest(script.as_bytes()).into())
                        .unwrap();
                let helper = PinnedExecutable::open(
                    &helper_path,
                    Sha256::digest(b"#!/bin/sh\nexit 0\n").into(),
                )
                .unwrap();
                // Pre-launch rejects leave the same fresh owner/run untouched.
                assert!(reservation
                    .launch_codex_candidate(
                        &mut fixture.storage,
                        &fixture.projects,
                        &mut process,
                        &cli,
                        &helper,
                        &AtomicBool::new(true),
                    )
                    .is_err());
                for (path, original) in [
                    (&cli_path, script.as_bytes()),
                    (&helper_path, b"#!/bin/sh\nexit 0\n".as_slice()),
                ] {
                    std::fs::write(path, b"changed").unwrap();
                    assert!(reservation
                        .launch_codex_candidate(
                            &mut fixture.storage,
                            &fixture.projects,
                            &mut process,
                            &cli,
                            &helper,
                            &AtomicBool::new(false),
                        )
                        .is_err());
                    assert!(fixture.storage.projection_records("run").unwrap()[0]
                        ["process_identity_internal"]
                        .is_null());
                    std::fs::write(path, original).unwrap();
                }
                let launch_cancel = std::sync::Arc::new(AtomicBool::new(false));
                if failure != "bound-native" {
                    let cli_path = cli_path.clone();
                    let helper_path = helper_path.clone();
                    let canceled = launch_cancel.clone();
                    crate::provider_preflight_session::BEFORE_RELEASE.with(|hook| {
                        *hook.borrow_mut() = Some(Box::new(move || match failure {
                            "bound-native-gate-cli" => {
                                std::fs::write(cli_path, b"changed").unwrap()
                            }
                            "bound-native-gate-helper" => {
                                std::fs::write(helper_path, b"changed").unwrap()
                            }
                            _ => canceled.store(true, std::sync::atomic::Ordering::Release),
                        }));
                    });
                }
                let launched = reservation.launch_codex_candidate(
                    &mut fixture.storage,
                    &fixture.projects,
                    &mut process,
                    &cli,
                    &helper,
                    &launch_cancel,
                );
                if failure != "bound-native" {
                    assert!(launched.is_err());
                    crate::provider_preflight_session::BEFORE_RELEASE
                        .with(|hook| assert!(hook.borrow().is_none()));
                    assert!(!fixture._root.path().join("native-argv").exists());
                    let run = &fixture.storage.projection_records("run").unwrap()[0];
                    assert_eq!(run["barrier"]["releaseGranted"], false);
                    assert!(!run["process_identity_internal"]
                        .as_array()
                        .unwrap()
                        .is_empty());
                    assert!(LoginProcess::reserve(&profile).is_err());
                    process.finish().unwrap();
                    assert!(!received.exists());
                    assert!(!fixture._root.path().join("native-argv").exists());
                    AgentRuntime::blocked()
                        .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
                        .unwrap();
                    assert_eq!(
                        fixture.storage.projection_records("run").unwrap()[0]["state"],
                        "recoveryRequired"
                    );
                    continue;
                }
                launched.unwrap();
            } else {
                // Toy peer cannot act until its first request; release has no payload.
                reservation
                    .launch_gated(
                        &mut fixture.storage,
                        &fixture.projects,
                        &mut process,
                        spawn,
                        |_| Ok(()),
                    )
                    .unwrap();
            }
            if matches!(failure, "bound-role" | "bound-input") {
                let mut saved = fixture
                    .storage
                    .projection_record("chat", &address.chat_id)
                    .unwrap()
                    .unwrap();
                if failure == "bound-input" {
                    saved["last_message"]["text"] = json!("different goal");
                } else {
                    saved["record"]["roleSnapshots"][2]["modelSelector"] = json!("changed-model");
                }
                fixture
                    .storage
                    .upsert_projection_record(
                        "chat",
                        &address.chat_id,
                        "op_changed_role",
                        None,
                        saved,
                    )
                    .unwrap();
            }
        } else {
            process.launch(spawn).unwrap();
        }
        if failure == "bound-persist" {
            fixture.storage.connection_for_test().execute_batch("CREATE TRIGGER test_reject_public_chat BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_agent_public_chat_%' BEGIN SELECT RAISE(ABORT, 'injected public chat persistence failure'); END;").unwrap();
        }
        if failure == "bound-cleanup" {
            process.inject_uncertain_cleanup_once();
        }
        if failure == "bound-terminal" {
            fixture.storage.connection_for_test().execute_batch("CREATE TRIGGER test_reject_terminal_chat BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_agent_native_chat_terminal_%' BEGIN SELECT RAISE(ABORT, 'injected terminal persistence failure'); END;").unwrap();
        }
        let mut writes = 0;
        let observations = std::cell::Cell::new(0);
        let observed_idle_child = std::cell::Cell::new(false);
        let mut sink = |storage: &mut Storage, event| {
            assert!(reservation.is_some() || observations.get() > 0);
            if fail_sink {
                return Err(SidecarError::new(
                    "testSinkFailed",
                    "injected storage sink failure",
                ));
            }
            let text = match event {
                Projection::Delta { text, .. } | Projection::Completed { text, .. } => text,
                _ => panic!("unexpected projection"),
            };
            writes += 1;
            // Synthetic sink verifies ownership/writes, not the actual-agent event contract.
            let mut record = storage
                .projection_record("chat", &address.chat_id)?
                .unwrap();
            record.insert("candidate_test_output".into(), json!(text));
            storage.upsert_projection_record(
                "chat",
                &address.chat_id,
                &format!("op_sink_{writes}"),
                None,
                record,
            )?;
            Ok(())
        };
        let mut observer = |storage: &mut Storage, process: &mut LoginProcess<'_>| {
            observations.set(observations.get() + 1);
            let (identities, observed) = process.observe_for_run();
            observed?;
            assert!(!identities.is_empty());
            if received.exists() && !responded.exists() && identities.len() > 1 {
                observed_idle_child.set(true);
            }
            if failure == "observer" || (failure == "idle-observer" && observed_idle_child.get()) {
                return Err(SidecarError::new(
                    "testObserverFailed",
                    "injected run observation sink failure",
                ));
            }
            let mut saved = storage
                .projection_record("chat", &address.chat_id)?
                .unwrap();
            saved.insert(
                "candidate_test_observations".into(),
                json!(observations.get()),
            );
            storage.upsert_projection_record(
                "chat",
                &address.chat_id,
                &format!("op_observe_{}", observations.get()),
                None,
                saved,
            )?;
            Ok(())
        };
        let result = if let Some(reservation) = &reservation {
            reservation.run_read_only_text(
                &mut fixture.storage,
                &mut fixture.projects,
                &mut process,
                &AtomicBool::new(failure == "bound-cancel"),
                &mut sink,
            )
        } else {
            let mut workspace =
                TextWorkspace::new(&fixture.projects, &mut fixture.storage, &address).unwrap();
            run_text_and_close(
                &mut process,
                flow,
                &AtomicBool::new(false),
                &mut workspace,
                "goal",
                &mut sink,
                &mut observer,
            )
        };
        if !success {
            assert!(result.is_err());
            assert_eq!(
                writes,
                if matches!(
                    failure,
                    "bound-persist" | "bound-cleanup" | "bound-terminal"
                ) {
                    2
                } else {
                    0
                }
            );
        } else {
            assert_eq!(
                result.unwrap(),
                if failure == "bound-interrupted" {
                    TurnOutcome::Interrupted
                } else {
                    TurnOutcome::Completed
                }
            );
            assert_eq!(writes, if failure == "bound-multiple" { 4 } else { 2 });
        }
        let stored = fixture
            .storage
            .projection_record("chat", &address.chat_id)
            .unwrap()
            .unwrap();
        let last_message = fixture
            .storage
            .aggregate_event_payloads("chat", &address.chat_id)
            .unwrap()
            .into_iter()
            .rev()
            .find_map(|value| {
                value
                    .get("last_message")
                    .filter(|message| !message.is_null())
                    .cloned()
            })
            .unwrap();
        assert_eq!(
            stored.get("candidate_test_output"),
            (success
                || matches!(
                    failure,
                    "bound-persist" | "bound-cleanup" | "bound-terminal"
                ))
            .then_some(&json!(if failure == "bound-multiple" {
                "plan"
            } else {
                "hello"
            }))
        );
        assert_eq!(
            received.exists(),
            !matches!(
                failure,
                "observer" | "bound-role" | "bound-write" | "bound-input" | "bound-cancel"
            )
        );
        if failure.starts_with("idle-") {
            assert!(observed_idle_child.get());
        }
        if failure == "idle-observer" {
            assert!(!responded.exists());
        }
        if success && reservation.is_none() {
            assert!(observations.get() > 10);
        }
        if matches!(failure, "bound" | "bound-native") {
            let requests: Vec<Value> = fs::read_to_string(&captured)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let turn = requests
                .iter()
                .find(|value| value["method"] == "turn/start")
                .unwrap();
            assert_eq!(turn["params"]["input"][0]["text"], "goal\n");
            let thread = requests
                .iter()
                .find(|value| value["method"] == "thread/start")
                .unwrap();
            assert_eq!(thread["params"]["model"], "test-model");
            let run = &fixture.storage.projection_records("run").unwrap()[0];
            assert_eq!(run["barrier"]["releaseGranted"], true);
            assert!(!run["process_identity_internal"]
                .as_array()
                .unwrap()
                .is_empty());
            assert_eq!(run["public_events"].as_array().unwrap().len(), 1);
            assert_eq!(run["public_events"][0]["kind"], "final");
            assert_eq!(run["public_events"][0]["text"], "hello");
            assert_eq!(last_message["kind"], "role");
            assert_eq!(last_message["text"], "hello");
            assert_eq!(last_message["roleId"], "review");
            if failure == "bound-native" {
                let args = fs::read_to_string(fixture._root.path().join("native-argv")).unwrap();
                assert_eq!(
                    args.lines().collect::<Vec<_>>(),
                    vec![
                        "app-server",
                        "--listen",
                        "stdio://",
                        "-c",
                        "model=\"test-model\"",
                        "-c",
                        "model_reasoning_effort=\"medium\"",
                        "-c",
                        "model_provider=\"openai\"",
                        "-c",
                        "approval_policy=\"on-request\"",
                        "-c",
                        "sandbox_mode=\"read-only\"",
                        "-c",
                        "features.hooks=false",
                    ]
                );
            }
        }
        if matches!(failure, "bound-persist" | "bound-sink" | "bound-cleanup") {
            let run = &fixture.storage.projection_records("run").unwrap()[0];
            assert!(run["public_events"].as_array().unwrap().is_empty());
            assert_eq!(last_message["text"], "goal");
        }
        if failure == "bound-interrupted" {
            let run = &fixture.storage.projection_records("run").unwrap()[0];
            assert_eq!(run["public_events"].as_array().unwrap().len(), 1);
            assert_eq!(run["public_events"][0]["kind"], "status");
            assert_eq!(last_message["kind"], "status");
        }
        if failure == "bound-multiple" {
            let run = &fixture.storage.projection_records("run").unwrap()[0];
            let events = run["public_events"].as_array().unwrap();
            assert_eq!(
                events
                    .iter()
                    .map(|event| (
                        event["kind"].as_str().unwrap(),
                        event["text"].as_str().unwrap()
                    ))
                    .collect::<Vec<_>>(),
                vec![("status", "hello"), ("status", "plan"), ("final", "world")]
            );
            assert_eq!(last_message["text"], "world");
        }
        if matches!(
            failure,
            "bound"
                | "bound-native"
                | "bound-multiple"
                | "bound-persist"
                | "bound-sink"
                | "bound-interrupted"
                | "bound-cleanup"
                | "bound-cancel"
        ) {
            let runs = fixture.storage.projection_records("run").unwrap();
            let run = &runs[0];
            assert_eq!(
                run["state"],
                match failure {
                    "bound" | "bound-native" | "bound-multiple" => "succeeded",
                    "bound-interrupted" | "bound-cancel" => "canceled",
                    "bound-cleanup" => "recoveryRequired",
                    _ => "failed",
                }
            );
            assert_eq!(
                run["termination"]["processesProvenGone"],
                failure != "bound-cleanup"
            );
            if failure == "bound-cleanup" {
                assert_eq!(
                    run["recovery"]["recoveryId"],
                    run["recovery_id_reservation"]
                );
                assert!(fixture
                    .projects
                    .verify_workspace(&fixture.storage, &address)
                    .is_err());
            } else {
                assert!(run["recovery"].is_null());
                assert!(run["termination"]["trackedProcesses"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|item| item["observedGone"] == true));
            }
            AgentRuntime::blocked()
                .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
                .unwrap();
            assert_eq!(fixture.storage.projection_records("run").unwrap(), runs);
        }
        // Rejection before claiming the protocol leaves cleanup with the caller.
        if matches!(failure, "bound-role" | "bound-write" | "bound-input") {
            process.finish().unwrap();
        }
        if matches!(failure, "bound-cleanup" | "bound-terminal") {
            assert!(LoginProcess::reserve(&profile).is_err());
            assert!(process.finish().is_err());
        } else {
            assert!(LoginProcess::reserve(&profile).is_ok());
        }
        if matches!(
            failure,
            "bound" | "bound-native" | "bound-terminal" | "bound-cleanup"
        ) {
            let before = fixture.storage.projection_records("run").unwrap();
            if failure == "bound-terminal" {
                assert_eq!(before[0]["state"], "preparing");
                assert!(before[0]["termination"].is_null());
                assert_ne!(stored["state"], "reviewReady");
                fixture
                    .storage
                    .connection_for_test()
                    .execute_batch("DROP TRIGGER test_reject_terminal_chat;")
                    .unwrap();
            }
            let database = fixture._root.path().join("data/hello-agent.sqlite3");
            drop(fixture.storage);
            fixture.storage = Storage::open(&database).unwrap();
            fixture.projects = ProjectManager::new(&database).unwrap();
            let runtime = AgentRuntime::blocked();
            runtime
                .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
                .unwrap();
            runtime.validate_persisted_runs(&fixture.storage).unwrap();
            let after = fixture.storage.projection_records("run").unwrap();
            if failure == "bound-terminal" {
                assert_eq!(after[0]["state"], "recoveryRequired");
                assert_eq!(
                    after[0]["recovery"]["recoveryId"],
                    before[0]["recovery_id_reservation"]
                );
                assert_eq!(after[0]["public_events"], before[0]["public_events"]);
                // Reopening the ledger never silently removes a retained profile fence.
                assert!(LoginProcess::reserve(&profile).is_err());
            } else {
                assert_eq!(after, before);
            }
        }
        assert_eq!(
            fs::read(fixture.source.join("src/value.txt")).unwrap(),
            b"base"
        );
    }
}

#[test]
fn text_candidate_requires_durable_chat_snapshot_and_rechecks_marker() {
    use crate::provider_preflight_session::TextWorkspace;
    let mut fixture = Fixture::new();
    let first = fixture.snapshot("req_text_first");
    let second = fixture.snapshot("req_text_second");
    let chat = fixture.create_chat(&first, "goal", "req_text_chat");
    let address = address_from_chat(&chat).unwrap();
    let binding = TextWorkspace::new(&fixture.projects, &mut fixture.storage, &address).unwrap();
    binding.revalidate().unwrap();
    drop(binding);
    let mut other = address.clone();
    other.worktree_id = second["worktreeId"].as_str().unwrap().into();
    other.workspace_identity = second["workspaceIdentity"].as_str().unwrap().into();
    assert!(TextWorkspace::new(&fixture.projects, &mut fixture.storage, &other).is_err());
    let mut missing = address.clone();
    missing.chat_id = "missing_chat".into();
    assert!(TextWorkspace::new(&fixture.projects, &mut fixture.storage, &missing).is_err());
    let binding = TextWorkspace::new(&fixture.projects, &mut fixture.storage, &address).unwrap();
    let marker = std::path::Path::new(first["worktreePath"].as_str().unwrap())
        .join(".git/hello-agent-snapshot-v1.json");
    let original = fs::read(&marker).unwrap();
    fs::write(&marker, b"{}").unwrap();
    assert!(binding.revalidate().is_err());
    fs::write(&marker, original).unwrap();
    binding.revalidate().unwrap();
    drop(binding);
    fixture
        .projects
        .workspace_close(&fixture.storage, &address)
        .unwrap();
    assert!(TextWorkspace::new(&fixture.projects, &mut fixture.storage, &address).is_err());
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );
}

#[test]
fn production_prepared_workspace_snapshot_persists_more_than_one_hundred_members() {
    let mut fixture = Fixture::new();
    let mut snapshots = Vec::new();
    for index in 0..101 {
        snapshots.push(fixture.snapshot(&format!("req_workspace_snapshot_{index}")));
    }
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    let prepared = project["prepared_workspaces"].as_object().unwrap();
    assert_eq!(prepared.len(), 101);
    for snapshot in &snapshots {
        let preparation_id = snapshot["preparationId"].as_str().unwrap();
        let record = &prepared[preparation_id];
        assert_eq!(record["preparationId"], preparation_id);
        assert_eq!(record["bindingState"], "unbound");
        assert_eq!(record["boundChatId"], Value::Null);
        let mut manifest = snapshot.clone();
        manifest.as_object_mut().unwrap().remove("preparationId");
        assert_eq!(record["manifest"], manifest);
        let workspace = std::path::Path::new(manifest["worktreePath"].as_str().unwrap());
        assert_eq!(fs::read(workspace.join("src/value.txt")).unwrap(), b"base");
    }
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );

    let database = fixture._root.path().join("data/hello-agent.sqlite3");
    fixture.storage.checkpoint().unwrap();
    drop(fixture.chats);
    drop(fixture.projects);
    drop(fixture.storage);
    // Keep the TempDir alive, but release every production handle before a
    // read-only reopen, before the private maintenance path below can mutate
    // derived records. Product startup readiness is a separate gate.
    let before = fs::read(&database).unwrap();
    let connection = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let (stored_json, sequence): (String, i64) = connection
        .query_row(
            "SELECT projection_json,last_event_sequence FROM projects WHERE project_id=?1",
            [&fixture.project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let mut stored: Value = serde_json::from_str(&stored_json).unwrap();
    // Match load_json_projection: the sequence lives in its typed
    // column, not in the stored projection JSON.
    stored["last_event_sequence"] = Value::from(sequence);
    assert!(
        stored == Value::Object(project),
        "persisted project with typed sequence differs from the live projection"
    );
    let (event_count, project_count, last_rowid): (i64, i64, i64) = connection
        .query_row(
            "SELECT count(*),sum(aggregate_type='project'),max(rowid) FROM events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    // One project.open event, then prepare lock/project, pre-activation,
    // active project/lock, and the G5 preparation-map upsert per snapshot.
    assert_eq!(event_count, 1 + 6 * 101);
    assert_eq!(project_count, 1 + 4 * 101);
    assert_eq!(sequence, project_count);
    let mut previous_rowid = 0;
    let mut canonical_bytes = 0_i64;
    let mut latest_source_bytes = 0_i64;
    for (index, snapshot) in snapshots.iter().enumerate() {
        let (rowid, source_sequence, payload, bytes): (i64, i64, String, i64) = connection
            .query_row(
                "SELECT rowid,aggregate_sequence,canonical_json_payload,\
                 length(CAST(canonical_json_payload AS BLOB)) FROM events \
                 WHERE operation_id=?1 AND aggregate_type='project' AND aggregate_id=?2",
                rusqlite::params![
                    format!("op_workspace_prepare_chat_req_workspace_snapshot_{index}"),
                    fixture.project_id,
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert!(rowid > previous_rowid);
        assert_eq!(source_sequence, 1 + 4 * (index as i64 + 1));
        assert_eq!(bytes as usize, payload.len());
        let source: Value = serde_json::from_str(&payload).unwrap();
        let members = source["prepared_workspaces"].as_object().unwrap();
        assert_eq!(members.len(), index + 1);
        assert_eq!(
            members[snapshot["preparationId"].as_str().unwrap()]["manifest"]["workspaceIdentity"],
            snapshot["workspaceIdentity"]
        );
        previous_rowid = rowid;
        canonical_bytes += bytes;
        latest_source_bytes = bytes;
    }
    assert_eq!(previous_rowid, last_rowid);
    drop(connection);
    assert_eq!(fs::read(&database).unwrap(), before);
    let mut connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    let source_digest = |connection: &rusqlite::Connection| {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        let mut statement=connection.prepare("SELECT rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp FROM events ORDER BY rowid").unwrap();
        let columns = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for column in 0..columns {
                use rusqlite::types::ValueRef;
                match row.get_ref(column).unwrap() {
                    ValueRef::Null => digest.update([0]),
                    ValueRef::Integer(value) => {
                        digest.update([1]);
                        digest.update(value.to_be_bytes());
                    }
                    ValueRef::Text(value) => {
                        digest.update([2]);
                        digest.update((value.len() as u64).to_be_bytes());
                        digest.update(value);
                    }
                    other => panic!("unexpected ledger type {other:?}"),
                }
            }
        }
        <[u8; 32]>::from(digest.finalize())
    };
    let source_before = source_digest(&connection);
    use crate::storage::reopen::snapshot::{ReopenMaintenanceSession, ReopenMaintenanceStep};
    let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
    let mut calls = 0;
    loop {
        calls += 1;
        // The preserved exact 607-source fixture completed in 172,685 calls.
        // This is a test hang guard, not a per-call or product startup budget.
        assert!(calls < 400_000);
        let (usage, done) = match session.step(&mut || false).unwrap() {
            ReopenMaintenanceStep::Pending(usage) => (usage, false),
            ReopenMaintenanceStep::ReadyVerified(usage) => (usage, true),
            ReopenMaintenanceStep::Cancelled => panic!("unexpected cancellation"),
        };
        assert!(usage.fragment_bytes <= 1024 * 1024);
        assert!(usage.encoded_bytes() <= 8 * 1024 * 1024);
        assert!(usage.work_units <= 100);
        if done {
            break;
        }
    }
    drop(session);
    assert_eq!(source_digest(&connection), source_before);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM prepared_workspaces", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        101
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT materialization_state FROM storage_schema_meta",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "ready"
    );
    eprintln!("production workspace global/catch-up/ready calls={calls} source_sha256={source_before:02x?}");
    eprintln!("production workspaces=101 project_sources={project_count} total_sources={event_count} last_source_rowid={last_rowid} preparation_source_bytes={canonical_bytes} latest_source_bytes={latest_source_bytes}");
}

#[test]
fn multiple_snapshots_and_chats_remain_explicitly_addressed() {
    let mut fixture = Fixture::new();
    let first = fixture.snapshot("req_snapshot_a");
    let second = fixture.snapshot("req_snapshot_b");
    assert_ne!(first["worktreeId"], second["worktreeId"]);
    assert_ne!(first["workspaceIdentity"], second["workspaceIdentity"]);

    let chat_a = fixture.create_chat(&first, "first goal", "req_chat_a");
    let chat_b = fixture.create_chat(&second, "second goal", "req_chat_b");
    let address_a = address_from_chat(&chat_a).unwrap();
    let address_b = address_from_chat(&chat_b).unwrap();

    let read_a = fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_a, "src/value.txt")
        .unwrap();
    let read_b = fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_b, "src/value.txt")
        .unwrap();
    assert_eq!(read_a.streams[0].chunks, vec!["base"]);
    assert_eq!(read_b.streams[0].chunks, vec!["base"]);

    let aliased = WorkspaceAddress {
        chat_id: address_a.chat_id.clone(),
        project_id: address_b.project_id.clone(),
        worktree_id: address_b.worktree_id.clone(),
        workspace_identity: address_b.workspace_identity.clone(),
    };
    assert_eq!(
        fixture
            .projects
            .workspace_read_file(&fixture.storage, &aliased, "src/value.txt")
            .unwrap_err()
            .kind,
        "manifestMismatch"
    );

    fixture
        .projects
        .workspace_close(&fixture.storage, &address_a)
        .unwrap();
    assert_eq!(
        fixture
            .projects
            .workspace_read_file(&fixture.storage, &address_a, "src/value.txt")
            .unwrap_err()
            .kind,
        "recoveryRequired"
    );
    fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_b, "src/value.txt")
        .unwrap();
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );
}

#[test]
fn followup_append_preserves_save_sessions_and_refuses_recovery_state() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_followup_state_snapshot");
    let chat = fixture.create_chat(&snapshot, "original", "req_followup_state_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let request = followup_request(&chat, 1, "user_0123456789abcdef0123456789abcdef");
    let mut stored = chat_projection_record(&fixture.storage, chat_id).unwrap();
    stored.insert("save_sessions".into(), json!([]));
    fixture
        .storage
        .upsert_projection_record("chat", chat_id, "op_followup_save_sessions", None, stored)
        .unwrap();
    fixture
        .chats
        .append_user_message(&mut fixture.storage, &fixture.projects, &request)
        .unwrap();
    let stored = chat_projection_record(&fixture.storage, chat_id).unwrap();
    assert_eq!(stored["save_sessions"], json!([]));
    let mut blocked = stored;
    blocked.insert("state".into(), json!("recoveryRequired"));
    blocked.get_mut("record").unwrap()["state"] = json!("recoveryRequired");
    fixture
        .storage
        .upsert_projection_record("chat", chat_id, "op_followup_recovery", None, blocked)
        .unwrap();
    let next = followup_request(&chat, 2, "user_abcdef0123456789abcdef0123456789");
    assert!(fixture
        .chats
        .append_user_message(&mut fixture.storage, &fixture.projects, &next)
        .is_err());
    assert!(fixture
        .storage
        .retained_chat_message(next["clientMessageId"].as_str().unwrap())
        .unwrap()
        .is_none());
}

#[test]
fn save_continuations_reject_cross_chat_and_preserve_other_workspace() {
    let mut fixture = Fixture::new();
    let first = fixture.snapshot("req_save_snapshot_a");
    let second = fixture.snapshot("req_save_snapshot_b");
    let chat_a = fixture.create_chat(&first, "save a", "req_save_chat_a");
    let chat_b = fixture.create_chat(&second, "save b", "req_save_chat_b");
    let address_a = address_from_chat(&chat_a).unwrap();
    let address_b = address_from_chat(&chat_b).unwrap();
    let expected = sha256_file_version_v1(b"base");
    let begin = fixture
        .projects
        .workspace_begin_save(
            &mut fixture.storage,
            &address_a,
            "src/value.txt",
            &expected,
            5,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap();
    assert_eq!(
        fixture
            .projects
            .workspace_append_save_chunk(&fixture.storage, &address_b, save_id, 1, "other")
            .unwrap_err()
            .kind,
        "manifestMismatch"
    );
    fixture
        .projects
        .workspace_append_save_chunk(&fixture.storage, &address_a, save_id, 1, "after")
        .unwrap();
    fixture
        .projects
        .workspace_commit_save(
            &mut fixture.storage,
            &address_a,
            save_id,
            &expected,
            1,
            5,
            "req_save_commit",
        )
        .unwrap();
    let b = fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_b, "src/value.txt")
        .unwrap();
    assert_eq!(b.streams[0].chunks, vec!["base"]);
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );
}

#[test]
fn settings_are_snapshotted_and_global_workspace_aliases_are_rejected() {
    let mut fixture = Fixture::new();
    let first = fixture.snapshot("req_settings_snapshot_a");
    let chat = fixture.create_chat(&first, "stable settings", "req_settings_chat_a");
    let roles = json!([
        {"roleId":"planning","runtime":"codex","modelSelector":"different","reasoningEffort":"medium","permission":"readOnly"},
        {"roleId":"coding","runtime":"claude-code","modelSelector":"opus","reasoningEffort":null,"permission":"workspaceWrite"},
        {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"high","permission":"readOnly"}
    ]);
    fixture
        .chats
        .update_role_settings(
            &mut fixture.storage,
            &fixture.project_id,
            1,
            roles,
            "req_settings_update",
        )
        .unwrap();
    let stored = fixture
        .storage
        .projection_record("chat", chat["chatId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(stored["record"]["settingsRevision"], 1);
    assert_eq!(stored["record"]["roleSnapshots"][0]["modelSelector"], "sol");
    let error = fixture
        .chats
        .create_chat(
            &mut fixture.storage,
            &fixture.projects,
            first["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            first["worktreeId"].as_str().unwrap(),
            first["workspaceIdentity"].as_str().unwrap(),
            "alias",
            2,
            "req_alias",
        )
        .unwrap_err();
    assert_eq!(error.kind, "manifestMismatch");
}

#[test]
fn chat_ledger_pages_keep_durable_sequence_separate_from_wire_streams() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_ledger_snapshot");
    let chat = fixture.create_chat(&snapshot, "persist me", "req_ledger_chat");
    let detail = fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
            false,
        )
        .unwrap();
    assert_eq!(detail["messages"][0]["eventSequence"], 1);
    assert_eq!(detail["messages"][0]["text"], "persist me");
    validate_g5_payload_v1("chatDetailPage", &detail).unwrap();
}

fn followup_request(chat: &Value, sequence: u64, id: &str) -> Value {
    json!({"projectId":chat["projectId"],"chatId":chat["chatId"],"taskId":chat["taskId"],
        "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
        "expectedEventSequence":sequence,"clientMessageId":id,"text":"후속 지시"})
}

#[test]
fn followup_retry_survives_actual_source_compaction_and_reopen() {
    use crate::storage_lifetime::compaction_writer::{CompactionStepResult, CompactionTarget};
    use hello_agent_protocol::storage_lifetime_v1::StorageCompactionPhaseV3;
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_followup_compact_snapshot");
    let chat = fixture.create_chat(&snapshot, "original", "req_followup_compact_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let request = followup_request(&chat, 1, "user_0123456789abcdef0123456789abcdef");
    let first = fixture
        .chats
        .append_user_message(&mut fixture.storage, &fixture.projects, &request)
        .unwrap();
    // Keep the two durable messages, but reach the real 1,000-source-event threshold.
    for index in 0..999 {
        persist_chat_state(
            &mut fixture.storage,
            &chat,
            &format!("op_followup_compact_{index}"),
        )
        .unwrap();
    }
    for _ in 0..20_000 {
        if !fixture.storage.materialization_requires_work().unwrap() {
            break;
        }
        fixture
            .storage
            .run_materialization_scheduler_step()
            .unwrap();
    }
    assert!(!fixture.storage.materialization_requires_work().unwrap());
    fixture.storage.checkpoint().unwrap();
    let database = fixture.storage.path().to_path_buf();
    drop(fixture.storage);
    fixture.storage =
        Storage::open_for_admission(&database, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    let target = CompactionTarget {
        aggregate_type: "chat".into(),
        aggregate_id: chat_id.into(),
    };
    let mut finalized = false;
    let mut partial_reopen_checked = false;
    for _ in 0..1000 {
        let step = fixture
            .storage
            .run_compaction_scheduler_step(&target)
            .unwrap();
        if let CompactionStepResult::ArmedWorked {
            phase: StorageCompactionPhaseV3::ReleasePlan,
            after_plan_ordinal,
            ..
        } = &step
        {
            if !partial_reopen_checked && (1..1000).contains(after_plan_ordinal) {
                fixture.storage.checkpoint().unwrap();
                drop(fixture.storage);
                assert_compacted_chat_missing_row_rejected(
                    &database,
                    chat_id,
                    "frozen_operation_sources",
                    *after_plan_ordinal,
                );
                assert_compacted_chat_missing_row_rejected(
                    &database,
                    chat_id,
                    "storage_compaction_plan_rows",
                    *after_plan_ordinal + 1,
                );
                fixture.storage = Storage::open_for_admission(
                    &database,
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
                partial_reopen_checked = true;
            }
        }
        if matches!(step, CompactionStepResult::Finalized { .. }) {
            finalized = true;
            break;
        }
    }
    assert!(finalized, "real compaction did not finalize");
    assert!(
        partial_reopen_checked,
        "partial release boundary was not exercised"
    );
    let database = fixture.storage.path().to_path_buf();
    let before = chat_projection_record(&fixture.storage, chat_id).unwrap();
    fixture.storage.checkpoint().unwrap();
    drop(fixture.storage);
    let inspection = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let remaining: i64 = inspection
        .query_row(
            "SELECT count(*) FROM events WHERE operation_id=?1",
            [format!(
                "op_followup_{}",
                request["clientMessageId"].as_str().unwrap()
            )],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0, "retry must not rely on the fresh source row");
    drop(inspection);
    assert_compacted_chat_missing_row_rejected(
        &database,
        chat_id,
        "frozen_operation_sources",
        1000,
    );
    let mut reopened =
        Storage::open_for_admission(&database, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    let replay = fixture
        .chats
        .append_user_message(&mut reopened, &fixture.projects, &request)
        .unwrap();
    assert_eq!(replay.result, first.result);
    assert!(replay.events.is_empty());
    assert_eq!(chat_projection_record(&reopened, chat_id).unwrap(), before);
}

fn assert_compacted_chat_missing_row_rejected(
    database: &std::path::Path,
    chat_id: &str,
    table: &str,
    sequence: u32,
) {
    // A released plan predecessor must still have a valid frozen source. Preserve
    // the schema while injecting corruption into a separate disposable database.
    assert!(matches!(
        table,
        "frozen_operation_sources" | "storage_compaction_plan_rows"
    ));
    let copy_root = tempfile::tempdir().unwrap();
    let damaged = copy_root.path().join("missing-predecessor.sqlite3");
    fs::copy(database, &damaged).unwrap();
    let connection = rusqlite::Connection::open(&damaged).unwrap();
    let triggers: Vec<(String, String)> = connection
        .prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name=?1")
        .unwrap()
        .query_map([table], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (name, _) in &triggers {
        connection
            .execute_batch(&format!("DROP TRIGGER \"{}\"", name.replace('"', "\"\"")))
            .unwrap();
    }
    assert_eq!(connection.execute(
        &format!("DELETE FROM {table} WHERE aggregate_type='chat' AND aggregate_id=?1 AND aggregate_sequence=?2"),
        rusqlite::params![chat_id, sequence],
    ).unwrap(), 1);
    for (_, sql) in triggers {
        connection.execute_batch(&sql).unwrap();
    }
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(connection);
    let error =
        match Storage::open_for_admission(&damaged, &std::sync::atomic::AtomicBool::new(false)) {
            Ok(_) => panic!("missing frozen predecessor was accepted"),
            Err(error) => error,
        };
    assert_eq!(error.kind(), "storageSchemaMismatch");
}

#[test]
fn followup_append_is_durable_and_retries_without_new_event() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_followup_snapshot");
    let chat = fixture.create_chat(&snapshot, "original", "req_followup_chat");
    let request = followup_request(&chat, 1, "user_0123456789abcdef0123456789abcdef");
    let original =
        chat_projection_record(&fixture.storage, chat["chatId"].as_str().unwrap()).unwrap();
    let first = fixture
        .chats
        .append_user_message(&mut fixture.storage, &fixture.projects, &request)
        .unwrap();
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.result["eventSequence"], 2);
    let retry = fixture
        .chats
        .append_user_message(&mut fixture.storage, &fixture.projects, &request)
        .unwrap();
    assert_eq!(retry.result, first.result);
    assert!(retry.events.is_empty());
    let current =
        chat_projection_record(&fixture.storage, chat["chatId"].as_str().unwrap()).unwrap();
    assert!(!fixture
        .storage
        .append_followup_checked(
            chat["chatId"].as_str().unwrap(),
            "op_interleaved_followup",
            &original,
            current.clone()
        )
        .unwrap());
    assert_eq!(
        chat_projection_record(&fixture.storage, chat["chatId"].as_str().unwrap()).unwrap(),
        current
    );
    for _ in 0..10_000 {
        if !fixture.storage.materialization_requires_work().unwrap() {
            break;
        }
        fixture
            .storage
            .run_materialization_scheduler_step()
            .unwrap();
    }
    assert!(!fixture.storage.materialization_requires_work().unwrap());
    let materialized_retry = fixture
        .chats
        .append_user_message(&mut fixture.storage, &fixture.projects, &request)
        .unwrap();
    assert_eq!(materialized_retry.result, first.result);
    assert!(materialized_retry.events.is_empty());
    let detail = fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
            false,
        )
        .unwrap();
    assert_eq!(detail["messages"].as_array().unwrap().len(), 2);
    assert_eq!(detail["chat"]["state"], "draft");
    assert_eq!(detail["chat"]["roleSnapshots"], chat["roleSnapshots"]);
    let database = fixture._root.path().join("data/hello-agent.sqlite3");
    drop(fixture.storage);
    let mut reopened = Storage::open(&database).unwrap();
    let replay = fixture
        .chats
        .append_user_message(&mut reopened, &fixture.projects, &request)
        .unwrap();
    assert_eq!(replay.result, first.result);
    assert!(replay.events.is_empty());
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );
}

#[test]
fn followup_rejects_id_reuse_identity_drift_and_stale_sequence_without_mutation() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_followup_reject_snapshot");
    let chat = fixture.create_chat(&snapshot, "original", "req_followup_reject_chat");
    let request = followup_request(&chat, 1, "user_0123456789abcdef0123456789abcdef");
    fixture
        .chats
        .append_user_message(&mut fixture.storage, &fixture.projects, &request)
        .unwrap();
    let before =
        chat_projection_record(&fixture.storage, chat["chatId"].as_str().unwrap()).unwrap();
    for field in ["text", "taskId", "workspaceIdentity"] {
        let mut invalid = request.clone();
        invalid[field] = json!("other");
        assert!(fixture
            .chats
            .append_user_message(&mut fixture.storage, &fixture.projects, &invalid)
            .is_err());
    }
    let stale = followup_request(&chat, 1, "user_abcdef0123456789abcdef0123456789");
    assert!(fixture
        .chats
        .append_user_message(&mut fixture.storage, &fixture.projects, &stale)
        .is_err());
    assert_eq!(
        chat_projection_record(&fixture.storage, chat["chatId"].as_str().unwrap()).unwrap(),
        before
    );
}

#[test]
fn unavailable_exact_profile_never_persists_or_spawns_a_run() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_unavailable_snapshot");
    fixture
        .chats
        .default_role_settings(&mut fixture.storage, &fixture.project_id, "req_defaults")
        .unwrap();
    let roles = json!([
        {"roleId":"planning","runtime":"codex","modelSelector":"unavailable","reasoningEffort":"medium","permission":"readOnly"},
        {"roleId":"coding","runtime":"claude-code","modelSelector":"opus","reasoningEffort":null,"permission":"workspaceWrite"},
        {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"high","permission":"readOnly"}
    ]);
    fixture
        .chats
        .update_role_settings(
            &mut fixture.storage,
            &fixture.project_id,
            1,
            roles,
            "req_unavailable_settings",
        )
        .unwrap();
    let chat = fixture
        .chats
        .create_chat(
            &mut fixture.storage,
            &fixture.projects,
            snapshot["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot["worktreeId"].as_str().unwrap(),
            snapshot["workspaceIdentity"].as_str().unwrap(),
            "no fallback",
            2,
            "req_unavailable_chat",
        )
        .unwrap()
        .result;
    let before = fixture.storage.projection_records("run").unwrap().len();
    let error = fixture
        .chats
        .start_fixture(
            &mut fixture.storage,
            &mut fixture.projects,
            chat["chatId"].as_str().unwrap(),
            "planning",
            "planning-success",
            "req_unavailable_run",
        )
        .unwrap_err();
    assert_eq!(error.kind, "manifestMismatch");
    assert_eq!(
        fixture.storage.projection_records("run").unwrap().len(),
        before
    );
}

#[test]
fn failed_b_creation_and_exact_abandon_leave_bound_a_fully_active() {
    let mut fixture = Fixture::new();
    let snapshot_a = fixture.snapshot("req_cleanup_snapshot_a");
    let chat_a = fixture.create_chat(&snapshot_a, "bound a", "req_cleanup_chat_a");
    let address_a = address_from_chat(&chat_a).unwrap();
    let snapshot_b = fixture.snapshot("req_cleanup_snapshot_b");
    let failed = fixture
        .chats
        .create_chat(
            &mut fixture.storage,
            &fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            snapshot_b["workspaceIdentity"].as_str().unwrap(),
            "must not bind",
            99,
            "req_cleanup_chat_b_failed",
        )
        .unwrap_err();
    assert_eq!(failed.kind, "dirtyConflict");
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        project["prepared_workspaces"][snapshot_b["preparationId"].as_str().unwrap()]
            ["bindingState"],
        "unbound"
    );
    assert!(fixture
        .chats
        .abandon_prepared_workspace(
            &mut fixture.storage,
            &mut fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            "workspace_wrong_alias",
            "req_cleanup_wrong_abandon_b",
        )
        .is_err());
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        project["prepared_workspaces"][snapshot_b["preparationId"].as_str().unwrap()]
            ["bindingState"],
        "unbound"
    );
    let abandoned = fixture
        .chats
        .abandon_prepared_workspace(
            &mut fixture.storage,
            &mut fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            snapshot_b["workspaceIdentity"].as_str().unwrap(),
            "req_cleanup_abandon_b",
        )
        .unwrap();
    assert_eq!(abandoned.result["bindingState"], "abandoned");
    assert!(fixture
        .chats
        .abandon_prepared_workspace(
            &mut fixture.storage,
            &mut fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            snapshot_b["workspaceIdentity"].as_str().unwrap(),
            "req_cleanup_duplicate_abandon_b",
        )
        .is_err());
    fixture
        .projects
        .workspace_get_tree(&fixture.storage, &address_a, "", None)
        .unwrap();
    let expected = sha256_file_version_v1(b"base");
    let begin = fixture
        .projects
        .workspace_begin_save(
            &mut fixture.storage,
            &address_a,
            "src/value.txt",
            &expected,
            4,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap();
    fixture
        .projects
        .workspace_append_save_chunk(&fixture.storage, &address_a, save_id, 1, "kept")
        .unwrap();
    fixture
        .projects
        .workspace_commit_save(
            &mut fixture.storage,
            &address_a,
            save_id,
            &expected,
            1,
            4,
            "req_cleanup_save_a",
        )
        .unwrap();
    assert_eq!(
        fixture
            .projects
            .workspace_read_file(&fixture.storage, &address_a, "src/value.txt")
            .unwrap()
            .streams[0]
            .chunks,
        vec!["kept"]
    );
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );
    assert!(fixture
        .chats
        .abandon_prepared_workspace(
            &mut fixture.storage,
            &mut fixture.projects,
            snapshot_a["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_a["worktreeId"].as_str().unwrap(),
            snapshot_a["workspaceIdentity"].as_str().unwrap(),
            "req_cleanup_late_abandon_a",
        )
        .is_err());
    fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_a, "src/value.txt")
        .unwrap();
}

#[test]
fn duplicate_chat_identity_rolls_back_and_keeps_new_preparation_unbound() {
    let mut fixture = Fixture::new();
    let snapshot_a = fixture.snapshot("req_duplicate_snapshot_a");
    let chat_a = fixture.create_chat(&snapshot_a, "chat a", "req_duplicate_chat_a");
    let snapshot_b = fixture.snapshot("req_duplicate_snapshot_b");
    let before = fixture.storage.projection_records("chat").unwrap().len();
    let error = fixture
        .chats
        .create_chat_inner(
            &mut fixture.storage,
            &fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            snapshot_b["workspaceIdentity"].as_str().unwrap(),
            "duplicate id",
            1,
            "req_duplicate_chat_b",
            chat_a["chatId"].as_str(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "manifestMismatch");
    assert_eq!(
        fixture.storage.projection_records("chat").unwrap().len(),
        before
    );
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        project["prepared_workspaces"][snapshot_b["preparationId"].as_str().unwrap()]
            ["bindingState"],
        "unbound"
    );
}

#[test]
fn injected_bind_crash_rolls_back_preparation_and_initial_message_together() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_crash_snapshot");
    fixture
        .chats
        .default_role_settings(
            &mut fixture.storage,
            &fixture.project_id,
            "req_crash_settings",
        )
        .unwrap();
    let settings =
        project_record(&fixture.storage, &fixture.project_id).unwrap()["role_settings"].clone();
    let created = now();
    let chat = json!({
        "schemaVersion":1,"chatId":"chat_injected_crash","taskId":"task_injected_crash",
        "projectId":fixture.project_id,"worktreeId":snapshot["worktreeId"],
        "workspaceIdentity":snapshot["workspaceIdentity"],"goal":"rollback together",
        "settingsRevision":1,"roleSnapshots":settings["roles"],"state":"draft",
        "createdAt":created,"updatedAt":created,
    });
    let message = team_message(&chat, 1, "user", None, None, "rollback together").unwrap();
    let payload = chat_projection(&chat, Some(message), 1).unwrap();
    let error = fixture
        .storage
        .bind_prepared_chat_atomic_with_failure(
            ChatBindInput {
                preparation_id: snapshot["preparationId"].as_str().unwrap().to_string(),
                project_id: fixture.project_id.clone(),
                worktree_id: snapshot["worktreeId"].as_str().unwrap().to_string(),
                workspace_identity: snapshot["workspaceIdentity"].as_str().unwrap().to_string(),
                expected_settings_revision: 1,
                chat_id: "chat_injected_crash".to_string(),
                request_id: "req_crash_bind".to_string(),
                chat_payload: payload,
            },
            crate::storage::ChatBindFailurePoint::AfterPreparationBindBeforeChat,
        )
        .unwrap_err();
    assert_eq!(error.kind(), "injectedFailure");
    assert!(fixture
        .storage
        .projection_record("chat", "chat_injected_crash")
        .unwrap()
        .is_none());
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        project["prepared_workspaces"][snapshot["preparationId"].as_str().unwrap()]["bindingState"],
        "unbound"
    );
}

fn git(path: &std::path::Path, args: &[&str]) {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sidecar_binary() -> std::path::PathBuf {
    // Cargo puts this test executable in <target>/<profile>/deps, including
    // custom target directories and target triples. Do not assume repo/target.
    let test_executable = std::env::current_exe().expect("current Cargo test executable");
    let binary = sidecar_binary_for_test_executable(&test_executable);
    assert!(
        binary.is_file(),
        "sidecar binary missing at {}: build it with `cargo build -p hello-agent-sidecar --locked --bin hello-agent-sidecar` using the same CARGO_TARGET_DIR, profile and target as this test",
        binary.display()
    );
    binary
}

fn sidecar_binary_for_test_executable(test_executable: &std::path::Path) -> std::path::PathBuf {
    let profile_directory = test_executable
        .parent()
        .and_then(std::path::Path::parent)
        .expect("Cargo test executable is inside a profile deps directory");
    profile_directory.join(format!(
        "hello-agent-sidecar{}",
        std::env::consts::EXE_SUFFIX
    ))
}

#[test]
fn sidecar_binary_follows_cargo_test_profile_directory() {
    for (executable, profile_directory) in [
        (
            "repo/target/debug/deps/hello_agent_sidecar-test",
            "repo/target/debug",
        ),
        (
            "shared/cargo-target/debug/deps/hello_agent_sidecar-test",
            "shared/cargo-target/debug",
        ),
        (
            "shared/cargo-target/aarch64-apple-darwin/release/deps/hello_agent_sidecar-test",
            "shared/cargo-target/aarch64-apple-darwin/release",
        ),
    ] {
        assert_eq!(
            sidecar_binary_for_test_executable(std::path::Path::new(executable)),
            std::path::Path::new(profile_directory).join(format!(
                "hello-agent-sidecar{}",
                std::env::consts::EXE_SUFFIX
            ))
        );
    }
}

fn initial_message_id(fixture: &Fixture, chat_id: &str) -> String {
    fixture
        .chats
        .get_chat(&fixture.storage, chat_id, 0, None, 10, false)
        .unwrap()["messages"][0]["messageId"]
        .as_str()
        .unwrap()
        .to_string()
}

fn start_exact_actual_scenario(
    fixture: &mut Fixture,
    scenario: FakeScenario,
    request: &str,
) -> (AgentRuntime, Value, String) {
    let snapshot = fixture.snapshot(&format!("{request}_snapshot"));
    let chat = fixture.create_exact_claude_chat(
        &snapshot,
        "provider boundary fixture",
        &format!("{request}_chat"),
    );
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(fixture, chat_id);
    let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let started = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            &format!("{request}_start"),
        )
        .unwrap();
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    (runtime, chat, run_id)
}

fn drain_actual_scenario(
    fixture: &mut Fixture,
    runtime: &mut AgentRuntime,
    run_id: &str,
) -> Vec<G5Event> {
    let mut events = Vec::new();
    for _ in 0..300 {
        events.extend(
            runtime
                .poll(&mut fixture.storage, &mut fixture.projects)
                .unwrap(),
        );
        if fixture
            .storage
            .projection_record("run", run_id)
            .unwrap()
            .unwrap()["record"]["state"]
            != "running"
        {
            return events;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("actual provider fixture did not reach a durable terminal state")
}

fn identityless_recovery_fixture(request: &str) -> (Fixture, AgentRuntime, Value, String) {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot(&format!("{request}_snapshot"));
    let chat = fixture.create_exact_claude_chat(
        &snapshot,
        "identityless recovery",
        &format!("{request}_chat"),
    );
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::SpawnFailure);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            &format!("{request}_start"),
        )
        .unwrap_err();
    let mut run = fixture
        .storage
        .projection_records("run")
        .unwrap()
        .into_iter()
        .find(|projection| projection["source_kind"] == "actualAgent")
        .unwrap();
    let run_id = format!("agent_run_crash_{}", uuid::Uuid::new_v4().simple());
    run.remove("last_event_sequence");
    run["run_id"] = Value::String(run_id.clone());
    run["record"]["runId"] = Value::String(run_id.clone());
    run["state"] = Value::String("preparing".into());
    run["record"]["state"] = Value::String("preparing".into());
    run["record"]["terminationOutcome"] = Value::Null;
    run["termination"] = Value::Null;
    run["recovery"] = Value::Null;
    run["barrier"] = Value::Null;
    run["process_identity_internal"] = Value::Null;

    let mut durable_chat = fixture
        .storage
        .projection_record("chat", chat_id)
        .unwrap()
        .unwrap();
    durable_chat.remove("last_event_sequence");
    durable_chat["state"] = Value::String("running".into());
    durable_chat["record"]["state"] = Value::String("running".into());
    let lock_id = format!("lock_{}", chat["worktreeId"].as_str().unwrap());
    let mut lock = fixture
        .storage
        .projection_record("lock", &lock_id)
        .unwrap()
        .unwrap();
    lock.remove("last_event_sequence");
    lock["writer_run_id"] = Value::String(run_id.clone());
    lock["poisoned"] = Value::Bool(false);
    lock["recovery_required"] = Value::Bool(false);
    fixture
        .storage
        .upsert_projection_records_atomic(vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run_id.clone(),
                operation_id: format!("op_{request}_crash_run"),
                request_id: None,
                payload: run,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: chat_id.into(),
                operation_id: format!("op_{request}_crash_chat"),
                request_id: None,
                payload: durable_chat,
            },
            ProjectionUpsert {
                aggregate_type: "lock",
                aggregate_id: lock_id,
                operation_id: format!("op_{request}_crash_lock"),
                request_id: None,
                payload: lock,
            },
        ])
        .unwrap();
    runtime = AgentRuntime::blocked();
    runtime
        .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
        .unwrap();
    (fixture, runtime, chat, run_id)
}

fn durable_provider_recovery_fixture(request: &str) -> (Fixture, Value, String) {
    let mut fixture = Fixture::new();
    let (mut runtime, chat, run_id) = start_exact_actual_scenario(
        &mut fixture,
        FakeScenario::ProviderTerminalRecoveryRequired,
        request,
    );
    drain_actual_scenario(&mut fixture, &mut runtime, &run_id);
    (fixture, chat, run_id)
}

#[test]
fn actual_agent_preflight_rejects_unknown_or_drifted_capability_with_zero_run_and_lock() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_agent_blocked_snapshot");
    let chat = fixture.create_chat(&snapshot, "blocked agent", "req_agent_blocked_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let before_runs = fixture.storage.projection_records("run").unwrap().len();
    let before_lock = fixture
        .storage
        .projection_record(
            "lock",
            &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
        )
        .unwrap()
        .unwrap();
    let mut runtime = AgentRuntime::blocked();
    let capability = runtime.get_capabilities("codex", false).unwrap();
    let error = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.to_string(),
                role_id: "planning".into(),
                input_message_ids: vec![message_id.clone()],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_agent_blocked",
        )
        .unwrap_err();
    assert!(error.detail.contains("before run, spawn, or lock"));
    assert_eq!(
        fixture.storage.projection_records("run").unwrap().len(),
        before_runs
    );
    assert_eq!(
        fixture
            .storage
            .projection_record(
                "lock",
                &format!("lock_{}", chat["worktreeId"].as_str().unwrap())
            )
            .unwrap()
            .unwrap(),
        before_lock
    );

    let mut fake = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let exact = fake.get_capabilities("claude-code", true).unwrap();
    assert_eq!(exact["canStart"], cfg!(target_os = "macos"));
    let error = fake
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.to_string(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: exact["fingerprint"].as_str().unwrap().into(),
            },
            "req_agent_drifted",
        )
        .unwrap_err();
    assert!(error.detail.contains("exactModelUnavailable"));
    assert_eq!(
        fixture.storage.projection_records("run").unwrap().len(),
        before_runs
    );
    assert_eq!(
        fixture
            .storage
            .projection_record(
                "lock",
                &format!("lock_{}", chat["worktreeId"].as_str().unwrap())
            )
            .unwrap()
            .unwrap(),
        before_lock
    );
}

#[test]
fn g5c_capability_cache_refresh_and_fresh_start_observation_fail_closed() {
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let cached = runtime.get_capabilities("claude-code", false).unwrap();
    assert_eq!(runtime.observation_count("claude-code"), 1);
    assert_eq!(
        runtime.get_capabilities("claude-code", false).unwrap(),
        cached
    );
    assert_eq!(runtime.observation_count("claude-code"), 1);
    assert_eq!(
        runtime.get_capabilities("claude-code", true).unwrap()["canStart"],
        cfg!(target_os = "macos")
    );
    assert_eq!(runtime.observation_count("claude-code"), 2);

    for (scenario, expected) in [
        (
            FakeScenario::DiscoveryMissingHelper,
            "installationIncomplete",
        ),
        (
            FakeScenario::DiscoveryUnknownVersion,
            "installationIncomplete",
        ),
        (FakeScenario::DiscoveryUnknownAuth, "notAuthenticated"),
    ] {
        let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        assert_eq!(capability["canStart"], false);
        assert_eq!(capability["blockedReason"], expected);
    }

    let mut codex = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let capability = codex.get_capabilities("codex", true).unwrap();
    assert_eq!(capability["canStart"], false);
    assert_eq!(capability["blockedReason"], "exactModelUnavailable");

    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_g5c_fingerprint_snapshot");
    let chat = fixture.create_exact_claude_chat(
        &snapshot,
        "fingerprint drift",
        "req_g5c_fingerprint_chat",
    );
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let before_runs = fixture.storage.projection_records("run").unwrap().len();
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let error = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: format!("sha256:{}", "0".repeat(64)),
            },
            "req_g5c_fingerprint_start",
        )
        .unwrap_err();
    assert!(error.detail.contains("profileChanged"));
    assert_eq!(
        fixture.storage.projection_records("run").unwrap().len(),
        before_runs
    );

    for scenario in [
        FakeScenario::DiscoveryStaleClosure,
        FakeScenario::DiscoveryProfileDrift,
    ] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_g5c_drift_snapshot");
        let chat =
            fixture.create_exact_claude_chat(&snapshot, "fresh discovery", "req_g5c_drift_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let before_runs = fixture.storage.projection_records("run").unwrap().len();
        let lock_id = format!("lock_{}", chat["worktreeId"].as_str().unwrap());
        let before_lock = fixture
            .storage
            .projection_record("lock", &lock_id)
            .unwrap()
            .unwrap();
        let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
        let cached = runtime.get_capabilities("claude-code", false).unwrap();
        assert_eq!(cached["canStart"], cfg!(target_os = "macos"));
        let error = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: cached["fingerprint"].as_str().unwrap().into(),
                },
                "req_g5c_drift_start",
            )
            .unwrap_err();
        assert!(error.detail.contains("before run, spawn, or lock"));
        assert_eq!(runtime.observation_count("claude-code"), 2);
        assert_eq!(
            fixture.storage.projection_records("run").unwrap().len(),
            before_runs
        );
        assert_eq!(
            fixture
                .storage
                .projection_record("lock", &lock_id)
                .unwrap()
                .unwrap(),
            before_lock
        );
    }
}

#[test]
fn g5c_each_claude_handshake_mismatch_and_attestation_failure_emit_nothing() {
    for scenario in [
        FakeScenario::HandshakeBeforePublic,
        FakeScenario::HandshakeVersionMismatch,
        FakeScenario::HandshakeModelMismatch,
        FakeScenario::HandshakeCwdMismatch,
        FakeScenario::HandshakePermissionMismatch,
        FakeScenario::HandshakeToolsMismatch,
        FakeScenario::HandshakeMcpMismatch,
        FakeScenario::HandshakePluginsMismatch,
        FakeScenario::HandshakeConfigMismatch,
        FakeScenario::AttestationPersistenceFailure,
        FakeScenario::ProviderAuthFailure,
    ] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_g5c_handshake_snapshot");
        let chat =
            fixture.create_exact_claude_chat(&snapshot, "handshake gate", "req_g5c_handshake_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let error = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_g5c_handshake_start",
            )
            .unwrap_err();
        assert!(
            error.detail.contains("handshake") || error.detail.contains("attestation"),
            "{}",
            error.detail
        );
        let run = fixture
            .storage
            .projection_records("run")
            .unwrap()
            .into_iter()
            .find(|run| run["source_kind"] == "actualAgent")
            .unwrap();
        assert_eq!(run["record"]["state"], "failed");
        assert!(run.get("runtime_attestation").is_none());
        assert!(run["public_events"].as_array().unwrap().is_empty());
        assert!(!runtime.has_active_run(run["run_id"].as_str().unwrap()));
        let lock = fixture
            .storage
            .projection_record(
                "lock",
                &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
            )
            .unwrap()
            .unwrap();
        assert_eq!(lock["writer_run_id"], Value::Null);
        assert_eq!(lock["poisoned"], false);
        let serialized = serde_json::to_string(&run).unwrap().to_lowercase();
        for forbidden in [
            "private@example.invalid",
            "private-org",
            "private-token",
            "must-not-escape",
        ] {
            assert!(!serialized.contains(forbidden));
            assert!(!error.detail.to_lowercase().contains(forbidden));
        }
    }
}

#[test]
fn actual_agent_pre_release_failures_are_terminal_and_release_writer_after_reopen() {
    use hello_agent_core::proctree::{ProbeResult, ProcIdentity, ProcTable, SystemProcTable};
    for scenario in [
        FakeScenario::SpawnFailure,
        FakeScenario::IdentityFailure,
        FakeScenario::ObserveFailure,
        FakeScenario::ReleaseFailure,
    ] {
        let mut fixture = Fixture::new();
        let database = fixture._root.path().join("data/hello-agent.sqlite3");
        let snapshot = fixture.snapshot("req_pre_release_snapshot");
        let chat = fixture.create_exact_claude_chat(
            &snapshot,
            "pre-release failure",
            "req_pre_release_chat",
        );
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let error = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_pre_release_failure",
            )
            .unwrap_err();
        assert!(error.detail.contains("failure"));
        let actual = fixture
            .storage
            .projection_records("run")
            .unwrap()
            .into_iter()
            .find(|run| run["source_kind"] == "actualAgent")
            .unwrap();
        let actual_run_id = actual["run_id"].as_str().unwrap().to_string();
        assert_eq!(actual["record"]["state"], "failed");
        assert_eq!(actual["pre_release_failure"]["processesProvenGone"], true);
        if scenario == FakeScenario::ReleaseFailure {
            assert_eq!(actual["barrier"]["releaseGranted"], false);
        }
        assert!(actual["process_identity_internal"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| matches!(
                SystemProcTable.probe(ProcIdentity {
                    pid: value["pid"].as_u64().unwrap() as u32,
                    start_us: value["start_us"].as_i64().unwrap(),
                }),
                ProbeResult::Gone
            )));
        let lock_id = format!("lock_{}", chat["worktreeId"].as_str().unwrap());
        let lock = fixture
            .storage
            .projection_record("lock", &lock_id)
            .unwrap()
            .unwrap();
        assert_eq!(lock["writer_run_id"], Value::Null);
        assert_eq!(lock["poisoned"], false);
        drop(runtime);
        let Fixture {
            _root: root_guard,
            storage,
            projects,
            chats,
            ..
        } = fixture;
        drop(storage);
        drop(projects);
        drop(chats);
        let reopened = Storage::open(&database).unwrap();
        let lock = reopened
            .projection_record("lock", &lock_id)
            .unwrap()
            .unwrap();
        assert_eq!(lock["writer_run_id"], Value::Null);
        assert_eq!(lock["recovery_required"], false);
        let run = reopened
            .projection_record("run", &actual_run_id)
            .unwrap()
            .unwrap();
        assert_eq!(run["record"]["state"], "failed");
        if scenario == FakeScenario::ReleaseFailure {
            assert_eq!(run["barrier"]["releaseGranted"], false);
        }
        drop(reopened);
        drop(root_guard);
    }
}

#[test]
fn exact_fake_agent_barrier_filters_private_events_and_keeps_fixture_ledger_distinct() {
    let executable = sidecar_binary();
    assert!(
        executable.is_file(),
        "build the sidecar binary for process gate"
    );
    let mut fixture = Fixture::new();
    let database = fixture._root.path().join("data/hello-agent.sqlite3");
    let snapshot = fixture.snapshot("req_agent_clean_snapshot");
    let chat = fixture.create_exact_claude_chat(&snapshot, "clean agent", "req_agent_clean_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let mut runtime = AgentRuntime::fake(executable, FakeScenario::Clean);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let started = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_agent_clean",
        )
        .unwrap();
    assert!(
        started.events.is_empty(),
        "accepted response must precede all public events"
    );
    for field in [
        "chatId",
        "taskId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        assert_eq!(started.result[field], chat[field]);
    }
    assert_eq!(started.result["roleId"], "coding");
    assert_eq!(
        started.result["capabilityFingerprint"],
        capability["fingerprint"]
    );
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    let preparing = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert_eq!(preparing["source_kind"], "actualAgent");
    assert_eq!(
        preparing["barrier"]["runAndLockPersistedBeforeRelease"],
        true
    );
    assert_eq!(preparing["barrier"]["identityPersistedBeforeRelease"], true);
    assert_eq!(preparing["barrier"]["releaseGranted"], true);
    assert_eq!(
        preparing["barrier"]["trackedProcesses"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(preparing["provider_discovery"]["providerId"], "claude-code");
    assert_eq!(preparing["provider_discovery"]["canStart"], true);
    assert_eq!(preparing["runtime_attestation"]["runId"], run_id);
    assert_eq!(
        preparing["runtime_attestation"]["capabilityFingerprint"],
        capability["fingerprint"]
    );
    assert_eq!(
        preparing["runtime_attestation"]["workspaceIdentity"],
        chat["workspaceIdentity"]
    );
    assert_eq!(
        preparing["runtime_attestation"]["effectiveProfile"]["modelSelector"],
        "claude-opus-5"
    );
    let provider_evidence = serde_json::to_string(&json!({
        "discovery":preparing["provider_discovery"],
        "attestation":preparing["runtime_attestation"]
    }))
    .unwrap()
    .to_lowercase();
    for forbidden in [
        "private@example.invalid",
        "private-org",
        "private-token",
        "token-never-persist",
        "prompt",
        "email",
        "organization",
    ] {
        assert!(!provider_evidence.contains(forbidden));
    }
    let mut wire_events = Vec::new();
    for _ in 0..100 {
        wire_events.extend(
            runtime
                .poll(&mut fixture.storage, &mut fixture.projects)
                .unwrap(),
        );
        if fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap()["state"]
            != "running"
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(wire_events.len(), 5);
    for (index, event) in wire_events.iter().enumerate() {
        assert_eq!(event.seq, (index + 1) as u64);
        assert_eq!(event.chat_id.as_deref(), Some(chat_id));
        assert_eq!(event.task_id.as_deref(), chat["taskId"].as_str());
        assert_eq!(event.run_id.as_deref(), Some(run_id.as_str()));
    }
    assert_eq!(wire_events[0].event, "agent.runStateChanged");
    assert_eq!(wire_events[0].payload["run"]["state"], "running");
    assert_eq!(wire_events[4].event, "agent.runStateChanged");
    assert_eq!(wire_events[4].payload["run"]["state"], "succeeded");
    for event in &wire_events[1..4] {
        assert_eq!(event.event, "agent.publicEvent");
        assert_eq!(event.payload["chatId"], chat["chatId"]);
        assert_eq!(event.payload["taskId"], chat["taskId"]);
        assert_eq!(event.payload["projectId"], chat["projectId"]);
        assert_eq!(event.payload["worktreeId"], chat["worktreeId"]);
        assert_eq!(
            event.payload["workspaceIdentity"],
            chat["workspaceIdentity"]
        );
        assert_eq!(event.payload["runId"], run_id);
        assert_eq!(event.payload["roleId"], "coding");
        assert_eq!(
            event.payload["capabilityFingerprint"],
            capability["fingerprint"]
        );
        hello_agent_protocol::agent_adapter_v1::validate_agent_adapter_payload_v1(
            "publicEvent",
            &event.payload,
        )
        .unwrap();
    }
    assert_eq!(
        wire_events[1..4]
            .iter()
            .map(|event| event.payload["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["status", "question", "final"]
    );
    let terminal = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert_eq!(terminal["record"]["state"], "succeeded");
    assert_eq!(terminal["termination"]["processesProvenGone"], true);
    let public_events = terminal["public_events"].as_array().unwrap();
    assert_eq!(terminal["public_event_count"], public_events.len() as u64);
    assert_eq!(
        terminal["public_text_bytes"],
        public_events
            .iter()
            .map(|event| event["text"].as_str().unwrap().len() as u64)
            .sum::<u64>()
    );
    assert_eq!(
        terminal["public_ledger_cumulative_snapshot_bytes"],
        hello_agent_protocol::agent_adapter_v1::agent_public_ledger_cumulative_snapshot_bytes_v1(
            public_events
        )
        .unwrap() as u64
    );
    runtime
        .validate_persisted_runs(&fixture.storage)
        .expect("durable counters validate after run completion");
    AgentRuntime::blocked()
        .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
        .expect("terminal run recovery scan is a no-op");
    assert_eq!(
        fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap(),
        terminal,
        "already terminal actual run must not be rewritten"
    );
    let serialized = serde_json::to_string(&terminal).unwrap();
    assert!(!serialized.contains("token-never-persist"));
    assert!(!serialized.contains("\"type\":\"reasoning\""));
    assert!(terminal["public_events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| {
            ["status", "question", "final"].contains(&event["kind"].as_str().unwrap_or_default())
        }));
    assert!(terminal["public_events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| {
            event["chatId"] == chat["chatId"]
                && event["taskId"] == chat["taskId"]
                && event["projectId"] == chat["projectId"]
                && event["worktreeId"] == chat["worktreeId"]
                && event["workspaceIdentity"] == chat["workspaceIdentity"]
                && event["runId"] == run_id
                && event["roleId"] == "coding"
                && event["capabilityFingerprint"] == capability["fingerprint"]
        }));
    let current_g5_detail = fixture
        .chats
        .get_chat(&fixture.storage, chat_id, 0, None, 10, false)
        .unwrap();
    assert!(current_g5_detail.get("agentRuns").is_none());
    assert!(current_g5_detail.get("agentRecovery").is_none());
    assert!(current_g5_detail["runs"].as_array().unwrap().is_empty());
    let g5a_detail = fixture
        .chats
        .get_chat(&fixture.storage, chat_id, 0, None, 10, true)
        .unwrap();
    assert_eq!(g5a_detail["agentRuns"][0]["state"], "succeeded");
    assert_eq!(g5a_detail["agentRecovery"], Value::Null);
    assert_eq!(g5a_detail["messages"].as_array().unwrap().len(), 4);
    hello_agent_protocol::agent_adapter_v1::validate_agent_adapter_payload_v1(
        "agentChatDetailPage",
        &g5a_detail,
    )
    .unwrap();
    drop(runtime);
    let Fixture {
        _root: root_guard,
        storage,
        projects,
        chats,
        ..
    } = fixture;
    drop(storage);
    drop(projects);
    drop(chats);
    let reopened = Storage::open(&database).expect("reopen durable agent ledger");
    AgentRuntime::blocked()
        .validate_persisted_runs(&reopened)
        .expect("reopened durable counters validate");
    drop(reopened);
    drop(root_guard);
}

#[test]
fn no_output_and_delayed_final_emit_running_then_terminal_from_durable_state() {
    for scenario in [FakeScenario::NoOutput, FakeScenario::FinalDelayed] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_agent_terminal_snapshot");
        let chat = fixture.create_exact_claude_chat(
            &snapshot,
            "terminal notification",
            "req_agent_terminal_chat",
        );
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let started = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_agent_terminal",
            )
            .unwrap();
        let run_id = started.result["runId"].as_str().unwrap().to_string();
        let mut wire = Vec::new();
        let mut saw_final_while_running = false;
        for _ in 0..150 {
            let polled = runtime
                .poll(&mut fixture.storage, &mut fixture.projects)
                .unwrap();
            if polled.iter().any(|event| event.payload["kind"] == "final") {
                saw_final_while_running = fixture
                    .storage
                    .projection_record("run", &run_id)
                    .unwrap()
                    .unwrap()["record"]["state"]
                    == "running";
            }
            wire.extend(polled);
            if fixture
                .storage
                .projection_record("run", &run_id)
                .unwrap()
                .unwrap()["record"]["state"]
                != "running"
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(wire.first().unwrap().event, "agent.runStateChanged");
        assert_eq!(wire.first().unwrap().payload["run"]["state"], "running");
        assert_eq!(wire.last().unwrap().event, "agent.runStateChanged");
        assert_eq!(wire.last().unwrap().payload["run"]["state"], "succeeded");
        assert_eq!(
            wire.iter().map(|event| event.seq).collect::<Vec<_>>(),
            (1..=wire.len() as u64).collect::<Vec<_>>()
        );
        if scenario == FakeScenario::NoOutput {
            assert_eq!(wire.len(), 2, "zero output still has both state events");
        } else {
            assert!(saw_final_while_running, "final is not terminal evidence");
        }
    }
}

#[test]
fn public_text_plus_one_is_not_stored_or_emitted_and_cleanly_fails_run() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_agent_budget_snapshot");
    let chat = fixture.create_exact_claude_chat(&snapshot, "event budget", "req_agent_budget_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let started = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_agent_budget",
        )
        .unwrap();
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    let mut projection = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    let events = (0..16)
        .map(|index| {
            json!({"schemaVersion":1,"eventId":format!("agent_event_budget_{index}"),"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"roleId":"coding","capabilityFingerprint":capability["fingerprint"],"kind":"status","text":"x".repeat(64 * 1024),"createdAt":projection["record"]["updatedAt"]})
        })
        .collect::<Vec<_>>();
    let cumulative =
        hello_agent_protocol::agent_adapter_v1::agent_public_ledger_cumulative_snapshot_bytes_v1(
            &events,
        )
        .unwrap();
    assert!(
        cumulative
            <= hello_agent_protocol::AGENT_PUBLIC_LEDGER_CUMULATIVE_SNAPSHOT_BYTES_PER_RUN_MAX_V1
    );
    let event_count = events.len();
    projection.insert("public_events".into(), Value::Array(events));
    projection.insert("public_event_count".into(), Value::from(event_count as u64));
    projection.insert(
        "public_text_bytes".into(),
        Value::from(hello_agent_protocol::AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1 as u64),
    );
    projection.insert(
        "public_ledger_cumulative_snapshot_bytes".into(),
        Value::from(cumulative as u64),
    );
    fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: "op_test_agent_budget_boundary".into(),
            request_id: None,
            payload: projection,
        }])
        .unwrap();
    let emitted = runtime
        .poll(&mut fixture.storage, &mut fixture.projects)
        .unwrap();
    assert_eq!(
        emitted
            .iter()
            .filter(|event| event.event == "agent.publicEvent")
            .count(),
        0
    );
    assert_eq!(emitted[0].payload["run"]["state"], "running");
    assert_eq!(emitted.last().unwrap().payload["run"]["state"], "failed");
    let terminal = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        terminal["public_text_bytes"],
        hello_agent_protocol::AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1 as u64
    );
    assert_eq!(terminal["record"]["state"], "failed");
    assert_eq!(terminal["termination"]["outcome"], "cleanExit");
    assert_eq!(terminal["termination"]["processesProvenGone"], true);
}

#[test]
fn real_provider_drain_accepts_exact_public_text_boundaries_before_rejecting_plus_one() {
    for (scenario, expected_events, expected_state) in [
        (FakeScenario::PublicTextExact, 1_u64, "succeeded"),
        (FakeScenario::PublicAggregateExact, 16_u64, "succeeded"),
        (FakeScenario::PublicAggregatePlusOne, 16_u64, "failed"),
    ] {
        let mut fixture = Fixture::new();
        let (mut runtime, _chat, run_id) =
            start_exact_actual_scenario(&mut fixture, scenario, "req_raw_boundary");
        let wire = drain_actual_scenario(&mut fixture, &mut runtime, &run_id);
        let terminal = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        assert_eq!(terminal["record"]["state"], expected_state);
        assert_eq!(terminal["public_event_count"], expected_events);
        assert_eq!(
            terminal["public_text_bytes"],
            expected_events * MAX_AGENT_EVENT_BYTES as u64
        );
        assert_eq!(
            terminal["public_events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| event["text"].as_str().unwrap().len())
                .collect::<Vec<_>>(),
            vec![MAX_AGENT_EVENT_BYTES; expected_events as usize]
        );
        assert_eq!(
            wire.iter()
                .filter(|event| event.event == "agent.publicEvent")
                .count(),
            expected_events as usize
        );
        assert_eq!(wire.last().unwrap().payload["run"]["state"], expected_state);
    }
}

#[test]
fn provider_terminal_recovery_required_poisons_even_after_clean_exit_until_abandon() {
    let mut fixture = Fixture::new();
    let (mut runtime, chat, run_id) = start_exact_actual_scenario(
        &mut fixture,
        FakeScenario::ProviderTerminalRecoveryRequired,
        "req_provider_recovery",
    );
    let wire = drain_actual_scenario(&mut fixture, &mut runtime, &run_id);
    assert_eq!(
        wire.last().unwrap().payload["run"]["state"],
        "recoveryRequired"
    );

    let run = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert_eq!(run["record"]["state"], "recoveryRequired");
    assert_eq!(run["termination"]["outcome"], "recoveryRequired");
    assert_eq!(run["termination"]["processesProvenGone"], false);
    assert!(run["termination"]["trackedProcesses"]
        .as_array()
        .unwrap()
        .iter()
        .all(|process| process["observedGone"] == true));
    assert_eq!(run["termination"]["poisoned"], true);
    assert_eq!(run["recovery"]["state"], "unresolved");
    assert_eq!(run["recovery"]["source"], "actualAgent");
    assert_eq!(
        run["recovery"]["recoveryId"],
        run["recovery_id_reservation"]
    );
    let durable_chat = fixture
        .storage
        .projection_record("chat", chat["chatId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(durable_chat["record"]["state"], "recoveryRequired");
    let lock = fixture
        .storage
        .projection_record(
            "lock",
            &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(lock["writer_run_id"], run_id);
    assert_eq!(lock["poisoned"], true);
    assert_eq!(lock["recovery_required"], true);
    assert_eq!(
        fixture
            .projects
            .workspace_read_file(
                &fixture.storage,
                &address_from_chat(&chat).unwrap(),
                "src/value.txt",
            )
            .unwrap_err()
            .kind,
        "recoveryRequired"
    );
    assert!(!fixture.storage.blocker_ids().unwrap().is_empty());
    assert!(fixture
        .chats
        .list_recoveries(&fixture.storage, None, 10)
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());

    let detail = fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
            true,
        )
        .unwrap();
    hello_agent_protocol::agent_adapter_v1::validate_agent_adapter_payload_v1(
        "agentChatDetailPage",
        &detail,
    )
    .unwrap();
    let recovery = detail["agentRecovery"].clone();
    assert_eq!(recovery, run["recovery"]);
    assert_eq!(recovery["chatId"], chat["chatId"]);
    assert_eq!(recovery["taskId"], chat["taskId"]);
    assert_eq!(recovery["projectId"], chat["projectId"]);
    assert_eq!(recovery["worktreeId"], chat["worktreeId"]);
    assert_eq!(recovery["workspaceIdentity"], chat["workspaceIdentity"]);
    assert_eq!(recovery["runId"], run_id);
    assert_eq!(recovery["revision"], 1);
    let request = json!({
        "recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],
        "taskId":chat["taskId"],"projectId":chat["projectId"],
        "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
        "runId":run_id,"expectedRevision":1,"resolution":"abandon"
    });
    let inspect_request = json!({
        "recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],
        "taskId":chat["taskId"],"projectId":chat["projectId"],
        "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
        "runId":run_id
    });
    assert_eq!(
        runtime
            .inspect_recovery(&fixture.storage, &inspect_request)
            .unwrap()["state"],
        "unresolved"
    );
    let resolved = runtime
        .resolve_recovery(
            &mut fixture.storage,
            &mut fixture.projects,
            &request,
            "req_provider_recovery_resolve",
        )
        .unwrap();
    assert_eq!(resolved["outcome"], "resolvedAbandoned");
    let lock = fixture
        .storage
        .projection_record(
            "lock",
            &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(lock["writer_run_id"], Value::Null);
    assert_eq!(lock["poisoned"], false);
    assert!(fixture.storage.blocker_ids().unwrap().is_empty());
    let resolved_detail = fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
            true,
        )
        .unwrap();
    assert_eq!(resolved_detail["agentRecovery"], Value::Null);
    assert_eq!(resolved_detail["agentRuns"][0]["state"], "canceled");
    assert_eq!(resolved_detail["chat"]["state"], "discarded");
    hello_agent_protocol::agent_adapter_v1::validate_agent_adapter_payload_v1(
        "agentChatDetailPage",
        &resolved_detail,
    )
    .unwrap();
}

#[test]
fn recovery_reservations_are_random_per_run_and_identityless_restart_is_immutable() {
    let (mut fixture, mut runtime, chat, run_id) =
        identityless_recovery_fixture("req_identityless_a");
    let run = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    let recovery = run["recovery"].clone();
    let recovery_id = recovery["recoveryId"].as_str().unwrap().to_string();
    assert_eq!(run["recovery_id_reservation"], recovery_id);
    assert_eq!(recovery["reason"], "processIdentityUnavailable");
    assert_eq!(recovery["trackedProcesses"], json!([]));
    assert!(!recovery_id.contains(&run_id));
    assert_eq!(recovery_id.len(), "agent_recovery_".len() + 32);
    let detail = fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
            true,
        )
        .unwrap();
    hello_agent_protocol::agent_adapter_v1::validate_agent_adapter_payload_v1(
        "agentChatDetailPage",
        &detail,
    )
    .unwrap();
    assert_eq!(detail["agentRecovery"], recovery);

    let request = json!({
        "recoveryId":recovery_id,"chatId":chat["chatId"],"taskId":chat["taskId"],
        "projectId":chat["projectId"],"worktreeId":chat["worktreeId"],
        "workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,
        "expectedRevision":2,"resolution":"abandon"
    });
    let stale = runtime
        .resolve_recovery(
            &mut fixture.storage,
            &mut fixture.projects,
            &request,
            "req_identityless_stale",
        )
        .unwrap();
    assert_eq!(stale["outcome"], "revisionConflict");
    let before_records = fixture.storage.projection_records("run").unwrap();
    let manifest_path = fixture
        ._root
        .path()
        .join("data/worktree-manifests-v1")
        .join(format!("{}.json", chat["worktreeId"].as_str().unwrap()));
    let before_manifest = fs::read(&manifest_path).unwrap();
    let mut matching = request;
    matching["expectedRevision"] = Value::from(1);
    let outcome = runtime
        .resolve_recovery(
            &mut fixture.storage,
            &mut fixture.projects,
            &matching,
            "req_identityless_matching",
        )
        .unwrap();
    assert_eq!(outcome["outcome"], "identityUnavailable");
    assert_eq!(outcome["after"], Value::Null);
    assert_eq!(
        fixture.storage.projection_records("run").unwrap(),
        before_records
    );
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    runtime
        .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
        .unwrap();
    assert_eq!(
        fixture
            .storage
            .projection_record("run", matching["runId"].as_str().unwrap())
            .unwrap()
            .unwrap()["recovery"]["recoveryId"],
        matching["recoveryId"]
    );

    let (mut fixture_b, runtime_b, _, run_id_b) =
        identityless_recovery_fixture("req_identityless_b");
    let run_b = fixture_b
        .storage
        .projection_record("run", &run_id_b)
        .unwrap()
        .unwrap();
    assert_ne!(run_b["recovery_id_reservation"], matching["recoveryId"]);
    assert!(!run_b["recovery_id_reservation"]
        .as_str()
        .unwrap()
        .contains(&run_id_b));

    let mut legacy = fixture_b
        .storage
        .projection_records("run")
        .unwrap()
        .into_iter()
        .find(|projection| {
            projection["source_kind"] == "actualAgent"
                && projection["run_id"] != run_id_b
                && projection["recovery"].is_null()
        })
        .unwrap();
    let legacy_run_id = legacy["run_id"].as_str().unwrap().to_string();
    legacy.remove("last_event_sequence");
    legacy.remove("recovery_id_reservation");
    fixture_b
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: legacy_run_id.clone(),
            operation_id: "op_test_remove_legacy_reservation".into(),
            request_id: None,
            payload: legacy,
        }])
        .unwrap();
    runtime_b
        .reserve_legacy_recovery_for_test(&mut fixture_b.storage, &legacy_run_id, true)
        .unwrap_err();
    assert!(fixture_b
        .storage
        .projection_record("run", &legacy_run_id)
        .unwrap()
        .unwrap()
        .get("recovery_id_reservation")
        .is_none());
    let retried = runtime_b
        .reserve_legacy_recovery_for_test(&mut fixture_b.storage, &legacy_run_id, false)
        .unwrap();
    let legacy_recovery_id = retried["recovery_id_reservation"].as_str().unwrap();
    assert_eq!(legacy_recovery_id.len(), "agent_recovery_".len() + 32);
    assert!(!legacy_recovery_id.contains(&legacy_run_id));
}

#[test]
fn startup_backfills_pre_g5c_recovery_ids_without_replacing_public_records() {
    let (mut unresolved_fixture, unresolved_chat, unresolved_run_id) =
        durable_provider_recovery_fixture("req_legacy_unresolved_recovery");
    let mut unresolved_projection = unresolved_fixture
        .storage
        .projection_record("run", &unresolved_run_id)
        .unwrap()
        .unwrap();
    let unresolved_recovery = unresolved_projection["recovery"].clone();
    let unresolved_recovery_id = unresolved_recovery["recoveryId"]
        .as_str()
        .unwrap()
        .to_string();
    unresolved_projection.remove("last_event_sequence");
    unresolved_projection.remove("recovery_id_reservation");
    unresolved_fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: unresolved_run_id.clone(),
            operation_id: "op_test_pre_g5c_unresolved_recovery".into(),
            request_id: None,
            payload: unresolved_projection,
        }])
        .unwrap();

    let mut unresolved_runtime = AgentRuntime::blocked();
    unresolved_runtime
        .recover_unclean_runs(
            &mut unresolved_fixture.storage,
            &mut unresolved_fixture.projects,
        )
        .unwrap();
    unresolved_runtime
        .validate_persisted_runs(&unresolved_fixture.storage)
        .unwrap();
    let backfilled_unresolved = unresolved_fixture
        .storage
        .projection_record("run", &unresolved_run_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        backfilled_unresolved["recovery_id_reservation"],
        unresolved_recovery_id
    );
    assert_eq!(backfilled_unresolved["recovery"], unresolved_recovery);
    let unresolved_inspect = json!({
        "recoveryId":unresolved_recovery_id,"chatId":unresolved_chat["chatId"],
        "taskId":unresolved_chat["taskId"],"projectId":unresolved_chat["projectId"],
        "worktreeId":unresolved_chat["worktreeId"],
        "workspaceIdentity":unresolved_chat["workspaceIdentity"],"runId":unresolved_run_id
    });
    assert_eq!(
        unresolved_runtime
            .inspect_recovery(&unresolved_fixture.storage, &unresolved_inspect)
            .unwrap(),
        unresolved_recovery
    );
    let mut unresolved_resolve = unresolved_inspect;
    unresolved_resolve["expectedRevision"] = Value::from(1);
    unresolved_resolve["resolution"] = Value::String("abandon".into());
    let resolved_from_legacy = unresolved_runtime
        .resolve_recovery(
            &mut unresolved_fixture.storage,
            &mut unresolved_fixture.projects,
            &unresolved_resolve,
            "req_resolve_pre_g5c_unresolved",
        )
        .unwrap();
    assert_eq!(resolved_from_legacy["outcome"], "resolvedAbandoned");
    assert_eq!(
        resolved_from_legacy["after"]["recoveryId"],
        unresolved_recovery_id
    );

    let (mut resolved_fixture, resolved_chat, resolved_run_id) =
        durable_provider_recovery_fixture("req_legacy_resolved_recovery");
    let mut resolved_runtime = AgentRuntime::blocked();
    let unresolved = resolved_fixture
        .storage
        .projection_record("run", &resolved_run_id)
        .unwrap()
        .unwrap()["recovery"]
        .clone();
    let resolved_recovery_id = unresolved["recoveryId"].as_str().unwrap().to_string();
    let resolve_request = json!({
        "recoveryId":resolved_recovery_id,"chatId":resolved_chat["chatId"],
        "taskId":resolved_chat["taskId"],"projectId":resolved_chat["projectId"],
        "worktreeId":resolved_chat["worktreeId"],
        "workspaceIdentity":resolved_chat["workspaceIdentity"],"runId":resolved_run_id,
        "expectedRevision":1,"resolution":"abandon"
    });
    let resolved_result = resolved_runtime
        .resolve_recovery(
            &mut resolved_fixture.storage,
            &mut resolved_fixture.projects,
            &resolve_request,
            "req_prepare_pre_g5c_resolved",
        )
        .unwrap();
    assert_eq!(resolved_result["outcome"], "resolvedAbandoned");
    let resolved_recovery = resolved_result["after"].clone();
    let mut resolved_projection = resolved_fixture
        .storage
        .projection_record("run", &resolved_run_id)
        .unwrap()
        .unwrap();
    resolved_projection.remove("last_event_sequence");
    resolved_projection.remove("recovery_id_reservation");
    resolved_fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: resolved_run_id.clone(),
            operation_id: "op_test_pre_g5c_resolved_recovery".into(),
            request_id: None,
            payload: resolved_projection,
        }])
        .unwrap();

    resolved_runtime
        .recover_unclean_runs(
            &mut resolved_fixture.storage,
            &mut resolved_fixture.projects,
        )
        .unwrap();
    resolved_runtime
        .validate_persisted_runs(&resolved_fixture.storage)
        .unwrap();
    let backfilled_resolved = resolved_fixture
        .storage
        .projection_record("run", &resolved_run_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        backfilled_resolved["recovery_id_reservation"],
        resolved_recovery_id
    );
    assert_eq!(backfilled_resolved["recovery"], resolved_recovery);
    let resolved_inspect = json!({
        "recoveryId":resolved_recovery_id,"chatId":resolved_chat["chatId"],
        "taskId":resolved_chat["taskId"],"projectId":resolved_chat["projectId"],
        "worktreeId":resolved_chat["worktreeId"],
        "workspaceIdentity":resolved_chat["workspaceIdentity"],"runId":resolved_run_id
    });
    assert_eq!(
        resolved_runtime
            .inspect_recovery(&resolved_fixture.storage, &resolved_inspect)
            .unwrap(),
        resolved_recovery
    );
    let mut repeat_resolve = resolved_inspect;
    repeat_resolve["expectedRevision"] = Value::from(1);
    repeat_resolve["resolution"] = Value::String("abandon".into());
    let before_repeat = resolved_fixture
        .storage
        .projection_record("run", &resolved_run_id)
        .unwrap()
        .unwrap();
    let repeat_error = resolved_runtime
        .resolve_recovery(
            &mut resolved_fixture.storage,
            &mut resolved_fixture.projects,
            &repeat_resolve,
            "req_repeat_pre_g5c_resolved",
        )
        .unwrap_err();
    assert_eq!(repeat_error.kind, "manifestMismatch");
    assert!(repeat_error
        .detail
        .contains("only unresolved recovery can be resolved"));
    assert_eq!(
        resolved_fixture
            .storage
            .projection_record("run", &resolved_run_id)
            .unwrap()
            .unwrap(),
        before_repeat
    );
}

#[test]
fn recovery_abandon_saga_reconciles_each_crash_boundary_and_mismatch_stays_poisoned() {
    let (mut fixture, chat, run_id) = durable_provider_recovery_fixture("req_saga_intent_fail");
    let run = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    let recovery = run["recovery"].clone();
    let request = json!({
        "recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],
        "taskId":chat["taskId"],"projectId":chat["projectId"],
        "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
        "runId":run_id,"expectedRevision":1,"resolution":"abandon"
    });
    let manifest_path = fixture
        ._root
        .path()
        .join("data/worktree-manifests-v1")
        .join(format!("{}.json", chat["worktreeId"].as_str().unwrap()));
    let manifest_before = fs::read(&manifest_path).unwrap();
    AgentRuntime::blocked()
        .resolve_recovery_with_failure(
            &mut fixture.storage,
            &mut fixture.projects,
            &request,
            "req_saga_intent_fail",
            RecoveryResolutionFailurePoint::BeforeIntentCommit,
        )
        .unwrap_err();
    let unchanged = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert!(unchanged.get("recovery_resolution_intent").is_none());
    assert_eq!(unchanged["recovery"]["state"], "unresolved");
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest_before);

    for (failure, finalized_before_response) in [
        (
            RecoveryResolutionFailurePoint::AfterIntentBeforeFilesystem,
            false,
        ),
        (
            RecoveryResolutionFailurePoint::AfterFilesystemBeforeFinalize,
            false,
        ),
        (RecoveryResolutionFailurePoint::BeforeFinalizeCommit, false),
        (
            RecoveryResolutionFailurePoint::AfterFinalizeBeforeResponse,
            true,
        ),
    ] {
        let request_name = format!("req_saga_{failure:?}");
        let (mut fixture, chat, run_id) = durable_provider_recovery_fixture(&request_name);
        let run = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        let recovery = run["recovery"].clone();
        let request = json!({
            "recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],
            "taskId":chat["taskId"],"projectId":chat["projectId"],
            "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
            "runId":run_id,"expectedRevision":1,"resolution":"abandon"
        });
        let mut runtime = AgentRuntime::blocked();
        runtime
            .resolve_recovery_with_failure(
                &mut fixture.storage,
                &mut fixture.projects,
                &request,
                &request_name,
                failure,
            )
            .unwrap_err();
        let interrupted = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        if finalized_before_response {
            assert_eq!(interrupted["record"]["state"], "canceled");
            assert!(interrupted.get("recovery_resolution_intent").is_none());
        } else {
            assert_eq!(interrupted["recovery"]["state"], "unresolved");
            assert!(interrupted["recovery_resolution_intent"].is_object());
        }
        runtime
            .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
            .unwrap();
        let finalized = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        assert_eq!(finalized["record"]["state"], "canceled");
        assert_eq!(finalized["recovery"]["state"], "resolvedAbandoned");
        assert!(finalized.get("recovery_resolution_intent").is_none());
        let lock = fixture
            .storage
            .projection_record(
                "lock",
                &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
            )
            .unwrap()
            .unwrap();
        assert_eq!(lock["writer_run_id"], Value::Null);
        assert_eq!(lock["poisoned"], false);
    }

    let (mut fixture, chat, run_id) = durable_provider_recovery_fixture("req_saga_mismatch");
    let recovery = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap()["recovery"]
        .clone();
    let request = json!({
        "recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],
        "taskId":chat["taskId"],"projectId":chat["projectId"],
        "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
        "runId":run_id,"expectedRevision":1,"resolution":"abandon"
    });
    let mut runtime = AgentRuntime::blocked();
    runtime
        .resolve_recovery_with_failure(
            &mut fixture.storage,
            &mut fixture.projects,
            &request,
            "req_saga_mismatch_intent",
            RecoveryResolutionFailurePoint::AfterIntentBeforeFilesystem,
        )
        .unwrap_err();
    let manifest_path = fixture
        ._root
        .path()
        .join("data/worktree-manifests-v1")
        .join(format!("{}.json", chat["worktreeId"].as_str().unwrap()));
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["baseCommit"] = Value::String("0".repeat(40));
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(runtime
        .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
        .unwrap_err()
        .detail()
        .contains("manifest differs"));
    let poisoned = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert_eq!(poisoned["recovery"]["state"], "unresolved");
    assert!(poisoned["recovery_resolution_intent"].is_object());
    let lock = fixture
        .storage
        .projection_record(
            "lock",
            &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(lock["writer_run_id"], run_id);
    assert_eq!(lock["poisoned"], true);
}

#[test]
fn agent_recovery_discovery_rejects_ambiguity_identity_and_cardinality_drift() {
    let (mut fixture, chat, run_id) = durable_provider_recovery_fixture("req_recovery_ambiguous");
    let mut duplicate = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    duplicate.remove("last_event_sequence");
    let duplicate_run_id = "agent_run_duplicate_recovery";
    duplicate["run_id"] = Value::String(duplicate_run_id.into());
    duplicate["record"]["runId"] = Value::String(duplicate_run_id.into());
    duplicate["recovery"]["runId"] = Value::String(duplicate_run_id.into());
    duplicate["recovery"]["recoveryId"] = Value::String("agent_recovery_duplicate_recovery".into());
    fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: duplicate_run_id.into(),
            operation_id: "op_duplicate_actual_recovery".into(),
            request_id: None,
            payload: duplicate,
        }])
        .unwrap();
    assert!(fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
            true,
        )
        .unwrap_err()
        .detail
        .contains("multiple unresolved"));

    let (mut fixture, chat, run_id) = durable_provider_recovery_fixture("req_recovery_identity");
    let mut mismatch = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    mismatch.remove("last_event_sequence");
    mismatch["recovery"]["workspaceIdentity"] = Value::String("workspace_mismatch".into());
    fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: "op_mismatched_actual_recovery".into(),
            request_id: None,
            payload: mismatch,
        }])
        .unwrap();
    assert!(fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
            true,
        )
        .unwrap_err()
        .detail
        .contains("identity differs"));

    let (mut fixture, chat, run_id) = durable_provider_recovery_fixture("req_recovery_cardinality");
    let mut missing = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    missing.remove("last_event_sequence");
    missing["recovery"] = Value::Null;
    fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id,
            operation_id: "op_missing_actual_recovery".into(),
            request_id: None,
            payload: missing,
        }])
        .unwrap();
    assert!(fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
            true,
        )
        .unwrap_err()
        .detail
        .contains("cardinality differ"));
}

#[test]
fn provider_terminal_failed_stays_non_poisoned_failed() {
    let mut fixture = Fixture::new();
    let (mut runtime, chat, run_id) = start_exact_actual_scenario(
        &mut fixture,
        FakeScenario::ProviderTerminalFailed,
        "req_provider_failed",
    );
    let wire = drain_actual_scenario(&mut fixture, &mut runtime, &run_id);
    assert_eq!(wire.last().unwrap().payload["run"]["state"], "failed");
    let run = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert_eq!(run["record"]["state"], "failed");
    assert_eq!(run["termination"]["outcome"], "cleanExit");
    assert_eq!(run["termination"]["poisoned"], false);
    assert_eq!(run["recovery"], Value::Null);
    let lock = fixture
        .storage
        .projection_record(
            "lock",
            &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(lock["writer_run_id"], Value::Null);
    assert_eq!(lock["poisoned"], false);
    assert!(fixture.storage.blocker_ids().unwrap().is_empty());
}

#[test]
fn poll_observe_and_wait_failures_propagate_after_fail_closed_recovery_persistence() {
    for scenario in [
        FakeScenario::PollObserveFailure,
        FakeScenario::PollWaitFailure,
    ] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_agent_poll_failure_snapshot");
        let chat = fixture.create_exact_claude_chat(
            &snapshot,
            "poll failure",
            "req_agent_poll_failure_chat",
        );
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let started = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_agent_poll_failure",
            )
            .unwrap();
        let run_id = started.result["runId"].as_str().unwrap();
        let error = runtime
            .poll(&mut fixture.storage, &mut fixture.projects)
            .expect_err("poll failure must reach the stdio owner");
        assert_eq!(error.kind(), "agentRuntimeFailure");
        let terminal = fixture
            .storage
            .projection_record("run", run_id)
            .unwrap()
            .unwrap();
        assert_eq!(terminal["record"]["state"], "recoveryRequired");
        assert_eq!(terminal["termination"]["processesProvenGone"], false);
        assert_eq!(terminal["recovery"]["state"], "unresolved");
    }
}

#[test]
fn corrupted_durable_counter_and_terminal_state_construction_errors_propagate() {
    for corrupt_counter in [true, false] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_agent_persist_failure_snapshot");
        let chat = fixture.create_exact_claude_chat(
            &snapshot,
            "persistence failure",
            "req_agent_persist_failure_chat",
        );
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::NoOutput);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let started = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_agent_persist_failure",
            )
            .unwrap();
        let run_id = started.result["runId"].as_str().unwrap().to_string();
        if corrupt_counter {
            let mut projection = fixture
                .storage
                .projection_record("run", &run_id)
                .unwrap()
                .unwrap();
            projection.insert("public_event_count".into(), Value::from(1));
            fixture
                .storage
                .upsert_projection_records_atomic(vec![ProjectionUpsert {
                    aggregate_type: "run",
                    aggregate_id: run_id.clone(),
                    operation_id: "op_test_agent_counter_corrupt".into(),
                    request_id: None,
                    payload: projection,
                }])
                .unwrap();
            assert!(runtime.validate_persisted_runs(&fixture.storage).is_err());
        } else {
            let mut projection = fixture
                .storage
                .projection_record("chat", chat_id)
                .unwrap()
                .unwrap();
            projection["record"]["chatId"] = Value::String(String::new());
            fixture
                .storage
                .upsert_projection_records_atomic(vec![ProjectionUpsert {
                    aggregate_type: "chat",
                    aggregate_id: chat_id.into(),
                    operation_id: "op_test_agent_chat_corrupt".into(),
                    request_id: None,
                    payload: projection,
                }])
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let error = runtime
            .poll(&mut fixture.storage, &mut fixture.projects)
            .expect_err("poll persistence/construction failure must propagate");
        assert_eq!(error.kind(), "agentRuntimeFailure");
        assert!(
            error.detail().contains("counter")
                || error.detail().contains("count does not match")
                || error.detail().contains("agent run record")
                || error.detail().contains("identity differs")
                || error.detail().contains("chatId"),
            "{}",
            error.detail()
        );
    }
}

#[test]
fn terminal_construction_and_upsert_failures_keep_active_until_restart_poison_recovery() {
    for scenario in [
        FakeScenario::TerminalConstructionFailure,
        FakeScenario::TerminalUpsertFailure,
    ] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_agent_terminal_failure_snapshot");
        let chat = fixture.create_exact_claude_chat(
            &snapshot,
            "terminal failure ownership",
            "req_agent_terminal_failure_chat",
        );
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let started = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_agent_terminal_failure",
            )
            .unwrap();
        let run_id = started.result["runId"].as_str().unwrap().to_string();
        let error = loop {
            match runtime.poll(&mut fixture.storage, &mut fixture.projects) {
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(error) => break error,
            }
        };
        assert_eq!(error.kind(), "agentRuntimeFailure");
        assert!(runtime.has_active_run(&run_id));
        let durable = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        assert_eq!(durable["record"]["state"], "running");
        assert_eq!(durable["termination"], Value::Null);
        let lock_id = format!("lock_{}", chat["worktreeId"].as_str().unwrap());
        let lock = fixture
            .storage
            .projection_record("lock", &lock_id)
            .unwrap()
            .unwrap();
        assert_eq!(lock["writer_run_id"], run_id);
        assert_eq!(lock["poisoned"], false);
        drop(runtime);

        AgentRuntime::blocked()
            .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
            .unwrap();
        let recovered = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        assert_eq!(recovered["record"]["state"], "recoveryRequired");
        assert_eq!(recovered["recovery"]["state"], "unresolved");
        let lock = fixture
            .storage
            .projection_record("lock", &lock_id)
            .unwrap()
            .unwrap();
        assert_eq!(lock["writer_run_id"], run_id);
        assert_eq!(lock["poisoned"], true);
        assert_eq!(lock["recovery_required"], true);
    }
}

#[test]
fn live_text_and_cumulative_counter_undercounts_block_append_and_recover_poisoned() {
    for counter in [
        "public_text_bytes",
        "public_ledger_cumulative_snapshot_bytes",
    ] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_agent_live_counter_snapshot");
        let chat = fixture.create_exact_claude_chat(
            &snapshot,
            "live counter drift",
            "req_agent_live_counter_chat",
        );
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::FinalDelayed);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let started = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_agent_live_counter",
            )
            .unwrap();
        let run_id = started.result["runId"].as_str().unwrap().to_string();
        let before = loop {
            let emitted = runtime
                .poll(&mut fixture.storage, &mut fixture.projects)
                .unwrap();
            let projection = fixture
                .storage
                .projection_record("run", &run_id)
                .unwrap()
                .unwrap();
            if projection["public_event_count"].as_u64().unwrap_or(0) > 0 {
                assert!(emitted
                    .iter()
                    .any(|event| event.event == "agent.publicEvent"));
                break projection;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let durable_event_count = before["public_event_count"].as_u64().unwrap();
        let mut drifted = before;
        let exact = drifted[counter].as_u64().unwrap();
        assert!(exact > 0);
        drifted.insert(counter.into(), Value::from(exact - 1));
        fixture
            .storage
            .upsert_projection_records_atomic(vec![ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run_id.clone(),
                operation_id: format!("op_test_agent_{counter}_drift"),
                request_id: None,
                payload: drifted,
            }])
            .unwrap();
        let error = runtime
            .poll(&mut fixture.storage, &mut fixture.projects)
            .expect_err("live durable counter drift must fail before append");
        assert_eq!(error.kind(), "agentRuntimeFailure");
        assert!(runtime.has_active_run(&run_id));
        let unchanged = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        assert_eq!(unchanged["public_event_count"], durable_event_count);
        drop(runtime);

        AgentRuntime::blocked()
            .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
            .unwrap();
        let recovered = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        assert_eq!(recovered["record"]["state"], "recoveryRequired");
        assert_eq!(recovered["recovery"]["state"], "unresolved");
        assert_eq!(recovered["public_event_count"], durable_event_count);
        AgentRuntime::blocked()
            .validate_persisted_runs(&fixture.storage)
            .unwrap();
        let lock = fixture
            .storage
            .projection_record(
                "lock",
                &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
            )
            .unwrap()
            .unwrap();
        assert_eq!(lock["poisoned"], true);
        assert_eq!(lock["recovery_required"], true);
    }
}

#[test]
fn fake_agent_cancel_uncertain_poison_survives_and_is_abandon_only() {
    let executable = sidecar_binary();
    assert!(
        executable.is_file(),
        "build the sidecar binary for process gate"
    );
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_agent_uncertain_snapshot");
    let chat =
        fixture.create_exact_claude_chat(&snapshot, "uncertain agent", "req_agent_uncertain_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let mut runtime = AgentRuntime::fake(executable, FakeScenario::CancelUncertain);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let started = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_agent_uncertain",
        )
        .unwrap();
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    let identity = RunIdentity {
        chat_id: chat_id.into(),
        task_id: chat["taskId"].as_str().unwrap().into(),
        project_id: chat["projectId"].as_str().unwrap().into(),
        worktree_id: chat["worktreeId"].as_str().unwrap().into(),
        workspace_identity: chat["workspaceIdentity"].as_str().unwrap().into(),
        run_id: run_id.clone(),
    };
    let canceled = runtime
        .cancel(&mut fixture.storage, &mut fixture.projects, &identity)
        .unwrap();
    assert_eq!(canceled.events.len(), 2);
    assert_eq!(canceled.events[0].event, "agent.runStateChanged");
    assert_eq!(canceled.events[0].payload["run"]["state"], "running");
    assert_eq!(canceled.events[1].event, "agent.runStateChanged");
    assert_eq!(
        canceled.events[1].payload["run"]["state"],
        "recoveryRequired"
    );
    assert_eq!(canceled.result["outcome"], "recoveryRequired");
    assert_eq!(
        canceled.result["termination"]["writerLockState"],
        "recoveryRequired"
    );
    let recovery = canceled.result["recovery"].clone();
    assert!(fixture
        .chats
        .list_recoveries(&fixture.storage, None, 10)
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(fixture
        .chats
        .inspect_recovery(&fixture.storage, recovery["recoveryId"].as_str().unwrap())
        .unwrap_err()
        .detail
        .contains("missing"));
    assert!(fixture
        .chats
        .mark_recovery_resolved(
            &mut fixture.storage,
            &mut fixture.projects,
            recovery["recoveryId"].as_str().unwrap(),
            1,
            "abandon",
            "req_legacy_cross_ledger",
        )
        .unwrap_err()
        .detail
        .contains("missing"));
    assert_eq!(
        fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap()["recovery"],
        recovery
    );
    let resolved = runtime.resolve_recovery(&mut fixture.storage,&mut fixture.projects,&json!({"recoveryId":recovery["recoveryId"],"chatId":identity.chat_id,"taskId":identity.task_id,"projectId":identity.project_id,"worktreeId":identity.worktree_id,"workspaceIdentity":identity.workspace_identity,"runId":identity.run_id,"expectedRevision":1,"resolution":"abandon"}),"req_agent_resolve").unwrap();
    assert_eq!(resolved["outcome"], "resolvedAbandoned");
    let lock = fixture
        .storage
        .projection_record(
            "lock",
            &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(lock["poisoned"], false);
    assert_eq!(lock["writer_run_id"], Value::Null);
}

#[test]
fn actual_recovery_api_rejects_legacy_fixture_ledger() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_legacy_recovery_snapshot");
    let chat = fixture.create_chat(&snapshot, "legacy recovery", "req_legacy_recovery_chat");
    let run_id = "agent_run_legacy_cross_ledger".to_string();
    let recovery = json!({"recoveryId":"agent_recovery_legacy_cross_ledger","chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"revision":1,"state":"unresolved"});
    fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: "op_legacy_cross_ledger".into(),
            request_id: None,
            payload: json!({"run_id":run_id,"chat_id":chat["chatId"],"state":"recoveryRequired","worktree_path":snapshot["worktreePath"],"record":{"runId":run_id},"recovery":recovery})
                .as_object()
                .unwrap()
                .clone(),
        }])
        .unwrap();
    let listed = fixture
        .chats
        .list_recoveries(&fixture.storage, None, 10)
        .unwrap();
    let recovery = listed["items"][0].clone();
    let request = json!({"recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id});
    let mut actual = AgentRuntime::blocked();
    assert!(actual
        .inspect_recovery(&fixture.storage, &request)
        .unwrap_err()
        .detail
        .contains("missing"));
    let mut resolve = request;
    resolve["expectedRevision"] = Value::from(1);
    resolve["resolution"] = Value::String("abandon".into());
    assert!(actual
        .resolve_recovery(
            &mut fixture.storage,
            &mut fixture.projects,
            &resolve,
            "req_actual_cross_ledger"
        )
        .unwrap_err()
        .detail
        .contains("missing"));
    assert_eq!(
        fixture
            .chats
            .inspect_recovery(&fixture.storage, recovery["recoveryId"].as_str().unwrap())
            .unwrap()["runId"],
        run_id
    );
}

#[test]
fn fake_agent_clean_cancel_releases_writer_and_malformed_or_oversize_fail_closed() {
    let executable = sidecar_binary();
    for scenario in [
        FakeScenario::CancelClean,
        FakeScenario::Malformed,
        FakeScenario::Oversized,
        FakeScenario::ProviderTerminalMalformed,
    ] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_agent_failure_snapshot");
        let chat = fixture.create_exact_claude_chat(
            &snapshot,
            "agent failure gate",
            "req_agent_failure_chat",
        );
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let (runtime_name, role_id) = ("claude-code", "coding");
        let mut runtime = AgentRuntime::fake(executable.clone(), scenario);
        let capability = runtime.get_capabilities(runtime_name, true).unwrap();
        let started = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: role_id.into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_agent_failure",
            )
            .unwrap();
        let run_id = started.result["runId"].as_str().unwrap().to_string();
        if scenario == FakeScenario::CancelClean {
            let identity = RunIdentity {
                chat_id: chat_id.into(),
                task_id: chat["taskId"].as_str().unwrap().into(),
                project_id: chat["projectId"].as_str().unwrap().into(),
                worktree_id: chat["worktreeId"].as_str().unwrap().into(),
                workspace_identity: chat["workspaceIdentity"].as_str().unwrap().into(),
                run_id: run_id.clone(),
            };
            let result = runtime
                .cancel(&mut fixture.storage, &mut fixture.projects, &identity)
                .unwrap();
            assert_eq!(result.result["outcome"], "canceled");
            assert_eq!(result.events.len(), 2);
            assert_eq!(result.events[0].payload["run"]["state"], "running");
            assert_eq!(result.events[1].payload["run"]["state"], "canceled");
            let lock = fixture
                .storage
                .projection_record(
                    "lock",
                    &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
                )
                .unwrap()
                .unwrap();
            assert_eq!(lock["writer_run_id"], Value::Null);
            assert_eq!(lock["poisoned"], false);
        } else {
            for _ in 0..100 {
                let _ = runtime
                    .poll(&mut fixture.storage, &mut fixture.projects)
                    .unwrap();
                if fixture
                    .storage
                    .projection_record("run", &run_id)
                    .unwrap()
                    .unwrap()["state"]
                    != "running"
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let terminal = fixture
                .storage
                .projection_record("run", &run_id)
                .unwrap()
                .unwrap();
            assert_eq!(terminal["record"]["state"], "failed");
            assert_eq!(terminal["termination"]["processesProvenGone"], true);
        }
    }
}

#[test]
fn control_launcher_child_has_no_descendant_before_release_and_tree_is_gone_afterward() {
    use hello_agent_core::proctree::{identity_of, ProbeResult, ProcTable, SystemProcTable};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    let mut launcher = Command::new(sidecar_binary())
        .args(["--agent-fixture-launcher", "clean"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let launcher_identity = identity_of(launcher.id()).unwrap();
    let mut reader = BufReader::new(launcher.stdout.take().unwrap());
    let mut ready = String::new();
    reader.read_line(&mut ready).unwrap();
    let child_pid: u32 = ready
        .trim()
        .strip_prefix("ready:")
        .unwrap()
        .parse()
        .unwrap();
    let child_identity = identity_of(child_pid).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        !SystemProcTable
            .list()
            .unwrap()
            .iter()
            .any(|entry| entry.ppid == child_pid),
        "waiting child must not create descendants before release"
    );
    launcher.stdin.take().unwrap().write_all(b"run\n").unwrap();
    let mut output = String::new();
    reader.read_to_string(&mut output).unwrap();
    assert!(launcher.wait().unwrap().success());
    assert!(output.contains("\"type\":\"final\""));
    assert!(matches!(
        SystemProcTable.probe(child_identity),
        ProbeResult::Gone
    ));
    assert!(matches!(
        SystemProcTable.probe(launcher_identity),
        ProbeResult::Gone
    ));
}

#[test]
fn dynamic_grandchild_is_durable_and_blocks_recovery_until_every_identity_is_gone() {
    use hello_agent_core::proctree::{
        ProbeResult, ProcIdentity, ProcTable, SystemProcTable, TreeSignal,
    };
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_descendant_snapshot");
    let chat =
        fixture.create_exact_claude_chat(&snapshot, "durable descendant", "req_descendant_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Descendant);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let started = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_descendant_start",
        )
        .unwrap();
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    let mut projection = Map::new();
    for _ in 0..100 {
        let _ = runtime
            .poll(&mut fixture.storage, &mut fixture.projects)
            .unwrap();
        projection = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        if projection["process_identity_internal"]
            .as_array()
            .is_some_and(|items| items.len() >= 3)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let internal = projection["process_identity_internal"]
        .as_array()
        .cloned()
        .unwrap();
    assert!(internal.len() >= 3, "grandchild identity must be durable");
    projection["state"] = Value::String("recoveryRequired".into());
    fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: "op_test_descendant_crash".into(),
            request_id: None,
            payload: projection,
        }])
        .unwrap();
    drop(runtime);
    let recovered = AgentRuntime::blocked();
    recovered
        .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
        .unwrap();
    let run = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    let recovery = run["recovery"].clone();
    assert!(recovery["trackedProcesses"].as_array().unwrap().len() >= 3);
    let request = json!({"recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"expectedRevision":1,"resolution":"abandon"});
    let mut recovered = recovered;
    assert_eq!(
        recovered
            .resolve_recovery(
                &mut fixture.storage,
                &mut fixture.projects,
                &request,
                "req_descendant_alive"
            )
            .unwrap()["outcome"],
        "processesRemain"
    );
    for value in &internal {
        let _ = SystemProcTable.signal(value["pid"].as_u64().unwrap() as u32, TreeSignal::Kill);
    }
    for _ in 0..100 {
        if internal.iter().all(|value| {
            matches!(
                SystemProcTable.probe(ProcIdentity {
                    pid: value["pid"].as_u64().unwrap() as u32,
                    start_us: value["start_us"].as_i64().unwrap(),
                }),
                ProbeResult::Gone
            )
        }) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        recovered
            .resolve_recovery(
                &mut fixture.storage,
                &mut fixture.projects,
                &request,
                "req_descendant_gone"
            )
            .unwrap()["outcome"],
        "resolvedAbandoned"
    );
}
