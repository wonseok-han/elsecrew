use super::*;
use crate::storage::Storage;
use hello_agent_protocol::storage_v1::{canonical_json_v1, LedgerEventV1, SQLITE_SCHEMA_VERSION};
use hello_agent_protocol::{ReadyPayloadV1, APP_SCHEMA_VERSION, PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

fn database() -> (TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("temporary directory");
    let path = root.path().join("hello-agent.sqlite3");
    (root, path)
}

fn event(
    id: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    payload: Value,
    timestamp: &str,
) -> LedgerEventV1 {
    sequenced_event(id, aggregate_type, aggregate_id, 1, payload, timestamp)
}

fn sequenced_event(
    id: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    sequence: u64,
    payload: Value,
    timestamp: &str,
) -> LedgerEventV1 {
    LedgerEventV1 {
        event_id: format!("evt_{id}"),
        operation_id: format!("op_{id}"),
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

fn workspace_manifest(
    project_id: &str,
    worktree_id: &str,
    workspace_identity: &str,
    lifecycle: &str,
) -> Value {
    json!({
        "schemaVersion":1,"projectId":project_id,"canonicalGitRoot":"/fixtures/repository",
        "worktreeId":worktree_id,"worktreePath":format!("/fixtures/workspaces/{worktree_id}"),
        "workspaceKind":"standaloneSnapshot","snapshotTransport":"gitBundle",
        "sourceMutationPolicy":"forbidden","objectFormat":"sha1",
        "baseCommit":"0123456789abcdef0123456789abcdef01234567",
        "baseTree":"89abcdef0123456789abcdef0123456789abcdef",
        "sourceIdentity":"source_repository_01","workspaceIdentity":workspace_identity,
        "headMode":"detached","lifecycle":lifecycle,
    })
}

fn prepared_workspace(
    preparation_id: &str,
    project_id: &str,
    worktree_id: &str,
    workspace_identity: &str,
    binding_state: &str,
    bound_chat_id: Option<&str>,
    timestamp: &str,
) -> Value {
    json!({
        "schemaVersion":1,"preparationId":preparation_id,
        "manifest":workspace_manifest(
            project_id,
            worktree_id,
            workspace_identity,
            if binding_state == "abandoned" { "abandoned" } else { "active" },
        ),
        "bindingState":binding_state,"boundChatId":bound_chat_id,
        "createdAt":timestamp,"updatedAt":timestamp,
    })
}

fn chat_record(state: &str, updated_at: &str) -> Value {
    json!({
        "schemaVersion":1,"chatId":"chat_lifetime","taskId":"task_lifetime",
        "projectId":"project_lifetime","worktreeId":"worktree_bound",
        "workspaceIdentity":"workspace_bound","goal":"public goal",
        "settingsRevision":1,"roleSnapshots":[
            {"roleId":"planning","runtime":"codex","modelSelector":"gpt-5.6-sol","reasoningEffort":"medium","permission":"readOnly"},
            {"roleId":"coding","runtime":"claude-code","modelSelector":"claude-opus-5","reasoningEffort":null,"permission":"workspaceWrite"},
            {"roleId":"review","runtime":"codex","modelSelector":"gpt-5.6-sol","reasoningEffort":"high","permission":"readOnly"}
        ],
        "state":state,"createdAt":"2026-08-28T05:00:01Z","updatedAt":updated_at,
    })
}

fn message(
    message_id: &str,
    sequence: u64,
    kind: &str,
    role_id: Option<&str>,
    run_id: Option<&str>,
    text: &str,
    created_at: &str,
) -> Value {
    json!({
        "schemaVersion":1,"messageId":message_id,"chatId":"chat_lifetime",
        "taskId":"task_lifetime","worktreeId":"worktree_bound",
        "eventSequence":sequence,"kind":kind,"roleId":role_id,"runId":run_id,
        "text":text,"createdAt":created_at,
    })
}

fn chat_projection(record: Value, last_message: Value, durable_sequence: u64) -> Value {
    json!({
        "chat_id":"chat_lifetime","project_id":"project_lifetime",
        "worktree_id":"worktree_bound","workspace_identity":"workspace_bound",
        "state":record["state"],"record":record,"last_message":last_message,
        "durable_event_sequence":durable_sequence,
    })
}

fn populate_lifetime_fixture(storage: &mut Storage) -> Vec<LedgerEventV1> {
    let project_initial = sequenced_event(
        "lifetime_project_1",
        "project",
        "project_lifetime",
        1,
        json!({
            "project_id":"project_lifetime","canonical_path":"/fixtures/repository",
            "prepared_workspaces":{},
        }),
        "2026-08-28T05:00:00Z",
    );
    let chat_first = sequenced_event(
        "lifetime_chat_1",
        "chat",
        "chat_lifetime",
        1,
        chat_projection(
            chat_record("draft", "2026-08-28T05:00:01Z"),
            message(
                "message_lifetime_1",
                1,
                "user",
                None,
                None,
                "public goal",
                "2026-08-28T05:00:01Z",
            ),
            1,
        ),
        "2026-08-28T05:00:01Z",
    );
    let chat_second = sequenced_event(
        "lifetime_chat_2",
        "chat",
        "chat_lifetime",
        2,
        chat_projection(
            chat_record("running", "2026-08-28T05:00:02Z"),
            message(
                "message_lifetime_2",
                2,
                "status",
                None,
                Some("run_actual"),
                "public progress only",
                "2026-08-28T05:00:02Z",
            ),
            2,
        ),
        "2026-08-28T05:00:02Z",
    );
    let project_prepared = sequenced_event(
        "lifetime_project_2",
        "project",
        "project_lifetime",
        2,
        json!({
            "project_id":"project_lifetime","canonical_path":"/fixtures/repository",
            "prepared_workspaces":{
                "preparation_bound":prepared_workspace(
                    "preparation_bound","project_lifetime","worktree_bound","workspace_bound",
                    "bound",Some("chat_lifetime"),"2026-08-28T05:00:01Z",
                ),
                "preparation_unbound":prepared_workspace(
                    "preparation_unbound","project_lifetime","worktree_unbound","workspace_unbound",
                    "unbound",None,"2026-08-28T05:00:02Z",
                ),
                "preparation_abandoned":prepared_workspace(
                    "preparation_abandoned","project_lifetime","worktree_abandoned","workspace_abandoned",
                    "abandoned",None,"2026-08-28T05:00:03Z",
                ),
            },
        }),
        "2026-08-28T05:00:03Z",
    );
    let legacy = event(
        "lifetime_legacy",
        "run",
        "run_legacy",
        json!({
            "run_id":"run_legacy","chat_id":"chat_lifetime","state":"recoveryRequired",
            "worktree_path":"/fixtures/workspaces/worktree_bound","source_kind":"fixture",
            "record":{"runId":"run_legacy"},
            "recovery":{"recoveryId":"recovery_legacy","state":"unresolved"},
        }),
        "2026-08-28T05:00:04Z",
    );
    let actual = event(
        "lifetime_actual",
        "run",
        "run_actual",
        json!({
            "run_id":"run_actual","chat_id":"chat_lifetime","state":"running",
            "worktree_path":"/fixtures/workspaces/worktree_bound","source_kind":"actualAgent",
            "record":{"runId":"run_actual"},"recovery":null,
        }),
        "2026-08-28T05:00:05Z",
    );
    let recovery = event(
        "lifetime_recovery_lock",
        "lock",
        "lock_recovery",
        json!({
            "lock_id":"lock_recovery","project_id":"project_lifetime",
            "writer_run_id":"run_legacy","worktree_path":"/fixtures/workspaces/worktree_bound",
            "poisoned":true,"recovery_required":true,
            "evidence":{"reason":"terminationUnknown"},
        }),
        "2026-08-28T05:00:06Z",
    );
    let events = vec![
        project_initial,
        chat_first,
        chat_second,
        project_prepared,
        legacy,
        actual,
        recovery,
    ];
    for event in &events {
        assert_eq!(
            storage.append_event(event).expect("append lifetime event"),
            crate::storage::AppendOutcome::Committed
        );
    }
    events
}

fn materialized_row_count(storage: &Storage) -> u64 {
    let count: i64 = storage
        .connection_for_test()
        .query_row(
            "SELECT (SELECT count(*) FROM ledger_baselines) + \
                    (SELECT count(*) FROM operation_receipts) + \
                    (SELECT count(*) FROM chat_messages) + \
                    (SELECT count(*) FROM prepared_workspaces);",
            [],
            |row| row.get(0),
        )
        .unwrap();
    count as u64
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
fn empty_materialization_is_atomic_ready_idempotent_and_reopens() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    let first = storage
        .materialize_lifetime_records(None)
        .expect("materialize empty storage");
    assert_eq!(first.cutoff_event_rowid, 0);
    assert_eq!(first.operation_receipts, 0);
    assert_eq!(first.chat_messages, 0);
    assert_eq!(first.prepared_workspaces, 0);
    assert_eq!(first.ledger_baselines, 0);
    assert_eq!(materialized_row_count(&storage), 0);
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "ready"
    );
    assert_eq!(
        storage
            .materialize_lifetime_records(None)
            .expect("idempotent empty rerun"),
        first
    );
    drop(storage);

    let mut reopened = Storage::open(&path).expect("ready storage reopens");
    assert_eq!(
        reopened
            .materialize_lifetime_records(None)
            .expect("ready storage reruns"),
        first
    );
}

#[test]
fn legacy_materialization_refuses_existing_v3_cursor_without_mutation() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    storage
        .connection_for_test()
        .execute(
            "INSERT INTO lifetime_materialization_job(\
               singleton,job_id,state,cutoff_event_rowid,phase,after_event_rowid,\
               failure_kind,created_at,updated_at,completed_at\
             ) VALUES (1,'bounded_cursor','pending',0,'receipts',0,NULL,\
               '2026-08-28T07:00:00Z','2026-08-28T07:00:00Z',NULL);",
            [],
        )
        .unwrap();
    let meta_before: String = storage
        .connection_for_test()
        .query_row(
            "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let job_before: String = storage
        .connection_for_test()
        .query_row(
            "SELECT json_object(\
               'singleton',singleton,'job_id',job_id,'state',state,\
               'cutoff_event_rowid',cutoff_event_rowid,'phase',phase,\
               'after_event_rowid',after_event_rowid,'failure_kind',failure_kind,\
               'created_at',created_at,'updated_at',updated_at,'completed_at',completed_at\
             ) FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let rows_before = materialized_row_count(&storage);
    let bytes_before = fs::read(&path).unwrap();

    let error = storage
        .materialize_lifetime_records(None)
        .expect_err("legacy materialization must not race the bounded cursor");
    assert_eq!(error.kind(), "storageMaterializationFailed");
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        meta_before
    );
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT json_object(\
                   'singleton',singleton,'job_id',job_id,'state',state,\
                   'cutoff_event_rowid',cutoff_event_rowid,'phase',phase,\
                   'after_event_rowid',after_event_rowid,'failure_kind',failure_kind,\
                   'created_at',created_at,'updated_at',updated_at,'completed_at',completed_at\
                 ) FROM lifetime_materialization_job WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        job_before
    );
    assert_eq!(materialized_row_count(&storage), rows_before);
    assert_eq!(fs::read(&path).unwrap(), bytes_before);
}

#[test]
fn populated_materialization_preserves_receipts_messages_workspaces_and_baseline_tail() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    let events = populate_lifetime_fixture(&mut storage);
    let events_before = crate::storage::load_events(storage.connection_for_test()).unwrap();
    let projections_before =
        crate::storage::load_projection(storage.connection_for_test()).unwrap();

    let first = storage
        .materialize_lifetime_records(None)
        .expect("materialize populated storage");
    assert_eq!(first.operation_receipts, events.len() as u64);
    assert_eq!(first.chat_messages, 2);
    assert_eq!(first.prepared_workspaces, 3);
    assert_eq!(first.ledger_baselines, 5);
    assert_eq!(
        crate::storage::load_events(storage.connection_for_test()).unwrap(),
        events_before
    );
    assert_eq!(
        crate::storage::load_projection(storage.connection_for_test()).unwrap(),
        projections_before
    );
    let receipt_namespaces: Vec<String> = storage
        .connection_for_test()
        .prepare(
            "SELECT json_extract(canonical_payload_json,'$.source_kind') \
             FROM operation_receipts WHERE aggregate_type='run' ORDER BY aggregate_id;",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        receipt_namespaces,
        vec!["actualAgent".to_string(), "fixture".to_string()]
    );
    let recovery_receipts: i64 = storage
        .connection_for_test()
        .query_row(
            "SELECT count(*) FROM operation_receipts \
             WHERE aggregate_type='lock' AND json_extract(canonical_payload_json,'$.recovery_required')=1;",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(recovery_receipts, 1);
    let messages: Vec<(i64, String)> = storage
        .connection_for_test()
        .prepare(
            "SELECT event_sequence,canonical_message_json FROM chat_messages \
             WHERE chat_id='chat_lifetime' ORDER BY event_sequence;",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        messages.iter().map(|entry| entry.0).collect::<Vec<_>>(),
        vec![1, 2]
    );
    for (_, canonical) in &messages {
        let message: Value = serde_json::from_str(canonical).unwrap();
        hello_agent_protocol::run_recovery_v1::validate_g5_payload_v1("teamMessage", &message)
            .unwrap();
        for forbidden in ["rawProvider", "rawReasoning", "token", "auth"] {
            assert!(!canonical.contains(forbidden));
        }
    }
    let workspace_states: Vec<String> = storage
        .connection_for_test()
        .prepare("SELECT binding_state FROM prepared_workspaces ORDER BY binding_state;")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(workspace_states, vec!["abandoned", "bound", "unbound"]);
    assert_eq!(
        storage
            .materialize_lifetime_records(None)
            .expect("idempotent populated rerun"),
        first
    );

    let third_message = message(
        "message_lifetime_3",
        3,
        "role",
        Some("planning"),
        Some("run_actual"),
        "public final",
        "2026-08-28T05:00:07Z",
    );
    let chat_tail = sequenced_event(
        "lifetime_chat_3",
        "chat",
        "chat_lifetime",
        3,
        chat_projection(
            chat_record("waitingForUser", "2026-08-28T05:00:07Z"),
            third_message,
            3,
        ),
        "2026-08-28T05:00:07Z",
    );
    assert_eq!(
        storage.append_event(&chat_tail).unwrap(),
        crate::storage::AppendOutcome::Committed
    );
    let mut project = storage
        .projection_record("project", "project_lifetime")
        .unwrap()
        .unwrap();
    project.remove("last_event_sequence");
    project["prepared_workspaces"]["preparation_unbound"]["bindingState"] =
        Value::String("abandoned".into());
    project["prepared_workspaces"]["preparation_unbound"]["manifest"]["lifecycle"] =
        Value::String("abandoned".into());
    project["prepared_workspaces"]["preparation_unbound"]["updatedAt"] =
        Value::String("2026-08-28T05:00:08Z".into());
    let workspace_tail = sequenced_event(
        "lifetime_project_3",
        "project",
        "project_lifetime",
        3,
        Value::Object(project),
        "2026-08-28T05:00:08Z",
    );
    assert_eq!(
        storage.append_event(&workspace_tail).unwrap(),
        crate::storage::AppendOutcome::Committed
    );
    let second = storage
        .materialize_lifetime_records(None)
        .expect("materialize new tails");
    assert_eq!(second.operation_receipts, first.operation_receipts + 2);
    assert_eq!(second.chat_messages, 3);
    assert_eq!(second.prepared_workspaces, 3);
    assert_eq!(second.ledger_baselines, first.ledger_baselines + 2);
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT binding_state FROM prepared_workspaces WHERE preparation_id='preparation_unbound';",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "abandoned"
    );
    assert_eq!(
        storage.append_event(&chat_tail).unwrap(),
        crate::storage::AppendOutcome::DuplicateNoOp
    );
    let mut conflict = chat_tail.clone();
    conflict.event_id.push_str("_different");
    assert_eq!(
        storage.append_event(&conflict).unwrap(),
        crate::storage::AppendOutcome::Conflict
    );
    drop(storage);

    let mut reopened = Storage::open(&path).expect("materialized ready storage reopens");
    assert_eq!(
        reopened
            .materialize_lifetime_records(None)
            .expect("reopened materialization reruns"),
        second
    );
}

