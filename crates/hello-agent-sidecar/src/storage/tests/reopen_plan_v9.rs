use super::*;
use crate::storage::reopen::snapshot::{ReopenMaintenanceSession, ReopenMaintenanceStep};
use std::sync::atomic::AtomicBool;

const STAMP: &str = "2026-09-08T04:00:00Z";
const PROJECT_ID: &str = "project_v9_admission";

fn v9_template(count: usize) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    for sequence in 1..=count {
        storage
            .append_event(&event(
                &format!("evt_v9_{sequence}"),
                &format!("op_v9_{sequence}"),
                "project",
                PROJECT_ID,
                sequence as u64,
                json!({
                    "canonical_path":"/fixture/v9-admission",
                    "project_id":PROJECT_ID,
                    "sequence":sequence,
                }),
                STAMP,
            ))
            .unwrap();
    }
    run_bounded_materializer_to_ready(&mut storage);
    storage.checkpoint().unwrap();
    drop(storage);
    let cancellation = AtomicBool::new(false);
    let storage = Storage::open_for_admission(&path, &cancellation).unwrap();
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}

fn copy_template(path: &Path) -> (TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let copy = root.path().join("hello-agent.sqlite3");
    fs::copy(path, &copy).unwrap();
    (root, copy)
}

fn private_state(phase: &str, after: u32, exhausted: bool) -> Value {
    let phase_index = [
        "planEligibility",
        "freezeOperations",
        "freezeMessages",
        "verifyPrefix",
        "authorizeDelete",
        "deletePrefix",
        "verifyOperations",
        "verifyEquivalence",
        "releasePlan",
        "finalize",
    ]
    .iter()
    .position(|value| *value == phase)
    .unwrap();
    json!({"storageCompactionV3PrivateState":{
        "schemaVersion":3,
        "cursor":{
            "schemaVersion":3,
            "cutoffEventRowid":1001,
            "phase":phase,
            "afterPlanOrdinal":after,
            "stepThroughPlanOrdinal":null,
            "scanExhausted":exhausted,
        },
        "planRowsComplete":phase_index>0 || exhausted,
        "operationsVerified":phase_index>6 || (phase_index==6 && exhausted),
        "equivalenceVerified":phase_index>7 || (phase_index==7 && exhausted),
        "planRowsReleased":phase_index>8 || (phase_index==8 && exhausted),
    }})
}

fn terminal_state(cutoff: u64) -> Value {
    let mut state = private_state("finalize", 1000, true);
    state["storageCompactionV3PrivateState"]["cursor"]["cutoffEventRowid"] = json!(cutoff);
    state
}

fn with_triggers_disabled(connection: &Connection, mutate: impl FnOnce(&Connection)) {
    let triggers: Vec<(String, String)> = connection
        .prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger' ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF; PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    for (name, _) in &triggers {
        connection
            .execute_batch(&format!("DROP TRIGGER \"{}\";", name.replace('"', "\"\"")))
            .unwrap();
    }
    mutate(connection);
    for (_, definition) in triggers {
        connection.execute_batch(&definition).unwrap();
    }
    connection
        .execute_batch("PRAGMA ignore_check_constraints=OFF; PRAGMA foreign_keys=ON;")
        .unwrap();
}

fn insert_baseline(connection: &Connection, sequence: u64) -> (String, String) {
    let canonical: String = connection
        .query_row(
            "SELECT canonical_json_payload FROM events WHERE aggregate_type='project' \
             AND aggregate_id=?1 AND aggregate_sequence=?2",
            params![PROJECT_ID, sequence as i64],
            |row| row.get(0),
        )
        .unwrap();
    let versioned = sha256_file_version_v1(canonical.as_bytes());
    let raw = versioned.strip_prefix("sha256:").unwrap().to_string();
    use sha2::{Digest, Sha256};
    let mut identity = Sha256::new();
    identity.update(b"project\0");
    identity.update(PROJECT_ID.as_bytes());
    identity.update([0]);
    identity.update(sequence.to_string().as_bytes());
    let baseline_id = format!("baseline_{:x}", identity.finalize());
    connection
        .execute(
            "INSERT INTO ledger_baselines(\
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at\
             ) VALUES (?1,'project',?2,?3,1,?4,?5,?6,?6)",
            params![baseline_id, PROJECT_ID, sequence as i64, canonical, versioned, STAMP],
        )
        .unwrap();
    (baseline_id, raw)
}

