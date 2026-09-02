use super::reopen::{FrozenOperationVerificationSession, FrozenOperationVerificationStep};
use super::*;
use hello_agent_protocol::project_v1::sha256_file_version_v1;
use hello_agent_protocol::storage_lifetime_v1::{
    canonical_frozen_operation_record_v1, FrozenOperationSourceV1,
};
use hello_agent_protocol::storage_v1::canonical_json_v1;
use rusqlite::types::Value as SqlValue;
use rusqlite::Connection;
use tempfile::TempDir;

mod reopen_closure;

type DatabaseSnapshot = (Vec<SchemaObject>, Vec<(String, Vec<Vec<SqlValue>>)>);
type ChatSourceIndexRow = (i64, Option<String>, Option<i64>, Option<String>);

fn database() -> (TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("temporary directory");
    let path = root.path().join("hello-agent.sqlite3");
    (root, path)
}

fn event(
    event_id: &str,
    operation_id: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    sequence: u64,
    payload: Value,
    timestamp: &str,
) -> LedgerEventV1 {
    LedgerEventV1 {
        event_id: event_id.to_string(),
        operation_id: operation_id.to_string(),
        request_id: None,
        aggregate_type: aggregate_type.to_string(),
        aggregate_id: aggregate_id.to_string(),
        aggregate_sequence: sequence,
        event_type: format!("{aggregate_type}.upserted"),
        payload_schema_version: SQLITE_SCHEMA_VERSION,
        canonical_json_payload: canonical_json_v1(payload).expect("canonical payload"),
        timestamp: timestamp.to_string(),
    }
}

fn team_message(chat_id: &str, message_id: &str, sequence: u64) -> Value {
    json!({
        "schemaVersion":1,
        "messageId":message_id,
        "chatId":chat_id,
        "taskId":"task_v6",
        "worktreeId":"worktree_v6",
        "eventSequence":sequence,
        "kind":"user",
        "roleId":null,
        "runId":null,
        "text":"message",
        "createdAt":"2026-08-28T13:20:00Z",
    })
}

fn v6_chat_payload(
    chat_id: &str,
    project_id: &str,
    durable_sequence: u64,
    last_message: Value,
) -> Value {
    json!({
        "chat_id":chat_id,
        "project_id":project_id,
        "worktree_id":"worktree_v6",
        "workspace_identity":"workspace_v6",
        "state":"draft",
        "record":{},
        "last_message":last_message,
        "durable_event_sequence":durable_sequence,
    })
}

fn materializable_chat_payload(
    chat_id: &str,
    project_id: &str,
    durable_sequence: u64,
    last_message: Value,
) -> Value {
    json!({
        "chat_id":chat_id,
        "project_id":project_id,
        "worktree_id":"worktree_v6",
        "workspace_identity":"workspace_v6",
        "state":"draft",
        "record":{"chatId":chat_id,"taskId":"task_v6","worktreeId":"worktree_v6"},
        "last_message":last_message,
        "durable_event_sequence":durable_sequence,
    })
}

fn ready(id: &str) -> ReadyPayloadV1 {
    ReadyPayloadV1 {
        protocol_version: PROTOCOL_VERSION,
        sidecar_schema_version: 1,
        app_schema_version: APP_SCHEMA_VERSION,
        pid: 4242,
        platform: "darwin".to_string(),
        capabilities: vec!["system.status".to_string(), "app.prepareQuit".to_string()],
        sidecar_instance_id: id.to_string(),
    }
}

#[test]
fn migration_failure_rolls_back_every_table_and_can_retry() {
    let (_root, path) = database();
    let mut connection = Connection::open(&path).expect("open database");
    configure_connection(&connection).expect("configure connection");
    let invalid = MIGRATION_V1.replacen("COMMIT;", "INVALID SQL;\nCOMMIT;", 1);

    let error = migrate(&mut connection, &invalid).expect_err("migration must fail");
    assert_eq!(error.kind(), "migrationFailed");
    assert!(connection.is_autocommit());
    let tables: u32 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%';",
            [],
            |row| row.get(0),
        )
        .expect("count tables");
    assert_eq!(tables, 0);

    migrate(&mut connection, MIGRATION_V1).expect("retry migration");
    verify_schema(&connection).expect("schema ready");
}

#[test]
fn fresh_database_applies_exact_v8_and_reopens_idempotently() {
    let (_root, path) = database();
    let storage = Storage::open(&path).expect("fresh storage");
    verify_schema(&storage.connection).expect("v1 schema");
    verify_schema_v8_for_open(&storage.connection).expect("v8 schema");
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT schema_version FROM schema_meta WHERE singleton=1;",
                [],
                |row| row.get::<_, u32>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT storage_schema_version, materialization_state FROM storage_schema_meta WHERE singleton=1;",
                [],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap(),
        (STORAGE_LIFETIME_SCHEMA_VERSION_V8, "pending".to_string())
    );
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT count(*) FROM lifetime_materialization_job;",
                [],
                |row| row.get::<_, u32>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM event_source_index;", [], |row| {
                row.get::<_, u32>(0)
            })
            .unwrap(),
        0,
        "fresh migration must not backfill event history"
    );
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT \
                   (SELECT count(*) FROM ledger_compaction_prefixes) + \
                   (SELECT count(*) FROM frozen_message_sources) + \
                   (SELECT count(*) FROM ledger_compaction_delete_authority) + \
                   (SELECT count(*) FROM frozen_operation_sources);",
                [],
                |row| row.get::<_, u32>(0),
            )
            .unwrap(),
        0,
        "fresh migration must not invent compaction provenance or authority"
    );
    drop(storage);

    let reopened = Storage::open(&path).expect("idempotent reopen");
    verify_schema_v8_for_open(&reopened.connection).expect("reopened v8 schema");
}

#[test]
fn populated_v1_migrates_without_changing_rows_triggers_or_materializing() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    storage
        .append_event(&event(
            "evt_project_v1",
            "op_project_v1",
            "project",
            "project_v1",
            1,
            json!({"canonical_path":"/fixture/project","project_id":"project_v1"}),
            "2026-08-28T02:00:00Z",
        ))
        .unwrap();
    storage
        .register_app_instance(&ready("sidecar_v1"))
        .expect("app instance");
    drop_lifetime_schema(&storage.connection);
    assert_eq!(
        schema_objects(&storage.connection).unwrap(),
        reference_schema_v1().unwrap(),
        "test fixture must restore exact v1 schema"
    );
    let tables = [
        "schema_meta",
        "app_instances",
        "events",
        "projects",
        "chats",
        "runs",
        "locks",
    ];
    let before = tables
        .iter()
        .map(|table| (*table, table_rows(&storage.connection, table)))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert!(storage
        .connection
        .execute("DELETE FROM events;", [])
        .is_err());
    drop(storage);

    let mut storage = Storage::open(&path).expect("migrate populated v1");
    for table in tables {
        assert_eq!(
            table_rows(&storage.connection, table),
            before[table],
            "{table}"
        );
    }
    assert!(storage
        .connection
        .execute("UPDATE events SET request_id='tamper';", [])
        .is_err());
    assert_eq!(
        storage
            .append_event(&event(
                "evt_project_v1_second",
                "op_project_v1_second",
                "project",
                "project_v1",
                2,
                json!({"canonical_path":"/fixture/project","project_id":"project_v1"}),
                "2026-08-28T02:00:02Z",
            ))
            .unwrap(),
        AppendOutcome::Committed
    );
    let empty_lifetime: u32 = storage
        .connection
        .query_row(
            "SELECT (SELECT count(*) FROM ledger_baselines) + \
                    (SELECT count(*) FROM operation_receipts) + \
                    (SELECT count(*) FROM chat_messages) + \
                    (SELECT count(*) FROM prepared_workspaces) + \
                    (SELECT count(*) FROM maintenance_jobs) + \
                    (SELECT count(*) FROM lifetime_materialization_job);",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        empty_lifetime, 0,
        "startup must not backfill, create a job, or materialize"
    );
}

#[test]
fn v2_migration_failure_rolls_back_and_partial_or_tampered_schema_fails_closed() {
    let (_root, path) = database();
    let mut connection = Connection::open(&path).unwrap();
    configure_connection(&connection).unwrap();
    migrate(&mut connection, MIGRATION_V1).unwrap();
    let invalid = MIGRATION_V2.replacen("COMMIT;", "INVALID SQL;\nCOMMIT;", 1);
    let error = execute_migration(&mut connection, &invalid, "v2")
        .expect_err("invalid v2 migration must fail");
    assert_eq!(error.kind(), "migrationFailed");
    let v2_objects: u32 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('storage_schema_meta','ledger_baselines','operation_receipts','chat_messages','prepared_workspaces','maintenance_jobs');",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(v2_objects, 0);
    drop(connection);

    let storage = Storage::open(&path).expect("retry valid migration");
    storage
        .connection
        .execute_batch("DROP TABLE storage_schema_meta;")
        .unwrap();
    drop(storage);
    let error = Storage::open(&path).err().expect("partial v2 must fail");
    assert_eq!(error.kind(), "storageSchemaMismatch");

    let (_root, path) = database();
    let mut connection = Connection::open(&path).unwrap();
    configure_connection(&connection).unwrap();
    migrate(&mut connection, MIGRATION_V1).unwrap();
    connection
        .execute("CREATE TABLE unexpected_partial_v2(value TEXT);", [])
        .unwrap();
    drop(connection);
    let error = Storage::open(&path)
        .err()
        .expect("unexpected base object must fail before v2 mutation");
    assert_eq!(error.kind(), "storageSchemaMismatch");
    let connection = Connection::open(&path).unwrap();
    let storage_meta_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='storage_schema_meta');",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!storage_meta_exists);

    let (_root, path) = database();
    let storage = Storage::open(&path).expect("fresh storage");
    storage
        .connection
        .execute("DROP INDEX maintenance_jobs_status_created;", [])
        .unwrap();
    drop(storage);
    let error = Storage::open(&path).err().expect("tampered v2 must fail");
    assert_eq!(error.kind(), "storageSchemaMismatch");

    let (_root, path) = database();
    let storage = Storage::open(&path).expect("fresh storage");
    storage
        .connection
        .execute(
            "UPDATE storage_schema_meta SET migrated_at='not-a-timestamp' WHERE singleton=1;",
            [],
        )
        .unwrap();
    drop(storage);
    let error = Storage::open(&path)
        .err()
        .expect("tampered v2 metadata must fail");
    assert_eq!(error.kind(), "storageSchemaMismatch");
}

#[test]
fn populated_v2_pending_and_ready_upgrade_preserves_all_rows_guards_and_status() {
    for state in ["pending", "ready"] {
        let (_root, path) = database();
        let storage = populated_v2_storage(&path, state);
        let preserved_tables = [
            "schema_meta",
            "app_instances",
            "events",
            "projects",
            "chats",
            "runs",
            "locks",
            "ledger_baselines",
            "operation_receipts",
            "chat_messages",
            "prepared_workspaces",
            "maintenance_jobs",
        ];
        let rows_before = preserved_tables
            .iter()
            .map(|table| (*table, table_rows(&storage.connection, table)))
            .collect::<std::collections::BTreeMap<_, _>>();
        let guarded_schema_before = guarded_schema_objects(&storage.connection);
        let compaction_before = storage.lifetime_status().unwrap().compaction;
        drop(storage);

        let mut upgraded = Storage::open(&path).expect("upgrade exact populated v2");
        verify_schema_v8_for_open(&upgraded.connection).expect("exact v8");
        assert_eq!(
            upgraded
                .connection
                .query_row(
                    "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "pending"
        );
        assert_eq!(
            upgraded
                .connection
                .query_row(
                    "SELECT count(*) FROM lifetime_materialization_job;",
                    [],
                    |row| row.get::<_, u32>(0),
                )
                .unwrap(),
            0,
            "migration must not create a materialization job"
        );
        for table in preserved_tables {
            assert_eq!(
                table_rows(&upgraded.connection, table),
                rows_before[table],
                "{state}: {table}"
            );
        }
        assert_eq!(
            guarded_schema_objects(&upgraded.connection),
            guarded_schema_before
        );
        assert_eq!(
            upgraded.lifetime_status().unwrap().compaction,
            compaction_before
        );
        if state == "pending" {
            upgraded
                .connection
                .execute_batch(
                    "INSERT INTO lifetime_materialization_job(\
                       singleton,job_id,state,cutoff_event_rowid,phase,after_scan_rowid,scan_exhausted,\
                       failure_kind,created_at,updated_at,completed_at\
                     ) VALUES (1,'lifetime_status_isolation','pending',2,'receipts',0,0,NULL,\
                       '2026-08-28T06:03:00Z','2026-08-28T06:03:00Z',NULL);",
                )
                .unwrap();
            assert_eq!(
                upgraded.lifetime_status().unwrap().compaction,
                compaction_before,
                "dedicated materialization state must not affect compaction status"
            );
            upgraded
                .connection
                .execute_batch("DELETE FROM lifetime_materialization_job;")
                .unwrap();
        }
        assert!(upgraded
            .connection
            .execute("DELETE FROM events;", [])
            .is_err());
        assert_eq!(
            upgraded
                .append_event(&event(
                    "evt_project_v2_second",
                    &format!("op_project_v2_second_{state}"),
                    "project",
                    "project_v2",
                    2,
                    json!({"canonical_path":"/fixture/project-v2","project_id":"project_v2","revision":2}),
                    "2026-08-28T06:10:00Z",
                ))
                .unwrap(),
            AppendOutcome::Committed
        );
    }
}

#[test]
fn unsafe_v2_materialization_states_fail_before_schema_or_data_mutation() {
    for state in ["materializing", "failed"] {
        let (_root, path) = database();
        let storage = populated_v2_storage(&path, state);
        let snapshot_before = database_snapshot(&storage.connection);
        drop(storage);
        let bytes_before = fs::read(&path).unwrap();

        let error = Storage::open(&path)
            .err()
            .expect("unsafe v2 state must fail closed");
        assert_eq!(error.kind(), "storageSchemaMismatch");

        let reopened = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&reopened), snapshot_before, "{state}");
        drop(reopened);
        assert_eq!(fs::read(&path).unwrap(), bytes_before, "{state}");
    }
}

#[test]
fn current_schema_partial_extra_and_metadata_table_version_mismatch_fail_closed() {
    let (_root, partial_path) = database();
    let partial = Storage::open(&partial_path).unwrap();
    partial
        .connection
        .execute_batch("DROP TABLE lifetime_materialization_job;")
        .unwrap();
    drop(partial);
    let error = Storage::open(&partial_path)
        .err()
        .expect("partial v5 must fail");
    assert_eq!(error.kind(), "storageSchemaMismatch");

    let (_root, extra_path) = database();
    let extra = Storage::open(&extra_path).unwrap();
    extra
        .connection
        .execute_batch("CREATE TABLE unexpected_v3(value TEXT) STRICT;")
        .unwrap();
    drop(extra);
    let error = Storage::open(&extra_path)
        .err()
        .expect("extra v5 must fail");
    assert_eq!(error.kind(), "storageSchemaMismatch");

    let (_root, mismatch_path) = database();
    let mismatch = populated_v2_storage(&mismatch_path, "pending");
    mismatch
        .connection
        .execute_batch(v3_job_table_statement())
        .unwrap();
    drop(mismatch);
    let error = Storage::open(&mismatch_path)
        .err()
        .expect("v3 job table with v2 metadata must fail");
    assert_eq!(error.kind(), "storageSchemaMismatch");
}

#[test]
fn partial_or_extra_v3_fails_before_v4_migration() {
    for mutation in [
        "DROP INDEX maintenance_jobs_status_created;",
        "CREATE TABLE unexpected_v3(value TEXT) STRICT;",
    ] {
        let (_root, path) = database();
        let storage = Storage::open_v3_for_test(&path).expect("exact v3");
        storage.connection.execute_batch(mutation).unwrap();
        let snapshot_before = database_snapshot(&storage.connection);
        drop(storage);

        let error = Storage::open(&path)
            .err()
            .expect("non-exact v3 must fail before v4");
        assert_eq!(error.kind(), "storageSchemaMismatch");
        let reopened = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&reopened), snapshot_before);
        assert_eq!(
            reopened
                .query_row(
                    "SELECT storage_schema_version FROM storage_schema_meta WHERE singleton=1;",
                    [],
                    |row| row.get::<_, u32>(0),
                )
                .unwrap(),
            STORAGE_LIFETIME_SCHEMA_VERSION_V3
        );
    }
}

#[test]
fn unsafe_v3_pairs_fail_before_v4_schema_or_data_mutation() {
    let cases = [
        ("ready_pending", "ready", Some("pending"), None, None, false),
        (
            "pending_bad_created",
            "pending",
            Some("pending"),
            None,
            None,
            false,
        ),
        (
            "failed_bad_completed",
            "pending",
            Some("failed"),
            Some("database"),
            Some("not-a-timestamp"),
            false,
        ),
        (
            "failed_bad_failure_kind",
            "pending",
            Some("failed"),
            Some("database/private/path"),
            Some("2026-08-28T07:02:00Z"),
            false,
        ),
    ];

    for (name, materialization_state, job_state, failure_kind, completed_at, valid) in cases {
        let (_root, path) = database();
        let storage = Storage::open_v3_for_test(&path).expect("fresh v3");
        storage
            .connection
            .execute(
                "UPDATE storage_schema_meta SET materialization_state=?1 WHERE singleton=1;",
                [materialization_state],
            )
            .unwrap();
        if let Some(job_state) = job_state {
            let created_at = if name == "pending_bad_created" {
                "not-a-timestamp"
            } else {
                "2026-08-28T07:00:00Z"
            };
            if name == "failed_bad_failure_kind" {
                storage
                    .connection
                    .execute_batch("PRAGMA ignore_check_constraints=ON;")
                    .unwrap();
            }
            insert_lifetime_job(
                &storage.connection,
                name,
                job_state,
                "receipts",
                0,
                failure_kind,
                created_at,
                completed_at,
            );
            if name == "failed_bad_failure_kind" {
                storage
                    .connection
                    .execute_batch("PRAGMA ignore_check_constraints=OFF;")
                    .unwrap();
            }
        }
        let snapshot_before = database_snapshot(&storage.connection);
        drop(storage);
        let bytes_before = fs::read(&path).unwrap();

        let error = Storage::open(&path).err().expect("unsafe v3 must fail");
        assert!(!valid, "only invalid preflight cases belong in this test");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
        assert_eq!(
            error.detail(),
            "storage lifetime materialization state is invalid",
            "{name}"
        );
        assert!(!error.detail().contains(name));
        assert!(!error.detail().contains(path.to_string_lossy().as_ref()));
        let reopened = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&reopened), snapshot_before, "{name}");
        drop(reopened);
        assert_eq!(fs::read(&path).unwrap(), bytes_before, "{name}");
    }
}

#[test]
fn v3_migration_failure_rolls_back_to_exact_populated_v2() {
    let (_root, path) = database();
    let mut storage = populated_v2_storage(&path, "ready");
    let snapshot_before = database_snapshot(&storage.connection);
    let invalid = MIGRATION_V3.replacen(
        "CREATE TABLE lifetime_materialization_job",
        "INVALID SQL;\nCREATE TABLE lifetime_materialization_job",
        1,
    );

    let error = execute_migration(&mut storage.connection, &invalid, "v3 injected")
        .expect_err("injected v3 migration must fail");
    assert_eq!(error.kind(), "migrationFailed");
    assert!(storage.connection.is_autocommit());
    verify_schema_v2(&storage.connection).expect("rollback restored exact v2");
    assert_eq!(database_snapshot(&storage.connection), snapshot_before);
}

