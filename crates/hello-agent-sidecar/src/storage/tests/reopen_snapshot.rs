//! Same-connection global verification and derived-only catch-up regressions.
use super::*;
use crate::storage::reopen::bytes::{ByteUsage, FRAGMENT_BYTES, INPUT_BYTES};
use crate::storage::reopen::snapshot::{ReopenMaintenanceSession, ReopenMaintenanceStep};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const STAMP: &str = "2026-09-03T06:00:00Z";
const STEP_GUARD: usize = 20_000;

#[test]
fn snapshot_maintenance_missing_chat_indexes_app_instance_and_ready_coverage() {
    let (_root, path) = fixture(false, 1);
    let mut storage = Storage::open(&path).unwrap();
    storage
        .register_app_instance(&ready("sidecar_snapshot"))
        .unwrap();
    storage.checkpoint().unwrap();
    drop(storage);
    let mut c = connection(&path);
    super::reopen_closure::corrupt(&c, "DELETE FROM event_source_index");
    let source = table_rows(&c, "events");
    let mut session = ReopenMaintenanceSession::start(&mut c).unwrap();
    complete(&mut session);
    drop(session);
    assert_eq!(table_rows(&c, "events"), source);
    assert_eq!(
        c.query_row("SELECT count(*) FROM event_source_index", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        source.len() as i64
    );
    for sql in [
        "UPDATE app_instances SET pid=0",
        "DELETE FROM projects",
        "DELETE FROM chat_messages WHERE event_sequence=2",
        "DELETE FROM ledger_baselines WHERE aggregate_type='chat'",
    ] {
        let temp = TempDir::new().unwrap();
        let trial = temp.path().join("trial.sqlite3");
        c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
        fs::copy(&path, &trial).unwrap();
        let mut damaged = connection(&trial);
        super::reopen_closure::corrupt(&damaged, sql);
        let before = database_snapshot(&damaged);
        let mut session = ReopenMaintenanceSession::start(&mut damaged).unwrap();
        let mut failed = false;
        for _ in 0..STEP_GUARD {
            match session.step(&mut || false) {
                Ok(ReopenMaintenanceStep::Pending(batch)) => usage(&batch),
                Err(_) => {
                    failed = true;
                    break;
                }
                other => panic!("ready corruption accepted {sql}: {other:?}"),
            }
        }
        assert!(failed);
        drop(session);
        assert_eq!(database_snapshot(&damaged), before);
    }
    super::reopen_closure::corrupt(&c,"DELETE FROM chat_messages WHERE event_sequence=2; UPDATE storage_schema_meta SET materialization_state='pending'");
    let mut session = ReopenMaintenanceSession::start(&mut c).unwrap();
    complete(&mut session);
    drop(session);
    assert_eq!(
        c.query_row("SELECT count(*) FROM chat_messages", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn snapshot_maintenance_compacted_chat_and_frozen_historical_workspace() {
    let (_root, path) = super::reopen_closure::fixture(4, true, true, 0, 0);
    let mut c = connection(&path);
    let mut session = ReopenMaintenanceSession::start(&mut c).unwrap();
    complete(&mut session);
    drop(session);
    let (_root, path) = super::reopen_closure::fixture(2, false, false, 0, 0);
    let c = connection(&path);
    let old = super::reopen_derived::workspace("project_r2", 0, false);
    super::reopen_closure::replace_deleted_payload(
        &c,
        1,
        json!({"project_id":"project_r2","canonical_path":"/fixture/r2","prepared_workspaces":{"preparation_b_0":old.clone()}}),
    );
    drop(c);
    let mut storage = Storage::open(&path).unwrap();
    let mut latest = old.clone();
    latest["bindingState"] = json!("abandoned");
    latest["manifest"]["lifecycle"] = json!("abandoned");
    latest["updatedAt"] = json!(STAMP);
    storage.append_event(&event("evt_frozen_history","op_frozen_history","project","project_r2",4,json!({"project_id":"project_r2","canonical_path":"/fixture/r2","prepared_workspaces":{"preparation_b_0":latest.clone()}}),STAMP)).unwrap();
    storage.checkpoint().unwrap();
    drop(storage);
    let mut c = connection(&path);
    super::reopen_derived::insert_workspace(&c, &old);
    let events = table_rows(&c, "events");
    let frozen = table_rows(&c, "frozen_operation_sources");
    let mut session = ReopenMaintenanceSession::start(&mut c).unwrap();
    let calls = complete(&mut session);
    drop(session);
    assert_eq!(table_rows(&c, "events"), events);
    assert_eq!(table_rows(&c, "frozen_operation_sources"), frozen);
    let stored: String = c
        .query_row(
            "SELECT canonical_record_json FROM prepared_workspaces",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, canonical_json_v1(latest).unwrap());
    println!("frozen historical workspace calls={calls}");
}

#[test]
fn snapshot_maintenance_historical_workspace_updates_after_index_catchup() {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    let mut member = super::reopen_derived::workspace("project_history", 0, false);
    for sequence in 1..=2 {
        if sequence == 2 {
            member["bindingState"] = json!("abandoned");
            member["manifest"]["lifecycle"] = json!("abandoned");
            member["updatedAt"] = json!(STAMP);
        }
        storage.append_event(&event(&format!("evt_history_{sequence}"),&format!("op_history_{sequence}"),"project","project_history",sequence,json!({"project_id":"project_history","canonical_path":"/fixture/history","prepared_workspaces":{"preparation_b_0":member.clone()}}),STAMP)).unwrap();
        if sequence == 1 {
            run_bounded_materializer_to_ready(&mut storage);
        }
    }
    storage.checkpoint().unwrap();
    drop(storage);
    let mut connection = connection(&path);
    super::reopen_closure::corrupt(&connection, "DELETE FROM event_source_index");
    let events = table_rows(&connection, "events");
    assert_eq!(
        connection
            .query_row("SELECT binding_state FROM prepared_workspaces", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "unbound"
    );
    let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
    let calls = complete(&mut session);
    drop(session);
    assert_eq!(table_rows(&connection, "events"), events);
    let raw: String = connection
        .query_row(
            "SELECT canonical_record_json FROM prepared_workspaces",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw, canonical_json_v1(member).unwrap());
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM event_source_index", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    println!("historical pending workspace calls={calls}");
    drop(root);
}

#[test]
fn snapshot_maintenance_uncapped_workspace_paths_and_mid_member_restart() {
    for large_path in [false, true] {
        let (_root, path) = if large_path {
            let (root, path) = database();
            let mut storage = Storage::open(&path).unwrap();
            let mut member = super::reopen_derived::workspace("project_members", 0, false);
            member["manifest"]["canonicalGitRoot"] =
                json!(format!("/source/{}", "한".repeat(350_000)));
            member["manifest"]["worktreePath"] =
                json!(format!("/snapshot/{}", "b".repeat(1024 * 1024 + 13)));
            hello_agent_protocol::run_recovery_v1::validate_g5_payload_v1(
                "preparedChatWorkspace",
                &member,
            )
            .unwrap();
            storage.append_event(&event("evt_large_member","op_large_member","project","project_members",1,json!({"project_id":"project_members","canonical_path":"/fixture/member","prepared_workspaces":{"preparation_b_0":member}}),STAMP)).unwrap();
            storage.checkpoint().unwrap();
            drop(storage);
            (root, path)
        } else {
            super::reopen_derived::members_fixture(3, false)
        };
        let mut connection = connection(&path);
        let observer = Connection::open(&path).unwrap();
        let events = table_rows(&connection, "events");
        let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
        if !large_path {
            let mut partial = false;
            for _ in 0..STEP_GUARD {
                one_pending(&mut session);
                let (members,pending,cursor):(i64,bool,i64)=observer.query_row("SELECT (SELECT count(*) FROM prepared_workspaces),state='pending',after_scan_rowid FROM lifetime_materialization_job",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().unwrap().unwrap_or((0,false,-1));
                if members == 1 && pending {
                    assert_eq!(cursor, 0);
                    partial = true;
                    break;
                }
            }
            assert!(partial, "no committed partial member boundary");
            drop(session);
            let partial_rows = table_rows(&observer, "prepared_workspaces");
            session = ReopenMaintenanceSession::start(&mut connection).unwrap();
            let calls = complete(&mut session);
            drop(session);
            let rows = table_rows(&connection, "prepared_workspaces");
            assert!(
                rows.contains(&partial_rows[0]),
                "idempotent replay changed the committed member"
            );
            assert_eq!(rows.len(), 3);
            println!("mid-member restart calls={calls}");
        } else {
            let calls = complete(&mut session);
            drop(session);
            let bytes: i64 = connection
                .query_row(
                    "SELECT length(CAST(canonical_record_json AS BLOB)) FROM prepared_workspaces",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(bytes > 2 * 1024 * 1024);
            println!("uncapped member bytes={bytes} calls={calls}");
        }
        assert_eq!(table_rows(&connection, "events"), events);
    }
}

#[test]
fn snapshot_maintenance_frozen_large_source_and_envelope_mismatch() {
    for corrupt_envelope in [false, true] {
        let (_root, path) = super::reopen_closure::fixture(2, false, false, 0, 0);
        let mut connection = connection(&path);
        super::reopen_closure::replace_deleted_payload(
            &connection,
            1,
            json!({"project_id":"project_r2","canonical_path":"/fixture/r2","padding":"x".repeat(1024*1024+19)}),
        );
        if corrupt_envelope {
            let mut envelope:Value=serde_json::from_str(&connection.query_row("SELECT canonical_record_json FROM frozen_operation_sources WHERE operation_id='op_r2_1'",[],|r|r.get::<_,String>(0)).unwrap()).unwrap();
            envelope["canonicalPayloadJson"] = json!("{}");
            let raw = canonical_json_v1(envelope).unwrap();
            let digest = sha256_file_version_v1(raw.as_bytes());
            super::reopen_closure::corrupt(&connection,&format!("UPDATE frozen_operation_sources SET canonical_record_json='{}',record_digest='{}' WHERE operation_id='op_r2_1'",raw.replace('\'',"''"),digest.trim_start_matches("sha256:")));
        }
        let before = database_snapshot(&connection);
        let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
        if corrupt_envelope {
            let mut rejected = false;
            for _ in 0..STEP_GUARD {
                match session.step(&mut || false) {
                    Ok(ReopenMaintenanceStep::Pending(batch)) => usage(&batch),
                    Err(_) => {
                        rejected = true;
                        break;
                    }
                    other => panic!("mismatched envelope accepted {other:?}"),
                }
            }
            assert!(rejected);
            drop(session);
            assert_eq!(database_snapshot(&connection), before);
        } else {
            let calls = complete(&mut session);
            drop(session);
            println!("large frozen payload calls={calls}");
        }
    }
}

fn fixture(ready: bool, projects: usize) -> (TempDir, PathBuf) {
    assert!(projects > 0);
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    for index in 0..projects {
        let id = format!("project_snapshot_{index:03}");
        assert_eq!(
            storage
                .append_event(&event(
                    &format!("evt_{id}"),
                    &format!("op_{id}"),
                    "project",
                    &id,
                    1,
                    json!({"project_id":id,"canonical_path":format!("/fixture/snapshot/{index:03}"),"marker":index}),
                    STAMP,
                ))
                .unwrap(),
            AppendOutcome::Committed
        );
    }
    for sequence in 1..=2 {
        assert_eq!(
            storage
                .append_event(&event(
                    &format!("evt_snapshot_chat_{sequence}"),
                    &format!("op_snapshot_chat_{sequence}"),
                    "chat",
                    "chat_snapshot",
                    sequence,
                    materializable_chat_payload(
                        "chat_snapshot",
                        "project_snapshot_000",
                        sequence,
                        team_message(
                            "chat_snapshot",
                            &format!("message_snapshot_{sequence}"),
                            sequence
                        ),
                    ),
                    STAMP,
                ))
                .unwrap(),
            AppendOutcome::Committed
        );
    }
    if ready {
        run_bounded_materializer_to_ready(&mut storage);
    }
    verify_projection_replay(&storage.connection).unwrap();
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        if ready { "ready" } else { "pending" },
    );
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}

fn connection(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=0;")
        .unwrap();
    connection
}

fn usage(usage: &ByteUsage) {
    assert!(usage.fragment_bytes <= FRAGMENT_BYTES);
    assert!(usage.encoded_bytes() <= INPUT_BYTES);
    assert!(usage.work_units <= 100);
}

fn one_pending(session: &mut ReopenMaintenanceSession<'_>) {
    match session.step(&mut || false).unwrap() {
        ReopenMaintenanceStep::Pending(batch) => usage(&batch),
        _ => panic!("fixture must expose a pending boundary"),
    }
}

fn complete(session: &mut ReopenMaintenanceSession<'_>) -> usize {
    for calls in 1..=STEP_GUARD {
        match session.step(&mut || false).unwrap() {
            ReopenMaintenanceStep::Pending(batch) => usage(&batch),
            ReopenMaintenanceStep::ReadyVerified(batch) => {
                usage(&batch);
                assert!(session.step(&mut || false).is_err());
                return calls;
            }
            ReopenMaintenanceStep::Cancelled => panic!("uncancelled session cancelled"),
        }
    }
    panic!("maintenance did not complete within the step guard");
}

fn insert_pending_compaction(
    connection: &Connection,
    job_id: &str,
    phase: &str,
    after: u64,
    scan_exhausted: bool,
    operations_verified: bool,
    equivalence_verified: bool,
) -> u64 {
    let cutoff: u64 = connection
        .query_row(
            "SELECT coalesce(max(source_rowid),0) FROM (\
               SELECT rowid AS source_rowid FROM events UNION ALL \
               SELECT source_event_rowid AS source_rowid FROM frozen_operation_sources);",
            [],
            |row| row_u64(row, 0),
        )
        .unwrap();
    let after = if after == u64::MAX { cutoff } else { after };
    let private = json!({
        "storageCompactionV2PrivateState": {
            "schemaVersion": 1,
            "cursor": {
                "schemaVersion": 2,
                "cutoffEventRowid": cutoff,
                "phase": phase,
                "afterEventRowid": after,
                "scanExhausted": scan_exhausted,
            },
            "operationsVerified": operations_verified,
            "equivalenceVerified": equivalence_verified,
        }
    });
    connection
        .execute(
            "INSERT INTO maintenance_jobs(\
               job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
               failure_kind,created_at,updated_at) \
             VALUES (?1,'losslessCompaction','pending',?2,NULL,NULL,NULL,NULL,?3,?3);",
            params![job_id, private.to_string(), STAMP],
        )
        .unwrap();
    cutoff
}

fn seed_ready_reopen_operations(storage: &mut Storage, count: usize) {
    let mut sources = Vec::new();
    for ordinal in 0..count {
        let project_id = format!("project_admission_{ordinal}");
        let source = event(
            &format!("evt_admission_{ordinal}"),
            &format!("op_admission_{ordinal}"),
            "project",
            &project_id,
            1,
            json!({
                "canonical_path": format!("/fixture/admission/{ordinal}"),
                "project_id": project_id,
            }),
            STAMP,
        );
        storage.append_event(&source).unwrap();
        sources.push(source);
    }
    run_bounded_materializer_to_ready(storage);
    for source in &sources {
        insert_exact_v8_frozen_operation(storage, source, STAMP);
    }
    let cutoff: u64 = storage
        .connection
        .query_row("SELECT coalesce(max(rowid),0) FROM events", [], |row| {
            row_u64(row, 0)
        })
        .unwrap();
    let proof = json!({"storageCompactionV2PrivateState": {
        "schemaVersion": 1,
        "cursor": {"schemaVersion": 2, "cutoffEventRowid": cutoff, "phase": "finalize",
                   "afterEventRowid": cutoff, "scanExhausted": true},
        "operationsVerified": true,
        "equivalenceVerified": true,
    }});
    storage.connection.execute(
        "INSERT INTO maintenance_jobs(\
           job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
           failure_kind,created_at,updated_at) \
         VALUES ('compact_admission_terminal','losslessCompaction','succeeded',?1,?2,?2,?2,NULL,?2,?2);",
        params![proof.to_string(), STAMP],
    ).unwrap();
}

fn observed(connection: &Connection, path: &Path) -> (DatabaseSnapshot, Vec<u8>) {
    (database_snapshot(connection), fs::read(path).unwrap())
}

fn unchanged(connection: &Connection, path: &Path, before: &(DatabaseSnapshot, Vec<u8>)) {
    assert!(connection.is_autocommit());
    assert_eq!(database_snapshot(connection), before.0);
    assert_eq!(fs::read(path).unwrap(), before.1);
}

#[test]
fn product_admission_reopens_more_than_one_hundred_frozen_operations_in_bounded_steps() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_ready_reopen_operations(&mut storage, 101);
    storage.checkpoint().unwrap();
    let before = database_snapshot(&storage.connection);
    let preserved = preserved_table_rows(&storage.connection);
    drop(storage);

    let legacy = match Storage::open(&path) {
        Ok(_) => panic!("legacy temporary bound remains unchanged"),
        Err(error) => error,
    };
    assert_eq!(legacy.kind(), "storageSchemaMismatch");

    let mut connection = connection(&path);
    let mut session = ReopenMaintenanceSession::start_for_v8_admission(&mut connection).unwrap();
    let calls = complete(&mut session);
    assert!(calls > 1, "101 frozen sources must cross a bounded step");
    drop(session);
    assert_eq!(database_snapshot(&connection), before);
    drop(connection);

    let cancellation = AtomicBool::new(false);
    let admitted = Storage::open_for_admission(&path, &cancellation)
        .expect("product admission consumes the complete private proof");
    assert_eq!(preserved_table_rows(&admitted.connection), preserved);
    assert_eq!(
        storage_schema_version(&admitted.connection).unwrap(),
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V10)
    );
}

#[test]
fn product_admission_accepts_uncompacted_storage_without_compaction_jobs() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    run_bounded_materializer_to_ready(&mut storage);
    storage.checkpoint().unwrap();
    let before = preserved_table_rows(&storage.connection);
    drop(storage);

    let cancellation = AtomicBool::new(false);
    let admitted = Storage::open_for_admission(&path, &cancellation).unwrap();
    assert_eq!(preserved_table_rows(&admitted.connection), before);
    assert_eq!(
        storage_schema_version(&admitted.connection).unwrap(),
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V10)
    );
}