fn insert_job(
    connection: &Connection,
    job_id: &str,
    state: &str,
    private: &Value,
    completed: Option<&str>,
) {
    connection
        .execute(
            "INSERT INTO maintenance_jobs(\
               job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
               failure_kind,created_at,updated_at\
             ) VALUES (?1,'losslessCompaction',?2,?3,NULL,?4,?4,NULL,?5,?5)",
            params![job_id, state, private.to_string(), completed, STAMP],
        )
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn insert_plan(
    connection: &Connection,
    job_id: &str,
    retained: u64,
    cutoff: u64,
    baseline_id: &str,
    baseline_digest: &str,
    previous: Option<&str>,
) {
    connection
        .execute(
            "INSERT INTO storage_compaction_plans(\
               job_id,plan_schema_version,cutoff_event_rowid,aggregate_type,aggregate_id,\
               retained_boundary_sequence,delete_through_sequence,anchor_aggregate_sequence,\
               expected_rows,baseline_id,baseline_projection_digest,previous_successful_job_id,\
               created_at,planned_at\
             ) VALUES (?1,1,?2,'project',?3,?4,?5,?6,1001,?7,?8,?9,?10,?10)",
            params![
                job_id,
                cutoff as i64,
                PROJECT_ID,
                retained as i64,
                (retained + 1000) as i64,
                (retained + 1001) as i64,
                baseline_id,
                baseline_digest,
                previous,
                STAMP,
            ],
        )
        .unwrap();
}

fn insert_plan_row(
    connection: &Connection,
    job_id: &str,
    ordinal: u32,
    retained: u64,
    baseline_id: &str,
    baseline_digest: &str,
) {
    let sequence = retained + u64::from(ordinal);
    let anchor = ordinal == 1001;
    let (event_rowid, event_id, operation_id, request_id, event_type, payload, timestamp): (
        i64,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
    ) = connection
        .query_row(
            "SELECT rowid,event_id,operation_id,request_id,event_type,canonical_json_payload,timestamp \
             FROM events WHERE aggregate_type='project' AND aggregate_id=?1 \
               AND aggregate_sequence=?2",
            params![PROJECT_ID, sequence as i64],
            |row| {
                Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                    row.get(5)?, row.get(6)?,
                ))
            },
        )
        .unwrap();
    let digest = sha256_file_version_v1(payload.as_bytes())
        .strip_prefix("sha256:")
        .unwrap()
        .to_string();
    connection
        .execute(
            "INSERT INTO storage_compaction_plan_rows(\
               job_id,ordinal,event_rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,\
               aggregate_sequence,event_type,payload_schema_version,event_timestamp,canonical_payload_json,\
               canonical_payload_digest,introduced_message_id,introduced_message_digest,decision,\
               baseline_id,baseline_projection_digest,operation_frozen_before_plan,message_frozen_before_plan\
             ) VALUES (?1,?2,?3,?4,?5,?6,'project',?7,?8,?9,1,?10,?11,?12,NULL,NULL,\
                       ?13,?14,?15,0,0)",
            params![
                job_id,
                ordinal,
                event_rowid,
                event_id,
                operation_id,
                request_id,
                PROJECT_ID,
                sequence as i64,
                event_type,
                timestamp,
                payload,
                digest,
                if anchor { "retainAnchor" } else { "delete" },
                if anchor { None } else { Some(baseline_id) },
                if anchor { None } else { Some(baseline_digest) },
            ],
        )
        .unwrap();
}