#[test]
fn every_admissible_v3_pair_migrates_to_exact_v4_without_changing_other_facts() {
    let job_cases = [
        ("pending", "receipts", "receipts"),
        ("pending", "messages", "messages"),
        ("pending", "workspaces", "workspaces"),
        ("pending", "baselines", "baselines"),
        ("pending", "verify", "verifySource"),
        ("failed", "receipts", "receipts"),
        ("failed", "messages", "messages"),
        ("failed", "workspaces", "workspaces"),
        ("failed", "baselines", "baselines"),
        ("failed", "verify", "verifySource"),
    ];
    for (job_state, v3_phase, v4_phase) in job_cases {
        let (_root, path) = database();
        let mut storage = populated_v3_storage(&path, "pending");
        let failure_kind = (job_state == "failed").then_some("database");
        let completed_at = (job_state == "failed").then_some("2026-08-28T07:02:00Z");
        insert_lifetime_job(
            &storage.connection,
            &format!("migration_{job_state}_{v3_phase}"),
            job_state,
            v3_phase,
            3,
            failure_kind,
            "2026-08-28T07:00:00Z",
            completed_at,
        );
        verify_schema_v3(&storage.connection).expect("admissible exact v3 pair");
        let preserved_tables = preserved_table_rows(&storage.connection);
        let guarded_schema = guarded_schema_objects(&storage.connection);
        let compaction = storage.lifetime_status().unwrap().compaction;

        migrate_v4(&mut storage.connection).expect("canonical v3 to v4 migration");
        assert_eq!(
            verify_schema_v4(&storage.connection).expect("exact structural v4"),
            "pending"
        );
        let migrated: (String, String, i64, i64) = storage
            .connection
            .query_row(
                "SELECT state,phase,after_scan_rowid,scan_exhausted FROM lifetime_materialization_job WHERE singleton=1;",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            migrated,
            (job_state.to_string(), v4_phase.to_string(), 3, 0),
            "{job_state} {v3_phase}"
        );
        assert_eq!(preserved_table_rows(&storage.connection), preserved_tables);
        assert_eq!(guarded_schema_objects(&storage.connection), guarded_schema);
        assert_eq!(storage.lifetime_status().unwrap().compaction, compaction);
        drop(storage);

        match Storage::open(&path) {
            Ok(_) if job_state == "pending" => {}
            Ok(_) => panic!("failed v4 job must not become runnable"),
            Err(error) if job_state == "failed" => {
                assert_eq!(error.kind(), "storageSchemaMismatch")
            }
            Err(error) => panic!("pending v4 job must reopen: {error:?}"),
        }
    }

    for materialization_state in ["pending", "ready"] {
        let (_root, path) = database();
        let mut storage = populated_v3_storage(&path, materialization_state);
        let preserved_tables = preserved_table_rows(&storage.connection);
        let guarded_schema = guarded_schema_objects(&storage.connection);
        migrate_v4(&mut storage.connection).expect("migrate absent v3 pair");
        verify_schema_v4_for_open(&storage.connection).expect("absent v4 pair reopens");
        assert_eq!(
            storage.connection.query_row(
                "SELECT storage_schema_version,materialization_state FROM storage_schema_meta WHERE singleton=1;",
                [],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            ).unwrap(),
            (STORAGE_LIFETIME_SCHEMA_VERSION_V4, materialization_state.to_string())
        );
        assert_eq!(
            storage
                .connection
                .query_row(
                    "SELECT count(*) FROM lifetime_materialization_job;",
                    [],
                    |row| row.get::<_, u32>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(preserved_table_rows(&storage.connection), preserved_tables);
        assert_eq!(guarded_schema_objects(&storage.connection), guarded_schema);
    }
}

#[test]
fn v4_rebuild_failure_rolls_back_to_exact_v3() {
    let (_root, path) = database();
    let mut storage = populated_v3_storage(&path, "pending");
    insert_lifetime_job(
        &storage.connection,
        "rollback_v4",
        "pending",
        "verify",
        2,
        None,
        "2026-08-28T08:00:00Z",
        None,
    );
    let snapshot_before = database_snapshot(&storage.connection);
    let invalid = MIGRATION_V4.replacen(
        "DROP TABLE lifetime_materialization_job_v3;",
        "INVALID SQL;\nDROP TABLE lifetime_materialization_job_v3;",
        1,
    );

    let error = execute_migration(&mut storage.connection, &invalid, "v4 injected")
        .expect_err("mid-rebuild failure must roll back");
    assert_eq!(error.kind(), "migrationFailed");
    assert!(storage.connection.is_autocommit());
    verify_schema_v3(&storage.connection).expect("rollback restored exact v3");
    assert_eq!(database_snapshot(&storage.connection), snapshot_before);
}

#[test]
fn v4_reopen_validates_pair_state_cursor_and_scan_exhaustion_fail_closed() {
    let valid_cases = [
        ("pending_absent", "pending", None),
        ("ready_absent", "ready", None),
        ("pending_job", "pending", Some("pending")),
    ];
    for (name, materialization_state, job_state) in valid_cases {
        let (_root, path) = database();
        let storage = Storage::open_v4_for_test(&path).expect("fresh v4");
        storage
            .connection
            .execute(
                "UPDATE storage_schema_meta SET materialization_state=?1 WHERE singleton=1;",
                [materialization_state],
            )
            .unwrap();
        if let Some(job_state) = job_state {
            insert_v4_job(
                &storage.connection,
                name,
                job_state,
                "receipts",
                20,
                0,
                0,
                None,
                None,
            );
        }
        verify_schema_v4_for_open(&storage.connection)
            .unwrap_or_else(|error| panic!("{name} must reopen: {error:?}"));
    }

    let (_root, path) = database();
    let storage = Storage::open_v4_for_test(&path).expect("fresh v4");
    insert_v4_job(
        &storage.connection,
        "pending_exhausted",
        "pending",
        "verifyReceipts",
        20,
        900,
        1,
        None,
        None,
    );
    verify_schema_v4_for_open(&storage.connection)
        .expect("persisted private exhaustion proof may reopen");

    for state in ["attempting", "failed"] {
        let (_root, path) = database();
        let storage = Storage::open_v4_for_test(&path).expect("fresh v4");
        let (failure_kind, completed_at) = if state == "failed" {
            (Some("database"), Some("2026-08-28T09:02:00Z"))
        } else {
            (None, None)
        };
        insert_v4_job(
            &storage.connection,
            state,
            state,
            "verifyReceipts",
            20,
            900,
            1,
            failure_kind,
            completed_at,
        );
        verify_schema_v4(&storage.connection).expect("non-runnable evidence is structural v4");
        let error = verify_schema_v4_for_open(&storage.connection)
            .expect_err("non-runnable v4 job rejected");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{state}");
        assert_eq!(
            error.detail(),
            "storage lifetime materialization state is invalid"
        );
    }

    for materialization_state in ["materializing", "failed"] {
        let (_root, path) = database();
        let storage = Storage::open_v4_for_test(&path).expect("fresh v4");
        storage
            .connection
            .execute(
                "UPDATE storage_schema_meta SET materialization_state=?1 WHERE singleton=1;",
                [materialization_state],
            )
            .unwrap();
        drop(storage);
        let error = Storage::open(&path)
            .err()
            .expect("unsafe v4 metadata state must fail");
        assert_eq!(
            error.kind(),
            "storageSchemaMismatch",
            "{materialization_state}"
        );
    }

    let malformed_cases = [
        ("schema_zero", "receipts", 20_i64, 0_i64, 0_i64),
        ("legacy_phase", "verify", 20, 0, 0),
        ("event_cursor_past_cutoff", "messages", 20, 21, 0),
        (
            "unsafe_derived_cursor",
            "verifyReceipts",
            20,
            9_007_199_254_740_992,
            0,
        ),
        ("invalid_scan_exhausted", "verifyReceipts", 20, 900, 2),
    ];
    for (name, phase, cutoff, after, exhausted) in malformed_cases {
        let (_root, path) = database();
        let storage = Storage::open_v4_for_test(&path).expect("fresh v4");
        storage
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        insert_v4_job(
            &storage.connection,
            name,
            "pending",
            phase,
            cutoff,
            after,
            exhausted,
            None,
            None,
        );
        if name == "schema_zero" {
            storage
                .connection
                .execute(
                    "UPDATE storage_schema_meta SET storage_schema_version=0 WHERE singleton=1;",
                    [],
                )
                .unwrap();
        }
        storage
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=OFF;")
            .unwrap();
        drop(storage);
        let error = Storage::open(&path).err().expect("malformed v4 must fail");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    }
}

#[test]
fn every_admissible_v4_job_migrates_to_exact_v5_with_conservative_projection_restart() {
    let phases = [
        "receipts",
        "messages",
        "workspaces",
        "baselines",
        "verifySource",
        "verifyReceipts",
        "verifyMessages",
        "verifyWorkspaces",
        "verifyBaselines",
    ];
    for state in ["pending", "attempting", "failed"] {
        for phase in phases {
            let (_root, path) = database();
            let mut storage = populated_v4_storage(&path, "pending");
            let failure_kind = (state == "failed").then_some("database");
            let completed_at = (state == "failed").then_some("2026-08-28T10:02:00Z");
            insert_v4_job(
                &storage.connection,
                &format!("v5_{state}_{phase}"),
                state,
                phase,
                20,
                3,
                1,
                failure_kind,
                completed_at,
            );
            verify_schema_v4(&storage.connection).expect("admissible exact v4 pair");
            let preserved_tables = preserved_table_rows(&storage.connection);
            let guarded_schema = guarded_schema_objects(&storage.connection);
            let compaction = storage.lifetime_status().unwrap().compaction;

            migrate_v5(&mut storage.connection).expect("canonical v4 to v5 migration");
            assert_eq!(
                verify_schema_v5(&storage.connection).expect("exact structural v5"),
                "pending"
            );
            let migrated: (
                String,
                String,
                i64,
                i64,
                Option<String>,
                String,
                String,
                Option<String>,
            ) = storage
                .connection
                .query_row(
                    "SELECT state,phase,after_scan_rowid,scan_exhausted,failure_kind,created_at,updated_at,completed_at FROM lifetime_materialization_job WHERE singleton=1;",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )
                .unwrap();
            let progressed_derived = matches!(
                phase,
                "verifyReceipts" | "verifyMessages" | "verifyWorkspaces" | "verifyBaselines"
            );
            assert_eq!(migrated.0, state, "{state} {phase}: state");
            assert_eq!(
                migrated.1,
                if progressed_derived {
                    "verifySource"
                } else {
                    phase
                },
                "{state} {phase}: phase"
            );
            assert_eq!(
                (migrated.2, migrated.3),
                if progressed_derived { (0, 0) } else { (3, 1) },
                "{state} {phase}: cursor"
            );
            assert_eq!(migrated.4.as_deref(), failure_kind, "{state} {phase}");
            assert_eq!(migrated.5, "2026-08-28T09:00:00Z", "{state} {phase}");
            assert_eq!(migrated.6, "2026-08-28T09:01:00Z", "{state} {phase}");
            assert_eq!(migrated.7.as_deref(), completed_at, "{state} {phase}");
            assert_eq!(preserved_table_rows(&storage.connection), preserved_tables);
            assert_eq!(guarded_schema_objects(&storage.connection), guarded_schema);
            assert_eq!(storage.lifetime_status().unwrap().compaction, compaction);

            match verify_schema_v5_for_open(&storage.connection) {
                Ok(()) if state == "pending" => {}
                Ok(()) => panic!("{state} {phase}: non-runnable job reopened"),
                Err(error) if state != "pending" => {
                    assert_eq!(error.kind(), "storageSchemaMismatch")
                }
                Err(error) => panic!("{state} {phase}: pending job must reopen: {error:?}"),
            }
            drop(storage);

            match Storage::open(&path) {
                Ok(_) if state == "pending" => {}
                Ok(_) => panic!("{state} {phase}: persisted non-runnable job reopened"),
                Err(error) if state != "pending" => {
                    assert_eq!(error.kind(), "storageSchemaMismatch")
                }
                Err(error) => panic!("{state} {phase}: persisted pending job failed: {error:?}"),
            }
        }
    }

    for materialization_state in ["pending", "ready"] {
        let (_root, path) = database();
        let mut storage = populated_v4_storage(&path, materialization_state);
        let preserved_tables = preserved_table_rows(&storage.connection);
        let guarded_schema = guarded_schema_objects(&storage.connection);
        migrate_v5(&mut storage.connection).expect("migrate absent v4 pair");
        verify_schema_v5_for_open(&storage.connection).expect("absent v5 pair reopens");
        assert_eq!(
            storage
                .connection
                .query_row(
                    "SELECT storage_schema_version,materialization_state FROM storage_schema_meta WHERE singleton=1;",
                    [],
                    |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
                )
                .unwrap(),
            (
                STORAGE_LIFETIME_SCHEMA_VERSION_V5,
                materialization_state.to_string()
            )
        );
        assert_eq!(preserved_table_rows(&storage.connection), preserved_tables);
        assert_eq!(guarded_schema_objects(&storage.connection), guarded_schema);
    }
}

#[test]
fn v5_rebuild_failure_rolls_back_to_exact_v4() {
    let (_root, path) = database();
    let mut storage = populated_v4_storage(&path, "pending");
    insert_v4_job(
        &storage.connection,
        "rollback_v5",
        "failed",
        "verifyReceipts",
        20,
        3,
        1,
        Some("database"),
        Some("2026-08-28T10:02:00Z"),
    );
    let snapshot_before = database_snapshot(&storage.connection);
    let invalid = MIGRATION_V5.replacen(
        "DROP TABLE lifetime_materialization_job_v4;",
        "INVALID SQL;\nDROP TABLE lifetime_materialization_job_v4;",
        1,
    );

    let error = execute_migration(&mut storage.connection, &invalid, "v5 injected")
        .expect_err("mid-rebuild failure must roll back");
    assert_eq!(error.kind(), "migrationFailed");
    assert!(storage.connection.is_autocommit());
    verify_schema_v4(&storage.connection).expect("rollback restored exact v4");
    assert_eq!(database_snapshot(&storage.connection), snapshot_before);
}

#[test]
fn v5_preflight_rejects_unsafe_v4_without_mutating_a_byte() {
    for (name, materialization_state, job) in [
        ("materializing_meta", "materializing", None),
        ("failed_meta", "failed", None),
        (
            "ready_with_job",
            "ready",
            Some(("pending", "receipts", 20_i64, 0_i64, 0_i64)),
        ),
        (
            "bad_phase",
            "pending",
            Some(("pending", "verify", 20, 0, 0)),
        ),
        (
            "cursor_past_cutoff",
            "pending",
            Some(("pending", "verifySource", 20, 21, 0)),
        ),
        (
            "bad_exhaustion",
            "pending",
            Some(("pending", "verifyReceipts", 20, 3, 2)),
        ),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v4_storage(&path, "pending");
        storage
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        storage
            .connection
            .execute(
                "UPDATE storage_schema_meta SET materialization_state=?1 WHERE singleton=1;",
                [materialization_state],
            )
            .unwrap();
        if let Some((state, phase, cutoff, after, exhausted)) = job {
            insert_v4_job(
                &storage.connection,
                name,
                state,
                phase,
                cutoff,
                after,
                exhausted,
                None,
                None,
            );
        }
        storage
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=OFF;")
            .unwrap();
        assert_v5_migration_rejected_without_mutation(&mut storage, name);
    }

    for (name, mutation) in [
        ("partial", "DROP TABLE lifetime_materialization_job;"),
        (
            "extra",
            "CREATE TABLE unexpected_storage_v4(value TEXT) STRICT;",
        ),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v4_storage(&path, "pending");
        storage.connection.execute_batch(mutation).unwrap();
        assert_v5_migration_rejected_without_mutation(&mut storage, name);
    }
}

#[test]
fn exact_v5_reopen_rejects_partial_extra_and_non_runnable_jobs() {
    for (name, mutation) in [
        ("partial", "DROP TABLE lifetime_materialization_job;"),
        (
            "extra",
            "CREATE TABLE unexpected_storage_v5(value TEXT) STRICT;",
        ),
    ] {
        let (_root, path) = database();
        let storage = Storage::open_v5_for_test(&path).expect("fresh v5");
        storage.connection.execute_batch(mutation).unwrap();
        drop(storage);
        let error = Storage::open(&path).err().expect("non-exact v5 must fail");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    }

    for state in ["attempting", "failed"] {
        let (_root, path) = database();
        let storage = Storage::open_v5_for_test(&path).expect("fresh v5");
        let (failure_kind, completed_at) = if state == "failed" {
            (Some("database"), Some("2026-08-28T10:02:00Z"))
        } else {
            (None, None)
        };
        insert_v4_job(
            &storage.connection,
            state,
            state,
            "verifyProjects",
            20,
            3,
            1,
            failure_kind,
            completed_at,
        );
        verify_schema_v5(&storage.connection).expect("structural v5 retains evidence");
        drop(storage);
        let error = Storage::open(&path)
            .err()
            .expect("non-runnable v5 must fail open");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{state}");
        assert_eq!(
            error.detail(),
            "storage lifetime materialization state is invalid"
        );
    }

    for (name, phase, cutoff, after, exhausted) in [
        ("legacy_phase", "verify", 20_i64, 0_i64, 0_i64),
        ("event_cursor_past_cutoff", "verifySource", 20, 21, 0),
        ("invalid_scan_exhausted", "verifyProjects", 20, 3, 2),
    ] {
        let (_root, path) = database();
        let storage = Storage::open_v5_for_test(&path).expect("fresh v5");
        storage
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        insert_v4_job(
            &storage.connection,
            name,
            "pending",
            phase,
            cutoff,
            after,
            exhausted,
            None,
            None,
        );
        storage
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=OFF;")
            .unwrap();
        drop(storage);
        let error = Storage::open(&path).err().expect("malformed v5 must fail");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    }
}

#[test]
fn every_admissible_v5_job_migrates_to_exact_v6_at_index_source() {
    let phases = [
        "receipts",
        "messages",
        "workspaces",
        "baselines",
        "verifySource",
        "verifyProjects",
        "verifyChats",
        "verifyRuns",
        "verifyLocks",
        "verifyAppInstances",
        "verifyReceipts",
        "verifyMessages",
        "verifyWorkspaces",
        "verifyBaselines",
    ];
    for state in ["pending", "attempting", "failed"] {
        for phase in phases {
            let (_root, path) = database();
            let mut storage = populated_v5_storage(&path, "pending");
            let failure_kind = (state == "failed").then_some("database");
            let completed_at = (state == "failed").then_some("2026-08-28T13:02:00Z");
            insert_v4_job(
                &storage.connection,
                &format!("v6_{state}_{phase}"),
                state,
                phase,
                20,
                3,
                1,
                failure_kind,
                completed_at,
            );
            verify_schema_v5(&storage.connection).expect("admissible exact v5 pair");
            let preserved_tables = preserved_table_rows(&storage.connection);
            let guarded_schema = guarded_schema_objects(&storage.connection);
            let compaction = storage.lifetime_status().unwrap().compaction;

            migrate_v6(&mut storage.connection).expect("canonical v5 to v6 migration");
            assert_eq!(
                verify_schema_v6(&storage.connection).expect("exact structural v6"),
                "pending"
            );
            let migrated: (
                String,
                String,
                i64,
                i64,
                Option<String>,
                String,
                String,
                Option<String>,
            ) = storage
                .connection
                .query_row(
                    "SELECT state,phase,after_scan_rowid,scan_exhausted,failure_kind,created_at,updated_at,completed_at FROM lifetime_materialization_job WHERE singleton=1;",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(migrated.0, state, "{state} {phase}: state");
            assert_eq!(migrated.1, "indexSource", "{state} {phase}: phase");
            assert_eq!((migrated.2, migrated.3), (0, 0), "{state} {phase}");
            assert_eq!(migrated.4.as_deref(), failure_kind, "{state} {phase}");
            assert_eq!(migrated.5, "2026-08-28T09:00:00Z", "{state} {phase}");
            assert_eq!(migrated.6, "2026-08-28T09:01:00Z", "{state} {phase}");
            assert_eq!(migrated.7.as_deref(), completed_at, "{state} {phase}");
            assert_eq!(preserved_table_rows(&storage.connection), preserved_tables);
            assert_eq!(guarded_schema_objects(&storage.connection), guarded_schema);
            assert_eq!(storage.lifetime_status().unwrap().compaction, compaction);
            assert_eq!(
                storage
                    .connection
                    .query_row("SELECT count(*) FROM event_source_index;", [], |row| {
                        row.get::<_, u32>(0)
                    })
                    .unwrap(),
                0,
                "migration must not backfill history"
            );

            match verify_schema_v6_for_open(&storage.connection) {
                Ok(()) if state == "pending" => {}
                Ok(()) => panic!("{state} {phase}: non-runnable v6 job reopened"),
                Err(error) if state != "pending" => {
                    assert_eq!(error.kind(), "storageSchemaMismatch")
                }
                Err(error) => panic!("{state} {phase}: pending v6 job must reopen: {error:?}"),
            }
        }
    }

    for previous_state in ["pending", "ready"] {
        let (_root, path) = database();
        let mut storage = populated_v5_storage(&path, previous_state);
        let preserved_tables = preserved_table_rows(&storage.connection);
        let guarded_schema = guarded_schema_objects(&storage.connection);
        migrate_v6(&mut storage.connection).expect("migrate absent v5 pair");
        verify_schema_v6_for_open(&storage.connection).expect("absent v6 pair reopens");
        assert_eq!(
            storage
                .connection
                .query_row(
                    "SELECT storage_schema_version,materialization_state FROM storage_schema_meta WHERE singleton=1;",
                    [],
                    |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
                )
                .unwrap(),
            (STORAGE_LIFETIME_SCHEMA_VERSION_V6, "pending".to_string())
        );
        assert_eq!(preserved_table_rows(&storage.connection), preserved_tables);
        assert_eq!(guarded_schema_objects(&storage.connection), guarded_schema);
        assert_eq!(
            storage
                .connection
                .query_row("SELECT count(*) FROM event_source_index;", [], |row| {
                    row.get::<_, u32>(0)
                })
                .unwrap(),
            0
        );
    }
}

#[test]
fn v6_rebuild_failure_rolls_back_to_exact_v5() {
    let (_root, path) = database();
    let mut storage = populated_v5_storage(&path, "pending");
    insert_v4_job(
        &storage.connection,
        "rollback_v6",
        "failed",
        "verifyBaselines",
        20,
        3,
        1,
        Some("database"),
        Some("2026-08-28T13:02:00Z"),
    );
    let snapshot_before = database_snapshot(&storage.connection);
    let invalid = MIGRATION_V6.replacen(
        "DROP TABLE lifetime_materialization_job_v5;",
        "INVALID SQL;\nDROP TABLE lifetime_materialization_job_v5;",
        1,
    );

    let error = execute_migration(&mut storage.connection, &invalid, "v6 injected")
        .expect_err("mid-rebuild failure must roll back");
    assert_eq!(error.kind(), "migrationFailed");
    assert!(storage.connection.is_autocommit());
    verify_schema_v5(&storage.connection).expect("rollback restored exact v5");
    assert_eq!(database_snapshot(&storage.connection), snapshot_before);
}

#[test]
fn v6_preflight_rejects_unsafe_v5_without_mutating_a_byte() {
    for (name, materialization_state, job) in [
        ("materializing_meta", "materializing", None),
        ("failed_meta", "failed", None),
        (
            "ready_with_job",
            "ready",
            Some(("pending", "receipts", 20_i64, 0_i64, 0_i64)),
        ),
        (
            "bad_phase",
            "pending",
            Some(("pending", "verify", 20, 0, 0)),
        ),
        (
            "cursor_past_cutoff",
            "pending",
            Some(("pending", "verifySource", 20, 21, 0)),
        ),
        (
            "bad_exhaustion",
            "pending",
            Some(("pending", "verifyProjects", 20, 3, 2)),
        ),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v5_storage(&path, "pending");
        storage
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        storage
            .connection
            .execute(
                "UPDATE storage_schema_meta SET materialization_state=?1 WHERE singleton=1;",
                [materialization_state],
            )
            .unwrap();
        if let Some((state, phase, cutoff, after, exhausted)) = job {
            insert_v4_job(
                &storage.connection,
                name,
                state,
                phase,
                cutoff,
                after,
                exhausted,
                None,
                None,
            );
        }
        storage
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=OFF;")
            .unwrap();
        assert_v6_migration_rejected_without_mutation(&mut storage, name);
    }

    for (name, mutation) in [
        ("partial", "DROP INDEX maintenance_jobs_status_created;"),
        (
            "extra",
            "CREATE TABLE unexpected_storage_v5(value TEXT) STRICT;",
        ),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v5_storage(&path, "pending");
        storage.connection.execute_batch(mutation).unwrap();
        assert_v6_migration_rejected_without_mutation(&mut storage, name);
    }
}

#[test]
fn v7_migration_preserves_every_admissible_v6_pair_and_all_existing_rows() {
    for (name, materialization_state, job_state) in [
        ("ready_absent", "ready", None),
        ("pending_absent", "pending", None),
        ("pending_pending", "pending", Some("pending")),
        ("pending_failed", "pending", Some("failed")),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v6_storage(&path, materialization_state);
        if materialization_state == "ready" {
            storage
                .connection
                .execute(
                    "UPDATE storage_schema_meta SET materialization_state='ready' WHERE singleton=1;",
                    [],
                )
                .unwrap();
        }
        if let Some(state) = job_state {
            let (failure_kind, completed_at) = if state == "failed" {
                (Some("database"), Some("2026-08-28T13:02:00Z"))
            } else {
                (None, None)
            };
            insert_v4_job(
                &storage.connection,
                name,
                state,
                "indexSource",
                20,
                0,
                0,
                failure_kind,
                completed_at,
            );
        }
        storage
            .connection
            .execute(
                "INSERT INTO maintenance_jobs(\
                   job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
                   failure_kind,created_at,updated_at\
                 ) VALUES (?1,'losslessCompaction','succeeded','{}',\
                   '2026-08-28T13:00:00Z','2026-08-28T13:01:00Z',\
                   '2026-08-28T13:01:00Z',NULL,'2026-08-28T13:00:00Z',\
                   '2026-08-28T13:01:00Z');",
                [format!("maintenance_{name}")],
            )
            .unwrap();
        verify_schema_v6(&storage.connection).expect("admissible exact v6 pair");
        let meta_before = table_rows(&storage.connection, "storage_schema_meta");
        let job_before = table_rows(&storage.connection, "lifetime_materialization_job");
        let maintenance_before = table_rows(&storage.connection, "maintenance_jobs");
        let source_index_before = table_rows(&storage.connection, "event_source_index");
        let preserved_before = preserved_table_rows(&storage.connection);

        migrate_v7(&mut storage.connection).expect("exact v6 to v7 migration");
        assert_eq!(
            verify_schema_v7(&storage.connection).expect("exact structural v7"),
            materialization_state,
            "{name}"
        );
        let meta_after = table_rows(&storage.connection, "storage_schema_meta");
        assert_eq!(meta_after.len(), 1, "{name}");
        assert_eq!(meta_after[0][3], meta_before[0][3], "{name}: state");
        assert_eq!(
            meta_after[0][4], meta_before[0][4],
            "{name}: materialization state"
        );
        assert_eq!(
            meta_after[0][5], meta_before[0][5],
            "{name}: migration timestamp"
        );
        assert_eq!(
            table_rows(&storage.connection, "lifetime_materialization_job"),
            job_before,
            "{name}: job evidence"
        );
        assert_eq!(
            table_rows(&storage.connection, "maintenance_jobs"),
            maintenance_before,
            "{name}: maintenance history"
        );
        assert_eq!(
            table_rows(&storage.connection, "event_source_index"),
            source_index_before,
            "{name}: source index"
        );
        assert_eq!(
            preserved_table_rows(&storage.connection),
            preserved_before,
            "{name}: preserved records"
        );
        assert_eq!(
            storage
                .connection
                .query_row(
                    "SELECT (SELECT count(*) FROM ledger_compaction_prefixes) + \
                            (SELECT count(*) FROM frozen_message_sources) + \
                            (SELECT count(*) FROM ledger_compaction_delete_authority);",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0,
            "{name}: migration must not backfill future compaction evidence"
        );
        match verify_schema_v7_for_open(&storage.connection) {
            Ok(()) if job_state != Some("failed") => {}
            Err(error) if job_state == Some("failed") => {
                assert_eq!(error.kind(), "storageSchemaMismatch")
            }
            result => panic!("{name}: unexpected V7 reopen result: {result:?}"),
        }
    }
}

#[test]
fn v7_preflight_rejects_attempting_active_partial_and_extra_v6_without_mutation() {
    for (name, lifetime_state, maintenance_state) in [
        ("attempting_lifetime", Some("attempting"), None),
        ("pending_compaction", None, Some("pending")),
        ("running_compaction", None, Some("running")),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v6_storage(&path, "pending");
        if let Some(state) = lifetime_state {
            insert_v4_job(
                &storage.connection,
                name,
                state,
                "indexSource",
                20,
                0,
                0,
                None,
                None,
            );
        }
        if let Some(state) = maintenance_state {
            storage
                .connection
                .execute(
                    "INSERT INTO maintenance_jobs(\
                       job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
                       failure_kind,created_at,updated_at\
                     ) VALUES (?1,'losslessCompaction',?2,NULL,NULL,NULL,NULL,NULL,\
                       '2026-08-28T13:00:00Z','2026-08-28T13:00:00Z');",
                    params![name, state],
                )
                .unwrap();
        }
        assert_v7_migration_rejected_without_mutation(&mut storage, name);
    }

    for (name, mutation) in [
        ("partial", "DROP INDEX event_source_index_message_unique;"),
        (
            "extra",
            "CREATE TABLE unexpected_storage_v6_for_v7(value TEXT) STRICT;",
        ),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v6_storage(&path, "pending");
        storage.connection.execute_batch(mutation).unwrap();
        assert_v7_migration_rejected_without_mutation(&mut storage, name);
    }
}

#[test]
fn v7_injected_migration_failure_restores_exact_v6() {
    let (_root, path) = database();
    let mut storage = populated_v6_storage(&path, "pending");
    storage.checkpoint().expect("checkpoint exact v6 fixture");
    let snapshot_before = database_snapshot(&storage.connection);
    let bytes_before = fs::read(storage.path()).expect("read exact v6 fixture");
    let invalid = MIGRATION_V7.replacen(
        "DROP TABLE storage_schema_meta_v6;",
        "INVALID SQL;\nDROP TABLE storage_schema_meta_v6;",
        1,
    );

    let error = execute_migration(&mut storage.connection, &invalid, "v7 injected")
        .expect_err("mid-migration failure must roll back");
    assert_eq!(error.kind(), "migrationFailed");
    assert!(storage.connection.is_autocommit());
    verify_schema_v6(&storage.connection).expect("rollback restored exact v6");
    assert_eq!(database_snapshot(&storage.connection), snapshot_before);
    storage
        .checkpoint()
        .expect("checkpoint rolled back v6 fixture");
    assert_eq!(
        fs::read(storage.path()).expect("read rolled back v6 fixture"),
        bytes_before
    );
}

#[test]
fn v8_migration_preserves_every_admissible_v7_pair_and_existing_fact() {
    for (name, materialization_state, job_state) in [
        ("ready_absent", "ready", None),
        ("pending_absent", "pending", None),
        ("pending_pending", "pending", Some("pending")),
        ("pending_failed", "pending", Some("failed")),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v7_storage(&path, "pending");
        if materialization_state == "ready" {
            storage
                .connection
                .execute(
                    "UPDATE storage_schema_meta SET materialization_state='ready' WHERE singleton=1;",
                    [],
                )
                .unwrap();
        }
        if let Some(state) = job_state {
            let (failure_kind, completed_at) = if state == "failed" {
                (Some("database"), Some("2026-09-01T04:02:00Z"))
            } else {
                (None, None)
            };
            insert_v4_job(
                &storage.connection,
                &format!("v8_{name}"),
                state,
                "indexSource",
                20,
                0,
                0,
                failure_kind,
                completed_at,
            );
        }
        storage
            .connection
            .execute(
                "INSERT INTO maintenance_jobs(\
                   job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
                   failure_kind,created_at,updated_at\
                 ) VALUES (?1,'losslessCompaction','failed','{}',\
                   '2026-09-01T04:00:00Z','2026-09-01T04:01:00Z',NULL,'database',\
                   '2026-09-01T04:00:00Z','2026-09-01T04:01:00Z');",
                [format!("v8_maintenance_{name}")],
            )
            .unwrap();
        verify_schema_v7(&storage.connection).expect("admissible exact v7 pair");
        let meta_before = table_rows(&storage.connection, "storage_schema_meta");
        let job_before = table_rows(&storage.connection, "lifetime_materialization_job");
        let maintenance_before = table_rows(&storage.connection, "maintenance_jobs");
        let source_before = table_rows(&storage.connection, "event_source_index");
        let preserved_before = preserved_table_rows(&storage.connection);

        migrate_v8(&mut storage.connection).expect("exact v7 to v8 migration");
        assert_eq!(
            verify_schema_v8(&storage.connection).expect("exact structural v8"),
            materialization_state,
            "{name}"
        );
        let meta_after = table_rows(&storage.connection, "storage_schema_meta");
        assert_eq!(
            meta_after[0][3], meta_before[0][3],
            "{name}: migration state"
        );
        assert_eq!(
            meta_after[0][4], meta_before[0][4],
            "{name}: materialization state"
        );
        assert_eq!(meta_after[0][5], meta_before[0][5], "{name}: migrated at");
        assert_eq!(
            table_rows(&storage.connection, "lifetime_materialization_job"),
            job_before,
            "{name}: lifetime job"
        );
        assert_eq!(
            table_rows(&storage.connection, "maintenance_jobs"),
            maintenance_before,
            "{name}: terminal maintenance"
        );
        assert_eq!(
            table_rows(&storage.connection, "event_source_index"),
            source_before,
            "{name}: source index"
        );
        assert_eq!(
            preserved_table_rows(&storage.connection),
            preserved_before,
            "{name}: preserved facts"
        );
        assert_eq!(
            storage
                .connection
                .query_row(
                    "SELECT count(*) FROM frozen_operation_sources;",
                    [],
                    |row| { row.get::<_, i64>(0) }
                )
                .unwrap(),
            0,
            "{name}: migration must not backfill frozen operations"
        );
        match verify_schema_v8_for_open(&storage.connection) {
            Ok(()) if job_state != Some("failed") => {}
            Err(error) if job_state == Some("failed") => {
                assert_eq!(error.kind(), "storageSchemaMismatch")
            }
            result => panic!("{name}: unexpected V8 reopen result: {result:?}"),
        }
    }
}

#[test]
fn v8_preflight_rejects_attempting_active_partial_and_extra_v7_without_mutation() {
    for (name, lifetime_state, maintenance_state) in [
        ("attempting_lifetime", Some("attempting"), None),
        ("pending_compaction", None, Some("pending")),
        ("running_compaction", None, Some("running")),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v7_storage(&path, "pending");
        if let Some(state) = lifetime_state {
            insert_v4_job(
                &storage.connection,
                name,
                state,
                "indexSource",
                20,
                0,
                0,
                None,
                None,
            );
        }
        if let Some(state) = maintenance_state {
            storage
                .connection
                .execute(
                    "INSERT INTO maintenance_jobs(\
                       job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
                       failure_kind,created_at,updated_at\
                     ) VALUES (?1,'losslessCompaction',?2,NULL,NULL,NULL,NULL,NULL,\
                       '2026-09-01T04:00:00Z','2026-09-01T04:00:00Z');",
                    params![name, state],
                )
                .unwrap();
        }
        assert_v8_migration_rejected_without_mutation(&mut storage, name);
    }

    for (name, mutation) in [
        ("partial", "DROP INDEX ledger_compaction_prefixes_latest;"),
        (
            "extra",
            "CREATE TABLE unexpected_storage_v7_for_v8(value TEXT) STRICT;",
        ),
        (
            "tamper",
            "DROP TRIGGER ledger_compaction_prefixes_require_exact_anchor;",
        ),
    ] {
        let (_root, path) = database();
        let mut storage = populated_v7_storage(&path, "pending");
        storage.connection.execute_batch(mutation).unwrap();
        assert_v8_migration_rejected_without_mutation(&mut storage, name);
    }
}

