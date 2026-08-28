use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::fmt::{self, Display};

use crate::project_v1::sha256_file_version_v1;
use crate::storage_v1::{
    canonical_json_v1, classify_operation_v1, LedgerEventV1, OperationDecisionV1,
};
use crate::MAX_SEQUENCE;

pub const STORAGE_LIFETIME_SCHEMA_VERSION_V1: u32 = 2;
pub const STORAGE_STATUS_SCHEMA_VERSION_V1: u32 = 1;
pub const STORAGE_STATUS_METHOD_V1: &str = "storage.status";
pub const STORAGE_STATUS_BRIDGE_VERSION_V1: u32 = 6;
pub const STORAGE_STATUS_BRIDGE_METHOD_V1: &str = "getStorageStatus";
pub const STORAGE_LIFETIME_TABLES_V2: &[&str] = &[
    "storage_schema_meta",
    "ledger_baselines",
    "operation_receipts",
    "chat_messages",
    "prepared_workspaces",
    "maintenance_jobs",
];
pub const STORAGE_LIFETIME_INDEXES_V2: &[&str] = &[
    "chats_identity_project",
    "ledger_baselines_cutoff",
    "operation_receipts_aggregate_sequence",
    "chat_messages_chat_sequence",
    "prepared_workspaces_project_state",
    "maintenance_jobs_status_created",
];
pub const OPERATION_RECEIPT_DIGEST_ALGORITHM_V2: &str = "sha256";
pub const OPERATION_RECEIPT_DIGEST_VERSION_V2: u32 = 1;
pub const OPERATION_RECEIPT_DIGEST_FORMAT_V2: &str = "lowercaseHex64";
pub const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1: u32 = 1;
pub const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V1: u32 = 1;
pub const STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1: usize = 100;
pub const STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1: u64 = 1_048_576;
pub const STORAGE_LIFETIME_KEYSET_SCAN_PREDICATE_V1: &str =
    "rowid > afterEventRowid AND rowid <= cutoffEventRowid";