#[test]
fn product_admission_rejects_unpaired_succeeded_compaction_control_state_without_mutation() {
    for cutoff_json in [
        "{}".to_owned(),
        json!({"storageCompactionV2PrivateState": {
            "schemaVersion": 1,
            "cursor": {"schemaVersion": 2, "cutoffEventRowid": 1, "phase": "finalize",
                       "afterEventRowid": 1, "scanExhausted": true},
            "operationsVerified": true,
            "equivalenceVerified": true,
        }})
        .to_string(),
    ] {
        let (_root, path) = database();
        let storage = Storage::open(&path).unwrap();
        storage
            .connection
            .execute(
                "INSERT INTO maintenance_jobs(\
               job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
               failure_kind,created_at,updated_at) \
             VALUES ('orphan_succeeded','losslessCompaction','succeeded',?1,?2,?2,?2,NULL,?2,?2);",
                params![cutoff_json, STAMP],
            )
            .unwrap();
        storage.checkpoint().unwrap();
        let before = observed(&storage.connection, &path);
        drop(storage);

        let cancellation = AtomicBool::new(false);
        assert!(Storage::open_for_admission(&path, &cancellation).is_err());
        let connection = self::connection(&path);
        unchanged(&connection, &path, &before);
    }
}

#[test]
fn product_admission_rejects_duplicate_succeeded_compaction_proofs_without_mutation() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_ready_reopen_operations(&mut storage, 101);
    storage
        .connection
        .execute(
            "INSERT INTO maintenance_jobs(\
           job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
           failure_kind,created_at,updated_at) \
         SELECT 'compact_admission_duplicate',job_kind,state,cutoff_json,started_at,completed_at,\
                last_successful_at,failure_kind,created_at,updated_at \
         FROM maintenance_jobs WHERE job_id='compact_admission_terminal';",
            [],
        )
        .unwrap();
    storage.checkpoint().unwrap();
    let before = observed(&storage.connection, &path);
    drop(storage);

    let cancellation = AtomicBool::new(false);
    assert!(Storage::open_for_admission(&path, &cancellation).is_err());
    let connection = self::connection(&path);
    unchanged(&connection, &path, &before);
}