#[test]
fn v8_preflight_rejects_every_nonempty_v7_compaction_table_without_mutation() {
    let (_root, prefix_path) = database();
    let mut prefix = populated_v7_storage(&prefix_path, "pending");
    seed_v7_prefix_for_v8(&mut prefix);
    assert_v8_migration_rejected_without_mutation(&mut prefix, "prefix");

    let (_root, frozen_path) = database();
    let mut frozen = populated_v7_storage(&frozen_path, "pending");
    seed_v7_frozen_message_for_v8(&mut frozen);
    assert_v8_migration_rejected_without_mutation(&mut frozen, "frozen_message");

    let (_root, authority_path) = database();
    let mut authority = populated_v7_storage(&authority_path, "pending");
    seed_v7_authority_for_v8(&mut authority);
    assert_v8_migration_rejected_without_mutation(&mut authority, "authority");
}

#[test]
fn v8_injected_migration_failure_restores_exact_v7_bytes() {
    let (_root, path) = database();
    let mut storage = populated_v7_storage(&path, "pending");
    storage.checkpoint().expect("checkpoint exact v7 fixture");
    let snapshot_before = database_snapshot(&storage.connection);
    let bytes_before = fs::read(storage.path()).expect("read exact v7 fixture");
    let invalid = MIGRATION_V8.replacen(
        "DROP TABLE storage_schema_meta_v7;",
        "INVALID SQL;\nDROP TABLE storage_schema_meta_v7;",
        1,
    );

    let error = execute_migration(&mut storage.connection, &invalid, "v8 injected")
        .expect_err("mid-migration failure must roll back");
    assert_eq!(error.kind(), "migrationFailed");
    assert!(storage.connection.is_autocommit());
    verify_schema_v7(&storage.connection).expect("rollback restored exact v7");
    assert_eq!(database_snapshot(&storage.connection), snapshot_before);
    storage
        .checkpoint()
        .expect("checkpoint rolled back v7 fixture");
    assert_eq!(
        fs::read(storage.path()).expect("read rolled back v7 fixture"),
        bytes_before
    );
}

#[test]
fn exact_v8_reopen_rejects_schema_tamper_and_non_runnable_state() {
    for (name, mutation) in [
        ("partial", "DROP TABLE frozen_operation_sources;"),
        (
            "extra",
            "CREATE TABLE unexpected_storage_v8(value TEXT) STRICT;",
        ),
        (
            "trigger_tamper",
            "DROP TRIGGER frozen_operation_sources_require_exact_live_source;",
        ),
        (
            "index_tamper",
            "DROP INDEX frozen_operation_sources_aggregate;",
        ),
    ] {
        let (_root, path) = database();
        let storage = Storage::open(&path).expect("fresh v8");
        storage.connection.execute_batch(mutation).unwrap();
        drop(storage);
        let error = Storage::open(&path).err().expect("non-exact v8 must fail");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    }

    for state in ["pending", "running"] {
        let (_root, path) = database();
        let storage = Storage::open(&path).expect("fresh v8");
        storage
            .connection
            .execute(
                "INSERT INTO maintenance_jobs(
                   job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
                   failure_kind,created_at,updated_at
                 ) VALUES (?1,'losslessCompaction',?2,NULL,NULL,NULL,NULL,NULL,
                   '2026-09-01T04:30:00Z','2026-09-01T04:30:00Z');",
                params![format!("v8_active_{state}"), state],
            )
            .unwrap();
        let before = database_snapshot(&storage.connection);
        drop(storage);
        let error = Storage::open(&path)
            .err()
            .expect("active V8 compaction must fail closed");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{state}");
        let connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        assert_eq!(database_snapshot(&connection), before, "{state}");
    }
}

#[test]
fn v8_forbidden_runtime_states_fence_duplicate_and_conflicting_resubmits_at_entry() {
    for state in [
        "prefix",
        "frozen_message",
        "authority",
        "frozen_operation",
        "active_compaction",
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("fresh v8");
        let original = event(
            &format!("evt_v8_resubmit_{state}"),
            &format!("op_v8_resubmit_{state}"),
            "project",
            &format!("project_v8_resubmit_{state}"),
            1,
            json!({
                "canonical_path":format!("/fixture/v8-resubmit-{state}"),
                "project_id":format!("project_v8_resubmit_{state}")
            }),
            "2026-09-01T05:00:00Z",
        );
        assert_eq!(
            storage.append_event(&original).expect("append original"),
            AppendOutcome::Committed,
            "{state}"
        );
        seed_v8_forbidden_append_state(&mut storage, state);
        assert_resubmits_rejected_without_mutation(&mut storage, &original, state);
    }
}

#[test]
fn v8_nonrunnable_materialization_pairs_fence_every_append_decision_at_entry() {
    for pair in ["attempting", "failed", "noncanonical"] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("fresh v8");
        let original = event(
            &format!("evt_v8_materialization_{pair}"),
            &format!("op_v8_materialization_{pair}"),
            "project",
            &format!("project_v8_materialization_{pair}"),
            1,
            json!({
                "canonical_path":format!("/fixture/v8-materialization-{pair}"),
                "project_id":format!("project_v8_materialization_{pair}")
            }),
            "2026-09-01T05:20:00Z",
        );
        assert_eq!(
            storage.append_event(&original).expect("append original"),
            AppendOutcome::Committed,
            "{pair}"
        );
        if pair == "noncanonical" {
            storage
                .connection
                .execute(
                    "UPDATE storage_schema_meta SET materialization_state='ready' WHERE singleton=1;",
                    [],
                )
                .unwrap();
        }
        let (state, failure_kind, completed_at) = match pair {
            "attempting" => ("attempting", None, None),
            "failed" => ("failed", Some("database"), Some("2026-09-01T05:20:02Z")),
            "noncanonical" => ("pending", None, None),
            _ => unreachable!(),
        };
        insert_v4_job(
            &storage.connection,
            &format!("v8_materialization_{pair}"),
            state,
            "indexSource",
            1,
            0,
            0,
            failure_kind,
            completed_at,
        );

        assert_resubmits_rejected_without_mutation(&mut storage, &original, pair);
        assert_fresh_append_rejected_without_mutation(&mut storage, pair);
    }
}

