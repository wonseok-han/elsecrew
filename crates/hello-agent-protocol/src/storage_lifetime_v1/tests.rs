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
    batching: Value,
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
    materialization_state_ready_required: bool,
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
    failure_rollback_scope: FailureRollbackScope,
    logical_delete_before_verification: bool,
    vacuum_is_logical_success_criterion: bool,
    physical_cleanup_enabled: bool,
    cleanup_candidate_api_enabled: bool,
    cleanup_action_api_enabled: bool,
    operation_receipts_immutable: bool,
    chat_messages_immutable: bool,
    operation_receipt: OperationReceiptContract,
    never_cleanup_states: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FailureRollbackScope {
    derived_preparation_batches_may_persist: bool,
    source_ledger_mutation_atomic_rollback_required: bool,
    compaction_trigger_change_atomic_rollback_required: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationReceiptContract {
    cases_file: String,
    immutable_fields: Vec<String>,
    request_id_comparison: String,
    payload_comparison: String,
    digest: OperationReceiptDigestContract,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationReceiptDigestContract {
    algorithm: String,
    version: u32,
    format: String,
    role: String,
    writer_must_recompute: bool,
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

#[derive(Deserialize)]
struct OperationReceiptCases {
    cases: Vec<OperationReceiptCase>,
    invalid: Vec<InvalidOperationReceiptCase>,
}

#[derive(Deserialize)]
struct OperationReceiptCase {
    name: String,
    namespace: String,
    receipt: OperationReceiptV2,
    incoming: LedgerEventV1,
    expected: OperationDecisionV1,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvalidOperationReceiptCase {
    name: String,
    expected_error: String,
    receipt: OperationReceiptV2,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchingCases {
    valid_cursors: Vec<NamedValue>,
    valid_payload_batches: Vec<NamedValue>,
    invalid_cursors: Vec<NamedValue>,
    invalid_payload_batches: Vec<NamedValue>,
    invalid_policy_mutations: Vec<InvalidPolicyMutation>,
}

#[derive(Deserialize)]
struct NamedValue {
    name: String,
    value: Value,
}

#[derive(Deserialize)]
struct InvalidPolicyMutation {
    name: String,
    path: Vec<String>,
    replacement: Value,
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
    assert_eq!(
        manifest.migration.materialization_trigger,
        "idleAfterSidecarHandshakeReady"
    );
    assert!(!manifest.migration.materialization_state_ready_required);
    assert_eq!(manifest.migration.downgrade_policy, "openQuestion");

    let sql = fs::read_to_string(root.join(&manifest.migration.file)).expect("read migration");
    for table in STORAGE_LIFETIME_TABLES_V2 {
        assert!(sql.contains(&format!("CREATE TABLE {table} (")), "{table}");
    }
    for index in STORAGE_LIFETIME_INDEXES_V2 {
        assert!(
            sql.contains(&format!("CREATE INDEX {index}"))
                || sql.contains(&format!("CREATE UNIQUE INDEX {index}")),
            "{index}"
        );
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
fn rust_validates_bounded_keyset_progress_cancellation_and_atomic_rollback() {
    let (root, manifest) = manifest();
    validate_storage_lifetime_batch_policy_v1(&manifest.batching).expect("fixed batch policy");
    let cases_file = manifest
        .batching
        .get("casesFile")
        .and_then(Value::as_str)
        .expect("batching cases file");
    let cases: BatchingCases = read_json(root.join(cases_file));

    for case in cases.valid_cursors {
        validate_storage_lifetime_cursor_v1(&case.value).expect(&case.name);
    }
    for case in cases.valid_payload_batches {
        let payloads: Vec<u64> = serde_json::from_value(case.value).expect(&case.name);
        validate_storage_lifetime_batch_source_payloads_v1(&payloads).expect(&case.name);
    }
    for case in cases.invalid_cursors {
        assert!(
            validate_storage_lifetime_cursor_v1(&case.value).is_err(),
            "{}",
            case.name
        );
    }
    for case in cases.invalid_payload_batches {
        let payloads: Vec<u64> = serde_json::from_value(case.value).expect(&case.name);
        assert!(
            validate_storage_lifetime_batch_source_payloads_v1(&payloads).is_err(),
            "{}",
            case.name
        );
    }
    for case in cases.invalid_policy_mutations {
        let mut invalid = manifest.batching.clone();
        set_value_path(&mut invalid, &case.path, case.replacement);
        assert!(
            validate_storage_lifetime_batch_policy_v1(&invalid).is_err(),
            "{}",
            case.name
        );
    }
}

#[test]
fn lossless_compaction_boundary_preserves_history_and_namespaces() {
    let (root, manifest) = manifest();
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
    assert!(
        manifest
            .compaction
            .failure_rollback_scope
            .derived_preparation_batches_may_persist
    );
    assert!(
        manifest
            .compaction
            .failure_rollback_scope
            .source_ledger_mutation_atomic_rollback_required
    );
    assert!(
        manifest
            .compaction
            .failure_rollback_scope
            .compaction_trigger_change_atomic_rollback_required
    );
    assert!(!manifest.compaction.logical_delete_before_verification);
    assert!(!manifest.compaction.vacuum_is_logical_success_criterion);
    assert!(!manifest.compaction.physical_cleanup_enabled);
    assert!(!manifest.compaction.cleanup_candidate_api_enabled);
    assert!(!manifest.compaction.cleanup_action_api_enabled);
    assert!(manifest.compaction.operation_receipts_immutable);
    assert!(manifest.compaction.chat_messages_immutable);
    assert_eq!(
        manifest.compaction.operation_receipt.immutable_fields,
        [
            "operationId",
            "eventId",
            "aggregateType",
            "aggregateId",
            "aggregateSequence",
            "eventType",
            "payloadSchemaVersion",
            "canonicalPayloadJson",
            "canonicalPayloadDigestAlgorithm",
            "canonicalPayloadDigestVersion",
            "canonicalPayloadDigestFormat",
            "canonicalPayloadDigest",
            "eventTimestamp",
        ]
    );
    assert_eq!(
        manifest.compaction.operation_receipt.request_id_comparison,
        "ignoredLikeStorageV1"
    );
    assert_eq!(
        manifest.compaction.operation_receipt.payload_comparison,
        "exactCanonicalJson"
    );
    assert_eq!(
        manifest.compaction.operation_receipt.digest.algorithm,
        OPERATION_RECEIPT_DIGEST_ALGORITHM_V2
    );
    assert_eq!(
        manifest.compaction.operation_receipt.digest.version,
        OPERATION_RECEIPT_DIGEST_VERSION_V2
    );
    assert_eq!(
        manifest.compaction.operation_receipt.digest.format,
        OPERATION_RECEIPT_DIGEST_FORMAT_V2
    );
    assert_eq!(
        manifest.compaction.operation_receipt.digest.role,
        "integrityCheckNotEqualitySubstitute"
    );
    assert!(
        manifest
            .compaction
            .operation_receipt
            .digest
            .writer_must_recompute
    );
    let receipts: OperationReceiptCases =
        read_json(root.join(&manifest.compaction.operation_receipt.cases_file));
    let mut namespaces = Vec::new();
    for case in &receipts.cases {
        namespaces.push(case.namespace.clone());
        assert_eq!(
            classify_operation_against_receipt_v2(&case.receipt, &case.incoming).expect(&case.name),
            case.expected,
            "{}",
            case.name
        );
    }
    assert!(namespaces.contains(&"legacy".to_string()));
    assert!(namespaces.contains(&"actualAgent".to_string()));
    assert!(namespaces.contains(&"recovery".to_string()));
    let incoming = &receipts.cases[0].incoming;
    for case in &receipts.invalid {
        let error = classify_operation_against_receipt_v2(&case.receipt, incoming)
            .expect_err(&case.name)
            .to_string();
        assert!(
            error.contains(&case.expected_error),
            "{}: {error}",
            case.name
        );
    }
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

fn set_value_path(target: &mut Value, path: &[String], replacement: Value) {
    let (last, parents) = path.split_last().expect("non-empty mutation path");
    let mut current = target;
    for segment in parents {
        current = current
            .as_object_mut()
            .and_then(|object| object.get_mut(segment))
            .expect("policy mutation parent");
    }
    current
        .as_object_mut()
        .expect("policy mutation object")
        .insert(last.clone(), replacement);
}
