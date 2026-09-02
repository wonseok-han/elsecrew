//! R2 temp-database evidence. Direct phase probes test local semantics independently
//! of the full session's intentionally earlier reverse-association rejection.
use super::*;
use crate::storage::reopen::{phase_batch_for_test, phase_queries_for_test, CompactionPhase};

const STAMP: &str = "2026-09-02T01:00:00Z";
const MIB: usize = 1024 * 1024;

fn baseline_id(source: &LedgerEventV1) -> String {
    let identity = format!(
        "{}\0{}\0{}",
        source.aggregate_type, source.aggregate_id, source.aggregate_sequence
    );
    format!(
        "baseline_{}",
        sha256_file_version_v1(identity.as_bytes()).trim_start_matches("sha256:")
    )
}

fn baseline(connection: &Connection, source: &LedgerEventV1) {
    connection.execute(
        "INSERT INTO ledger_baselines(baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at) VALUES (?1,?2,?3,?4,1,?5,?6,?7,?7)",
        params![baseline_id(source),source.aggregate_type,source.aggregate_id,sql_i64(source.aggregate_sequence).unwrap(),source.canonical_json_payload,sha256_file_version_v1(source.canonical_json_payload.as_bytes()),STAMP],
    ).unwrap();
}

/// Every deleted sequence has its own exact prefix, with historical frozen anchors
/// and one retained live anchor. All fixture mutations use the production guards.
pub(super) fn seed(
    storage: &mut Storage,
    deleted: usize,
    chat: bool,
    null_repeat: bool,
    payload_bytes: usize,
    request_bytes: usize,
) -> Vec<LedgerEventV1> {
    seed_with_anchor_intro(
        storage,
        deleted,
        chat,
        null_repeat,
        payload_bytes,
        request_bytes,
        false,
    )
}

fn seed_with_anchor_intro(
    storage: &mut Storage,
    deleted: usize,
    chat: bool,
    null_repeat: bool,
    payload_bytes: usize,
    request_bytes: usize,
    anchor_intro: bool,
) -> Vec<LedgerEventV1> {
    if chat {
        storage
            .append_event(&event(
                "evt_r2_project",
                "op_r2_project",
                "project",
                "project_r2",
                1,
                json!({"canonical_path":"/fixture/r2", "project_id":"project_r2"}),
                STAMP,
            ))
            .unwrap();
    }
    let mut sources = Vec::new();
    let mut durable = 0;
    let mut snapshot = Value::Null;
    for sequence in 1..=deleted + 1 {
        let payload = if chat {
            if (sequence <= deleted && (!null_repeat || sequence == 1 || sequence == 4))
                || (sequence > deleted && anchor_intro)
            {
                durable += 1;
                snapshot = team_message("chat_r2", &format!("message_r2_{durable}"), durable);
            } else if null_repeat && sequence == 3 {
                snapshot = Value::Null;
            }
            materializable_chat_payload("chat_r2", "project_r2", durable, snapshot.clone())
        } else {
            let mut payload =
                json!({"canonical_path":"/fixture/r2", "project_id":"project_r2", "padding":""});
            let size = canonical_json_v1(payload.clone()).unwrap().len();
            if payload_bytes > 0 {
                payload["padding"] = json!("x".repeat(payload_bytes - size));
            }
            payload
        };
        let mut source = event(
            &format!("evt_r2_{sequence}"),
            &format!("op_r2_{sequence}"),
            if chat { "chat" } else { "project" },
            if chat { "chat_r2" } else { "project_r2" },
            sequence as u64,
            payload,
            STAMP,
        );
        source.request_id = (request_bytes > 0).then(|| "r".repeat(request_bytes));
        assert_eq!(
            storage.append_event(&source).unwrap(),
            AppendOutcome::Committed
        );
        sources.push(source);
    }
    run_bounded_materializer_to_ready(storage);
    for source in &sources {
        insert_exact_v8_frozen_operation(storage, source, STAMP);
    }
    storage.connection.execute(
        "INSERT INTO frozen_message_sources(message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at)
         SELECT m.message_id,i.event_rowid,i.event_id,i.operation_id,m.chat_id,i.aggregate_sequence,m.event_sequence,m.canonical_message_json,m.message_digest,?1
         FROM event_source_index i JOIN chat_messages m ON m.message_id=i.introduced_message_id WHERE i.aggregate_type='chat' AND i.aggregate_id='chat_r2' AND i.aggregate_sequence<=?2 ORDER BY i.event_rowid", params![STAMP,deleted as i64]).unwrap();
    for (index, source) in sources.iter().take(deleted).enumerate() {
        baseline(&storage.connection, source);
        storage.connection.execute(
            "INSERT INTO ledger_compaction_prefixes(prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,baseline_projection_digest,created_at,verified_at)
             SELECT ?1,aggregate_type,aggregate_id,?2,event_rowid,event_id,operation_id,aggregate_sequence,?3,?4,?5,?5 FROM event_source_index WHERE operation_id=?6",
            params![format!("prefix_r2_{}",index+1),sql_i64(source.aggregate_sequence).unwrap(),baseline_id(source),sha256_file_version_v1(source.canonical_json_payload.as_bytes()),STAMP,sources[index+1].operation_id]).unwrap();
    }
    let cutoff: i64 = storage
        .connection
        .query_row("SELECT max(rowid) FROM events", [], |row| row.get(0))
        .unwrap();
    let proof = json!({"storageCompactionV2PrivateState":{"schemaVersion":1,"cursor":{"schemaVersion":2,"cutoffEventRowid":cutoff,"phase":"finalize","afterEventRowid":cutoff,"scanExhausted":true},"operationsVerified":true,"equivalenceVerified":true}});
    storage.connection.execute("INSERT INTO maintenance_jobs(job_id,job_kind,state,cutoff_json,started_at,created_at,updated_at) VALUES ('compact_r2','losslessCompaction','running',?1,?2,?2,?2)",params![proof.to_string(),STAMP]).unwrap();
    for source in sources.iter().take(deleted) {
        storage.connection.execute("INSERT INTO ledger_compaction_delete_authority(job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,authorized_at) SELECT 'compact_r2',1,?1,event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,?2 FROM event_source_index WHERE operation_id=?3",params![format!("prefix_r2_{}",source.aggregate_sequence),STAMP,source.operation_id]).unwrap();
        storage
            .connection
            .execute(
                "DELETE FROM event_source_index WHERE operation_id=?1",
                [&source.operation_id],
            )
            .unwrap();
        storage
            .connection
            .execute(
                "DELETE FROM events WHERE operation_id=?1",
                [&source.operation_id],
            )
            .unwrap();
    }
    storage.connection.execute("UPDATE maintenance_jobs SET state='succeeded',completed_at=?1,last_successful_at=?1,updated_at=?1 WHERE job_id='compact_r2'",[STAMP]).unwrap();
    sources
}

fn cutoff(connection: &Connection) -> u64 {
    connection
        .query_row(
            "SELECT max(source_event_rowid) FROM frozen_operation_sources",
            [],
            |row| row_u64(row, 0),
        )
        .unwrap()
}

pub(super) fn fixture(
    deleted: usize,
    chat: bool,
    null_repeat: bool,
    payload: usize,
    request: usize,
) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed(&mut storage, deleted, chat, null_repeat, payload, request);
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}

