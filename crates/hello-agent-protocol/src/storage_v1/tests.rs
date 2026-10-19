use super::*;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    migration: MigrationContract,
    replay_file: String,
    idempotency_file: String,
    recovery_file: String,
    transaction_file: String,
    idempotency_comparison: String,
    recovery_validation_failure_status: String,
    timestamp_format: String,
    connection: ConnectionContract,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationContract {
    file: String,
    tables: Vec<String>,
    ledger_fields: Vec<String>,
    operation_id_unique: bool,
    request_id_durable_key: bool,
    events_append_only: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionContract {
    scope: String,
    enable_statement: String,
    verify_query: String,
    required_value: u32,
    steps: Vec<String>,
    failure_status: String,
    ready_before_verification: bool,
}

#[derive(Deserialize)]
struct ReplayFixture {
    events: Vec<LedgerEventV1>,
    expected_projection: ProjectionStateV1,
}

#[derive(Deserialize)]
struct IdempotencyFixture {
    existing: Vec<OperationRecordV1>,
    cases: Vec<IdempotencyCase>,
}

#[derive(Deserialize)]
struct IdempotencyCase {
    name: String,
    incoming: OperationRecordV1,
    expected: OperationDecisionV1,
}

#[derive(Deserialize)]
struct RecoveryFixture {
    cases: Vec<RecoveryCase>,
}

#[derive(Deserialize)]
struct RecoveryCase {
    name: String,
    expected_valid: bool,
    input: RecoveryInputV1,
    expected: Option<RecoveryResultV1>,
}

#[derive(Deserialize)]
struct TransactionFixture {
    before: StorageStateV1,
    cases: Vec<TransactionCase>,
}

#[derive(Deserialize)]
struct TransactionCase {
    name: String,
    failure_point: Option<TransactionFailurePointV1>,
    incoming: LedgerEventV1,
    expected: TransactionExpected,
}

#[derive(Debug, PartialEq, Deserialize)]
struct TransactionExpected {
    outcome: TransactionOutcomeV1,
    steps: Vec<String>,
    event_ids: Vec<String>,
    projection: ProjectionStateV1,
    risky_operation_started: bool,
}

#[test]
fn sqlite_v1_migration_matches_the_language_neutral_schema_contract() {
    let (root, manifest) = manifest();
    assert_eq!(manifest.schema_version, SQLITE_SCHEMA_VERSION);
    assert!(SQLITE_TABLES_V1.iter().copied().eq(manifest
        .migration
        .tables
        .iter()
        .map(String::as_str)));
    assert!(LEDGER_FIELDS_V1.iter().copied().eq(manifest
        .migration
        .ledger_fields
        .iter()
        .map(String::as_str)));
    assert!(manifest.migration.operation_id_unique);
    assert!(!manifest.migration.request_id_durable_key);
    assert!(manifest.migration.events_append_only);
    assert_eq!(
        manifest.idempotency_comparison,
        "fullImmutableLedgerRecordExceptRequestId"
    );
    assert_eq!(
        manifest.recovery_validation_failure_status,
        RECOVERY_VALIDATION_FAILURE_STATUS_V1
    );
    assert_eq!(manifest.timestamp_format, UTC_TIMESTAMP_FORMAT_V1);
    assert_eq!(manifest.connection.scope, "everySidecarConnection");
    assert_eq!(
        manifest.connection.enable_statement,
        SQLITE_FOREIGN_KEYS_ENABLE_V1
    );
    assert_eq!(
        manifest.connection.verify_query,
        SQLITE_FOREIGN_KEYS_VERIFY_V1
    );
    assert_eq!(manifest.connection.required_value, 1);
    assert!(SQLITE_CONNECTION_OPEN_STEPS_V1.iter().copied().eq(manifest
        .connection
        .steps
        .iter()
        .map(String::as_str)));
    assert_eq!(manifest.connection.failure_status, "unavailable");
    assert!(!manifest.connection.ready_before_verification);
    assert!(validate_foreign_keys_enabled_v1(0).is_err());
    validate_foreign_keys_enabled_v1(1).expect("foreign keys enabled");

    let sql = fs::read_to_string(root.join(&manifest.migration.file)).expect("read migration");
    let created_tables = sql.matches("CREATE TABLE ").count();
    assert_eq!(created_tables, SQLITE_TABLES_V1.len());
    for table in SQLITE_TABLES_V1 {
        assert!(sql.contains(&format!("CREATE TABLE {table} (")), "{table}");
    }
    for field in LEDGER_FIELDS_V1 {
        assert!(events_definition(&sql).contains(field), "{field}");
    }
    assert!(events_definition(&sql).contains("operation_id TEXT NOT NULL UNIQUE"));
    assert!(!events_definition(&sql).contains("request_id TEXT NOT NULL UNIQUE"));
    assert!(!events_definition(&sql).contains("content_hash"));
    assert!(sql.contains("BEFORE UPDATE ON events"));
    assert!(sql.contains("BEFORE DELETE ON events"));
}

#[test]
fn rust_replay_and_idempotency_match_canonical_storage_fixtures() {
    let (root, manifest) = manifest();
    let replay: ReplayFixture = read_json(root.join(manifest.replay_file));
    assert_eq!(
        replay_ledger_v1(&replay.events).expect("replay"),
        replay.expected_projection
    );

    let fixture: IdempotencyFixture = read_json(root.join(manifest.idempotency_file));
    for case in fixture.cases {
        assert_eq!(
            classify_operation_v1(&fixture.existing, &case.incoming).expect(&case.name),
            case.expected,
            "{}",
            case.name
        );
    }
}

#[test]
fn rust_unclean_recovery_matches_canonical_storage_fixtures() {
    let (root, manifest) = manifest();
    let fixture: RecoveryFixture = read_json(root.join(manifest.recovery_file));
    for case in fixture.cases {
        let result = recover_unclean_shutdown_v1(case.input);
        assert_eq!(result.is_ok(), case.expected_valid, "{}", case.name);
        if let Some(expected) = case.expected {
            assert_eq!(result.expect(&case.name), expected, "{}", case.name);
        }
    }
}

#[test]
fn replay_operation_index_preserves_distant_duplicates_and_conflicts() {
    let (root, manifest) = manifest();
    let fixture: ReplayFixture = read_json(root.join(manifest.replay_file));
    let template = fixture.events.first().expect("initial event");
    let events: Vec<_> = (1..=2048)
        .map(|sequence| {
            let mut event = template.clone();
            event.event_id = format!("evt_index_{sequence}");
            event.operation_id = format!("op_index_{sequence}");
            event.aggregate_sequence = sequence;
            event
        })
        .collect();
    let expected = replay_ledger_v1(&events).expect("unique history");
    for index in [0, 1024, 2047] {
        let mut repeated = events.clone();
        repeated.push(events[index].clone());
        assert_eq!(replay_ledger_v1(&repeated).unwrap(), expected);
        repeated.last_mut().unwrap().canonical_json_payload = "{}".into();
        assert!(
            replay_ledger_v1(&repeated).is_err(),
            "conflicting operation {index}"
        );
        *repeated.last_mut().unwrap() = events[index].clone();
        repeated.last_mut().unwrap().aggregate_sequence = 0;
        assert!(
            replay_ledger_v1(&repeated).is_err(),
            "invalid duplicate {index}"
        );
    }
    let mut reused_event = events;
    let mut incoming = reused_event.last().unwrap().clone();
    incoming.operation_id = "op_new_reused_event".into();
    incoming.event_id = reused_event[0].event_id.clone();
    incoming.aggregate_sequence += 1;
    reused_event.push(incoming);
    assert!(replay_ledger_v1(&reused_event).is_err());
}

#[test]
fn rust_transaction_rolls_back_partial_writes_and_commits_before_risky_work() {
    let (root, manifest) = manifest();
    let fixture: TransactionFixture = read_json(root.join(manifest.transaction_file));
    for case in fixture.cases {
        let result = apply_pre_risky_storage_transaction_v1(
            &fixture.before,
            &case.incoming,
            case.failure_point,
        )
        .expect(&case.name);
        let actual = TransactionExpected {
            outcome: result.outcome,
            steps: result.steps,
            event_ids: result
                .state
                .events
                .iter()
                .map(|event| event.event_id.clone())
                .collect(),
            projection: result.state.projection,
            risky_operation_started: result.risky_operation_started,
        };
        assert_eq!(actual, case.expected, "{}", case.name);
    }
}

fn manifest() -> (PathBuf, Manifest) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/storage-v1");
    let manifest = read_json(root.join("manifest.json"));
    (root, manifest)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> T {
    let source = fs::read(path).expect("read fixture");
    serde_json::from_slice(&source).expect("decode fixture")
}

fn events_definition(sql: &str) -> &str {
    let start = sql.find("CREATE TABLE events (").expect("events table");
    let tail = &sql[start..];
    let end = tail.find(") STRICT;").expect("events table end");
    &tail[..end]
}
