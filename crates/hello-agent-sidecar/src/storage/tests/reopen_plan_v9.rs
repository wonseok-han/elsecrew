use super::*;
use crate::storage::reopen::snapshot::{ReopenMaintenanceSession, ReopenMaintenanceStep};
use std::sync::atomic::AtomicBool;

const STAMP: &str = "2026-09-08T04:00:00Z";
const PROJECT_ID: &str = "project_v9_admission";
const CHAT_ID: &str = "chat_v9_admission";

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

fn chat_v9_template(with_message: bool) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    storage
        .append_event(&event(
            "evt_v9_chat_project",
            "op_v9_chat_project",
            "project",
            PROJECT_ID,
            1,
            json!({
                "canonical_path":"/fixture/v9-chat-admission",
                "project_id":PROJECT_ID,
                "sequence":1,
            }),
            STAMP,
        ))
        .unwrap();
    for sequence in 1..=1001 {
        let (durable, last_message) = if with_message {
            (1, team_message(CHAT_ID, "message_v9_chat_1", 1))
        } else {
            (0, Value::Null)
        };
        storage
            .append_event(&event(
                &format!("evt_v9_chat_{sequence}"),
                &format!("op_v9_chat_{sequence}"),
                "chat",
                CHAT_ID,
                sequence,
                materializable_chat_payload(CHAT_ID, PROJECT_ID, durable, last_message),
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

fn chat_private_state(phase: &str, after: u32, exhausted: bool) -> Value {
    let mut state = private_state(phase, after, exhausted);
    state["storageCompactionV3PrivateState"]["cursor"]["cutoffEventRowid"] = json!(1002);
    state
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
    insert_baseline_for(connection, "project", PROJECT_ID, sequence)
}

fn insert_baseline_for(
    connection: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
    sequence: u64,
) -> (String, String) {
    let canonical: String = connection
        .query_row(
            "SELECT canonical_json_payload FROM events WHERE aggregate_type=?1 \
             AND aggregate_id=?2 AND aggregate_sequence=?3",
            params![aggregate_type, aggregate_id, sequence as i64],
            |row| row.get(0),
        )
        .unwrap();
    let versioned = sha256_file_version_v1(canonical.as_bytes());
    let raw = versioned.strip_prefix("sha256:").unwrap().to_string();
    use sha2::{Digest, Sha256};
    let mut identity = Sha256::new();
    identity.update(aggregate_type.as_bytes());
    identity.update([0]);
    identity.update(aggregate_id.as_bytes());
    identity.update([0]);
    identity.update(sequence.to_string().as_bytes());
    let baseline_id = format!("baseline_{:x}", identity.finalize());
    connection
        .execute(
            "INSERT INTO ledger_baselines(\
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at\
             ) VALUES (?1,?2,?3,?4,1,?5,?6,?7,?7)",
            params![
                baseline_id,
                aggregate_type,
                aggregate_id,
                sequence as i64,
                canonical,
                versioned,
                STAMP
            ],
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
    insert_plan_for(
        connection,
        job_id,
        "project",
        PROJECT_ID,
        retained,
        cutoff,
        baseline_id,
        baseline_digest,
        previous,
    );
}

#[allow(clippy::too_many_arguments)]
fn insert_plan_for(
    connection: &Connection,
    job_id: &str,
    aggregate_type: &str,
    aggregate_id: &str,
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
             ) VALUES (?1,1,?2,?3,?4,?5,?6,?7,1001,?8,?9,?10,?11,?11)",
            params![
                job_id,
                cutoff as i64,
                aggregate_type,
                aggregate_id,
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
    insert_plan_row_for(
        connection,
        job_id,
        "project",
        PROJECT_ID,
        ordinal,
        retained,
        baseline_id,
        baseline_digest,
    );
}

#[allow(clippy::too_many_arguments)]
fn insert_plan_row_for(
    connection: &Connection,
    job_id: &str,
    aggregate_type: &str,
    aggregate_id: &str,
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
             FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 \
               AND aggregate_sequence=?3",
            params![aggregate_type, aggregate_id, sequence as i64],
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
    let introduced_message_id: Option<String> = connection
        .query_row(
            "SELECT introduced_message_id FROM event_source_index WHERE event_rowid=?1",
            [event_rowid],
            |row| row.get(0),
        )
        .unwrap();
    let introduced_message_digest = introduced_message_id.as_ref().map(|message_id| {
        let digest: String = connection
            .query_row(
                "SELECT message_digest FROM chat_messages WHERE message_id=?1",
                [message_id],
                |row| row.get(0),
            )
            .unwrap();
        digest.strip_prefix("sha256:").unwrap().to_string()
    });
    connection
        .execute(
            "INSERT INTO storage_compaction_plan_rows(\
               job_id,ordinal,event_rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,\
               aggregate_sequence,event_type,payload_schema_version,event_timestamp,canonical_payload_json,\
               canonical_payload_digest,introduced_message_id,introduced_message_digest,decision,\
               baseline_id,baseline_projection_digest,operation_frozen_before_plan,message_frozen_before_plan\
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1,?11,?12,?13,?14,?15,\
                       ?16,?17,?18,0,0)",
            params![
                job_id,
                ordinal,
                event_rowid,
                event_id,
                operation_id,
                request_id,
                aggregate_type,
                aggregate_id,
                sequence as i64,
                event_type,
                timestamp,
                payload,
                digest,
                introduced_message_id,
                introduced_message_digest,
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
    insert_prefix_for(
        connection,
        job_id,
        "project",
        PROJECT_ID,
        retained,
        baseline_id,
        baseline_digest,
    );
}

#[allow(clippy::too_many_arguments)]
fn insert_prefix_for(
    connection: &Connection,
    job_id: &str,
    aggregate_type: &str,
    aggregate_id: &str,
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
             ) SELECT ?1,?2,?3,?4,rowid,event_id,operation_id,?5,?6,'sha256:'||?7,?8,?8 \
               FROM events WHERE aggregate_type=?2 AND aggregate_id=?3 \
                 AND aggregate_sequence=?5",
            params![
                format!("prefix_{job_id}"),
                aggregate_type,
                aggregate_id,
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
    freeze_range_for(storage, "project", PROJECT_ID, start, end);
}

fn freeze_range_for(
    storage: &mut Storage,
    aggregate_type: &str,
    aggregate_id: &str,
    start: u64,
    end: u64,
) {
    for sequence in start..=end {
        let source = storage
            .connection
            .query_row(
                "SELECT event_id,operation_id,request_id,aggregate_type,aggregate_id,\
                        aggregate_sequence,event_type,payload_schema_version,\
                        canonical_json_payload,timestamp FROM events \
                 WHERE aggregate_type=?1 AND aggregate_id=?2 AND aggregate_sequence=?3",
                params![aggregate_type, aggregate_id, sequence as i64],
                |row| {
                    Ok(LedgerEventV1 {
                        event_id: row.get(0)?,
                        operation_id: row.get(1)?,
                        request_id: row.get(2)?,
                        aggregate_type: row.get(3)?,
                        aggregate_id: row.get(4)?,
                        aggregate_sequence: row_u64(row, 5)?,
                        event_type: row.get(6)?,
                        payload_schema_version: row.get(7)?,
                        canonical_json_payload: row.get(8)?,
                        timestamp: row.get(9)?,
                    })
                },
            )
            .unwrap();
        insert_exact_v8_frozen_operation(storage, &source, STAMP);
    }
}

fn freeze_chat_messages(storage: &mut Storage, aggregate_id: &str) {
    storage
        .connection
        .execute(
            "INSERT INTO frozen_message_sources(\
               message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,\
               source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at\
             ) SELECT message.message_id,source.event_rowid,source.event_id,source.operation_id,\
                      message.chat_id,source.aggregate_sequence,message.event_sequence,\
                      message.canonical_message_json,message.message_digest,?1 \
               FROM event_source_index source \
               JOIN chat_messages message ON message.message_id=source.introduced_message_id \
               WHERE source.aggregate_type='chat' AND source.aggregate_id=?2",
            params![STAMP, aggregate_id],
        )
        .unwrap();
}

fn delete_live_prefix(connection: &Connection, through: u64) {
    delete_live_prefix_for(connection, "project", PROJECT_ID, through);
}

fn delete_live_prefix_for(
    connection: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
    through: u64,
) {
    connection
        .execute(
            "DELETE FROM event_source_index WHERE aggregate_type=?1 \
             AND aggregate_id=?2 AND aggregate_sequence<=?3",
            params![aggregate_type, aggregate_id, through as i64],
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM events WHERE aggregate_type=?1 \
             AND aggregate_id=?2 AND aggregate_sequence<=?3",
            params![aggregate_type, aggregate_id, through as i64],
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
    } else if matches!(
        phase,
        "verifyPrefix" | "authorizeDelete" | "deletePrefix" | "verifyOperations" | "releasePlan"
    ) {
        freeze_range(&mut storage, 1, 1001);
    }
    with_triggers_disabled(&storage.connection, |connection| {
        insert_job(
            connection,
            "compact_v9_001",
            "pending",
            &private_state(
                phase,
                after,
                matches!(phase, "verifyPrefix" | "authorizeDelete") && after == 1001,
            ),
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
        if matches!(
            phase,
            "authorizeDelete" | "deletePrefix" | "verifyOperations" | "releasePlan"
        ) {
            insert_prefix(
                connection,
                "compact_v9_001",
                0,
                &baseline_id,
                &baseline_digest,
            );
            let delete_through = if phase == "deletePrefix" {
                u64::from(after)
            } else if matches!(phase, "verifyOperations" | "releasePlan") {
                1000
            } else {
                0
            };
            if delete_through > 0 {
                delete_live_prefix(connection, delete_through);
            }
        }
    });
    storage.checkpoint().unwrap();
}

fn seed_chat_pending(path: &Path, phase: &str, after: u32) {
    let mut storage = Storage {
        connection: Connection::open(path).unwrap(),
        path: path.to_path_buf(),
        writes_admitted: true,
    };
    let (baseline_id, baseline_digest) =
        insert_baseline_for(&storage.connection, "chat", CHAT_ID, 1000);
    if phase == "authorizeDelete" {
        freeze_range_for(&mut storage, "chat", CHAT_ID, 1, 1001);
        freeze_chat_messages(&mut storage, CHAT_ID);
    }
    with_triggers_disabled(&storage.connection, |connection| {
        insert_job(
            connection,
            "compact_v9_chat_001",
            "pending",
            &chat_private_state(phase, after, false),
            None,
        );
        insert_plan_for(
            connection,
            "compact_v9_chat_001",
            "chat",
            CHAT_ID,
            0,
            1002,
            &baseline_id,
            &baseline_digest,
            None,
        );
        let end = if phase == "planEligibility" {
            after
        } else {
            1001
        };
        for ordinal in 1..=end {
            insert_plan_row_for(
                connection,
                "compact_v9_chat_001",
                "chat",
                CHAT_ID,
                ordinal,
                0,
                &baseline_id,
                &baseline_digest,
            );
        }
        if phase == "authorizeDelete" {
            insert_prefix_for(
                connection,
                "compact_v9_chat_001",
                "chat",
                CHAT_ID,
                0,
                &baseline_id,
                &baseline_digest,
            );
        }
    });
    storage.checkpoint().unwrap();
}

fn tamper_chat_transition(path: &Path, sequence: u64) {
    let connection = Connection::open(path).unwrap();
    let canonical = canonical_json_v1(materializable_chat_payload(
        CHAT_ID,
        PROJECT_ID,
        2,
        Value::Null,
    ))
    .unwrap();
    let versioned = sha256_file_version_v1(canonical.as_bytes());
    let raw = versioned.strip_prefix("sha256:").unwrap();
    with_triggers_disabled(&connection, |connection| {
        assert_eq!(
            connection
                .execute(
                    "UPDATE events SET canonical_json_payload=?1 \
                     WHERE aggregate_type='chat' AND aggregate_id=?2 \
                       AND aggregate_sequence=?3",
                    params![canonical, CHAT_ID, sequence as i64],
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .execute(
                    "UPDATE operation_receipts \
                     SET canonical_payload_json=?1,canonical_payload_digest=?2 \
                     WHERE aggregate_type='chat' AND aggregate_id=?3 \
                       AND aggregate_sequence=?4",
                    params![canonical, raw, CHAT_ID, sequence as i64],
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .execute(
                    "UPDATE event_source_index SET chat_durable_sequence=2,\
                       chat_snapshot_message_id=NULL,introduced_message_id=NULL \
                     WHERE aggregate_type='chat' AND aggregate_id=?1 \
                       AND aggregate_sequence=?2",
                    params![CHAT_ID, sequence as i64],
                )
                .unwrap(),
            1
        );
        if sequence == 1001 {
            assert_eq!(
                connection
                    .execute(
                        "UPDATE chats SET projection_json=?1 \
                         WHERE chat_id=?2 AND last_event_sequence=1001",
                        params![canonical, CHAT_ID],
                    )
                    .unwrap(),
                1
            );
        }
    });
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}

fn tamper_chat_root_identity(path: &Path) {
    let connection = Connection::open(path).unwrap();
    let canonical = canonical_json_v1(materializable_chat_payload(
        "hostile_chat_v9_admission",
        PROJECT_ID,
        0,
        Value::Null,
    ))
    .unwrap();
    let digest = sha256_file_version_v1(canonical.as_bytes());
    let raw = digest.strip_prefix("sha256:").unwrap();
    with_triggers_disabled(&connection, |connection| {
        assert_eq!(
            connection
                .execute(
                    "UPDATE events SET canonical_json_payload=?1 \
                     WHERE aggregate_type='chat' AND aggregate_id=?2 \
                       AND aggregate_sequence=2",
                    params![canonical, CHAT_ID],
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .execute(
                    "UPDATE operation_receipts \
                     SET canonical_payload_json=?1,canonical_payload_digest=?2 \
                     WHERE aggregate_type='chat' AND aggregate_id=?3 \
                       AND aggregate_sequence=2",
                    params![canonical, raw, CHAT_ID],
                )
                .unwrap(),
            1
        );
    });
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}

fn tamper_project_plan_parity(path: &Path, case: &str) {
    let connection = Connection::open(path).unwrap();
    with_triggers_disabled(&connection, |connection| match case {
        "non_chat_durable" => {
            assert_eq!(
                connection
                    .execute(
                        "UPDATE event_source_index SET chat_durable_sequence=0 \
                         WHERE aggregate_type='project' AND aggregate_id=?1 \
                           AND aggregate_sequence=2",
                        [PROJECT_ID],
                    )
                    .unwrap(),
                1
            );
        }
        "non_chat_snapshot" => {
            assert_eq!(
                connection
                    .execute(
                        "UPDATE event_source_index \
                         SET chat_snapshot_message_id='hostile_message' \
                         WHERE aggregate_type='project' AND aggregate_id=?1 \
                           AND aggregate_sequence=2",
                        [PROJECT_ID],
                    )
                    .unwrap(),
                1
            );
        }
        "mismatched_extra_baseline" => {
            insert_baseline(connection, 2);
            assert_eq!(
                connection
                    .execute(
                        "UPDATE ledger_baselines SET canonical_projection_json='{}' \
                         WHERE aggregate_type='project' AND aggregate_id=?1 \
                           AND cutoff_aggregate_sequence=2",
                        [PROJECT_ID],
                    )
                    .unwrap(),
                1
            );
        }
        _ => unreachable!(),
    });
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}

fn tamper_message_copy_metadata(path: &Path, case: &str) {
    let connection = Connection::open(path).unwrap();
    with_triggers_disabled(&connection, |connection| match case {
        "message_event_sequence" => {
            assert_eq!(
                connection
                    .execute(
                        "UPDATE chat_messages SET event_sequence=2 \
                         WHERE message_id='message_v9_chat_1'",
                        [],
                    )
                    .unwrap(),
                1
            );
        }
        "message_created_at" => {
            assert_eq!(
                connection
                    .execute(
                        "UPDATE chat_messages SET created_at='2026-09-08T04:00:01Z' \
                         WHERE message_id='message_v9_chat_1'",
                        [],
                    )
                    .unwrap(),
                1
            );
        }
        "frozen_durable_sequence" => {
            assert_eq!(
                connection
                    .execute(
                        "UPDATE frozen_message_sources SET durable_sequence=2 \
                         WHERE message_id='message_v9_chat_1'",
                        [],
                    )
                    .unwrap(),
                1
            );
        }
        "frozen_at" => {
            assert_eq!(
                connection
                    .execute(
                        "UPDATE frozen_message_sources SET frozen_at='invalid' \
                         WHERE message_id='message_v9_chat_1'",
                        [],
                    )
                    .unwrap(),
                1
            );
        }
        _ => unreachable!(),
    });
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}

fn append_after_plan_cutoff(path: &Path) {
    let mut storage = Storage {
        connection: Connection::open(path).unwrap(),
        path: path.to_path_buf(),
        writes_admitted: true,
    };
    assert_eq!(
        storage
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
fn exhausted_pending_verify_prefix_without_prefix_is_resumable_in_v9_and_v10() {
    let (_v9_root, v9_path) = exact_v9_template(1001);
    seed_pending(&v9_path, "verifyPrefix", 1001);
    let mut v9 = Connection::open(&v9_path).unwrap();
    configure_connection(&v9).unwrap();
    let v9_before = database_snapshot(&v9);
    let mut session = ReopenMaintenanceSession::start_for_admission(&mut v9).unwrap();
    loop {
        match session.step(&mut || false).unwrap() {
            ReopenMaintenanceStep::Pending(_) => {}
            ReopenMaintenanceStep::ReadyVerified(_) => break,
            ReopenMaintenanceStep::Cancelled => panic!("V9 verification was not cancelled"),
        }
    }
    drop(session);
    assert_eq!(database_snapshot(&v9), v9_before);
    assert!(v9.is_autocommit());

    let (_v10_root, v10_path) = v9_template(1001);
    seed_pending(&v10_path, "verifyPrefix", 1001);
    let v10 = Connection::open(&v10_path).unwrap();
    let v10_before = database_snapshot(&v10);
    drop(v10);
    let cancellation = AtomicBool::new(false);
    let admitted = Storage::open_for_admission(&v10_path, &cancellation).unwrap();
    assert_eq!(database_snapshot(&admitted.connection), v10_before);
    assert!(admitted.connection.is_autocommit());
}

#[test]
fn pending_verify_prefix_rejects_premature_or_malformed_prefix_without_mutation() {
    let (_premature_root, premature_path) = v9_template(1001);
    seed_pending(&premature_path, "verifyPrefix", 1001);
    let premature = Connection::open(&premature_path).unwrap();
    let (baseline_id, baseline_digest): (String, String) = premature
        .query_row(
            "SELECT baseline_id,baseline_projection_digest \
             FROM storage_compaction_plans WHERE job_id='compact_v9_001'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    with_triggers_disabled(&premature, |connection| {
        insert_prefix(
            connection,
            "compact_v9_001",
            0,
            &baseline_id,
            &baseline_digest,
        );
    });
    premature
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let premature_before = database_snapshot(&premature);
    drop(premature);
    let cancellation = AtomicBool::new(false);
    let error = match Storage::open_for_admission(&premature_path, &cancellation) {
        Ok(_) => panic!("pending verifyPrefix accepted a premature exact prefix"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageSchemaMismatch");
    let premature = Connection::open(&premature_path).unwrap();
    assert_eq!(database_snapshot(&premature), premature_before);

    let (_malformed_root, malformed_path) = v9_template(1001);
    seed_pending(&malformed_path, "authorizeDelete", 0);
    let malformed = Connection::open(&malformed_path).unwrap();
    with_triggers_disabled(&malformed, |connection| {
        connection
            .execute(
                "UPDATE ledger_compaction_prefixes SET anchor_event_id='malformed' \
                 WHERE prefix_id='prefix_compact_v9_001'",
                [],
            )
            .unwrap();
    });
    malformed
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let malformed_before = database_snapshot(&malformed);
    drop(malformed);
    let error = match Storage::open_for_admission(&malformed_path, &cancellation) {
        Ok(_) => panic!("authorizeDelete accepted a malformed prefix"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageSchemaMismatch");
    let malformed = Connection::open(&malformed_path).unwrap();
    assert_eq!(database_snapshot(&malformed), malformed_before);
}

#[test]
fn product_admission_accepts_post_prefix_pending_v9_with_live_tail_and_preserves_witness() {
    let (_root, template) = v9_template(1001);
    for (phase, after) in [("authorizeDelete", 0), ("deletePrefix", 137)] {
        let (_copy_root, path) = copy_template(&template);
        append_after_plan_cutoff(&path);
        seed_pending(&path, phase, after);
        let connection = Connection::open(&path).unwrap();
        let before = [
            witness_snapshot(&connection),
            ["events", "event_source_index", "projects"]
                .map(|table| table_rows(&connection, table))
                .to_vec(),
        ];
        drop(connection);

        let cancellation = AtomicBool::new(false);
        let admitted = Storage::open_for_admission(&path, &cancellation)
            .unwrap_or_else(|error| panic!("{phase}: {error:?}"));
        let after_open = [
            witness_snapshot(&admitted.connection),
            ["events", "event_source_index", "projects"]
                .map(|table| table_rows(&admitted.connection, table))
                .to_vec(),
        ];
        assert_eq!(after_open, before, "{phase}");
        assert!(admitted.connection.is_autocommit(), "{phase}");
    }
}

#[test]
fn product_admission_accepts_valid_planned_chat_transitions_without_mutation() {
    let (_root, template) = chat_v9_template(false);
    for (phase, after) in [("planEligibility", 137), ("authorizeDelete", 0)] {
        let (_copy_root, path) = copy_template(&template);
        seed_chat_pending(&path, phase, after);
        let connection = Connection::open(&path).unwrap();
        let before = database_snapshot(&connection);
        drop(connection);

        let cancellation = AtomicBool::new(false);
        let admitted = Storage::open_for_admission(&path, &cancellation)
            .unwrap_or_else(|error| panic!("{phase}: {error:?}"));
        assert_eq!(database_snapshot(&admitted.connection), before, "{phase}");
        assert!(admitted.connection.is_autocommit(), "{phase}");
    }
}

#[test]
fn product_admission_rejects_coherent_planned_chat_transition_tamper_without_mutation() {
    let (_root, template) = chat_v9_template(false);
    for (case, phase, after, tampered_sequence) in [
        ("early_live", "planEligibility", 137, 2),
        ("post_prefix", "authorizeDelete", 0, 1001),
    ] {
        let (_copy_root, path) = copy_template(&template);
        tamper_chat_transition(&path, tampered_sequence);
        seed_chat_pending(&path, phase, after);
        let connection = Connection::open(&path).unwrap();
        let before = database_snapshot(&connection);
        drop(connection);

        let cancellation = AtomicBool::new(false);
        let error = match Storage::open_for_admission(&path, &cancellation) {
            Ok(_) => panic!("accepted coherent planned chat transition tamper: {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&connection), before, "{case}");
        assert!(connection.is_autocommit(), "{case}");
    }
}

#[test]
fn product_admission_rejects_planned_source_parity_tamper_without_mutation() {
    let (_chat_root, chat_template) = chat_v9_template(false);
    let (_copy_root, chat_path) = copy_template(&chat_template);
    tamper_chat_root_identity(&chat_path);
    seed_chat_pending(&chat_path, "planEligibility", 137);
    let connection = Connection::open(&chat_path).unwrap();
    let before = database_snapshot(&connection);
    drop(connection);
    let cancellation = AtomicBool::new(false);
    let error = match Storage::open_for_admission(&chat_path, &cancellation) {
        Ok(_) => panic!("accepted coherent planned chat root identity tamper"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageSchemaMismatch");
    let connection = Connection::open(&chat_path).unwrap();
    assert_eq!(database_snapshot(&connection), before);
    assert!(connection.is_autocommit());

    let (_project_root, project_template) = v9_template(1001);
    for case in [
        "non_chat_durable",
        "non_chat_snapshot",
        "mismatched_extra_baseline",
    ] {
        let (_copy_root, path) = copy_template(&project_template);
        seed_pending(&path, "planEligibility", 137);
        tamper_project_plan_parity(&path, case);
        let connection = Connection::open(&path).unwrap();
        let before = database_snapshot(&connection);
        drop(connection);

        let error = match Storage::open_for_admission(&path, &cancellation) {
            Ok(_) => panic!("accepted planned source parity tamper: {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&connection), before, "{case}");
        assert!(connection.is_autocommit(), "{case}");
    }
}

#[test]
fn product_admission_validates_planned_team_message_copy_metadata_without_mutation() {
    let (_root, template) = chat_v9_template(true);
    for (phase, after) in [("planEligibility", 137), ("authorizeDelete", 0)] {
        let (_copy_root, path) = copy_template(&template);
        seed_chat_pending(&path, phase, after);
        let connection = Connection::open(&path).unwrap();
        let before = database_snapshot(&connection);
        drop(connection);

        let cancellation = AtomicBool::new(false);
        let admitted = Storage::open_for_admission(&path, &cancellation)
            .unwrap_or_else(|error| panic!("valid TeamMessage {phase}: {error:?}"));
        assert_eq!(database_snapshot(&admitted.connection), before, "{phase}");
    }

    for (case, phase, after) in [
        ("message_event_sequence", "planEligibility", 137),
        ("message_created_at", "planEligibility", 137),
        ("frozen_durable_sequence", "authorizeDelete", 0),
        ("frozen_at", "authorizeDelete", 0),
    ] {
        let (_copy_root, path) = copy_template(&template);
        seed_chat_pending(&path, phase, after);
        tamper_message_copy_metadata(&path, case);
        let connection = Connection::open(&path).unwrap();
        let before = database_snapshot(&connection);
        drop(connection);

        let cancellation = AtomicBool::new(false);
        let error = match Storage::open_for_admission(&path, &cancellation) {
            Ok(_) => panic!("accepted planned TeamMessage metadata tamper: {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&connection), before, "{case}");
        assert!(connection.is_autocommit(), "{case}");
    }
}

#[test]
fn product_admission_rejects_post_prefix_projection_or_source_mismatch_without_mutation() {
    let (_root, template) = v9_template(1001);
    for case in ["projection", "source"] {
        let (_copy_root, path) = copy_template(&template);
        append_after_plan_cutoff(&path);
        seed_pending(&path, "authorizeDelete", 0);
        let connection = Connection::open(&path).unwrap();
        if case == "projection" {
            connection
                .execute(
                    "UPDATE projects SET canonical_path='/hostile/projection' WHERE project_id=?1",
                    [PROJECT_ID],
                )
                .unwrap();
        } else {
            with_triggers_disabled(&connection, |connection| {
                connection
                    .execute(
                        "UPDATE events SET canonical_json_payload='{}' \
                         WHERE aggregate_type='project' AND aggregate_id=?1 \
                           AND aggregate_sequence=1001",
                        [PROJECT_ID],
                    )
                    .unwrap();
            });
        }
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        let before = database_snapshot(&connection);
        drop(connection);

        let cancellation = AtomicBool::new(false);
        let error = match Storage::open_for_admission(&path, &cancellation) {
            Ok(_) => panic!("accepted hostile post-prefix {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&connection), before, "{case}");
        assert!(connection.is_autocommit(), "{case}");
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

fn exact_v9_template(count: usize) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    for sequence in 1..=count {
        storage
            .append_event(&event(
                &format!("evt_v9_migration_{sequence}"),
                &format!("op_v9_migration_{sequence}"),
                "project",
                PROJECT_ID,
                sequence as u64,
                json!({
                    "canonical_path":"/fixture/v9-migration",
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

    let mut connection = Connection::open(&path).unwrap();
    configure_connection(&connection).unwrap();
    migrate_v9(&mut connection).unwrap();
    verify_schema_v9(&connection).unwrap();
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(connection);
    (root, path)
}

#[test]
fn product_admission_migrates_exact_v9_to_v10_and_preserves_terminal_evidence() {
    let (_root, path) = exact_v9_template(1001);
    let mut storage = Storage {
        connection: Connection::open(&path).unwrap(),
        path: path.clone(),
        writes_admitted: true,
    };
    configure_connection(&storage.connection).unwrap();
    let (baseline_id, baseline_digest) = insert_baseline(&storage.connection, 1000);
    freeze_range(&mut storage, 1, 1001);
    with_triggers_disabled(&storage.connection, |connection| {
        insert_job(
            connection,
            "compact_v9_migration_succeeded",
            "succeeded",
            &terminal_state(1001),
            Some(STAMP),
        );
        insert_plan(
            connection,
            "compact_v9_migration_succeeded",
            0,
            1001,
            &baseline_id,
            &baseline_digest,
            None,
        );
        insert_plan_row(
            connection,
            "compact_v9_migration_succeeded",
            1001,
            0,
            &baseline_id,
            &baseline_digest,
        );
        insert_prefix(
            connection,
            "compact_v9_migration_succeeded",
            0,
            &baseline_id,
            &baseline_digest,
        );
        delete_live_prefix(connection, 1000);
        connection
            .execute(
                "INSERT INTO maintenance_jobs(\
                   job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
                   failure_kind,created_at,updated_at) \
                 VALUES ('compact_v9_migration_failed','losslessCompaction','failed','{}',NULL,?1,NULL,\
                         'verificationFailed',?1,?1)",
                [STAMP],
            )
            .unwrap();
    });
    storage.checkpoint().unwrap();
    let before = database_snapshot(&storage.connection);
    drop(storage);

    let cancellation = AtomicBool::new(false);
    let mut admitted = Storage::open_for_admission(&path, &cancellation).unwrap();
    assert_eq!(
        storage_schema_version(&admitted.connection).unwrap(),
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V10)
    );
    let identity = schema_identity_v10_golden().unwrap();
    assert!(schema_identity_v10_matches(&admitted.connection, &identity).unwrap());
    for (table, rows) in before.1 {
        if table == "storage_schema_meta" {
            continue;
        }
        assert_eq!(
            table_rows(&admitted.connection, &table),
            rows,
            "preserved {} rows",
            table
        );
    }
    assert_eq!(
        admitted
            .append_event(&event(
                "evt_v10_append_1002",
                "op_v10_append_1002",
                "project",
                PROJECT_ID,
                1002,
                json!({
                    "canonical_path":"/fixture/v9-migration",
                    "project_id":PROJECT_ID,
                    "sequence":1002,
                }),
                STAMP,
            ))
            .unwrap(),
        AppendOutcome::Committed
    );
}

#[test]
fn v9_to_v10_preflight_rejects_drift_active_jobs_and_authority_without_mutation() {
    for case in ["drift", "pending", "running", "authority"] {
        let (_root, path) = exact_v9_template(0);
        let connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        match case {
            "drift" => connection
                .execute_batch("CREATE TABLE unexpected_v9_drift(id INTEGER PRIMARY KEY) STRICT;")
                .unwrap(),
            "pending" | "running" => insert_job(
                &connection,
                "compact_v9_active",
                case,
                &private_state("planEligibility", 0, false),
                None,
            ),
            "authority" => with_triggers_disabled(&connection, |connection| {
                connection
                    .execute(
                        "INSERT INTO ledger_compaction_delete_authority(\
                           job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,\
                           aggregate_id,aggregate_sequence,authorized_at) \
                         VALUES ('missing_job',1,'missing_prefix',1,'event','operation','project',\
                                 'project',1,?1)",
                        [STAMP],
                    )
                    .unwrap();
            }),
            _ => unreachable!(),
        }
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        let before = database_snapshot(&connection);
        drop(connection);

        let cancellation = AtomicBool::new(false);
        let error = match Storage::open_for_admission(&path, &cancellation) {
            Ok(_) => panic!("accepted unsafe V9 migration preflight case: {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "migrationFailed", "{case}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&connection), before, "{case}");
        assert_eq!(
            storage_schema_version(&connection).unwrap(),
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V9),
            "{case}"
        );
        assert!(connection.is_autocommit(), "{case}");
    }
}

#[test]
fn v10_exact_schema_and_future_version_fail_closed_without_mutation() {
    for case in ["trigger_drift", "future_version"] {
        let (_root, path) = v9_template(0);
        let connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        match case {
            "trigger_drift" => connection
                .execute_batch(
                    "DROP TRIGGER events_require_v10_complete_delete_batch; \
                     CREATE TRIGGER events_require_v10_complete_delete_batch \
                     BEFORE DELETE ON events BEGIN SELECT 1; END;",
                )
                .unwrap(),
            "future_version" => {
                connection
                    .execute_batch("PRAGMA ignore_check_constraints=ON;")
                    .unwrap();
                connection
                    .execute(
                        "UPDATE storage_schema_meta SET storage_schema_version=11 WHERE singleton=1",
                        [],
                    )
                    .unwrap();
                connection
                    .execute_batch("PRAGMA ignore_check_constraints=OFF;")
                    .unwrap();
            }
            _ => unreachable!(),
        }
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        let before = database_snapshot(&connection);
        drop(connection);

        let cancellation = AtomicBool::new(false);
        let error = match Storage::open_for_admission(&path, &cancellation) {
            Ok(_) => panic!("accepted unsupported V10 case: {case}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(database_snapshot(&connection), before, "{case}");
        assert!(connection.is_autocommit(), "{case}");
    }
}

#[test]
fn v10_migration_ddl_failure_and_cancellation_leave_exact_v9_unchanged() {
    let (_root, path) = exact_v9_template(0);
    let mut connection = Connection::open(&path).unwrap();
    configure_connection(&connection).unwrap();
    let before = database_snapshot(&connection);
    let broken = MIGRATION_V10.replacen(
        "COMMIT;",
        "INSERT INTO missing_v10_failure_target(value) VALUES (1);\nCOMMIT;",
        1,
    );
    assert!(execute_migration(&mut connection, &broken, "broken v10").is_err());
    assert_eq!(database_snapshot(&connection), before);
    assert!(connection.is_autocommit());
    drop(connection);

    let cancellation = AtomicBool::new(true);
    let error = match Storage::open_for_admission(&path, &cancellation) {
        Ok(_) => panic!("cancelled V9 admission unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageAdmissionCancelled");
    let connection = Connection::open(&path).unwrap();
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(
        storage_schema_version(&connection).unwrap(),
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V9)
    );
}

#[test]
fn legacy_open_rejects_exact_v10_without_mutation() {
    let (_root, path) = v9_template(0);
    let connection = Connection::open(&path).unwrap();
    let before = database_snapshot(&connection);
    drop(connection);
    let error = match Storage::open(&path) {
        Ok(_) => panic!("legacy open accepted V10"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageSchemaMismatch");
    let connection = Connection::open(&path).unwrap();
    assert_eq!(database_snapshot(&connection), before);
}

#[test]
fn v10_schema_identity_rejects_a_seventy_seventh_object_before_fetch_without_mutation() {
    let (_root, path) = v9_template(0);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("CREATE TABLE unexpected_v10_object(id INTEGER PRIMARY KEY) STRICT;")
        .unwrap();
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(connection);
    let before = fs::read(&path).unwrap();

    let cancellation = AtomicBool::new(false);
    let error = match Storage::open_for_admission(&path, &cancellation) {
        Ok(_) => panic!("accepted a seventy-seventh V10 schema object"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageSchemaMismatch");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn v10_schema_identity_rejects_giant_sql_before_string_fetch_without_mutation() {
    let (_root, path) = v9_template(0);
    let connection = Connection::open(&path).unwrap();
    let giant = "x".repeat(STORAGE_SCHEMA_IDENTITY_MAX_BYTES + 1);
    connection
        .execute_batch(&format!(
            "DROP TRIGGER events_require_v10_complete_delete_batch; \
             CREATE TRIGGER events_require_v10_complete_delete_batch \
             BEFORE DELETE ON events BEGIN SELECT length('{giant}'); END;"
        ))
        .unwrap();
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(connection);
    let before = fs::read(&path).unwrap();

    let cancellation = AtomicBool::new(false);
    let error = match Storage::open_for_admission(&path, &cancellation) {
        Ok(_) => panic!("accepted an over-budget V10 schema definition"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageSchemaMismatch");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn v10_schema_identity_golden_rejects_altered_truncated_duplicate_and_reordered_inventory() {
    assert_eq!(
        schema_identity_v10_golden_from_str(SCHEMA_IDENTITY_V10)
            .unwrap()
            .len(),
        STORAGE_SCHEMA_IDENTITY_OBJECT_COUNT_V10
    );
    let original: Value = serde_json::from_str(SCHEMA_IDENTITY_V10).unwrap();
    for case in ["altered", "truncated", "duplicate", "reordered"] {
        let mut invalid = original.clone();
        let objects = invalid["objects"].as_array_mut().unwrap();
        match case {
            "altered" => {
                objects[0]["canonicalSqlSha256"] = Value::String("A".repeat(64));
            }
            "truncated" => {
                objects.pop();
            }
            "duplicate" => {
                objects[1] = objects[0].clone();
            }
            "reordered" => {
                objects.swap(0, 1);
            }
            _ => unreachable!(),
        }
        let source = serde_json::to_string(&invalid).unwrap();
        let error = schema_identity_v10_golden_from_str(&source).unwrap_err();
        assert_eq!(error.kind(), "storageSchemaMismatch", "{case}");
    }
}