#[test]
fn uncompacted_v7_and_v8_keep_duplicate_and_conflict_semantics() {
    for version in ["v7", "v8"] {
        let (_root, path) = database();
        let mut storage = if version == "v7" {
            Storage::open_v7_for_test(&path).expect("fresh v7")
        } else {
            Storage::open(&path).expect("fresh v8")
        };
        let original = event(
            &format!("evt_{version}_valid_resubmit"),
            &format!("op_{version}_valid_resubmit"),
            "project",
            &format!("project_{version}_valid_resubmit"),
            1,
            json!({
                "canonical_path":format!("/fixture/{version}-valid-resubmit"),
                "project_id":format!("project_{version}_valid_resubmit")
            }),
            "2026-09-01T05:10:00Z",
        );
        assert_eq!(
            storage.append_event(&original).expect("append original"),
            AppendOutcome::Committed,
            "{version}"
        );
        storage.checkpoint().expect("checkpoint valid resubmit");
        let snapshot = database_snapshot(&storage.connection);
        let bytes = fs::read(storage.path()).expect("read valid resubmit database");

        assert_eq!(
            storage.append_event(&original).expect("duplicate resubmit"),
            AppendOutcome::DuplicateNoOp,
            "{version}"
        );
        assert_eq!(
            database_snapshot(&storage.connection),
            snapshot,
            "{version}"
        );
        assert_eq!(
            fs::read(storage.path()).expect("reread duplicate database"),
            bytes,
            "{version}"
        );

        let mut conflict = original.clone();
        conflict.canonical_json_payload = canonical_json_v1(json!({
            "canonical_path":format!("/fixture/{version}-conflict"),
            "project_id":format!("project_{version}_valid_resubmit")
        }))
        .unwrap();
        assert_eq!(
            storage
                .append_event(&conflict)
                .expect("conflicting resubmit"),
            AppendOutcome::Conflict,
            "{version}"
        );
        assert_eq!(
            database_snapshot(&storage.connection),
            snapshot,
            "{version}"
        );
        assert_eq!(
            fs::read(storage.path()).expect("reread conflict database"),
            bytes,
            "{version}"
        );
    }
}

#[test]
fn v8_frozen_operation_validation_recomputes_digests_and_cross_checks_authority() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("fresh v8");
    let operation_id = seed_exact_v8_frozen_operation(&mut storage);
    validate_v8_frozen_operation_source(&storage.connection, &operation_id)
        .expect("exact frozen operation validates");
    let before = database_snapshot(&storage.connection);
    let error = storage
        .materialize_lifetime_batch_for_test(&mut crate::storage_lifetime::NeverCancel, None)
        .expect_err("unexpected frozen operation must fence bounded lifetime work");
    assert_eq!(error.kind(), "storageSchemaMismatch");
    assert_eq!(database_snapshot(&storage.connection), before);
    let error = storage
        .append_event(&event(
            "evt_after_frozen_operation",
            "op_after_frozen_operation",
            "project",
            "project_after_frozen",
            1,
            json!({
                "canonical_path":"/fixture/after-frozen",
                "project_id":"project_after_frozen"
            }),
            "2026-09-01T04:32:00Z",
        ))
        .expect_err("unexpected frozen operation must fence append");
    assert_eq!(error.kind(), "storageSchemaMismatch");
    assert_eq!(database_snapshot(&storage.connection), before);
    drop(storage);
    assert_eq!(
        Storage::open(&path)
            .err()
            .expect("unexpected frozen operation must fence reopen")
            .kind(),
        "storageSchemaMismatch"
    );

    for (name, mutation) in [
        (
            "payload_digest",
            "DROP TRIGGER frozen_operation_sources_append_only_update;
             UPDATE frozen_operation_sources SET canonical_payload_digest=
               'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';",
        ),
        (
            "record_digest",
            "DROP TRIGGER frozen_operation_sources_append_only_update;
             UPDATE frozen_operation_sources SET record_digest=
               'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';",
        ),
        (
            "receipt_binding",
            "DROP TRIGGER operation_receipts_compaction_immutable_update;
             UPDATE operation_receipts SET event_id='evt_receipt_tamper';",
        ),
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("fresh tamper v8");
        let operation_id = seed_exact_v8_frozen_operation(&mut storage);
        storage.connection.execute_batch(mutation).unwrap();
        let snapshot = database_snapshot(&storage.connection);
        let error = validate_v8_frozen_operation_source(&storage.connection, &operation_id)
            .expect_err("frozen operation tamper must fail closed");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
        let error = verify_v8_uncompacted_runtime_state(&storage.connection)
            .expect_err("runtime authority must reject tamper");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
        assert_eq!(database_snapshot(&storage.connection), snapshot, "{name}");
    }
}

#[test]
fn terminal_compacted_v8_reopens_replays_appends_and_classifies_deleted_operation() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("fresh v8");
    let first = event(
        "evt_v8_c1b1_project_1",
        "op_v8_c1b1_project_1",
        "project",
        "project_v8_c1b1",
        1,
        json!({"canonical_path":"/fixture/v8-c1b1-1","project_id":"project_v8_c1b1"}),
        "2026-09-01T11:00:00Z",
    );
    let second = event(
        "evt_v8_c1b1_project_2",
        "op_v8_c1b1_project_2",
        "project",
        "project_v8_c1b1",
        2,
        json!({"canonical_path":"/fixture/v8-c1b1-2","project_id":"project_v8_c1b1"}),
        "2026-09-01T11:00:01Z",
    );
    storage.append_event(&first).expect("append first");
    storage.append_event(&second).expect("append second");
    run_bounded_materializer_to_ready(&mut storage);
    insert_exact_v8_frozen_operation(&mut storage, &first, "2026-09-01T11:01:00Z");
    insert_exact_v8_frozen_operation(&mut storage, &second, "2026-09-01T11:01:01Z");
    let baseline_identity = "project\0project_v8_c1b1\u{0}1";
    let baseline_id = format!(
        "baseline_{}",
        sha256_file_version_v1(baseline_identity.as_bytes())
            .strip_prefix("sha256:")
            .unwrap()
    );
    let baseline_digest = sha256_file_version_v1(first.canonical_json_payload.as_bytes());
    storage
        .connection
        .execute(
            "INSERT INTO ledger_baselines(
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at
             ) VALUES (?1,'project','project_v8_c1b1',1,1,?2,?3,?4,?4);",
            params![
                baseline_id,
                first.canonical_json_payload,
                baseline_digest,
                "2026-09-01T11:01:02Z"
            ],
        )
        .expect("insert historical baseline");
    storage
        .connection
        .execute(
            "INSERT INTO ledger_compaction_prefixes(
               prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,
               anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,
               baseline_projection_digest,created_at,verified_at
             ) SELECT 'prefix_v8_c1b1','project','project_v8_c1b1',1,event_rowid,event_id,
               operation_id,aggregate_sequence,?1,?2,?3,?3
               FROM event_source_index WHERE operation_id=?4;",
            params![
                baseline_id,
                baseline_digest,
                "2026-09-01T11:01:03Z",
                second.operation_id
            ],
        )
        .expect("insert exact prefix");
    let cutoff: i64 = storage
        .connection
        .query_row(
            "SELECT rowid FROM events WHERE operation_id=?1;",
            [&second.operation_id],
            |row| row.get(0),
        )
        .unwrap();
    let proof = json!({
        "storageCompactionV2PrivateState": {
            "schemaVersion": 1,
            "cursor": {
                "schemaVersion": 2,
                "cutoffEventRowid": cutoff,
                "phase": "finalize",
                "afterEventRowid": cutoff,
                "scanExhausted": true
            },
            "operationsVerified": true,
            "equivalenceVerified": true
        }
    });
    storage
        .connection
        .execute(
            "INSERT INTO maintenance_jobs(
               job_id,job_kind,state,cutoff_json,started_at,created_at,updated_at
             ) VALUES ('compact_v8_c1b1','losslessCompaction','running',?1,?2,?2,?2);",
            params![proof.to_string(), "2026-09-01T11:01:04Z"],
        )
        .expect("insert fixture compaction job");
    storage
        .connection
        .execute(
            "INSERT INTO ledger_compaction_delete_authority(
               job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,
               aggregate_id,aggregate_sequence,authorized_at
             ) SELECT 'compact_v8_c1b1',1,'prefix_v8_c1b1',event_rowid,event_id,operation_id,
               aggregate_type,aggregate_id,aggregate_sequence,?1
               FROM event_source_index WHERE operation_id=?2;",
            params!["2026-09-01T11:01:05Z", first.operation_id],
        )
        .expect("insert exact fixture authority");
    storage
        .connection
        .execute(
            "DELETE FROM event_source_index WHERE operation_id=?1;",
            [&first.operation_id],
        )
        .expect("delete compacted source index");
    storage
        .connection
        .execute(
            "DELETE FROM events WHERE operation_id=?1;",
            [&first.operation_id],
        )
        .expect("delete compacted source event");
    storage
        .connection
        .execute(
            "UPDATE maintenance_jobs SET state='succeeded',completed_at=?1,last_successful_at=?1,
             updated_at=?1 WHERE job_id='compact_v8_c1b1';",
            ["2026-09-01T11:01:06Z"],
        )
        .expect("publish terminal fixture proof");

    verify_v8_compaction_runtime_state(&storage.connection).expect("valid compacted runtime");
    verify_projection_replay(&storage.connection).expect("baseline plus tail replay");
    let before = database_snapshot(&storage.connection);
    {
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        assert!(matches!(verifier.step(&mut || false).unwrap(),
        FrozenOperationVerificationStep::FrozenOperationsVerified(usage) if usage.rows == 2));
    }
    assert_eq!(database_snapshot(&storage.connection), before);
    drop(storage);
    let mut reopened = Storage::open(&path).expect("terminal compacted v8 reopens");
    let mut retried_with_new_request = first.clone();
    retried_with_new_request.request_id = Some("request_v8_c1b1_retry".to_string());
    assert_eq!(
        reopened
            .append_event(&retried_with_new_request)
            .expect("deleted operation resubmit"),
        AppendOutcome::DuplicateNoOp
    );
    let mut conflict = first.clone();
    conflict.canonical_json_payload = canonical_json_v1(json!({
        "canonical_path":"/fixture/v8-c1b1-conflict",
        "project_id":"project_v8_c1b1"
    }))
    .unwrap();
    assert_eq!(
        reopened
            .append_event(&conflict)
            .expect("deleted operation conflict"),
        AppendOutcome::Conflict
    );
    let third = event(
        "evt_v8_c1b1_project_3",
        "op_v8_c1b1_project_3",
        "project",
        "project_v8_c1b1",
        3,
        json!({"canonical_path":"/fixture/v8-c1b1-3","project_id":"project_v8_c1b1"}),
        "2026-09-01T11:02:00Z",
    );
    assert_eq!(
        reopened.append_event(&third).unwrap(),
        AppendOutcome::Committed
    );
    run_bounded_materializer_to_ready(&mut reopened);
    drop(reopened);
    Storage::open(&path).expect("post-compaction append and materialization reopen");
}

