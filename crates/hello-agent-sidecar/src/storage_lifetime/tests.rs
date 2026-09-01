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

fn materialization_state(storage: &Storage) -> String {
    storage
        .connection_for_test()
        .query_row(
            "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .unwrap()
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

fn drive_bounded_v6(storage: &mut Storage) -> Vec<LifetimeMaterializationBatchResult> {
    let mut results = Vec::new();
    for _ in 0..256 {
        let mut cancellation = NeverCancel;
        let result = storage
            .materialize_lifetime_batch_for_test(&mut cancellation, None)
            .expect("bounded materialization");
        let completed = matches!(result, LifetimeMaterializationBatchResult::Completed { .. });
        results.push(result);
        if completed {
            return results;
        }
    }
    panic!("bounded materialization did not complete");
}

fn drive_until_cursor(storage: &mut Storage, phase: &str, scan_exhausted: i64) {
    for _ in 0..256 {
        let pair: Option<(String, i64)> = storage
            .connection_for_test()
            .query_row(
                "SELECT phase,scan_exhausted FROM lifetime_materialization_job WHERE singleton=1;",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .unwrap();
        if pair == Some((phase.to_string(), scan_exhausted)) {
            return;
        }
        storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("advance bounded cursor");
    }
    panic!("bounded cursor did not reach {phase}/{scan_exhausted}");
}

#[test]
fn bounded_v6_uses_private_empty_witness_and_separate_phase_advances() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    let first = storage
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("empty source-index batch");
    assert!(matches!(
        first,
        LifetimeMaterializationBatchResult::Progress {
            cursor: LifetimeMaterializationCursorFacts {
                phase: StorageLifetimePhaseV4::IndexSource,
                scan_exhausted: true,
                ..
            },
            source_rows: 0,
            ..
        }
    ));
    let second = storage
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("separate phase advance");
    assert!(matches!(
        second,
        LifetimeMaterializationBatchResult::Progress {
            cursor: LifetimeMaterializationCursorFacts {
                phase: StorageLifetimePhaseV4::VerifySourceIndex,
                after_scan_rowid: 0,
                scan_exhausted: false,
                ..
            },
            source_rows: 0,
            ..
        }
    ));
    let mut results = vec![first, second];
    results.extend(drive_bounded_v6(&mut storage));
    assert!(matches!(
        results.last(),
        Some(LifetimeMaterializationBatchResult::Completed {
            cutoff_event_rowid: 0
        })
    ));
    assert_eq!(results.len(), 32);
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

fn insert_raw_batch_event(storage: &Storage, id: &str, payload: &str) {
    storage
        .connection_for_test()
        .execute(
            "INSERT INTO events(event_id,operation_id,request_id,aggregate_type,aggregate_id,\
             aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp) \
             VALUES (?1,?2,NULL,'run',?3,1,'run.upserted',1,?4,'2026-08-28T09:00:00Z');",
            params![
                format!("evt_{id}"),
                format!("op_{id}"),
                format!("run_{id}"),
                payload
            ],
        )
        .unwrap();
}

fn insert_raw_event(storage: &Storage, event: &LedgerEventV1) {
    storage
        .connection_for_test()
        .execute(
            "INSERT INTO events(event_id,operation_id,request_id,aggregate_type,aggregate_id,\
             aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10);",
            params![
                event.event_id,
                event.operation_id,
                event.request_id,
                event.aggregate_type,
                event.aggregate_id,
                sql_i64(event.aggregate_sequence).unwrap(),
                event.event_type,
                event.payload_schema_version,
                event.canonical_json_payload,
                event.timestamp,
            ],
        )
        .unwrap();
}

#[test]
fn bounded_v6_indexes_long_single_aggregate_in_strict_bounded_batches() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v6 storage");
    for sequence in 1..=251_u64 {
        let source = sequenced_event(
            &format!("long_chat_{sequence}"),
            "chat",
            "chat_lifetime",
            sequence,
            chat_projection(
                chat_record("running", "2026-09-01T00:00:00Z"),
                message(
                    &format!("message_long_{sequence}"),
                    sequence,
                    "status",
                    None,
                    None,
                    "bounded",
                    "2026-09-01T00:00:00Z",
                ),
                sequence,
            ),
            "2026-09-01T00:00:00Z",
        );
        insert_raw_event(&storage, &source);
    }
    for expected in [100_i64, 200, 251] {
        let result = storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("source index batch");
        assert!(matches!(
            result,
            LifetimeMaterializationBatchResult::Progress {
                cursor: LifetimeMaterializationCursorFacts {
                    phase: StorageLifetimePhaseV4::IndexSource,
                    scan_exhausted: false,
                    ..
                },
                source_rows: 1..=100,
                source_payload_bytes: 1..=1_048_576,
            }
        ));
        let indexed: i64 = storage
            .connection_for_test()
            .query_row("SELECT count(*) FROM event_source_index;", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(indexed, expected);
    }
    let last: (i64, String) = storage
        .connection_for_test()
        .query_row(
            "SELECT aggregate_sequence,introduced_message_id FROM event_source_index \
             WHERE aggregate_type='chat' AND aggregate_id='chat_lifetime' \
             AND aggregate_sequence=251;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(last, (251, "message_long_251".to_string()));
}

#[test]
fn bounded_v6_index_source_rejects_chat_gap_without_raw_history_fallback() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v6 storage");
    let source = sequenced_event(
        "chat_gap",
        "chat",
        "chat_lifetime",
        2,
        chat_projection(
            chat_record("running", "2026-09-01T00:01:00Z"),
            message(
                "message_gap",
                1,
                "status",
                None,
                None,
                "gap",
                "2026-09-01T00:01:00Z",
            ),
            1,
        ),
        "2026-09-01T00:01:00Z",
    );
    insert_raw_event(&storage, &source);
    assert!(matches!(
        storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("gap is terminal fact"),
        LifetimeMaterializationBatchResult::Failed { failure_kind, .. }
            if failure_kind == "validationFailed"
    ));
    assert_eq!(
        storage
            .connection_for_test()
            .query_row("SELECT count(*) FROM event_source_index;", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn bounded_v6_materializes_only_indexed_message_introductions() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v6 storage");
    let project = event(
        "repeat_project",
        "project",
        "project_lifetime",
        json!({
            "project_id":"project_lifetime",
            "canonical_path":"/fixtures/repository",
            "prepared_workspaces":{},
        }),
        "2026-09-01T00:01:59Z",
    );
    assert_eq!(
        storage.append_event(&project).expect("append project"),
        crate::storage::AppendOutcome::Committed
    );
    let first_message = message(
        "message_once",
        1,
        "status",
        None,
        None,
        "once",
        "2026-09-01T00:02:00Z",
    );
    for (sequence, state) in [(1, "draft"), (2, "running")] {
        let source = sequenced_event(
            &format!("repeat_snapshot_{sequence}"),
            "chat",
            "chat_lifetime",
            sequence,
            chat_projection(
                chat_record(state, "2026-09-01T00:02:00Z"),
                first_message.clone(),
                1,
            ),
            "2026-09-01T00:02:00Z",
        );
        assert_eq!(
            storage.append_event(&source).expect("append chat snapshot"),
            crate::storage::AppendOutcome::Committed
        );
    }
    drive_until_cursor(&mut storage, "messages", 0);
    let message_result = storage
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("message batch");
    assert!(
        matches!(
            message_result,
            LifetimeMaterializationBatchResult::Progress {
                cursor: LifetimeMaterializationCursorFacts {
                    phase: StorageLifetimePhaseV4::Messages,
                    after_scan_rowid: 3,
                    ..
                },
                source_rows: 3,
                ..
            }
        ),
        "{message_result:?}"
    );
    let facts: (i64, Option<String>, Option<String>) = storage
        .connection_for_test()
        .query_row(
            "SELECT (SELECT count(*) FROM chat_messages),\
                    (SELECT introduced_message_id FROM event_source_index WHERE event_rowid=2),\
                    (SELECT introduced_message_id FROM event_source_index WHERE event_rowid=3);",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(facts, (1, Some("message_once".to_string()), None));
}

#[test]
fn bounded_v6_enforces_exact_mebibyte_and_single_source_oversize() {
    let (_root, exact_path) = database();
    let mut exact = Storage::open(&exact_path).expect("exact storage");
    let prefix = "{\"blob\":\"";
    let suffix = "\"}";
    let payload = format!(
        "{prefix}{}{suffix}",
        "x".repeat((1 << 20) - prefix.len() - suffix.len())
    );
    assert_eq!(payload.len(), 1 << 20);
    insert_raw_batch_event(&exact, "exact_mib", &payload);
    let result = exact
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("exact MiB accepted");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Progress {
            source_rows: 1,
            source_payload_bytes: 1_048_576,
            ..
        }
    ));

    let (_root, oversize_path) = database();
    let mut oversize = Storage::open(&oversize_path).expect("oversize storage");
    insert_raw_batch_event(&oversize, "oversize", &(payload + " "));
    let result = oversize
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("oversize becomes terminal fact");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Failed {
            failure_kind,
            ..
        } if failure_kind == "sourcePayloadOversize"
    ));
    let persisted: (String, String) = oversize
        .connection_for_test()
        .query_row(
            "SELECT state,failure_kind FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        persisted,
        ("failed".to_string(), "sourcePayloadOversize".to_string())
    );
}

#[test]
fn bounded_v6_aggregate_payload_boundary_stops_before_crossing_limit() {
    let rows = bounded_derived_rows(vec![
        (1, "first".to_string(), 1_048_575),
        (2, "second".to_string(), 1),
    ])
    .expect("exact aggregate boundary");
    assert_eq!(rows.len(), 2);
    let rows = bounded_derived_rows(vec![
        (1, "first".to_string(), 1_048_575),
        (2, "second".to_string(), 2),
    ])
    .expect("bounded aggregate");
    assert_eq!(rows.len(), 1);
}

#[test]
fn bounded_v6_batches_one_hundred_rows_and_resumes_across_reopen() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    for index in 0..251_u64 {
        let event = sequenced_event(
            &format!("bounded_{index}"),
            "run",
            &format!("run_bounded_{index}"),
            1,
            json!({
                "run_id":format!("run_bounded_{index}"),
                "chat_id":null,
                "state":"running",
                "worktree_path":null,
                "source_kind":"fixture",
                "record":{"runId":format!("run_bounded_{index}")},
                "recovery":null,
            }),
            "2026-08-28T08:00:00Z",
        );
        assert_eq!(
            storage.append_event(&event).expect("append"),
            crate::storage::AppendOutcome::Committed
        );
    }
    let mut cancellation = NeverCancel;
    let first = storage
        .materialize_lifetime_batch_for_test(&mut cancellation, None)
        .expect("first batch");
    assert!(matches!(
        first,
        LifetimeMaterializationBatchResult::Progress {
            source_rows: 100,
            cursor: LifetimeMaterializationCursorFacts {
                after_scan_rowid: 100,
                scan_exhausted: false,
                ..
            },
            ..
        }
    ));
    drop(storage);
    let mut reopened = Storage::open(&path).expect("pending cursor reopens");
    let second = reopened
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("second batch");
    assert!(matches!(
        second,
        LifetimeMaterializationBatchResult::Progress {
            source_rows: 100,
            cursor: LifetimeMaterializationCursorFacts {
                after_scan_rowid: 200,
                ..
            },
            ..
        }
    ));
    drive_until_cursor(&mut reopened, "verifyRuns", 0);
    let mut cancel_projection = |point| point == MaterializationCancellationPoint::AfterSourceRow;
    let suspended = reopened
        .materialize_lifetime_batch_for_test(&mut cancel_projection, None)
        .expect("cancel bounded projection batch");
    assert!(matches!(
        suspended,
        LifetimeMaterializationBatchResult::Suspended {
            cursor: Some(LifetimeMaterializationCursorFacts {
                phase: StorageLifetimePhaseV4::VerifyRuns,
                after_scan_rowid: 0,
                scan_exhausted: false,
                ..
            })
        }
    ));
    let run_batch = reopened
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("bounded projection run batch");
    assert!(matches!(
        run_batch,
        LifetimeMaterializationBatchResult::Progress {
            source_rows: 100,
            source_payload_bytes: 1..=1_048_576,
            cursor: LifetimeMaterializationCursorFacts {
                phase: StorageLifetimePhaseV4::VerifyRuns,
                scan_exhausted: false,
                ..
            },
        }
    ));
    drive_until_cursor(&mut reopened, "verifyReceipts", 0);
    let receipt_batch = reopened
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("bounded derived receipt batch");
    assert!(matches!(
        receipt_batch,
        LifetimeMaterializationBatchResult::Progress {
            source_rows: 100,
            source_payload_bytes: 1..=1_048_576,
            cursor: LifetimeMaterializationCursorFacts {
                phase: StorageLifetimePhaseV4::VerifyReceipts,
                scan_exhausted: false,
                ..
            },
        }
    ));
    drive_until_cursor(&mut reopened, "verifyBaselines", 0);
    let baseline_batch = reopened
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("bounded derived baseline batch");
    assert!(matches!(
        baseline_batch,
        LifetimeMaterializationBatchResult::Progress {
            source_rows: 100,
            source_payload_bytes: 1..=1_048_576,
            cursor: LifetimeMaterializationCursorFacts {
                phase: StorageLifetimePhaseV4::VerifyBaselines,
                scan_exhausted: false,
                ..
            },
        }
    ));
}

#[test]
fn bounded_v6_projection_phases_detect_indexed_drift_and_source_less_rows() {
    for (phase, mutation) in [
        (
            "verifyProjects",
            "UPDATE projects SET canonical_path='/drift' WHERE project_id='project_lifetime';",
        ),
        (
            "verifyChats",
            "UPDATE chats SET project_id='project_drift' WHERE chat_id='chat_lifetime';",
        ),
        (
            "verifyRuns",
            "UPDATE runs SET state='completed' WHERE run_id='run_legacy';",
        ),
        (
            "verifyLocks",
            "UPDATE locks SET poisoned=0 WHERE lock_id='lock_recovery';",
        ),
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("v5 storage");
        populate_lifetime_fixture(&mut storage);
        drive_until_cursor(&mut storage, phase, 0);
        storage
            .connection_for_test()
            .execute_batch("PRAGMA foreign_keys=OFF;")
            .unwrap();
        storage
            .connection_for_test()
            .execute_batch(mutation)
            .unwrap();
        assert!(matches!(
            storage
                .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
                .expect("projection validation"),
            LifetimeMaterializationBatchResult::Failed {
                failure_kind,
                ..
            } if failure_kind == "validationFailed"
        ));
    }

    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v5 storage");
    drive_until_cursor(&mut storage, "verifyAppInstances", 0);
    storage
        .connection_for_test()
        .execute(
            "INSERT INTO app_instances(sidecar_instance_id,pid,platform,started_at,heartbeat_at,\
             clean_shutdown_at,last_event_sequence) VALUES ('source_less_app',1,'darwin',\
             '2026-08-28T11:00:00Z','2026-08-28T11:00:00Z',NULL,1);",
            [],
        )
        .unwrap();
    assert!(matches!(
        storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("source-less app instance"),
        LifetimeMaterializationBatchResult::Failed { .. }
    ));
}

#[test]
fn bounded_v6_verify_source_detects_missing_projection() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v5 storage");
    populate_lifetime_fixture(&mut storage);
    drive_until_cursor(&mut storage, "verifySource", 0);
    storage
        .connection_for_test()
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    storage
        .connection_for_test()
        .execute(
            "DELETE FROM projects WHERE project_id='project_lifetime';",
            [],
        )
        .unwrap();
    assert!(matches!(
        storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("missing projection"),
        LifetimeMaterializationBatchResult::Failed { .. }
    ));
}