#[test]
fn every_materialization_failure_point_rolls_back_rows_metadata_and_source_facts() {
    for failure in [
        MaterializationFailurePoint::Receipts,
        MaterializationFailurePoint::MessagesAndWorkspaces,
        MaterializationFailurePoint::BaselinesBeforeMetadata,
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("storage");
        populate_lifetime_fixture(&mut storage);
        let events_before = crate::storage::load_events(storage.connection_for_test()).unwrap();
        let projections_before =
            crate::storage::load_projection(storage.connection_for_test()).unwrap();

        let error = storage
            .materialize_lifetime_records(Some(failure))
            .expect_err("injected materialization must fail");
        assert_eq!(error.kind(), "injectedFailure");
        assert_eq!(materialized_row_count(&storage), 0, "{failure:?}");
        assert_eq!(
            storage
                .connection_for_test()
                .query_row(
                    "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "pending",
            "{failure:?}"
        );
        assert_eq!(
            crate::storage::load_events(storage.connection_for_test()).unwrap(),
            events_before,
            "{failure:?}"
        );
        assert_eq!(
            crate::storage::load_projection(storage.connection_for_test()).unwrap(),
            projections_before,
            "{failure:?}"
        );
    }
}

#[test]
fn materialization_rejects_corrupt_rows_without_overwriting_or_deleting() {
    let corruptions = [
        "UPDATE operation_receipts SET canonical_payload_digest=printf('%064d',0) WHERE operation_id='op_lifetime_actual';",
        "UPDATE chat_messages SET message_digest='sha256:corrupt' WHERE message_id='message_lifetime_1';",
        "UPDATE prepared_workspaces SET record_digest='sha256:corrupt' WHERE preparation_id='preparation_bound';",
        "UPDATE ledger_baselines SET projection_digest='sha256:corrupt' WHERE aggregate_type='chat';",
    ];
    for corruption in corruptions {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("storage");
        populate_lifetime_fixture(&mut storage);
        storage
            .materialize_lifetime_records(None)
            .expect("initial materialization");
        storage
            .connection_for_test()
            .execute_batch(corruption)
            .unwrap();
        let events_before = crate::storage::load_events(storage.connection_for_test()).unwrap();
        let row_count_before = materialized_row_count(&storage);

        let error = storage
            .materialize_lifetime_records(None)
            .expect_err("corrupt materialized row must fail closed");
        assert_eq!(error.kind(), "storageMaterializationFailed");
        assert_eq!(materialized_row_count(&storage), row_count_before);
        assert_eq!(
            crate::storage::load_events(storage.connection_for_test()).unwrap(),
            events_before
        );
        assert_eq!(
            storage
                .connection_for_test()
                .query_row(
                    "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "ready"
        );
    }
}

#[test]
fn persisted_materializing_or_failed_state_fails_closed_on_reopen() {
    for state in ["materializing", "failed"] {
        let (_root, path) = database();
        let storage = Storage::open(&path).expect("storage");
        storage
            .connection_for_test()
            .execute(
                "UPDATE storage_schema_meta SET materialization_state=?1 WHERE singleton=1;",
                [state],
            )
            .unwrap();
        drop(storage);
        let error = Storage::open(&path)
            .err()
            .expect("unsafe materialization restart must fail closed");
        assert_eq!(error.kind(), "storageSchemaMismatch", "{state}");
    }
}

#[test]
fn status_reports_database_facts_without_mutating_rows() {
    let (_root, path) = database();
    let storage = Storage::open(&path).expect("storage");
    let changes_before = storage.connection_for_test().total_changes();
    let before = storage.lifetime_status().expect("status before");
    let after = storage.lifetime_status().expect("status after");
    let expected_wal = fs::metadata(wal_path(&path))
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    assert_eq!(before.database.wal_bytes, expected_wal);
    assert_eq!(before.ledger_tail.event_count, 0);
    assert_eq!(before.snapshot_aggregates.count, 0);
    assert_eq!(after.ledger_tail, before.ledger_tail);
    assert_eq!(after.snapshot_aggregates, before.snapshot_aggregates);
    let page_size: i64 = storage
        .connection_for_test()
        .query_row("PRAGMA page_size;", [], |row| row.get(0))
        .unwrap();
    let page_count: i64 = storage
        .connection_for_test()
        .query_row("PRAGMA page_count;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        after.database.logical_bytes,
        (page_size * page_count) as u64
    );
    assert_eq!(
        after.database.physical_bytes,
        fs::metadata(&path).unwrap().len()
    );
    assert_eq!(
        after.pressure_facts.collection_status,
        StoragePressureCollectionStatusV1::Unavailable
    );
    assert_eq!(after.pressure_facts.volume_capacity_bytes, None);
    assert!(!after.physical_cleanup.enabled);
    assert_eq!(
        storage.connection_for_test().total_changes(),
        changes_before
    );
}

#[test]
fn absent_wal_is_reported_as_zero() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("absent.sqlite3-wal");
    assert_eq!(regular_file_len(&missing, true).unwrap(), 0);
}

#[test]
fn status_counts_utf8_ledger_and_projection_bytes_and_excludes_app_instances() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("storage");
    let payloads = [
        json!({"canonical_path":"/fixture/프로젝트","project_id":"project_status"}),
        json!({"chat_id":"chat_status","project_id":"project_status","title":"비밀 대화"}),
        json!({"chat_id":"chat_status","run_id":"run_status","state":"running","worktree_path":"/fixture/worktree"}),
        json!({"evidence":{"detail":"private provider text"},"lock_id":"lock_status","poisoned":false,"project_id":"project_status","recovery_required":false,"worktree_path":"/fixture/worktree","writer_run_id":"run_status"}),
    ];
    let aggregates = [
        ("project", "project_status"),
        ("chat", "chat_status"),
        ("run", "run_status"),
        ("lock", "lock_status"),
    ];
    let mut expected_bytes = 0_u64;
    for (index, ((aggregate_type, aggregate_id), payload)) in
        aggregates.into_iter().zip(payloads).enumerate()
    {
        expected_bytes += canonical_json_v1(payload.clone()).unwrap().len() as u64;
        storage
            .append_event(&event(
                &format!("status_{index}"),
                aggregate_type,
                aggregate_id,
                payload,
                &format!("2026-08-28T02:00:0{index}Z"),
            ))
            .unwrap();
    }
    storage
        .register_app_instance(&ready("status_sidecar"))
        .expect("app instance");

    let status = storage.lifetime_status().expect("status");
    let expected_ledger_bytes: i64 = storage
        .connection_for_test()
        .query_row(
            "SELECT sum(length(CAST(canonical_json_payload AS BLOB))) FROM events;",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status.ledger_tail.event_count, 5);
    assert_eq!(
        status.ledger_tail.payload_bytes,
        expected_ledger_bytes as u64
    );
    assert_eq!(status.snapshot_aggregates.count, 4);
    assert_eq!(status.snapshot_aggregates.payload_bytes, expected_bytes);
    assert_eq!(
        status.database.wal_bytes,
        fs::metadata(wal_path(&path)).expect("WAL exists").len()
    );
    assert!(status.database.wal_bytes > 0);
    let serialized = serde_json::to_string(&status).unwrap();
    for forbidden in [
        "비밀 대화",
        "private provider text",
        "/fixture/",
        "token",
        "auth",
    ] {
        assert!(!serialized.contains(forbidden), "leaked {forbidden}");
    }
}