fn insert_prefix(
    connection: &Connection,
    job_id: &str,
    retained: u64,
    baseline_id: &str,
    baseline_digest: &str,
) {
    let anchor = retained + 1001;
    connection
        .execute(
            "INSERT INTO ledger_compaction_prefixes(\
               prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,\
               anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,\
               baseline_projection_digest,created_at,verified_at\
             ) SELECT ?1,'project',?2,?3,rowid,event_id,operation_id,?4,?5,'sha256:'||?6,?7,?7 \
               FROM events WHERE aggregate_type='project' AND aggregate_id=?2 \
                 AND aggregate_sequence=?4",
            params![
                format!("prefix_{job_id}"),
                PROJECT_ID,
                (retained + 1000) as i64,
                anchor as i64,
                baseline_id,
                baseline_digest,
                STAMP,
            ],
        )
        .unwrap();
}

fn freeze_range(storage: &mut Storage, start: u64, end: u64) {
    for sequence in start..=end {
        let source = event(
            &format!("evt_v9_{sequence}"),
            &format!("op_v9_{sequence}"),
            "project",
            PROJECT_ID,
            sequence,
            json!({
                "canonical_path":"/fixture/v9-admission",
                "project_id":PROJECT_ID,
                "sequence":sequence,
            }),
            STAMP,
        );
        insert_exact_v8_frozen_operation(storage, &source, STAMP);
    }
}

fn delete_live_prefix(connection: &Connection, through: u64) {
    connection
        .execute(
            "DELETE FROM event_source_index WHERE aggregate_type='project' \
             AND aggregate_id=?1 AND aggregate_sequence<=?2",
            params![PROJECT_ID, through as i64],
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM events WHERE aggregate_type='project' \
             AND aggregate_id=?1 AND aggregate_sequence<=?2",
            params![PROJECT_ID, through as i64],
        )
        .unwrap();
}

fn seed_pending(path: &Path, phase: &str, after: u32) {
    let mut storage = Storage {
        connection: Connection::open(path).unwrap(),
        path: path.to_path_buf(),
        writes_admitted: true,
    };
    let (baseline_id, baseline_digest) = insert_baseline(&storage.connection, 1000);
    if phase == "freezeOperations" {
        freeze_range(&mut storage, 1, u64::from(after));
    } else if matches!(phase, "verifyOperations" | "releasePlan") {
        freeze_range(&mut storage, 1, 1001);
    }
    with_triggers_disabled(&storage.connection, |connection| {
        insert_job(
            connection,
            "compact_v9_001",
            "pending",
            &private_state(phase, after, false),
            None,
        );
        insert_plan(
            connection,
            "compact_v9_001",
            0,
            1001,
            &baseline_id,
            &baseline_digest,
            None,
        );
        let start = if phase == "releasePlan" { after + 1 } else { 1 };
        let end = if phase == "planEligibility" {
            after
        } else {
            1001
        };
        for ordinal in start..=end {
            insert_plan_row(
                connection,
                "compact_v9_001",
                ordinal,
                0,
                &baseline_id,
                &baseline_digest,
            );
        }
        if matches!(phase, "verifyOperations" | "releasePlan") {
            insert_prefix(
                connection,
                "compact_v9_001",
                0,
                &baseline_id,
                &baseline_digest,
            );
            delete_live_prefix(connection, 1000);
        }
    });
    storage.checkpoint().unwrap();
}

fn witness_snapshot(connection: &Connection) -> Vec<Vec<Vec<SqlValue>>> {
    [
        "maintenance_jobs",
        "storage_compaction_plans",
        "storage_compaction_plan_rows",
        "ledger_compaction_delete_authority",
    ]
    .into_iter()
    .map(|table| table_rows(connection, table))
    .collect()
}

#[test]
fn product_admission_accepts_exact_pending_v9_plan_phases_without_mutation() {
    let (_root, template) = v9_template(1001);
    for (phase, after) in [
        ("planEligibility", 137),
        ("freezeOperations", 137),
        ("verifyOperations", 137),
        ("releasePlan", 137),
    ] {
        let (_copy_root, path) = copy_template(&template);
        seed_pending(&path, phase, after);
        let connection = Connection::open(&path).unwrap();
        let before = witness_snapshot(&connection);
        drop(connection);
        let cancellation = AtomicBool::new(false);
        let mut admitted = Storage::open_for_admission(&path, &cancellation)
            .unwrap_or_else(|error| panic!("{phase}: {error:?}"));
        assert_eq!(witness_snapshot(&admitted.connection), before, "{phase}");
        assert!(admitted.connection.is_autocommit(), "{phase}");
        if phase == "planEligibility" {
            assert_eq!(
                admitted
                    .append_event(&event(
                        "evt_v9_1002",
                        "op_v9_1002",
                        "project",
                        PROJECT_ID,
                        1002,
                        json!({
                            "canonical_path":"/fixture/v9-admission",
                            "project_id":PROJECT_ID,
                            "sequence":1002,
                        }),
                        STAMP,
                    ))
                    .unwrap(),
                AppendOutcome::Committed
            );
        }
    }
}