#[test]
fn bounded_v6_app_instances_verify_without_creating_baselines() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v5 storage");
    storage
        .register_app_instance(&ready("bounded_app"))
        .expect("register app instance");
    drive_bounded_v6(&mut storage);
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT count(*) FROM ledger_baselines WHERE aggregate_type='app_instance';",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn bounded_v6_verification_fences_reset_new_and_updated_aggregates() {
    for phase in [
        "verifySourceIndex",
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
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("v5 storage");
        let first = sequenced_event(
            "fence_existing_1",
            "run",
            "run_fence_existing",
            1,
            json!({
                "run_id":"run_fence_existing","chat_id":null,"state":"running",
                "worktree_path":null,"source_kind":"fixture",
                "record":{"runId":"run_fence_existing"},"recovery":null,
            }),
            "2026-08-28T11:01:00Z",
        );
        storage.append_event(&first).expect("initial aggregate");
        drive_until_cursor(&mut storage, phase, 0);
        let update = sequenced_event(
            "fence_existing_2",
            "run",
            "run_fence_existing",
            2,
            json!({
                "run_id":"run_fence_existing","chat_id":null,"state":"completed",
                "worktree_path":null,"source_kind":"fixture",
                "record":{"runId":"run_fence_existing"},"recovery":null,
            }),
            "2026-08-28T11:01:01Z",
        );
        let new = event(
            "fence_new",
            "run",
            "run_fence_new",
            json!({
                "run_id":"run_fence_new","chat_id":null,"state":"running",
                "worktree_path":null,"source_kind":"fixture",
                "record":{"runId":"run_fence_new"},"recovery":null,
            }),
            "2026-08-28T11:01:02Z",
        );
        storage.append_event(&update).expect("update aggregate");
        storage.append_event(&new).expect("new aggregate");
        let reset = storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("verification fence reset");
        assert!(
            matches!(
                reset,
                LifetimeMaterializationBatchResult::Progress {
                    cursor: LifetimeMaterializationCursorFacts {
                        phase: StorageLifetimePhaseV4::IndexSource,
                        after_scan_rowid: 0,
                        scan_exhausted: false,
                        ..
                    },
                    ..
                }
            ),
            "phase {phase}"
        );
        drive_bounded_v6(&mut storage);
    }
}

