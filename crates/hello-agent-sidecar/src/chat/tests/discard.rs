use super::*;

fn ready(f: &mut Fixture) -> (Value, WorkspaceAddress, u64) {
    let snapshot = f.snapshot("req_discard_snapshot");
    let chat = f.create_chat(&snapshot, "discard test", "req_discard_chat");
    let chat = update_chat_state(&chat, "reviewReady").unwrap();
    persist_chat_state(&mut f.storage, &chat, "op_discard_ready").unwrap();
    let address = address_from_chat(&chat).unwrap();
    let preview = f
        .chats
        .preview_workspace_discard(
            &f.storage,
            &f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
        )
        .unwrap();
    (chat, address, preview["chatRevision"].as_u64().unwrap())
}

#[test]
fn review_discard_preserves_files_and_history_and_prevents_workspace_reuse() {
    let mut f = Fixture::new();
    let (chat, address, revision) = ready(&mut f);
    let before = f.projects.verify_workspace(&f.storage, &address).unwrap();
    let path = std::path::Path::new(before["worktreePath"].as_str().unwrap());
    fs::write(path.join("src/value.txt"), b"edited copy").unwrap();
    let history = f.storage.chat_message_history(&address.chat_id).unwrap();
    let result = f
        .chats
        .discard_workspace(
            &mut f.storage,
            &mut f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
            revision,
            "req_discard",
        )
        .unwrap();
    assert_eq!(result["state"], "discarded");
    assert_eq!(fs::read(f.source.join("src/value.txt")).unwrap(), b"base");
    assert_eq!(
        fs::read(path.join("src/value.txt")).unwrap(),
        b"edited copy"
    );
    assert!(f.projects.verify_workspace(&f.storage, &address).is_err());
    let after = f.storage.chat_message_history(&address.chat_id).unwrap();
    assert_eq!(&after[..history.len()], history.as_slice());
    assert_eq!(after.len(), history.len() + 1);
    assert!(f
        .chats
        .discard_workspace(
            &mut f.storage,
            &mut f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
            revision,
            "req_repeat"
        )
        .is_ok());
    assert_eq!(
        f.storage.chat_message_history(&address.chat_id).unwrap(),
        after
    );
}

#[test]
fn review_discard_wrong_task_and_stale_revision_do_not_change_workspace() {
    let mut f = Fixture::new();
    let (chat, address, revision) = ready(&mut f);
    let before = f.projects.verify_workspace(&f.storage, &address).unwrap();
    let stored = chat_projection_record(&f.storage, &address.chat_id).unwrap();
    for (task, expected) in [
        ("wrong-task", revision),
        (chat["taskId"].as_str().unwrap(), revision + 1),
    ] {
        assert!(f
            .chats
            .discard_workspace(
                &mut f.storage,
                &mut f.projects,
                &address,
                task,
                expected,
                "req_bad_discard"
            )
            .is_err());
        assert_eq!(
            f.projects.verify_workspace(&f.storage, &address).unwrap(),
            before
        );
        assert_eq!(
            chat_projection_record(&f.storage, &address.chat_id).unwrap(),
            stored
        );
    }
}

#[test]
fn review_discard_can_resume_the_terminal_manifest_before_db_commit() {
    let mut f = Fixture::new();
    let (chat, address, revision) = ready(&mut f);
    let before = f
        .projects
        .verify_discard_workspace(&f.storage, &address)
        .unwrap();
    // Model the interruption boundary before SQLite commits. Actual DB failure
    // injection and process restart remain integration checks, not claimed here.
    f.projects
        .terminalize_discard_workspace(&address, &before)
        .unwrap();
    assert!(f.projects.verify_workspace(&f.storage, &address).is_err());
    let terminal = f
        .projects
        .verify_discard_workspace(&f.storage, &address)
        .unwrap();
    let mut expected = before;
    expected["lifecycle"] = json!("abandoned");
    assert_eq!(terminal, expected);
    let result = f
        .chats
        .discard_workspace(
            &mut f.storage,
            &mut f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
            revision,
            "req_resume_discard",
        )
        .unwrap();
    assert_eq!(result["state"], "discarded");
}

