use super::reopen_closure::{corrupt, fixture as compacted_fixture};
use super::*;
use crate::storage::reopen::{
    projection_batch_for_test, projection_queries_for_test, LedgerProjectionPhase, ProjectionPhase,
};

const STAMP: &str = "2026-09-02T01:00:00Z";
const MIB: usize = 1024 * 1024;
const PHASES: [ProjectionPhase; 6] = [
    ProjectionPhase::Ledger,
    ProjectionPhase::AppInstances,
    ProjectionPhase::Projects,
    ProjectionPhase::Chats,
    ProjectionPhase::Runs,
    ProjectionPhase::Locks,
];
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn seed_mixed(storage: &mut Storage, count: usize, ready: bool) {
    for n in 0..count {
        let project = format!("project_r3_{n}");
        let chat = format!("chat_r3_{n}");
        let run = format!("run_r3_{n}");
        let lock = format!("lock_r3_{n}");
        let app = format!("app_r3_{n}");
        for (aggregate, id, payload) in [
            (
                "app_instance",
                app.clone(),
                json!({"sidecar_instance_id":app,"pid":100+n,"platform":"macos","started_at":STAMP,"heartbeat_at":STAMP,"clean_shutdown_at":null}),
            ),
            (
                "project",
                project.clone(),
                json!({"project_id":project,"canonical_path":format!("/fixture/r3/{n}")}),
            ),
            (
                "chat",
                chat.clone(),
                materializable_chat_payload(&chat, &project, 0, Value::Null),
            ),
            (
                "run",
                run.clone(),
                json!({"run_id":run,"chat_id":chat,"state":"succeeded","worktree_path":format!("/fixture/r3/worktree/{n}")}),
            ),
            (
                "lock",
                lock.clone(),
                json!({"lock_id":lock,"project_id":project,"writer_run_id":run,"worktree_path":format!("/fixture/r3/worktree/{n}"),"poisoned":false,"recovery_required":false,"evidence":{"a":1,"b":2}}),
            ),
        ] {
            assert_eq!(
                storage
                    .append_event(&event(
                        &format!("evt_{id}"),
                        &format!("op_{id}"),
                        aggregate,
                        &id,
                        1,
                        payload,
                        STAMP
                    ))
                    .unwrap(),
                AppendOutcome::Committed
            );
        }
    }
    if ready {
        run_bounded_materializer_to_ready(storage);
    }
}
fn mixed_fixture(count: usize, ready: bool) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_mixed(&mut storage, count, ready);
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}
fn mixed_compacted_fixture() -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed_mixed(&mut storage, 101, false);
    super::reopen_closure::seed(&mut storage, 101, true, false, 0, 0);
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}
fn rejects(connection: &mut Connection, path: &Path) {
    let before = database_snapshot(connection);
    let bytes = fs::read(path).unwrap();
    {
        let mut verifier =
            FrozenOperationVerificationSession::start_ledger_projection(connection).unwrap();
        let mut failed = false;
        for _ in 0..100 {
            match verifier.step(&mut || false) {
                Err(e) => {
                    assert_eq!(e.kind, "storageSchemaMismatch");
                    failed = true;
                    break;
                }
                Ok(FrozenOperationVerificationStep::LedgerProjectionPending { .. }) => {}
                other => panic!("accepted corrupted fixture {other:?}"),
            }
        }
        assert!(failed);
        assert!(verifier.step(&mut || false).is_err());
    }
    assert!(connection.is_autocommit());
    assert_eq!(database_snapshot(connection), before);
    assert_eq!(fs::read(path).unwrap(), bytes);
}
fn chain_fixture(count: usize, chat: bool) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    if chat {
        storage
            .append_event(&event(
                "evt_chain_project",
                "op_chain_project",
                "project",
                "project_chain",
                1,
                json!({"project_id":"project_chain","canonical_path":"/fixture/chain"}),
                STAMP,
            ))
            .unwrap();
    }
    for sequence in 1..=count {
        let payload = if chat {
            materializable_chat_payload(
                "chat_chain",
                "project_chain",
                sequence as u64,
                team_message(
                    "chat_chain",
                    &format!("message_chain_{sequence}"),
                    sequence as u64,
                ),
            )
        } else {
            json!({"project_id":"project_chain","canonical_path":"/fixture/chain","sequence":sequence})
        };
        storage
            .append_event(&event(
                &format!("evt_chain_{sequence}"),
                &format!("op_chain_{sequence}"),
                if chat { "chat" } else { "project" },
                if chat { "chat_chain" } else { "project_chain" },
                sequence as u64,
                payload,
                STAMP,
            ))
            .unwrap();
    }
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}
fn complete(connection: &mut Connection) -> Vec<(LedgerProjectionPhase, usize, usize, usize)> {
    let mut session =
        FrozenOperationVerificationSession::start_ledger_projection(connection).unwrap();
    let mut batches = Vec::new();
    for _ in 0..1000 {
        let (phase, usage, done) = match session.step(&mut || false).unwrap() {
            FrozenOperationVerificationStep::LedgerProjectionPending { phase, usage } => {
                (phase, usage, false)
            }
            FrozenOperationVerificationStep::LedgerProjectionVerified(usage) => (
                LedgerProjectionPhase::Projection(ProjectionPhase::Locks),
                usage,
                true,
            ),
            other => panic!("unexpected R3A result {other:?}"),
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
        if done {
            assert!(session.step(&mut || false).is_err());
            return batches;
        }
    }
    panic!("R3A did not complete");
}

#[test]
fn ledger_projection_empty_pending_ready_and_small_replay_differential() {
    for count in [0, 2] {
        for ready in [false, true] {
            let (_root, path) = mixed_fixture(count, ready);
            let mut connection = Connection::open(&path).unwrap();
            verify_projection_replay(&connection).unwrap();
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let batches = complete(&mut connection);
            assert_eq!(batches.len(), 6);
            assert_eq!(batches[0].1, 5 * count);
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}

#[test]
fn ledger_projection_over_one_hundred_all_five_tables_and_compaction_lifetime() {
    let (_root, path) = mixed_fixture(101, true);
    let mut connection = Connection::open(&path).unwrap();
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    let batches = complete(&mut connection);
    assert_eq!(batches.len(), 16);
    assert_eq!(
        batches.iter().map(|b| b.1).collect::<Vec<_>>(),
        [100, 100, 100, 100, 100, 5, 100, 1, 100, 1, 100, 1, 100, 1, 100, 1]
    );
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let (_root, path) = mixed_compacted_fixture();
    let mut connection = Connection::open(&path).unwrap();
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    let batches = complete(&mut connection);
    assert_eq!(batches.len(), 22);
    assert!(matches!(batches[0].0, LedgerProjectionPhase::Compaction(_)));
    assert_eq!(
        batches[6].0,
        LedgerProjectionPhase::Projection(ProjectionPhase::Ledger)
    );
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(connection);
    assert!(Storage::open(&path).is_err());
    let (_root, path) = compacted_fixture(2, true, false, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    verify_projection_replay(&connection).unwrap();
    complete(&mut connection);
}

fn table(phase: ProjectionPhase) -> &'static str {
    match phase {
        ProjectionPhase::Ledger => "events",
        ProjectionPhase::AppInstances => "app_instances",
        ProjectionPhase::Projects => "projects",
        ProjectionPhase::Chats => "chats",
        ProjectionPhase::Runs => "runs",
        ProjectionPhase::Locks => "locks",
    }
}
fn source_id(phase: ProjectionPhase, n: usize) -> String {
    format!(
        "{}_r3_{n}",
        match phase {
            ProjectionPhase::Ledger | ProjectionPhase::AppInstances => "app",
            ProjectionPhase::Projects => "project",
            ProjectionPhase::Chats => "chat",
            ProjectionPhase::Runs => "run",
            ProjectionPhase::Locks => "lock",
        }
    )
}
fn resize_primary(connection: &mut Connection, phase: ProjectionPhase, n: usize, size: usize) {
    let id = source_id(phase, n);
    let source: String = connection
        .query_row(
            "SELECT canonical_json_payload FROM events WHERE aggregate_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    let mut payload: Value = serde_json::from_str(&source).unwrap();
    let padding = if matches!(
        phase,
        ProjectionPhase::Ledger | ProjectionPhase::AppInstances
    ) {
        "platform"
    } else {
        "padding"
    };
    let current = payload
        .get(padding)
        .and_then(Value::as_str)
        .unwrap_or("")
        .len();
    let baseline = if phase == ProjectionPhase::AppInstances {
        connection.query_row("SELECT length(CAST(sidecar_instance_id AS BLOB))+length(CAST(pid AS BLOB))+length(CAST(platform AS BLOB))+length(CAST(started_at AS BLOB))+length(CAST(heartbeat_at AS BLOB))+coalesce(length(CAST(clean_shutdown_at AS BLOB)),0)+length(CAST(last_event_sequence AS BLOB)) FROM app_instances WHERE sidecar_instance_id=?1",[&id],|r|r.get::<_,i64>(0)).unwrap() as usize
    } else {
        if padding == "padding" {
            payload[padding] = json!("");
        }
        canonical_json_v1(payload.clone()).unwrap().len()
    };
    payload[padding] = json!("x".repeat(size - baseline + current));
    let canonical = canonical_json_v1(payload.clone()).unwrap();
    let projection = if matches!(
        phase,
        ProjectionPhase::Ledger | ProjectionPhase::AppInstances
    ) {
        format!(
            "UPDATE app_instances SET platform={} WHERE sidecar_instance_id={};",
            literal(payload["platform"].as_str().unwrap()),
            literal(&id)
        )
    } else {
        format!(
            "UPDATE {} SET projection_json={} WHERE rowid={};",
            table(phase),
            literal(&canonical),
            n + 1
        )
    };
    corrupt(
        connection,
        &format!(
            "UPDATE events SET canonical_json_payload={} WHERE aggregate_id={}; {projection}",
            literal(&canonical),
            literal(&id)
        ),
    );
}

#[test]
fn ledger_projection_each_phase_primary_exact_mib_plus_one_and_yield() {
    for phase in PHASES {
        for size in [MIB, MIB + 1] {
            let (_root, path) = mixed_fixture(1, false);
            let mut connection = Connection::open(&path).unwrap();
            resize_primary(&mut connection, phase, 0, size);
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let batch = projection_batch_for_test(&mut connection, phase, 0, 0, false);
            if size == MIB {
                let batch = batch.unwrap_or_else(|e| panic!("{phase:?}: {e:?}"));
                assert_eq!(batch.1.rows, 1);
                assert_eq!(batch.1.canonical_source_bytes, MIB);
                assert!(batch.1.encoded_closure_bytes <= 8 * MIB);
            } else {
                assert_eq!(batch.unwrap_err().kind, "storageSchemaMismatch");
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let (_root, path) = mixed_fixture(2, false);
        let mut connection = Connection::open(&path).unwrap();
        resize_primary(&mut connection, phase, 0, 600_000);
        resize_primary(&mut connection, phase, 1, 600_000);
        let before = database_snapshot(&connection);
        let bytes = fs::read(&path).unwrap();
        let first = projection_batch_for_test(&mut connection, phase, 0, 0, false).unwrap();
        assert!(!first.2);
        assert!(first.1.canonical_source_bytes >= 600_000);
        assert!(first.1.canonical_source_bytes <= MIB);
        let second = projection_batch_for_test(&mut connection, phase, first.0, 0, false).unwrap();
        assert!(second.1.canonical_source_bytes >= 600_000);
        assert!(second.1.canonical_source_bytes <= MIB);
        assert!(second.2);
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn ledger_projection_each_phase_secondary_exact_plus_one_multibyte_and_yield() {
    for phase in PHASES {
        for extra in [0, 1] {
            let (_root, path) = mixed_fixture(1, false);
            let mut connection = Connection::open(&path).unwrap();
            let all = projection_batch_for_test(&mut connection, phase, 0, 0, false).unwrap();
            let cost = if phase == ProjectionPhase::Ledger {
                all.1.encoded_closure_bytes
                    - projection_batch_for_test(&mut connection, phase, 1, 0, false)
                        .unwrap()
                        .1
                        .encoded_closure_bytes
            } else {
                all.1.encoded_closure_bytes
            };
            corrupt(
                &connection,
                &format!(
                    "UPDATE events SET request_id={} WHERE aggregate_id={}",
                    literal(&"x".repeat(8 * MIB - cost + extra)),
                    literal(&source_id(phase, 0))
                ),
            );
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let result = projection_batch_for_test(&mut connection, phase, 0, 0, false);
            if extra == 0 {
                let batch = result.unwrap();
                assert_eq!(batch.1.rows, 1);
                assert_eq!(batch.1.encoded_closure_bytes, 8 * MIB);
            } else {
                assert_eq!(result.unwrap_err().kind, "storageSchemaMismatch");
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let (_root, path) = mixed_fixture(2, false);
        let mut connection = Connection::open(&path).unwrap();
        let multibyte = "界".repeat(3 * MIB);
        assert!(multibyte.chars().count() < 8 * MIB);
        assert!(multibyte.len() > 8 * MIB);
        corrupt(
            &connection,
            &format!(
                "UPDATE events SET request_id={} WHERE aggregate_id={}",
                literal(&multibyte),
                literal(&source_id(phase, 1))
            ),
        );
        assert_eq!(
            projection_batch_for_test(&mut connection, phase, 0, 0, false)
                .unwrap_err()
                .kind,
            "storageSchemaMismatch"
        );
        rejects(&mut connection, &path);
        let (_root, path) = mixed_fixture(2, false);
        let mut connection = Connection::open(&path).unwrap();
        for n in 0..2 {
            corrupt(
                &connection,
                &format!(
                    "UPDATE events SET request_id={} WHERE aggregate_id={}",
                    literal(&"x".repeat(4 * MIB)),
                    literal(&source_id(phase, n))
                ),
            );
        }
        let before = database_snapshot(&connection);
        let bytes = fs::read(&path).unwrap();
        let first = projection_batch_for_test(&mut connection, phase, 0, 0, false).unwrap();
        assert!(!first.2);
        assert!(first.1.encoded_closure_bytes > 4 * MIB);
        let second = projection_batch_for_test(&mut connection, phase, first.0, 0, false).unwrap();
        assert!(second.1.encoded_closure_bytes > 4 * MIB);
        assert!(second.1.encoded_closure_bytes <= 8 * MIB);
        assert!(second.2);
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn ledger_projection_pending_missing_index_allowed_ready_missing_rejected_and_json_order_preserved()
{
    for ready in [false, true] {
        let (_root, path) = mixed_fixture(2, ready);
        let mut connection = Connection::open(&path).unwrap();
        corrupt(&connection, "DELETE FROM event_source_index");
        if ready {
            let mut session =
                FrozenOperationVerificationSession::start_ledger_projection(&mut connection)
                    .unwrap();
            assert_eq!(
                session.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
        } else {
            complete(&mut connection);
        }
    }
    let (_root, path) = mixed_fixture(1, false);
    let mut connection = Connection::open(&path).unwrap();
    connection.execute("UPDATE projects SET projection_json=' { \"canonical_path\" : \"/fixture/r3/0\", \"project_id\" : \"project_r3_0\" } '",[]).unwrap();
    connection
        .execute(
            "UPDATE locks SET evidence_json=' { \"b\" : 2, \"a\" : 1 } '",
            [],
        )
        .unwrap();
    complete(&mut connection);
    let (_root, path) = mixed_fixture(2, false);
    let mut connection = Connection::open(&path).unwrap();
    insert_v4_job(
        &connection,
        "job_r3_pending",
        "pending",
        "indexSource",
        10,
        0,
        0,
        None,
        None,
    );
    complete(&mut connection);
}

#[test]
fn ledger_projection_first_and_late_ledger_corruption_matrix() {
    let (_template, template) = chain_fixture(102, false);
    for target in [1, 102] {
        for kind in [
            "payload",
            "canonical",
            "id",
            "event_type",
            "sequence",
            "row_order",
            "index",
        ] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let sql=match kind {
            "payload"=>format!("UPDATE events SET canonical_json_payload='[]' WHERE aggregate_sequence={target}"),
            "canonical"=>format!("UPDATE events SET canonical_json_payload=' '||canonical_json_payload WHERE aggregate_sequence={target}"),
            "id"=>format!("UPDATE events SET aggregate_id='project_wrong' WHERE aggregate_sequence={target}"),
            "event_type"=>format!("UPDATE events SET event_type='project.deleted' WHERE aggregate_sequence={target}"),
            "sequence"=>format!("UPDATE events SET aggregate_sequence=999 WHERE aggregate_sequence={target}"),
            "row_order"=>format!("UPDATE events SET rowid=1000 WHERE aggregate_sequence={}; UPDATE event_source_index SET event_rowid=1000 WHERE aggregate_sequence={}",if target==1 {1}else{101},if target==1 {1}else{101}),
            "index"=>format!("UPDATE event_source_index SET chat_durable_sequence=1 WHERE aggregate_sequence={target}"),
            _=>unreachable!(),
        };
            corrupt(&connection, &sql);
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let mut after = 0;
            if target > 100 {
                let first = projection_batch_for_test(
                    &mut connection,
                    ProjectionPhase::Ledger,
                    0,
                    0,
                    false,
                )
                .unwrap();
                assert_eq!(first.1.rows, 100, "{kind}");
                assert!(!first.2);
                after = first.0;
            }
            assert_eq!(
                projection_batch_for_test(
                    &mut connection,
                    ProjectionPhase::Ledger,
                    after,
                    0,
                    false
                )
                .unwrap_err()
                .kind,
                "storageSchemaMismatch",
                "{kind} {target}"
            );
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            rejects(&mut connection, &path);
        }
    }
}

#[test]
fn ledger_projection_first_and_late_all_projection_corruption_matrix() {
    let (_template, template) = mixed_fixture(102, true);
    for (phase, table, primary_id, column) in [
        (
            ProjectionPhase::AppInstances,
            "app_instances",
            "sidecar_instance_id",
            "platform",
        ),
        (
            ProjectionPhase::Projects,
            "projects",
            "project_id",
            "canonical_path",
        ),
        (ProjectionPhase::Chats, "chats", "chat_id", "project_id"),
        (ProjectionPhase::Runs, "runs", "run_id", "state"),
        (ProjectionPhase::Locks, "locks", "lock_id", "evidence_json"),
    ] {
        for target in [1, 102] {
            for kind in ["payload", "column", "stale", "missing", "extra"] {
                let (_root, path) = database();
                fs::copy(&template, &path).unwrap();
                let mut connection = Connection::open(&path).unwrap();
                let sql = match kind {
                    "payload" if phase == ProjectionPhase::AppInstances => {
                        format!("UPDATE {table} SET heartbeat_at='bad' WHERE rowid={target}")
                    }
                    "payload" => {
                        format!("UPDATE {table} SET projection_json='{{}}' WHERE rowid={target}")
                    }
                    "column" => format!(
                        "UPDATE {table} SET {column}='{}' WHERE rowid={target}",
                        if phase == ProjectionPhase::Locks {
                            "{}"
                        } else {
                            "wrong"
                        }
                    ),
                    "stale" => {
                        format!("UPDATE {table} SET last_event_sequence=2 WHERE rowid={target}")
                    }
                    "missing" => format!("DELETE FROM {table} WHERE rowid={target}"),
                    "extra" => format!(
                        "UPDATE {table} SET {primary_id}='extra_projection' WHERE rowid={target}"
                    ),
                    _ => unreachable!(),
                };
                corrupt(&connection, &sql);
                let before = database_snapshot(&connection);
                let bytes = fs::read(&path).unwrap();
                if kind != "missing" {
                    let mut after = 0;
                    if target > 100 {
                        let first =
                            projection_batch_for_test(&mut connection, phase, 0, 0, true).unwrap();
                        assert_eq!(first.1.rows, 100, "{phase:?} {kind}");
                        after = first.0;
                    }
                    assert_eq!(
                        projection_batch_for_test(&mut connection, phase, after, 0, true)
                            .unwrap_err()
                            .kind,
                        "storageSchemaMismatch",
                        "{phase:?} {kind} {target}"
                    );
                }
                assert_eq!(database_snapshot(&connection), before);
                assert_eq!(fs::read(&path).unwrap(), bytes);
                rejects(&mut connection, &path);
            }
        }
    }
}

#[test]
fn ledger_projection_chat_null_same_introduction_and_transition_corruption() {
    let (_root, path) = compacted_fixture(4, true, true, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    complete(&mut connection);
    let (_template, template) = chain_fixture(102, true);
    for target in [1, 102] {
        for present in [false, true] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let payload = materializable_chat_payload(
                "chat_chain",
                "project_chain",
                target + 1,
                team_message("chat_chain", &format!("message_chain_{target}"), target + 1),
            );
            let sql=format!("UPDATE events SET canonical_json_payload={} WHERE operation_id='op_chain_{target}'; UPDATE event_source_index SET chat_durable_sequence={} WHERE operation_id='op_chain_{target}'; {}",literal(&canonical_json_v1(payload).unwrap()),target+1,if present {String::new()}else{"DELETE FROM event_source_index;".into()});
            corrupt(&connection, &sql);
            if target > 100 {
                let first = projection_batch_for_test(
                    &mut connection,
                    ProjectionPhase::Ledger,
                    0,
                    0,
                    false,
                )
                .unwrap();
                assert_eq!(first.1.rows, 100);
                assert_eq!(
                    projection_batch_for_test(
                        &mut connection,
                        ProjectionPhase::Ledger,
                        first.0,
                        0,
                        false
                    )
                    .unwrap_err()
                    .kind,
                    "storageSchemaMismatch"
                );
            }
            rejects(&mut connection, &path);
        }
    }
}

#[test]
fn ledger_projection_hidden_rowids_and_compaction_bootstrap_fail_closed() {
    for table in [
        "events",
        "app_instances",
        "projects",
        "chats",
        "runs",
        "locks",
    ] {
        for rowid in [0, -1] {
            let (_root, path) = mixed_fixture(1, false);
            let mut connection = Connection::open(&path).unwrap();
            corrupt(
                &connection,
                &format!("UPDATE {table} SET rowid={rowid} WHERE rowid=1"),
            );
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            match FrozenOperationVerificationSession::start_ledger_projection(&mut connection) {
                Err(e) => assert_eq!(e.kind, "storageSchemaMismatch"),
                Ok(_) => panic!("hidden {table} row accepted"),
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    let (_root, path) = compacted_fixture(1, false, false, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    connection
        .execute("DELETE FROM maintenance_jobs", [])
        .unwrap();
    assert!(FrozenOperationVerificationSession::start_ledger_projection(&mut connection).is_err());
    let (_root, path) = mixed_fixture(1, false);
    let mut connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN").unwrap();
    assert!(FrozenOperationVerificationSession::start_ledger_projection(&mut connection).is_err());
    assert!(!connection.is_autocommit());
    connection.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn ledger_projection_fixed_query_plans_use_keysets_and_exact_indexes() {
    let (_root, path) = database();
    let storage = Storage::open(&path).unwrap();
    for phase in PHASES {
        for query in projection_queries_for_test(phase) {
            let mut statement = storage
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
                .unwrap();
            let params = vec![1_i64; statement.parameter_count()];
            let plans: Vec<String> = statement
                .query_map(rusqlite::params_from_iter(params), |r| r.get(3))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert!(
                plans
                    .iter()
                    .all(|p| !p.contains("SCAN ") && !p.contains("TEMP B-TREE")),
                "{query}: {plans:?}"
            );
            assert!(
                plans.iter().any(|p| p.contains("SEARCH ")),
                "{query}: {plans:?}"
            );
        }
    }
    // Exercise the test-private probe without the caller having a product cursor.
    let mut connection = Connection::open_in_memory().unwrap();
    assert!(
        projection_batch_for_test(&mut connection, ProjectionPhase::Ledger, 0, 0, false).is_err()
    );
}

#[test]
fn ledger_projection_all_typed_and_indexed_columns_reject_first_and_late_drift() {
    let (_template, template) = mixed_fixture(102, true);
    for (phase, assignments) in [
        (
            ProjectionPhase::AppInstances,
            vec![
                "sidecar_instance_id='wrong'",
                "pid=0",
                "pid=4294967296",
                "platform='wrong'",
                "started_at='wrong'",
                "heartbeat_at='wrong'",
                "clean_shutdown_at='wrong'",
                "last_event_sequence=2",
            ],
        ),
        (
            ProjectionPhase::Projects,
            vec![
                "project_id='wrong'",
                "canonical_path='wrong'",
                "last_event_sequence=2",
            ],
        ),
        (
            ProjectionPhase::Chats,
            vec![
                "chat_id='wrong'",
                "project_id='wrong'",
                "last_event_sequence=2",
            ],
        ),
        (
            ProjectionPhase::Runs,
            vec![
                "run_id='wrong'",
                "chat_id='wrong'",
                "state='wrong'",
                "worktree_path='wrong'",
                "last_event_sequence=2",
            ],
        ),
        (
            ProjectionPhase::Locks,
            vec![
                "lock_id='wrong'",
                "project_id='wrong'",
                "writer_run_id='wrong'",
                "worktree_path='wrong'",
                "poisoned=1",
                "recovery_required=1",
                "evidence_json='{}'",
                "last_event_sequence=2",
            ],
        ),
    ] {
        for target in [1, 102] {
            for assignment in &assignments {
                let (_root, path) = database();
                fs::copy(&template, &path).unwrap();
                let mut connection = Connection::open(&path).unwrap();
                corrupt(
                    &connection,
                    &format!(
                        "UPDATE {} SET {assignment} WHERE rowid={target}",
                        table(phase)
                    ),
                );
                let before = database_snapshot(&connection);
                let bytes = fs::read(&path).unwrap();
                let after = if target > 100 {
                    let batch =
                        projection_batch_for_test(&mut connection, phase, 0, 0, true).unwrap();
                    assert_eq!(batch.1.rows, 100);
                    batch.0
                } else {
                    0
                };
                assert_eq!(
                    projection_batch_for_test(&mut connection, phase, after, 0, true)
                        .unwrap_err()
                        .kind,
                    "storageSchemaMismatch",
                    "{phase:?} {assignment} {target}"
                );
                assert_eq!(database_snapshot(&connection), before);
                assert_eq!(fs::read(&path).unwrap(), bytes);
                rejects(&mut connection, &path);
            }
        }
    }
}

#[test]
fn ledger_projection_live_null_same_snapshots_and_small_invalid_replay_differential() {
    for indexed in [false, true] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        storage
            .append_event(&event(
                "evt_live_project",
                "op_live_project",
                "project",
                "project_live",
                1,
                json!({"project_id":"project_live","canonical_path":"/fixture/live"}),
                STAMP,
            ))
            .unwrap();
        let first = team_message("chat_live", "message_live_1", 1);
        for (offset, (durable, snapshot)) in [
            (1, first.clone()),
            (1, first),
            (1, Value::Null),
            (2, team_message("chat_live", "message_live_2", 2)),
        ]
        .into_iter()
        .enumerate()
        {
            storage
                .append_event(&event(
                    &format!("evt_live_{offset}"),
                    &format!("op_live_{offset}"),
                    "chat",
                    "chat_live",
                    offset as u64 + 1,
                    materializable_chat_payload("chat_live", "project_live", durable, snapshot),
                    STAMP,
                ))
                .unwrap();
        }
        storage.checkpoint().unwrap();
        drop(storage);
        let mut connection = Connection::open(&path).unwrap();
        if !indexed {
            corrupt(&connection, "DELETE FROM event_source_index");
        }
        complete(&mut connection);
    }
    for sql in [
        "UPDATE projects SET projection_json='{}'",
        "UPDATE events SET canonical_json_payload='{}' WHERE aggregate_type='project'",
        "UPDATE projects SET last_event_sequence=2",
    ] {
        let (_root, path) = mixed_fixture(1, false);
        let mut connection = Connection::open(&path).unwrap();
        corrupt(&connection, sql);
        // Compare acceptance, not the legacy replay's distinct error taxonomy.
        assert!(verify_projection_replay(&connection).is_err());
        rejects(&mut connection, &path);
    }
}

#[test]
fn ledger_projection_source_index_binding_first_and_late_and_compacted_anchor_boundary() {
    let (_template, template) = mixed_fixture(102, true);
    for target in [1, 501] {
        for assignment in [
            "event_rowid=9000",
            "event_id='wrong'",
            "operation_id='wrong'",
            "aggregate_type='project'",
            "aggregate_id='wrong'",
            "aggregate_sequence=9",
            "introduced_message_id='wrong'",
            "chat_durable_sequence=1",
            "chat_snapshot_message_id='wrong'",
        ] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            corrupt(
                &connection,
                &format!("UPDATE event_source_index SET {assignment} WHERE event_rowid={target}"),
            );
            if target > 100 {
                let first =
                    projection_batch_for_test(&mut connection, ProjectionPhase::Ledger, 0, 0, true)
                        .unwrap();
                assert_eq!(first.1.rows, 100);
            }
            rejects(&mut connection, &path);
        }
    }
    let (_first, first) = compacted_fixture(1, true, false, 0, 0);
    let (_late, late) = mixed_compacted_fixture();
    for (template, late) in [(&first, false), (&late, true)] {
        for kind in ["below_prefix", "missing_anchor", "anchor_gap"] {
            let (_root, path) = database();
            fs::copy(template, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let sql=match kind {
            "below_prefix"=>"INSERT INTO events(rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp) SELECT source_event_rowid,source_event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,event_timestamp FROM frozen_operation_sources WHERE operation_id='op_r2_1'",
            "missing_anchor"=>"DELETE FROM events WHERE aggregate_id='chat_r2'",
            "anchor_gap"=>"UPDATE events SET aggregate_sequence=aggregate_sequence+1 WHERE aggregate_id='chat_r2'",
            _=>unreachable!(),
        };
            corrupt(&connection, sql);
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            if kind != "missing_anchor" {
                let mut after = 0;
                loop {
                    match projection_batch_for_test(
                        &mut connection,
                        ProjectionPhase::Ledger,
                        after,
                        1000,
                        true,
                    ) {
                        Ok((next, usage, done)) => {
                            assert!(late && !done);
                            assert_eq!(usage.rows, 100);
                            after = next;
                        }
                        Err(error) => {
                            assert_eq!(error.kind, "storageSchemaMismatch");
                            break;
                        }
                    }
                }
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            // Missing retained anchors are intentionally rejected in preceding R2,
            // not by inventing a global reverse-prefix scan in the live ledger phase.
            rejects(&mut connection, &path);
        }
    }
}

fn advance(session: &mut FrozenOperationVerificationSession<'_>, calls: usize) {
    for _ in 0..calls {
        assert!(matches!(
            session.step(&mut || false).unwrap(),
            FrozenOperationVerificationStep::LedgerProjectionPending { .. }
        ));
    }
}
fn fresh_starts_at_zero(connection: &mut Connection, compacted: bool) {
    let mut fresh =
        FrozenOperationVerificationSession::start_ledger_projection(connection).unwrap();
    let FrozenOperationVerificationStep::LedgerProjectionPending { phase, usage } =
        fresh.step(&mut || false).unwrap()
    else {
        panic!("fresh session skipped initial phase")
    };
    if compacted {
        assert!(matches!(phase, LedgerProjectionPhase::Compaction(_)));
    } else {
        assert_eq!(
            phase,
            LedgerProjectionPhase::Projection(ProjectionPhase::Ledger)
        );
        assert_eq!(usage.rows, 100);
    }
}

#[test]
fn ledger_projection_external_commit_invalidates_every_phase_tail_and_terminal() {
    let (_template, template) = mixed_fixture(101, true);
    for calls in 0..=16 {
        let (_root, path) = database();
        fs::copy(&template, &path).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        let other = Connection::open(&path).unwrap();
        let before;
        let bytes;
        {
            let mut session =
                FrozenOperationVerificationSession::start_ledger_projection(&mut connection)
                    .unwrap();
            advance(&mut session, calls.min(15));
            if calls == 16 {
                assert!(matches!(
                    session.step(&mut || false).unwrap(),
                    FrozenOperationVerificationStep::LedgerProjectionVerified(_)
                ));
            }
            other.execute_batch("BEGIN IMMEDIATE; UPDATE storage_schema_meta SET migrated_at='2026-09-02 02:00:00'; COMMIT").unwrap();
            assert_eq!(other.changes(), 1);
            before = database_snapshot(&other);
            bytes = fs::read(&path).unwrap();
            assert_eq!(
                session.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            assert!(session.step(&mut || false).is_err());
        }
        assert!(connection.is_autocommit());
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fresh_starts_at_zero(&mut connection, false);
    }
    let (_root, path) = compacted_fixture(1, true, false, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    let other = Connection::open(&path).unwrap();
    {
        let mut session =
            FrozenOperationVerificationSession::start_ledger_projection(&mut connection).unwrap();
        advance(&mut session, 3);
        other
            .execute(
                "UPDATE maintenance_jobs SET updated_at='2026-09-02T02:00:00Z'",
                [],
            )
            .unwrap();
        assert!(session.step(&mut || false).is_err());
    }
    fresh_starts_at_zero(&mut connection, true);
}

#[test]
fn ledger_projection_cancellation_entry_middle_precommit_and_empty_phases() {
    let (_template, template) = mixed_fixture(101, true);
    for calls in 0..16 {
        let rows = if calls < 5 || calls >= 6 && calls % 2 == 0 {
            100
        } else if calls == 5 {
            5
        } else {
            1
        };
        let mut polls = vec![1, 2, rows + 2];
        if rows > 1 {
            polls.push(if rows == 100 { 50 } else { 4 });
        }
        for poll in polls {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            {
                let mut session =
                    FrozenOperationVerificationSession::start_ledger_projection(&mut connection)
                        .unwrap();
                advance(&mut session, calls);
                let mut count = 0;
                assert_eq!(
                    session
                        .step(&mut || {
                            count += 1;
                            count == poll
                        })
                        .unwrap(),
                    FrozenOperationVerificationStep::Cancelled,
                    "calls={calls},poll={poll}"
                );
                assert!(session.step(&mut || false).is_err());
            }
            assert!(connection.is_autocommit());
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            fresh_starts_at_zero(&mut connection, false);
        }
    }
    for calls in 0..6 {
        for poll in [1, 2] {
            let (_root, path) = mixed_fixture(0, false);
            let mut connection = Connection::open(&path).unwrap();
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            {
                let mut session =
                    FrozenOperationVerificationSession::start_ledger_projection(&mut connection)
                        .unwrap();
                advance(&mut session, calls);
                let mut count = 0;
                assert_eq!(
                    session
                        .step(&mut || {
                            count += 1;
                            count == poll
                        })
                        .unwrap(),
                    FrozenOperationVerificationStep::Cancelled
                );
                assert!(session.step(&mut || false).is_err());
            }
            assert!(connection.is_autocommit());
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(complete(&mut connection).len(), 6);
        }
    }
    let (_root, path) = compacted_fixture(1, true, false, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    {
        let mut session =
            FrozenOperationVerificationSession::start_ledger_projection(&mut connection).unwrap();
        advance(&mut session, 3);
        assert_eq!(
            session.step(&mut || true).unwrap(),
            FrozenOperationVerificationStep::Cancelled
        );
    }
    fresh_starts_at_zero(&mut connection, true);
}

#[test]
fn ledger_projection_failed_commit_every_phase_and_read_or_begin_error_discards() {
    use std::sync::atomic::{AtomicU8, Ordering};
    unsafe extern "C" fn deny(
        context: *mut std::ffi::c_void,
        action: std::ffi::c_int,
        first: *const std::ffi::c_char,
        _second: *const std::ffi::c_char,
        _database: *const std::ffi::c_char,
        _trigger: *const std::ffi::c_char,
    ) -> std::ffi::c_int {
        // SAFETY: the owning test unregisters before the boxed flag drops.
        let mode = unsafe { &*context.cast::<AtomicU8>() }.load(Ordering::Relaxed);
        let name = if first.is_null() {
            b"".as_slice()
        } else {
            unsafe { std::ffi::CStr::from_ptr(first) }.to_bytes()
        };
        if mode == 1 && action == rusqlite::ffi::SQLITE_TRANSACTION && name == b"COMMIT"
            || mode == 2 && action == rusqlite::ffi::SQLITE_READ && name == b"events"
        {
            rusqlite::ffi::SQLITE_DENY
        } else {
            rusqlite::ffi::SQLITE_OK
        }
    }
    let (_template, template) = mixed_fixture(101, true);
    let (_compact_template, compact_template) = compacted_fixture(1, true, false, 0, 0);
    for (calls, mode, compacted) in (0..16)
        .map(|c| (c, 1, false))
        .chain([(0, 2, false), (3, 1, true)])
    {
        let (_root, path) = database();
        fs::copy(
            if compacted {
                &compact_template
            } else {
                &template
            },
            &path,
        )
        .unwrap();
        let mut connection = Connection::open(&path).unwrap();
        let before = database_snapshot(&connection);
        let bytes = fs::read(&path).unwrap();
        let reject = Box::new(AtomicU8::new(0));
        // SAFETY: callback context outlives registration and connection is exclusive.
        unsafe {
            assert_eq!(
                rusqlite::ffi::sqlite3_set_authorizer(
                    connection.handle(),
                    Some(deny),
                    (&*reject as *const AtomicU8).cast_mut().cast()
                ),
                rusqlite::ffi::SQLITE_OK
            );
        }
        {
            let mut session =
                FrozenOperationVerificationSession::start_ledger_projection(&mut connection)
                    .unwrap();
            advance(&mut session, calls);
            reject.store(mode, Ordering::Relaxed);
            assert_eq!(
                session.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            reject.store(0, Ordering::Relaxed);
            assert!(session.step(&mut || false).is_err());
        }
        // SAFETY: unregister while callback context is still alive.
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
        fresh_starts_at_zero(&mut connection, compacted);
    }
    let (_root, path) = mixed_fixture(101, true);
    let mut connection = Connection::open(&path).unwrap();
    connection.busy_timeout(std::time::Duration::ZERO).unwrap();
    let other = Connection::open(&path).unwrap();
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    {
        let mut session =
            FrozenOperationVerificationSession::start_ledger_projection(&mut connection).unwrap();
        other.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert_eq!(
            session.step(&mut || false).unwrap_err().kind,
            "storageSchemaMismatch"
        );
        other.execute_batch("ROLLBACK").unwrap();
        assert!(session.step(&mut || false).is_err());
    }
    assert!(connection.is_autocommit());
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fresh_starts_at_zero(&mut connection, false);
}