#[test]
fn bounded_v6_sql_is_fixed_limit_and_forbidden_scan_free() {
    let source = include_str!("../storage_lifetime.rs");
    let bounded = source
        .split("pub(crate) fn materialize_lifetime_records")
        .next()
        .expect("bounded worker source");
    for forbidden in [
        "LIMIT 101",
        " OFFSET ",
        "COUNT(",
        "count(",
        "GROUP BY",
        " OVER ",
        "NOT EXISTS",
        "WITH ",
        "json_each(",
        "json_extract(",
        "SCAN events",
        "SCAN event_source_index",
        "USE TEMP B-TREE",
    ] {
        assert!(
            !bounded.contains(forbidden),
            "bounded worker contains forbidden SQL token {forbidden}"
        );
    }
    assert!(bounded.matches("LIMIT 100").count() >= 2);
}

#[test]
fn bounded_v6_query_plans_use_only_indexed_source_shapes() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v6 storage");
    for index in 0..251_u64 {
        let source = event(
            &format!("plan_{index}"),
            "run",
            &format!("run_plan_{index}"),
            json!({
                "run_id":format!("run_plan_{index}"),"chat_id":null,"state":"running",
                "worktree_path":null,"source_kind":"fixture",
                "record":{"runId":format!("run_plan_{index}")},"recovery":null,
            }),
            "2026-09-01T00:03:00Z",
        );
        storage.append_event(&source).expect("append plan source");
    }
    let queries = [
        (
            "EXPLAIN QUERY PLAN SELECT rowid,event_id,operation_id,request_id,aggregate_type,\
             aggregate_id,aggregate_sequence,event_type,payload_schema_version,\
             canonical_json_payload,timestamp FROM events WHERE rowid>0 AND rowid<=20 \
             ORDER BY rowid LIMIT 100;",
            "SEARCH events USING INTEGER PRIMARY KEY",
        ),
        (
            "EXPLAIN QUERY PLAN SELECT event_rowid,event_id,operation_id,aggregate_type,aggregate_id,\
             aggregate_sequence,introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE event_rowid>0 AND event_rowid<=20 \
             ORDER BY event_rowid LIMIT 100;",
            "SEARCH event_source_index USING INTEGER PRIMARY KEY",
        ),
        (
            "EXPLAIN QUERY PLAN SELECT event_rowid FROM event_source_index \
             WHERE aggregate_type='project' AND aggregate_id='project_1' AND event_rowid<=20 \
             ORDER BY event_rowid DESC LIMIT 1;",
            "event_source_index_aggregate_latest",
        ),
        (
            "EXPLAIN QUERY PLAN SELECT event_rowid FROM event_source_index \
             WHERE introduced_message_id='message_1';",
            "event_source_index_message_unique",
        ),
        (
            "EXPLAIN QUERY PLAN SELECT event_rowid,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE aggregate_type='chat' AND aggregate_id='chat_1' \
             AND aggregate_sequence=2;",
            "sqlite_autoindex_event_source_index_3",
        ),
        (
            "EXPLAIN QUERY PLAN SELECT canonical_json_payload FROM events WHERE rowid=1;",
            "SEARCH events USING INTEGER PRIMARY KEY",
        ),
    ];
    for (sql, expected) in queries {
        let mut statement = storage.connection_for_test().prepare(sql).unwrap();
        let details = statement
            .query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            details.iter().any(|detail| detail.contains(expected)),
            "{details:?}"
        );
        assert!(details.iter().all(|detail| {
            !detail.contains("SCAN events")
                && !detail.contains("SCAN event_source_index")
                && !detail.contains("USE TEMP B-TREE")
        }));
    }

    let statements = [
        (
            "SELECT rowid FROM events WHERE rowid>0 AND rowid<=251 ORDER BY rowid LIMIT 100;",
            100,
        ),
        (
            "SELECT event_rowid FROM event_source_index WHERE event_rowid>0 AND event_rowid<=251 \
             ORDER BY event_rowid LIMIT 100;",
            100,
        ),
        (
            "SELECT event_rowid FROM event_source_index WHERE aggregate_type='run' \
             AND aggregate_id='run_plan_250' AND event_rowid<=251 \
             ORDER BY event_rowid DESC LIMIT 1;",
            1,
        ),
        (
            "SELECT event_rowid FROM event_source_index WHERE introduced_message_id='absent';",
            0,
        ),
        (
            "SELECT event_rowid FROM event_source_index WHERE aggregate_type='run' \
             AND aggregate_id='run_plan_250' AND aggregate_sequence=1;",
            1,
        ),
        (
            "SELECT canonical_json_payload FROM events WHERE rowid=251;",
            1,
        ),
    ];
    for (sql, expected_rows) in statements {
        let mut statement = storage.connection_for_test().prepare(sql).unwrap();
        let actual_rows = {
            let mut rows = statement.query([]).unwrap();
            let mut actual_rows = 0;
            while rows.next().unwrap().is_some() {
                actual_rows += 1;
            }
            actual_rows
        };
        assert_eq!(actual_rows, expected_rows);
        assert_eq!(
            statement.get_status(rusqlite::StatementStatus::FullscanStep),
            0
        );
        assert_eq!(statement.get_status(rusqlite::StatementStatus::Sort), 0);
        assert_eq!(
            statement.get_status(rusqlite::StatementStatus::AutoIndex),
            0
        );
    }
}