pub const STORAGE_LIFETIME_KEYSET_SCAN_ORDER_V1: &str = "rowid";
pub const STORAGE_LIFETIME_PHASES_V1: &[&str] = &[
    "receipts",
    "messages",
    "workspaces",
    "baselines",
    "verify",
    "compact",
];
pub const LOSSLESS_COMPACTION_STEPS_V1: &[&str] = &[
    "beginImmediate",
    "freezeCutoff",
    "materializeOperationReceipts",
    "materializeChatMessagesAndPreparedWorkspaces",
    "materializeLedgerBaselines",
    "verifyBaselineTailProjectionEquivalent",
    "verifyOperationDuplicateConflictEquivalent",
    "verifyNamespaceAndRecoveryEquivalent",
    "deleteVerifiedLedgerCutoff",
    "verifyAppendOnlyGuardRestored",
    "commit",
    "checkpointBestEffort",
];
pub const NEVER_CLEANUP_STATES_V1: &[&str] = &[
    "activeRun",
    "cancellingRun",
    "recoveryRequiredRun",
    "unresolvedRecovery",
    "poisonedLock",
    "writerLock",
    "pendingIntent",
    "pendingSaveSession",
    "incompleteOperation",
    "activeWorkspace",
    "boundWorkspace",
    "roleSettings",
    "userChatHistory",
    "userEvidence",
    "identityMismatch",
    "orphanOrSymlinkUnknown",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLifetimeContractError {
    detail: String,
}

impl StorageLifetimeContractError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for StorageLifetimeContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for StorageLifetimeContractError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageStatusRequestV1 {
    pub schema_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageDatabaseFactsV1 {
    pub logical_bytes: u64,
    pub physical_bytes: u64,
    pub wal_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLedgerTailFactsV1 {
    pub event_count: u64,
    pub payload_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageSnapshotAggregateFactsV1 {
    pub count: u64,
    pub payload_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoragePressureCollectionStatusV1 {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StoragePressureFactsV1 {
    pub collection_status: StoragePressureCollectionStatusV1,
    pub volume_capacity_bytes: Option<u64>,
    pub volume_available_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionAttemptStatusV1 {
    NeverRun,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionStatusV1 {
    pub last_attempt_status: StorageCompactionAttemptStatusV1,
    pub last_attempted_at: Option<String>,
    pub last_successful_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PhysicalCleanupDisabledReasonV1 {
    UserPolicyNotConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PhysicalCleanupStatusV1 {
    pub enabled: bool,
    pub reason: PhysicalCleanupDisabledReasonV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageStatusV1 {
    pub schema_version: u32,
    pub collected_at: String,
    pub database: StorageDatabaseFactsV1,
    pub ledger_tail: StorageLedgerTailFactsV1,
    pub snapshot_aggregates: StorageSnapshotAggregateFactsV1,
    pub pressure_facts: StoragePressureFactsV1,
    pub compaction: StorageCompactionStatusV1,
    pub physical_cleanup: PhysicalCleanupStatusV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OperationReceiptV2 {
    pub operation_id: String,
    pub event_id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_sequence: u64,
    pub event_type: String,
    pub payload_schema_version: u32,
    pub canonical_payload_json: String,
    pub canonical_payload_digest_algorithm: String,
    pub canonical_payload_digest_version: u32,
    pub canonical_payload_digest_format: String,
    pub canonical_payload_digest: String,
    pub event_timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimePhaseV1 {
    Receipts,
    Messages,
    Workspaces,
    Baselines,
    Verify,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeCursorV1 {
    pub schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV1,
    pub after_event_rowid: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeBatchPolicyV1 {
    schema_version: u32,
    cases_file: String,
    guards: StorageLifetimeBatchGuardsV1,
    cursor: StorageLifetimeCursorPolicyV1,
    scan: StorageLifetimeScanPolicyV1,
    source: StorageLifetimeSourcePolicyV1,
    transaction: StorageLifetimeTransactionPolicyV1,
    cancellation: StorageLifetimeCancellationPolicyV1,
    job: StorageLifetimeJobPolicyV1,
    append: StorageLifetimeAppendPolicyV1,
    final_publication: StorageLifetimeFinalPublicationPolicyV1,
    rollback: StorageLifetimeRollbackPolicyV1,
    activation: StorageLifetimeActivationPolicyV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeBatchGuardsV1 {
    max_source_rows_per_batch: u64,
    max_canonical_source_payload_bytes_per_batch: u64,
    single_source_payload_oversize: String,
    caller_configurable: bool,
    applies_to_phases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeCursorPolicyV1 {
    schema_version: u32,
    phases: Vec<String>,
    scan_phases: Vec<String>,
    phase_start_after_event_rowid: u64,
    same_phase_progress: String,
    phase_transition: String,
    compact: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeScanPolicyV1 {
    predicate: String,
    order_by: String,
    offset_allowed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeSourcePolicyV1 {
    artifact_authority: String,
    workspace_and_baseline_source: String,
    repeated_projection_snapshots: String,
    live_projection_use: String,
    unbounded_collection_allowed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeTransactionPolicyV1 {
    cadence: String,
    committed_derived_rows_may_persist: bool,
    committed_progress_state: String,
    implementation_may_commit_running: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeCancellationPolicyV1 {
    before_begin: String,
    checks: Vec<String>,
    during_transaction: String,
    outcome: String,
    crash: String,
    committed_batch_wins_after_final_precommit_check: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeJobPolicyV1 {
    table: String,
    job_kind: String,
    cursor_column: String,
    cursor_encoding: String,
    maximum_active_jobs: u64,
    active_states: Vec<String>,
    resumable_state: String,
    in_memory_only_outcome: String,
    persisted_maintenance_job_fail_closed_states: Vec<String>,
    terminal_maintenance_job_states: Vec<String>,
    failed_maintenance_job_role: String,
    storage_meta_fail_closed_states: Vec<String>,
    materializing_location: String,
    sql_unique_constraint_present: bool,
    single_sqlite_owner_required: bool,
    transaction_serialization: String,
    transaction_boundary_validation: String,
    second_job_creation_allowed_when_any_lifetime_job_exists: bool,
    initial_transition: String,
    append_reactivation: String,
    batch_commit: String,
    cancellation_or_current_failure: String,
    final_publication_transition: String,
    post_rollback_failure_record: String,
    automatic_retry_enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeAppendPolicyV1 {
    ready_invalidation: String,
    ready_publication: String,
    append_during_materialization: String,
    ready_regression_sequence: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeFinalPublicationPolicyV1 {
    transaction: String,
    prerequisites: Vec<String>,
    max_rowid_changed: String,
    unbounded_collection_allowed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeRollbackPolicyV1 {
    derived_preparation_batches_may_persist: bool,
    source_ledger_mutation_atomic_rollback_required: bool,
    compaction_trigger_change_atomic_rollback_required: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimeActivationPolicyV1 {
    ledger_deletion_enabled: bool,
    automatic_scheduling_enabled: bool,
    compaction_implementation_enabled: bool,
    status_wire_enabled: bool,
}

pub fn validate_storage_lifetime_cursor_v1(
    value: &Value,
) -> Result<StorageLifetimeCursorV1, StorageLifetimeContractError> {
    let cursor: StorageLifetimeCursorV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_storage_lifetime_cursor_fields_v1(&cursor)?;
    Ok(cursor)
}

pub fn validate_storage_lifetime_cursor_transition_v1(
    previous: Option<&Value>,
    next: &Value,
) -> Result<StorageLifetimeCursorV1, StorageLifetimeContractError> {
    let next = validate_storage_lifetime_cursor_v1(next)?;
    let Some(previous) = previous else {
        if next.phase != StorageLifetimePhaseV1::Receipts || next.after_event_rowid != 0 {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime initial cursor must start receipts at afterEventRowid 0",
            ));
        }
        return Ok(next);
    };
    let previous = validate_storage_lifetime_cursor_v1(previous)?;

    if previous.phase == StorageLifetimePhaseV1::Compact {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime compact is reserved and cannot advance",
        ));
    }
    if next.cutoff_event_rowid == previous.cutoff_event_rowid {
        if next.phase == previous.phase {
            if next.after_event_rowid <= previous.after_event_rowid {
                return Err(StorageLifetimeContractError::new(
                    "storage lifetime same-phase cursor must advance afterEventRowid",
                ));
            }
            return Ok(next);
        }
        if next_phase(previous.phase) != Some(next.phase)
            || previous.after_event_rowid != previous.cutoff_event_rowid
            || next.after_event_rowid != 0
        {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime phase transition requires completed current phase and next phase at afterEventRowid 0",
            ));
        }
        return Ok(next);
    }
    if next.cutoff_event_rowid > previous.cutoff_event_rowid
        && previous.phase == StorageLifetimePhaseV1::Verify
        && previous.after_event_rowid == previous.cutoff_event_rowid
        && next.phase == StorageLifetimePhaseV1::Receipts
        && next.after_event_rowid == 0
    {
        return Ok(next);
    }
    Err(StorageLifetimeContractError::new(
        "storage lifetime cutoff can change only after completed verify when catch-up restarts receipts at 0",
    ))
}

pub fn validate_storage_lifetime_batch_source_payloads_v1(
    source_payload_bytes: &[u64],
) -> Result<(), StorageLifetimeContractError> {
    if source_payload_bytes.len() > STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1 {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime batch exceeds the fixed 100 source row limit",
        ));
    }
    let mut total = 0_u64;
    for payload_bytes in source_payload_bytes {
        if *payload_bytes > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime source payload exceeds the fixed 1 MiB contract limit",
            ));
        }
        total = total.checked_add(*payload_bytes).ok_or_else(|| {
            StorageLifetimeContractError::new(
                "storage lifetime batch source payload byte sum overflowed",
            )
        })?;
        if total > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime batch exceeds the fixed 1 MiB source payload limit",
            ));
        }
    }
    Ok(())
}

pub fn validate_storage_lifetime_batch_policy_v1(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let policy: StorageLifetimeBatchPolicyV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let exact = policy.schema_version == STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V1
        && policy.cases_file == "batching-cases.json"
        && policy.guards.max_source_rows_per_batch
            == STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1 as u64
        && policy.guards.max_canonical_source_payload_bytes_per_batch
            == STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1
        && policy.guards.single_source_payload_oversize == "contractError"
        && !policy.guards.caller_configurable
        && strings_equal(
            &policy.guards.applies_to_phases,
            &["receipts", "messages", "workspaces", "baselines", "verify"],
        )
        && policy.cursor.schema_version == STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1
        && strings_equal(&policy.cursor.phases, STORAGE_LIFETIME_PHASES_V1)
        && strings_equal(
            &policy.cursor.scan_phases,
            &["receipts", "messages", "workspaces", "baselines", "verify"],
        )
        && policy.cursor.phase_start_after_event_rowid == 0
        && policy.cursor.same_phase_progress == "strictlyIncreasingWithinFrozenCutoff"
        && policy.cursor.phase_transition == "currentAtCutoffThenNextPhaseAfterZero"
        && policy.cursor.compact == "reservedDisabledAfterVerifyCompletionOnly"
        && policy.scan.predicate == STORAGE_LIFETIME_KEYSET_SCAN_PREDICATE_V1
        && policy.scan.order_by == STORAGE_LIFETIME_KEYSET_SCAN_ORDER_V1
        && !policy.scan.offset_allowed
        && policy.source.artifact_authority == "eventsAtOrBelowFrozenCutoffOnly"
        && policy.source.workspace_and_baseline_source
            == "eventProjectionSnapshotsAtOrBelowFrozenCutoff"
        && policy.source.repeated_projection_snapshots == "idempotent"
        && policy.source.live_projection_use
            == "finalCatchUpEquivalenceOnlyAfterMaxEventRowidEqualsCutoff"
        && !policy.source.unbounded_collection_allowed
        && policy.transaction.cadence == "oneBatchOneBeginImmediatePerIdleTurn"
        && policy.transaction.committed_derived_rows_may_persist
        && policy.transaction.committed_progress_state == "pending"
        && !policy.transaction.implementation_may_commit_running
        && policy.cancellation.before_begin == "noMutation"
        && strings_equal(
            &policy.cancellation.checks,
            &[
                "beforeEachSourceRow",
                "afterEachSourceRow",
                "immediatelyPrecommit",
            ],
        )
        && policy.cancellation.during_transaction == "rollbackCurrentBatch"
        && policy.cancellation.outcome == "Suspended"
        && policy.cancellation.crash == "rollbackCurrentBatchResumePrecedingCommittedPendingCursor"
        && policy
            .cancellation
            .committed_batch_wins_after_final_precommit_check
        && policy.job.table == "maintenance_jobs"
        && policy.job.job_kind == "losslessCompaction"
        && policy.job.cursor_column == "cutoff_json"
        && policy.job.cursor_encoding == "canonicalJsonCursorSchemaVersion1"
        && policy.job.maximum_active_jobs == 1
        && strings_equal(&policy.job.active_states, &["pending", "running"])
        && policy.job.resumable_state == "pending"
        && policy.job.in_memory_only_outcome == "Suspended"
        && strings_equal(
            &policy.job.persisted_maintenance_job_fail_closed_states,
            &["running"],
        )
        && strings_equal(
            &policy.job.terminal_maintenance_job_states,
            &["succeeded", "failed"],
        )
        && policy.job.failed_maintenance_job_role == "terminalHistoryNoAutomaticRetry"
        && strings_equal(
            &policy.job.storage_meta_fail_closed_states,
            &["materializing", "failed"],
        )
        && policy.job.materializing_location == "storage_schema_meta.materialization_state"
        && !policy.job.sql_unique_constraint_present
        && policy.job.single_sqlite_owner_required
        && policy.job.transaction_serialization == "beginImmediate"
        && policy.job.transaction_boundary_validation
            == "zeroOrOneActiveExactPendingJobIdentityAndValidCursor"
        && !policy
            .job
            .second_job_creation_allowed_when_any_lifetime_job_exists
        && policy.job.initial_transition == "absentToPendingCursor"
        && policy.job.append_reactivation == "succeededToPendingSameJobCursorInAppendTransaction"
        && policy.job.batch_commit == "derivedRowsAndAdvancedPendingCursorAtomic"
        && policy.job.cancellation_or_current_failure == "rollbackLeavesPriorPendingCursor"
        && policy.job.final_publication_transition == "pendingToSucceeded"
        && policy.job.post_rollback_failure_record
            == "optionalSeparateBestEffortCreateOrUpdateSanitizedFailedTransaction"
        && !policy.job.automatic_retry_enabled
        && policy.append.ready_invalidation == "pendingInSameAppendTransaction"
        && policy.append.ready_publication == "separateAtomicCatchUpVerificationTransaction"
        && policy.append.append_during_materialization == "frozenCutoffUnchangedUntilFinalCatchUp"
        && strings_equal(
            &policy.append.ready_regression_sequence,
            &[
                "materializeReady",
                "appendAndInvalidateReadyToPendingSameTransaction",
                "separateCatchUpVerification",
                "publishReady",
            ],
        )
        && policy.final_publication.transaction == "separateBeginImmediate"
        && strings_equal(
            &policy.final_publication.prerequisites,
            &[
                "maxEventRowidEqualsCutoff",
                "exactDerivedCompletenessAndEquivalence",
                "liveProjectionEqualsCutoffReconstruction",
                "boundedSqlKeysetEvidence",
            ],
        )
        && policy.final_publication.max_rowid_changed
            == "remainPendingStartOrContinueCatchUpAtObservedCutoffNeverPublishReady"
        && !policy.final_publication.unbounded_collection_allowed
        && policy.rollback.derived_preparation_batches_may_persist
        && policy
            .rollback
            .source_ledger_mutation_atomic_rollback_required
        && policy
            .rollback
            .compaction_trigger_change_atomic_rollback_required
        && !policy.activation.ledger_deletion_enabled
        && !policy.activation.automatic_scheduling_enabled
        && !policy.activation.compaction_implementation_enabled
        && !policy.activation.status_wire_enabled;
    if !exact {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime batch policy contradicts the fixed v1 safety contract",
        ));
    }
    Ok(())
}

fn validate_storage_lifetime_cursor_fields_v1(
    cursor: &StorageLifetimeCursorV1,
) -> Result<(), StorageLifetimeContractError> {
    if cursor.schema_version != STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1 {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime cursor schemaVersion must be 1",
        ));
    }
    require_js_safe_unsigned(cursor.cutoff_event_rowid, "cursor.cutoffEventRowid")?;
    require_js_safe_unsigned(cursor.after_event_rowid, "cursor.afterEventRowid")?;
    if cursor.after_event_rowid > cursor.cutoff_event_rowid {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime cursor afterEventRowid cannot exceed cutoffEventRowid",
        ));
    }
    if cursor.phase == StorageLifetimePhaseV1::Compact && cursor.after_event_rowid != 0 {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime reserved compact cursor must use afterEventRowid 0",
        ));
    }
    Ok(())
}

fn next_phase(phase: StorageLifetimePhaseV1) -> Option<StorageLifetimePhaseV1> {
    match phase {
        StorageLifetimePhaseV1::Receipts => Some(StorageLifetimePhaseV1::Messages),
        StorageLifetimePhaseV1::Messages => Some(StorageLifetimePhaseV1::Workspaces),
        StorageLifetimePhaseV1::Workspaces => Some(StorageLifetimePhaseV1::Baselines),
        StorageLifetimePhaseV1::Baselines => Some(StorageLifetimePhaseV1::Verify),
        StorageLifetimePhaseV1::Verify => Some(StorageLifetimePhaseV1::Compact),
        StorageLifetimePhaseV1::Compact => None,
    }
}

fn strings_equal(actual: &[String], expected: &[&str]) -> bool {
    actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
}

pub fn validate_storage_status_request_v1(
    value: &Value,
) -> Result<StorageStatusRequestV1, StorageLifetimeContractError> {
    let request: StorageStatusRequestV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if request.schema_version != STORAGE_STATUS_SCHEMA_VERSION_V1 {
        return Err(StorageLifetimeContractError::new(
            "storage status request schemaVersion must be 1",
        ));
    }
    Ok(request)
}

pub fn validate_storage_status_v1(
    value: &Value,
) -> Result<StorageStatusV1, StorageLifetimeContractError> {
    let status: StorageStatusV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if status.schema_version != STORAGE_STATUS_SCHEMA_VERSION_V1 {
        return Err(StorageLifetimeContractError::new(
            "storage status schemaVersion must be 1",
        ));
    }
    require_timestamp(&status.collected_at, "collectedAt")?;
    for (name, number) in [
        ("database.logicalBytes", status.database.logical_bytes),
        ("database.physicalBytes", status.database.physical_bytes),
        ("database.walBytes", status.database.wal_bytes),
        ("ledgerTail.eventCount", status.ledger_tail.event_count),
        ("ledgerTail.payloadBytes", status.ledger_tail.payload_bytes),
        ("snapshotAggregates.count", status.snapshot_aggregates.count),
        (
            "snapshotAggregates.payloadBytes",
            status.snapshot_aggregates.payload_bytes,
        ),
    ] {
        require_js_safe_unsigned(number, name)?;
    }
    validate_pressure_facts(&status.pressure_facts)?;
    validate_compaction_status(&status.compaction)?;
    if status.physical_cleanup.enabled {
        return Err(StorageLifetimeContractError::new(
            "physical cleanup must remain disabled until the user policy is decided",
        ));
    }
    Ok(status)
}

pub fn classify_operation_against_receipt_v2(
    receipt: &OperationReceiptV2,
    incoming: &LedgerEventV1,
) -> Result<OperationDecisionV1, StorageLifetimeContractError> {
    validate_operation_receipt_v2(receipt)?;
    classify_operation_v1(&[], incoming)
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if receipt.operation_id != incoming.operation_id {
        return Ok(OperationDecisionV1::Append);
    }
    let exact = receipt.event_id == incoming.event_id
        && receipt.aggregate_type == incoming.aggregate_type
        && receipt.aggregate_id == incoming.aggregate_id
        && receipt.aggregate_sequence == incoming.aggregate_sequence
        && receipt.event_type == incoming.event_type
        && receipt.payload_schema_version == incoming.payload_schema_version
        && receipt.canonical_payload_json == incoming.canonical_json_payload
        && receipt.event_timestamp == incoming.timestamp;
    Ok(if exact {
        OperationDecisionV1::DuplicateNoOp
    } else {
        OperationDecisionV1::Conflict
    })
}

fn validate_operation_receipt_v2(
    receipt: &OperationReceiptV2,
) -> Result<(), StorageLifetimeContractError> {
    for (name, value) in [
        ("operationId", receipt.operation_id.as_str()),
        ("eventId", receipt.event_id.as_str()),
        ("aggregateType", receipt.aggregate_type.as_str()),
        ("aggregateId", receipt.aggregate_id.as_str()),
        ("eventType", receipt.event_type.as_str()),
        (
            "canonicalPayloadJson",
            receipt.canonical_payload_json.as_str(),
        ),
    ] {
        if value.is_empty() {
            return Err(StorageLifetimeContractError::new(format!(
                "operation receipt {name} is required"
            )));
        }
    }
    if receipt.aggregate_sequence == 0 || receipt.aggregate_sequence > MAX_SEQUENCE {
        return Err(StorageLifetimeContractError::new(
            "operation receipt aggregateSequence must be a positive JavaScript safe integer",
        ));
    }
    if receipt.payload_schema_version != 1 {
        return Err(StorageLifetimeContractError::new(
            "operation receipt payloadSchemaVersion must be 1",
        ));
    }
    if receipt.canonical_payload_digest_algorithm != OPERATION_RECEIPT_DIGEST_ALGORITHM_V2
        || receipt.canonical_payload_digest_version != OPERATION_RECEIPT_DIGEST_VERSION_V2
        || receipt.canonical_payload_digest_format != OPERATION_RECEIPT_DIGEST_FORMAT_V2
        || receipt.canonical_payload_digest.len() != 64
        || !receipt
            .canonical_payload_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StorageLifetimeContractError::new(
            "operation receipt digest metadata must be sha256 v1 lowercaseHex64",
        ));
    }
    let payload: Value =
        serde_json::from_str(&receipt.canonical_payload_json).map_err(|error| {
            StorageLifetimeContractError::new(format!(
                "operation receipt canonicalPayloadJson must be valid JSON: {error}"
            ))
        })?;
    let canonical = canonical_json_v1(payload.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if canonical != receipt.canonical_payload_json {
        return Err(StorageLifetimeContractError::new(
            "operation receipt canonicalPayloadJson must be canonical JSON",
        ));
    }
    if !payload.is_object() {
        return Err(StorageLifetimeContractError::new(
            "operation receipt canonicalPayloadJson must be an object",
        ));
    }
    let computed = sha256_file_version_v1(receipt.canonical_payload_json.as_bytes());
    if computed.strip_prefix("sha256:") != Some(receipt.canonical_payload_digest.as_str()) {
        return Err(StorageLifetimeContractError::new(
            "operation receipt canonicalPayloadDigest does not match canonicalPayloadJson",
        ));
    }
    require_timestamp(&receipt.event_timestamp, "operation receipt eventTimestamp")
}

fn validate_pressure_facts(
    facts: &StoragePressureFactsV1,
) -> Result<(), StorageLifetimeContractError> {
    match facts.collection_status {
        StoragePressureCollectionStatusV1::Available => {
            let capacity = facts.volume_capacity_bytes.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "available pressure facts require volumeCapacityBytes",
                )
            })?;
            let available = facts.volume_available_bytes.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "available pressure facts require volumeAvailableBytes",
                )
            })?;
            require_js_safe_unsigned(capacity, "pressureFacts.volumeCapacityBytes")?;
            require_js_safe_unsigned(available, "pressureFacts.volumeAvailableBytes")?;
            if capacity == 0 || available > capacity {
                return Err(StorageLifetimeContractError::new(
                    "available pressure facts must have positive capacity and available bytes within capacity",
                ));
            }
        }
        StoragePressureCollectionStatusV1::Unavailable => {
            if facts.volume_capacity_bytes.is_some() || facts.volume_available_bytes.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "unavailable pressure facts cannot include partial volume values",
                ));
            }
        }
    }
    Ok(())
}

