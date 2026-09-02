//! R3B disposable-fixture tests. Phase probes are explicitly distinct from
//! unchanged full-session prerequisite/earliest-rejection evidence.
use super::reopen_closure::{corrupt, fixture as compacted_fixture};
use super::*;
use crate::storage::reopen::{
    derived_queries_for_test, DerivedPhase, DerivedProbe, DerivedUsage, DerivedVerificationPhase,
};
const STAMP: &str = "2026-09-02T08:00:00Z";
const MIB: usize = 1024 * 1024;
const PHASES: [DerivedPhase; 7] = [
    DerivedPhase::SourceIndexes,
    DerivedPhase::Receipts,
    DerivedPhase::Baselines,
    DerivedPhase::IntroductionIdentity,
    DerivedPhase::Messages,
    DerivedPhase::Workspaces,
    DerivedPhase::Coverage,
];
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
fn workspace(project: &str, n: usize, bound: bool) -> Value {
    json!({"schemaVersion":1,"preparationId":format!("preparation_b_{n}"),"manifest":{"schemaVersion":1,"projectId":project,"canonicalGitRoot":"/fixtures/repository","worktreeId":format!("worktree_b_{n}"),"worktreePath":format!("/fixtures/workspaces/worktree_b_{n}"),"workspaceKind":"standaloneSnapshot","snapshotTransport":"gitBundle","sourceMutationPolicy":"forbidden","objectFormat":"sha1","baseCommit":"0123456789abcdef0123456789abcdef01234567","baseTree":"89abcdef0123456789abcdef0123456789abcdef","sourceIdentity":"source_repository_01","workspaceIdentity":format!("workspace_b_{n}"),"headMode":"detached","lifecycle":"active"},"bindingState":if bound {"bound"}else{"unbound"},"boundChatId":if bound {json!(format!("chat_b_{n}"))}else{Value::Null},"createdAt":STAMP,"updatedAt":STAMP})
}
fn chat_payload(chat: &str, project: &str, n: usize, durable: u64, message: Value) -> Value {
    let mut payload = materializable_chat_payload(chat, project, durable, message);
    payload["worktree_id"] = json!(format!("worktree_b_{n}"));
    payload["workspace_identity"] = json!(format!("workspace_b_{n}"));
    payload["record"]["worktreeId"] = json!(format!("worktree_b_{n}"));
    if payload["last_message"].is_object() {
        payload["last_message"]["worktreeId"] = json!(format!("worktree_b_{n}"));
    }
    payload
}
fn seed(storage: &mut Storage, count: usize, ready: bool) {
    for n in 0..count {
        let project = format!("project_b_{n}");
        let chat = format!("chat_b_{n}");
        let id = format!("preparation_b_{n}");
        storage.append_event(&event(&format!("evt_project_b_{n}"),&format!("op_project_b_{n}"),"project",&project,1,json!({"project_id":project,"canonical_path":format!("/fixtures/project/{n}"),"prepared_workspaces":{id:workspace(&project,n,true)}}),STAMP)).unwrap();
        storage
            .append_event(&event(
                &format!("evt_chat_b_{n}"),
                &format!("op_chat_b_{n}"),
                "chat",
                &chat,
                1,
                chat_payload(
                    &chat,
                    &project,
                    n,
                    1,
                    team_message(&chat, &format!("message_b_{n}"), 1),
                ),
                STAMP,
            ))
            .unwrap();
    }
    if ready {
        run_bounded_materializer_to_ready(storage);
    }
}
fn fixture(count: usize, ready: bool) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed(&mut storage, count, ready);
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}
fn usage(usage: &DerivedUsage) {
    assert!(usage.rows <= usage.work_units);
    assert!(usage.work_units <= 100);
    assert!(usage.canonical_source_bytes <= MIB);
    assert!(usage.encoded_closure_bytes <= 8 * MIB);
    assert!(usage.retained_bytes <= usage.encoded_closure_bytes);
}
fn complete(connection: &mut Connection) -> Vec<(DerivedVerificationPhase, DerivedUsage)> {
    let mut session =
        FrozenOperationVerificationSession::start_derived_records(connection).unwrap();
    let mut batches = Vec::new();
    for call in 0..100_000 {
        match session
            .step(&mut || false)
            .unwrap_or_else(|error| panic!("full session call {call} after {batches:?}: {error:?}"))
        {
            FrozenOperationVerificationStep::DerivedRecordsPending {
                phase,
                usage: batch,
            } => {
                usage(&batch);
                batches.push((phase, batch));
            }
            FrozenOperationVerificationStep::DerivedRecordsVerified(batch) => {
                usage(&batch);
                batches.push((
                    DerivedVerificationPhase::Derived(DerivedPhase::Coverage),
                    batch,
                ));
                assert!(session.step(&mut || false).is_err());
                return batches;
            }
            other => panic!("wrong result {other:?}"),
        }
    }
    panic!("derived scan did not finish");
}
fn rejects(connection: &mut Connection, path: &Path) {
    let before = database_snapshot(connection);
    let bytes = fs::read(path).unwrap();
    {
        let mut session =
            FrozenOperationVerificationSession::start_derived_records(connection).unwrap();
        let mut failed = false;
        for _ in 0..100_000 {
            match session.step(&mut || false) {
                Err(error) => {
                    assert_eq!(error.kind, "storageSchemaMismatch");
                    failed = true;
                    break;
                }
                Ok(FrozenOperationVerificationStep::DerivedRecordsPending {
                    usage: batch, ..
                }) => usage(&batch),
                other => panic!("corrupt fixture accepted {other:?}"),
            }
        }
        assert!(failed);
        assert!(session.step(&mut || false).is_err());
    }
    assert!(connection.is_autocommit());
    assert_eq!(database_snapshot(connection), before);
    assert_eq!(fs::read(path).unwrap(), bytes);
}
fn probe(
    connection: &mut Connection,
    phase: DerivedPhase,
    ready: bool,
    cutoff: u64,
) -> Vec<DerivedUsage> {
    let mut probe = DerivedProbe::new(phase);
    let mut batches = Vec::new();
    for _ in 0..100_000 {
        let (batch, done) = probe.step(connection, cutoff, ready).unwrap();
        usage(&batch);
        batches.push(batch);
        if done {
            return batches;
        }
    }
    panic!("probe did not finish")
}

