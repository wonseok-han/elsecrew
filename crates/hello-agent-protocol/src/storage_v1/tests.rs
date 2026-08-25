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
    input: RecoveryInputV1,
    expected: RecoveryResultV1,
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
        assert_eq!(
            recover_unclean_shutdown_v1(case.input).expect(&case.name),
            case.expected,
            "{}",
            case.name
        );
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