#[test]
fn terminal_compacted_v8_chat_uses_frozen_predecessor_and_reserves_message_identity() {
    let (root, path) = database();
    let mut storage = Storage::open(&path).expect("fresh v8");
    storage
        .append_event(&event(
            "evt_v8_c1b1_chat_project",
            "op_v8_c1b1_chat_project",
            "project",
            "project_v8_c1b1",
            1,
            json!({"canonical_path":"/fixture/v8-c1b1-chat","project_id":"project_v8_c1b1"}),
            "2026-09-01T11:09:59Z",
        ))
        .expect("append chat project");
    let message_one = team_message("chat_v8_c1b1", "message_v8_c1b1_1", 1);
    let first = event(
        "evt_v8_c1b1_chat_1",
        "op_v8_c1b1_chat_1",
        "chat",
        "chat_v8_c1b1",
        1,
        materializable_chat_payload("chat_v8_c1b1", "project_v8_c1b1", 1, message_one.clone()),
        "2026-09-01T11:10:00Z",
    );
    let second = event(
        "evt_v8_c1b1_chat_2",
        "op_v8_c1b1_chat_2",
        "chat",
        "chat_v8_c1b1",
        2,
        materializable_chat_payload("chat_v8_c1b1", "project_v8_c1b1", 1, message_one.clone()),
        "2026-09-01T11:10:01Z",
    );
    storage
        .append_event(&first)
        .expect("append chat introduction");
    storage
        .append_event(&second)
        .expect("append repeated snapshot");
    run_bounded_materializer_to_ready(&mut storage);
    insert_exact_v8_frozen_operation(&mut storage, &first, "2026-09-01T11:11:00Z");
    insert_exact_v8_frozen_operation(&mut storage, &second, "2026-09-01T11:11:01Z");
    storage
        .connection
        .execute(
            "INSERT INTO frozen_message_sources(
               message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,
               source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at
             ) SELECT message.message_id,source.event_rowid,source.event_id,source.operation_id,
               message.chat_id,source.aggregate_sequence,message.event_sequence,
               message.canonical_message_json,message.message_digest,?1
               FROM event_source_index AS source JOIN chat_messages AS message
                 ON message.message_id=source.introduced_message_id
               WHERE source.operation_id=?2;",
            params!["2026-09-01T11:11:02Z", first.operation_id],
        )
        .expect("freeze introduced message");
    let baseline_identity = "chat\0chat_v8_c1b1\u{0}1";
    let baseline_id = format!(
        "baseline_{}",
        sha256_file_version_v1(baseline_identity.as_bytes())
            .strip_prefix("sha256:")
            .unwrap()
    );
    let baseline_digest = sha256_file_version_v1(first.canonical_json_payload.as_bytes());
    storage
        .connection
        .execute(
            "INSERT INTO ledger_baselines(
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at
             ) VALUES (?1,'chat','chat_v8_c1b1',1,1,?2,?3,?4,?4);",
            params![
                baseline_id,
                first.canonical_json_payload,
                baseline_digest,
                "2026-09-01T11:11:03Z"
            ],
        )
        .expect("insert chat baseline");
    storage
        .connection
        .execute(
            "INSERT INTO ledger_compaction_prefixes(
               prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,
               anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,
               baseline_projection_digest,created_at,verified_at
             ) SELECT 'prefix_v8_c1b1_chat','chat','chat_v8_c1b1',1,event_rowid,event_id,
               operation_id,aggregate_sequence,?1,?2,?3,?3
               FROM event_source_index WHERE operation_id=?4;",
            params![
                baseline_id,
                baseline_digest,
                "2026-09-01T11:11:04Z",
                second.operation_id
            ],
        )
        .expect("insert chat prefix");
    let cutoff: i64 = storage
        .connection
        .query_row(
            "SELECT rowid FROM events WHERE operation_id=?1;",
            [&second.operation_id],
            |row| row.get(0),
        )
        .unwrap();
    let proof = json!({"storageCompactionV2PrivateState": {
        "schemaVersion":1,
        "cursor":{"schemaVersion":2,"cutoffEventRowid":cutoff,"phase":"finalize",
                  "afterEventRowid":cutoff,"scanExhausted":true},
        "operationsVerified":true,"equivalenceVerified":true
    }});
    storage.connection.execute(
        "INSERT INTO maintenance_jobs(job_id,job_kind,state,cutoff_json,started_at,created_at,updated_at)
         VALUES ('compact_v8_c1b1_chat','losslessCompaction','running',?1,?2,?2,?2);",
        params![proof.to_string(), "2026-09-01T11:11:05Z"],
    ).unwrap();
    storage
        .connection
        .execute(
            "INSERT INTO ledger_compaction_delete_authority(
           job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,
           aggregate_id,aggregate_sequence,authorized_at)
         SELECT 'compact_v8_c1b1_chat',1,'prefix_v8_c1b1_chat',event_rowid,event_id,operation_id,
           aggregate_type,aggregate_id,aggregate_sequence,?1 FROM event_source_index
           WHERE operation_id=?2;",
            params!["2026-09-01T11:11:06Z", first.operation_id],
        )
        .unwrap();
    storage
        .connection
        .execute(
            "DELETE FROM event_source_index WHERE operation_id=?1;",
            [&first.operation_id],
        )
        .unwrap();
    storage
        .connection
        .execute(
            "DELETE FROM events WHERE operation_id=?1;",
            [&first.operation_id],
        )
        .unwrap();
    storage
        .connection
        .execute(
            "UPDATE maintenance_jobs SET state='succeeded',completed_at=?1,last_successful_at=?1,
         updated_at=?1 WHERE job_id='compact_v8_c1b1_chat';",
            ["2026-09-01T11:11:07Z"],
        )
        .unwrap();
    drop(storage);

    let mut reopened = Storage::open(&path).expect("compacted chat reopens");
    let message_two = team_message("chat_v8_c1b1", "message_v8_c1b1_2", 2);
    let third = event(
        "evt_v8_c1b1_chat_3",
        "op_v8_c1b1_chat_3",
        "chat",
        "chat_v8_c1b1",
        3,
        materializable_chat_payload("chat_v8_c1b1", "project_v8_c1b1", 2, message_two),
        "2026-09-01T11:12:00Z",
    );
    assert_eq!(
        reopened.append_event(&third).unwrap(),
        AppendOutcome::Committed
    );
    let reuse = event(
        "evt_v8_c1b1_chat_reuse",
        "op_v8_c1b1_chat_reuse",
        "chat",
        "chat_v8_c1b1",
        4,
        materializable_chat_payload("chat_v8_c1b1", "project_v8_c1b1", 3, message_one),
        "2026-09-01T11:12:01Z",
    );
    let before = database_snapshot(&reopened.connection);
    reopened
        .append_event(&reuse)
        .expect_err("frozen message id reuse must fail");
    assert_eq!(database_snapshot(&reopened.connection), before);
    run_bounded_materializer_to_ready(&mut reopened);
    drop(reopened);
    let valid = Storage::open(&path).expect("compacted chat materializes and reopens");
    valid.checkpoint().expect("checkpoint compacted chat");
    drop(valid);
    for (name, mutation) in [
        (
            "durable",
            "UPDATE event_source_index SET chat_durable_sequence=2 \
             WHERE operation_id='op_v8_c1b1_chat_2';",
        ),
        (
            "snapshot",
            "UPDATE event_source_index SET chat_snapshot_message_id='message_tampered' \
             WHERE operation_id='op_v8_c1b1_chat_2';",
        ),
        (
            "introduced",
            "UPDATE event_source_index SET introduced_message_id='message_tampered' \
             WHERE operation_id='op_v8_c1b1_chat_2';",
        ),
    ] {
        let tampered_path = root.path().join(format!("v8-chat-anchor-{name}.sqlite3"));
        fs::copy(&path, &tampered_path).expect("copy compacted chat");
        let tampered = Storage::open(&tampered_path).expect("valid compacted chat copy");
        let trigger: String = tampered
            .connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' \
                 AND name='event_source_index_append_only_update';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        tampered
            .connection
            .execute_batch("DROP TRIGGER event_source_index_append_only_update;")
            .unwrap();
        tampered.connection.execute_batch(mutation).unwrap();
        tampered.connection.execute_batch(&trigger).unwrap();
        tampered.checkpoint().unwrap();
        let snapshot = database_snapshot(&tampered.connection);
        let bytes = fs::read(&tampered_path).unwrap();
        drop(tampered);
        let error = match Storage::open(&tampered_path) {
            Ok(_) => panic!("tampered compacted anchor must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
        assert_eq!(fs::read(&tampered_path).unwrap(), bytes, "{name}");
        let connection = Connection::open(&tampered_path).unwrap();
        configure_connection(&connection).unwrap();
        assert_eq!(database_snapshot(&connection), snapshot, "{name}");
    }
}

#[test]
fn terminal_compacted_v8_over_one_hundred_frozen_operations_fails_closed_without_mutation() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("fresh v8");
    let mut events = Vec::new();
    for ordinal in 0..=100 {
        let event = event(
            &format!("evt_v8_bound_{ordinal}"),
            &format!("op_v8_bound_{ordinal}"),
            "project",
            &format!("project_v8_bound_{ordinal}"),
            1,
            json!({
                "canonical_path":format!("/fixture/v8-bound-{ordinal}"),
                "project_id":format!("project_v8_bound_{ordinal}")
            }),
            "2026-09-01T11:20:00Z",
        );
        storage
            .append_event(&event)
            .expect("append bounded fixture event");
        events.push(event);
    }
    run_bounded_materializer_to_ready(&mut storage);
    for event in &events {
        insert_exact_v8_frozen_operation(&mut storage, event, "2026-09-01T11:21:00Z");
    }
    let cutoff: i64 = storage
        .connection
        .query_row("SELECT max(rowid) FROM events;", [], |row| row.get(0))
        .unwrap();
    let proof = json!({"storageCompactionV2PrivateState": {
        "schemaVersion":1,
        "cursor":{"schemaVersion":2,"cutoffEventRowid":cutoff,"phase":"finalize",
                  "afterEventRowid":cutoff,"scanExhausted":true},
        "operationsVerified":true,"equivalenceVerified":true
    }});
    storage.connection.execute(
        "INSERT INTO maintenance_jobs(
           job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,created_at,updated_at)
         VALUES ('compact_v8_bound','losslessCompaction','succeeded',?1,?2,?2,?2,?2,?2);",
        params![proof.to_string(), "2026-09-01T11:23:00Z"],
    ).unwrap();
    let snapshot = database_snapshot(&storage.connection);
    let error = verify_v8_compaction_runtime_state(&storage.connection)
        .expect_err("101st frozen operation must exceed temporary C1B1 reopen bound");
    assert_eq!(error.kind(), "storageSchemaMismatch");
    assert_eq!(database_snapshot(&storage.connection), snapshot);
    storage.checkpoint().expect("checkpoint bounded fixture");
    let bytes = fs::read(storage.path()).expect("read bounded fixture");
    drop(storage);
    let error = match Storage::open(&path) {
        Ok(_) => panic!("over-bound compacted V8 must not reopen"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageSchemaMismatch");
    assert_eq!(fs::read(&path).expect("reread bounded fixture"), bytes);
}

#[test]
fn v8_frozen_operation_reopen_budget_accepts_exact_mib_and_rejects_single_oversize() {
    for (name, target, succeeds) in [
        ("exact", COMPACTION_VERIFY_MAX_PAYLOAD_BYTES, true),
        ("oversize", COMPACTION_VERIFY_MAX_PAYLOAD_BYTES + 1, false),
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("fresh v8");
        let project_id = format!("project_v8_payload_{name}");
        let empty =
            canonical_json_v1(json!({"canonical_path":"","project_id":project_id})).unwrap();
        let padding = "x".repeat(target - empty.len());
        let source = event(
            &format!("evt_v8_payload_{name}"),
            &format!("op_v8_payload_{name}"),
            "project",
            &project_id,
            1,
            json!({"canonical_path":padding,"project_id":project_id}),
            "2026-09-01T11:30:00Z",
        );
        assert_eq!(source.canonical_json_payload.len(), target);
        storage
            .append_event(&source)
            .expect("append payload boundary source");
        insert_exact_v8_frozen_operation(&mut storage, &source, "2026-09-01T11:30:01Z");
        let cutoff: u64 = storage
            .connection
            .query_row(
                "SELECT rowid FROM events WHERE operation_id=?1;",
                [&source.operation_id],
                |row| row_u64(row, 0),
            )
            .unwrap();
        let result = verify_frozen_operation_batch(&storage.connection, cutoff);
        assert_eq!(result.is_ok(), succeeds, "{name}: {result:?}");
    }
}

fn run_bounded_materializer_to_ready(storage: &mut Storage) {
    for _ in 0..256 {
        match storage
            .materialize_lifetime_batch_for_test(&mut crate::storage_lifetime::NeverCancel, None)
            .expect("bounded materialization batch")
        {
            crate::storage_lifetime::LifetimeMaterializationBatchResult::Completed { .. } => return,
            crate::storage_lifetime::LifetimeMaterializationBatchResult::Progress { .. } => {}
            other => panic!("unexpected materialization result: {other:?}"),
        }
    }
    panic!("bounded materializer did not complete");
}

// These fixtures only write isolated temporary databases, never product state.
fn seed_reopen_operations(
    storage: &mut Storage,
    count: usize,
    payload_bytes: usize,
    request_bytes: usize,
) -> Vec<LedgerEventV1> {
    let mut sources = Vec::new();
    for ordinal in 0..count {
        let project_id = format!("project_reopen_{ordinal}");
        let empty =
            canonical_json_v1(json!({"canonical_path":"", "project_id":project_id})).unwrap();
        let mut source = event(
            &format!("evt_reopen_{ordinal}"),
            &format!("op_reopen_{ordinal}"),
            "project",
            &project_id,
            1,
            json!({"canonical_path":format!("{ordinal:08}{}", "x".repeat(payload_bytes - empty.len() - 8)), "project_id":project_id}),
            "2026-09-02T01:00:00Z",
        );
        source.request_id = (request_bytes > 0).then(|| "r".repeat(request_bytes));
        assert_eq!(source.canonical_json_payload.len(), payload_bytes);
        storage.append_event(&source).unwrap();
        sources.push(source);
    }
    for source in &sources {
        insert_exact_v8_frozen_operation(storage, source, "2026-09-02T01:00:01Z");
    }
    let cutoff: i64 = storage
        .connection
        .query_row("SELECT coalesce(max(rowid),0) FROM events;", [], |row| {
            row.get(0)
        })
        .unwrap();
    let proof = json!({"storageCompactionV2PrivateState": {
        "schemaVersion":1,
        "cursor":{"schemaVersion":2,"cutoffEventRowid":cutoff,"phase":"finalize",
                  "afterEventRowid":cutoff,"scanExhausted":true},
        "operationsVerified":true,"equivalenceVerified":true
    }});
    storage.connection.execute(
        "INSERT INTO maintenance_jobs(job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,created_at,updated_at)
         VALUES ('compact_reopen','losslessCompaction','succeeded',?1,?2,?2,?2,?2,?2);",
        params![proof.to_string(), "2026-09-02T01:00:02Z"]).unwrap();
    sources
}

fn mutate_reopen_fixture(
    connection: &Connection,
    trigger_name: &str,
    sql: &str,
    parameters: impl rusqlite::Params,
) {
    let trigger: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1;",
            [trigger_name],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute_batch(&format!("DROP TRIGGER {trigger_name};"))
        .unwrap();
    connection.execute(sql, parameters).unwrap();
    connection.execute_batch(&trigger).unwrap();
}

#[test]
fn frozen_reopen_session_scans_over_one_hundred_without_changing_db_or_product_fences() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    let sources = seed_reopen_operations(&mut storage, 201, 512, 0);
    storage.checkpoint().unwrap();
    let bytes = fs::read(&path).unwrap();
    let snapshot = database_snapshot(&storage.connection);
    let initial_marker: i64 = storage
        .connection
        .query_row("PRAGMA data_version;", [], |row| row.get(0))
        .unwrap();
    {
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        for expected_rows in [100, 100] {
            let FrozenOperationVerificationStep::Pending(usage) =
                verifier.step(&mut || false).unwrap()
            else {
                panic!("pending");
            };
            assert_eq!(usage.rows, expected_rows);
            assert_eq!(usage.canonical_source_bytes, 512 * expected_rows);
            assert!(usage.encoded_closure_bytes <= 8 * 1_048_576);
        }
        assert!(matches!(verifier.step(&mut || false).unwrap(),
        FrozenOperationVerificationStep::FrozenOperationsVerified(usage) if usage.rows == 1));
        // Terminal outcome cannot be queried repeatedly or used as admission.
        assert!(verifier.step(&mut || false).is_err());
    }
    assert!(storage.connection.is_autocommit());
    let marker: i64 = storage
        .connection
        .query_row("PRAGMA data_version;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        marker, initial_marker,
        "same connection's read-only transactions preserve marker"
    );
    assert_eq!(database_snapshot(&storage.connection), snapshot);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    for source in [
        &sources[0],
        &event(
            "evt_new",
            "op_new",
            "project",
            "project_new",
            1,
            json!({"project_id":"project_new","canonical_path":"/fixture/new"}),
            "2026-09-02T01:00:03Z",
        ),
    ] {
        assert_eq!(
            storage.append_event(source).unwrap_err().kind(),
            "storageSchemaMismatch"
        );
    }
    assert_eq!(database_snapshot(&storage.connection), snapshot);
    drop(storage);
    assert_eq!(
        Storage::open(&path).err().unwrap().kind(),
        "storageSchemaMismatch"
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn frozen_reopen_session_exact_hundred_finishes_without_empty_drain_call() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_reopen_operations(&mut storage, 100, 512, 0);
    let mut verifier = FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
    assert!(matches!(verifier.step(&mut || false).unwrap(),
        FrozenOperationVerificationStep::FrozenOperationsVerified(usage) if usage.rows == 100));
}

#[test]
fn frozen_reopen_session_yields_on_canonical_budget_and_rescans_from_zero_after_drop() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_reopen_operations(&mut storage, 3, 600_000, 0);
    {
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        assert!(
            matches!(verifier.step(&mut || false).unwrap(), FrozenOperationVerificationStep::Pending(usage)
        if usage.rows == 1 && usage.canonical_source_bytes == 600_000)
        );
    }
    let mut verifier = FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
    let mut total = 0;
    for ordinal in 0..3 {
        let outcome = verifier.step(&mut || false).unwrap();
        let usage = match outcome {
            FrozenOperationVerificationStep::Pending(usage) if ordinal < 2 => usage,
            FrozenOperationVerificationStep::FrozenOperationsVerified(usage) if ordinal == 2 => {
                usage
            }
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(usage.rows, 1);
        assert_eq!(usage.canonical_source_bytes, 600_000);
        total += usage.canonical_source_bytes;
    }
    assert!(total > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES);
}

#[test]
fn frozen_reopen_session_accepts_exact_mib_and_rejects_single_oversize_terminally() {
    for size in [1_048_576, 1_048_577] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        seed_reopen_operations(&mut storage, 1, size, 0);
        let before = database_snapshot(&storage.connection);
        {
            let mut verifier =
                FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
            let outcome = verifier.step(&mut || false);
            if size == 1_048_576 {
                assert!(
                    matches!(outcome.unwrap(), FrozenOperationVerificationStep::FrozenOperationsVerified(usage)
                if usage.rows == 1 && usage.canonical_source_bytes == size)
                );
            } else {
                assert_eq!(outcome.unwrap_err().kind(), "storageSchemaMismatch");
                assert!(verifier.step(&mut || false).is_err());
            }
        }
        assert_eq!(database_snapshot(&storage.connection), before);
    }
}

#[test]
fn frozen_reopen_session_secondary_budget_yields_but_impossible_single_closure_fails() {
    for request_size in [1_500_000, 3_000_000] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        seed_reopen_operations(&mut storage, 2, 512, request_size);
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        if request_size == 1_500_000 {
            assert!(
                matches!(verifier.step(&mut || false).unwrap(), FrozenOperationVerificationStep::Pending(usage)
                if usage.rows == 1 && usage.canonical_source_bytes == 512 && usage.encoded_closure_bytes > 4_000_000)
            );
            assert!(matches!(verifier.step(&mut || false).unwrap(),
                FrozenOperationVerificationStep::FrozenOperationsVerified(usage) if usage.rows == 1));
        } else {
            assert!(verifier.step(&mut || false).is_err());
            assert!(
                verifier.step(&mut || false).is_err(),
                "no infinite pending retry"
            );
        }
    }
}

#[test]
fn frozen_reopen_session_preflights_all_large_encoded_dependencies_before_decoding() {
    for (table, column, trigger) in [
        (
            "frozen_operation_sources",
            "canonical_record_json",
            "frozen_operation_sources_append_only_update",
        ),
        (
            "frozen_operation_sources",
            "request_id",
            "frozen_operation_sources_append_only_update",
        ),
        (
            "operation_receipts",
            "canonical_payload_json",
            "operation_receipts_compaction_immutable_update",
        ),
        (
            "operation_receipts",
            "event_id",
            "operation_receipts_compaction_immutable_update",
        ),
        (
            "events",
            "canonical_json_payload",
            "events_are_append_only_update",
        ),
        ("events", "request_id", "events_are_append_only_update"),
        (
            "event_source_index",
            "introduced_message_id",
            "event_source_index_append_only_update",
        ),
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        seed_reopen_operations(&mut storage, 1, 512, 0);
        // Multibyte data proves byte length, not SQLite character length, is used.
        let oversized = "한".repeat(3_000_000);
        mutate_reopen_fixture(
            &storage.connection,
            trigger,
            &format!("UPDATE {table} SET {column}=?1;"),
            [&oversized],
        );
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        let error = verifier.step(&mut || false).unwrap_err();
        assert_eq!(
            error.detail(),
            "frozen operation verification session is invalid",
            "{table}.{column}: rejected by length gate"
        );
        assert!(verifier.step(&mut || false).is_err());
    }
}