#[test]
fn derived_records_empty_pending_ready_and_all_five_tables_over_one_hundred() {
    for count in [0, 2, 101] {
        for ready in [false, true] {
            let (_root, path) = fixture(count, ready);
            let mut connection = Connection::open(&path).unwrap();
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let batches = complete(&mut connection);
            for phase in PHASES {
                assert!(batches
                    .iter()
                    .any(|(p, _)| *p == DerivedVerificationPhase::Derived(phase)));
            }
            if count == 101 && ready {
                for phase in [
                    DerivedPhase::SourceIndexes,
                    DerivedPhase::Receipts,
                    DerivedPhase::Baselines,
                    DerivedPhase::Messages,
                    DerivedPhase::Workspaces,
                ] {
                    assert!(
                        batches
                            .iter()
                            .filter(|(p, _)| *p == DerivedVerificationPhase::Derived(phase))
                            .map(|(_, u)| u.rows)
                            .sum::<usize>()
                            > 100
                    );
                }
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    let (_root, path) = compacted_fixture(101, true, false, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    complete(&mut connection);
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(connection);
    assert!(Storage::open(&path).is_err());
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    seed(&mut storage, 101, true);
    super::reopen_closure::seed(&mut storage, 103, false, false, 0, 0);
    storage.checkpoint().unwrap();
    drop(storage);
    let mut connection = Connection::open(&path).unwrap();
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    let batches = complete(&mut connection);
    for phase in [
        DerivedPhase::SourceIndexes,
        DerivedPhase::Receipts,
        DerivedPhase::Baselines,
        DerivedPhase::Messages,
        DerivedPhase::Workspaces,
    ] {
        assert!(
            batches
                .iter()
                .filter(|(p, _)| *p == DerivedVerificationPhase::Derived(phase))
                .map(|(_, u)| u.rows)
                .sum::<usize>()
                > 100
        );
    }
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(connection);
    assert!(Storage::open(&path).is_err());
}

#[test]
fn derived_records_pending_empty_index_keeps_raw_receipt_baseline_and_frozen_fallback() {
    let (_root, path) = fixture(3, true);
    let mut connection = Connection::open(&path).unwrap();
    corrupt(&connection,"DELETE FROM event_source_index; UPDATE storage_schema_meta SET materialization_state='pending';");
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    complete(&mut connection);
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let (_root, path) = compacted_fixture(3, true, true, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE storage_schema_meta SET materialization_state='pending'",
            [],
        )
        .unwrap();
    complete(&mut connection);
    let (_root, path) = fixture(2, true);
    let mut connection = Connection::open(&path).unwrap();
    corrupt(&connection,"DELETE FROM event_source_index WHERE event_rowid=1; UPDATE storage_schema_meta SET materialization_state='pending';");
    complete(&mut connection);
}

#[test]
fn derived_records_query_plans_use_unfiltered_keysets_and_exact_indexes() {
    let (_root, path) = fixture(1, true);
    let storage = Storage::open(&path).unwrap();
    for query in derived_queries_for_test() {
        assert!(
            !query.contains("json_extract")
                && !query.contains("OFFSET")
                && !query.contains("COUNT(")
        );
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
        assert!(plans.iter().any(|p| p.contains("SEARCH ")));
    }
    drop(storage);
    let mut connection = Connection::open(&path).unwrap();
    assert_eq!(
        probe(&mut connection, DerivedPhase::SourceIndexes, true, 0)
            .iter()
            .map(|u| u.rows)
            .sum::<usize>(),
        2
    );
}

fn phase_rejects(connection: &mut Connection, path: &Path, phase: DerivedPhase, late: bool) {
    let before = database_snapshot(connection);
    let bytes = fs::read(path).unwrap();
    let mut probe = DerivedProbe::new(phase);
    let mut rows = 0;
    loop {
        match probe.step(connection, 0, true) {
            Ok((batch, false)) => {
                usage(&batch);
                rows += batch.rows;
            }
            Err(error) => {
                assert_eq!(error.kind, "storageSchemaMismatch");
                break;
            }
            other => panic!("corruption accepted {phase:?}: {other:?}"),
        }
    }
    if late {
        assert!(
            rows >= 100,
            "{phase:?} failed before first 100 rows: {rows}"
        );
    }
    assert!(probe.step(connection, 0, true).is_err());
    assert!(connection.is_autocommit());
    assert_eq!(database_snapshot(connection), before);
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn derived_records_each_reverse_table_first_and_after_one_hundred_corruption() {
    let (_template, original) = fixture(102, true);
    let cases = [
        (
            DerivedPhase::SourceIndexes,
            "event_source_index",
            "event_id='wrong'",
        ),
        (
            DerivedPhase::SourceIndexes,
            "event_source_index",
            "aggregate_sequence=900",
        ),
        (
            DerivedPhase::Receipts,
            "operation_receipts",
            "event_id='wrong'",
        ),
        (
            DerivedPhase::Receipts,
            "operation_receipts",
            "canonical_payload_json='{}'",
        ),
        (
            DerivedPhase::Receipts,
            "operation_receipts",
            "canonical_payload_digest='sha256:bad'",
        ),
        (
            DerivedPhase::Receipts,
            "operation_receipts",
            "canonical_payload_digest_algorithm='wrong'",
        ),
        (
            DerivedPhase::Receipts,
            "operation_receipts",
            "canonical_payload_digest_version=2",
        ),
        (
            DerivedPhase::Receipts,
            "operation_receipts",
            "canonical_payload_digest_format='wrong'",
        ),
        (
            DerivedPhase::Receipts,
            "operation_receipts",
            "event_timestamp='wrong'",
        ),
        (
            DerivedPhase::Baselines,
            "ledger_baselines",
            "baseline_id='wrong'",
        ),
        (
            DerivedPhase::Baselines,
            "ledger_baselines",
            "canonical_projection_json='{}'",
        ),
        (
            DerivedPhase::Baselines,
            "ledger_baselines",
            "projection_digest='sha256:bad'",
        ),
        (
            DerivedPhase::Baselines,
            "ledger_baselines",
            "payload_schema_version=2",
        ),
        (
            DerivedPhase::Messages,
            "chat_messages",
            "message_id='wrong'",
        ),
        (DerivedPhase::Messages, "chat_messages", "chat_id='wrong'"),
        (
            DerivedPhase::Messages,
            "chat_messages",
            "event_sequence=900",
        ),
        (
            DerivedPhase::Messages,
            "chat_messages",
            "canonical_message_json='{}'",
        ),
        (
            DerivedPhase::Messages,
            "chat_messages",
            "message_digest='sha256:bad'",
        ),
        (
            DerivedPhase::Messages,
            "chat_messages",
            "created_at='wrong'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "preparation_id='wrong'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "project_id='wrong'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "worktree_id='wrong'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "workspace_identity='wrong'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "binding_state='unbound'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "bound_chat_id='wrong'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "canonical_record_json='{}'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "record_digest='sha256:bad'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "created_at='wrong'",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "updated_at='wrong'",
        ),
    ];
    for (phase, table, mutation) in cases {
        for row in [1, 101] {
            let (_root, path) = database();
            fs::copy(&original, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            corrupt(
                &connection,
                &format!("UPDATE {table} SET {mutation} WHERE rowid={row}"),
            );
            phase_rejects(&mut connection, &path, phase, row == 101);
            rejects(&mut connection, &path);
        }
    }
}

#[test]
fn derived_records_ready_forward_missing_rows_pending_absence_and_hidden_rowids() {
    let (_template, original) = fixture(102, true);
    for table in [
        "event_source_index",
        "operation_receipts",
        "ledger_baselines",
        "chat_messages",
        "prepared_workspaces",
    ] {
        for row in [1, 101] {
            let (_root, path) = database();
            fs::copy(&original, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            corrupt(
                &connection,
                &format!("DELETE FROM {table} WHERE rowid={row}"),
            );
            rejects(&mut connection, &path);
            connection
                .execute(
                    "UPDATE storage_schema_meta SET materialization_state='pending'",
                    [],
                )
                .unwrap();
            complete(&mut connection);
        }
        for row in [0, -1] {
            let (_root, path) = database();
            fs::copy(&original, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            corrupt(
                &connection,
                &format!("UPDATE {table} SET rowid={row} WHERE rowid=1"),
            );
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let result = FrozenOperationVerificationSession::start_derived_records(&mut connection);
            assert!(matches!(result,Err(error) if error.kind=="storageSchemaMismatch"));
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}

fn replace_live_chat(connection: &Connection, id: &str, payload: Value, keep_index: bool) {
    let canonical = canonical_json_v1(payload.clone()).unwrap();
    let index = if keep_index {
        format!("UPDATE event_source_index SET introduced_message_id={} WHERE aggregate_type='chat' AND aggregate_id={};",literal(payload["last_message"]["messageId"].as_str().unwrap()),literal(id))
    } else {
        format!(
            "DELETE FROM event_source_index WHERE aggregate_type='chat' AND aggregate_id={};",
            literal(id)
        )
    };
    corrupt(connection,&format!("UPDATE events SET canonical_json_payload={} WHERE aggregate_type='chat' AND aggregate_id={}; UPDATE chats SET projection_json={} WHERE chat_id={}; DELETE FROM operation_receipts WHERE aggregate_type='chat' AND aggregate_id={}; DELETE FROM ledger_baselines WHERE aggregate_type='chat' AND aggregate_id={}; DELETE FROM chat_messages WHERE chat_id={}; UPDATE storage_schema_meta SET materialization_state='pending'; {index}",literal(&canonical),literal(id),literal(&canonical),literal(id),literal(id),literal(id),literal(id)));
}

#[test]
fn derived_records_global_unindexed_identity_exhausts_and_rejects_live_or_frozen_collisions() {
    let (_root, path) = fixture(102, true);
    let mut connection = Connection::open(&path).unwrap();
    corrupt(&connection,"DELETE FROM event_source_index; DELETE FROM chat_messages; UPDATE storage_schema_meta SET materialization_state='pending';");
    let batches = probe(
        &mut connection,
        DerivedPhase::IntroductionIdentity,
        false,
        0,
    );
    assert!(batches.iter().any(|u| u.retained_bytes > 0));
    assert_eq!(
        batches.iter().map(|u| u.source_candidates).sum::<usize>(),
        102 * 204
    );
    replace_live_chat(
        &connection,
        "chat_b_101",
        chat_payload(
            "chat_b_101",
            "project_b_101",
            101,
            1,
            team_message("chat_b_101", "message_b_0", 1),
        ),
        false,
    );
    let mut scan = DerivedProbe::new(DerivedPhase::IntroductionIdentity);
    let (first, done) = scan.step(&mut connection, 0, false).unwrap();
    assert!(!done);
    assert_eq!(first.work_units, 100);
    assert_eq!(first.source_candidates, 98);
    assert_eq!(
        scan.step(&mut connection, 0, false)
            .unwrap()
            .0
            .source_candidates,
        100
    );
    assert_eq!(
        scan.step(&mut connection, 0, false).unwrap_err().kind,
        "storageSchemaMismatch"
    );
    rejects(&mut connection, &path);
    let (_root, path) = fixture(2, true);
    let mut connection = Connection::open(&path).unwrap();
    replace_live_chat(
        &connection,
        "chat_b_1",
        chat_payload(
            "chat_b_1",
            "project_b_1",
            1,
            1,
            team_message("chat_b_1", "message_b_0", 1),
        ),
        false,
    );
    rejects(&mut connection, &path);
    for indexed in [false, true] {
        let (_root, path) = compacted_fixture(3, true, false, 0, 0);
        let mut storage = Storage::open(&path).unwrap();
        seed(&mut storage, 1, true);
        storage.checkpoint().unwrap();
        drop(storage);
        let mut connection = Connection::open(&path).unwrap();
        // Bypass then restore the exact guards solely to model stored corruption;
        // ordinary INSERT is already blocked by the frozen-message-reuse trigger.
        replace_live_chat(
            &connection,
            "chat_b_0",
            chat_payload(
                "chat_b_0",
                "project_b_0",
                0,
                1,
                team_message("chat_b_0", "message_r2_1", 1),
            ),
            indexed,
        );
        let mut scan = DerivedProbe::new(DerivedPhase::IntroductionIdentity);
        assert_eq!(
            scan.step(&mut connection, 5, false).unwrap_err().kind,
            "storageSchemaMismatch"
        );
        rejects(&mut connection, &path);
    }
}

fn message_chain(count: usize) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    storage
        .append_event(&event(
            "evt_chain_project",
            "op_chain_project",
            "project",
            "project_chain",
            1,
            json!({"project_id":"project_chain","canonical_path":"/fixtures/chain"}),
            STAMP,
        ))
        .unwrap();
    for sequence in 1..=count {
        storage
            .append_event(&event(
                &format!("evt_chain_{sequence}"),
                &format!("op_chain_{sequence}"),
                "chat",
                "chat_chain",
                sequence as u64,
                materializable_chat_payload(
                    "chat_chain",
                    "project_chain",
                    u64::from(sequence == count),
                    if sequence == count {
                        team_message("chat_chain", "message_chain", 1)
                    } else {
                        Value::Null
                    },
                ),
                STAMP,
            ))
            .unwrap();
    }
    run_bounded_materializer_to_ready(&mut storage);
    storage.checkpoint().unwrap();
    drop(storage);
    let connection = Connection::open(&path).unwrap();
    corrupt(&connection,"DELETE FROM event_source_index; UPDATE storage_schema_meta SET materialization_state='pending';");
    drop(connection);
    (root, path)
}

#[test]
fn derived_records_unindexed_message_resolver_match_or_exhaustion_after_one_hundred() {
    let (_root, path) = message_chain(103);
    let mut connection = Connection::open(&path).unwrap();
    let batches = probe(&mut connection, DerivedPhase::Messages, false, 0);
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].rows, 1);
    assert_eq!(batches[0].source_candidates, 99);
    assert_eq!(batches[1].rows, 0);
    assert_eq!(batches[1].source_candidates, 4);
    assert!(batches[1].retained_bytes > 0);
    complete(&mut connection);
    let mut message = team_message("chat_chain", "missing_message", 1);
    message["worktreeId"] = json!("worktree_v6");
    let canonical = canonical_json_v1(message).unwrap();
    corrupt(&connection,&format!("UPDATE chat_messages SET message_id='missing_message',canonical_message_json={},message_digest={}",literal(&canonical),literal(&sha256_file_version_v1(canonical.as_bytes()))));
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    let mut scan = DerivedProbe::new(DerivedPhase::Messages);
    let (first, done) = scan.step(&mut connection, 0, false).unwrap();
    assert!(!done);
    assert_eq!(first.source_candidates, 99);
    assert_eq!(
        scan.step(&mut connection, 0, false).unwrap_err().kind,
        "storageSchemaMismatch"
    );
    assert!(scan.step(&mut connection, 0, false).is_err());
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    rejects(&mut connection, &path);
}

fn insert_workspace(connection: &Connection, record: &Value) {
    let canonical = canonical_json_v1(record.clone()).unwrap();
    let manifest = &record["manifest"];
    connection.execute("INSERT INTO prepared_workspaces(preparation_id,project_id,worktree_id,workspace_identity,binding_state,bound_chat_id,canonical_record_json,record_digest,created_at,updated_at,materialized_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9,?9)",params![record["preparationId"].as_str().unwrap(),manifest["projectId"].as_str().unwrap(),manifest["worktreeId"].as_str().unwrap(),manifest["workspaceIdentity"].as_str().unwrap(),record["bindingState"].as_str().unwrap(),record["boundChatId"].as_str(),canonical,sha256_file_version_v1(canonical.as_bytes()),STAMP]).unwrap();
}

fn members_fixture(count: usize, ready: bool) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    let members: Map<String, Value> = (0..count)
        .map(|n| {
            (
                format!("preparation_b_{n}"),
                workspace("project_members", n, false),
            )
        })
        .collect();
    storage.append_event(&event("evt_members","op_members","project","project_members",1,json!({"project_id":"project_members","canonical_path":"/fixtures/members","prepared_workspaces":members}),STAMP)).unwrap();
    if ready {
        run_bounded_materializer_to_ready(&mut storage);
    }
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}

fn historical_workspace(count: u64) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    for sequence in 1..=count {
        storage.append_event(&event(&format!("evt_workspace_{sequence}"),&format!("op_workspace_{sequence}"),"project","project_history",sequence,json!({"project_id":"project_history","canonical_path":"/fixtures/history","prepared_workspaces":if sequence==count-1 {json!({"preparation_b_0":workspace("project_history",0,false)})}else{json!({})}}),STAMP)).unwrap();
        if sequence == count - 1 {
            run_bounded_materializer_to_ready(&mut storage);
        }
    }
    storage.checkpoint().unwrap();
    drop(storage);
    let connection = Connection::open(&path).unwrap();
    corrupt(&connection,"DELETE FROM event_source_index; UPDATE storage_schema_meta SET materialization_state='pending'");
    drop(connection);
    (root, path)
}
fn workspace_absent(connection: &mut Connection, path: &Path, cutoff: u64) {
    let before = database_snapshot(connection);
    let bytes = fs::read(path).unwrap();
    let mut scan = DerivedProbe::new(DerivedPhase::Workspaces);
    let mut visited = 0;
    loop {
        match scan.step(connection, cutoff, false) {
            Ok((batch, false)) => {
                usage(&batch);
                visited += batch.source_candidates;
            }
            Err(error) => {
                assert_eq!(error.kind, "storageSchemaMismatch");
                break;
            }
            other => panic!("absent workspace accepted {other:?}"),
        }
    }
    assert!(visited > 100);
    assert!(scan.step(connection, cutoff, false).is_err());
    assert_eq!(database_snapshot(connection), before);
    assert_eq!(fs::read(path).unwrap(), bytes);
    rejects(connection, path);
}

#[test]
fn derived_records_pending_historical_live_and_frozen_workspace_candidates() {
    let (_root, path) = historical_workspace(205);
    let mut connection = Connection::open(&path).unwrap();
    let batches = probe(&mut connection, DerivedPhase::Workspaces, false, 0);
    assert_eq!(
        batches.iter().map(|u| u.source_candidates).sum::<usize>(),
        204
    );
    assert!(batches.len() > 1);
    assert!(batches[1].retained_bytes > 0);
    complete(&mut connection);
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    let mut scan = DerivedProbe::new(DerivedPhase::Workspaces);
    assert_eq!(
        scan.step(&mut connection, 0, true).unwrap_err().kind,
        "storageSchemaMismatch"
    );
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    corrupt(&connection, "DELETE FROM prepared_workspaces");
    insert_workspace(&connection, &workspace("project_history", 1, false));
    workspace_absent(&mut connection, &path, 0);
    let (_root, path) = compacted_fixture(205, false, false, 0, 0);
    let mut connection = Connection::open(&path).unwrap();
    super::reopen_closure::replace_deleted_payload(
        &connection,
        203,
        json!({"canonical_path":"/fixture/r2","project_id":"project_r2","prepared_workspaces":{"preparation_b_0":workspace("project_r2",0,false)}}),
    );
    insert_workspace(&connection, &workspace("project_r2", 0, false));
    connection
        .execute(
            "UPDATE storage_schema_meta SET materialization_state='pending'",
            [],
        )
        .unwrap();
    let batches = probe(&mut connection, DerivedPhase::Workspaces, false, 206);
    assert_eq!(
        batches.iter().map(|u| u.source_candidates).sum::<usize>(),
        204
    );
    assert!(batches.len() > 1);
    complete(&mut connection);
    corrupt(&connection, "DELETE FROM prepared_workspaces");
    insert_workspace(&connection, &workspace("project_r2", 1, false));
    workspace_absent(&mut connection, &path, 206);
}

#[test]
fn derived_records_ready_extra_workspace_after_one_hundred_rejects() {
    let (_root, path) = members_fixture(121, true);
    let mut connection = Connection::open(&path).unwrap();
    insert_workspace(&connection, &workspace("project_members", 1000, false));
    phase_rejects(&mut connection, &path, DerivedPhase::Workspaces, true);
    rejects(&mut connection, &path);
}

#[test]
fn derived_records_workspace_live_and_frozen_candidate_primary_exact_and_plus_one() {
    for frozen in [false, true] {
        for size in [MIB, MIB + 1] {
            let (_root, path) = if frozen {
                compacted_fixture(1, false, false, 0, 0)
            } else {
                members_fixture(0, false)
            };
            let mut connection = Connection::open(&path).unwrap();
            let project = if frozen {
                "project_r2"
            } else {
                "project_members"
            };
            let mut payload = json!({"project_id":project,"canonical_path":if frozen {"/fixture/r2"}else{"/fixtures/members"},"prepared_workspaces":{"preparation_b_0":workspace(project,0,false)},"padding":""});
            let base = canonical_json_v1(payload.clone()).unwrap().len();
            payload["padding"] = json!("x".repeat(size - base));
            if frozen {
                super::reopen_closure::replace_deleted_payload(&connection, 1, payload);
            } else {
                let canonical = canonical_json_v1(payload).unwrap();
                corrupt(&connection,&format!("UPDATE events SET canonical_json_payload={}; UPDATE projects SET projection_json={}",literal(&canonical),literal(&canonical)));
            }
            insert_workspace(&connection, &workspace(project, 0, false));
            connection
                .execute(
                    "UPDATE storage_schema_meta SET materialization_state='pending'",
                    [],
                )
                .unwrap();
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            if size == MIB {
                let batches = probe(
                    &mut connection,
                    DerivedPhase::Workspaces,
                    false,
                    if frozen { 2 } else { 0 },
                );
                assert_eq!(batches.len(), 2);
                assert_eq!(batches[0].rows, 1);
                assert_eq!(batches[1].rows, 0);
                assert_eq!(batches[1].canonical_source_bytes, MIB);
                assert!(batches[1].retained_bytes > 0);
                complete(&mut connection);
            } else {
                let mut scan = DerivedProbe::new(DerivedPhase::Workspaces);
                assert_eq!(
                    scan.step(&mut connection, if frozen { 2 } else { 0 }, false)
                        .unwrap_err()
                        .kind,
                    "storageSchemaMismatch"
                );
                assert!(scan
                    .step(&mut connection, if frozen { 2 } else { 0 }, false)
                    .is_err());
                rejects(&mut connection, &path);
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}

#[test]
fn derived_records_latest_raw_chat_binding_and_bounded_member_traversal() {
    let (_root, path) = fixture(1, true);
    let mut connection = Connection::open(&path).unwrap();
    corrupt(&connection,"DELETE FROM event_source_index; UPDATE storage_schema_meta SET materialization_state='pending'");
    complete(&mut connection);
    let mut changed = chat_payload(
        "chat_b_0",
        "project_b_0",
        7,
        1,
        team_message("chat_b_0", "message_b_0", 1),
    );
    changed["workspace_identity"] = json!("different_identity");
    replace_live_chat(&connection, "chat_b_0", changed, false);
    let mut scan = DerivedProbe::new(DerivedPhase::Workspaces);
    assert_eq!(
        scan.step(&mut connection, 0, false).unwrap_err().kind,
        "storageSchemaMismatch"
    );
    rejects(&mut connection, &path);
    for ready in [false, true] {
        let (_root, path) = members_fixture(103, ready);
        let mut connection = Connection::open(&path).unwrap();
        let batches = probe(&mut connection, DerivedPhase::Coverage, ready, 0);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].members, 99);
        assert_eq!(batches[1].members, 4);
        assert_eq!(batches[1].rows, 0);
        assert!(batches[1].retained_bytes > 0);
        complete(&mut connection);
        if ready {
            corrupt(
                &connection,
                "DELETE FROM prepared_workspaces WHERE preparation_id='preparation_b_99'",
            );
            let mut scan = DerivedProbe::new(DerivedPhase::Coverage);
            assert_eq!(scan.step(&mut connection, 0, true).unwrap().0.members, 99);
            assert_eq!(
                scan.step(&mut connection, 0, true).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            rejects(&mut connection, &path);
        }
    }
    let (_root, path) = members_fixture(1, false);
    let mut connection = Connection::open(&path).unwrap();
    let payload = json!({"project_id":"project_members","canonical_path":"/fixtures/members","prepared_workspaces":{"wrong_key":workspace("project_members",0,false)}});
    let canonical = canonical_json_v1(payload).unwrap();
    corrupt(
        &connection,
        &format!(
            "UPDATE events SET canonical_json_payload={}; UPDATE projects SET projection_json={}",
            literal(&canonical),
            literal(&canonical)
        ),
    );
    rejects(&mut connection, &path);
}

fn project_size(connection: &Connection, size: usize) {
    let raw: String = connection
        .query_row(
            "SELECT canonical_json_payload FROM events WHERE rowid=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut value: Value = serde_json::from_str(&raw).unwrap();
    value["padding"] = json!("");
    let base = canonical_json_v1(value.clone()).unwrap().len();
    value["padding"] = json!("x".repeat(size - base));
    let canonical = canonical_json_v1(value).unwrap();
    assert_eq!(canonical.len(), size);
    let digest = sha256_file_version_v1(canonical.as_bytes());
    corrupt(connection,&format!("UPDATE events SET canonical_json_payload={} WHERE rowid=1; UPDATE projects SET projection_json={}; UPDATE operation_receipts SET canonical_payload_json={},canonical_payload_digest={}; UPDATE ledger_baselines SET canonical_projection_json={},projection_digest={}",literal(&canonical),literal(&canonical),literal(&canonical),literal(digest.trim_start_matches("sha256:")),literal(&canonical),literal(&digest)));
}

#[test]
fn derived_records_primary_exact_limit_plus_one_and_remaining_budget_yield() {
    for phase in [
        DerivedPhase::SourceIndexes,
        DerivedPhase::Receipts,
        DerivedPhase::Baselines,
        DerivedPhase::IntroductionIdentity,
        DerivedPhase::Coverage,
    ] {
        for size in [MIB, MIB + 1] {
            let (_root, path) = members_fixture(0, true);
            let mut connection = Connection::open(&path).unwrap();
            project_size(&connection, size);
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let mut scan = DerivedProbe::new(phase);
            if size == MIB {
                let batches = probe(&mut connection, phase, true, 0);
                assert_eq!(
                    batches
                        .iter()
                        .map(|u| u.canonical_source_bytes)
                        .sum::<usize>(),
                    MIB
                );
                complete(&mut connection);
            } else {
                assert_eq!(
                    scan.step(&mut connection, 0, true).unwrap_err().kind,
                    "storageSchemaMismatch"
                );
                rejects(&mut connection, &path);
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    for (phase, table, column) in [
        (
            DerivedPhase::Messages,
            "chat_messages",
            "canonical_message_json",
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "canonical_record_json",
        ),
    ] {
        let (_root, path) = fixture(1, true);
        let mut connection = Connection::open(&path).unwrap();
        corrupt(
            &connection,
            &format!(
                "UPDATE {table} SET {column}={} WHERE rowid=1",
                literal(&"x".repeat(MIB + 1))
            ),
        );
        let mut scan = DerivedProbe::new(phase);
        assert_eq!(
            scan.step(&mut connection, 0, true).unwrap_err().kind,
            "storageSchemaMismatch"
        );
        rejects(&mut connection, &path);
    }
    let (_root, path) = members_fixture(0, true);
    let connection = Connection::open(&path).unwrap();
    project_size(&connection, MIB);
    drop(connection);
    let mut storage = Storage::open(&path).unwrap();
    storage
        .append_event(&event(
            "evt_second",
            "op_second",
            "project",
            "project_second",
            1,
            json!({"project_id":"project_second","canonical_path":"/fixtures/second"}),
            STAMP,
        ))
        .unwrap();
    run_bounded_materializer_to_ready(&mut storage);
    storage.checkpoint().unwrap();
    drop(storage);
    let mut connection = Connection::open(&path).unwrap();
    for phase in [
        DerivedPhase::SourceIndexes,
        DerivedPhase::Receipts,
        DerivedPhase::Baselines,
    ] {
        let batches = probe(&mut connection, phase, true, 0);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].rows, 1);
        assert_eq!(batches[0].canonical_source_bytes, MIB);
        assert_eq!(batches[1].rows, 1);
    }
}

#[test]
fn derived_records_exact_closure_utf8_overflow_and_retained_fresh_fit() {
    for (phase, table, column, with_chat) in [
        (DerivedPhase::SourceIndexes, "events", "request_id", false),
        (
            DerivedPhase::Receipts,
            "operation_receipts",
            "materialized_at",
            false,
        ),
        (
            DerivedPhase::Baselines,
            "ledger_baselines",
            "created_at",
            false,
        ),
        (
            DerivedPhase::Messages,
            "chat_messages",
            "materialized_at",
            true,
        ),
        (
            DerivedPhase::Workspaces,
            "prepared_workspaces",
            "materialized_at",
            true,
        ),
    ] {
        let (_root, path) = if with_chat {
            fixture(1, true)
        } else {
            members_fixture(0, true)
        };
        let mut connection = Connection::open(&path).unwrap();
        corrupt(
            &connection,
            &format!("UPDATE {table} SET {column}='x' WHERE rowid=1"),
        );
        let base = probe(&mut connection, phase, true, 0)
            .iter()
            .map(|u| u.encoded_closure_bytes)
            .sum::<usize>()
            - 1;
        for extra in [0, 1] {
            let padding = "x".repeat(8 * MIB - base + extra);
            corrupt(
                &connection,
                &format!(
                    "UPDATE {table} SET {column}={} WHERE rowid=1",
                    literal(&padding)
                ),
            );
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let mut scan = DerivedProbe::new(phase);
            if extra == 0 {
                let batches = probe(&mut connection, phase, true, 0);
                assert_eq!(
                    batches
                        .iter()
                        .map(|u| u.encoded_closure_bytes)
                        .sum::<usize>(),
                    8 * MIB
                );
            } else {
                assert_eq!(
                    scan.step(&mut connection, 0, true).unwrap_err().kind,
                    "storageSchemaMismatch"
                );
                assert!(scan.step(&mut connection, 0, true).is_err());
            }
            assert_eq!(database_snapshot(&connection), before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let padding = "한".repeat((8 * MIB - base) / 3 + 1);
        assert!(padding.chars().count() < 8 * MIB);
        corrupt(
            &connection,
            &format!(
                "UPDATE {table} SET {column}={} WHERE rowid=1",
                literal(&padding)
            ),
        );
        let mut scan = DerivedProbe::new(phase);
        assert_eq!(
            scan.step(&mut connection, 0, true).unwrap_err().kind,
            "storageSchemaMismatch"
        );
    }
    // Large retained row + fresh nested input must fail intrinsically, not yield
    // forever. Each independently fits 8 MiB; only the combined plan does not.
    let (_root, path) = message_chain(103);
    let mut connection = Connection::open(&path).unwrap();
    corrupt(&connection,&format!("UPDATE chat_messages SET materialized_at={}; UPDATE events SET request_id={} WHERE aggregate_type='chat' AND aggregate_sequence=102",literal(&"m".repeat(5*MIB)),literal(&"s".repeat(4*MIB))));
    let mut scan = DerivedProbe::new(DerivedPhase::Messages);
    let (first, done) = scan.step(&mut connection, 0, false).unwrap();
    assert!(!done);
    assert_eq!(first.source_candidates, 99);
    assert_eq!(
        scan.step(&mut connection, 0, false).unwrap_err().kind,
        "storageSchemaMismatch"
    );
    assert!(scan.step(&mut connection, 0, false).is_err());
}

#[test]
fn derived_records_message_and_workspace_valid_aggregate_exact_primary() {
    for messages in [true, false] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).unwrap();
        let mut workspace_primary = 0;
        for n in 0..17 {
            let project = format!("project_sized_{n}");
            let chat = format!("chat_sized_{n}");
            let mut record = workspace(&project, n, false);
            if !messages {
                record["manifest"]["worktreePath"] = json!("/");
                let base = canonical_json_v1(record.clone()).unwrap().len();
                record["manifest"]["worktreePath"] =
                    json!(format!("/{}", "x".repeat(32700 - base)));
                assert_eq!(canonical_json_v1(record.clone()).unwrap().len(), 32700);
            }
            let mut payload = json!({"project_id":project,"canonical_path":format!("/fixtures/sized/{n}"),"prepared_workspaces":if messages {json!({})}else{json!({format!("preparation_b_{n}"):record})}});
            if !messages {
                if n == 15 {
                    payload["padding"] = json!("");
                    let size = canonical_json_v1(payload.clone()).unwrap().len();
                    payload["padding"] = json!("x".repeat(MIB - workspace_primary - 32700 - size));
                }
                workspace_primary += 32700 + canonical_json_v1(payload.clone()).unwrap().len();
            }
            storage
                .append_event(&event(
                    &format!("evt_sized_project_{n}"),
                    &format!("op_sized_project_{n}"),
                    "project",
                    &project,
                    1,
                    payload,
                    STAMP,
                ))
                .unwrap();
            if messages {
                let mut message = team_message(&chat, &format!("message_sized_{n}"), 1);
                message["text"] = json!("");
                let base = canonical_json_v1(message.clone()).unwrap().len();
                message["text"] = json!("x".repeat(65536 - base));
                assert_eq!(canonical_json_v1(message.clone()).unwrap().len(), 65536);
                storage
                    .append_event(&event(
                        &format!("evt_sized_chat_{n}"),
                        &format!("op_sized_chat_{n}"),
                        "chat",
                        &chat,
                        1,
                        materializable_chat_payload(&chat, &project, 1, message),
                        STAMP,
                    ))
                    .unwrap();
            }
        }
        run_bounded_materializer_to_ready(&mut storage);
        storage.checkpoint().unwrap();
        drop(storage);
        let mut connection = Connection::open(&path).unwrap();
        let batches = probe(
            &mut connection,
            if messages {
                DerivedPhase::Messages
            } else {
                DerivedPhase::Workspaces
            },
            true,
            0,
        );
        assert_eq!(batches[0].canonical_source_bytes, MIB);
        assert_eq!(batches[0].rows, 16);
        assert!(batches.len() > 1);
        complete(&mut connection);
    }
    // A valid workspace record can itself occupy 1MiB. This phase-local
    // orphan control accepts its bounded decode, yields, then rejects absence.
    let (_root, path) = members_fixture(0, false);
    let mut connection = Connection::open(&path).unwrap();
    let mut record = workspace("project_members", 0, false);
    record["manifest"]["worktreePath"] = json!("/");
    let base = canonical_json_v1(record.clone()).unwrap().len();
    record["manifest"]["worktreePath"] = json!(format!("/{}", "x".repeat(MIB - base)));
    insert_workspace(&connection, &record);
    let mut scan = DerivedProbe::new(DerivedPhase::Workspaces);
    let (batch, done) = scan.step(&mut connection, 0, false).unwrap();
    assert!(!done);
    assert_eq!(batch.rows, 1);
    assert_eq!(batch.canonical_source_bytes, MIB);
    assert_eq!(
        scan.step(&mut connection, 0, false).unwrap_err().kind,
        "storageSchemaMismatch"
    );
}

#[test]
fn derived_records_coverage_retained_exact_closure_and_intrinsic_plus_one() {
    let (_root, path) = members_fixture(0, true);
    let mut connection = Connection::open(&path).unwrap();
    corrupt(
        &connection,
        "UPDATE operation_receipts SET materialized_at='x'",
    );
    let base = probe(&mut connection, DerivedPhase::Coverage, true, 0)[0].encoded_closure_bytes - 1;
    let padding = 8 * MIB - base + 1;
    corrupt(
        &connection,
        &format!(
            "UPDATE operation_receipts SET materialized_at={}",
            literal(&"x".repeat(padding))
        ),
    );
    let batches = probe(&mut connection, DerivedPhase::Coverage, true, 0);
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].rows, 1);
    assert_eq!(batches[1].rows, 0);
    assert_eq!(batches[1].canonical_source_bytes, 0);
    assert!(batches[1].retained_bytes > 0);
    let intrinsic = batches[1].encoded_closure_bytes - padding;
    for extra in [0, 1] {
        corrupt(
            &connection,
            &format!(
                "UPDATE operation_receipts SET materialized_at={}",
                literal(&"x".repeat(8 * MIB - intrinsic + extra))
            ),
        );
        if extra == 0 {
            let batches = probe(&mut connection, DerivedPhase::Coverage, true, 0);
            assert_eq!(batches.len(), 2);
            assert_eq!(batches[1].encoded_closure_bytes, 8 * MIB);
        } else {
            let mut scan = DerivedProbe::new(DerivedPhase::Coverage);
            assert_eq!(
                scan.step(&mut connection, 0, true).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            assert!(scan.step(&mut connection, 0, true).is_err());
        }
    }
}

#[test]
fn derived_records_identity_retained_candidate_exact_closure_and_impossible_plus_one() {
    let (_root, path) = fixture(1, true);
    let mut connection = Connection::open(&path).unwrap();
    corrupt(&connection,"DELETE FROM event_source_index; UPDATE storage_schema_meta SET materialization_state='pending'; UPDATE events SET request_id='x' WHERE rowid=1");
    let size_query = derived_queries_for_test()
        .into_iter()
        .find(|q| q.contains("length(CAST(") && q.ends_with("FROM events WHERE rowid=?1 LIMIT 1"))
        .unwrap();
    let fixed: usize = connection
        .query_row(&size_query, [1], |r| r.get::<_, i64>(0))
        .unwrap() as usize
        - 1;
    corrupt(
        &connection,
        &format!(
            "UPDATE events SET request_id={} WHERE rowid=1",
            literal(&"x".repeat(8 * MIB - 2000))
        ),
    );
    let batches = probe(
        &mut connection,
        DerivedPhase::IntroductionIdentity,
        false,
        0,
    );
    let retained = batches
        .iter()
        .find(|u| u.retained_bytes > 0)
        .unwrap()
        .retained_bytes;
    assert!(batches.len() > 1);
    assert!(batches
        .iter()
        .any(|u| u.rows == 0 && u.source_candidates > 0));
    for extra in [0, 1] {
        let padding = 8 * MIB - retained - fixed + extra;
        corrupt(
            &connection,
            &format!(
                "UPDATE events SET request_id={} WHERE rowid=1",
                literal(&"x".repeat(padding))
            ),
        );
        let before = database_snapshot(&connection);
        let bytes = fs::read(&path).unwrap();
        if extra == 0 {
            let batches = probe(
                &mut connection,
                DerivedPhase::IntroductionIdentity,
                false,
                0,
            );
            assert!(batches.iter().any(|u| u.encoded_closure_bytes == 8 * MIB));
            assert_eq!(
                batches.iter().map(|u| u.source_candidates).sum::<usize>(),
                2
            );
        } else {
            let mut scan = DerivedProbe::new(DerivedPhase::IntroductionIdentity);
            let mut failed = false;
            for _ in 0..4 {
                match scan.step(&mut connection, 0, false) {
                    Err(error) => {
                        assert_eq!(error.kind, "storageSchemaMismatch");
                        failed = true;
                        break;
                    }
                    Ok((batch, false)) => usage(&batch),
                    other => panic!("impossible identity closure accepted {other:?}"),
                }
            }
            assert!(failed);
            assert!(scan.step(&mut connection, 0, false).is_err());
        }
        assert_eq!(database_snapshot(&connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

fn advance(session: &mut FrozenOperationVerificationSession<'_>, calls: usize) {
    for _ in 0..calls {
        assert!(matches!(
            session.step(&mut || false).unwrap(),
            FrozenOperationVerificationStep::DerivedRecordsPending { .. }
        ));
    }
}
fn phase_calls(connection: &mut Connection) -> Vec<usize> {
    let batches = complete(connection);
    let mut calls = Vec::new();
    for phase in PHASES {
        calls.push(
            batches
                .iter()
                .position(|(p, _)| *p == DerivedVerificationPhase::Derived(phase))
                .unwrap(),
        );
    }
    calls
}
fn first_again(connection: &mut Connection) {
    let mut session =
        FrozenOperationVerificationSession::start_derived_records(connection).unwrap();
    assert!(matches!(
        session.step(&mut || false).unwrap(),
        FrozenOperationVerificationStep::DerivedRecordsPending {
            phase: DerivedVerificationPhase::Prerequisite(
                crate::storage::reopen::LedgerProjectionPhase::Projection(
                    crate::storage::reopen::ProjectionPhase::Ledger
                )
            ),
            ..
        }
    ));
}

#[test]
fn derived_records_external_commits_each_phase_nested_late_and_terminal_discard() {
    let (_template, original) = fixture(102, true);
    let mut connection = Connection::open(&original).unwrap();
    let mut calls = phase_calls(&mut connection);
    let total = complete(&mut connection).len();
    calls.push(total);
    drop(connection);
    for calls in calls {
        let (_root, path) = database();
        fs::copy(&original, &path).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        let other = Connection::open(&path).unwrap();
        let (expected, expected_bytes);
        {
            let mut session =
                FrozenOperationVerificationSession::start_derived_records(&mut connection).unwrap();
            if calls == total {
                advance(&mut session, total - 1);
                assert!(matches!(
                    session.step(&mut || false).unwrap(),
                    FrozenOperationVerificationStep::DerivedRecordsVerified(_)
                ));
            } else {
                advance(&mut session, calls);
            }
            other
                .execute(
                    "UPDATE storage_schema_meta SET migrated_at='2026-09-02 09:00:00'",
                    [],
                )
                .unwrap();
            expected = database_snapshot(&other);
            expected_bytes = fs::read(&path).unwrap();
            assert_eq!(
                session.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            assert!(session.step(&mut || false).is_err());
        }
        assert!(connection.is_autocommit());
        assert_eq!(database_snapshot(&connection), expected);
        assert_eq!(fs::read(&path).unwrap(), expected_bytes);
        first_again(&mut connection);
        assert_eq!(database_snapshot(&connection), expected);
        assert_eq!(fs::read(&path).unwrap(), expected_bytes);
    }
    let (_root, path) = message_chain(103);
    let mut connection = Connection::open(&path).unwrap();
    let calls = phase_calls(&mut connection)[4] + 1;
    let other = Connection::open(&path).unwrap();
    let (expected, bytes);
    {
        let mut session =
            FrozenOperationVerificationSession::start_derived_records(&mut connection).unwrap();
        advance(&mut session, calls);
        other
            .execute(
                "UPDATE storage_schema_meta SET migrated_at='2026-09-02 09:00:00'",
                [],
            )
            .unwrap();
        expected = database_snapshot(&other);
        bytes = fs::read(&path).unwrap();
        assert_eq!(
            session.step(&mut || false).unwrap_err().kind,
            "storageSchemaMismatch"
        );
        assert!(session.step(&mut || false).is_err());
    }
    assert_eq!(database_snapshot(&connection), expected);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    first_again(&mut connection);
}

#[test]
fn derived_records_cancel_entry_mid_precommit_empty_and_retained_progress() {
    for count in [0, 102] {
        let (_template, original) = fixture(count, true);
        let mut connection = Connection::open(&original).unwrap();
        let calls = phase_calls(&mut connection);
        drop(connection);
        for calls in calls {
            let mut connection = Connection::open(&original).unwrap();
            let mut polls = 0;
            {
                let mut session =
                    FrozenOperationVerificationSession::start_derived_records(&mut connection)
                        .unwrap();
                advance(&mut session, calls);
                session
                    .step(&mut || {
                        polls += 1;
                        false
                    })
                    .unwrap();
            }
            for target in [1, (polls / 2).max(1), polls] {
                let before = database_snapshot(&connection);
                let bytes = fs::read(&original).unwrap();
                {
                    let mut session =
                        FrozenOperationVerificationSession::start_derived_records(&mut connection)
                            .unwrap();
                    advance(&mut session, calls);
                    let mut seen = 0;
                    assert_eq!(
                        session
                            .step(&mut || {
                                seen += 1;
                                seen == target
                            })
                            .unwrap(),
                        FrozenOperationVerificationStep::Cancelled
                    );
                    assert!(session.step(&mut || false).is_err());
                }
                assert!(connection.is_autocommit());
                assert_eq!(database_snapshot(&connection), before);
                assert_eq!(fs::read(&original).unwrap(), bytes);
                first_again(&mut connection);
            }
        }
    }
    let (_root, path) = message_chain(103);
    let mut connection = Connection::open(&path).unwrap();
    let calls = phase_calls(&mut connection)[4] + 1;
    let before = database_snapshot(&connection);
    let bytes = fs::read(&path).unwrap();
    {
        let mut session =
            FrozenOperationVerificationSession::start_derived_records(&mut connection).unwrap();
        advance(&mut session, calls);
        let mut polls = 0;
        assert_eq!(
            session
                .step(&mut || {
                    polls += 1;
                    polls == 3
                })
                .unwrap(),
            FrozenOperationVerificationStep::Cancelled
        );
        assert!(session.step(&mut || false).is_err());
    }
    assert_eq!(database_snapshot(&connection), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    first_again(&mut connection);
}

#[test]
fn derived_records_actual_commit_read_and_busy_begin_errors_discard_every_phase() {
    use std::sync::atomic::{AtomicU8, Ordering};
    unsafe extern "C" fn deny(
        context: *mut std::ffi::c_void,
        action: std::ffi::c_int,
        first: *const std::ffi::c_char,
        _second: *const std::ffi::c_char,
        _database: *const std::ffi::c_char,
        _trigger: *const std::ffi::c_char,
    ) -> std::ffi::c_int {
        // SAFETY: boxed flag lives until after callback unregisters.
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
    let (_template, original) = fixture(102, true);
    let mut connection = Connection::open(&original).unwrap();
    let calls = phase_calls(&mut connection);
    drop(connection);
    for calls in calls {
        for mode in [1, 2, 3] {
            let (_root, path) = database();
            fs::copy(&original, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            connection.busy_timeout(std::time::Duration::ZERO).unwrap();
            let other = Connection::open(&path).unwrap();
            let before = database_snapshot(&connection);
            let bytes = fs::read(&path).unwrap();
            let reject = Box::new(AtomicU8::new(0));
            // SAFETY: exclusive connection and live callback context until unregister.
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
                    FrozenOperationVerificationSession::start_derived_records(&mut connection)
                        .unwrap();
                advance(&mut session, calls);
                if mode == 3 {
                    other.execute_batch("BEGIN IMMEDIATE").unwrap();
                } else {
                    reject.store(mode, Ordering::Relaxed);
                }
                assert_eq!(
                    session.step(&mut || false).unwrap_err().kind,
                    "storageSchemaMismatch"
                );
                reject.store(0, Ordering::Relaxed);
                if mode == 3 {
                    other.execute_batch("ROLLBACK").unwrap();
                }
                assert!(session.step(&mut || false).is_err());
            }
            // SAFETY: unregister before dropping boxed callback state.
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
            first_again(&mut connection);
        }
    }
}

#[test]
fn derived_records_each_retained_cursor_external_cancel_and_failed_commit_discards() {
    use std::sync::atomic::{AtomicBool, Ordering};
    unsafe extern "C" fn deny_commit(
        context: *mut std::ffi::c_void,
        action: std::ffi::c_int,
        first: *const std::ffi::c_char,
        _second: *const std::ffi::c_char,
        _database: *const std::ffi::c_char,
        _trigger: *const std::ffi::c_char,
    ) -> std::ffi::c_int {
        // SAFETY: context is live until unregister; SQLite supplies valid names.
        if unsafe { &*context.cast::<AtomicBool>() }.load(Ordering::Relaxed)
            && action == rusqlite::ffi::SQLITE_TRANSACTION
            && !first.is_null()
            && unsafe { std::ffi::CStr::from_ptr(first) }.to_bytes() == b"COMMIT"
        {
            rusqlite::ffi::SQLITE_DENY
        } else {
            rusqlite::ffi::SQLITE_OK
        }
    }
    for phase in [
        DerivedPhase::IntroductionIdentity,
        DerivedPhase::Messages,
        DerivedPhase::Workspaces,
        DerivedPhase::Coverage,
    ] {
        let (_template, original) = match phase {
            DerivedPhase::Workspaces => historical_workspace(205),
            DerivedPhase::Coverage => members_fixture(103, true),
            _ => message_chain(103),
        };
        let mut connection = Connection::open(&original).unwrap();
        let batches = complete(&mut connection);
        let calls = batches
            .iter()
            .position(|(p, u)| {
                *p == DerivedVerificationPhase::Derived(phase) && u.retained_bytes > 0
            })
            .unwrap();
        let mut polls = 0;
        {
            let mut session =
                FrozenOperationVerificationSession::start_derived_records(&mut connection).unwrap();
            advance(&mut session, calls);
            session
                .step(&mut || {
                    polls += 1;
                    false
                })
                .unwrap();
        }
        drop(connection);
        for mode in 0..5 {
            let (_root, path) = database();
            fs::copy(&original, &path).unwrap();
            let mut connection = Connection::open(&path).unwrap();
            let other = Connection::open(&path).unwrap();
            let mut expected = database_snapshot(&connection);
            let mut bytes = fs::read(&path).unwrap();
            let flag = Box::new(AtomicBool::new(false));
            // SAFETY: flag outlives registration and session owns the connection.
            unsafe {
                assert_eq!(
                    rusqlite::ffi::sqlite3_set_authorizer(
                        connection.handle(),
                        Some(deny_commit),
                        (&*flag as *const AtomicBool).cast_mut().cast()
                    ),
                    rusqlite::ffi::SQLITE_OK
                );
            }
            {
                let mut session =
                    FrozenOperationVerificationSession::start_derived_records(&mut connection)
                        .unwrap();
                advance(&mut session, calls);
                if mode == 0 {
                    other
                        .execute(
                            "UPDATE storage_schema_meta SET migrated_at='2026-09-02 09:00:00'",
                            [],
                        )
                        .unwrap();
                    expected = database_snapshot(&other);
                    bytes = fs::read(&path).unwrap();
                    assert_eq!(
                        session.step(&mut || false).unwrap_err().kind,
                        "storageSchemaMismatch"
                    );
                } else if mode == 4 {
                    flag.store(true, Ordering::Relaxed);
                    assert_eq!(
                        session.step(&mut || false).unwrap_err().kind,
                        "storageSchemaMismatch"
                    );
                    flag.store(false, Ordering::Relaxed);
                } else {
                    let target = match mode {
                        1 => 1,
                        2 => (polls / 2).max(1),
                        _ => polls,
                    };
                    let mut seen = 0;
                    assert_eq!(
                        session
                            .step(&mut || {
                                seen += 1;
                                seen == target
                            })
                            .unwrap(),
                        FrozenOperationVerificationStep::Cancelled
                    );
                }
                assert!(session.step(&mut || false).is_err());
            }
            // SAFETY: unregister before the flag drops.
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
            assert_eq!(database_snapshot(&connection), expected);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            first_again(&mut connection);
        }
    }
}

#[test]
fn derived_records_app_receipt_no_app_baseline_and_null_repeat_introductions() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    storage.append_event(&event("evt_app_b","op_app_b","app_instance","app_b",1,json!({"sidecar_instance_id":"app_b","pid":101,"platform":"macos","started_at":STAMP,"heartbeat_at":STAMP,"clean_shutdown_at":null}),STAMP)).unwrap();
    run_bounded_materializer_to_ready(&mut storage);
    storage.checkpoint().unwrap();
    drop(storage);
    let mut connection = Connection::open(&path).unwrap();
    complete(&mut connection);
    let raw: String = connection
        .query_row("SELECT canonical_json_payload FROM events", [], |r| {
            r.get(0)
        })
        .unwrap();
    let id = format!(
        "baseline_{}",
        sha256_file_version_v1(b"app_instance\0app_b\x001").trim_start_matches("sha256:")
    );
    corrupt(&connection,&format!("INSERT INTO ledger_baselines(baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at) VALUES({},'app_instance','app_b',1,1,{},{},{},{})",literal(&id),literal(&raw),literal(&sha256_file_version_v1(raw.as_bytes())),literal(STAMP),literal(STAMP)));
    phase_rejects(&mut connection, &path, DerivedPhase::Baselines, false);
    rejects(&mut connection, &path);
    let (_root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    storage
        .append_event(&event(
            "evt_repeat_project",
            "op_repeat_project",
            "project",
            "project_repeat",
            1,
            json!({"project_id":"project_repeat","canonical_path":"/fixtures/repeat"}),
            STAMP,
        ))
        .unwrap();
    for (sequence, durable, id) in [
        (1, 1, Some("message_a")),
        (2, 1, Some("message_a")),
        (3, 1, None),
        (4, 2, Some("message_b")),
        (5, 2, None),
        (6, 3, Some("message_c")),
    ] {
        let message = id.map_or(Value::Null, |id| team_message("chat_repeat", id, durable));
        storage
            .append_event(&event(
                &format!("evt_repeat_{sequence}"),
                &format!("op_repeat_{sequence}"),
                "chat",
                "chat_repeat",
                sequence,
                materializable_chat_payload("chat_repeat", "project_repeat", durable, message),
                STAMP,
            ))
            .unwrap();
    }
    run_bounded_materializer_to_ready(&mut storage);
    storage.checkpoint().unwrap();
    drop(storage);
    let mut connection = Connection::open(&path).unwrap();
    complete(&mut connection);
    corrupt(&connection,"DELETE FROM event_source_index; UPDATE storage_schema_meta SET materialization_state='pending'");
    complete(&mut connection);
    let batches = probe(
        &mut connection,
        DerivedPhase::IntroductionIdentity,
        false,
        0,
    );
    assert_eq!(
        batches.iter().map(|u| u.source_candidates).sum::<usize>(),
        3 * 7
    );
    let batches = probe(&mut connection, DerivedPhase::Messages, false, 0);
    assert_eq!(
        batches.iter().map(|u| u.source_candidates).sum::<usize>(),
        1 + 4 + 6
    );
}