#[test]
fn bounded_v6_verify_source_index_rejects_source_less_and_mirrored_drift() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v6 storage");
    populate_lifetime_fixture(&mut storage);
    drive_until_cursor(&mut storage, "verifySourceIndex", 0);
    storage
        .connection_for_test()
        .execute_batch(
            "PRAGMA foreign_keys=OFF;\
             DROP TRIGGER event_source_index_append_only_update;\
             UPDATE event_source_index SET operation_id='op_drift' WHERE event_rowid=1;",
        )
        .unwrap();
    assert!(matches!(
        storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("mirrored drift result"),
        LifetimeMaterializationBatchResult::Failed { failure_kind, .. }
            if failure_kind == "validationFailed"
    ));

    let (_root, source_less_path) = database();
    let mut source_less = Storage::open(&source_less_path).expect("v6 storage");
    populate_lifetime_fixture(&mut source_less);
    drive_until_cursor(&mut source_less, "verifySourceIndex", 0);
    source_less
        .connection_for_test()
        .execute_batch(
            "PRAGMA foreign_keys=OFF;\
             DROP TRIGGER event_source_index_append_only_delete;\
             DROP TRIGGER events_are_append_only_delete;\
             DELETE FROM events WHERE rowid=1;",
        )
        .unwrap();
    assert!(matches!(
        source_less
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("source-less index result"),
        LifetimeMaterializationBatchResult::Failed { failure_kind, .. }
            if failure_kind == "validationFailed"
    ));
}