#[test]
fn frozen_reopen_session_rejects_digest_source_receipt_and_late_corruption() {
    for (count, target) in [(1, 0), (101, 100)] {
        for (table, column, value, trigger) in [
            (
                "frozen_operation_sources",
                "canonical_payload_digest",
                "0".repeat(64),
                "frozen_operation_sources_append_only_update",
            ),
            (
                "frozen_operation_sources",
                "record_digest",
                "0".repeat(64),
                "frozen_operation_sources_append_only_update",
            ),
            (
                "frozen_operation_sources",
                "canonical_record_json",
                "{}".into(),
                "frozen_operation_sources_append_only_update",
            ),
            (
                "operation_receipts",
                "canonical_payload_digest",
                "0".repeat(64),
                "operation_receipts_compaction_immutable_update",
            ),
            (
                "operation_receipts",
                "event_id",
                "evt_corrupt".into(),
                "operation_receipts_compaction_immutable_update",
            ),
            (
                "events",
                "request_id",
                "request_corrupt".into(),
                "events_are_append_only_update",
            ),
            (
                "event_source_index",
                "introduced_message_id",
                "evt_corrupt".into(),
                "event_source_index_append_only_update",
            ),
        ] {
            let (_root, path) = database();
            let mut storage = Storage::open(&path).unwrap();
            seed_reopen_operations(&mut storage, count, 512, 0);
            mutate_reopen_fixture(
                &storage.connection,
                trigger,
                &format!("UPDATE {table} SET {column}=?1 WHERE operation_id=?2;"),
                params![value, format!("op_reopen_{target}")],
            );
            storage.checkpoint().unwrap();
            let bytes = fs::read(&path).unwrap();
            let before = database_snapshot(&storage.connection);
            {
                let mut verifier =
                    FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
                if count > 100 {
                    assert!(
                        matches!(verifier.step(&mut || false).unwrap(), FrozenOperationVerificationStep::Pending(usage) if usage.rows == 100),
                        "{table}.{column} at source {target}: first 100 must verify"
                    );
                }
                assert_eq!(
                    verifier.step(&mut || false).unwrap_err().kind(),
                    "storageSchemaMismatch",
                    "{table}.{column} at source {target}"
                );
                assert!(
                    verifier.step(&mut || false).is_err(),
                    "{table}.{column} at source {target}: no resume"
                );
            }
            assert!(storage.connection.is_autocommit());
            assert_eq!(database_snapshot(&storage.connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}

#[test]
fn frozen_reopen_session_external_commits_invalidate_including_after_terminal() {
    for count in [1, 101] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        seed_reopen_operations(&mut storage, count, 512, 0);
        storage.checkpoint().unwrap();
        drop(storage);
        // Production owns an EXCLUSIVE connection. Cross-connection scenarios
        // use raw, normally locked test connections without changing that policy.
        let mut connection = Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        let external = Connection::open(&path).unwrap();
        external.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        // An external pre-session commit is part of the initial marker, not a
        // false invalidation based on a marker obtained from another connection.
        external
            .execute(
                "UPDATE maintenance_jobs SET updated_at='2026-09-02T01:01:00Z';",
                [],
            )
            .unwrap();
        {
            let mut verifier = FrozenOperationVerificationSession::start(&mut connection).unwrap();
            assert!(verifier.step(&mut || false).is_ok());
            external
                .execute(
                    "UPDATE maintenance_jobs SET updated_at='2026-09-02T01:02:00Z';",
                    [],
                )
                .unwrap();
            assert!(verifier.step(&mut || false).is_err());
            assert!(verifier.step(&mut || false).is_err());
        }
        // New session always starts at zero, even if 100 rows were checked.
        let mut verifier = FrozenOperationVerificationSession::start(&mut connection).unwrap();
        let outcome = verifier.step(&mut || false).unwrap();
        assert!(
            matches!(outcome, FrozenOperationVerificationStep::Pending(usage) if usage.rows == 100)
                || count == 1
        );
    }
}

#[test]
fn frozen_reopen_session_cancellation_at_entry_mid_batch_and_precommit_discards_progress() {
    for cancel_poll in [1, 2, 51, 102] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        seed_reopen_operations(&mut storage, 101, 512, 0);
        storage.checkpoint().unwrap();
        let before = database_snapshot(&storage.connection);
        let bytes = fs::read(&path).unwrap();
        {
            let mut verifier =
                FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
            let mut polls = 0;
            assert_eq!(
                verifier
                    .step(&mut || {
                        polls += 1;
                        polls == cancel_poll
                    })
                    .unwrap(),
                FrozenOperationVerificationStep::Cancelled
            );
            assert!(verifier.step(&mut || false).is_err());
        }
        assert!(storage.connection.is_autocommit());
        assert_eq!(database_snapshot(&storage.connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        assert!(
            matches!(verifier.step(&mut || false).unwrap(), FrozenOperationVerificationStep::Pending(usage) if usage.rows == 100)
        );
    }
}

#[test]
fn frozen_reopen_session_busy_begin_invalidates_and_no_lock_survives_a_call() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_reopen_operations(&mut storage, 101, 512, 0);
    storage.checkpoint().unwrap();
    drop(storage);
    let mut connection = Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    connection.busy_timeout(std::time::Duration::ZERO).unwrap();
    let external = Connection::open(&path).unwrap();
    external.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    let mut verifier = FrozenOperationVerificationSession::start(&mut connection).unwrap();
    assert!(matches!(
        verifier.step(&mut || false).unwrap(),
        FrozenOperationVerificationStep::Pending(_)
    ));
    external
        .execute_batch("BEGIN IMMEDIATE;")
        .expect("no lock retained across calls");
    assert!(verifier.step(&mut || false).is_err());
    external.execute_batch("ROLLBACK;").unwrap();
    assert!(
        verifier.step(&mut || false).is_err(),
        "errors terminally discard progress"
    );
}

#[test]
fn frozen_reopen_session_commit_error_discards_cursor_and_rolls_back() {
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::sync::atomic::{AtomicBool, Ordering};

    // SQLite's real authorizer rejects COMMIT, with no production failure hook.
    unsafe extern "C" fn deny_commit(
        context: *mut c_void,
        action: c_int,
        argument: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
    ) -> c_int {
        // SAFETY: the boxed AtomicBool outlives the connection, SQLite supplies a
        // valid NUL-terminated argument for SQLITE_TRANSACTION, checked for NULL.
        let reject = unsafe { &*context.cast::<AtomicBool>() };
        if action == rusqlite::ffi::SQLITE_TRANSACTION
            && !argument.is_null()
            && unsafe { CStr::from_ptr(argument) }.to_bytes() == b"COMMIT"
            && reject.load(Ordering::Relaxed)
        {
            rusqlite::ffi::SQLITE_DENY
        } else {
            rusqlite::ffi::SQLITE_OK
        }
    }

    let reject = Box::new(AtomicBool::new(false));
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_reopen_operations(&mut storage, 101, 512, 0);
    let before = database_snapshot(&storage.connection);
    // SAFETY: this test exclusively owns the connection; no SQLite API is called
    // from the callback. reject remains allocated until after storage is dropped.
    unsafe {
        assert_eq!(
            rusqlite::ffi::sqlite3_set_authorizer(
                storage.connection.handle(),
                Some(deny_commit),
                (&*reject as *const AtomicBool).cast_mut().cast()
            ),
            rusqlite::ffi::SQLITE_OK
        );
    }
    {
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        assert!(matches!(
            verifier.step(&mut || false).unwrap(),
            FrozenOperationVerificationStep::Pending(_)
        ));
        reject.store(true, Ordering::Relaxed);
        assert!(
            verifier.step(&mut || false).is_err(),
            "terminal COMMIT must fail"
        );
        reject.store(false, Ordering::Relaxed);
        assert!(
            verifier.step(&mut || false).is_err(),
            "cannot resume after completion error"
        );
    }
    assert!(
        storage.connection.is_autocommit(),
        "failed COMMIT was rolled back"
    );
    assert_eq!(database_snapshot(&storage.connection), before);
    {
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        assert!(
            matches!(verifier.step(&mut || false).unwrap(), FrozenOperationVerificationStep::Pending(usage) if usage.rows == 100)
        );
    }
    drop(storage);
}

#[test]
fn frozen_reopen_session_queries_use_keysets_and_exact_dependency_indexes() {
    let (_root, path) = database();
    let storage = Storage::open(&path).unwrap();
    for (query, required_index) in [
        ("SELECT source_event_rowid,length(CAST(canonical_payload_json AS BLOB)) FROM frozen_operation_sources
          WHERE source_event_rowid>?1 ORDER BY source_event_rowid LIMIT 100", "frozen_operation_sources_source_rowid"),
        ("SELECT source_event_rowid FROM frozen_operation_sources WHERE source_event_rowid>?1 ORDER BY source_event_rowid LIMIT 1", "frozen_operation_sources_source_rowid"),
        ("SELECT r.canonical_payload_json FROM operation_receipts r JOIN frozen_operation_sources f ON r.operation_id=f.operation_id
          WHERE f.source_event_rowid=?1 LIMIT 1", "sqlite_autoindex_operation_receipts_1"),
        ("SELECT canonical_json_payload FROM events WHERE rowid=?1 LIMIT 1", "INTEGER PRIMARY KEY"),
        ("SELECT event_id FROM event_source_index WHERE event_rowid=?1 LIMIT 1", "INTEGER PRIMARY KEY"),
    ] {
        let plans: Vec<String> = storage.connection.prepare(&format!("EXPLAIN QUERY PLAN {query}")).unwrap()
            .query_map([1_i64], |row| row.get(3)).unwrap().map(Result::unwrap).collect();
        assert!(plans.iter().any(|plan| plan.contains(required_index)), "{query}: {plans:?}");
        assert!(plans.iter().all(|plan| !plan.contains("SCAN ") && !plan.contains("TEMP B-TREE")), "{query}: {plans:?}");
    }
}

#[test]
fn frozen_reopen_session_rejects_nested_transaction_and_empty_or_out_of_cutoff_sources() {
    for count in [0, 1] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        seed_reopen_operations(&mut storage, count, 512, 0);
        storage.connection.execute_batch("BEGIN;").unwrap();
        assert!(FrozenOperationVerificationSession::start(&mut storage.connection).is_err());
        assert!(
            !storage.connection.is_autocommit(),
            "caller transaction untouched"
        );
        storage.connection.execute_batch("ROLLBACK;").unwrap();
        if count == 1 {
            // A valid-shaped proof with cutoff zero must not hide row one.
            storage
                .connection
                .execute(
                    "UPDATE maintenance_jobs SET cutoff_json=json_set(cutoff_json,
                '$.storageCompactionV2PrivateState.cursor.cutoffEventRowid',0,
                '$.storageCompactionV2PrivateState.cursor.afterEventRowid',0);",
                    [],
                )
                .unwrap();
        }
        let mut verifier =
            FrozenOperationVerificationSession::start(&mut storage.connection).unwrap();
        assert!(verifier.step(&mut || false).is_err());
    }
}

#[test]
fn frozen_reopen_session_preflight_rejects_schema_pair_active_and_nonunique_proof() {
    for mutation in [
        "DROP INDEX frozen_operation_sources_aggregate;",
        "UPDATE storage_schema_meta SET materialization_state='failed';",
        "PRAGMA ignore_check_constraints=ON;
         INSERT INTO storage_schema_meta SELECT 2,storage_schema_version,protocol_version,migration_state,materialization_state,migrated_at FROM storage_schema_meta;
         PRAGMA ignore_check_constraints=OFF;",
        "UPDATE maintenance_jobs SET state='pending',completed_at=NULL;",
        "UPDATE maintenance_jobs SET cutoff_json='{}';",
        "INSERT INTO maintenance_jobs(job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,created_at,updated_at)
         SELECT 'second_proof',job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,created_at,updated_at FROM maintenance_jobs;",
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        seed_reopen_operations(&mut storage, 1, 512, 0);
        storage.connection.execute_batch(mutation).unwrap();
        let before = database_snapshot(&storage.connection);
        assert!(FrozenOperationVerificationSession::start(&mut storage.connection).is_err(), "{mutation}");
        assert!(storage.connection.is_autocommit());
        assert_eq!(database_snapshot(&storage.connection), before);
    }
}

#[test]
fn exact_v6_reopen_rejects_partial_extra_tampered_and_non_runnable_state() {
    for (name, mutation) in [
        ("partial", "DROP TABLE event_source_index;"),
        (
            "extra",
            "CREATE TABLE unexpected_storage_v6(value TEXT) STRICT;",
        ),
        (
            "trigger_tamper",
            "DROP TRIGGER event_source_index_requires_exact_event;",
        ),
        (
            "index_tamper",
            "DROP INDEX event_source_index_message_unique;",
        ),
    ] {
        let (_root, path) = database();
        let storage = Storage::open_v6_for_test(&path).expect("fresh v6");
        storage.connection.execute_batch(mutation).unwrap();
        drop(storage);
        let error = Storage::open(&path).err().expect("non-exact v6 must fail");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    }

    for state in ["attempting", "failed"] {
        let (_root, path) = database();
        let storage = Storage::open_v6_for_test(&path).expect("fresh v6");
        let (failure_kind, completed_at) = if state == "failed" {
            (Some("database"), Some("2026-08-28T13:02:00Z"))
        } else {
            (None, None)
        };
        insert_v4_job(
            &storage.connection,
            state,
            state,
            "indexSource",
            20,
            0,
            0,
            failure_kind,
            completed_at,
        );
        verify_schema_v6(&storage.connection).expect("structural v6 retains evidence");
        drop(storage);
        let error = Storage::open(&path)
            .err()
            .expect("non-runnable v6 must fail open");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{state}");
        assert_eq!(
            error.detail(),
            "storage lifetime materialization state is invalid"
        );
    }
}

#[test]
fn exact_v7_reopen_rejects_schema_tamper_and_active_compaction_without_mutation() {
    for (name, mutation) in [
        ("partial", "DROP TABLE frozen_message_sources;"),
        (
            "extra",
            "CREATE TABLE unexpected_storage_v7(value TEXT) STRICT;",
        ),
        (
            "trigger_tamper",
            "DROP TRIGGER frozen_message_sources_require_exact_source;",
        ),
        (
            "index_tamper",
            "DROP INDEX ledger_compaction_prefixes_latest;",
        ),
    ] {
        let (_root, path) = database();
        let storage = Storage::open_v7_for_test(&path).expect("fresh v7");
        storage.connection.execute_batch(mutation).unwrap();
        drop(storage);
        let error = Storage::open(&path).err().expect("non-exact v7 must fail");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    }

    for state in ["pending", "running"] {
        let (_root, path) = database();
        let storage = Storage::open_v7_for_test(&path).expect("fresh v7");
        storage
            .connection
            .execute(
                "INSERT INTO maintenance_jobs(\
                   job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
                   failure_kind,created_at,updated_at\
                 ) VALUES (?1,'losslessCompaction',?2,NULL,NULL,NULL,NULL,NULL,\
                   '2026-08-28T13:00:00Z','2026-08-28T13:00:00Z');",
                params![format!("active_{state}"), state],
            )
            .unwrap();
        let before = database_snapshot(&storage.connection);
        drop(storage);
        let error = Storage::open(&path)
            .err()
            .expect("active compaction must fail closed");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{state}");
        let connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        assert_eq!(database_snapshot(&connection), before, "{state}");
    }
}

#[test]
fn v7_rejects_durable_compaction_authority_and_frozen_sources_before_c1b() {
    let (_root, path) = database();
    let mut storage = Storage::open_v7_for_test(&path).expect("fresh v7");
    for sequence in 1..=2 {
        storage
            .append_event(&event(
                &format!("evt_authority_{sequence}"),
                &format!("op_authority_{sequence}"),
                "project",
                "project_authority",
                sequence,
                json!({
                    "canonical_path":"/fixture/authority",
                    "project_id":"project_authority"
                }),
                &format!("2026-08-28T14:00:0{sequence}Z"),
            ))
            .unwrap();
    }
    storage
        .connection
        .execute_batch(
            "INSERT INTO ledger_baselines(\
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at\
             ) VALUES ('baseline_authority','project','project_authority',1,1,'{}',\
               'digest_authority','2026-08-28T14:01:00Z','2026-08-28T14:01:00Z');\
             INSERT INTO ledger_compaction_prefixes(\
               prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,\
               anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,\
               baseline_projection_digest,created_at,verified_at\
             ) SELECT 'prefix_authority','project','project_authority',1,event_rowid,\
               event_id,operation_id,aggregate_sequence,'baseline_authority','digest_authority',\
               '2026-08-28T14:01:00Z','2026-08-28T14:01:00Z'\
               FROM event_source_index WHERE event_id='evt_authority_2';\
             INSERT INTO maintenance_jobs(\
               job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
               failure_kind,created_at,updated_at\
             ) VALUES ('authority_job','losslessCompaction','running','{}',\
               '2026-08-28T14:02:00Z',NULL,NULL,NULL,\
               '2026-08-28T14:02:00Z','2026-08-28T14:02:00Z');\
             INSERT INTO ledger_compaction_delete_authority(\
               job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,\
               aggregate_id,aggregate_sequence,authorized_at\
             ) SELECT 'authority_job',1,'prefix_authority',event_rowid,event_id,operation_id,\
               aggregate_type,aggregate_id,aggregate_sequence,'2026-08-28T14:02:00Z'\
               FROM event_source_index WHERE event_id='evt_authority_1';\
             UPDATE maintenance_jobs SET state='succeeded',\
               completed_at='2026-08-28T14:03:00Z',updated_at='2026-08-28T14:03:00Z'\
               WHERE job_id='authority_job';",
        )
        .unwrap();
    verify_schema_v7(&storage.connection).expect("durable authority retains exact v7 schema");
    let events_before = table_rows(&storage.connection, "events");
    let authority_before = table_rows(&storage.connection, "ledger_compaction_delete_authority");
    let error = storage
        .append_event(&event(
            "evt_authority_3",
            "op_authority_3",
            "project",
            "project_authority",
            3,
            json!({
                "canonical_path":"/fixture/authority",
                "project_id":"project_authority"
            }),
            "2026-08-28T14:00:03Z",
        ))
        .expect_err("C1A must reject append against durable compaction history");
    assert_eq!(error.kind(), "storageSchemaMismatch");
    assert_eq!(table_rows(&storage.connection, "events"), events_before);
    assert_eq!(
        table_rows(&storage.connection, "ledger_compaction_delete_authority"),
        authority_before
    );
    drop(storage);
    assert_eq!(
        Storage::open(&path)
            .err()
            .expect("durable authority must reject reopen")
            .kind(),
        "storageSchemaMismatch"
    );

    let (_root, frozen_path) = database();
    let mut frozen = Storage::open_v7_for_test(&frozen_path).expect("fresh v7");
    frozen
        .append_event(&event(
            "evt_frozen_project",
            "op_frozen_project",
            "project",
            "project_frozen",
            1,
            json!({"canonical_path":"/fixture/frozen","project_id":"project_frozen"}),
            "2026-08-28T14:10:00Z",
        ))
        .unwrap();
    frozen
        .append_event(&event(
            "evt_frozen_chat",
            "op_frozen_chat",
            "chat",
            "chat_frozen",
            1,
            v6_chat_payload(
                "chat_frozen",
                "project_frozen",
                1,
                team_message("chat_frozen", "message_frozen", 1),
            ),
            "2026-08-28T14:10:01Z",
        ))
        .unwrap();
    frozen
        .connection
        .execute(
            "INSERT INTO chat_messages(\
               message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at\
             ) VALUES ('message_frozen','chat_frozen',1,?1,'digest_frozen',\
               '2026-08-28T14:10:01Z','2026-08-28T14:10:02Z');",
            [canonical_json_v1(team_message("chat_frozen", "message_frozen", 1)).unwrap()],
        )
        .unwrap();
    assert_eq!(
        frozen
            .connection
            .execute(
            "INSERT INTO frozen_message_sources(
               message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,
               source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at
             ) SELECT message.message_id,source.event_rowid,source.event_id,source.operation_id,
               message.chat_id,source.aggregate_sequence,message.event_sequence,
               message.canonical_message_json,message.message_digest,'2026-08-28T14:11:00Z'
               FROM event_source_index AS source
               JOIN chat_messages AS message ON message.message_id=source.introduced_message_id
               WHERE source.event_id='evt_frozen_chat';",
                [],
            )
            .unwrap(),
        1,
        "the exact materialized message source must be frozen"
    );
    let frozen_before = database_snapshot(&frozen.connection);
    let error = frozen
        .append_event(&event(
            "evt_frozen_other",
            "op_frozen_other",
            "project",
            "project_other",
            1,
            json!({"canonical_path":"/fixture/other","project_id":"project_other"}),
            "2026-08-28T14:12:00Z",
        ))
        .expect_err("any frozen provenance is not runnable before C1B");
    assert_eq!(error.kind(), "storageSchemaMismatch");
    assert_eq!(database_snapshot(&frozen.connection), frozen_before);
    drop(frozen);
    assert_eq!(
        Storage::open(&frozen_path)
            .err()
            .expect("frozen provenance must reject reopen")
            .kind(),
        "storageSchemaMismatch"
    );
}

#[test]
fn v6_append_indexes_message_introduction_and_repeated_snapshot_transitions() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("fresh v6");
    storage
        .connection
        .execute(
            "UPDATE storage_schema_meta SET materialization_state='ready' WHERE singleton=1;",
            [],
        )
        .unwrap();
    storage
        .append_event(&event(
            "evt_v6_project",
            "op_v6_project",
            "project",
            "project_v6",
            1,
            json!({"canonical_path":"/fixture/v6","project_id":"project_v6"}),
            "2026-08-28T13:19:59Z",
        ))
        .unwrap();
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "pending"
    );
    insert_v4_job(
        &storage.connection,
        "append_preserves_v6_job",
        "pending",
        "indexSource",
        1,
        0,
        0,
        None,
        None,
    );
    let job_before = table_rows(&storage.connection, "lifetime_materialization_job");

    let chat_events = [
        event(
            "evt_v6_chat_1",
            "op_v6_chat_1",
            "chat",
            "chat_v6",
            1,
            v6_chat_payload(
                "chat_v6",
                "project_v6",
                1,
                team_message("chat_v6", "message_a", 1),
            ),
            "2026-08-28T13:20:00Z",
        ),
        event(
            "evt_v6_chat_2",
            "op_v6_chat_2",
            "chat",
            "chat_v6",
            2,
            v6_chat_payload(
                "chat_v6",
                "project_v6",
                1,
                team_message("chat_v6", "message_a", 1),
            ),
            "2026-08-28T13:20:01Z",
        ),
        event(
            "evt_v6_chat_3",
            "op_v6_chat_3",
            "chat",
            "chat_v6",
            3,
            v6_chat_payload("chat_v6", "project_v6", 1, Value::Null),
            "2026-08-28T13:20:02Z",
        ),
        event(
            "evt_v6_chat_4",
            "op_v6_chat_4",
            "chat",
            "chat_v6",
            4,
            v6_chat_payload(
                "chat_v6",
                "project_v6",
                2,
                team_message("chat_v6", "message_b", 2),
            ),
            "2026-08-28T13:20:03Z",
        ),
    ];
    for event in &chat_events {
        assert_eq!(
            storage.append_event(event).unwrap(),
            AppendOutcome::Committed
        );
    }
    assert_eq!(
        table_rows(&storage.connection, "lifetime_materialization_job"),
        job_before,
        "append must preserve the pending job evidence and cursor"
    );

    let rows: Vec<ChatSourceIndexRow> = storage
        .connection
        .prepare(
            "SELECT aggregate_sequence,introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE aggregate_type='chat' ORDER BY aggregate_sequence;",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                1,
                Some("message_a".to_string()),
                Some(1),
                Some("message_a".to_string())
            ),
            (2, None, Some(1), Some("message_a".to_string())),
            (3, None, Some(1), None),
            (
                4,
                Some("message_b".to_string()),
                Some(2),
                Some("message_b".to_string())
            ),
        ]
    );
    let project_fields: (Option<String>, Option<i64>, Option<String>) = storage
        .connection
        .query_row(
            "SELECT introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE event_id='evt_v6_project';",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(project_fields, (None, None, None));
}

#[test]
fn v6_chat_append_rejects_missing_predecessor_and_invalid_provenance_atomically() {
    let (_root, path) = database();
    let mut storage = populated_v5_storage(&path, "pending");
    migrate_v6(&mut storage.connection).expect("v5 to v6");
    let events_before = table_rows(&storage.connection, "events");
    let error = storage
        .append_event(&event(
            "evt_missing_predecessor",
            "op_missing_predecessor",
            "chat",
            "chat_v2",
            2,
            v6_chat_payload(
                "chat_v2",
                "project_v2",
                1,
                team_message("chat_v2", "message_after_migration", 1),
            ),
            "2026-08-28T13:30:00Z",
        ))
        .expect_err("raw predecessor event must not substitute for exact source index");
    assert_eq!(error.kind(), "storageContractViolation");
    assert_eq!(table_rows(&storage.connection, "events"), events_before);
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM event_source_index;", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );

    for (name, payload) in [
        (
            "wrong_chat",
            v6_chat_payload(
                "chat_invalid",
                "project_v2",
                1,
                team_message("different_chat", "message_wrong_chat", 1),
            ),
        ),
        (
            "wrong_sequence",
            v6_chat_payload(
                "chat_invalid",
                "project_v2",
                1,
                team_message("chat_invalid", "message_wrong_sequence", 2),
            ),
        ),
    ] {
        let (_root, path) = database();
        let mut fresh = Storage::open(&path).expect("fresh v6");
        let error = fresh
            .append_event(&event(
                &format!("evt_{name}"),
                &format!("op_{name}"),
                "chat",
                "chat_invalid",
                1,
                payload,
                "2026-08-28T13:30:01Z",
            ))
            .expect_err("invalid TeamMessage provenance must fail");
        assert_eq!(error.kind(), "storageContractViolation", "{name}");
        assert_eq!(
            fresh
                .connection
                .query_row("SELECT count(*) FROM events;", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0,
            "{name}"
        );
        assert_eq!(
            fresh
                .connection
                .query_row("SELECT count(*) FROM event_source_index;", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0,
            "{name}"
        );
    }
}

#[test]
fn v6_chat_sequence_one_uses_synthetic_zero_and_predecessor_tamper_fails_in_rust() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("fresh v6");
    storage
        .append_event(&event(
            "evt_zero_project",
            "op_zero_project",
            "project",
            "project_zero",
            1,
            json!({"canonical_path":"/fixture/zero","project_id":"project_zero"}),
            "2026-08-28T13:34:59Z",
        ))
        .unwrap();
    let zero = event(
        "evt_zero_chat_1",
        "op_zero_chat_1",
        "chat",
        "chat_zero",
        1,
        v6_chat_payload("chat_zero", "project_zero", 0, Value::Null),
        "2026-08-28T13:35:00Z",
    );
    assert_eq!(
        storage.append_event(&zero).unwrap(),
        AppendOutcome::Committed
    );
    let zero_index: (Option<String>, Option<i64>, Option<String>) = storage
        .connection
        .query_row(
            "SELECT introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE event_id='evt_zero_chat_1';",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(zero_index, (None, Some(0), None));

    let introduced = event(
        "evt_zero_chat_2",
        "op_zero_chat_2",
        "chat",
        "chat_zero",
        2,
        v6_chat_payload(
            "chat_zero",
            "project_zero",
            1,
            team_message("chat_zero", "message_zero", 1),
        ),
        "2026-08-28T13:35:01Z",
    );
    assert_eq!(
        storage.append_event(&introduced).unwrap(),
        AppendOutcome::Committed
    );
    storage
        .connection
        .execute_batch(
            "DROP TRIGGER event_source_index_append_only_update;\
             UPDATE event_source_index SET chat_snapshot_message_id='tampered' \
             WHERE event_id='evt_zero_chat_2';",
        )
        .unwrap();
    let events_before = table_rows(&storage.connection, "events");
    let error = storage
        .append_event(&event(
            "evt_zero_chat_3",
            "op_zero_chat_3",
            "chat",
            "chat_zero",
            3,
            v6_chat_payload(
                "chat_zero",
                "project_zero",
                1,
                team_message("chat_zero", "message_zero", 1),
            ),
            "2026-08-28T13:35:02Z",
        ))
        .expect_err("Rust predecessor provenance must reject a corrupted index row");
    assert_eq!(error.kind(), "storageContractViolation");
    assert_eq!(table_rows(&storage.connection, "events"), events_before);
}

#[test]
fn v6_chat_append_rejects_gap_changed_snapshot_and_duplicate_message_identity() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("fresh v6");
    storage
        .append_event(&event(
            "evt_transition_project",
            "op_transition_project",
            "project",
            "project_transition",
            1,
            json!({
                "canonical_path":"/fixture/transition",
                "project_id":"project_transition"
            }),
            "2026-08-28T13:39:59Z",
        ))
        .unwrap();
    let first = event(
        "evt_transition_1",
        "op_transition_1",
        "chat",
        "chat_transition",
        1,
        v6_chat_payload(
            "chat_transition",
            "project_transition",
            1,
            team_message("chat_transition", "message_transition", 1),
        ),
        "2026-08-28T13:40:00Z",
    );
    assert_eq!(
        storage.append_event(&first).unwrap(),
        AppendOutcome::Committed
    );
    let rows_before = table_rows(&storage.connection, "event_source_index");

    for (name, durable, message_id, message_sequence) in [
        ("gap", 3, "message_gap", 3),
        ("changed_snapshot", 1, "message_changed", 1),
    ] {
        let error = storage
            .append_event(&event(
                &format!("evt_{name}"),
                &format!("op_{name}"),
                "chat",
                "chat_transition",
                2,
                v6_chat_payload(
                    "chat_transition",
                    "project_transition",
                    durable,
                    team_message("chat_transition", message_id, message_sequence),
                ),
                "2026-08-28T13:40:01Z",
            ))
            .expect_err("invalid transition must fail");
        assert_eq!(error.kind(), "storageContractViolation", "{name}");
        assert_eq!(
            table_rows(&storage.connection, "event_source_index"),
            rows_before,
            "{name}"
        );
    }

    let duplicate = first.clone();
    assert_eq!(
        storage.append_event(&duplicate).unwrap(),
        AppendOutcome::DuplicateNoOp
    );
    let mut conflict = first;
    conflict.canonical_json_payload = canonical_json_v1(v6_chat_payload(
        "chat_transition",
        "project_transition",
        1,
        Value::Null,
    ))
    .unwrap();
    assert_eq!(
        storage.append_event(&conflict).unwrap(),
        AppendOutcome::Conflict
    );
    assert_eq!(
        table_rows(&storage.connection, "event_source_index"),
        rows_before
    );

    let identity_conflict = event(
        "evt_identity_conflict",
        "op_identity_conflict",
        "chat",
        "chat_other",
        1,
        v6_chat_payload(
            "chat_other",
            "project_other",
            1,
            team_message("chat_other", "message_transition", 1),
        ),
        "2026-08-28T13:40:02Z",
    );
    let error = storage
        .append_event(&identity_conflict)
        .expect_err("message introduction identity must be globally unique");
    assert_eq!(error.kind(), "databaseFailure");
    assert_eq!(
        table_rows(&storage.connection, "event_source_index"),
        rows_before
    );
}

#[test]
fn v6_injected_append_failure_rolls_back_event_index_projection_and_readiness() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("fresh v6");
    storage
        .connection
        .execute(
            "UPDATE storage_schema_meta SET materialization_state='ready' WHERE singleton=1;",
            [],
        )
        .unwrap();
    let project = event(
        "evt_v6_rollback",
        "op_v6_rollback",
        "project",
        "project_v6_rollback",
        1,
        json!({"canonical_path":"/fixture/v6","project_id":"project_v6_rollback"}),
        "2026-08-28T13:50:00Z",
    );
    let error = storage
        .append_event_with_failure(
            &project,
            Some(AppendFailurePoint::AfterEventInsertBeforeProjection),
        )
        .expect_err("injected failure must roll back all append state");
    assert_eq!(error.kind(), "injectedFailure");
    for table in ["events", "event_source_index", "projects"] {
        assert_eq!(
            storage
                .connection
                .query_row(&format!("SELECT count(*) FROM {table};"), [], |row| {
                    row.get::<_, u32>(0)
                })
                .unwrap(),
            0,
            "{table}"
        );
    }
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "ready"
    );
}

