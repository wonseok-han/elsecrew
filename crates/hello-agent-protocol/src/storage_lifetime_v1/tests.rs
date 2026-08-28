use super::*;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    storage_schema_version: u32,
    protocol_version: u32,
    wire: WireContract,
    migration: MigrationContract,
    compaction: CompactionContract,
    public_status_privacy: PrivacyContract,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireContract {
    method: String,
    payload_schema_version: u32,
    bridge_version: u32,
    bridge_method: String,
    advertised_before_d1: bool,
    dispatchable_before_d1: bool,
    cases_file: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationContract {
    file: String,
    from_version: u32,
    to_version: u32,
    mode: String,
    tables: Vec<String>,
    indexes: Vec<String>,
    existing_tables_preserved: Vec<String>,
    existing_events_deleted: bool,
    existing_projection_columns_changed: bool,
    startup_ready_before_materialization: bool,
    materialization_trigger: String,
    downgrade_policy: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompactionContract {
    mode: String,
    steps: Vec<String>,
    baseline_tail_replay_equivalent: bool,
    old_operation_duplicate_conflict_preserved: bool,
    actual_legacy_namespace_preserved: bool,
    failure_rollback_required: bool,
    logical_delete_before_verification: bool,
    vacuum_is_logical_success_criterion: bool,
    physical_cleanup_enabled: bool,
    cleanup_candidate_api_enabled: bool,
    cleanup_action_api_enabled: bool,
    operation_receipts_immutable: bool,
    chat_messages_immutable: bool,
    never_cleanup_states: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrivacyContract {
    allowed_content: String,
    forbidden_fields: Vec<String>,
}

#[derive(Deserialize)]
struct StatusCases {
    valid: Vec<ValidStatusCase>,
    invalid: Vec<InvalidStatusCase>,
}

#[derive(Deserialize)]
struct ValidStatusCase {
    name: String,
    request: Value,
    response: Value,
}

#[derive(Deserialize)]
struct InvalidStatusCase {
    name: String,
    target: String,
    value: Value,
}

#[test]
fn storage_v2_golden_is_additive_and_keeps_d0a_non_destructive() {
    let (root, manifest) = manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.storage_schema_version,
        STORAGE_LIFETIME_SCHEMA_VERSION_V1
    );
    assert_eq!(manifest.protocol_version, crate::PROTOCOL_VERSION);
    assert_eq!(manifest.migration.from_version, 1);
    assert_eq!(manifest.migration.to_version, 2);
    assert_eq!(manifest.migration.mode, "additiveDdlIndexesMetadataOnly");
    assert!(STORAGE_LIFETIME_TABLES_V2.iter().copied().eq(manifest
        .migration
        .tables
        .iter()
        .map(String::as_str)));
    assert!(STORAGE_LIFETIME_INDEXES_V2.iter().copied().eq(manifest
        .migration
        .indexes
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.migration.existing_tables_preserved,
        [
            "schema_meta",
            "app_instances",
            "events",
            "projects",
            "chats",
            "runs",
            "locks",
        ]
    );
    assert!(!manifest.migration.existing_events_deleted);
    assert!(!manifest.migration.existing_projection_columns_changed);
    assert!(manifest.migration.startup_ready_before_materialization);
    assert_eq!(manifest.migration.materialization_trigger, "idleAfterReady");
    assert_eq!(manifest.migration.downgrade_policy, "openQuestion");

    let sql = fs::read_to_string(root.join(&manifest.migration.file)).expect("read migration");
    for table in STORAGE_LIFETIME_TABLES_V2 {
        assert!(sql.contains(&format!("CREATE TABLE {table} (")), "{table}");
    }
    for index in STORAGE_LIFETIME_INDEXES_V2 {
        assert!(sql.contains(&format!("CREATE INDEX {index}")), "{index}");
    }
    let normalized = sql.to_ascii_uppercase();
    for forbidden in [
        "ALTER TABLE",
        "DROP TABLE",
        "DROP TRIGGER",
        "DELETE FROM EVENTS",
        "UPDATE EVENTS",
        "UPDATE PROJECTS",
        "UPDATE CHATS",
        "UPDATE RUNS",
        "UPDATE LOCKS",
    ] {
        assert!(
            !normalized.contains(forbidden),
            "forbidden SQL: {forbidden}"
        );
    }
}

#[test]
fn rust_validates_exact_privacy_safe_storage_status_cases() {
    let (root, manifest) = manifest();
    assert_eq!(manifest.wire.method, STORAGE_STATUS_METHOD_V1);
    assert_eq!(
        manifest.wire.payload_schema_version,
        STORAGE_STATUS_SCHEMA_VERSION_V1
    );
    assert_eq!(
        manifest.wire.bridge_version,
        STORAGE_STATUS_BRIDGE_VERSION_V1
    );
    assert_eq!(manifest.wire.bridge_method, STORAGE_STATUS_BRIDGE_METHOD_V1);
    assert!(!manifest.wire.advertised_before_d1);
    assert!(!manifest.wire.dispatchable_before_d1);
    assert_eq!(
        manifest.public_status_privacy.allowed_content,
        "aggregateCountsBytesAndMaintenanceStateOnly"
    );
    assert_eq!(
        manifest.public_status_privacy.forbidden_fields,
        [
            "rawChatText",
            "rawProviderText",
            "rawReasoning",
            "prompt",
            "token",
            "auth",
            "absolutePath",
        ]
    );

    let cases: StatusCases = read_json(root.join(&manifest.wire.cases_file));
    for case in cases.valid {
        validate_storage_status_request_v1(&case.request).expect(&case.name);
        validate_storage_status_v1(&case.response).expect(&case.name);
    }
    for case in cases.invalid {
        let result = if case.target == "request" {
            validate_storage_status_request_v1(&case.value).map(|_| ())
        } else {
            validate_storage_status_v1(&case.value).map(|_| ())
        };
        assert!(result.is_err(), "{}", case.name);
    }
}

#[test]
fn lossless_compaction_boundary_preserves_history_and_namespaces() {
    let (_, manifest) = manifest();
    assert_eq!(manifest.compaction.mode, "losslessInternalOnly");
    assert!(LOSSLESS_COMPACTION_STEPS_V1.iter().copied().eq(manifest
        .compaction
        .steps
        .iter()
        .map(String::as_str)));
    assert!(NEVER_CLEANUP_STATES_V1.iter().copied().eq(manifest
        .compaction
        .never_cleanup_states
        .iter()
        .map(String::as_str)));
    assert!(manifest.compaction.baseline_tail_replay_equivalent);
    assert!(
        manifest
            .compaction
            .old_operation_duplicate_conflict_preserved
    );
    assert!(manifest.compaction.actual_legacy_namespace_preserved);
    assert!(manifest.compaction.failure_rollback_required);
    assert!(!manifest.compaction.logical_delete_before_verification);
    assert!(!manifest.compaction.vacuum_is_logical_success_criterion);
    assert!(!manifest.compaction.physical_cleanup_enabled);
    assert!(!manifest.compaction.cleanup_candidate_api_enabled);
    assert!(!manifest.compaction.cleanup_action_api_enabled);
    assert!(manifest.compaction.operation_receipts_immutable);
    assert!(manifest.compaction.chat_messages_immutable);
    assert!(!crate::METHODS_V1.contains(&STORAGE_STATUS_METHOD_V1));
}

fn manifest() -> (PathBuf, Manifest) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/storage-v2");
    let manifest = read_json(root.join("manifest.json"));
    (root, manifest)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> T {
    let source = fs::read(path).expect("read fixture");
    serde_json::from_slice(&source).expect("decode fixture")
}