#[test]
fn status_derives_completed_compaction_and_rejects_invalid_or_unsafe_facts() {
    let (_root, path) = database();
    let storage = Storage::open(&path).expect("storage");
    storage
        .connection_for_test()
        .execute(
            "INSERT INTO maintenance_jobs(job_id,job_kind,state,completed_at,last_successful_at,created_at,updated_at) VALUES ('job_ok','losslessCompaction','failed','2026-08-28T03:00:00Z','2026-08-28T02:00:00Z','2026-08-28T02:59:00Z','2026-08-28T03:00:00Z');",
            [],
        )
        .unwrap();
    let status = storage.lifetime_status().expect("failed status");
    assert_eq!(
        status.compaction.last_attempt_status,
        StorageCompactionAttemptStatusV1::Failed
    );
    assert_eq!(
        status.compaction.last_attempted_at.as_deref(),
        Some("2026-08-28T03:00:00Z")
    );
    assert_eq!(
        status.compaction.last_successful_at.as_deref(),
        Some("2026-08-28T02:00:00Z")
    );
    storage
        .connection_for_test()
        .execute(
            "UPDATE maintenance_jobs SET last_successful_at='2026-08-28T04:00:00Z' WHERE job_id='job_ok';",
            [],
        )
        .unwrap();
    let error = storage
        .lifetime_status()
        .expect_err("future success must fail");
    assert_eq!(error.kind(), "storageStatusInvalid");
    assert!(!error.detail().contains(path.to_string_lossy().as_ref()));

    storage
        .connection_for_test()
        .execute_batch("DELETE FROM maintenance_jobs;")
        .unwrap();
}

#[cfg(unix)]
#[test]
fn size_collection_rejects_a_symlink_without_leaking_the_target_path() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("private-token-provider-output.txt");
    let link = root.path().join("storage.sqlite3-wal");
    fs::write(&target, b"secret").unwrap();
    symlink(&target, &link).unwrap();
    let error = regular_file_len(&link, true).expect_err("WAL symlink rejected");
    assert_eq!(error.kind(), "storageStatusUnavailable");
    assert!(!error.detail().contains("private-token-provider-output"));
    assert!(!error.detail().contains(link.to_string_lossy().as_ref()));
}
