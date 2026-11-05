//! Native recovery integration over real files/SQLite. Snapshot preparation is
//! seeded, not a Git admission test; save guard/source/rename use product code.
use super::*;
use cap_std::fs::OpenOptionsExt as _;

const SAVE: &str = "save_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SOURCE: &str = ".hello-agent-save-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.tmp";

struct Fixture {
    root: tempfile::TempDir,
    db: PathBuf,
    workspace: PathBuf,
    original: PathBuf,
    address: WorkspaceAddress,
}
impl Fixture {
    fn interrupted(replace: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let original = root.path().join("original");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&original).unwrap();
        fs::write(original.join("file.txt"), b"original").unwrap();
        let db = data.join("save.sqlite3");
        let mut storage = Storage::open(&db).unwrap();
        let manager = ProjectManager::new(&db).unwrap();
        let address = WorkspaceAddress {
            chat_id: "chat_native".into(),
            project_id: "project_native".into(),
            worktree_id: "worktree_native".into(),
            workspace_identity: "workspace_native".into(),
        };
        let workspace = manager.worktrees_root.join(&address.worktree_id);
        fs::create_dir_all(workspace.join("nested")).unwrap();
        fs::create_dir_all(workspace.join(".git")).unwrap();
        fs::write(workspace.join("nested/file.txt"), b"original").unwrap();
        let active = ActiveProject {
            project_id: address.project_id.clone(),
            canonical_git_root: fs::canonicalize(&original).unwrap(),
            worktree_id: address.worktree_id.clone(),
            worktree_path: workspace.clone(),
            base_commit: "a".repeat(40),
            base_tree: "b".repeat(40),
            object_format: "sha1".into(),
            source_identity: "source_native".into(),
            workspace_identity: address.workspace_identity.clone(),
            manifest_path: manager
                .manifests_root
                .join(format!("{}.json", address.worktree_id)),
            lock_id: format!("lock_{}", address.worktree_id),
        };
        let manifest = manifest_value(&active, "active").unwrap();
        validate_project_payload_v1("worktreeManifest", &manifest).unwrap();
        fs::write(
            &active.manifest_path,
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            workspace.join(".git").join(SNAPSHOT_MARKER),
            serde_json::to_vec(&snapshot_identity(&active).unwrap()).unwrap(),
        )
        .unwrap();
        let original_path = canonical_protocol_path(&original).unwrap();
        let expected = sha256_file_version_v1(b"original");
        let session = json!({"chatId":address.chat_id,"projectId":address.project_id,"worktreeId":address.worktree_id,"workspaceIdentity":address.workspace_identity,"saveId":SAVE,"path":"nested/file.txt","expectedVersion":expected});
        let records = [
            (
                "project",
                address.project_id.as_str(),
                json!({"project_id":address.project_id,"canonical_path":original_path,"native_path_binding":path_binding::value(&original_path)}),
            ),
            (
                "lock",
                active.lock_id.as_str(),
                json!({"lock_id":active.lock_id,"project_id":address.project_id,"writer_run_id":Value::Null,"worktree_path":canonical_protocol_path(&workspace).unwrap(),"poisoned":false,"recovery_required":false,"evidence":{}}),
            ),
            (
                "chat",
                address.chat_id.as_str(),
                json!({"chat_id":address.chat_id,"project_id":address.project_id,"worktree_id":address.worktree_id,"workspace_identity":address.workspace_identity,"state":"draft","record":{},"last_message":Value::Null,"durable_event_sequence":0,"save_sessions":{(SAVE):session}}),
            ),
        ];
        for (kind, id, payload) in records {
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
        let staging_name = format!("{SAVE}.stage");
        let mut options = CapOpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(3)
            .follow(FollowSymlinks::No);
        let mut staged = manager
            .save_staging_dir
            .open_with(&staging_name, &options)
            .unwrap();
        staged.write_all(b"replacement").unwrap();
        staged.sync_all().unwrap();
        let mut pending = PendingSave {
            recovery_fenced: false,
            chat_id: Some(address.chat_id.clone()),
            worktree_id: address.worktree_id.clone(),
            workspace_identity: address.workspace_identity.clone(),
            project_id: address.project_id.clone(),
            relative_path: "nested/file.txt".into(),
            expected_version: expected.clone(),
            total_bytes: 11,
            received_bytes: 11,
            last_seq: 1,
            staging_name,
            file: Some(staged),
        };
        let worktree = manager.worktree_capability(&active).unwrap();
        let (parent, target) = open_parent_capability(&worktree, "nested/file.txt").unwrap();
        let guard = manager
            .prepare_windows_save_guard(
                &mut storage,
                &active,
                &mut pending,
                SAVE,
                "prepare",
                &worktree,
                &parent,
                &target,
                SOURCE,
            )
            .unwrap();
        let mut source = worktree
            .open_with(SOURCE, &windows_replace::source_options())
            .unwrap();
        source.write_all(b"replacement").unwrap();
        source.sync_all().unwrap();
        let mut prepared = PreparedTarget {
            data_root: manager.data_dir.try_clone().unwrap(),
            temp_parent: worktree.try_clone().unwrap(),
            temp_name: SOURCE.into(),
            temp_relative: PathBuf::from(WORKTREE_DIRECTORY)
                .join(&address.worktree_id)
                .join(SOURCE),
            target_relative: PathBuf::from(WORKTREE_DIRECTORY)
                .join(&address.worktree_id)
                .join("nested/file.txt"),
            file: None,
            renamed: false,
        };
        manager
            .persist_windows_save_source(
                &mut storage,
                &active,
                SAVE,
                "source",
                &guard,
                &prepared,
                &source,
                &sha256_file_version_v1(b"replacement"),
            )
            .unwrap();
        prepared.file = Some(source);
        if replace {
            manager
                .replace_windows_save(
                    &mut storage,
                    &active,
                    SAVE,
                    "replace",
                    &guard,
                    &mut prepared,
                    &expected,
                    None,
                    None,
                )
                .unwrap();
        }
        // Close every owning handle and database before a fresh manager reopens.
        drop((prepared, pending, guard, parent, worktree, manager, storage));
        Self {
            root,
            db,
            workspace,
            original,
            address,
        }
    }