#[test]
fn product_admission_defers_pending_without_decision_witness_across_phases() {
    for (phase, operations, equivalence) in [
        ("freezeOperations", false, false),
        ("verifyEquivalence", true, false),
        ("finalize", true, true),
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        if phase == "freezeOperations" {
            for ordinal in 0..2 {
                storage
                    .append_event(&event(
                        &format!("evt_pending_{phase}_{ordinal}"),
                        &format!("op_pending_{phase}_{ordinal}"),
                        "project",
                        &format!("project_pending_{phase}_{ordinal}"),
                        1,
                        json!({
                            "project_id": format!("project_pending_{phase}_{ordinal}"),
                            "canonical_path": format!("/fixture/pending/{ordinal}"),
                        }),
                        STAMP,
                    ))
                    .unwrap();
            }
            run_bounded_materializer_to_ready(&mut storage);
        } else {
            seed_ready_reopen_operations(&mut storage, 101);
        }
        let cutoff = storage
            .connection
            .query_row("SELECT coalesce(max(rowid),0) FROM events", [], |row| {
                row_u64(row, 0)
            })
            .unwrap();
        insert_pending_compaction(
            &storage.connection,
            &format!("pending_{phase}"),
            phase,
            if phase == "freezeOperations" {
                0
            } else {
                u64::MAX
            },
            false,
            operations,
            equivalence,
        );
        assert!(cutoff > 0);
        storage.checkpoint().unwrap();
        let pending_before = table_rows(&storage.connection, "maintenance_jobs");
        let database_before = database_snapshot(&storage.connection);
        drop(storage);

        assert!(
            Storage::open(&path).is_err(),
            "legacy {phase} remains fenced"
        );
        let cancellation = AtomicBool::new(false);
        assert!(
            Storage::open_for_admission(&path, &cancellation).is_err(),
            "{phase}: V2 cursor alone cannot prove skipped eligibility"
        );
        let admitted = self::connection(&path);
        assert_eq!(
            table_rows(&admitted, "maintenance_jobs"),
            pending_before,
            "{phase}: pending job must remain byte-exact"
        );
        assert_eq!(database_snapshot(&admitted), database_before);
        assert!(admitted.is_autocommit());
    }
}