#[test]
fn bounded_v6_populated_fixture_preserves_d1b_artifacts() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    let events = populate_lifetime_fixture(&mut storage);
    let results = drive_bounded_v6(&mut storage);
    assert!(matches!(
        results.last(),
        Some(LifetimeMaterializationBatchResult::Completed {
            cutoff_event_rowid: 7
        })
    ));
    let counts: (i64, i64, i64, i64) = storage
        .connection_for_test()
        .query_row(
            "SELECT (SELECT count(*) FROM operation_receipts),\
                    (SELECT count(*) FROM chat_messages),\
                    (SELECT count(*) FROM prepared_workspaces),\
                    (SELECT count(*) FROM ledger_baselines);",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(counts, (events.len() as i64, 2, 3, 5));
}

#[test]
fn bounded_v6_cancellation_rolls_back_current_batch_and_restores_pending() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    let event = event(
        "cancel",
        "run",
        "run_cancel",
        json!({
            "run_id":"run_cancel","chat_id":null,"state":"running",
            "worktree_path":null,"source_kind":"fixture",
            "record":{"runId":"run_cancel"},"recovery":null,
        }),
        "2026-08-28T08:01:00Z",
    );
    storage.append_event(&event).expect("append");
    let mut before_row = |point| point == MaterializationCancellationPoint::BeforeSourceRow;
    let before_row_result = storage
        .materialize_lifetime_batch_for_test(&mut before_row, None)
        .expect("cancel before row");
    assert!(matches!(
        before_row_result,
        LifetimeMaterializationBatchResult::Suspended {
            cursor: Some(LifetimeMaterializationCursorFacts {
                after_scan_rowid: 0,
                ..
            })
        }
    ));
    let mut cancellation = |point| point == MaterializationCancellationPoint::AfterSourceRow;
    let result = storage
        .materialize_lifetime_batch_for_test(&mut cancellation, None)
        .expect("cancelled batch");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Suspended { .. }
    ));
    let facts: (String, i64, i64) = storage
        .connection_for_test()
        .query_row(
            "SELECT state,after_scan_rowid,(SELECT count(*) FROM operation_receipts) \
             FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(facts, ("pending".to_string(), 0, 0));

    let (_root, precommit_path) = database();
    let mut precommit = Storage::open(&precommit_path).expect("v4 storage");
    precommit.append_event(&event).expect("append");
    let mut cancellation = |point| point == MaterializationCancellationPoint::ImmediatelyPrecommit;
    let result = precommit
        .materialize_lifetime_batch_for_test(&mut cancellation, None)
        .expect("precommit cancellation");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Suspended {
            cursor: Some(LifetimeMaterializationCursorFacts {
                after_scan_rowid: 0,
                scan_exhausted: false,
                ..
            })
        }
    ));
    assert_eq!(
        precommit
            .connection_for_test()
            .query_row("SELECT count(*) FROM operation_receipts;", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn bounded_v6_cancellation_before_begin_has_zero_mutation() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    let mut cancellation = |point| point == MaterializationCancellationPoint::BeforeBegin;
    let result = storage
        .materialize_lifetime_batch_for_test(&mut cancellation, None)
        .expect("cancel before begin");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Suspended { cursor: None }
    ));
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT count(*) FROM lifetime_materialization_job;",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn bounded_v6_unconfirmed_batch_commit_leaves_attempting_and_reopen_fails_closed() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    let result = storage.materialize_lifetime_batch_for_test(
        &mut NeverCancel,
        Some(BatchMaterializationFailurePoint::BatchCommitUnavailable),
    );
    assert_eq!(
        result.expect_err("commit unavailable").kind(),
        "storageMaterializationFailed"
    );
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT state FROM lifetime_materialization_job WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "attempting"
    );
    drop(storage);
    let error = match Storage::open(&path) {
        Ok(_) => panic!("attempting reopen must fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), "storageSchemaMismatch");
}

#[test]
fn bounded_v6_after_arm_and_validation_failures_are_terminally_distinct() {
    let (_root, arm_path) = database();
    let mut arm = Storage::open(&arm_path).expect("v4 storage");
    assert_eq!(
        arm.materialize_lifetime_batch_for_test(
            &mut NeverCancel,
            Some(BatchMaterializationFailurePoint::AfterArmCommit),
        )
        .expect_err("after-arm loss")
        .kind(),
        "storageMaterializationFailed"
    );
    assert_eq!(
        arm.connection_for_test()
            .query_row(
                "SELECT state FROM lifetime_materialization_job WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "attempting"
    );

    let (_root, validation_path) = database();
    let mut validation = Storage::open(&validation_path).expect("v4 storage");
    insert_raw_batch_event(&validation, "validation", "{}");
    let result = validation
        .materialize_lifetime_batch_for_test(
            &mut NeverCancel,
            Some(BatchMaterializationFailurePoint::AfterDerivedRows),
        )
        .expect("terminal validation fact");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Failed {
            failure_kind,
            cursor: LifetimeMaterializationCursorFacts {
                after_scan_rowid: 0,
                ..
            },
        } if failure_kind == "injectedFailure"
    ));
    assert_eq!(
        validation
            .connection_for_test()
            .query_row("SELECT count(*) FROM operation_receipts;", [], |row| row
                .get::<_, i64>(0),)
            .unwrap(),
        0
    );
}

