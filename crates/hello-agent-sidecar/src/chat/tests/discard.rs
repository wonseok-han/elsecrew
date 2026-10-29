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
        .is_err());
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