#[test]
fn every_storage_connection_enables_foreign_keys_before_use() {
    let (_root, path) = database();
    let storage = Storage::open(&path).expect("first connection");
    let enabled: u32 = storage
        .connection
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .expect("foreign keys");
    assert_eq!(enabled, 1);
    drop(storage);

    let raw = Connection::open(&path).expect("raw second connection");
    raw.execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("disable foreign keys on raw connection");
    let raw_enabled: u32 = raw
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .expect("foreign keys");
    assert_eq!(
        raw_enabled, 0,
        "test precondition: raw connection is unsafe"
    );
    drop(raw);

    let storage = Storage::open(&path).expect("verified second connection");
    let enabled: u32 = storage
        .connection
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .expect("foreign keys");
    assert_eq!(enabled, 1);
}

#[test]
fn append_projection_is_atomic_and_idempotency_compares_full_immutable_record() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    let mut first = event(
        "evt_run_01",
        "op_run_01",
        "run",
        "run_01",
        1,
        json!({"run_id":"run_01","state":"preparing","worktree_path":"/fixture/worktree"}),
        "2026-08-25T06:00:00Z",
    );
    first.request_id = Some("req_01".to_string());
    assert_eq!(
        storage.append_event(&first).unwrap(),
        AppendOutcome::Committed
    );

    let mut duplicate = first.clone();
    duplicate.request_id = Some("req_after_restart".to_string());
    assert_eq!(
        storage.append_event(&duplicate).unwrap(),
        AppendOutcome::DuplicateNoOp
    );
    let mut conflict = duplicate.clone();
    conflict.canonical_json_payload = canonical_json_v1(json!({
        "run_id":"run_01","state":"completed","worktree_path":"/fixture/worktree"
    }))
    .unwrap();
    assert_eq!(
        storage.append_event(&conflict).unwrap(),
        AppendOutcome::Conflict
    );

    let mut second = event(
        "evt_run_02",
        "op_run_02",
        "run",
        "run_01",
        2,
        json!({"run_id":"run_01","state":"running","worktree_path":"/fixture/worktree"}),
        "2026-08-25T06:00:01Z",
    );
    second.request_id = Some("req_02".to_string());
    let error = storage
        .append_event_with_failure(
            &second,
            Some(AppendFailurePoint::AfterEventInsertBeforeProjection),
        )
        .expect_err("injected rollback");
    assert_eq!(error.kind(), "injectedFailure");
    let events: u32 = storage
        .connection
        .query_row("SELECT count(*) FROM events;", [], |row| row.get(0))
        .unwrap();
    let state: String = storage
        .connection
        .query_row("SELECT state FROM runs WHERE run_id='run_01';", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(events, 1);
    assert_eq!(state, "preparing");
}

#[test]
fn projection_tamper_prevents_reopen() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    let first = event(
        "evt_run_tamper",
        "op_run_tamper",
        "run",
        "run_tamper",
        1,
        json!({"run_id":"run_tamper","state":"preparing"}),
        "2026-08-25T06:10:00Z",
    );
    storage.append_event(&first).expect("append run");
    storage
        .connection
        .execute(
            r#"UPDATE runs SET projection_json='{"run_id":"run_tamper","state":"completed"}' WHERE run_id='run_tamper';"#,
            [],
        )
        .expect("tamper projection");
    drop(storage);

    let error = Storage::open(&path).err().expect("tamper must fail closed");
    assert_eq!(error.kind(), "projectionMismatch");
}

#[test]
fn unclean_recovery_is_persisted_once_and_locks_remain_blockers() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    storage
        .register_app_instance(&ready("sidecar_previous"))
        .expect("previous instance");
    storage
        .append_event(&event(
            "evt_run_active",
            "op_run_active",
            "run",
            "run_active",
            1,
            json!({"run_id":"run_active","state":"running","worktree_path":"/fixture/worktree"}),
            "2026-08-25T06:20:01Z",
        ))
        .expect("active run");
    storage
        .append_event(&event(
            "evt_lock_active",
            "op_lock_active",
            "lock",
            "lock_active",
            1,
            json!({
                "evidence":{},"lock_id":"lock_active","poisoned":false,
                "recovery_required":false,"worktree_path":"/fixture/worktree",
                "writer_run_id":"run_active"
            }),
            "2026-08-25T06:20:02Z",
        ))
        .expect("active lock");
    drop(storage);

    let mut storage = Storage::open(&path).expect("restart storage");
    storage.recover_unclean_shutdown().expect("recover once");
    let after_first: u32 = storage
        .connection
        .query_row("SELECT count(*) FROM events;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        storage.blocker_ids().unwrap(),
        vec!["lock:lock_active".to_string(), "run:run_active".to_string()]
    );
    storage
        .recover_unclean_shutdown()
        .expect("recover idempotently");
    let after_second: u32 = storage
        .connection
        .query_row("SELECT count(*) FROM events;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(after_first, after_second);
    let run_state: String = storage
        .connection
        .query_row(
            "SELECT state FROM runs WHERE run_id='run_active';",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let lock_recovery: bool = storage
        .connection
        .query_row(
            "SELECT recovery_required FROM locks WHERE lock_id='lock_active';",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(run_state, "recoveryRequired");
    assert!(lock_recovery);
}

#[test]
fn poisoned_lock_survives_clean_shutdown_and_recovery() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    storage
        .register_app_instance(&ready("sidecar_clean"))
        .expect("instance");
    storage
        .append_event(&event(
            "evt_lock_poison",
            "op_lock_poison",
            "lock",
            "lock_poison",
            1,
            json!({
                "evidence":{"detail":"termination unknown"},"lock_id":"lock_poison",
                "poisoned":true,"recovery_required":false,
                "worktree_path":"/fixture/worktree","writer_run_id":null
            }),
            "2026-08-25T06:30:00Z",
        ))
        .expect("poisoned lock");
    storage.checkpoint().expect("checkpoint");
    storage
        .mark_app_instance_clean("sidecar_clean")
        .expect("clean marker");
    drop(storage);

    let mut storage = Storage::open(&path).expect("restart");
    storage.recover_unclean_shutdown().expect("clean recovery");
    assert_eq!(
        storage.blocker_ids().unwrap(),
        vec!["lock:lock_poison".to_string()]
    );
}

#[cfg(unix)]
#[test]
fn database_symlink_is_rejected_without_touching_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target.sqlite3");
    fs::write(&target, b"not a database").unwrap();
    let link = root.path().join("hello-agent.sqlite3");
    symlink(&target, &link).unwrap();
    let error = Storage::open(&link).err().expect("symlink rejected");
    assert_eq!(error.kind(), "databasePathInvalid");
    assert_eq!(fs::read(&target).unwrap(), b"not a database");
}

fn drop_lifetime_schema(connection: &Connection) {
    connection
        .execute_batch(
            "DROP TRIGGER events_consume_exact_v7_delete_authority;
             DROP TRIGGER events_are_append_only_delete;
             DROP TABLE frozen_operation_sources;
             DROP TABLE ledger_compaction_delete_authority;
             DROP TABLE frozen_message_sources;
             DROP TABLE ledger_compaction_prefixes;
             DROP TABLE event_source_index;
             DROP TABLE lifetime_materialization_job;
             DROP TABLE maintenance_jobs;
             DROP TABLE prepared_workspaces;
             DROP TABLE chat_messages;
             DROP TABLE operation_receipts;
             DROP TABLE ledger_baselines;
             DROP TABLE storage_schema_meta;
             DROP INDEX chats_identity_project;",
        )
        .expect("remove additive v2 objects for v1 migration fixture");
    let v1_delete_trigger = reference_schema_v1()
        .expect("reference v1 schema")
        .into_iter()
        .find_map(|(kind, name, _, sql)| {
            (kind == "trigger" && name == "events_are_append_only_delete").then_some(sql)
        })
        .flatten()
        .expect("reference v1 delete trigger");
    connection
        .execute_batch(&v1_delete_trigger)
        .expect("restore exact v1 delete trigger");
}

fn populated_v2_storage(path: &Path, state: &str) -> Storage {
    let mut connection = Connection::open(path).expect("open v2 fixture database");
    configure_connection(&connection).expect("configure v2 fixture database");
    migrate(&mut connection, MIGRATION_V1).expect("apply v1 fixture");
    let mut storage = Storage {
        connection,
        path: path.to_path_buf(),
    };
    storage
        .append_event(&event(
            "evt_project_v2",
            "op_project_v2",
            "project",
            "project_v2",
            1,
            json!({"canonical_path":"/fixture/project-v2","project_id":"project_v2"}),
            "2026-08-28T06:00:00Z",
        ))
        .unwrap();
    storage
        .append_event(&event(
            "evt_chat_v2",
            "op_chat_v2",
            "chat",
            "chat_v2",
            1,
            json!({"chat_id":"chat_v2","messages":[],"project_id":"project_v2"}),
            "2026-08-28T06:00:01Z",
        ))
        .unwrap();
    storage.connection.execute_batch(MIGRATION_V2).unwrap();
    storage
        .connection
        .execute_batch(
            "INSERT INTO ledger_baselines(\
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at\
             ) VALUES ('baseline_v2','project','project_v2',1,1,'{}','baseline-digest','2026-08-28T06:01:00Z','2026-08-28T06:01:00Z');\
             INSERT INTO operation_receipts(\
               operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,\
               payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,\
               canonical_payload_digest_version,canonical_payload_digest_format,canonical_payload_digest,\
               event_timestamp,materialized_at\
             ) VALUES ('receipt_op_v2','receipt_evt_v2','project','receipt_project_v2',1,\
               'project.upserted',1,'{}','sha256',1,'lowercaseHex64',\
               '0000000000000000000000000000000000000000000000000000000000000000',\
               '2026-08-28T06:01:00Z','2026-08-28T06:01:00Z');\
             INSERT INTO chat_messages(\
               message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at\
             ) VALUES ('message_v2','chat_v2',1,'{}','message-digest','2026-08-28T06:01:00Z','2026-08-28T06:01:00Z');\
             INSERT INTO prepared_workspaces(\
               preparation_id,project_id,worktree_id,workspace_identity,binding_state,bound_chat_id,\
               canonical_record_json,record_digest,created_at,updated_at,materialized_at\
             ) VALUES ('preparation_v2','project_v2','worktree_v2','workspace_v2','bound','chat_v2',\
               '{}','workspace-digest','2026-08-28T06:01:00Z','2026-08-28T06:01:00Z','2026-08-28T06:01:00Z');\
             INSERT INTO maintenance_jobs(\
               job_id,job_kind,state,completed_at,last_successful_at,created_at,updated_at\
             ) VALUES ('compaction_v2','losslessCompaction','failed','2026-08-28T06:02:00Z',\
               '2026-08-28T06:01:30Z','2026-08-28T06:01:00Z','2026-08-28T06:02:00Z');",
        )
        .unwrap();
    storage
        .connection
        .execute(
            "UPDATE storage_schema_meta SET materialization_state=?1 WHERE singleton=1;",
            [state],
        )
        .unwrap();
    if matches!(state, "pending" | "ready") {
        verify_schema_v2(&storage.connection).expect("exact populated v2 fixture");
    }
    storage
}

fn populated_v3_storage(path: &Path, state: &str) -> Storage {
    let mut storage = populated_v2_storage(path, state);
    migrate_v3(&mut storage.connection).expect("migrate populated fixture to v3");
    verify_schema_v3(&storage.connection).expect("exact populated v3 fixture");
    storage
}

fn populated_v4_storage(path: &Path, state: &str) -> Storage {
    let mut storage = populated_v3_storage(path, state);
    migrate_v4(&mut storage.connection).expect("migrate populated fixture to v4");
    verify_schema_v4(&storage.connection).expect("exact populated v4 fixture");
    storage
}

fn populated_v5_storage(path: &Path, state: &str) -> Storage {
    let mut storage = populated_v4_storage(path, state);
    migrate_v5(&mut storage.connection).expect("migrate populated fixture to v5");
    verify_schema_v5(&storage.connection).expect("exact populated v5 fixture");
    storage
}

fn populated_v6_storage(path: &Path, state: &str) -> Storage {
    let mut storage = populated_v5_storage(path, state);
    migrate_v6(&mut storage.connection).expect("migrate populated fixture to v6");
    verify_schema_v6(&storage.connection).expect("exact populated v6 fixture");
    storage
}

fn populated_v7_storage(path: &Path, state: &str) -> Storage {
    let mut storage = populated_v6_storage(path, state);
    migrate_v7(&mut storage.connection).expect("migrate populated fixture to v7");
    verify_schema_v7(&storage.connection).expect("exact populated v7 fixture");
    storage
}

fn seed_v7_prefix_for_v8(storage: &mut Storage) {
    for sequence in 1..=2 {
        storage
            .append_event(&event(
                &format!("evt_v8_prefix_{sequence}"),
                &format!("op_v8_prefix_{sequence}"),
                "project",
                "project_v8_prefix",
                sequence,
                json!({
                    "canonical_path":"/fixture/v8-prefix",
                    "project_id":"project_v8_prefix"
                }),
                &format!("2026-09-01T04:10:0{sequence}Z"),
            ))
            .unwrap();
    }
    storage
        .connection
        .execute_batch(
            "INSERT INTO ledger_baselines(
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at
             ) VALUES ('baseline_v8','project','project_v8_prefix',1,1,'{}','digest_v8',
               '2026-09-01T04:11:00Z','2026-09-01T04:11:00Z');
             INSERT INTO ledger_compaction_prefixes(
               prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,
               anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,
               baseline_projection_digest,created_at,verified_at
             ) SELECT 'prefix_v8','project','project_v8_prefix',1,event_rowid,event_id,
               operation_id,aggregate_sequence,'baseline_v8','digest_v8',
               '2026-09-01T04:11:00Z','2026-09-01T04:11:00Z'
               FROM event_source_index WHERE event_id='evt_v8_prefix_2';",
        )
        .unwrap();
}

fn seed_v7_frozen_message_for_v8(storage: &mut Storage) {
    storage
        .append_event(&event(
            "evt_v8_frozen_project",
            "op_v8_frozen_project",
            "project",
            "project_v8_frozen",
            1,
            json!({
                "canonical_path":"/fixture/v8-frozen",
                "project_id":"project_v8_frozen"
            }),
            "2026-09-01T04:20:00Z",
        ))
        .unwrap();
    storage
        .append_event(&event(
            "evt_v8_frozen_chat",
            "op_v8_frozen_chat",
            "chat",
            "chat_v8_frozen",
            1,
            v6_chat_payload(
                "chat_v8_frozen",
                "project_v8_frozen",
                1,
                team_message("chat_v8_frozen", "message_v8_frozen", 1),
            ),
            "2026-09-01T04:20:01Z",
        ))
        .unwrap();
    storage
        .connection
        .execute(
            "INSERT INTO chat_messages(
               message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at
             ) VALUES ('message_v8_frozen','chat_v8_frozen',1,?1,'digest_v8_frozen',
               '2026-09-01T04:20:01Z','2026-09-01T04:20:02Z');",
            [canonical_json_v1(team_message(
                "chat_v8_frozen",
                "message_v8_frozen",
                1,
            ))
            .unwrap()],
        )
        .unwrap();
    assert_eq!(
        storage
            .connection
            .execute(
                "INSERT INTO frozen_message_sources(
                   message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,
                   source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at
                 ) SELECT message.message_id,source.event_rowid,source.event_id,source.operation_id,
                   message.chat_id,source.aggregate_sequence,message.event_sequence,
                   message.canonical_message_json,message.message_digest,'2026-09-01T04:21:00Z'
                   FROM event_source_index AS source
                   JOIN chat_messages AS message ON message.message_id=source.introduced_message_id
                   WHERE source.event_id='evt_v8_frozen_chat';",
                [],
            )
            .unwrap(),
        1
    );
}

fn seed_v7_authority_for_v8(storage: &mut Storage) {
    seed_v7_prefix_for_v8(storage);
    storage
        .connection
        .execute_batch(
            "INSERT INTO maintenance_jobs(
               job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
               failure_kind,created_at,updated_at
             ) VALUES ('v8_authority_job','losslessCompaction','running','{}',
               '2026-09-01T04:12:00Z',NULL,NULL,NULL,
               '2026-09-01T04:12:00Z','2026-09-01T04:12:00Z');
             INSERT INTO ledger_compaction_delete_authority(
               job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,
               aggregate_id,aggregate_sequence,authorized_at
             ) SELECT 'v8_authority_job',1,'prefix_v8',event_rowid,event_id,operation_id,
               aggregate_type,aggregate_id,aggregate_sequence,'2026-09-01T04:12:00Z'
               FROM event_source_index WHERE event_id='evt_v8_prefix_1';
             UPDATE maintenance_jobs SET state='succeeded',
               completed_at='2026-09-01T04:13:00Z',updated_at='2026-09-01T04:13:00Z'
               WHERE job_id='v8_authority_job';",
        )
        .unwrap();
}

