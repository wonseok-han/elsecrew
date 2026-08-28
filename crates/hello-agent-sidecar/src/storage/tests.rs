use super::*;
use hello_agent_protocol::storage_v1::canonical_json_v1;
use rusqlite::types::Value as SqlValue;
use rusqlite::Connection;
use tempfile::TempDir;

type DatabaseSnapshot = (Vec<SchemaObject>, Vec<(String, Vec<Vec<SqlValue>>)>);

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
fn fresh_database_applies_exact_v4_and_reopens_idempotently() {
    let (_root, path) = database();
    let storage = Storage::open(&path).expect("fresh storage");
    verify_schema(&storage.connection).expect("v1 schema");
    verify_schema_v4_for_open(&storage.connection).expect("v4 schema");
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
        (4, "pending".to_string())
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
    drop(storage);

    let reopened = Storage::open(&path).expect("idempotent reopen");
    verify_schema_v4_for_open(&reopened.connection).expect("reopened v4 schema");
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
        verify_schema_v4_for_open(&upgraded.connection).expect("exact v4");
        assert_eq!(
            upgraded
                .connection
                .query_row(
                    "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            state
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
fn v4_partial_extra_and_metadata_table_version_mismatch_fail_closed() {
    let (_root, partial_path) = database();
    let partial = Storage::open(&partial_path).unwrap();
    partial
        .connection
        .execute_batch("DROP TABLE lifetime_materialization_job;")
        .unwrap();
    drop(partial);
    let error = Storage::open(&partial_path)
        .err()
        .expect("partial v4 must fail");
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
        .expect("extra v4 must fail");
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
        let storage = Storage::open(&path).expect("fresh v4");
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
        drop(storage);
        Storage::open(&path).unwrap_or_else(|error| panic!("{name} must reopen: {error:?}"));
    }

    let (_root, path) = database();
    let storage = Storage::open(&path).expect("fresh v4");
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
    drop(storage);
    Storage::open(&path).expect("persisted private exhaustion proof may reopen");

    for state in ["attempting", "failed"] {
        let (_root, path) = database();
        let storage = Storage::open(&path).expect("fresh v4");
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
        drop(storage);
        let error = Storage::open(&path)
            .err()
            .expect("non-runnable v4 job rejected");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{state}");
        assert_eq!(
            error.detail(),
            "storage lifetime materialization state is invalid"
        );
    }

    for materialization_state in ["materializing", "failed"] {
        let (_root, path) = database();
        let storage = Storage::open(&path).expect("fresh v4");
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
        let storage = Storage::open(&path).expect("fresh v4");
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
            "DROP TABLE lifetime_materialization_job;\
             DROP TABLE maintenance_jobs;\
             DROP TABLE prepared_workspaces;\
             DROP TABLE chat_messages;\
             DROP TABLE operation_receipts;\
             DROP TABLE ledger_baselines;\
             DROP TABLE storage_schema_meta;\
             DROP INDEX chats_identity_project;",
        )
        .expect("remove additive v2 objects for v1 migration fixture");
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

fn guarded_schema_objects(connection: &Connection) -> Vec<SchemaObject> {
    schema_objects(connection)
        .unwrap()
        .into_iter()
        .filter(|(_, name, _, _)| {
            name != "storage_schema_meta" && name != "lifetime_materialization_job"
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