/// Deliberately corrupt ONLY a disposable fixture, then restore its exact schema.
pub(super) fn corrupt(connection: &Connection, sql: &str) {
    let triggers: Vec<(String, String)> = connection
        .prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger'")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF; PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    for (name, _) in &triggers {
        connection
            .execute_batch(&format!("DROP TRIGGER {name}"))
            .unwrap();
    }
    connection.execute_batch(sql).unwrap();
    for (_, definition) in triggers {
        connection.execute_batch(&definition).unwrap();
    }
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA ignore_check_constraints=OFF;")
        .unwrap();
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Keep the closed deleted tuple internally consistent while varying primary size.
/// This is test-only fixture construction, not a product mutation/repair path.
pub(super) fn replace_deleted_payload(connection: &Connection, sequence: usize, payload: Value) {
    let operation = format!("op_r2_{sequence}");
    let mut source: FrozenOperationSourceV1 = serde_json::from_value(
        load_frozen_operation_value(connection, &operation)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    source.canonical_payload_json = canonical_json_v1(payload.clone()).unwrap();
    source.canonical_payload_digest =
        sha256_file_version_v1(source.canonical_payload_json.as_bytes())
            .trim_start_matches("sha256:")
            .into();
    source.canonical_record_json = canonical_frozen_operation_record_v1(&source).unwrap();
    source.record_digest = sha256_file_version_v1(source.canonical_record_json.as_bytes())
        .trim_start_matches("sha256:")
        .into();
    let json = literal(&source.canonical_payload_json);
    let digest = literal(&source.canonical_payload_digest);
    let projection_digest = literal(&format!("sha256:{}", source.canonical_payload_digest));
    let mut sql=format!("UPDATE frozen_operation_sources SET canonical_payload_json={json},canonical_payload_digest={digest},canonical_record_json={},record_digest={} WHERE operation_id='op_r2_{sequence}'; UPDATE operation_receipts SET canonical_payload_json={json},canonical_payload_digest={digest} WHERE operation_id='op_r2_{sequence}'; UPDATE ledger_baselines SET canonical_projection_json={json},projection_digest={projection_digest} WHERE aggregate_type='{}' AND cutoff_aggregate_sequence={sequence}; UPDATE ledger_compaction_prefixes SET baseline_projection_digest={projection_digest} WHERE deleted_through_sequence={sequence};",literal(&source.canonical_record_json),literal(&source.record_digest),source.aggregate_type);
    if source.aggregate_type == "chat" {
        let message = canonical_json_v1(payload["last_message"].clone()).unwrap();
        let digest = sha256_file_version_v1(message.as_bytes());
        sql.push_str(&format!("UPDATE frozen_message_sources SET canonical_message_json={},message_digest={} WHERE source_aggregate_sequence={sequence}; UPDATE chat_messages SET canonical_message_json={},message_digest={} WHERE event_sequence={sequence};",literal(&message),literal(&digest),literal(&message),literal(&digest)));
    }
    corrupt(connection, &sql);
}

fn sized_payload(chat: bool, sequence: usize, size: usize) -> Value {
    if chat {
        let mut message = team_message(
            "chat_r2",
            &format!("message_r2_{sequence}"),
            sequence as u64,
        );
        message["text"] = json!("");
        let empty = canonical_json_v1(message.clone()).unwrap().len();
        message["text"] = json!("x".repeat(size - empty));
        assert_eq!(canonical_json_v1(message.clone()).unwrap().len(), size);
        materializable_chat_payload("chat_r2", "project_r2", sequence as u64, message)
    } else {
        let mut payload =
            json!({"canonical_path":"/fixture/r2","project_id":"project_r2","padding":""});
        let empty = canonical_json_v1(payload.clone()).unwrap().len();
        payload["padding"] = json!("x".repeat(size - empty));
        assert_eq!(canonical_json_v1(payload.clone()).unwrap().len(), size);
        payload
    }
}

#[test]
fn compaction_closure_each_primary_exact_one_mib_and_oversize() {
    for phase in [
        CompactionPhase::Operations,
        CompactionPhase::Prefixes,
        CompactionPhase::FrozenMessages,
    ] {
        for size in [MIB, MIB + 1] {
            // TeamMessage text has a separate64KiB semantic cap. Use sixteen
            // valid messages for an exact1MiB message-phase primary aggregate.
            let count = if phase == CompactionPhase::FrozenMessages && size == MIB {
                16
            } else {
                1
            };
            let (_root, path) =
                fixture(count, phase == CompactionPhase::FrozenMessages, false, 0, 0);
            let mut connection = Connection::open(&path).unwrap();
            for sequence in 1..=count {
                replace_deleted_payload(
                    &connection,
                    sequence,
                    sized_payload(
                        phase == CompactionPhase::FrozenMessages,
                        sequence,
                        size / count,
                    ),
                );
            }
            let end = cutoff(&connection);
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let result = phase_batch_for_test(&mut connection, phase, 0, end, &mut || false);
            if size == MIB {
                let batch = result
                    .unwrap_or_else(|e| panic!("{phase:?} size={size}: {e:?}"))
                    .unwrap();
                assert_eq!(batch.1.rows, count);
                assert_eq!(batch.1.canonical_source_bytes, MIB);
                assert!(batch.1.encoded_closure_bytes <= 8 * MIB);
                if phase != CompactionPhase::FrozenMessages {
                    verify_all(&mut connection);
                }
            } else {
                assert_eq!(result.unwrap_err().kind, "storageSchemaMismatch");
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}

#[test]
fn compaction_closure_primary_budget_yields_without_consuming_next_row() {
    for phase in [
        CompactionPhase::Operations,
        CompactionPhase::Prefixes,
        CompactionPhase::FrozenMessages,
    ] {
        let messages = phase == CompactionPhase::FrozenMessages;
        let count = if messages { 17 } else { 2 };
        let size = if messages { 65536 } else { 600_000 };
        let (_root, path) = fixture(count, messages, false, 0, 0);
        let mut connection = Connection::open(&path).unwrap();
        for sequence in 1..=count {
            replace_deleted_payload(
                &connection,
                sequence,
                sized_payload(messages, sequence, size),
            );
        }
        let end = cutoff(&connection);
        let before = database_snapshot(&connection);
        let bytes = fs::read(&path).unwrap();
        let first = phase_batch_for_test(&mut connection, phase, 0, end, &mut || false)
            .unwrap_or_else(|e| panic!("first {phase:?}: {e:?}"))
            .unwrap();
        assert_eq!(first.1.rows, if messages { 16 } else { 1 });
        assert_eq!(
            first.1.canonical_source_bytes,
            if messages { MIB } else { size }
        );
        assert!(!first.2);
        let second = phase_batch_for_test(&mut connection, phase, first.0, end, &mut || false)
            .unwrap_or_else(|e| panic!("second {phase:?}: {e:?}"))
            .unwrap();
        assert!(second.1.rows >= 1);
        assert!(second.1.canonical_source_bytes >= size);
        assert!(second.1.canonical_source_bytes <= MIB);
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn compaction_closure_exact_secondary_cap_oversize_and_remaining_budget_yield() {
    for phase in [
        CompactionPhase::Operations,
        CompactionPhase::Prefixes,
        CompactionPhase::FrozenMessages,
    ] {
        for over in [false, true] {
            let (_root, path) = fixture(1, phase == CompactionPhase::FrozenMessages, false, 0, 0);
            let mut connection = Connection::open(&path).unwrap();
            let end = cutoff(&connection);
            // Derive the exact first-row input cost using an isolated keyset window.
            let mut first_bytes = 0;
            if phase == CompactionPhase::Operations {
                let all = phase_batch_for_test(&mut connection, phase, 0, end, &mut || false)
                    .unwrap()
                    .unwrap();
                let tail = phase_batch_for_test(&mut connection, phase, 1, end, &mut || false)
                    .unwrap()
                    .unwrap();
                first_bytes = all.1.encoded_closure_bytes - tail.1.encoded_closure_bytes;
            }
            let cost = if phase == CompactionPhase::Operations {
                first_bytes
            } else {
                phase_batch_for_test(&mut connection, phase, 0, end, &mut || false)
                    .unwrap()
                    .unwrap()
                    .1
                    .encoded_closure_bytes
            };
            let (table, column, predicate) = if phase == CompactionPhase::FrozenMessages {
                ("chat_messages", "materialized_at", "event_sequence=1")
            } else {
                (
                    "ledger_baselines",
                    "created_at",
                    "aggregate_type='project' AND cutoff_aggregate_sequence=1",
                )
            };
            let previous: i64 = connection
                .query_row(
                    &format!(
                        "SELECT length(CAST({column} AS BLOB)) FROM {table} WHERE {predicate}"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let padding = "x".repeat(8 * MIB - cost + previous as usize + usize::from(over));
            corrupt(
                &connection,
                &format!(
                    "UPDATE {table} SET {column}={} WHERE {predicate}",
                    literal(&padding)
                ),
            );
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let result = phase_batch_for_test(&mut connection, phase, 0, end, &mut || false);
            if over {
                assert_eq!(result.unwrap_err().kind, "storageSchemaMismatch");
            } else {
                let first = result.unwrap().unwrap();
                assert_eq!(first.1.rows, 1);
                assert_eq!(first.1.encoded_closure_bytes, 8 * MIB);
                if phase == CompactionPhase::Operations {
                    assert!(!first.2);
                    let next =
                        phase_batch_for_test(&mut connection, phase, first.0, end, &mut || false)
                            .unwrap()
                            .unwrap();
                    assert_eq!(next.1.rows, 1);
                    assert!(next.2);
                }
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        // A normal row followed by an intrinsically oversized closure must fail
        // the call, not keep yielding the impossible row forever.
        let (_root, path) = fixture(2, phase == CompactionPhase::FrozenMessages, false, 0, 0);
        let mut connection = Connection::open(&path).unwrap();
        let end = cutoff(&connection);
        // Three Mi characters are below the character cap but nine Mi UTF-8
        // bytes exceed it: a regression to LENGTH(TEXT) must reject this row.
        let multibyte = "界".repeat(3 * MIB);
        assert!(multibyte.chars().count() < 8 * MIB);
        assert!(multibyte.len() > 8 * MIB);
        let sql = if phase == CompactionPhase::FrozenMessages {
            format!(
                "UPDATE chat_messages SET materialized_at={} WHERE event_sequence=2",
                literal(&multibyte)
            )
        } else {
            format!("UPDATE ledger_baselines SET created_at={} WHERE aggregate_type='project' AND cutoff_aggregate_sequence=2",literal(&multibyte))
        };
        corrupt(&connection, &sql);
        assert_eq!(
            phase_batch_for_test(&mut connection, phase, 0, end, &mut || false)
                .unwrap_err()
                .kind,
            "storageSchemaMismatch"
        );
        rejects_without_mutation(&mut connection, &path);

        let (_root, path) = fixture(2, phase == CompactionPhase::FrozenMessages, false, 0, 0);
        let mut connection = Connection::open(&path).unwrap();
        let end = cutoff(&connection);
        let sql = if phase == CompactionPhase::FrozenMessages {
            format!(
                "UPDATE chat_messages SET materialized_at={} WHERE event_sequence<=2",
                literal(&"x".repeat(4 * MIB))
            )
        } else {
            format!("UPDATE ledger_baselines SET created_at={} WHERE aggregate_type='project' AND cutoff_aggregate_sequence<=2",literal(&"x".repeat(4*MIB)))
        };
        corrupt(&connection, &sql);
        let before = database_snapshot(&connection);
        let bytes = fs::read(&path).unwrap();
        let first = phase_batch_for_test(&mut connection, phase, 0, end, &mut || false)
            .unwrap()
            .unwrap();
        assert_eq!(first.1.rows, 1);
        assert!(!first.2);
        assert!(first.1.encoded_closure_bytes > 4 * MIB);
        let second = phase_batch_for_test(&mut connection, phase, first.0, end, &mut || false)
            .unwrap()
            .unwrap();
        assert!(second.1.rows >= 1);
        assert!(second.1.encoded_closure_bytes > 4 * MIB);
        assert!(second.1.encoded_closure_bytes <= 8 * MIB);
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn compaction_closure_rejects_hidden_rowids_and_empty_prefixes() {
    for table in ["ledger_compaction_prefixes", "frozen_message_sources"] {
        for rowid in [0, -1] {
            let (_root, path) = fixture(1, true, false, 0, 0);
            let mut connection = Connection::open(&path).unwrap();
            corrupt(
                &connection,
                &format!("UPDATE {table} SET rowid={rowid} WHERE rowid=1"),
            );
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            match FrozenOperationVerificationSession::start_compaction_closure(&mut connection) {
                Err(error) => assert_eq!(error.kind, "storageSchemaMismatch"),
                Ok(_) => panic!("hidden row admitted"),
            }
            assert!(connection.is_autocommit());
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    let (_root, path) = fixture(1, false, false, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    corrupt(&connection, "DELETE FROM ledger_compaction_prefixes");
    let end = cutoff(&connection);
    assert_eq!(
        phase_batch_for_test(
            &mut connection,
            CompactionPhase::Prefixes,
            0,
            end,
            &mut || false
        )
        .unwrap_err()
        .kind,
        "storageSchemaMismatch"
    );
    rejects_without_mutation(&mut connection, &path);
}

#[test]
fn compaction_closure_rejects_extra_nonintroduction_message_first_and_late() {
    for late in [false, true] {
        let (_root, path) = fixture(if late { 102 } else { 4 }, true, !late, 0, 0);
        let mut connection = Connection::open(&path).unwrap();
        let end = cutoff(&connection);
        if late {
            let message = team_message("chat_r2", "message_r2_101", 101);
            replace_deleted_payload(
                &connection,
                102,
                materializable_chat_payload("chat_r2", "project_r2", 101, message),
            );
        } else {
            corrupt(&connection,"UPDATE frozen_message_sources SET source_event_rowid=3,source_event_id='evt_r2_2',source_operation_id='op_r2_2',source_aggregate_sequence=2 WHERE source_aggregate_sequence=1");
        }
        let mut after = 0;
        if late {
            let first = phase_batch_for_test(
                &mut connection,
                CompactionPhase::FrozenMessages,
                0,
                end,
                &mut || false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(first.1.rows, 100);
            after = first.0;
        }
        assert_eq!(
            phase_batch_for_test(
                &mut connection,
                CompactionPhase::FrozenMessages,
                after,
                end,
                &mut || false
            )
            .unwrap_err()
            .kind,
            "storageSchemaMismatch"
        );
        rejects_without_mutation(&mut connection, &path);
    }
}

#[test]
fn compaction_closure_first_latest_anchor_corruption_rejects() {
    for sql in [
        "UPDATE events SET canonical_json_payload='{}' WHERE aggregate_sequence=2",
        "UPDATE operation_receipts SET canonical_payload_json='{}' WHERE aggregate_sequence=2",
        "UPDATE event_source_index SET chat_durable_sequence=0 WHERE aggregate_sequence=2",
    ] {
        let (_root, path) = fixture(1, true, false, 0, 0);
        let mut connection = Connection::open(&path).unwrap();
        let end = cutoff(&connection);
        corrupt(&connection, sql);
        assert_eq!(
            phase_batch_for_test(
                &mut connection,
                CompactionPhase::Prefixes,
                0,
                end,
                &mut || false
            )
            .unwrap_err()
            .kind,
            "storageSchemaMismatch"
        );
        rejects_without_mutation(&mut connection, &path);
    }
}

fn rejects_without_mutation(connection: &mut Connection, path: &Path) {
    let before = database_snapshot(connection);
    let bytes = fs::read(path).unwrap();
    {
        let mut session =
            FrozenOperationVerificationSession::start_compaction_closure(connection).unwrap();
        let mut failed = false;
        for _ in 0..20 {
            match session.step(&mut || false) {
                Err(error) => {
                    assert_eq!(error.kind, "storageSchemaMismatch");
                    failed = true;
                    break;
                }
                Ok(FrozenOperationVerificationStep::CompactionClosurePending { .. }) => {}
                other => panic!("unexpected corruption outcome {other:?}"),
            }
        }
        assert!(failed);
        assert!(session.step(&mut || false).is_err());
    }
    assert!(connection.is_autocommit());
    assert_eq!(database_snapshot(connection), before);
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn compaction_closure_prefix_first_and_late_corruption_matrix() {
    let (_template, template) = fixture(103, true, false, 0, 0);
    for late in [false, true] {
        let target = if late { 102 } else { 1 };
        for kind in [
            "baseline_payload",
            "baseline_digest",
            "baseline_identity",
            "baseline_schema",
            "deleted_source",
            "missing_source",
            "historical_anchor",
            "prefix_chain",
            "latest_anchor",
            "latest_receipt",
            "latest_index",
        ] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let end = cutoff(&connection);
            let source = if kind.starts_with("latest") {
                104
            } else {
                target
            };
            let sql=match kind {
                "baseline_payload"=>format!("UPDATE ledger_baselines SET canonical_projection_json='{{}}' WHERE cutoff_aggregate_sequence={target} AND aggregate_type='chat'"),
                "baseline_digest"=>format!("UPDATE ledger_baselines SET projection_digest='sha256:{}' WHERE cutoff_aggregate_sequence={target} AND aggregate_type='chat'","0".repeat(64)),
                "baseline_identity"=>format!("UPDATE ledger_baselines SET baseline_id='baseline_bad' WHERE cutoff_aggregate_sequence={target} AND aggregate_type='chat'; UPDATE ledger_compaction_prefixes SET baseline_id='baseline_bad' WHERE deleted_through_sequence={target}"),
                "baseline_schema"=>format!("UPDATE ledger_baselines SET payload_schema_version=2 WHERE cutoff_aggregate_sequence={target} AND aggregate_type='chat'"),
                "deleted_source"=>format!("UPDATE frozen_operation_sources SET record_digest='{}' WHERE aggregate_sequence={source}","0".repeat(64)),
                "missing_source"=>format!("DELETE FROM frozen_operation_sources WHERE aggregate_sequence={source}"),
                "historical_anchor"=>format!("UPDATE ledger_compaction_prefixes SET anchor_event_id='evt_wrong' WHERE deleted_through_sequence={target}"),
                "prefix_chain" if late => "DELETE FROM ledger_compaction_prefixes WHERE deleted_through_sequence=101".into(),
                "prefix_chain"=>"DELETE FROM ledger_compaction_prefixes WHERE deleted_through_sequence=1".into(),
                "latest_anchor"=>"UPDATE events SET canonical_json_payload='{}' WHERE aggregate_sequence=104".into(),
                "latest_receipt"=>"UPDATE operation_receipts SET canonical_payload_json='{}' WHERE aggregate_sequence=104".into(),
                "latest_index"=>"UPDATE event_source_index SET chat_durable_sequence=0 WHERE aggregate_sequence=104".into(),
                _=>unreachable!(),
            };
            corrupt(&connection, &sql);
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let mut after = 0;
            // Latest anchor lives at the end in both variants. Missing prefix101
            // likewise fails only after the first100 intact rows.
            if late || kind.starts_with("latest") {
                let batch = phase_batch_for_test(
                    &mut connection,
                    CompactionPhase::Prefixes,
                    0,
                    end,
                    &mut || false,
                )
                .unwrap()
                .unwrap();
                assert_eq!(batch.1.rows, 100, "{kind}");
                assert!(!batch.2);
                after = batch.0;
            }
            assert_eq!(
                phase_batch_for_test(
                    &mut connection,
                    CompactionPhase::Prefixes,
                    after,
                    end,
                    &mut || false
                )
                .unwrap_err()
                .kind,
                "storageSchemaMismatch",
                "{kind} late={late}"
            );
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            rejects_without_mutation(&mut connection, &path);
        }
    }
}

#[test]
fn compaction_closure_message_first_and_late_corruption_matrix() {
    let (_template, template) = fixture(102, true, false, 0, 0);
    for target in [1, 102] {
        for kind in [
            "identity",
            "digest",
            "message_payload",
            "normalized_payload",
            "normalized_digest",
            "normalized_identity",
            "created_at",
            "missing_message",
            "missing_normalized",
            "missing_prefix",
            "missing_predecessor",
        ] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let end = cutoff(&connection);
            let sql=match kind {
                "identity"=>format!("UPDATE frozen_message_sources SET source_event_id='evt_wrong' WHERE source_aggregate_sequence={target}"),
                "digest"=>format!("UPDATE frozen_message_sources SET message_digest='sha256:{}' WHERE source_aggregate_sequence={target}","0".repeat(64)),
                "message_payload"=>format!("UPDATE frozen_message_sources SET canonical_message_json='{{}}' WHERE source_aggregate_sequence={target}"),
                "normalized_payload"=>format!("UPDATE chat_messages SET canonical_message_json='{{}}' WHERE event_sequence={target}"),
                "normalized_digest"=>format!("UPDATE chat_messages SET message_digest='sha256:{}' WHERE event_sequence={target}","0".repeat(64)),
                "normalized_identity"=>format!("UPDATE chat_messages SET chat_id='chat_wrong' WHERE event_sequence={target}"),
                "created_at"=>format!("UPDATE chat_messages SET created_at='2026-09-01T01:00:00Z' WHERE event_sequence={target}"),
                "missing_message"=>format!("DELETE FROM frozen_message_sources WHERE source_aggregate_sequence={target}"),
                "missing_normalized"=>format!("DELETE FROM chat_messages WHERE event_sequence={target}"),
                "missing_prefix"=>format!("DELETE FROM ledger_compaction_prefixes WHERE deleted_through_sequence={target}"),
                "missing_predecessor"=>format!("DELETE FROM frozen_operation_sources WHERE aggregate_sequence={}",if target==1 {1}else{target-1}),
                _=>unreachable!(),
            };
            corrupt(&connection, &sql);
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            if kind != "missing_message" {
                let mut after = 0;
                if target > 100 {
                    let batch = phase_batch_for_test(
                        &mut connection,
                        CompactionPhase::FrozenMessages,
                        0,
                        end,
                        &mut || false,
                    )
                    .unwrap_or_else(|e| panic!("{kind} target={target}: {e:?}"))
                    .unwrap();
                    assert_eq!(batch.1.rows, 100, "{kind}");
                    after = batch.0;
                }
                assert_eq!(
                    phase_batch_for_test(
                        &mut connection,
                        CompactionPhase::FrozenMessages,
                        after,
                        end,
                        &mut || false
                    )
                    .unwrap_err()
                    .kind,
                    "storageSchemaMismatch",
                    "{kind} target={target}"
                );
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            rejects_without_mutation(&mut connection, &path);
        }
    }
}

fn verify_all(connection: &mut Connection) -> Vec<(CompactionPhase, usize, usize, usize)> {
    let mut session =
        FrozenOperationVerificationSession::start_compaction_closure(connection).unwrap();
    let mut batches = Vec::new();
    for _ in 0..1000 {
        let (phase, usage, finished) = match session.step(&mut || false).unwrap() {
            FrozenOperationVerificationStep::CompactionClosurePending { phase, usage } => {
                (phase, usage, false)
            }
            FrozenOperationVerificationStep::CompactionClosureVerified(usage) => {
                (CompactionPhase::FrozenMessages, usage, true)
            }
            other => panic!("unexpected R2 result {other:?}"),
        };
        assert!(usage.rows <= 100);
        assert!(usage.canonical_source_bytes <= MIB);
        assert!(usage.encoded_closure_bytes <= 8 * MIB);
        batches.push((
            phase,
            usage.rows,
            usage.canonical_source_bytes,
            usage.encoded_closure_bytes,
        ));
        if finished {
            assert!(session.step(&mut || false).is_err(), "never remint success");
            return batches;
        }
    }
    panic!("R2 did not finish");
}

#[test]
fn compaction_closure_scans_three_phases_over_one_hundred_without_mutation() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    let sources = seed(&mut storage, 101, true, false, 0, 0);
    storage.checkpoint().unwrap();
    let before = database_snapshot(&storage.connection);
    let bytes = fs::read(&path).unwrap();
    let batches = verify_all(&mut storage.connection);
    use CompactionPhase::*;
    assert_eq!(
        batches.iter().map(|b| (b.0, b.1)).collect::<Vec<_>>(),
        vec![
            (Operations, 100),
            (Operations, 2),
            (Prefixes, 100),
            (Prefixes, 1),
            (FrozenMessages, 100),
            (FrozenMessages, 1)
        ]
    );
    assert_eq!(database_snapshot(&storage.connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(
        storage.append_event(&sources[101]).is_err(),
        "product append remains fail closed"
    );
    drop(storage);
    assert!(
        Storage::open(&path).is_err(),
        "product open remains fail closed"
    );
}

#[test]
fn compaction_closure_accepts_null_repeat_introductions_empty_messages_and_unused_baseline() {
    for chat in [false, true] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        let sources = seed(&mut storage, 4, chat, chat, 0, 0);
        // Materialization already wrote the retained anchor's historical baseline.
        let unused: bool = storage.connection.query_row("SELECT EXISTS(SELECT 1 FROM ledger_baselines b WHERE b.baseline_id=?1 AND NOT EXISTS(SELECT 1 FROM ledger_compaction_prefixes p WHERE p.baseline_id=b.baseline_id))",[baseline_id(sources.last().unwrap())],|row|row.get(0)).unwrap();
        assert!(unused);
        let batches = verify_all(&mut storage.connection);
        assert_eq!(batches.last().unwrap().1, if chat { 2 } else { 0 });
    }
}

#[test]
fn compaction_closure_allows_unfrozen_live_anchor_introduction() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_with_anchor_intro(&mut storage, 1, true, false, 0, 0, true);
    let frozen: bool = storage
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM frozen_message_sources WHERE source_aggregate_sequence=2)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!frozen);
    verify_all(&mut storage.connection);
}

fn advance(session: &mut FrozenOperationVerificationSession<'_>, calls: usize) {
    for _ in 0..calls {
        assert!(matches!(
            session.step(&mut || false).unwrap(),
            FrozenOperationVerificationStep::CompactionClosurePending { .. }
        ));
    }
}

#[test]
fn compaction_closure_external_commits_invalidate_each_phase_and_late_batch() {
    let (_template, template) = fixture(101, true, false, 0, 0);
    for calls in 0..=6 {
        let (_root, path) = database();
        fs::copy(&template, &path).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        let other = Connection::open(&path).unwrap();
        let before;
        let bytes;
        {
            let mut session =
                FrozenOperationVerificationSession::start_compaction_closure(&mut connection)
                    .unwrap();
            advance(&mut session, calls.min(5));
            if calls == 6 {
                assert!(matches!(
                    session.step(&mut || false).unwrap(),
                    FrozenOperationVerificationStep::CompactionClosureVerified(_)
                ));
            }
            // Also demonstrates no transaction/lock survives a step.
            other.execute_batch("BEGIN IMMEDIATE; UPDATE maintenance_jobs SET updated_at='2026-09-02T02:00:00Z'; COMMIT;").unwrap();
            before = database_snapshot(&other);
            bytes = fs::read(&path).unwrap();
            assert_eq!(
                session.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            assert!(session.step(&mut || false).is_err());
        }
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut fresh =
            FrozenOperationVerificationSession::start_compaction_closure(&mut connection).unwrap();
        assert!(
            matches!(fresh.step(&mut ||false).unwrap(),FrozenOperationVerificationStep::CompactionClosurePending{phase:CompactionPhase::Operations,usage} if usage.rows==100)
        );
    }
}

#[test]
fn compaction_closure_cancellation_discards_each_phase_at_entry_row_and_precommit() {
    let (_template, template) = fixture(101, true, false, 0, 0);
    for calls in 0..6 {
        let mut cancellation_polls = vec![
            1,
            2,
            if calls % 2 == 0 {
                102
            } else if calls == 1 {
                4
            } else {
                3
            },
        ];
        if calls % 2 == 0 {
            cancellation_polls.push(50);
        } else if calls == 1 {
            cancellation_polls.push(3);
        }
        for poll in cancellation_polls {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            {
                let mut session =
                    FrozenOperationVerificationSession::start_compaction_closure(&mut connection)
                        .unwrap();
                advance(&mut session, calls);
                let mut polls = 0;
                assert_eq!(
                    session
                        .step(&mut || {
                            polls += 1;
                            polls == poll
                        })
                        .unwrap(),
                    FrozenOperationVerificationStep::Cancelled,
                    "calls={calls} poll={poll}"
                );
                assert!(session.step(&mut || false).is_err());
            }
            assert!(connection.is_autocommit());
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            let mut fresh =
                FrozenOperationVerificationSession::start_compaction_closure(&mut connection)
                    .unwrap();
            assert!(
                matches!(fresh.step(&mut ||false).unwrap(),FrozenOperationVerificationStep::CompactionClosurePending{phase:CompactionPhase::Operations,usage} if usage.rows==100)
            );
        }
    }
    for poll in [1, 2] {
        let (_root, path) = fixture(1, false, false, 0, 0);
        let mut connection = Connection::open(&path).unwrap();
        let before = database_snapshot(&connection);
        let bytes = fs::read(&path).unwrap();
        {
            let mut session =
                FrozenOperationVerificationSession::start_compaction_closure(&mut connection)
                    .unwrap();
            advance(&mut session, 2); // Operations and prefixes exhausted; messages empty.
            let mut polls = 0;
            assert_eq!(
                session
                    .step(&mut || {
                        polls += 1;
                        polls == poll
                    })
                    .unwrap(),
                FrozenOperationVerificationStep::Cancelled
            );
            assert!(session.step(&mut || false).is_err());
        }
        assert!(connection.is_autocommit());
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut fresh =
            FrozenOperationVerificationSession::start_compaction_closure(&mut connection).unwrap();
        assert!(
            matches!(fresh.step(&mut ||false).unwrap(),FrozenOperationVerificationStep::CompactionClosurePending{phase:CompactionPhase::Operations,usage} if usage.rows==2)
        );
    }
}

#[test]
fn compaction_closure_failed_commit_discards_each_phase_and_late_batch() {
    use std::sync::atomic::{AtomicBool, Ordering};
    unsafe extern "C" fn deny_commit(
        context: *mut std::ffi::c_void,
        action: std::ffi::c_int,
        first: *const std::ffi::c_char,
        _second: *const std::ffi::c_char,
        _database: *const std::ffi::c_char,
        _trigger: *const std::ffi::c_char,
    ) -> std::ffi::c_int {
        // SAFETY: each connection unregisters the callback before the boxed flag drops.
        let reject = unsafe { &*context.cast::<AtomicBool>() };
        if action == rusqlite::ffi::SQLITE_TRANSACTION
            && !first.is_null()
            && unsafe { std::ffi::CStr::from_ptr(first) }.to_bytes() == b"COMMIT"
            && reject.load(Ordering::Relaxed)
        {
            rusqlite::ffi::SQLITE_DENY
        } else {
            rusqlite::ffi::SQLITE_OK
        }
    }
    let (_template, template) = fixture(101, true, false, 0, 0);
    for calls in 0..6 {
        let (_root, path) = database();
        fs::copy(&template, &path).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        let before = database_snapshot(&connection);
        let bytes = fs::read(&path).unwrap();
        let reject = Box::new(AtomicBool::new(false));
        // SAFETY: the test exclusively owns this connection and callback context.
        unsafe {
            assert_eq!(
                rusqlite::ffi::sqlite3_set_authorizer(
                    connection.handle(),
                    Some(deny_commit),
                    (&*reject as *const AtomicBool).cast_mut().cast()
                ),
                rusqlite::ffi::SQLITE_OK
            );
        }
        {
            let mut session =
                FrozenOperationVerificationSession::start_compaction_closure(&mut connection)
                    .unwrap();
            advance(&mut session, calls);
            reject.store(true, Ordering::Relaxed);
            assert_eq!(
                session.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            reject.store(false, Ordering::Relaxed);
            assert!(session.step(&mut || false).is_err());
        }
        // SAFETY: remove the callback while its context is still alive.
        unsafe {
            assert_eq!(
                rusqlite::ffi::sqlite3_set_authorizer(
                    connection.handle(),
                    None,
                    std::ptr::null_mut()
                ),
                rusqlite::ffi::SQLITE_OK
            );
        }
        assert!(connection.is_autocommit());
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut fresh =
            FrozenOperationVerificationSession::start_compaction_closure(&mut connection).unwrap();
        assert!(
            matches!(fresh.step(&mut ||false).unwrap(),FrozenOperationVerificationStep::CompactionClosurePending{phase:CompactionPhase::Operations,usage} if usage.rows==100)
        );
    }
}

#[test]
fn compaction_closure_queries_use_exact_indexes_without_global_scans() {
    let (_root, path) = database();
    let storage = Storage::open(&path).unwrap();
    for phase in [
        CompactionPhase::Operations,
        CompactionPhase::Prefixes,
        CompactionPhase::FrozenMessages,
    ] {
        for query in phase_queries_for_test(phase) {
            let mut stmt = storage
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
                .unwrap();
            let bindings = vec![1_i64; stmt.parameter_count()];
            let plans: Vec<String> = stmt
                .query_map(rusqlite::params_from_iter(bindings), |row| row.get(3))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert!(
                plans
                    .iter()
                    .all(|p| p.contains("SEARCH ") && !p.contains("TEMP B-TREE")),
                "{query}: {plans:?}"
            );
        }
    }
}
