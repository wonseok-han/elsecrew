use super::*;
use hello_agent_protocol::storage_v1::canonical_json_v1;
use rusqlite::types::Value as SqlValue;
use rusqlite::Connection;
use tempfile::TempDir;

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
fn fresh_database_applies_exact_additive_v2_and_reopens_idempotently() {
    let (_root, path) = database();
    let storage = Storage::open(&path).expect("fresh storage");
    verify_schema(&storage.connection).expect("v1 schema");
    verify_schema_v2(&storage.connection).expect("v2 schema");
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
        (2, "pending".to_string())
    );
    drop(storage);

    let reopened = Storage::open(&path).expect("idempotent reopen");
    verify_schema_v2(&reopened.connection).expect("reopened v2 schema");
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
    drop_v2_schema(&storage.connection);
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
    let empty_v2: u32 = storage
        .connection
        .query_row(
            "SELECT (SELECT count(*) FROM ledger_baselines) + \
                    (SELECT count(*) FROM operation_receipts) + \
                    (SELECT count(*) FROM chat_messages) + \
                    (SELECT count(*) FROM prepared_workspaces) + \
                    (SELECT count(*) FROM maintenance_jobs);",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(empty_v2, 0, "startup must not backfill or materialize");
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

fn drop_v2_schema(connection: &Connection) {
    connection
        .execute_batch(
            "DROP TABLE maintenance_jobs;\
             DROP TABLE prepared_workspaces;\
             DROP TABLE chat_messages;\
             DROP TABLE operation_receipts;\
             DROP TABLE ledger_baselines;\
             DROP TABLE storage_schema_meta;\
             DROP INDEX chats_identity_project;",
        )
        .expect("remove additive v2 objects for v1 migration fixture");
}

fn table_rows(connection: &Connection, table: &str) -> Vec<Vec<SqlValue>> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid;"))
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