#[test]
fn product_admission_rejects_unsafe_or_malformed_compaction_state_without_mutation() {
    for mutation in [
        "UPDATE maintenance_jobs SET state='running' WHERE job_id='pending_reject';",
        "UPDATE maintenance_jobs SET cutoff_json=json_set(cutoff_json,'$.storageCompactionV2PrivateState.cursor.cutoffEventRowid',999999,'$.storageCompactionV2PrivateState.cursor.afterEventRowid',999999) WHERE job_id='pending_reject';",
        "UPDATE maintenance_jobs SET cutoff_json=json_set(cutoff_json,'$.storageCompactionV2PrivateState.extra',1) WHERE job_id='pending_reject';",
        "UPDATE maintenance_jobs SET cutoff_json=json_set(cutoff_json,'$.storageCompactionV2PrivateState.cursor.phase','finalize') WHERE job_id='pending_reject';",
        "INSERT INTO maintenance_jobs(job_id,job_kind,state,cutoff_json,created_at,updated_at) SELECT 'pending_second',job_kind,state,cutoff_json,created_at,updated_at FROM maintenance_jobs WHERE job_id='pending_reject';",
    ] {
        let (_root, path) = fixture(true, 2);
        let connection = connection(&path);
        insert_pending_compaction(
            &connection,
            "pending_reject",
            "freezeOperations",
            0,
            false,
            false,
            false,
        );
        connection.execute_batch(mutation).unwrap();
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
        let before = observed(&connection, &path);
        drop(connection);
        let cancellation = AtomicBool::new(false);
        assert!(Storage::open_for_admission(&path, &cancellation).is_err());
        let connection = self::connection(&path);
        unchanged(&connection, &path, &before);
    }

    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_exact_v8_authority(&mut storage);
    storage.checkpoint().unwrap();
    let before = observed(&storage.connection, &path);
    drop(storage);
    let cancellation = AtomicBool::new(false);
    assert!(Storage::open_for_admission(&path, &cancellation).is_err());
    let connection = self::connection(&path);
    unchanged(&connection, &path, &before);
}

