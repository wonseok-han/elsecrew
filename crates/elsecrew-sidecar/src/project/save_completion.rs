//! Atomic lock/chat publication shared with portable database integration tests.
use super::*;
impl ProjectManager {
    pub(super) fn complete_save_session_atomic(
        &self,
        storage: &mut Storage,
        address: &WorkspaceAddress,
        lock_id: &str,
        save_id: &str,
        request_id: &str,
        payload: &Value,
    ) -> Result<(), ProjectFailure> {
        self.verify_durable_save_session(storage, address, save_id)?;
        let lock = storage
            .projection_record("lock", lock_id)?
            .ok_or_else(|| ProjectFailure::ambiguous("save lock is missing"))?;
        let mut chat = storage
            .projection_record("chat", address.chat_id.as_str())?
            .ok_or_else(|| ProjectFailure::ambiguous("save chat is missing"))?;
        let revision = |record: &Map<String, Value>| {
            record
                .get("last_event_sequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| ProjectFailure::ambiguous("save completion revision is missing"))
        };
        let lock_revision = revision(&lock)?;
        let chat_revision = revision(&chat)?;
        let sessions = chat
            .get_mut("save_sessions")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| ProjectFailure::ambiguous("save sessions are missing"))?;
        if sessions.remove(save_id).is_none() {
            return Err(ProjectFailure::ambiguous("save session disappeared"));
        }
        storage.upsert_projection_records_atomic_checked(
            vec![
                crate::storage::ProjectionUpsert {
                    aggregate_type: "lock",
                    aggregate_id: lock_id.to_string(),
                    operation_id: format!("op_save_commit_{save_id}"),
                    request_id: Some(request_id.to_string()),
                    payload: payload
                        .as_object()
                        .cloned()
                        .ok_or_else(|| ProjectFailure::manifest("save lock payload differs"))?,
                },
                crate::storage::ProjectionUpsert {
                    aggregate_type: "chat",
                    aggregate_id: address.chat_id.as_str().to_string(),
                    operation_id: format!("op_workspace_save_session_finish_{save_id}"),
                    request_id: None,
                    payload: chat,
                },
            ],
            &[
                ("lock", lock_id, lock_revision),
                ("chat", address.chat_id.as_str(), chat_revision),
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (
        tempfile::TempDir,
        Storage,
        ProjectManager,
        WorkspaceAddress,
        Value,
    ) {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join("save.sqlite3");
        let mut storage = Storage::open(&db).unwrap();
        let manager = ProjectManager::new(&db).unwrap();
        let address = WorkspaceAddress {
            chat_id: "chat_one".into(),
            project_id: "project_one".into(),
            worktree_id: "worktree_one".into(),
            workspace_identity: "workspace_one".into(),
        };
        let session = json!({"chatId":address.chat_id,"projectId":address.project_id,"worktreeId":address.worktree_id,"workspaceIdentity":address.workspace_identity,"saveId":"save_one"});
        let lock = json!({"lock_id":"lock_one","project_id":address.project_id,"writer_run_id":Value::Null,"worktree_path":"/fixture/worktree","poisoned":false,"recovery_required":true,"evidence":{"phase":"cleanupComplete"}});
        let chat = json!({"chat_id":address.chat_id,"project_id":address.project_id,"worktree_id":address.worktree_id,"workspace_identity":address.workspace_identity,"state":"draft","record":{},"last_message":Value::Null,"durable_event_sequence":0,"save_sessions":{"save_one":session,"save_other":{"preserve":true}},"unrelated":"keep"});
        for (kind, id, payload) in [
            (
                "project",
                "project_one",
                json!({"project_id":"project_one","canonical_path":"/fixture"}),
            ),
            ("lock", "lock_one", lock.clone()),
            ("chat", "chat_one", chat),
        ] {
            storage
                .upsert_projection_record(
                    kind,
                    id,
                    &format!("seed_{kind}"),
                    None,
                    payload.as_object().unwrap().clone(),
                )
                .unwrap();
        }
        let mut completed = lock;
        completed["recovery_required"] = json!(false);
        completed["evidence"] = json!({"phase":"saveCommitted"});
        (root, storage, manager, address, completed)
    }

    #[test]
    fn save_completion_commits_both_records_once_and_survives_reopen() {
        let (root, mut storage, manager, address, payload) = setup();
        manager
            .complete_save_session_atomic(
                &mut storage,
                &address,
                "lock_one",
                "save_one",
                "finish",
                &payload,
            )
            .unwrap();
        let lock = storage
            .projection_record("lock", "lock_one")
            .unwrap()
            .unwrap();
        let chat = storage
            .projection_record("chat", "chat_one")
            .unwrap()
            .unwrap();
        assert_eq!(lock["recovery_required"], false);
        assert!(chat["save_sessions"].get("save_one").is_none());
        assert_eq!(chat["save_sessions"]["save_other"]["preserve"], true);
        assert_eq!(chat["unrelated"], "keep");
        assert!(manager
            .complete_save_session_atomic(
                &mut storage,
                &address,
                "lock_one",
                "save_one",
                "repeat",
                &payload
            )
            .is_err());
        assert_eq!(
            storage
                .projection_record("lock", "lock_one")
                .unwrap()
                .unwrap(),
            lock
        );
        assert_eq!(
            storage
                .projection_record("chat", "chat_one")
                .unwrap()
                .unwrap(),
            chat
        );
        drop(storage);
        let reopened = Storage::open(&root.path().join("save.sqlite3")).unwrap();
        assert_eq!(
            reopened
                .projection_record("lock", "lock_one")
                .unwrap()
                .unwrap(),
            lock
        );
        assert_eq!(
            reopened
                .projection_record("chat", "chat_one")
                .unwrap()
                .unwrap(),
            chat
        );
    }

    #[test]
    fn save_completion_second_write_conflict_rolls_back_lock_clear() {
        let (_root, mut storage, manager, address, payload) = setup();
        let chat = storage
            .projection_record("chat", "chat_one")
            .unwrap()
            .unwrap();
        // Reserve the second operation with different content. The first lock
        // event is attempted inside the transaction before this conflict occurs.
        storage
            .upsert_projection_record(
                "chat",
                "chat_one",
                "op_workspace_save_session_finish_save_one",
                None,
                chat,
            )
            .unwrap();
        let before_lock = storage.projection_record("lock", "lock_one").unwrap();
        let before_chat = storage.projection_record("chat", "chat_one").unwrap();
        assert!(manager
            .complete_save_session_atomic(
                &mut storage,
                &address,
                "lock_one",
                "save_one",
                "finish",
                &payload
            )
            .is_err());
        assert_eq!(
            storage.projection_record("lock", "lock_one").unwrap(),
            before_lock
        );
        assert_eq!(
            storage.projection_record("chat", "chat_one").unwrap(),
            before_chat
        );
    }

    #[test]
    fn save_completion_refuses_session_from_another_workspace() {
        let (_root, mut storage, manager, mut address, payload) = setup();
        let before_lock = storage.projection_record("lock", "lock_one").unwrap();
        let before_chat = storage.projection_record("chat", "chat_one").unwrap();
        address.workspace_identity = "workspace_other".into();
        assert!(manager
            .complete_save_session_atomic(
                &mut storage,
                &address,
                "lock_one",
                "save_one",
                "finish",
                &payload
            )
            .is_err());
        assert_eq!(
            storage.projection_record("lock", "lock_one").unwrap(),
            before_lock
        );
        assert_eq!(
            storage.projection_record("chat", "chat_one").unwrap(),
            before_chat
        );
    }
}