#[test]
fn review_discard_sqlite_failure_preserves_history_and_retries_after_reopen() {
    let mut f = Fixture::new();
    let (chat, address, revision) = ready(&mut f);
    let stored = chat_projection_record(&f.storage, &address.chat_id).unwrap();
    let messages = f.storage.chat_message_history(&address.chat_id).unwrap();
    f.storage
        .connection_for_test()
        .execute_batch("CREATE TEMP TRIGGER discard_write_failure AFTER INSERT ON events WHEN NEW.operation_id LIKE 'op_chat_discard_%' BEGIN SELECT RAISE(ABORT, 'injected discard failure'); END;")
        .unwrap();
    assert!(f
        .chats
        .discard_workspace(
            &mut f.storage,
            &mut f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
            revision,
            "req_readonly_discard"
        )
        .is_err());
    assert_eq!(
        chat_projection_record(&f.storage, &address.chat_id).unwrap(),
        stored
    );
    assert_eq!(
        f.storage.chat_message_history(&address.chat_id).unwrap(),
        messages
    );
    assert!(f.projects.verify_workspace(&f.storage, &address).is_err());
    let database = f._root.path().join("data/hello-agent.sqlite3");
    drop(f.storage);
    f.storage = Storage::open(&database).unwrap();
    f.projects = ProjectManager::new(&database).unwrap();
    f.chats = G5Manager::default();
    let result = f
        .chats
        .discard_workspace(
            &mut f.storage,
            &mut f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
            revision,
            "req_reopen_discard",
        )
        .unwrap();
    assert_eq!(result["state"], "discarded");
    assert_eq!(
        f.storage
            .chat_message_history(&address.chat_id)
            .unwrap()
            .len(),
        messages.len() + 1
    );
    drop(f.storage);
    f.storage = Storage::open(&database).unwrap();
    let recovered = f
        .chats
        .preview_workspace_discard(
            &f.storage,
            &f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(recovered["chatRevision"], revision);
    assert!(f
        .chats
        .preview_workspace_discard(&f.storage, &f.projects, &address, "wrong_task")
        .is_err());
    assert_eq!(
        f.chats
            .discard_workspace(
                &mut f.storage,
                &mut f.projects,
                &address,
                chat["taskId"].as_str().unwrap(),
                revision,
                "req_lost_response"
            )
            .unwrap(),
        result
    );
    assert_eq!(
        f.storage
            .chat_message_history(&address.chat_id)
            .unwrap()
            .len(),
        messages.len() + 1
    );
    assert!(f
        .chats
        .discard_workspace(
            &mut f.storage,
            &mut f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
            revision + 1,
            "req_wrong_receipt"
        )
        .is_err());
    assert_eq!(fs::read(f.source.join("src/value.txt")).unwrap(), b"base");
}

#[test]
fn review_discard_keeps_fake_actual_terminal_evidence() {
    let mut f = Fixture::new();
    let (mut runtime, chat, run_id) =
        start_exact_actual_scenario(&mut f, FakeScenario::Clean, "req_discard_actual");
    let address = address_from_chat(&chat).unwrap();
    assert!(f
        .chats
        .preview_workspace_discard(
            &f.storage,
            &f.projects,
            &address,
            chat["taskId"].as_str().unwrap()
        )
        .is_err());
    drain_actual_scenario(&mut f, &mut runtime, &run_id);
    assert!(runtime.owned_run_ids().is_empty());
    let runs = f.storage.projection_records("run").unwrap();
    let inspection = f
        .chats
        .preview_workspace_discard(
            &f.storage,
            &f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
        )
        .unwrap();
    f.chats
        .discard_workspace(
            &mut f.storage,
            &mut f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
            inspection["chatRevision"].as_u64().unwrap(),
            "req_actual_discard",
        )
        .unwrap();
    assert_eq!(f.storage.projection_records("run").unwrap(), runs);
    assert_eq!(fs::read(f.source.join("src/value.txt")).unwrap(), b"base");
}

#[test]
fn review_discard_rejects_writer_poison_recovery_and_pending_save_without_mutation() {
    for blocked in ["writer", "poison", "recovery", "save"] {
        let mut f = Fixture::new();
        let (chat, address, revision, writer_id) = if blocked == "writer" {
            let (mut runtime, chat, run_id) =
                start_exact_actual_scenario(&mut f, FakeScenario::Clean, "req_writer_discard");
            drain_actual_scenario(&mut f, &mut runtime, &run_id);
            assert!(runtime.owned_run_ids().is_empty());
            let address = address_from_chat(&chat).unwrap();
            let inspection = f
                .chats
                .preview_workspace_discard(
                    &f.storage,
                    &f.projects,
                    &address,
                    chat["taskId"].as_str().unwrap(),
                )
                .unwrap();
            (
                chat,
                address,
                inspection["chatRevision"].as_u64().unwrap(),
                Some(run_id),
            )
        } else {
            let (chat, address, revision) = ready(&mut f);
            (chat, address, revision, None)
        };
        if blocked == "save" {
            f.projects
                .workspace_begin_save(
                    &mut f.storage,
                    &address,
                    "src/value.txt",
                    &sha256_file_version_v1(b"base"),
                    4,
                    "utf8",
                )
                .unwrap();
        } else {
            let lock_id = format!("lock_{}", address.worktree_id);
            let mut lock = f
                .storage
                .projection_record("lock", &lock_id)
                .unwrap()
                .unwrap();
            match blocked {
                "writer" => {
                    lock.insert("writer_run_id".into(), json!(writer_id.unwrap()));
                }
                "poison" => {
                    lock.insert("poisoned".into(), json!(true));
                }
                _ => {
                    lock.insert("recovery_required".into(), json!(true));
                }
            }
            f.storage
                .upsert_projection_record("lock", &lock_id, "op_discard_blocker", None, lock)
                .unwrap();
        }
        let manifest = f
            ._root
            .path()
            .join("data/worktree-manifests-v1")
            .join(format!("{}.json", address.worktree_id));
        let bytes = fs::read(&manifest).unwrap();
        let stored = chat_projection_record(&f.storage, &address.chat_id).unwrap();
        assert!(
            f.chats
                .discard_workspace(
                    &mut f.storage,
                    &mut f.projects,
                    &address,
                    chat["taskId"].as_str().unwrap(),
                    revision,
                    "req_blocked_discard"
                )
                .is_err(),
            "{blocked}"
        );
        assert_eq!(fs::read(&manifest).unwrap(), bytes);
        assert_eq!(
            chat_projection_record(&f.storage, &address.chat_id).unwrap(),
            stored
        );
    }
}

#[test]
fn review_discard_preserves_valid_synthetic_legacy_terminal_run() {
    let mut f = Fixture::new();
    let (chat, address, revision) = ready(&mut f);
    let manifest = f.projects.verify_workspace(&f.storage, &address).unwrap();
    let run = json!({"schemaVersion":1,"runId":"run_discard_legacy","chatId":address.chat_id,"taskId":chat["taskId"],
        "worktreeId":address.worktree_id,"workspaceIdentity":address.workspace_identity,
        "selectedProfile":chat["roleSnapshots"].as_array().unwrap().iter().find(|role| role["roleId"] == "coding").unwrap(),"fixtureScenario":"coding-success","state":"succeeded","stage":"implementing",
        "terminationEvidence":{"outcome":"cleanExit","processesProvenGone":true,"exitCode":0,"signal":null,"observedAt":now()},"createdAt":now(),"updatedAt":now()});
    validate_g5_payload_v1("runRecord", &run).unwrap();
    let projection = run_projection(&run, &manifest, None).unwrap();
    f.storage
        .upsert_projection_record(
            "run",
            "run_discard_legacy",
            "op_legacy_discard_seed",
            None,
            projection,
        )
        .unwrap();
    let runs = f.storage.projection_records("run").unwrap();
    f.chats
        .discard_workspace(
            &mut f.storage,
            &mut f.projects,
            &address,
            chat["taskId"].as_str().unwrap(),
            revision,
            "req_legacy_discard",
        )
        .unwrap();
    assert_eq!(f.storage.projection_records("run").unwrap(), runs);
}

#[test]
fn review_discard_dispatcher_rejects_owned_fake_actual_run_without_mutation() {
    let mut f = Fixture::new();
    let (mut runtime, chat, run_id) =
        start_exact_actual_scenario(&mut f, FakeScenario::Clean, "owned_discard");
    assert!(!runtime.owned_run_ids().is_empty());
    let ready = crate::SidecarSession::new("discard_owned".into(), AgentRuntime::blocked())
        .ready
        .clone();
    let mut owner = crate::Sidecar::initialize(
        f._root.path().join("other.sqlite3"),
        ready,
        AgentRuntime::blocked(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    std::mem::swap(&mut owner.storage, &mut f.storage);
    std::mem::swap(&mut owner.projects, &mut f.projects);
    std::mem::swap(&mut owner.agent_runtime, &mut runtime);
    let before = owner
        .storage
        .projection_record("chat", chat["chatId"].as_str().unwrap())
        .unwrap();
    let manifest = f
        ._root
        .path()
        .join("data/worktree-manifests-v1")
        .join(format!("{}.json", chat["worktreeId"].as_str().unwrap()));
    let bytes = fs::read(&manifest).unwrap();
    let mut outcomes = Vec::new();
    for action in ["inspectChatDiscard", "discardChatWorkspace"] {
        let request = hello_agent_protocol::RequestV1 {
            protocol_version: hello_agent_protocol::PROTOCOL_VERSION,
            kind: "request".into(),
            sidecar_instance_id: "discard_owned".into(),
            request_id: action.into(),
            method: "project.catalog".into(),
            params: json!({"action":action,"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"]}),
        };
        outcomes.push(owner.handle_request(request).unwrap().response.unwrap());
    }
    let after = owner
        .storage
        .projection_record("chat", chat["chatId"].as_str().unwrap())
        .unwrap();
    let after_bytes = fs::read(&manifest).unwrap();
    std::mem::swap(&mut owner.storage, &mut f.storage);
    std::mem::swap(&mut owner.projects, &mut f.projects);
    std::mem::swap(&mut owner.agent_runtime, &mut runtime);
    drain_actual_scenario(&mut f, &mut runtime, &run_id);
    assert!(runtime.owned_run_ids().is_empty());
    for response in outcomes {
        let error = response.error.unwrap();
        assert_eq!(error.code, "requestBlocked");
        assert!(error.detail.contains("idle native execution"));
    }
    assert_eq!(before, after);
    assert_eq!(bytes, after_bytes);
}