#[test]
fn product_admission_no_progress_injection_fails_instead_of_spinning() {
    let (_root, path) = fixture(true, 2);
    let mut connection = connection(&path);
    let before = observed(&connection, &path);
    let mut session = ReopenMaintenanceSession::start_for_v8_admission(&mut connection).unwrap();
    session.force_no_progress_for_test();
    assert_eq!(
        session.step(&mut || false).unwrap_err().kind(),
        "storageSchemaMismatch"
    );
    assert!(session.step(&mut || false).is_err());
    drop(session);
    unchanged(&connection, &path, &before);
}

#[test]
fn snapshot_maintenance_small_ready_and_pending_reach_ready_with_bounded_usage() {
    for initially_ready in [false, true] {
        let (_root, path) = fixture(initially_ready, 2);
        let mut connection = connection(&path);
        let schema = schema_objects(&connection).unwrap();
        let events = table_rows(&connection, "events");
        let projects = table_rows(&connection, "projects");
        let chats = table_rows(&connection, "chats");
        let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
        let calls = complete(&mut session);
        drop(session);
        assert!(connection.is_autocommit());
        assert_eq!(schema_objects(&connection).unwrap(), schema);
        assert_eq!(table_rows(&connection, "events"), events);
        assert_eq!(table_rows(&connection, "projects"), projects);
        assert_eq!(table_rows(&connection, "chats"), chats);
        assert_eq!(
            connection
                .query_row(
                    "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "ready"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM chat_messages WHERE chat_id='chat_snapshot'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
        println!("snapshot maintenance initially_ready={initially_ready} calls={calls}");
    }
}

// Test-only stored corruption: restore the exact original trigger definitions
// before starting verification. No production mutator or policy is changed.
fn corrupt_late_source(connection: &Connection) {
    let schema = schema_objects(connection).unwrap();
    let triggers = connection
        .prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name='events'")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for (name, _) in &triggers {
        connection
            .execute_batch(&format!("DROP TRIGGER {name}"))
            .unwrap();
    }
    assert_eq!(connection.execute("UPDATE events SET canonical_json_payload=canonical_json_payload||' ' WHERE aggregate_type='project' AND aggregate_id='project_snapshot_100'", []).unwrap(), 1);
    for (_, sql) in triggers {
        connection.execute_batch(&sql).unwrap();
    }
    assert_eq!(schema_objects(connection).unwrap(), schema);
}

#[test]
fn snapshot_maintenance_late_source_and_projection_corruption_never_verify_or_mutate() {
    for source_corruption in [true, false] {
        let (_root, path) = fixture(true, 101);
        let mut connection = connection(&path);
        let rowid: i64 = connection.query_row("SELECT rowid FROM events WHERE aggregate_type='project' AND aggregate_id='project_snapshot_100'", [], |row| row.get(0)).unwrap();
        assert!(rowid > 100);
        let projection_rowid: i64 = connection
            .query_row(
                "SELECT rowid FROM projects WHERE project_id='project_snapshot_100'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(projection_rowid > 100);
        if source_corruption {
            corrupt_late_source(&connection);
        } else {
            assert_eq!(connection.execute("UPDATE projects SET projection_json=json_set(projection_json,'$.marker',-1) WHERE project_id='project_snapshot_100'", []).unwrap(), 1);
        }
        let before = observed(&connection, &path);
        let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
        let mut pending = 0;
        let mut rejected = false;
        for _ in 0..STEP_GUARD {
            match session.step(&mut || false) {
                Ok(ReopenMaintenanceStep::Pending(batch)) => {
                    usage(&batch);
                    pending += 1;
                }
                Ok(ReopenMaintenanceStep::ReadyVerified(batch)) => {
                    usage(&batch);
                    panic!("corrupt stored input produced ReadyVerified");
                }
                Ok(ReopenMaintenanceStep::Cancelled) => panic!("unexpected cancellation"),
                Err(error) => {
                    assert_eq!(error.kind, "storageSchemaMismatch");
                    rejected = true;
                    break;
                }
            }
        }
        assert!(rejected, "corruption did not reject before the step guard");
        assert!(
            pending > 0,
            "late corruption must follow committed scan progress"
        );
        assert!(session.step(&mut || false).is_err());
        drop(session);
        unchanged(&connection, &path, &before);
    }
}

#[test]
fn snapshot_maintenance_external_commit_invalidates_pending_and_terminal_session() {
    for after_terminal in [false, true] {
        let (_root, path) = fixture(true, 2);
        let mut connection = connection(&path);
        let other = self::connection(&path);
        let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
        if after_terminal {
            // Complete without querying the terminal outcome a second time yet.
            let mut verified = false;
            for _ in 0..STEP_GUARD {
                match session.step(&mut || false).unwrap() {
                    ReopenMaintenanceStep::Pending(batch) => usage(&batch),
                    ReopenMaintenanceStep::ReadyVerified(batch) => {
                        usage(&batch);
                        verified = true;
                        break;
                    }
                    ReopenMaintenanceStep::Cancelled => panic!("unexpected cancellation"),
                }
            }
            assert!(verified, "terminal proof did not finish before guard");
        } else {
            one_pending(&mut session);
        }
        assert_eq!(other.execute("UPDATE projects SET projection_json=projection_json||' ' WHERE project_id='project_snapshot_000'", []).unwrap(), 1);
        // Isolate the intentional external mutation from verifier behavior.
        let after_external = observed(&other, &path);
        assert!(session.step(&mut || false).is_err());
        assert!(session.step(&mut || false).is_err());
        drop(session);
        unchanged(&connection, &path, &after_external);
    }
}

#[test]
fn snapshot_maintenance_cancellation_discards_entry_and_resumed_progress() {
    for initially_ready in [false, true] {
        for after_pending in [false, true] {
            let (_root, path) = fixture(initially_ready, 2);
            let mut connection = connection(&path);
            let observer = self::connection(&path);
            let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
            if after_pending {
                one_pending(&mut session);
            }
            let before_cancel = observed(&observer, &path);
            assert!(matches!(
                session.step(&mut || true).unwrap(),
                ReopenMaintenanceStep::Cancelled
            ));
            assert!(session.step(&mut || false).is_err());
            drop(session);
            unchanged(&connection, &path, &before_cancel);
        }
    }
}

struct FaultFlags {
    read: AtomicBool,
    commit: AtomicBool,
    hits: AtomicUsize,
    workspace_inserts: AtomicUsize,
}
struct SqlFault {
    flags: Box<FaultFlags>,
    handle: *mut rusqlite::ffi::sqlite3,
}
impl SqlFault {
    fn install(connection: &Connection) -> Self {
        unsafe extern "C" fn authorize(
            context: *mut std::ffi::c_void,
            action: std::ffi::c_int,
            first: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
        ) -> std::ffi::c_int {
            // Box address remains stable until the hook is removed in Drop;
            // SQLite supplies NUL-terminated strings for non-null arguments.
            let flags = unsafe { &*context.cast::<FaultFlags>() };
            if action == rusqlite::ffi::SQLITE_INSERT
                && !first.is_null()
                && unsafe { std::ffi::CStr::from_ptr(first) }.to_bytes() == b"prepared_workspaces"
            {
                flags.workspace_inserts.fetch_add(1, Ordering::Relaxed);
            }
            let commit = action == rusqlite::ffi::SQLITE_TRANSACTION
                && !first.is_null()
                && unsafe { std::ffi::CStr::from_ptr(first) }.to_bytes() == b"COMMIT";
            if (flags.read.load(Ordering::Relaxed) && action == rusqlite::ffi::SQLITE_READ)
                || (flags.commit.load(Ordering::Relaxed) && commit)
            {
                flags.hits.fetch_add(1, Ordering::Relaxed);
                rusqlite::ffi::SQLITE_DENY
            } else {
                rusqlite::ffi::SQLITE_OK
            }
        }
        let flags = Box::new(FaultFlags {
            read: AtomicBool::new(false),
            commit: AtomicBool::new(false),
            hits: AtomicUsize::new(0),
            workspace_inserts: AtomicUsize::new(0),
        });
        let handle = unsafe { connection.handle() };
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_set_authorizer(
                    handle,
                    Some(authorize),
                    (&*flags as *const FaultFlags).cast_mut().cast(),
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        Self { flags, handle }
    }

    fn arm(&self, read: bool) {
        self.flags.read.store(read, Ordering::Relaxed);
        self.flags.commit.store(!read, Ordering::Relaxed);
    }

    fn clear(&self) {
        self.flags.read.store(false, Ordering::Relaxed);
        self.flags.commit.store(false, Ordering::Relaxed);
    }
}

#[test]
fn snapshot_maintenance_workspace_dml_commit_failure_and_cancel_rollback() {
    for cancel in [false, true] {
        let (_root, path) = super::reopen_derived::members_fixture(3, false);
        let mut connection = connection(&path);
        let observer = self::connection(&path);
        let fault = SqlFault::install(&connection);
        let events = table_rows(&observer, "events");
        let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
        let mut armed = false;
        for _ in 0..STEP_GUARD {
            one_pending(&mut session);
            armed=observer.query_row("SELECT EXISTS(SELECT 1 FROM lifetime_materialization_job WHERE phase='workspaces' AND state='attempting')",[],|r|r.get(0)).unwrap();
            if armed {
                break;
            }
        }
        assert!(armed);
        assert_eq!(fault.flags.workspace_inserts.load(Ordering::Relaxed), 0);
        let before = observed(&observer, &path);
        if cancel {
            assert!(matches!(
                session
                    .step(&mut || fault.flags.workspace_inserts.load(Ordering::Relaxed) > 0)
                    .unwrap(),
                ReopenMaintenanceStep::Cancelled
            ));
        } else {
            fault.arm(false);
            assert!(session.step(&mut || false).is_err());
            assert!(fault.flags.hits.load(Ordering::Relaxed) > 0);
            fault.clear();
        }
        assert!(
            fault.flags.workspace_inserts.load(Ordering::Relaxed) > 0,
            "did not reach actual workspace INSERT"
        );
        assert!(session.step(&mut || false).is_err());
        drop(session);
        assert_eq!(table_rows(&connection, "events"), events);
        assert!(table_rows(&connection, "prepared_workspaces").is_empty());
        if cancel {
            let state: (String, i64) = connection
                .query_row(
                    "SELECT state,after_scan_rowid FROM lifetime_materialization_job",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(state, ("pending".into(), 0));
            let mut resumed = ReopenMaintenanceSession::start(&mut connection).unwrap();
            complete(&mut resumed);
        } else {
            unchanged(&connection, &path, &before);
        }
    }
}
impl Drop for SqlFault {
    fn drop(&mut self) {
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(self.handle, None, std::ptr::null_mut());
        }
    }
}

#[test]
fn snapshot_maintenance_sql_read_and_commit_failures_rollback_and_forbid_resume() {
    for deny_read in [false, true] {
        for boundary in [0, 1, 2] {
            let (_root, path) = fixture(false, 2);
            let mut connection = connection(&path);
            let observer = self::connection(&path);
            let fault = SqlFault::install(&connection);
            if boundary == 0 {
                let before_start = observed(&observer, &path);
                fault.arm(deny_read);
                assert!(ReopenMaintenanceSession::start(&mut connection).is_err());
                fault.clear();
                assert!(fault.flags.hits.load(Ordering::Relaxed) > 0);
                unchanged(&connection, &path, &before_start);
                continue;
            }
            let mut session = ReopenMaintenanceSession::start(&mut connection).unwrap();
            if boundary == 2 {
                one_pending(&mut session);
            }
            let before_failure = observed(&observer, &path);
            fault.arm(deny_read);
            assert!(session.step(&mut || false).is_err());
            fault.clear();
            assert!(fault.flags.hits.load(Ordering::Relaxed) > 0);
            assert!(session.step(&mut || false).is_err());
            drop(session);
            unchanged(&connection, &path, &before_failure);
        }
    }
}