fn seed_exact_v8_prefix(storage: &mut Storage) {
    let first = event(
        "evt_v8_runtime_prefix_1",
        "op_v8_runtime_prefix_1",
        "project",
        "project_v8_runtime_prefix",
        1,
        json!({
            "canonical_path":"/fixture/v8-runtime-prefix",
            "project_id":"project_v8_runtime_prefix"
        }),
        "2026-09-01T05:02:01Z",
    );
    let second = event(
        "evt_v8_runtime_prefix_2",
        "op_v8_runtime_prefix_2",
        "project",
        "project_v8_runtime_prefix",
        2,
        json!({
            "canonical_path":"/fixture/v8-runtime-prefix",
            "project_id":"project_v8_runtime_prefix"
        }),
        "2026-09-01T05:02:02Z",
    );
    storage
        .append_event(&first)
        .expect("append V8 prefix row 1");
    storage
        .append_event(&second)
        .expect("append V8 prefix row 2");
    insert_exact_v8_frozen_operation(storage, &first, "2026-09-01T05:03:01Z");
    insert_exact_v8_frozen_operation(storage, &second, "2026-09-01T05:03:02Z");
    storage
        .connection
        .execute_batch(
            "INSERT INTO ledger_baselines(
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at
             ) VALUES ('baseline_v8_runtime','project','project_v8_runtime_prefix',1,1,
               '{}','digest_v8_runtime','2026-09-01T05:04:00Z','2026-09-01T05:04:00Z');
             INSERT INTO ledger_compaction_prefixes(
               prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,
               anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,
               baseline_projection_digest,created_at,verified_at
             ) SELECT 'prefix_v8_runtime','project','project_v8_runtime_prefix',1,event_rowid,
               event_id,operation_id,aggregate_sequence,'baseline_v8_runtime','digest_v8_runtime',
               '2026-09-01T05:04:00Z','2026-09-01T05:04:00Z'
               FROM event_source_index WHERE event_id='evt_v8_runtime_prefix_2';",
        )
        .expect("insert exact V8 prefix");
}

fn seed_exact_v8_frozen_message(storage: &mut Storage) {
    storage
        .append_event(&event(
            "evt_v8_runtime_frozen_project",
            "op_v8_runtime_frozen_project",
            "project",
            "project_v8_runtime_frozen",
            1,
            json!({
                "canonical_path":"/fixture/v8-runtime-frozen",
                "project_id":"project_v8_runtime_frozen"
            }),
            "2026-09-01T05:05:00Z",
        ))
        .expect("append V8 frozen-message project");
    let source = event(
        "evt_v8_runtime_frozen_chat",
        "op_v8_runtime_frozen_chat",
        "chat",
        "chat_v8_runtime_frozen",
        1,
        v6_chat_payload(
            "chat_v8_runtime_frozen",
            "project_v8_runtime_frozen",
            1,
            team_message("chat_v8_runtime_frozen", "message_v8_runtime_frozen", 1),
        ),
        "2026-09-01T05:05:01Z",
    );
    storage
        .append_event(&source)
        .expect("append V8 frozen-message chat");
    insert_exact_v8_frozen_operation(storage, &source, "2026-09-01T05:06:00Z");
    storage
        .connection
        .execute(
            "INSERT INTO chat_messages(
               message_id,chat_id,event_sequence,canonical_message_json,message_digest,
               created_at,materialized_at
             ) VALUES ('message_v8_runtime_frozen','chat_v8_runtime_frozen',1,?1,
               'digest_v8_runtime_frozen','2026-09-01T05:05:01Z','2026-09-01T05:06:00Z');",
            [canonical_json_v1(team_message(
                "chat_v8_runtime_frozen",
                "message_v8_runtime_frozen",
                1,
            ))
            .unwrap()],
        )
        .expect("insert exact V8 chat message");
    assert_eq!(
        storage
            .connection
            .execute(
                "INSERT INTO frozen_message_sources(
                   message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,
                   source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at
                 ) SELECT message.message_id,source.event_rowid,source.event_id,source.operation_id,
                   message.chat_id,source.aggregate_sequence,message.event_sequence,
                   message.canonical_message_json,message.message_digest,'2026-09-01T05:06:01Z'
                   FROM event_source_index AS source
                   JOIN chat_messages AS message ON message.message_id=source.introduced_message_id
                   WHERE source.event_id='evt_v8_runtime_frozen_chat';",
                [],
            )
            .expect("insert exact V8 frozen message"),
        1
    );
}

fn seed_exact_v8_authority(storage: &mut Storage) {
    seed_exact_v8_prefix(storage);
    storage
        .connection
        .execute_batch(
            "INSERT INTO maintenance_jobs(
               job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
               failure_kind,created_at,updated_at
             ) VALUES ('v8_runtime_authority_job','losslessCompaction','running','{}',
               '2026-09-01T05:07:00Z',NULL,NULL,NULL,
               '2026-09-01T05:07:00Z','2026-09-01T05:07:00Z');
             INSERT INTO ledger_compaction_delete_authority(
               job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,
               aggregate_id,aggregate_sequence,authorized_at
             ) SELECT 'v8_runtime_authority_job',1,'prefix_v8_runtime',event_rowid,event_id,
               operation_id,aggregate_type,aggregate_id,aggregate_sequence,'2026-09-01T05:07:01Z'
               FROM event_source_index WHERE event_id='evt_v8_runtime_prefix_1';
             UPDATE maintenance_jobs SET state='succeeded',
               completed_at='2026-09-01T05:07:02Z',updated_at='2026-09-01T05:07:02Z'
               WHERE job_id='v8_runtime_authority_job';",
        )
        .expect("insert exact V8 authority");
}

fn seed_v8_forbidden_append_state(storage: &mut Storage, state: &str) {
    match state {
        "prefix" => seed_exact_v8_prefix(storage),
        "frozen_message" => seed_exact_v8_frozen_message(storage),
        "authority" => seed_exact_v8_authority(storage),
        "frozen_operation" => {
            seed_exact_v8_frozen_operation(storage);
        }
        "active_compaction" => {
            storage
                .connection
                .execute(
                    "INSERT INTO maintenance_jobs(
                       job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
                       failure_kind,created_at,updated_at
                     ) VALUES ('v8_resubmit_active','losslessCompaction','running','{}',
                       '2026-09-01T05:01:00Z',NULL,NULL,NULL,
                       '2026-09-01T05:01:00Z','2026-09-01T05:01:00Z');",
                    [],
                )
                .unwrap();
        }
        _ => panic!("unknown V8 forbidden append state: {state}"),
    }
}

fn seed_exact_v8_frozen_operation(storage: &mut Storage) -> String {
    let source = event(
        "evt_v8_frozen_operation",
        "op_v8_frozen_operation",
        "project",
        "project_v8_frozen_operation",
        1,
        json!({
            "canonical_path":"/fixture/v8-frozen-operation",
            "project_id":"project_v8_frozen_operation"
        }),
        "2026-09-01T04:31:00Z",
    );
    storage.append_event(&source).expect("append V8 source");
    insert_exact_v8_frozen_operation(storage, &source, "2026-09-01T04:31:01Z");
    source.operation_id
}

fn insert_exact_v8_frozen_operation(
    storage: &mut Storage,
    source: &LedgerEventV1,
    frozen_at: &str,
) {
    let source_event_rowid: i64 = storage
        .connection
        .query_row(
            "SELECT rowid FROM events WHERE operation_id=?1;",
            [&source.operation_id],
            |row| row.get(0),
        )
        .unwrap();
    let canonical_payload_digest = sha256_file_version_v1(source.canonical_json_payload.as_bytes())
        .strip_prefix("sha256:")
        .unwrap()
        .to_string();
    storage
        .connection
        .execute(
            "INSERT OR IGNORE INTO operation_receipts(
               operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,
               payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,
               canonical_payload_digest_version,canonical_payload_digest_format,
               canonical_payload_digest,event_timestamp,materialized_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'sha256',1,'lowercaseHex64',?9,?10,?11);",
            params![
                source.operation_id,
                source.event_id,
                source.aggregate_type,
                source.aggregate_id,
                sql_i64(source.aggregate_sequence).unwrap(),
                source.event_type,
                source.payload_schema_version,
                source.canonical_json_payload,
                canonical_payload_digest,
                source.timestamp,
                frozen_at,
            ],
        )
        .expect("insert exact V8 operation receipt");
    let mut frozen = FrozenOperationSourceV1 {
        operation_id: source.operation_id.clone(),
        source_event_rowid: u64::try_from(source_event_rowid).unwrap(),
        source_event_id: source.event_id.clone(),
        request_id: source.request_id.clone(),
        aggregate_type: source.aggregate_type.clone(),
        aggregate_id: source.aggregate_id.clone(),
        aggregate_sequence: source.aggregate_sequence,
        event_type: source.event_type.clone(),
        payload_schema_version: source.payload_schema_version,
        canonical_payload_json: source.canonical_json_payload.clone(),
        canonical_payload_digest,
        event_timestamp: source.timestamp.clone(),
        canonical_record_json: String::new(),
        record_digest: String::new(),
        frozen_at: frozen_at.to_string(),
    };
    frozen.canonical_record_json =
        canonical_frozen_operation_record_v1(&frozen).expect("canonical frozen operation record");
    frozen.record_digest = sha256_file_version_v1(frozen.canonical_record_json.as_bytes())
        .strip_prefix("sha256:")
        .unwrap()
        .to_string();
    storage
        .connection
        .execute(
            "INSERT INTO frozen_operation_sources(
               operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,
               aggregate_id,aggregate_sequence,event_type,payload_schema_version,
               canonical_payload_json,canonical_payload_digest,event_timestamp,
               canonical_record_json,record_digest,frozen_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15);",
            params![
                frozen.operation_id,
                source_event_rowid,
                frozen.source_event_id,
                frozen.request_id,
                frozen.aggregate_type,
                frozen.aggregate_id,
                sql_i64(frozen.aggregate_sequence).unwrap(),
                frozen.event_type,
                frozen.payload_schema_version,
                frozen.canonical_payload_json,
                frozen.canonical_payload_digest,
                frozen.event_timestamp,
                frozen.canonical_record_json,
                frozen.record_digest,
                frozen.frozen_at,
            ],
        )
        .expect("insert exact frozen operation");
}

fn assert_v5_migration_rejected_without_mutation(storage: &mut Storage, name: &str) {
    storage.checkpoint().expect("checkpoint unsafe v4 fixture");
    let snapshot_before = database_snapshot(&storage.connection);
    let bytes_before = fs::read(storage.path()).expect("read unsafe v4 fixture");

    let error = migrate_v5(&mut storage.connection).expect_err("unsafe v4 must fail preflight");
    assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    assert_eq!(
        database_snapshot(&storage.connection),
        snapshot_before,
        "{name}"
    );
    assert_eq!(
        fs::read(storage.path()).expect("reread unsafe v4 fixture"),
        bytes_before,
        "{name}"
    );
}

fn assert_v6_migration_rejected_without_mutation(storage: &mut Storage, name: &str) {
    storage.checkpoint().expect("checkpoint unsafe v5 fixture");
    let snapshot_before = database_snapshot(&storage.connection);
    let bytes_before = fs::read(storage.path()).expect("read unsafe v5 fixture");

    let error = migrate_v6(&mut storage.connection).expect_err("unsafe v5 must fail preflight");
    assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    assert_eq!(
        database_snapshot(&storage.connection),
        snapshot_before,
        "{name}"
    );
    assert_eq!(
        fs::read(storage.path()).expect("reread unsafe v5 fixture"),
        bytes_before,
        "{name}"
    );
}

fn assert_v7_migration_rejected_without_mutation(storage: &mut Storage, name: &str) {
    storage.checkpoint().expect("checkpoint unsafe v6 fixture");
    let snapshot_before = database_snapshot(&storage.connection);
    let bytes_before = fs::read(storage.path()).expect("read unsafe v6 fixture");

    let error = migrate_v7(&mut storage.connection).expect_err("unsafe v6 must fail preflight");
    assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    assert_eq!(
        database_snapshot(&storage.connection),
        snapshot_before,
        "{name}"
    );
    assert_eq!(
        fs::read(storage.path()).expect("reread unsafe v6 fixture"),
        bytes_before,
        "{name}"
    );
}

fn assert_v8_migration_rejected_without_mutation(storage: &mut Storage, name: &str) {
    storage.checkpoint().expect("checkpoint unsafe v7 fixture");
    let snapshot_before = database_snapshot(&storage.connection);
    let bytes_before = fs::read(storage.path()).expect("read unsafe v7 fixture");

    let error = migrate_v8(&mut storage.connection).expect_err("unsafe v7 must fail preflight");
    assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    assert_eq!(
        database_snapshot(&storage.connection),
        snapshot_before,
        "{name}"
    );
    assert_eq!(
        fs::read(storage.path()).expect("reread unsafe v7 fixture"),
        bytes_before,
        "{name}"
    );
}

fn assert_resubmits_rejected_without_mutation(
    storage: &mut Storage,
    original: &LedgerEventV1,
    name: &str,
) {
    storage
        .checkpoint()
        .expect("checkpoint forbidden append fixture");
    let snapshot = database_snapshot(&storage.connection);
    let bytes = fs::read(storage.path()).expect("read forbidden append database");

    let duplicate_error = storage
        .append_event(original)
        .expect_err("duplicate must hit the runtime fence before classification");
    assert_eq!(duplicate_error.kind(), "storageSchemaMismatch", "{name}");
    assert_eq!(database_snapshot(&storage.connection), snapshot, "{name}");
    assert_eq!(
        fs::read(storage.path()).expect("reread duplicate database"),
        bytes,
        "{name}"
    );

    let mut conflict = original.clone();
    conflict.canonical_json_payload =
        canonical_json_v1(json!({"forbiddenResubmitConflict":name})).unwrap();
    let conflict_error = storage
        .append_event(&conflict)
        .expect_err("conflict must hit the runtime fence before classification");
    assert_eq!(conflict_error.kind(), "storageSchemaMismatch", "{name}");
    assert_eq!(database_snapshot(&storage.connection), snapshot, "{name}");
    assert_eq!(
        fs::read(storage.path()).expect("reread conflict database"),
        bytes,
        "{name}"
    );
}

fn assert_fresh_append_rejected_without_mutation(storage: &mut Storage, name: &str) {
    storage
        .checkpoint()
        .expect("checkpoint nonrunnable materialization fixture");
    let snapshot = database_snapshot(&storage.connection);
    let bytes = fs::read(storage.path()).expect("read nonrunnable materialization database");
    let fresh = event(
        &format!("evt_v8_fresh_rejected_{name}"),
        &format!("op_v8_fresh_rejected_{name}"),
        "project",
        &format!("project_v8_fresh_rejected_{name}"),
        1,
        json!({
            "canonical_path":format!("/fixture/v8-fresh-rejected-{name}"),
            "project_id":format!("project_v8_fresh_rejected_{name}")
        }),
        "2026-09-01T05:21:00Z",
    );
    let error = storage
        .append_event(&fresh)
        .expect_err("fresh append must hit the materialization fence");
    assert_eq!(error.kind(), "storageSchemaMismatch", "{name}");
    assert_eq!(database_snapshot(&storage.connection), snapshot, "{name}");
    assert_eq!(
        fs::read(storage.path()).expect("reread fresh append database"),
        bytes,
        "{name}"
    );
}

fn guarded_schema_objects(connection: &Connection) -> Vec<SchemaObject> {
    schema_objects(connection)
        .unwrap()
        .into_iter()
        .filter(|(_, name, _, _)| {
            ![
                "storage_schema_meta",
                "lifetime_materialization_job",
                "event_source_index",
                "event_source_index_aggregate_latest",
                "event_source_index_message_unique",
                "event_source_index_requires_exact_event",
                "event_source_index_append_only_update",
                "event_source_index_append_only_delete",
                "events_are_append_only_delete",
                "ledger_compaction_prefixes",
                "ledger_compaction_prefixes_latest",
                "ledger_compaction_prefixes_require_exact_anchor",
                "ledger_compaction_prefixes_append_only_update",
                "ledger_compaction_prefixes_append_only_delete",
                "frozen_message_sources",
                "frozen_message_sources_chat_sequence",
                "frozen_message_sources_require_exact_source",
                "frozen_message_sources_append_only_update",
                "frozen_message_sources_append_only_delete",
                "event_source_index_rejects_frozen_message_reuse",
                "maintenance_jobs_single_running_lossless_compaction",
                "ledger_compaction_delete_authority",
                "ledger_compaction_delete_authority_require_exact_tuple",
                "ledger_compaction_delete_authority_no_update",
                "ledger_compaction_delete_authority_delete_after_source",
                "events_consume_exact_v7_delete_authority",
                "ledger_baselines_compaction_immutable_update",
                "ledger_baselines_compaction_immutable_delete",
                "frozen_operation_sources",
                "frozen_operation_sources_source_rowid",
                "frozen_operation_sources_source_event",
                "frozen_operation_sources_aggregate",
                "frozen_operation_sources_require_exact_live_source",
                "frozen_operation_sources_append_only_update",
                "frozen_operation_sources_append_only_delete",
                "operation_receipts_compaction_immutable_update",
                "operation_receipts_compaction_immutable_delete",
            ]
            .contains(&name.as_str())
        })
        .collect()
}

fn preserved_table_rows(
    connection: &Connection,
) -> std::collections::BTreeMap<&'static str, Vec<Vec<SqlValue>>> {
    [
        "schema_meta",
        "app_instances",
        "events",
        "projects",
        "chats",
        "runs",
        "locks",
        "ledger_baselines",
        "operation_receipts",
        "chat_messages",
        "prepared_workspaces",
        "maintenance_jobs",
    ]
    .into_iter()
    .map(|table| (table, table_rows(connection, table)))
    .collect()
}

fn database_snapshot(connection: &Connection) -> DatabaseSnapshot {
    let schemas = schema_objects(connection).unwrap();
    let tables = schemas
        .iter()
        .filter(|(kind, _, _, _)| kind == "table")
        .map(|(_, name, _, _)| (name.clone(), table_rows(connection, name)))
        .collect();
    (schemas, tables)
}

fn v3_job_table_statement() -> &'static str {
    let start = MIGRATION_V3
        .find("CREATE TABLE lifetime_materialization_job")
        .expect("canonical v3 job table start");
    let remaining = &MIGRATION_V3[start..];
    let end = remaining
        .find("\n\nCOMMIT;")
        .expect("canonical v3 job table end");
    &remaining[..end]
}

#[allow(clippy::too_many_arguments)]
fn insert_lifetime_job(
    connection: &Connection,
    job_id: &str,
    state: &str,
    phase: &str,
    after_event_rowid: i64,
    failure_kind: Option<&str>,
    created_at: &str,
    completed_at: Option<&str>,
) {
    connection
        .execute(
            "INSERT INTO lifetime_materialization_job(\
               singleton,job_id,state,cutoff_event_rowid,phase,after_event_rowid,\
               failure_kind,created_at,updated_at,completed_at\
             ) VALUES (1,?1,?2,20,?3,?4,?5,?6,'2026-08-28T07:01:00Z',?7);",
            params![
                job_id,
                state,
                phase,
                after_event_rowid,
                failure_kind,
                created_at,
                completed_at
            ],
        )
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn insert_v4_job(
    connection: &Connection,
    job_id: &str,
    state: &str,
    phase: &str,
    cutoff_event_rowid: i64,
    after_scan_rowid: i64,
    scan_exhausted: i64,
    failure_kind: Option<&str>,
    completed_at: Option<&str>,
) {
    connection
        .execute(
            "INSERT INTO lifetime_materialization_job(\
               singleton,job_id,state,cutoff_event_rowid,phase,after_scan_rowid,scan_exhausted,\
               failure_kind,created_at,updated_at,completed_at\
             ) VALUES (1,?1,?2,?4,?3,?5,?6,?7,\
               '2026-08-28T09:00:00Z','2026-08-28T09:01:00Z',?8);",
            params![
                job_id,
                state,
                phase,
                cutoff_event_rowid,
                after_scan_rowid,
                scan_exhausted,
                failure_kind,
                completed_at,
            ],
        )
        .unwrap();
}

fn table_rows(connection: &Connection, table: &str) -> Vec<Vec<SqlValue>> {
    let order = if table == "frozen_operation_sources" {
        "operation_id"
    } else {
        "rowid"
    };
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table} ORDER BY {order};"))
        .expect("prepare table read");
    let column_count = statement.column_count();
    statement
        .query_map([], |row| {
            (0..column_count)
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<SqlValue>>>()
        })
        .expect("query table")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("read table")
}
