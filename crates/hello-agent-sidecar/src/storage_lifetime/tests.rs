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
    LedgerEventV1 {
        event_id: format!("evt_{id}"),
        operation_id: format!("op_{id}"),
        request_id: None,
        aggregate_type: aggregate_type.to_string(),
        aggregate_id: aggregate_id.to_string(),
        aggregate_sequence: 1,
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