#[test]
fn product_admission_rejects_v2_running_and_authority_control_states_without_mutation() {
    let (_root, template) = v9_template(1001);
    for case in ["v2", "running", "authority"] {
        let (_copy_root, path) = copy_template(&template);
        if case == "v2" {
            let connection = Connection::open(&path).unwrap();
            let cutoff = json!({"storageCompactionV2PrivateState":{
                "schemaVersion":1,
                "cursor":{"schemaVersion":2,"cutoffEventRowid":1001,"phase":"finalize",
                          "afterEventRowid":1001,"scanExhausted":true},
                "operationsVerified":true,"equivalenceVerified":true,
            }});
            insert_job(&connection, "compact_v2_pending", "pending", &cutoff, None);
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                .unwrap();
        } else {
            seed_pending(&path, "planEligibility", 137);
            let connection = Connection::open(&path).unwrap();
            with_triggers_disabled(&connection, |connection| {
                if case == "running" {
                    connection
                        .execute(
                            "UPDATE maintenance_jobs SET state='running' WHERE job_id='compact_v9_001'",
                            [],
                        )
                        .unwrap();
                } else {
                    let (baseline_id, baseline_digest): (String, String) = connection
                        .query_row(
                            "SELECT baseline_id,baseline_projection_digest \
                             FROM storage_compaction_plans WHERE job_id='compact_v9_001'",
                            [],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .unwrap();
                    for ordinal in 138..=1001 {
                        insert_plan_row(
                            connection,
                            "compact_v9_001",
                            ordinal,
                            0,
                            &baseline_id,
                            &baseline_digest,
                        );
                    }
                    insert_prefix(
                        connection,
                        "compact_v9_001",
                        0,
                        &baseline_id,
                        &baseline_digest,
                    );
                    connection.execute(
                        "INSERT INTO ledger_compaction_delete_authority(\
                           job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,\
                           aggregate_id,aggregate_sequence,authorized_at\
                         ) SELECT 'compact_v9_001',1,'prefix_compact_v9_001',rowid,event_id,\
                                  operation_id,aggregate_type,aggregate_id,aggregate_sequence,?1 \
                           FROM events WHERE aggregate_type='project' AND aggregate_id=?2 \
                             AND aggregate_sequence=1",
                        params![STAMP, PROJECT_ID],
                    ).unwrap();
                }
            });
        }
        let connection = Connection::open(&path).unwrap();
        let before = witness_snapshot(&connection);
        drop(connection);
        let cancellation = AtomicBool::new(false);
        let error = match Storage::open_for_admission(&path, &cancellation) {
            Ok(_) => panic!("accepted {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(witness_snapshot(&connection), before, "{case}");
        assert!(connection.is_autocommit(), "{case}");
    }
}

#[test]
fn product_admission_accepts_two_succeeded_v9_plans_as_one_bounded_chain() {
    let (_root, path) = v9_template(2001);
    let mut storage = Storage {
        connection: Connection::open(&path).unwrap(),
        path: path.clone(),
        writes_admitted: true,
    };
    let (baseline_1000, digest_1000) = insert_baseline(&storage.connection, 1000);
    let (baseline_2000, digest_2000) = insert_baseline(&storage.connection, 2000);
    freeze_range(&mut storage, 1, 2001);
    with_triggers_disabled(&storage.connection, |connection| {
        insert_job(
            connection,
            "compact_v9_001",
            "succeeded",
            &terminal_state(1001),
            Some(STAMP),
        );
        insert_plan(
            connection,
            "compact_v9_001",
            0,
            1001,
            &baseline_1000,
            &digest_1000,
            None,
        );
        insert_plan_row(
            connection,
            "compact_v9_001",
            1001,
            0,
            &baseline_1000,
            &digest_1000,
        );
        insert_prefix(
            connection,
            "compact_v9_001",
            0,
            &baseline_1000,
            &digest_1000,
        );

        insert_job(
            connection,
            "compact_v9_002",
            "succeeded",
            &terminal_state(2001),
            Some(STAMP),
        );
        insert_plan(
            connection,
            "compact_v9_002",
            1000,
            2001,
            &baseline_2000,
            &digest_2000,
            Some("compact_v9_001"),
        );
        insert_plan_row(
            connection,
            "compact_v9_002",
            1001,
            1000,
            &baseline_2000,
            &digest_2000,
        );
        insert_prefix(
            connection,
            "compact_v9_002",
            1000,
            &baseline_2000,
            &digest_2000,
        );
        delete_live_prefix(connection, 2000);
    });
    storage.checkpoint().unwrap();
    let before = witness_snapshot(&storage.connection);
    drop(storage);

    let cancellation = AtomicBool::new(false);
    let admitted = Storage::open_for_admission(&path, &cancellation).unwrap();
    assert_eq!(witness_snapshot(&admitted.connection), before);
    assert!(admitted.connection.is_autocommit());
    admitted.checkpoint().unwrap();
    drop(admitted);

    for case in ["gap", "cycle", "ambiguous_v2"] {
        let (_copy_root, copy) = copy_template(&path);
        let connection = Connection::open(&copy).unwrap();
        with_triggers_disabled(&connection, |connection| match case {
            "gap" => {
                connection
                    .execute(
                        "UPDATE storage_compaction_plans SET previous_successful_job_id=NULL \
                         WHERE job_id='compact_v9_002'",
                        [],
                    )
                    .unwrap();
            }
            "cycle" => {
                connection
                    .execute(
                        "UPDATE storage_compaction_plans \
                         SET previous_successful_job_id='compact_v9_002' \
                         WHERE job_id='compact_v9_001'",
                        [],
                    )
                    .unwrap();
            }
            "ambiguous_v2" => {
                let v2 = json!({"storageCompactionV2PrivateState":{
                    "schemaVersion":1,
                    "cursor":{"schemaVersion":2,"cutoffEventRowid":2001,"phase":"finalize",
                              "afterEventRowid":2001,"scanExhausted":true},
                    "operationsVerified":true,"equivalenceVerified":true,
                }});
                insert_job(connection, "a_v2_1", "succeeded", &v2, Some(STAMP));
                insert_job(connection, "a_v2_2", "succeeded", &v2, Some(STAMP));
            }
            _ => unreachable!(),
        });
        let before = witness_snapshot(&connection);
        drop(connection);
        let error = match Storage::open_for_admission(&copy, &cancellation) {
            Ok(_) => panic!("accepted terminal {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
        let connection = Connection::open(&copy).unwrap();
        assert_eq!(witness_snapshot(&connection), before, "{case}");
    }
}

#[test]
fn product_admission_rejects_pending_v9_witness_gaps_extras_and_hostile_digests() {
    let (_root, template) = v9_template(1001);
    for case in [
        "gap",
        "extra",
        "correlated_digest",
        "oversized_baseline_digest",
        "large_job_id_buffers",
        "artifact",
    ] {
        let (_copy_root, path) = copy_template(&template);
        let phase = if case == "correlated_digest" {
            "freezeOperations"
        } else {
            "planEligibility"
        };
        let after = if case == "large_job_id_buffers" {
            0
        } else {
            137
        };
        seed_pending(&path, phase, after);
        let connection = Connection::open(&path).unwrap();
        with_triggers_disabled(&connection, |connection| match case {
            "gap" => {
                connection
                    .execute(
                        "DELETE FROM storage_compaction_plan_rows WHERE ordinal=50",
                        [],
                    )
                    .unwrap();
            }
            "extra" => {
                let (baseline_id, baseline_digest): (String, String) = connection.query_row(
                    "SELECT baseline_id,baseline_projection_digest FROM storage_compaction_plans",
                    [], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
                insert_plan_row(
                    connection,
                    "compact_v9_001",
                    138,
                    0,
                    &baseline_id,
                    &baseline_digest,
                );
            }
            "correlated_digest" => {
                let wrong = "a".repeat(64);
                let operation_id: String = connection
                    .query_row(
                        "SELECT operation_id FROM storage_compaction_plan_rows WHERE ordinal=1",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                connection
                    .execute(
                        "UPDATE storage_compaction_plan_rows SET canonical_payload_digest=?1 \
                         WHERE ordinal=1",
                        [&wrong],
                    )
                    .unwrap();
                connection
                    .execute(
                        "UPDATE frozen_operation_sources SET canonical_payload_digest=?1 \
                         WHERE operation_id=?2",
                        params![wrong, operation_id],
                    )
                    .unwrap();
                connection
                    .execute(
                        "UPDATE operation_receipts SET canonical_payload_digest=?1 \
                         WHERE operation_id=?2",
                        params![wrong, operation_id],
                    )
                    .unwrap();
            }
            "oversized_baseline_digest" => {
                connection
                    .execute(
                        "UPDATE ledger_baselines SET projection_digest=?1",
                        params!["a".repeat(8 * 1024 * 1024 + 1)],
                    )
                    .unwrap();
            }
            "large_job_id_buffers" => {
                let large_job_id = format!("large_{}", "j".repeat(4 * 1024 * 1024));
                connection
                    .execute(
                        "UPDATE maintenance_jobs SET job_id=?1 WHERE job_id='compact_v9_001'",
                        [&large_job_id],
                    )
                    .unwrap();
                connection
                    .execute(
                        "UPDATE storage_compaction_plans SET job_id=?1 \
                         WHERE job_id='compact_v9_001'",
                        [&large_job_id],
                    )
                    .unwrap();
            }
            "artifact" => {
                let private = private_state("freezeOperations", 137, false);
                connection
                    .execute(
                        "UPDATE maintenance_jobs SET cutoff_json=?1",
                        [private.to_string()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        });
        let before = witness_snapshot(&connection);
        let related_before = [
            "ledger_baselines",
            "frozen_operation_sources",
            "operation_receipts",
        ]
        .map(|table| table_rows(&connection, table));
        drop(connection);
        let cancellation = AtomicBool::new(false);
        let error = match Storage::open_for_admission(&path, &cancellation) {
            Ok(_) => panic!("accepted {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(witness_snapshot(&connection), before, "{case}");
        let related_after = [
            "ledger_baselines",
            "frozen_operation_sources",
            "operation_receipts",
        ]
        .map(|table| table_rows(&connection, table));
        assert_eq!(related_after, related_before, "{case}");
        assert!(connection.is_autocommit(), "{case}");
    }
}

#[test]
fn pending_v9_plan_cancellation_no_progress_and_external_commit_fail_closed() {
    let (_root, template) = v9_template(1001);
    let (_copy_root, path) = copy_template(&template);
    seed_pending(&path, "planEligibility", 137);

    let mut connection = Connection::open(&path).unwrap();
    let mut session = ReopenMaintenanceSession::start_for_admission(&mut connection).unwrap();
    assert_eq!(
        session.step(&mut || true).unwrap(),
        ReopenMaintenanceStep::Cancelled
    );
    drop(session);
    assert!(connection.is_autocommit());

    let mut session = ReopenMaintenanceSession::start_for_admission(&mut connection).unwrap();
    session.force_no_progress_for_test();
    assert_eq!(
        session.step(&mut || false).unwrap_err().kind(),
        "storageSchemaMismatch"
    );
    drop(session);
    assert!(connection.is_autocommit());

    let mut session = ReopenMaintenanceSession::start_for_admission(&mut connection).unwrap();
    let external = Connection::open(&path).unwrap();
    external.execute("PRAGMA user_version=37", []).unwrap();
    assert_eq!(
        session.step(&mut || false).unwrap_err().kind(),
        "storageSchemaMismatch"
    );
    drop(session);
    assert!(connection.is_autocommit());
}