fn validate_compaction_status(
    status: &StorageCompactionStatusV1,
) -> Result<(), StorageLifetimeContractError> {
    match status.last_attempt_status {
        StorageCompactionAttemptStatusV1::NeverRun => {
            if status.last_attempted_at.is_some() || status.last_successful_at.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "neverRun compaction cannot include attempt timestamps",
                ));
            }
        }
        StorageCompactionAttemptStatusV1::Succeeded => {
            let attempted = require_optional_timestamp(
                status.last_attempted_at.as_deref(),
                "compaction.lastAttemptedAt",
            )?;
            let successful = require_optional_timestamp(
                status.last_successful_at.as_deref(),
                "compaction.lastSuccessfulAt",
            )?;
            if attempted != successful {
                return Err(StorageLifetimeContractError::new(
                    "succeeded compaction must report the same attempt and success time",
                ));
            }
        }
        StorageCompactionAttemptStatusV1::Failed => {
            let attempted = require_optional_timestamp(
                status.last_attempted_at.as_deref(),
                "compaction.lastAttemptedAt",
            )?;
            if let Some(value) = status.last_successful_at.as_deref() {
                require_timestamp(value, "compaction.lastSuccessfulAt")?;
                if value > attempted {
                    return Err(StorageLifetimeContractError::new(
                        "failed compaction lastSuccessfulAt cannot be later than lastAttemptedAt",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn require_js_safe_unsigned(value: u64, name: &str) -> Result<(), StorageLifetimeContractError> {
    if value > MAX_SEQUENCE {
        return Err(StorageLifetimeContractError::new(format!(
            "{name} must fit a non-negative JavaScript safe integer"
        )));
    }
    Ok(())
}

fn require_optional_timestamp<'a>(
    value: Option<&'a str>,
    name: &str,
) -> Result<&'a str, StorageLifetimeContractError> {
    let value = value
        .ok_or_else(|| StorageLifetimeContractError::new(format!("{name} must be present")))?;
    require_timestamp(value, name)?;
    Ok(value)
}

fn require_timestamp(value: &str, name: &str) -> Result<(), StorageLifetimeContractError> {
    let bytes = value.as_bytes();
    let separators = [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'Z'),
    ];
    let shape_valid = bytes.len() == 20
        && separators
            .iter()
            .all(|(index, expected)| bytes.get(*index) == Some(expected))
        && bytes.iter().enumerate().all(|(index, byte)| {
            separators.iter().any(|(separator, _)| *separator == index) || byte.is_ascii_digit()
        });
    if !shape_valid {
        return Err(StorageLifetimeContractError::new(format!(
            "{name} must be an RFC 3339 UTC second timestamp"
        )));
    }
    let parse = |start: usize, end: usize| value[start..end].parse::<u32>().ok();
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        parse(0, 4),
        parse(5, 7),
        parse(8, 10),
        parse(11, 13),
        parse(14, 16),
        parse(17, 19),
    ) else {
        return Err(StorageLifetimeContractError::new(format!(
            "{name} must be an RFC 3339 UTC second timestamp"
        )));
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        0,
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > month_days[month as usize]
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(StorageLifetimeContractError::new(format!(
            "{name} must be a real RFC 3339 UTC calendar timestamp"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