    fn recover(&self, abort: bool) {
        let mut storage = Storage::open(&self.db).unwrap();
        let manager = ProjectManager::new(&self.db).unwrap();
        let inspection = manager
            .inspect_windows_save_recovery(&storage, &self.address, SAVE)
            .unwrap();
        let revision = inspection["lockRevision"].as_u64().unwrap();
        let lock_id = format!("lock_{}", self.address.worktree_id);
        let before_lock = storage.projection_record("lock", &lock_id).unwrap();
        let before_chat = storage
            .projection_record("chat", &self.address.chat_id)
            .unwrap();
        let guard_name = before_lock.as_ref().unwrap()["evidence"]["windowsSaveGuard"]["guardName"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(manager
            .resume_windows_save_cleanup(&mut storage, &self.address, SAVE, revision + 1, "stale")
            .is_err());
        assert_eq!(
            storage.projection_record("lock", &lock_id).unwrap(),
            before_lock
        );
        assert_eq!(
            storage
                .projection_record("chat", &self.address.chat_id)
                .unwrap(),
            before_chat
        );
        let outcome = if abort {
            manager
                .abort_windows_save(&mut storage, &self.address, SAVE, revision, "recover")
                .unwrap()
        } else {
            manager
                .resume_windows_save_cleanup(&mut storage, &self.address, SAVE, revision, "recover")
                .unwrap()
        };
        assert_eq!(
            outcome.result["outcome"],
            if abort { "aborted" } else { "committed" }
        );
        assert_eq!(
            fs::read(self.workspace.join("nested/file.txt")).unwrap(),
            if abort {
                b"original".as_slice()
            } else {
                b"replacement".as_slice()
            }
        );
        assert_eq!(
            fs::read(self.original.join("file.txt")).unwrap(),
            b"original"
        );
        assert!(!self.workspace.join(SOURCE).exists());
        assert!(!self.workspace.join("nested").join(guard_name).exists());
        assert!(!manager
            .save_staging_root
            .join(format!("{SAVE}.stage"))
            .exists());
        assert_eq!(
            manager
                .describe_windows_save_recovery(&storage, &self.address)
                .unwrap()
                .result["recovery"],
            Value::Null
        );
        assert!(manager
            .abort_windows_save(&mut storage, &self.address, SAVE, revision, "repeat")
            .is_err());
        drop((manager, storage));
        let reopened = Storage::open(&self.db).unwrap();
        assert_eq!(
            reopened
                .projection_record("lock", &format!("lock_{}", self.address.worktree_id))
                .unwrap()
                .unwrap()["recovery_required"],
            false
        );
        assert!(reopened
            .projection_record("chat", &self.address.chat_id)
            .unwrap()
            .unwrap()["save_sessions"]
            .get(SAVE)
            .is_none());
        assert!(self.root.path().exists());
    }
}

#[test]
fn interrupted_replaced_save_completes_after_database_and_handles_reopen() {
    Fixture::interrupted(true).recover(false);
}
#[test]
fn interrupted_prepared_save_aborts_after_database_and_handles_reopen() {
    Fixture::interrupted(false).recover(true);
}
