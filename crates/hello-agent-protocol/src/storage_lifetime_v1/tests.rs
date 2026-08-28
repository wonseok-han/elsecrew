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
struct ManifestV3 {
    schema_version: u32,
    storage_schema_version: u32,
    protocol_version: u32,
    payload_schema_version: u32,
    migration: MigrationContractV3,
    batching: Value,
    compaction_boundary: CompactionBoundaryV3,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestV4 {
    schema_version: u32,
    storage_schema_version: u32,
    protocol_version: u32,
    payload_schema_version: u32,
    migration: MigrationContractV4,
    bounded_verification: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestV5 {
    schema_version: u32,
    storage_schema_version: u32,
    protocol_version: u32,
    payload_schema_version: u32,
    migration: MigrationContractV4,
    bounded_projection_verification: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestV6 {
    schema_version: u32,
    storage_schema_version: u32,
    protocol_version: u32,
    payload_schema_version: u32,
    migration: MigrationContractV6,
    indexed_bounded_verification: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationContractV6 {
    file: String,
    predecessor_migration_file: String,
    predecessor_migration_mutation_allowed: bool,
    from_version: u32,
    to_version: u32,
    mode: String,
    preflight: Value,
    idempotency: String,
    partial_extra_or_unsafe_pair: String,
    history_backfill: String,
    tables_added: Vec<String>,
    tables_rebuilt: Vec<String>,
    indexes_added: Vec<String>,
    triggers_added: Vec<String>,
    preserved_tables: Vec<String>,
    preserved_indexes: Vec<String>,
    preserved_triggers: Vec<String>,
    downgrade_policy: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationContractV4 {
    file: String,
    predecessor_migration_file: String,
    predecessor_migration_mutation_allowed: bool,
    from_version: u32,
    to_version: u32,
    mode: String,
    preflight: Value,
    idempotency: String,
    partial_extra_or_unsafe_pair: String,
    tables_added: Vec<String>,
    tables_rebuilt: Vec<String>,
    preserved_tables: Vec<String>,
    preserved_indexes: Vec<String>,
    preserved_triggers: Vec<String>,
    downgrade_policy: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationContractV3 {
    file: String,
    predecessor_migration_file: String,
    predecessor_migration_mutation_allowed: bool,
    from_version: u32,
    to_version: u32,
    mode: String,
    preflight: Value,
    idempotency: String,
    partial_or_extra_schema: String,
    tables_added: Vec<String>,
    tables_rebuilt: Vec<String>,
    preserved_tables: Vec<String>,
    preserved_indexes: Vec<String>,
    preserved_triggers: Vec<String>,
    preserved_materialization_pairs: Vec<String>,
    downgrade_policy: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompactionBoundaryV3 {
    maintenance_jobs_unchanged: bool,
    maintenance_job_kind: String,
    materialization_job_visible_to_compaction_status: bool,
    physical_cleanup_enabled: bool,
    ledger_deletion_enabled: bool,
    downgrade_policy: String,
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
    valid_transitions: Vec<CursorTransitionCase>,
    valid_payload_batches: Vec<NamedValue>,
    invalid_cursors: Vec<NamedValue>,
    invalid_transitions: Vec<CursorTransitionCase>,
    invalid_payload_batches: Vec<NamedValue>,
    valid_jobs: Vec<NamedValue>,
    valid_pairs: Vec<NamedValue>,
    valid_lifecycle_transitions: Vec<LifecycleTransitionCase>,
    invalid_jobs: Vec<NamedValue>,
    invalid_pairs: Vec<NamedValue>,
    invalid_lifecycle_transitions: Vec<LifecycleTransitionCase>,
    future_sidecar_regression: FutureSidecarRegression,
    invalid_preflight_mutations: Vec<InvalidPolicyMutation>,
    invalid_policy_mutations: Vec<InvalidPolicyMutation>,
}

#[derive(Deserialize)]
struct LifecycleTransitionCase {
    name: String,
    operation: StorageLifetimeLifecycleOperationV1,
    previous: Value,
    next: Value,
}

#[derive(Deserialize)]
struct CursorTransitionCase {
    name: String,
    previous: Option<Value>,
    next: Value,
}

#[derive(Deserialize)]
struct FutureSidecarRegression {
    name: String,
    steps: Vec<String>,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V4Cases {
    v3_migration_cases: Vec<V3MigrationCase>,
    v3_absent_migration_cases: Vec<V3AbsentMigrationCase>,
    removed_lifecycle_operations: Vec<String>,
    valid_cursors: Vec<NamedValue>,
    invalid_cursors: Vec<NamedValue>,
    valid_cursor_transitions: Vec<CursorTransitionCase>,
    invalid_cursor_transitions: Vec<CursorTransitionCase>,
    valid_payload_batches: Vec<NamedValue>,
    invalid_payload_batches: Vec<NamedValue>,
    valid_jobs: Vec<NamedValue>,
    invalid_jobs: Vec<NamedValue>,
    valid_pairs: Vec<NamedValue>,
    invalid_pairs: Vec<NamedValue>,
    runnable_reopen_pairs: Vec<usize>,
    fail_closed_reopen_pairs: Vec<usize>,
    valid_lifecycle_transitions: Vec<LifecycleTransitionCaseV2>,
    invalid_lifecycle_transitions: Vec<LifecycleTransitionCaseV2>,
    valid_verification_evidence: Vec<NamedValue>,
    invalid_verification_evidence: Vec<NamedValue>,
    execution_models: ExecutionModels,
    invalid_preflight_mutations: Vec<InvalidPolicyMutation>,
    invalid_policy_mutations: Vec<InvalidPolicyMutation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V3MigrationCase {
    name: String,
    v3_state: String,
    v3_phase: String,
    after_event_rowid: u64,
    v4_state: String,
    v4_phase: String,
    after_scan_rowid: u64,
    scan_exhausted: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V3AbsentMigrationCase {
    name: String,
    materialization_state: String,
}

#[derive(Deserialize)]
struct LifecycleTransitionCaseV2 {
    name: String,
    operation: StorageLifetimeLifecycleOperationV2,
    previous: Value,
    next: Value,
}

#[derive(Deserialize)]
struct ExecutionModels {
    valid: Vec<ExecutionModel>,
    invalid: Vec<ExecutionModel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionModel {
    name: String,
    verify_batch_committed: bool,
    outcome: String,
    final_publication_same_call: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V5Cases {
    migration_matrix: V5MigrationMatrix,
    phase_order: Vec<String>,
    valid_catch_up_resets: Vec<V5CatchUpCase>,
    invalid_catch_up_resets: Vec<V5CatchUpCase>,
    reopen_pairs: V5ReopenPairs,
    valid_lifecycle_operations: Vec<String>,
    invalid_preflight_mutations: Vec<InvalidPolicyMutation>,
    invalid_policy_mutations: Vec<InvalidPolicyMutation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V5ReopenPairs {
    valid: Vec<NamedValue>,
    fail_closed: Vec<NamedValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V5MigrationMatrix {
    job_states: Vec<String>,
    position_preserved_phases: Vec<String>,
    projection_restart_phases: Vec<String>,
    absent_materialization_states: Vec<String>,
    state_mapping: String,
    position_preserved_mapping: String,
    projection_restart_mapping: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V5CatchUpCase {
    name: String,
    previous_state: String,
    previous_phase: String,
    previous_after_scan_rowid: u64,
    previous_scan_exhausted: bool,
    previous_cutoff: u64,
    observed_cutoff: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V6Cases {
    migration_matrix: V6MigrationMatrix,
    phase_order: Vec<String>,
    source_index: V6SourceIndexCases,
    query_plans: Vec<V6QueryPlan>,
    workspace_provenance: V6WorkspaceProvenance,
    reopen_pairs: V5ReopenPairs,
    valid_lifecycle_operations: Vec<String>,
    invalid_preflight_mutations: Vec<InvalidPolicyMutation>,
    invalid_policy_mutations: Vec<InvalidPolicyMutation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V6MigrationMatrix {
    job_states: Vec<String>,
    v5_phases: Vec<String>,
    present_job_mapping: String,
    state_identity_timestamps_failure_evidence: String,
    absent_mappings: Vec<Value>,
    history_backfill_rows: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V6SourceIndexCases {
    columns: Vec<String>,
    source_events: Vec<V6SourceEvent>,
    valid_rows: Vec<NamedValue>,
    invalid_binding_fields: Vec<String>,
    duplicate_conflicts: Vec<String>,
    mutation_guards: Vec<String>,
    valid_transitions: Vec<Value>,
    invalid_transitions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V6SourceEvent {
    name: String,
    event_rowid: u64,
    event: LedgerEventV1,
}

#[derive(Deserialize)]
struct V6QueryPlan {
    name: String,
    sql: String,
    detail: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct V6WorkspaceProvenance {
    project_id: String,
    canonical_payload: String,
    payload_bytes: usize,
    max_payload_bytes: u64,
    comparison: String,
    removals_legal: bool,
    stop_when: String,
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
    let (root, manifest) = manifest_v3();
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
    for case in cases.valid_transitions {
        validate_storage_lifetime_cursor_transition_v1(case.previous.as_ref(), &case.next)
            .expect(&case.name);
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
    for case in cases.invalid_transitions {
        assert!(
            validate_storage_lifetime_cursor_transition_v1(case.previous.as_ref(), &case.next)
                .is_err(),
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
    for case in cases.valid_jobs {
        validate_storage_lifetime_materialization_job_v1(&case.value).expect(&case.name);
    }
    for case in cases.valid_pairs {
        validate_storage_lifetime_materialization_pair_v1(&case.value).expect(&case.name);
    }
    for case in cases.valid_lifecycle_transitions {
        validate_storage_lifetime_lifecycle_transition_v1(
            case.operation,
            &case.previous,
            &case.next,
        )
        .expect(&case.name);
    }
    for case in cases.invalid_jobs {
        assert!(
            validate_storage_lifetime_materialization_job_v1(&case.value).is_err(),
            "{}",
            case.name
        );
    }
    for case in cases.invalid_pairs {
        assert!(
            validate_storage_lifetime_materialization_pair_v1(&case.value).is_err(),
            "{}",
            case.name
        );
    }
    for case in cases.invalid_lifecycle_transitions {
        assert!(
            validate_storage_lifetime_lifecycle_transition_v1(
                case.operation,
                &case.previous,
                &case.next,
            )
            .is_err(),
            "{}",
            case.name
        );
    }
    validate_storage_lifetime_v3_preflight_policy_v1(&manifest.migration.preflight)
        .expect("fixed v3 preflight");
    for case in cases.invalid_preflight_mutations {
        let mut invalid = manifest.migration.preflight.clone();
        set_value_path(&mut invalid, &case.path, case.replacement);
        assert!(
            validate_storage_lifetime_v3_preflight_policy_v1(&invalid).is_err(),
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
    assert_eq!(
        cases.future_sidecar_regression.name,
        "ready append catch-up ready"
    );
    assert_eq!(
        cases.future_sidecar_regression.steps,
        [
            "materializeReady",
            "appendAndInvalidateReadyToPendingSameTransaction",
            "separateCatchUpVerification",
            "publishReady",
        ]
    );
}

#[test]
fn storage_v3_contract_separates_materialization_from_compaction_history() {
    let (root, manifest) = manifest_v3();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.storage_schema_version,
        STORAGE_LIFETIME_SCHEMA_VERSION_V3
    );
    assert_eq!(manifest.protocol_version, crate::PROTOCOL_VERSION);
    assert_eq!(manifest.payload_schema_version, 1);
    assert_eq!(manifest.migration.from_version, 2);
    assert_eq!(manifest.migration.to_version, 3);
    assert_eq!(
        manifest.migration.predecessor_migration_file,
        "../storage-v2/migration-v1-to-v2.sql"
    );
    assert!(!manifest.migration.predecessor_migration_mutation_allowed);
    assert_eq!(
        manifest.migration.mode,
        "exactV2PreflightThenTransactionalMetaRebuildAndDedicatedJobTable"
    );
    validate_storage_lifetime_v3_preflight_policy_v1(&manifest.migration.preflight)
        .expect("exact v3 preflight policy");
    assert_eq!(
        manifest.migration.idempotency,
        "exactStorageV3NoOpBeforeMigrationSql"
    );
    assert_eq!(
        manifest.migration.partial_or_extra_schema,
        "failClosedBeforeMutation"
    );
    assert_eq!(
        manifest.migration.tables_added,
        ["lifetime_materialization_job"]
    );
    assert_eq!(manifest.migration.tables_rebuilt, ["storage_schema_meta"]);
    assert_eq!(
        manifest.migration.preserved_tables,
        [
            "schema_meta",
            "app_instances",
            "events",
            "projects",
            "chats",
            "runs",
            "locks",
            "ledger_baselines",
            "operation_receipts",
            "chat_messages",
            "prepared_workspaces",
            "maintenance_jobs",
        ]
    );
    assert!(STORAGE_LIFETIME_INDEXES_V2.iter().copied().eq(manifest
        .migration
        .preserved_indexes
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.migration.preserved_triggers,
        [
            "events_are_append_only_update",
            "events_are_append_only_delete"
        ]
    );
    assert_eq!(
        manifest.migration.preserved_materialization_pairs,
        ["pending->pending+absent", "ready->ready+absent"]
    );
    assert_eq!(manifest.migration.downgrade_policy, "openQuestion");
    assert!(manifest.compaction_boundary.maintenance_jobs_unchanged);
    assert_eq!(
        manifest.compaction_boundary.maintenance_job_kind,
        "losslessCompaction"
    );
    assert!(
        !manifest
            .compaction_boundary
            .materialization_job_visible_to_compaction_status
    );
    assert!(!manifest.compaction_boundary.physical_cleanup_enabled);
    assert!(!manifest.compaction_boundary.ledger_deletion_enabled);
    assert_eq!(
        manifest.compaction_boundary.downgrade_policy,
        "openQuestion"
    );

    let sql = fs::read_to_string(root.join(&manifest.migration.file)).expect("read v3 migration");
    let normalized = sql.to_ascii_uppercase();
    assert!(sql.contains("CREATE TABLE lifetime_materialization_job ("));
    assert!(sql.contains("state IN ('pending', 'failed')"));
    assert!(sql.contains("phase IN ('receipts', 'messages', 'workspaces', 'baselines', 'verify')"));
    assert!(!normalized.contains("ALTER TABLE MAINTENANCE_JOBS"));
    assert!(!normalized.contains("DROP TABLE MAINTENANCE_JOBS"));
    assert!(!normalized.contains("DELETE FROM MAINTENANCE_JOBS"));
    assert!(!normalized.contains("UPDATE MAINTENANCE_JOBS"));
    assert!(!normalized.contains("DELETE FROM EVENTS"));
    assert!(!normalized.contains("DROP TRIGGER"));
}

#[test]
fn storage_v4_contract_freezes_bounded_verification_and_exact_v3_mapping() {
    let (root, manifest) = manifest_v4();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.storage_schema_version,
        STORAGE_LIFETIME_SCHEMA_VERSION_V4
    );
    assert_eq!(manifest.protocol_version, crate::PROTOCOL_VERSION);
    assert_eq!(manifest.payload_schema_version, 1);
    assert_eq!(manifest.migration.from_version, 3);
    assert_eq!(manifest.migration.to_version, 4);
    assert_eq!(
        manifest.migration.predecessor_migration_file,
        "../storage-v3/migration-v2-to-v3.sql"
    );
    assert!(!manifest.migration.predecessor_migration_mutation_allowed);
    assert_eq!(
        manifest.migration.mode,
        "exactV3AndAdmissiblePairPreflightThenTransactionalMetaAndJobRebuild"
    );
    assert_eq!(
        manifest.migration.idempotency,
        "exactStorageV4NoOpBeforeMigrationSql"
    );
    assert_eq!(
        manifest.migration.partial_extra_or_unsafe_pair,
        "failClosedBeforeMutation"
    );
    assert!(manifest.migration.tables_added.is_empty());
    assert_eq!(
        manifest.migration.tables_rebuilt,
        ["storage_schema_meta", "lifetime_materialization_job"]
    );
    assert_eq!(
        manifest.migration.preserved_tables,
        [
            "schema_meta",
            "app_instances",
            "events",
            "projects",
            "chats",
            "runs",
            "locks",
            "ledger_baselines",
            "operation_receipts",
            "chat_messages",
            "prepared_workspaces",
            "maintenance_jobs",
        ]
    );
    assert!(STORAGE_LIFETIME_INDEXES_V2.iter().copied().eq(manifest
        .migration
        .preserved_indexes
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.migration.preserved_triggers,
        [
            "events_are_append_only_update",
            "events_are_append_only_delete"
        ]
    );
    assert_eq!(manifest.migration.downgrade_policy, "openQuestion");
    validate_storage_lifetime_v4_preflight_policy_v1(&manifest.migration.preflight)
        .expect("exact v4 preflight");
    validate_storage_lifetime_batch_policy_v2(&manifest.bounded_verification)
        .expect("exact v4 bounded verification policy");

    let cases_file = manifest
        .bounded_verification
        .get("casesFile")
        .and_then(Value::as_str)
        .expect("v4 cases file");
    let cases: V4Cases = read_json(root.join(cases_file));
    assert_eq!(
        cases
            .v3_migration_cases
            .iter()
            .map(|case| format!("{}:{}", case.v3_state, case.v3_phase))
            .collect::<Vec<_>>(),
        [
            "pending:receipts",
            "pending:messages",
            "pending:workspaces",
            "pending:baselines",
            "pending:verify",
            "failed:receipts",
            "failed:messages",
            "failed:workspaces",
            "failed:baselines",
            "failed:verify",
        ]
    );
    for case in &cases.v3_migration_cases {
        let mapped_phase = if case.v3_phase == "verify" {
            "verifySource"
        } else {
            case.v3_phase.as_str()
        };
        assert_eq!(case.v4_phase, mapped_phase, "{}", case.name);
        assert_eq!(case.v4_state, case.v3_state, "{}", case.name);
        assert_eq!(
            case.after_scan_rowid, case.after_event_rowid,
            "{}",
            case.name
        );
        assert!(!case.scan_exhausted, "{}", case.name);
        assert_ne!(case.v3_state, "attempting", "{}", case.name);
    }
    assert_eq!(cases.v3_absent_migration_cases.len(), 2);
    assert!(cases
        .v3_absent_migration_cases
        .iter()
        .any(|case| { case.name == "pending absent" && case.materialization_state == "pending" }));
    assert!(cases
        .v3_absent_migration_cases
        .iter()
        .any(|case| { case.name == "ready absent" && case.materialization_state == "ready" }));
    assert_eq!(cases.removed_lifecycle_operations, ["markScanExhausted"]);
    for operation in &cases.removed_lifecycle_operations {
        assert!(
            serde_json::from_value::<StorageLifetimeLifecycleOperationV2>(Value::String(
                operation.clone(),
            ))
            .is_err()
        );
    }
    for case in &cases.valid_cursors {
        validate_storage_lifetime_cursor_v2(&case.value).expect(&case.name);
    }
    for case in &cases.invalid_cursors {
        assert!(
            validate_storage_lifetime_cursor_v2(&case.value).is_err(),
            "{}",
            case.name
        );
    }
    for case in &cases.valid_cursor_transitions {
        validate_storage_lifetime_cursor_transition_v2(case.previous.as_ref(), &case.next)
            .expect(&case.name);
    }
    for case in &cases.invalid_cursor_transitions {
        assert!(
            validate_storage_lifetime_cursor_transition_v2(case.previous.as_ref(), &case.next)
                .is_err(),
            "{}",
            case.name
        );
    }
    for case in &cases.valid_payload_batches {
        let payloads: Vec<u64> = serde_json::from_value(case.value.clone()).expect(&case.name);
        validate_storage_lifetime_batch_source_payloads_v1(&payloads).expect(&case.name);
    }
    for case in &cases.invalid_payload_batches {
        let payloads: Vec<u64> = serde_json::from_value(case.value.clone()).expect(&case.name);
        assert!(
            validate_storage_lifetime_batch_source_payloads_v1(&payloads).is_err(),
            "{}",
            case.name
        );
    }
    for case in &cases.valid_jobs {
        validate_storage_lifetime_materialization_job_v2(&case.value).expect(&case.name);
    }
    for case in &cases.invalid_jobs {
        assert!(
            validate_storage_lifetime_materialization_job_v2(&case.value).is_err(),
            "{}",
            case.name
        );
    }
    for case in &cases.valid_pairs {
        validate_storage_lifetime_materialization_pair_v2(&case.value).expect(&case.name);
    }
    for case in &cases.invalid_pairs {
        assert!(
            validate_storage_lifetime_materialization_pair_v2(&case.value).is_err(),
            "{}",
            case.name
        );
    }
    for index in &cases.runnable_reopen_pairs {
        validate_storage_lifetime_reopen_pair_v2(&cases.valid_pairs[*index].value)
            .expect(&cases.valid_pairs[*index].name);
    }
    for index in &cases.fail_closed_reopen_pairs {
        assert!(
            validate_storage_lifetime_reopen_pair_v2(&cases.valid_pairs[*index].value).is_err(),
            "{}",
            cases.valid_pairs[*index].name
        );
    }
    for case in &cases.valid_lifecycle_transitions {
        validate_storage_lifetime_lifecycle_transition_v2(
            case.operation,
            &case.previous,
            &case.next,
        )
        .expect(&case.name);
    }
    for case in &cases.invalid_lifecycle_transitions {
        assert!(
            validate_storage_lifetime_lifecycle_transition_v2(
                case.operation,
                &case.previous,
                &case.next,
            )
            .is_err(),
            "{}",
            case.name
        );
    }
    let direct_forge = cases
        .invalid_lifecycle_transitions
        .iter()
        .find(|case| case.name == "direct lifecycle input cannot set exhaustion")
        .expect("direct exhaustion forge case");
    for operation in [
        StorageLifetimeLifecycleOperationV2::Create,
        StorageLifetimeLifecycleOperationV2::Arm,
        StorageLifetimeLifecycleOperationV2::BatchSuccess,
        StorageLifetimeLifecycleOperationV2::PhaseAdvance,
        StorageLifetimeLifecycleOperationV2::Cancel,
        StorageLifetimeLifecycleOperationV2::Fail,
        StorageLifetimeLifecycleOperationV2::FailureRecordUnavailable,
        StorageLifetimeLifecycleOperationV2::FinalPublish,
        StorageLifetimeLifecycleOperationV2::FinalReset,
        StorageLifetimeLifecycleOperationV2::Append,
    ] {
        assert!(
            validate_storage_lifetime_lifecycle_transition_v2(
                operation,
                &direct_forge.previous,
                &direct_forge.next,
            )
            .is_err(),
            "public operation {operation:?} must not set scanExhausted false to true"
        );
    }
    for case in &cases.valid_verification_evidence {
        validate_storage_lifetime_verification_evidence_v2(&case.value).expect(&case.name);
    }
    for case in &cases.invalid_verification_evidence {
        assert!(
            validate_storage_lifetime_verification_evidence_v2(&case.value).is_err(),
            "{}",
            case.name
        );
    }
    for case in &cases.execution_models.valid {
        assert_eq!(case.outcome, "Suspended", "{}", case.name);
        assert!(!case.final_publication_same_call, "{}", case.name);
        let _ = case.verify_batch_committed;
    }
    for case in &cases.execution_models.invalid {
        assert!(
            case.verify_batch_committed
                && case.outcome == "Suspended"
                && case.final_publication_same_call,
            "{}",
            case.name
        );
    }
    for case in cases.invalid_preflight_mutations {
        let mut invalid = manifest.migration.preflight.clone();
        set_value_path(&mut invalid, &case.path, case.replacement);
        assert!(
            validate_storage_lifetime_v4_preflight_policy_v1(&invalid).is_err(),
            "{}",
            case.name
        );
    }
    for case in cases.invalid_policy_mutations {
        let mut invalid = manifest.bounded_verification.clone();
        set_value_path(&mut invalid, &case.path, case.replacement);
        assert!(
            validate_storage_lifetime_batch_policy_v2(&invalid).is_err(),
            "{}",
            case.name
        );
    }

    let sql = fs::read_to_string(root.join(&manifest.migration.file)).expect("read v4 migration");
    let normalized = sql.to_ascii_uppercase();
    assert!(
        sql.contains("state TEXT NOT NULL CHECK (state IN ('pending', 'attempting', 'failed'))")
    );
    assert!(sql.contains("WHEN 'verify' THEN 'verifySource'"));
    assert!(sql.contains("after_scan_rowid"));
    assert!(sql.contains("scan_exhausted INTEGER NOT NULL CHECK (scan_exhausted IN (0, 1))"));
    assert!(sql.contains("after_event_rowid,\n    0,\n    failure_kind"));
    assert!(!normalized.contains("ALTER TABLE MAINTENANCE_JOBS"));
    assert!(!normalized.contains("DROP TABLE MAINTENANCE_JOBS"));
    assert!(!normalized.contains("DELETE FROM EVENTS"));
    assert!(!normalized.contains("DROP TRIGGER"));
}

#[test]
fn storage_v5_freezes_bounded_projection_verification_and_catch_up() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/storage-v5");
    let manifest: ManifestV5 = read_json(root.join("manifest.json"));
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.storage_schema_version,
        STORAGE_LIFETIME_SCHEMA_VERSION_V5
    );
    assert_eq!(manifest.protocol_version, crate::PROTOCOL_VERSION);
    assert_eq!(manifest.payload_schema_version, 1);
    assert_eq!(manifest.migration.from_version, 4);
    assert_eq!(manifest.migration.to_version, 5);
    assert!(!manifest.migration.predecessor_migration_mutation_allowed);
    assert_eq!(
        manifest.migration.predecessor_migration_file,
        "../storage-v4/migration-v3-to-v4.sql"
    );
    assert_eq!(
        manifest.migration.mode,
        "exactV4AndAdmissiblePairPreflightThenTransactionalMetaAndJobRebuild"
    );
    assert_eq!(
        manifest.migration.idempotency,
        "exactStorageV5NoOpBeforeMigrationSql"
    );
    assert_eq!(
        manifest.migration.partial_extra_or_unsafe_pair,
        "failClosedBeforeMutation"
    );
    assert!(manifest.migration.tables_added.is_empty());
    assert_eq!(
        manifest.migration.tables_rebuilt,
        ["storage_schema_meta", "lifetime_materialization_job"]
    );
    assert_eq!(
        manifest.migration.preserved_tables,
        [
            "schema_meta",
            "app_instances",
            "events",
            "projects",
            "chats",
            "runs",
            "locks",
            "ledger_baselines",
            "operation_receipts",
            "chat_messages",
            "prepared_workspaces",
            "maintenance_jobs",
        ]
    );
    assert!(STORAGE_LIFETIME_INDEXES_V2.iter().copied().eq(manifest
        .migration
        .preserved_indexes
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.migration.preserved_triggers,
        [
            "events_are_append_only_update",
            "events_are_append_only_delete"
        ]
    );
    assert_eq!(manifest.migration.downgrade_policy, "openQuestion");
    validate_storage_lifetime_v5_preflight_policy_v1(&manifest.migration.preflight)
        .expect("exact v5 preflight");
    validate_storage_lifetime_batch_policy_v3(&manifest.bounded_projection_verification)
        .expect("exact v5 bounded projection policy");

    let cases: V5Cases = read_json(root.join("cases.json"));
    assert_eq!(cases.phase_order, STORAGE_LIFETIME_PHASES_V3);
    assert_eq!(
        cases.migration_matrix.job_states,
        STORAGE_LIFETIME_JOB_STATES_V2
    );
    assert_eq!(
        cases.migration_matrix.position_preserved_phases,
        STORAGE_LIFETIME_EVENT_ROWID_PHASES_V3
    );
    assert_eq!(
        cases.migration_matrix.projection_restart_phases,
        STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V2
    );
    assert_eq!(
        cases.migration_matrix.absent_materialization_states,
        ["pending", "ready"]
    );
    assert_eq!(cases.migration_matrix.state_mapping, "preserve");
    assert_eq!(
        cases.migration_matrix.position_preserved_mapping,
        "preservePhaseCursorAndExhaustion"
    );
    assert_eq!(
        cases.migration_matrix.projection_restart_mapping,
        "verifySourceZeroUnexhausted"
    );
    for case in &cases.reopen_pairs.valid {
        validate_storage_lifetime_reopen_pair_v3(&case.value).expect(&case.name);
    }
    for case in &cases.reopen_pairs.fail_closed {
        assert!(
            validate_storage_lifetime_reopen_pair_v3(&case.value).is_err(),
            "{}",
            case.name
        );
    }

    for (index, phase) in STORAGE_LIFETIME_PHASES_V3.iter().enumerate() {
        let cursor = serde_json::json!({
            "schemaVersion": 3,
            "cutoffEventRowid": 20,
            "phase": phase,
            "afterScanRowid": if index < STORAGE_LIFETIME_EVENT_ROWID_PHASES_V3.len() { 20 } else { 900 },
            "scanExhausted": true,
        });
        validate_storage_lifetime_cursor_v3(&cursor).expect(phase);
        if let Some(next_phase) = STORAGE_LIFETIME_PHASES_V3.get(index + 1) {
            let next = serde_json::json!({
                "schemaVersion": 3,
                "cutoffEventRowid": 20,
                "phase": next_phase,
                "afterScanRowid": 0,
                "scanExhausted": false,
            });
            validate_storage_lifetime_cursor_transition_v3(Some(&cursor), &next)
                .expect("exact phase order");
        }
    }

    let expected_operations = [
        "create",
        "arm",
        "batchSuccess",
        "phaseAdvance",
        "cancel",
        "fail",
        "failureRecordUnavailable",
        "finalPublish",
        "catchUpReset",
        "append",
    ];
    assert_eq!(cases.valid_lifecycle_operations, expected_operations);
    let pending_absent = serde_json::json!({"materializationState":"pending","job":null});
    let ready_absent = serde_json::json!({"materializationState":"ready","job":null});
    let created = v5_snapshot("pending", "receipts", 0, false, 20);
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::Create,
        &pending_absent,
        &created,
    )
    .expect("create");
    let armed = v5_snapshot("attempting", "receipts", 0, false, 20);
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::Arm,
        &created,
        &armed,
    )
    .expect("arm");
    let batch = v5_snapshot("pending", "receipts", 1, false, 20);
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::BatchSuccess,
        &armed,
        &batch,
    )
    .expect("batch success");
    let exhausted_project = v5_snapshot("pending", "verifyProjects", 900, true, 20);
    let entered_chats = v5_snapshot("pending", "verifyChats", 0, false, 20);
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::PhaseAdvance,
        &exhausted_project,
        &entered_chats,
    )
    .expect("phase advance");
    let attempting_project = v5_snapshot("attempting", "verifyProjects", 900, false, 20);
    let pending_project = v5_snapshot("pending", "verifyProjects", 900, false, 20);
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::Cancel,
        &attempting_project,
        &pending_project,
    )
    .expect("cancel");
    let failed_project = v5_snapshot("failed", "verifyProjects", 900, false, 20);
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::Fail,
        &attempting_project,
        &failed_project,
    )
    .expect("fail");
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::FailureRecordUnavailable,
        &attempting_project,
        &attempting_project,
    )
    .expect("attempting sentinel");
    let final_cursor = v5_snapshot("pending", "verifyBaselines", 901, true, 20);
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::FinalPublish,
        &final_cursor,
        &ready_absent,
    )
    .expect("final publish");
    validate_storage_lifetime_lifecycle_transition_v3(
        StorageLifetimeLifecycleOperationV3::Append,
        &ready_absent,
        &pending_absent,
    )
    .expect("append invalidates ready");
    for case in &cases.valid_catch_up_resets {
        let previous =
            v5_snapshot_for_case(case, case.previous_cutoff, case.previous_phase.as_str());
        let next = v5_snapshot_for_case(case, case.observed_cutoff, "receipts");
        validate_storage_lifetime_lifecycle_transition_v3(
            StorageLifetimeLifecycleOperationV3::CatchUpReset,
            &previous,
            &next,
        )
        .expect(&case.name);
    }
    for case in &cases.invalid_catch_up_resets {
        let previous =
            v5_snapshot_for_case(case, case.previous_cutoff, case.previous_phase.as_str());
        let next = v5_snapshot_for_case(case, case.observed_cutoff, "receipts");
        assert!(
            validate_storage_lifetime_lifecycle_transition_v3(
                StorageLifetimeLifecycleOperationV3::CatchUpReset,
                &previous,
                &next,
            )
            .is_err(),
            "{}",
            case.name
        );
    }
    let direct_exhaustion_previous = v5_snapshot("pending", "verifyProjects", 0, false, 20);
    let direct_exhaustion_next = v5_snapshot("pending", "verifyProjects", 0, true, 20);
    for operation in [
        StorageLifetimeLifecycleOperationV3::Create,
        StorageLifetimeLifecycleOperationV3::Arm,
        StorageLifetimeLifecycleOperationV3::BatchSuccess,
        StorageLifetimeLifecycleOperationV3::PhaseAdvance,
        StorageLifetimeLifecycleOperationV3::Cancel,
        StorageLifetimeLifecycleOperationV3::Fail,
        StorageLifetimeLifecycleOperationV3::FailureRecordUnavailable,
        StorageLifetimeLifecycleOperationV3::FinalPublish,
        StorageLifetimeLifecycleOperationV3::CatchUpReset,
        StorageLifetimeLifecycleOperationV3::Append,
    ] {
        assert!(
            validate_storage_lifetime_lifecycle_transition_v3(
                operation,
                &direct_exhaustion_previous,
                &direct_exhaustion_next,
            )
            .is_err(),
            "{operation:?} cannot forge private exhaustion"
        );
    }
    for case in cases.invalid_preflight_mutations {
        let mut invalid = manifest.migration.preflight.clone();
        set_value_path(&mut invalid, &case.path, case.replacement);
        assert!(
            validate_storage_lifetime_v5_preflight_policy_v1(&invalid).is_err(),
            "{}",
            case.name
        );
    }
    for case in cases.invalid_policy_mutations {
        let mut invalid = manifest.bounded_projection_verification.clone();
        set_value_path(&mut invalid, &case.path, case.replacement);
        assert!(
            validate_storage_lifetime_batch_policy_v3(&invalid).is_err(),
            "{}",
            case.name
        );
    }

    let sql = fs::read_to_string(root.join(&manifest.migration.file)).expect("read v5 migration");
    let normalized = sql.to_ascii_uppercase();
    assert!(
        sql.contains("storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 5)")
    );
    assert!(sql.contains("'verifyProjects', 'verifyChats', 'verifyRuns', 'verifyLocks'"));
    assert!(sql.contains("WHEN 'verifyReceipts' THEN 'verifySource'"));
    assert!(sql.contains("ELSE after_scan_rowid"));
    assert!(sql.contains("ELSE scan_exhausted"));
    assert!(!normalized.contains("DELETE FROM EVENTS"));
    assert!(!normalized.contains("ALTER TABLE PROJECTS"));
    assert!(!normalized.contains("DROP TRIGGER"));
}

#[test]
fn storage_v6_freezes_source_index_migration_and_sixteen_bounded_phases() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/storage-v6");
    let manifest: ManifestV6 = read_json(root.join("manifest.json"));
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.storage_schema_version,
        STORAGE_LIFETIME_SCHEMA_VERSION_V6
    );
    assert_eq!(manifest.protocol_version, crate::PROTOCOL_VERSION);
    assert_eq!(manifest.payload_schema_version, 1);
    assert_eq!(manifest.migration.from_version, 5);
    assert_eq!(manifest.migration.to_version, 6);
    assert!(!manifest.migration.predecessor_migration_mutation_allowed);
    assert_eq!(
        manifest.migration.predecessor_migration_file,
        "../storage-v5/migration-v4-to-v5.sql"
    );
    assert_eq!(
        manifest.migration.mode,
        "exactV5AndAdmissiblePairPreflightThenSchemaOnlySourceIndexAndJobReset"
    );
    assert_eq!(
        manifest.migration.idempotency,
        "exactStorageV6NoOpBeforeMigrationSql"
    );
    assert_eq!(
        manifest.migration.partial_extra_or_unsafe_pair,
        "failClosedBeforeMutation"
    );
    assert_eq!(manifest.migration.history_backfill, "none");
    assert_eq!(manifest.migration.tables_added, ["event_source_index"]);
    assert_eq!(
        manifest.migration.tables_rebuilt,
        ["storage_schema_meta", "lifetime_materialization_job"]
    );
    assert_eq!(
        manifest.migration.indexes_added,
        [
            "event_source_index_aggregate_latest",
            "event_source_index_message_unique"
        ]
    );
    assert_eq!(
        manifest.migration.triggers_added,
        [
            "event_source_index_requires_exact_event",
            "event_source_index_append_only_update",
            "event_source_index_append_only_delete"
        ]
    );
    assert_eq!(manifest.migration.preserved_tables.len(), 12);
    assert_eq!(
        manifest.migration.preserved_indexes,
        STORAGE_LIFETIME_INDEXES_V2
    );
    assert_eq!(
        manifest.migration.preserved_triggers,
        [
            "events_are_append_only_update",
            "events_are_append_only_delete"
        ]
    );
    assert_eq!(manifest.migration.downgrade_policy, "openQuestion");
    validate_storage_lifetime_v6_preflight_policy_v1(&manifest.migration.preflight)
        .expect("exact v6 preflight");
    validate_storage_lifetime_batch_policy_v4(&manifest.indexed_bounded_verification)
        .expect("exact v6 indexed bounded policy");

    let cases: V6Cases = read_json(root.join("cases.json"));
    assert_eq!(cases.phase_order, STORAGE_LIFETIME_PHASES_V4);
    assert_eq!(
        cases.migration_matrix.job_states,
        STORAGE_LIFETIME_JOB_STATES_V2
    );
    assert_eq!(cases.migration_matrix.v5_phases, STORAGE_LIFETIME_PHASES_V3);
    assert_eq!(
        cases.migration_matrix.present_job_mapping,
        "indexSourceZeroUnexhausted"
    );
    assert_eq!(
        cases
            .migration_matrix
            .state_identity_timestamps_failure_evidence,
        "preserve"
    );
    assert_eq!(cases.migration_matrix.absent_mappings.len(), 2);
    assert_eq!(cases.migration_matrix.history_backfill_rows, 0);
    assert_eq!(
        cases.source_index.columns,
        STORAGE_EVENT_SOURCE_INDEX_COLUMNS_V1
    );
    assert_eq!(cases.source_index.valid_rows.len(), 5);
    assert_eq!(cases.source_index.source_events.len(), 6);
    for row in &cases.source_index.valid_rows {
        assert!(!row.name.is_empty());
        let tuple = row.value.as_array().expect("source row tuple");
        assert_eq!(tuple.len(), 9);
        let entry = serde_json::json!({
            "eventRowid": tuple[0],
            "eventId": tuple[1],
            "operationId": tuple[2],
            "aggregateType": tuple[3],
            "aggregateId": tuple[4],
            "aggregateSequence": tuple[5],
            "introducedMessageId": tuple[6],
            "chatDurableSequence": tuple[7],
            "chatSnapshotMessageId": tuple[8],
        });
        validate_storage_event_source_index_entry_v1(&entry).expect(&row.name);
        let event_rowid = tuple[0].as_u64().expect("event rowid");
        let source = cases
            .source_index
            .source_events
            .iter()
            .find(|source| source.event_rowid == event_rowid)
            .expect("exact source event");
        assert!(!source.name.is_empty());
        validate_storage_event_source_index_binding_v1(event_rowid, &entry, &source.event)
            .expect(&row.name);
    }
    let project_source = &cases.source_index.source_events[0];
    let chat_source = &cases.source_index.source_events[1];
    let project_revision = &cases.source_index.source_events[2];
    let project_tuple = cases.source_index.valid_rows[0]
        .value
        .as_array()
        .expect("project source tuple");
    let project_entry = serde_json::json!({
        "eventRowid": project_tuple[0],
        "eventId": project_tuple[1],
        "operationId": project_tuple[2],
        "aggregateType": project_tuple[3],
        "aggregateId": project_tuple[4],
        "aggregateSequence": project_tuple[5],
        "introducedMessageId": project_tuple[6],
        "chatDurableSequence": project_tuple[7],
        "chatSnapshotMessageId": project_tuple[8],
    });
    let mut invalid_bindings = Vec::new();
    for (field, replacement) in [
        ("eventRowid", Value::from(chat_source.event_rowid)),
        ("eventId", Value::from(chat_source.event.event_id.clone())),
        (
            "operationId",
            Value::from(chat_source.event.operation_id.clone()),
        ),
        (
            "aggregateType",
            Value::from(chat_source.event.aggregate_type.clone()),
        ),
        (
            "aggregateId",
            Value::from(chat_source.event.aggregate_id.clone()),
        ),
        (
            "aggregateSequence",
            Value::from(project_revision.event.aggregate_sequence),
        ),
        ("introducedMessageId", Value::from("message_a")),
        ("chatDurableSequence", Value::from(1)),
        ("chatSnapshotMessageId", Value::from("message_a")),
    ] {
        let mut invalid = project_entry.clone();
        invalid
            .as_object_mut()
            .expect("entry object")
            .insert(field.to_string(), replacement);
        invalid_bindings.push(invalid);
    }
    let mut cross_event_mix = project_entry.clone();
    let cross_event_object = cross_event_mix.as_object_mut().expect("cross event entry");
    cross_event_object.insert(
        "eventId".to_string(),
        Value::from(chat_source.event.event_id.clone()),
    );
    cross_event_object.insert(
        "operationId".to_string(),
        Value::from(project_revision.event.operation_id.clone()),
    );
    invalid_bindings.push(cross_event_mix);
    assert_eq!(
        invalid_bindings.len(),
        cases.source_index.invalid_binding_fields.len()
    );
    for (index, invalid) in invalid_bindings.iter().enumerate() {
        assert!(
            validate_storage_event_source_index_binding_v1(
                project_source.event_rowid,
                invalid,
                &project_source.event,
            )
            .is_err(),
            "{}",
            cases.source_index.invalid_binding_fields[index]
        );
    }
    assert_eq!(
        cases.source_index.duplicate_conflicts,
        [
            "event_rowid",
            "event_id",
            "operation_id",
            "aggregateTuple",
            "introduced_message_id"
        ]
    );
    assert_eq!(
        cases.source_index.mutation_guards,
        ["insert", "update", "delete"]
    );
    let entry_for_rowid = |event_rowid: u64| {
        let tuple = cases
            .source_index
            .valid_rows
            .iter()
            .find_map(|row| {
                let tuple = row.value.as_array().expect("source row tuple");
                (tuple[0].as_u64() == Some(event_rowid)).then_some(tuple)
            })
            .expect("indexed transition row");
        serde_json::json!({
            "eventRowid": tuple[0], "eventId": tuple[1], "operationId": tuple[2],
            "aggregateType": tuple[3], "aggregateId": tuple[4],
            "aggregateSequence": tuple[5], "introducedMessageId": tuple[6],
            "chatDurableSequence": tuple[7], "chatSnapshotMessageId": tuple[8],
        })
    };
    let source_for_rowid = |event_rowid: u64| {
        &cases
            .source_index
            .source_events
            .iter()
            .find(|source| source.event_rowid == event_rowid)
            .expect("transition source")
            .event
    };
    let first = entry_for_rowid(2);
    validate_storage_event_source_index_transition_v1(2, &first, source_for_rowid(2), None, None)
        .expect("first message A");
    let repeated = entry_for_rowid(4);
    validate_storage_event_source_index_transition_v1(
        4,
        &repeated,
        source_for_rowid(4),
        Some(&first),
        None,
    )
    .expect("repeat message A");
    let null_snapshot = entry_for_rowid(5);
    validate_storage_event_source_index_transition_v1(
        5,
        &null_snapshot,
        source_for_rowid(5),
        Some(&repeated),
        None,
    )
    .expect("null snapshot");
    let second = entry_for_rowid(6);
    validate_storage_event_source_index_transition_v1(
        6,
        &second,
        source_for_rowid(6),
        Some(&null_snapshot),
        None,
    )
    .expect("second message B");
    validate_storage_event_source_index_transition_v1(
        6,
        &second,
        source_for_rowid(6),
        None,
        Some(source_for_rowid(5)),
    )
    .expect("migration-pending exact predecessor event fallback");

    let mut double_introduction = repeated.clone();
    double_introduction
        .as_object_mut()
        .expect("repeat entry")
        .insert("introducedMessageId".into(), Value::from("message_a"));
    assert!(validate_storage_event_source_index_transition_v1(
        4,
        &double_introduction,
        source_for_rowid(4),
        Some(&first),
        None,
    )
    .is_err());
    assert!(validate_storage_event_source_index_transition_v1(
        6,
        &second,
        source_for_rowid(6),
        None,
        None,
    )
    .is_err());
    let source_with_payload = |base: &LedgerEventV1, durable: u64, message: Value| {
        let mut source = base.clone();
        source.canonical_json_payload = serde_json::to_string(&serde_json::json!({
            "durable_event_sequence": durable,
            "last_message": message,
        }))
        .expect("synthetic chat source payload");
        source
    };
    let entry_with_state = |base: &Value, durable: u64, introduced: Value, snapshot: Value| {
        let mut entry = base.clone();
        let object = entry.as_object_mut().expect("chat source entry");
        object.insert("chatDurableSequence".into(), Value::from(durable));
        object.insert("introducedMessageId".into(), introduced);
        object.insert("chatSnapshotMessageId".into(), snapshot);
        entry
    };
    let mut message_b = serde_json::from_str::<Value>(&source_for_rowid(6).canonical_json_payload)
        .expect("message B payload")["last_message"]
        .clone();

    let decrease_source = source_with_payload(source_for_rowid(5), 0, Value::Null);
    let decrease_entry = entry_with_state(&null_snapshot, 0, Value::Null, Value::Null);
    assert!(validate_storage_event_source_index_transition_v1(
        5,
        &decrease_entry,
        &decrease_source,
        Some(&repeated),
        None,
    )
    .is_err());

    message_b["eventSequence"] = Value::from(3);
    let jump_source = source_with_payload(source_for_rowid(6), 3, message_b.clone());
    let jump_entry = entry_with_state(
        &second,
        3,
        Value::from("message_b"),
        Value::from("message_b"),
    );
    assert!(validate_storage_event_source_index_transition_v1(
        6,
        &jump_entry,
        &jump_source,
        Some(&null_snapshot),
        None,
    )
    .is_err());

    let missing_message_source = source_with_payload(source_for_rowid(6), 2, Value::Null);
    let missing_message_entry = entry_with_state(&second, 2, Value::Null, Value::Null);
    assert!(validate_storage_event_source_index_transition_v1(
        6,
        &missing_message_entry,
        &missing_message_source,
        Some(&null_snapshot),
        None,
    )
    .is_err());

    let mut changed_message =
        serde_json::from_str::<Value>(&source_for_rowid(4).canonical_json_payload)
            .expect("repeated message payload")["last_message"]
            .clone();
    changed_message["messageId"] = Value::from("message_changed");
    let changed_source = source_with_payload(source_for_rowid(4), 1, changed_message);
    let changed_entry = entry_with_state(&repeated, 1, Value::Null, Value::from("message_changed"));
    assert!(validate_storage_event_source_index_transition_v1(
        4,
        &changed_entry,
        &changed_source,
        Some(&first),
        None,
    )
    .is_err());

    let wrong_sequence_source = source_with_payload(source_for_rowid(6), 2, message_b.clone());
    assert!(validate_storage_event_source_index_transition_v1(
        6,
        &second,
        &wrong_sequence_source,
        Some(&null_snapshot),
        None,
    )
    .is_err());
    message_b["eventSequence"] = Value::from(2);
    message_b["chatId"] = Value::from("chat_other");
    let wrong_chat_source = source_with_payload(source_for_rowid(6), 2, message_b);
    assert!(validate_storage_event_source_index_transition_v1(
        6,
        &second,
        &wrong_chat_source,
        Some(&null_snapshot),
        None,
    )
    .is_err());
    assert_eq!(cases.source_index.valid_transitions.len(), 5);
    assert_eq!(cases.source_index.invalid_transitions.len(), 9);
    assert_eq!(cases.query_plans.len(), 7);
    for plan in &cases.query_plans {
        assert!(
            plan.sql.starts_with("EXPLAIN QUERY PLAN SELECT "),
            "{}",
            plan.name
        );
        assert!(plan.detail.starts_with("SEARCH "), "{}", plan.name);
        assert!(!plan.detail.starts_with("SCAN "), "{}", plan.name);
    }
    assert_eq!(
        cases.workspace_provenance.canonical_payload.len(),
        cases.workspace_provenance.payload_bytes
    );
    assert_eq!(
        cases.workspace_provenance.max_payload_bytes,
        STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1
    );
    assert!(
        cases.workspace_provenance.payload_bytes as u64
            <= cases.workspace_provenance.max_payload_bytes
    );
    let project: Value = serde_json::from_str(&cases.workspace_provenance.canonical_payload)
        .expect("project payload");
    assert_eq!(
        project.get("project_id").and_then(Value::as_str),
        Some(cases.workspace_provenance.project_id.as_str())
    );
    assert_eq!(
        cases.workspace_provenance.comparison,
        "rustExactCanonicalProjectPayloadAndWorkspaceProjectId"
    );
    assert_eq!(
        validate_storage_lifetime_workspace_project_source_v1(
            &cases.workspace_provenance.project_id,
            "project",
            &cases.workspace_provenance.project_id,
            &cases.workspace_provenance.canonical_payload,
        )
        .expect("exact workspace provenance"),
        project
    );
    assert!(validate_storage_lifetime_workspace_project_source_v1(
        &cases.workspace_provenance.project_id,
        "chat",
        &cases.workspace_provenance.project_id,
        &cases.workspace_provenance.canonical_payload,
    )
    .is_err());
    let oversized = format!(
        "{{\"canonical_path\":\"/{}\",\"project_id\":\"{}\"}}",
        "x".repeat(1_048_576),
        cases.workspace_provenance.project_id
    );
    assert!(validate_storage_lifetime_workspace_project_source_v1(
        &cases.workspace_provenance.project_id,
        "project",
        &cases.workspace_provenance.project_id,
        &oversized,
    )
    .is_err());
    assert!(!cases.workspace_provenance.removals_legal);
    assert_eq!(
        cases.workspace_provenance.stop_when,
        "aggregateRemovalOrTombstoneBecomesLegalUntilSourceIndexSemanticsAreVersioned"
    );

    for case in &cases.reopen_pairs.valid {
        validate_storage_lifetime_reopen_pair_v4(&case.value).expect(&case.name);
    }
    for case in &cases.reopen_pairs.fail_closed {
        assert!(
            validate_storage_lifetime_reopen_pair_v4(&case.value).is_err(),
            "{}",
            case.name
        );
    }
    assert_eq!(
        cases.valid_lifecycle_operations,
        [
            "create",
            "arm",
            "batchSuccess",
            "phaseAdvance",
            "cancel",
            "fail",
            "failureRecordUnavailable",
            "finalPublish",
            "catchUpReset",
            "append",
        ]
    );
    for (index, phase) in STORAGE_LIFETIME_PHASES_V4.iter().enumerate() {
        let cursor = serde_json::json!({
            "schemaVersion": 4,
            "cutoffEventRowid": 20,
            "phase": phase,
            "afterScanRowid": if index < STORAGE_LIFETIME_EVENT_ROWID_PHASES_V4.len() { 20 } else { 900 },
            "scanExhausted": true,
        });
        validate_storage_lifetime_cursor_v4(&cursor).expect(phase);
        if let Some(next_phase) = STORAGE_LIFETIME_PHASES_V4.get(index + 1) {
            let next = serde_json::json!({
                "schemaVersion": 4,
                "cutoffEventRowid": 20,
                "phase": next_phase,
                "afterScanRowid": 0,
                "scanExhausted": false,
            });
            validate_storage_lifetime_cursor_transition_v4(Some(&cursor), &next)
                .expect("exact v6 phase order");
        }
    }

    let pending_absent = serde_json::json!({"materializationState":"pending","job":null});
    let ready_absent = serde_json::json!({"materializationState":"ready","job":null});
    let created = v6_snapshot("pending", "indexSource", 0, false, 20);
    validate_storage_lifetime_lifecycle_transition_v4(
        StorageLifetimeLifecycleOperationV4::Create,
        &pending_absent,
        &created,
    )
    .expect("create v6 job");
    let armed = v6_snapshot("attempting", "indexSource", 0, false, 20);
    validate_storage_lifetime_lifecycle_transition_v4(
        StorageLifetimeLifecycleOperationV4::Arm,
        &created,
        &armed,
    )
    .expect("arm v6 batch");
    validate_storage_lifetime_lifecycle_transition_v4(
        StorageLifetimeLifecycleOperationV4::BatchSuccess,
        &armed,
        &v6_snapshot("pending", "indexSource", 1, false, 20),
    )
    .expect("commit source index batch");
    validate_storage_lifetime_lifecycle_transition_v4(
        StorageLifetimeLifecycleOperationV4::CatchUpReset,
        &v6_snapshot("pending", "verifySourceIndex", 20, true, 20),
        &v6_snapshot("pending", "indexSource", 0, false, 21),
    )
    .expect("catch up restarts source index");
    validate_storage_lifetime_lifecycle_transition_v4(
        StorageLifetimeLifecycleOperationV4::FinalPublish,
        &v6_snapshot("pending", "verifyBaselines", 900, true, 20),
        &ready_absent,
    )
    .expect("final publish");
    validate_storage_lifetime_lifecycle_transition_v4(
        StorageLifetimeLifecycleOperationV4::Append,
        &ready_absent,
        &pending_absent,
    )
    .expect("append invalidates ready");

    for case in cases.invalid_preflight_mutations {
        let mut invalid = manifest.migration.preflight.clone();
        set_value_path(&mut invalid, &case.path, case.replacement);
        assert!(
            validate_storage_lifetime_v6_preflight_policy_v1(&invalid).is_err(),
            "{}",
            case.name
        );
    }
    for case in cases.invalid_policy_mutations {
        let mut invalid = manifest.indexed_bounded_verification.clone();
        set_value_path(&mut invalid, &case.path, case.replacement);
        assert!(
            validate_storage_lifetime_batch_policy_v4(&invalid).is_err(),
            "{}",
            case.name
        );
    }

    let sql = fs::read_to_string(root.join(&manifest.migration.file)).expect("read v6 migration");
    let normalized = sql.to_ascii_uppercase();
    assert!(sql.contains("CREATE TABLE event_source_index"));
    assert!(sql.contains("CREATE INDEX event_source_index_aggregate_latest"));
    assert!(sql.contains("CREATE UNIQUE INDEX event_source_index_message_unique"));
    assert!(sql.contains("WHERE introduced_message_id IS NOT NULL"));
    assert!(sql.contains("FOREIGN KEY (event_id) REFERENCES events(event_id)"));
    assert!(sql.contains("FOREIGN KEY (operation_id) REFERENCES events(operation_id)"));
    assert!(sql.contains("FOREIGN KEY (aggregate_type, aggregate_id, aggregate_sequence)"));
    assert!(sql.contains("CREATE TRIGGER event_source_index_requires_exact_event"));
    assert!(sql.contains("source.rowid = NEW.event_rowid"));
    assert!(sql.contains("'$.durable_event_sequence'"));
    assert!(sql.contains("'$.last_message.messageId'"));
    assert!(sql.contains("'$.last_message.eventSequence'"));
    assert!(sql.contains("'$.last_message.chatId'"));
    assert!(sql.contains("CREATE TRIGGER event_source_index_append_only_update"));
    assert!(sql.contains("CREATE TRIGGER event_source_index_append_only_delete"));
    assert!(sql.contains("'indexSource', 'verifySourceIndex'"));
    assert!(sql.contains("CASE materialization_state WHEN 'ready' THEN 'pending'"));
    assert!(!normalized.contains("INSERT INTO EVENT_SOURCE_INDEX"));
    assert!(!normalized.contains("DELETE FROM EVENTS"));
    assert!(!normalized.contains("ALTER TABLE PROJECTS"));
    assert!(!normalized.contains("DROP TRIGGER EVENTS_"));
}

fn v6_snapshot(state: &str, phase: &str, after: u64, exhausted: bool, cutoff: u64) -> Value {
    serde_json::json!({
        "materializationState": "pending",
        "job": {
            "singleton": 1,
            "jobId": "lifetime-v6",
            "state": state,
            "cutoffEventRowid": cutoff,
            "phase": phase,
            "afterScanRowid": after,
            "scanExhausted": exhausted,
            "failureKind": if state == "failed" { Value::String("contractValidation".to_string()) } else { Value::Null },
            "createdAt": "2026-08-28T13:00:00Z",
            "updatedAt": if state == "failed" { "2026-08-28T13:02:00Z" } else { "2026-08-28T13:01:00Z" },
            "completedAt": if state == "failed" { Value::String("2026-08-28T13:02:00Z".to_string()) } else { Value::Null },
        }
    })
}

fn v5_snapshot_for_case(case: &V5CatchUpCase, cutoff: u64, phase: &str) -> Value {
    let is_next = phase == "receipts" && cutoff == case.observed_cutoff;
    let state = if is_next {
        "pending"
    } else {
        case.previous_state.as_str()
    };
    serde_json::json!({
        "materializationState": "pending",
        "job": {
            "singleton": 1,
            "jobId": "lifetime-v5",
            "state": state,
            "cutoffEventRowid": cutoff,
            "phase": phase,
            "afterScanRowid": if is_next { 0 } else { case.previous_after_scan_rowid },
            "scanExhausted": if is_next { false } else { case.previous_scan_exhausted },
            "failureKind": if state == "failed" { Value::String("contractValidation".to_string()) } else { Value::Null },
            "createdAt": "2026-08-28T12:00:00Z",
            "updatedAt": if state == "failed" { "2026-08-28T12:02:00Z" } else { "2026-08-28T12:01:00Z" },
            "completedAt": if state == "failed" { Value::String("2026-08-28T12:02:00Z".to_string()) } else { Value::Null },
        }
    })
}

fn v5_snapshot(state: &str, phase: &str, after: u64, exhausted: bool, cutoff: u64) -> Value {
    serde_json::json!({
        "materializationState": "pending",
        "job": {
            "singleton": 1,
            "jobId": "lifetime-v5",
            "state": state,
            "cutoffEventRowid": cutoff,
            "phase": phase,
            "afterScanRowid": after,
            "scanExhausted": exhausted,
            "failureKind": if state == "failed" { Value::String("contractValidation".to_string()) } else { Value::Null },
            "createdAt": "2026-08-28T12:00:00Z",
            "updatedAt": if state == "failed" { "2026-08-28T12:02:00Z" } else { "2026-08-28T12:01:00Z" },
            "completedAt": if state == "failed" { Value::String("2026-08-28T12:02:00Z".to_string()) } else { Value::Null },
        }
    })
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

fn manifest_v3() -> (PathBuf, ManifestV3) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/storage-v3");
    let manifest = read_json(root.join("manifest.json"));
    (root, manifest)
}

fn manifest_v4() -> (PathBuf, ManifestV4) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/storage-v4");
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
        if current.is_object() {
            current = current
                .as_object_mut()
                .and_then(|object| object.get_mut(segment))
                .expect("policy mutation parent");
        } else {
            current = current
                .as_array_mut()
                .and_then(|array| array.get_mut(segment.parse::<usize>().expect("array index")))
                .expect("policy mutation array parent");
        }
    }
    if let Some(object) = current.as_object_mut() {
        object.insert(last.clone(), replacement);
    } else if let Some(array) = current.as_array_mut() {
        array[last.parse::<usize>().expect("array index")] = replacement;
    } else {
        panic!("policy mutation target");
    }
}