#[test]
fn bounded_v6_failure_record_commit_loss_leaves_attempting() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    let oversized = format!("{{\"blob\":\"{}\"}}", "x".repeat(1 << 20));
    insert_raw_batch_event(&storage, "failure_record", &oversized);
    let result = storage.materialize_lifetime_batch_for_test(
        &mut NeverCancel,
        Some(BatchMaterializationFailurePoint::FailureRecordCommitUnavailable),
    );
    assert_eq!(
        result
            .expect_err("failure record commit unavailable")
            .kind(),
        "storageMaterializationFailed"
    );
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT state FROM lifetime_materialization_job WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "attempting"
    );
}

#[test]
fn bounded_v6_after_verify_commit_suspends_before_separate_final_publish() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    drive_until_cursor(&mut storage, "verifyBaselines", 0);
    let mut cancel_after_commit = |point| point == MaterializationCancellationPoint::AfterCommit;
    let result = storage
        .materialize_lifetime_batch_for_test(&mut cancel_after_commit, None)
        .expect("suspend after verify commit");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Suspended {
            cursor: Some(LifetimeMaterializationCursorFacts {
                phase: StorageLifetimePhaseV4::VerifyBaselines,
                scan_exhausted: true,
                ..
            })
        }
    ));
    assert_eq!(materialization_state(&storage), "pending");
    let completed = storage
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("separate final");
    assert!(matches!(
        completed,
        LifetimeMaterializationBatchResult::Completed { .. }
    ));
}

#[test]
fn bounded_v6_final_precommit_cancellation_rolls_back_publication() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    drive_until_cursor(&mut storage, "verifyBaselines", 1);
    let mut cancel_before_begin =
        |point| point == MaterializationCancellationPoint::FinalBeforeBegin;
    let result = storage
        .materialize_lifetime_batch_for_test(&mut cancel_before_begin, None)
        .expect("cancel final before begin");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Suspended { .. }
    ));
    assert_eq!(materialization_state(&storage), "pending");
    let mut cancel_before_mutation =
        |point| point == MaterializationCancellationPoint::FinalBeforeMutation;
    let result = storage
        .materialize_lifetime_batch_for_test(&mut cancel_before_mutation, None)
        .expect("cancel final before mutation");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Suspended { .. }
    ));
    assert_eq!(materialization_state(&storage), "pending");
    let mut cancel_precommit =
        |point| point == MaterializationCancellationPoint::FinalImmediatelyPrecommit;
    let result = storage
        .materialize_lifetime_batch_for_test(&mut cancel_precommit, None)
        .expect("cancel final precommit");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Suspended { .. }
    ));
    assert_eq!(materialization_state(&storage), "pending");
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT state FROM lifetime_materialization_job WHERE singleton=1;",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "pending"
    );
}

#[test]
fn bounded_v6_derived_verification_rejects_source_less_rows() {
    for phase in [
        "verifyReceipts",
        "verifyMessages",
        "verifyWorkspaces",
        "verifyBaselines",
    ] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("v4 storage");
        populate_lifetime_fixture(&mut storage);
        drive_until_cursor(&mut storage, phase, 0);
        let connection = storage.connection_for_test();
        match phase {
            "verifyReceipts" => {
                connection
                    .execute(
                        "INSERT INTO operation_receipts(operation_id,event_id,aggregate_type,aggregate_id,\
                         aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,\
                         canonical_payload_digest_algorithm,canonical_payload_digest_version,\
                         canonical_payload_digest_format,canonical_payload_digest,event_timestamp,materialized_at) \
                         VALUES ('op_extra','evt_extra','run','run_extra',1,'run.upserted',1,'{}',\
                         'sha256',1,'lowercaseHex64',?1,'2026-08-28T10:00:00Z','2026-08-28T10:00:00Z');",
                        [digest_hex_unchecked(b"{}")],
                    )
                    .unwrap();
            }
            "verifyMessages" => {
                let value = message(
                    "message_extra",
                    1,
                    "status",
                    None,
                    None,
                    "extra",
                    "2026-08-28T10:00:00Z",
                );
                let canonical = canonical_json_v1(value).unwrap();
                connection
                    .execute(
                        "INSERT INTO chat_messages(message_id,chat_id,event_sequence,\
                         canonical_message_json,message_digest,created_at,materialized_at) \
                         VALUES ('message_extra','chat_lifetime',99,?1,?2,\
                         '2026-08-28T10:00:00Z','2026-08-28T10:00:00Z');",
                        params![canonical, sha256_file_version_v1(canonical.as_bytes())],
                    )
                    .unwrap();
            }
            "verifyWorkspaces" => {
                let value = prepared_workspace(
                    "preparation_extra",
                    "project_lifetime",
                    "worktree_extra",
                    "workspace_extra",
                    "unbound",
                    None,
                    "2026-08-28T10:00:00Z",
                );
                let canonical = canonical_json_v1(value).unwrap();
                connection
                    .execute(
                        "INSERT INTO prepared_workspaces(preparation_id,project_id,worktree_id,\
                         workspace_identity,binding_state,bound_chat_id,canonical_record_json,\
                         record_digest,created_at,updated_at,materialized_at) VALUES (\
                         'preparation_extra','project_lifetime','worktree_extra','workspace_extra',\
                         'unbound',NULL,?1,?2,'2026-08-28T10:00:00Z',\
                         '2026-08-28T10:00:00Z','2026-08-28T10:00:00Z');",
                        params![canonical, sha256_file_version_v1(canonical.as_bytes())],
                    )
                    .unwrap();
            }
            "verifyBaselines" => {
                connection
                    .execute(
                        "INSERT INTO ledger_baselines(baseline_id,aggregate_type,aggregate_id,\
                         cutoff_aggregate_sequence,payload_schema_version,canonical_projection_json,\
                         projection_digest,created_at,verified_at) VALUES (\
                         'baseline_extra','run','run_extra',1,1,'{}',?1,\
                         '2026-08-28T10:00:00Z','2026-08-28T10:00:00Z');",
                        [sha256_file_version_v1(b"{}")],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let result = storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("derived validation result");
        assert!(
            matches!(
                result,
                LifetimeMaterializationBatchResult::Failed {
                    failure_kind,
                    ..
                } if failure_kind == "validationFailed"
            ),
            "phase {phase}"
        );
    }
}

#[test]
fn bounded_v6_accepts_source_backed_historical_baseline_but_requires_newest() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    let events = populate_lifetime_fixture(&mut storage);
    drive_until_cursor(&mut storage, "verifyBaselines", 0);
    let historical = events
        .iter()
        .find(|event| event.aggregate_type == "project" && event.aggregate_sequence == 1)
        .unwrap();
    let historical_id = baseline_id("project", "project_lifetime", 1);
    storage
        .connection_for_test()
        .execute(
            "INSERT INTO ledger_baselines(baseline_id,aggregate_type,aggregate_id,\
             cutoff_aggregate_sequence,payload_schema_version,canonical_projection_json,\
             projection_digest,created_at,verified_at) VALUES (?1,'project','project_lifetime',\
             1,1,?2,?3,'2026-08-28T10:01:00Z','2026-08-28T10:01:00Z');",
            params![
                historical_id,
                historical.canonical_json_payload,
                sha256_file_version_v1(historical.canonical_json_payload.as_bytes())
            ],
        )
        .unwrap();
    assert!(matches!(
        storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("historical baseline verifies"),
        LifetimeMaterializationBatchResult::Progress { .. }
    ));

    let (_root, missing_path) = database();
    let mut missing = Storage::open(&missing_path).expect("v4 storage");
    populate_lifetime_fixture(&mut missing);
    drive_until_cursor(&mut missing, "verifySource", 0);
    let newest_id = baseline_id("project", "project_lifetime", 2);
    missing
        .connection_for_test()
        .execute(
            "DELETE FROM ledger_baselines WHERE baseline_id=?1;",
            [newest_id],
        )
        .unwrap();
    let result = missing
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("missing newest baseline result");
    assert!(matches!(
        result,
        LifetimeMaterializationBatchResult::Failed { .. }
    ));
}

#[test]
fn bounded_v6_append_during_frozen_job_resets_and_catches_up() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    populate_lifetime_fixture(&mut storage);
    drive_until_cursor(&mut storage, "verifyBaselines", 1);
    let frozen: i64 = storage
        .connection_for_test()
        .query_row(
            "SELECT cutoff_event_rowid FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let appended = event(
        "catch_up",
        "run",
        "run_catch_up",
        json!({
            "run_id":"run_catch_up","chat_id":null,"state":"running",
            "worktree_path":null,"source_kind":"fixture",
            "record":{"runId":"run_catch_up"},"recovery":null,
        }),
        "2026-08-28T10:02:00Z",
    );
    storage
        .append_event(&appended)
        .expect("append during frozen job");
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT cutoff_event_rowid FROM lifetime_materialization_job WHERE singleton=1;",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        frozen
    );
    let reset = storage
        .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
        .expect("catch-up reset");
    assert!(matches!(
        reset,
        LifetimeMaterializationBatchResult::Progress {
            cursor: LifetimeMaterializationCursorFacts {
                phase: StorageLifetimePhaseV4::IndexSource,
                after_scan_rowid: 0,
                scan_exhausted: false,
                ..
            },
            ..
        }
    ));
    drive_bounded_v6(&mut storage);
    assert!(
        load_receipt(storage.connection_for_test(), &appended.operation_id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn bounded_v6_materialization_phases_keep_the_frozen_cutoff_until_verification() {
    for phase in ["receipts", "messages", "workspaces", "baselines"] {
        let (_root, path) = database();
        let mut storage = Storage::open(&path).expect("v6 storage");
        let initial = event(
            &format!("frozen_initial_{phase}"),
            "run",
            &format!("run_frozen_initial_{phase}"),
            json!({
                "run_id":format!("run_frozen_initial_{phase}"),"chat_id":null,
                "state":"running","worktree_path":null,"source_kind":"fixture",
                "record":{"runId":format!("run_frozen_initial_{phase}")},"recovery":null,
            }),
            "2026-09-01T00:04:00Z",
        );
        storage.append_event(&initial).expect("initial append");
        drive_until_cursor(&mut storage, phase, 0);
        let appended = event(
            &format!("frozen_appended_{phase}"),
            "run",
            &format!("run_frozen_appended_{phase}"),
            json!({
                "run_id":format!("run_frozen_appended_{phase}"),"chat_id":null,
                "state":"running","worktree_path":null,"source_kind":"fixture",
                "record":{"runId":format!("run_frozen_appended_{phase}")},"recovery":null,
            }),
            "2026-09-01T00:04:01Z",
        );
        storage
            .append_event(&appended)
            .expect("append after frozen phase");
        let phase_result = storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("frozen materialization batch");
        assert!(
            matches!(
                phase_result,
                LifetimeMaterializationBatchResult::Progress {
                    cursor: LifetimeMaterializationCursorFacts {
                        cutoff_event_rowid: 1,
                        ..
                    },
                    ..
                }
            ),
            "phase {phase}: {phase_result:?}"
        );
        assert!(
            load_receipt(storage.connection_for_test(), &appended.operation_id)
                .unwrap()
                .is_none()
        );
        drive_until_cursor(&mut storage, "verifySource", 0);
        let reset = storage
            .materialize_lifetime_batch_for_test(&mut NeverCancel, None)
            .expect("verification catch-up reset");
        assert!(
            matches!(
                reset,
                LifetimeMaterializationBatchResult::Progress {
                    cursor: LifetimeMaterializationCursorFacts {
                        cutoff_event_rowid: 2,
                        phase: StorageLifetimePhaseV4::IndexSource,
                        after_scan_rowid: 0,
                        scan_exhausted: false,
                    },
                    source_rows: 0,
                    ..
                }
            ),
            "phase {phase}: {reset:?}"
        );
    }
}

#[test]
fn bounded_v6_ready_append_invalidates_but_duplicate_and_conflict_do_not() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    drive_bounded_v6(&mut storage);
    let appended = event(
        "invalidate",
        "run",
        "run_invalidate",
        json!({
            "run_id":"run_invalidate","chat_id":null,"state":"running",
            "worktree_path":null,"source_kind":"fixture",
            "record":{"runId":"run_invalidate"},"recovery":null,
        }),
        "2026-08-28T08:02:00Z",
    );
    let mut rolled_back = appended.clone();
    rolled_back.event_id = "evt_rolled_back".to_string();
    rolled_back.operation_id = "op_rolled_back".to_string();
    rolled_back.aggregate_id = "run_rolled_back".to_string();
    assert_eq!(
        storage
            .append_event_with_failure(
                &rolled_back,
                Some(crate::storage::AppendFailurePoint::AfterEventInsertBeforeProjection),
            )
            .expect_err("append rollback")
            .kind(),
        "injectedFailure"
    );
    assert_eq!(materialization_state(&storage), "ready");
    assert_eq!(
        storage.append_event(&appended).unwrap(),
        crate::storage::AppendOutcome::Committed
    );
    assert_eq!(materialization_state(&storage), "pending");
    assert_eq!(
        storage.append_event(&appended).unwrap(),
        crate::storage::AppendOutcome::DuplicateNoOp
    );
    assert_eq!(materialization_state(&storage), "pending");
    let mut conflict = appended.clone();
    conflict.event_id.push_str("_conflict");
    assert_eq!(
        storage.append_event(&conflict).unwrap(),
        crate::storage::AppendOutcome::Conflict
    );
    assert_eq!(materialization_state(&storage), "pending");
}

#[test]
fn empty_materialization_is_atomic_ready_idempotent_and_reopens() {
    let (_root, path) = database();
    let mut storage = Storage::open_v3_for_test(&path).expect("storage");
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

    let mut reopened = Storage::open_v3_for_test(&path).expect("ready storage reopens");
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
    let mut storage = Storage::open_v3_for_test(&path).expect("storage");
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
fn legacy_materialization_refuses_existing_v4_cursor_without_mutation() {
    let (_root, path) = database();
    let mut storage = Storage::open(&path).expect("v4 storage");
    storage
        .connection_for_test()
        .execute(
            "INSERT INTO lifetime_materialization_job(\
               singleton,job_id,state,cutoff_event_rowid,phase,after_scan_rowid,scan_exhausted,\
               failure_kind,created_at,updated_at,completed_at\
             ) VALUES (1,'bounded_v6_cursor','pending',0,'receipts',0,0,NULL,\
               '2026-08-28T08:00:00Z','2026-08-28T08:00:00Z',NULL);",
            [],
        )
        .unwrap();
    let snapshot_before: (String, String) = storage
        .connection_for_test()
        .query_row(
            "SELECT materialization_state,json_object(\
               'job_id',job_id,'state',state,'phase',phase,\
               'after_scan_rowid',after_scan_rowid,'scan_exhausted',scan_exhausted\
             ) FROM storage_schema_meta,lifetime_materialization_job \
             WHERE storage_schema_meta.singleton=1 AND lifetime_materialization_job.singleton=1;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let rows_before = materialized_row_count(&storage);

    let error = storage
        .materialize_lifetime_records(None)
        .expect_err("legacy materialization must not consume a v4 cursor");
    assert_eq!(error.kind(), "storageMaterializationFailed");
    assert_eq!(
        storage
            .connection_for_test()
            .query_row(
                "SELECT materialization_state,json_object(\
                   'job_id',job_id,'state',state,'phase',phase,\
                   'after_scan_rowid',after_scan_rowid,'scan_exhausted',scan_exhausted\
                 ) FROM storage_schema_meta,lifetime_materialization_job \
                 WHERE storage_schema_meta.singleton=1 AND lifetime_materialization_job.singleton=1;",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap(),
        snapshot_before
    );
    assert_eq!(materialized_row_count(&storage), rows_before);
}

#[test]
fn populated_materialization_preserves_receipts_messages_workspaces_and_baseline_tail() {
    let (_root, path) = database();
    let mut storage = Storage::open_v3_for_test(&path).expect("storage");
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

    let mut reopened =
        Storage::open_v3_for_test(&path).expect("materialized ready storage reopens");
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
        let mut storage = Storage::open_v3_for_test(&path).expect("storage");
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
        let mut storage = Storage::open_v3_for_test(&path).expect("storage");
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
        let storage = Storage::open_v3_for_test(&path).expect("storage");
        storage
            .connection_for_test()
            .execute(
                "UPDATE storage_schema_meta SET materialization_state=?1 WHERE singleton=1;",
                [state],
            )
            .unwrap();
        drop(storage);
        let error = Storage::open_v3_for_test(&path)
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
        json!({
            "chat_id":"chat_status","project_id":"project_status","title":"비밀 대화",
            "durable_event_sequence":0,"last_message":null
        }),
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
