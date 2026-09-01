use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::fmt::{self, Display};

use crate::project_v1::sha256_file_version_v1;
use crate::run_recovery_v1::validate_g5_payload_v1;
use crate::storage_v1::{
    canonical_json_v1, classify_operation_v1, LedgerEventV1, OperationDecisionV1,
};
use crate::MAX_SEQUENCE;

// Kept at v2 until the sidecar consumer implements the contract-only v3 migration.
pub const STORAGE_LIFETIME_SCHEMA_VERSION_V1: u32 = 2;
pub const STORAGE_LIFETIME_SCHEMA_VERSION_V3: u32 = 3;
pub const STORAGE_LIFETIME_SCHEMA_VERSION_V4: u32 = 4;
pub const STORAGE_LIFETIME_SCHEMA_VERSION_V5: u32 = 5;
pub const STORAGE_LIFETIME_SCHEMA_VERSION_V6: u32 = 6;
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
pub const STORAGE_LIFETIME_TABLES_V3: &[&str] = &[
    "storage_schema_meta",
    "ledger_baselines",
    "operation_receipts",
    "chat_messages",
    "prepared_workspaces",
    "maintenance_jobs",
    "lifetime_materialization_job",
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
pub const STORAGE_LIFETIME_PHASES_V1: &[&str] =
    &["receipts", "messages", "workspaces", "baselines", "verify"];
pub const STORAGE_LIFETIME_JOB_STATES_V1: &[&str] = &["pending", "failed"];
pub const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2: u32 = 2;
pub const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V2: u32 = 2;
pub const STORAGE_LIFETIME_PHASES_V2: &[&str] = &[
    "receipts",
    "messages",
    "workspaces",
    "baselines",
    "verifySource",
    "verifyReceipts",
    "verifyMessages",
    "verifyWorkspaces",
    "verifyBaselines",
];
pub const STORAGE_LIFETIME_EVENT_ROWID_PHASES_V2: &[&str] = &[
    "receipts",
    "messages",
    "workspaces",
    "baselines",
    "verifySource",
];
pub const STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V2: &[&str] = &[
    "verifyReceipts",
    "verifyMessages",
    "verifyWorkspaces",
    "verifyBaselines",
];
pub const STORAGE_LIFETIME_JOB_STATES_V2: &[&str] = &["pending", "attempting", "failed"];
pub const STORAGE_LIFETIME_KEYSET_LIMIT_SQL_V2: &str = "LIMIT 100";
pub const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3: u32 = 3;
pub const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V3: u32 = 3;
pub const STORAGE_LIFETIME_PHASES_V3: &[&str] = &[
    "receipts",
    "messages",
    "workspaces",
    "baselines",
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
];
pub const STORAGE_LIFETIME_EVENT_ROWID_PHASES_V3: &[&str] = &[
    "receipts",
    "messages",
    "workspaces",
    "baselines",
    "verifySource",
];
pub const STORAGE_LIFETIME_PROJECTION_ROWID_PHASES_V3: &[&str] = &[
    "verifyProjects",
    "verifyChats",
    "verifyRuns",
    "verifyLocks",
    "verifyAppInstances",
];
pub const STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V3: &[&str] = &[
    "verifyReceipts",
    "verifyMessages",
    "verifyWorkspaces",
    "verifyBaselines",
];
pub const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4: u32 = 4;
pub const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V4: u32 = 4;
pub const STORAGE_LIFETIME_PHASES_V4: &[&str] = &[
    "indexSource",
    "verifySourceIndex",
    "receipts",
    "messages",
    "workspaces",
    "baselines",
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
];
pub const STORAGE_LIFETIME_EVENT_ROWID_PHASES_V4: &[&str] = &[
    "indexSource",
    "verifySourceIndex",
    "receipts",
    "messages",
    "workspaces",
    "baselines",
    "verifySource",
];
pub const STORAGE_LIFETIME_PROJECTION_ROWID_PHASES_V4: &[&str] = &[
    "verifyProjects",
    "verifyChats",
    "verifyRuns",
    "verifyLocks",
    "verifyAppInstances",
];
pub const STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V4: &[&str] = &[
    "verifyReceipts",
    "verifyMessages",
    "verifyWorkspaces",
    "verifyBaselines",
];
pub const STORAGE_EVENT_SOURCE_INDEX_COLUMNS_V1: &[&str] = &[
    "event_rowid",
    "event_id",
    "operation_id",
    "aggregate_type",
    "aggregate_id",
    "aggregate_sequence",
    "introduced_message_id",
    "chat_durable_sequence",
    "chat_snapshot_message_id",
];
pub const STORAGE_LIFETIME_V3_PREFLIGHT_ALLOWED_STATES_V1: &[&str] = &["pending", "ready"];
pub const STORAGE_LIFETIME_V3_PREFLIGHT_REJECTED_STATES_V1: &[&str] = &["materializing", "failed"];
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeCursorV1 {
    pub schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV1,
    pub after_event_rowid: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimeJobStateV1 {
    Pending,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageMaterializationStateV1 {
    Pending,
    Materializing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeMaterializationJobV1 {
    pub singleton: u32,
    pub job_id: String,
    pub state: StorageLifetimeJobStateV1,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV1,
    pub after_event_rowid: u64,
    pub failure_kind: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeMaterializationSnapshotV1 {
    pub materialization_state: StorageMaterializationStateV1,
    pub job: Option<StorageLifetimeMaterializationJobV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeV3PreflightPolicyV1 {
    pub schema: String,
    pub allowed_materialization_states: Vec<String>,
    pub rejected_materialization_states: Vec<String>,
    pub timing: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeV4PreflightPolicyV1 {
    pub schema: String,
    pub admissible_pairs: Vec<String>,
    pub migration_mappings: Vec<String>,
    pub reopen_runnable_pairs: Vec<String>,
    pub reopen_fail_closed_pairs: Vec<String>,
    pub timing: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeV5PreflightPolicyV1 {
    pub schema: String,
    pub admissible_pairs: Vec<String>,
    pub migration_mappings: Vec<String>,
    pub reopen_runnable_pairs: Vec<String>,
    pub reopen_fail_closed_pairs: Vec<String>,
    pub timing: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeV6PreflightPolicyV1 {
    pub schema: String,
    pub admissible_pairs: Vec<String>,
    pub migration_mappings: Vec<String>,
    pub reopen_runnable_pairs: Vec<String>,
    pub reopen_fail_closed_pairs: Vec<String>,
    pub timing: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimeLifecycleOperationV1 {
    Create,
    CommitBatch,
    Cancel,
    Resume,
    Fail,
    Complete,
    Append,
    CatchUpReset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimePhaseV2 {
    Receipts,
    Messages,
    Workspaces,
    Baselines,
    VerifySource,
    VerifyReceipts,
    VerifyMessages,
    VerifyWorkspaces,
    VerifyBaselines,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeCursorV2 {
    pub schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV2,
    pub after_scan_rowid: u64,
    pub scan_exhausted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimeJobStateV2 {
    Pending,
    Attempting,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeMaterializationJobV2 {
    pub singleton: u32,
    pub job_id: String,
    pub state: StorageLifetimeJobStateV2,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV2,
    pub after_scan_rowid: u64,
    pub scan_exhausted: bool,
    pub failure_kind: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeMaterializationSnapshotV2 {
    pub materialization_state: StorageMaterializationStateV1,
    pub job: Option<StorageLifetimeMaterializationJobV2>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimeLifecycleOperationV2 {
    Create,
    Arm,
    BatchSuccess,
    PhaseAdvance,
    Cancel,
    Fail,
    FailureRecordUnavailable,
    FinalPublish,
    FinalReset,
    Append,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimePhaseV3 {
    Receipts,
    Messages,
    Workspaces,
    Baselines,
    VerifySource,
    VerifyProjects,
    VerifyChats,
    VerifyRuns,
    VerifyLocks,
    VerifyAppInstances,
    VerifyReceipts,
    VerifyMessages,
    VerifyWorkspaces,
    VerifyBaselines,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeCursorV3 {
    pub schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV3,
    pub after_scan_rowid: u64,
    pub scan_exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeMaterializationJobV3 {
    pub singleton: u32,
    pub job_id: String,
    pub state: StorageLifetimeJobStateV2,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV3,
    pub after_scan_rowid: u64,
    pub scan_exhausted: bool,
    pub failure_kind: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeMaterializationSnapshotV3 {
    pub materialization_state: StorageMaterializationStateV1,
    pub job: Option<StorageLifetimeMaterializationJobV3>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimeLifecycleOperationV3 {
    Create,
    Arm,
    BatchSuccess,
    PhaseAdvance,
    Cancel,
    Fail,
    FailureRecordUnavailable,
    FinalPublish,
    CatchUpReset,
    Append,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimePhaseV4 {
    IndexSource,
    VerifySourceIndex,
    Receipts,
    Messages,
    Workspaces,
    Baselines,
    VerifySource,
    VerifyProjects,
    VerifyChats,
    VerifyRuns,
    VerifyLocks,
    VerifyAppInstances,
    VerifyReceipts,
    VerifyMessages,
    VerifyWorkspaces,
    VerifyBaselines,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeCursorV4 {
    pub schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV4,
    pub after_scan_rowid: u64,
    pub scan_exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeMaterializationJobV4 {
    pub singleton: u32,
    pub job_id: String,
    pub state: StorageLifetimeJobStateV2,
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV4,
    pub after_scan_rowid: u64,
    pub scan_exhausted: bool,
    pub failure_kind: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeMaterializationSnapshotV4 {
    pub materialization_state: StorageMaterializationStateV1,
    pub job: Option<StorageLifetimeMaterializationJobV4>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageEventSourceIndexEntryV1 {
    pub event_rowid: u64,
    pub event_id: String,
    pub operation_id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_sequence: u64,
    pub introduced_message_id: Option<String>,
    pub chat_durable_sequence: Option<u64>,
    pub chat_snapshot_message_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageLifetimeLifecycleOperationV4 {
    Create,
    Arm,
    BatchSuccess,
    PhaseAdvance,
    Cancel,
    Fail,
    FailureRecordUnavailable,
    FinalPublish,
    CatchUpReset,
    Append,
}

pub fn validate_storage_lifetime_v3_preflight_policy_v1(
    value: &Value,
) -> Result<StorageLifetimeV3PreflightPolicyV1, StorageLifetimeContractError> {
    let policy: StorageLifetimeV3PreflightPolicyV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let exact = policy.schema == "exactStorageV2"
        && strings_equal(
            &policy.allowed_materialization_states,
            STORAGE_LIFETIME_V3_PREFLIGHT_ALLOWED_STATES_V1,
        )
        && strings_equal(
            &policy.rejected_materialization_states,
            STORAGE_LIFETIME_V3_PREFLIGHT_REJECTED_STATES_V1,
        )
        && policy.timing == "beforeAnyDdlOrMutation";
    if !exact {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v3 preflight policy is not fail-closed",
        ));
    }
    Ok(policy)
}

pub fn validate_storage_lifetime_v4_preflight_policy_v1(
    value: &Value,
) -> Result<StorageLifetimeV4PreflightPolicyV1, StorageLifetimeContractError> {
    let policy: StorageLifetimeV4PreflightPolicyV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let exact = policy.schema == "exactStorageV3"
        && strings_equal(
            &policy.admissible_pairs,
            &[
                "ready+absent",
                "pending+absent",
                "pending+pending",
                "pending+failed",
            ],
        )
        && strings_equal(
            &policy.migration_mappings,
            &[
                "pendingPhasesSameName",
                "verifyToVerifySource",
                "afterEventRowidToAfterScanRowid",
                "scanExhaustedFalse",
                "failedPreserved",
                "attemptingImpossibleInV3",
            ],
        )
        && strings_equal(
            &policy.reopen_runnable_pairs,
            &["ready+absent", "pending+absent", "pending+pending"],
        )
        && strings_equal(
            &policy.reopen_fail_closed_pairs,
            &["pending+attempting", "pending+failed"],
        )
        && policy.timing == "beforeAnyDdlOrMutation";
    if !exact {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 preflight policy is not fail-closed",
        ));
    }
    Ok(policy)
}

pub fn validate_storage_lifetime_v5_preflight_policy_v1(
    value: &Value,
) -> Result<StorageLifetimeV5PreflightPolicyV1, StorageLifetimeContractError> {
    let policy: StorageLifetimeV5PreflightPolicyV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let exact = policy.schema == "exactStorageV4"
        && strings_equal(
            &policy.admissible_pairs,
            &[
                "ready+absent",
                "pending+absent",
                "pending+pending",
                "pending+attempting",
                "pending+failed",
            ],
        )
        && strings_equal(
            &policy.migration_mappings,
            &[
                "materializationStateAndAbsentJobPreserved",
                "receiptsThroughVerifySourcePositionPreserved",
                "legacyDerivedVerificationRestartsVerifySourceAtZeroUnexhausted",
                "pendingAttemptingFailedStatePreserved",
                "failureEvidencePreserved",
            ],
        )
        && strings_equal(
            &policy.reopen_runnable_pairs,
            &["ready+absent", "pending+absent", "pending+pending"],
        )
        && strings_equal(
            &policy.reopen_fail_closed_pairs,
            &["pending+attempting", "pending+failed"],
        )
        && policy.timing == "beforeAnyDdlOrMutation";
    if !exact {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 preflight policy is not fail-closed",
        ));
    }
    Ok(policy)
}

pub fn validate_storage_lifetime_v6_preflight_policy_v1(
    value: &Value,
) -> Result<StorageLifetimeV6PreflightPolicyV1, StorageLifetimeContractError> {
    let policy: StorageLifetimeV6PreflightPolicyV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let exact = policy.schema == "exactStorageV5"
        && strings_equal(
            &policy.admissible_pairs,
            &[
                "ready+absent",
                "pending+absent",
                "pending+pending",
                "pending+attempting",
                "pending+failed",
            ],
        )
        && strings_equal(
            &policy.migration_mappings,
            &[
                "readyAbsentBecomesPendingAbsent",
                "pendingAbsentPreserved",
                "everyPresentJobRestartsIndexSourceAtZeroUnexhausted",
                "pendingAttemptingFailedStatePreserved",
                "jobIdentityAndTimestampsPreserved",
                "failureEvidencePreserved",
                "existingEventHistoryNotBackfilledByMigration",
            ],
        )
        && strings_equal(
            &policy.reopen_runnable_pairs,
            &["ready+absent", "pending+absent", "pending+pending"],
        )
        && strings_equal(
            &policy.reopen_fail_closed_pairs,
            &["pending+attempting", "pending+failed"],
        )
        && policy.timing == "beforeAnyDdlOrMutation";
    if !exact {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 preflight policy is not fail-closed",
        ));
    }
    Ok(policy)
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
    pairing: StorageLifetimePairingPolicyV1,
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
    maximum_rows: u64,
    singleton_value: u64,
    unique_job_id: bool,
    states: Vec<String>,
    cursor_columns: Vec<String>,
    maintenance_jobs_role: String,
    single_sqlite_owner_required: bool,
    transaction_serialization: String,
    transaction_boundary_validation: String,
    batch_commit: String,
    cancellation: String,
    terminal_failure: String,
    terminal_failure_meta_state: String,
    failure_record_unavailable: String,
    automatic_retry_enabled: bool,
    completion: String,
    append_after_ready: String,
    append_during_pending: String,
    catch_up: String,
    scheduler_creation_implemented: bool,
    forbidden_committed_states: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StorageLifetimePairingPolicyV1 {
    canonical_readiness_authority: String,
    valid_pairs: Vec<String>,
    fail_closed_pairs: Vec<String>,
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

pub fn validate_storage_lifetime_materialization_job_v1(
    value: &Value,
) -> Result<StorageLifetimeMaterializationJobV1, StorageLifetimeContractError> {
    let job: StorageLifetimeMaterializationJobV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if job.singleton != 1 || job.job_id.is_empty() {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime job requires singleton 1 and non-empty jobId",
        ));
    }
    validate_storage_lifetime_cursor_fields_v1(&job_cursor(&job))?;
    require_timestamp(&job.created_at, "job.createdAt")?;
    require_timestamp(&job.updated_at, "job.updatedAt")?;
    match job.state {
        StorageLifetimeJobStateV1::Pending => {
            if job.failure_kind.is_some() || job.completed_at.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "pending storage lifetime job cannot carry failure or completion evidence",
                ));
            }
        }
        StorageLifetimeJobStateV1::Failed => {
            if job
                .failure_kind
                .as_deref()
                .is_none_or(|kind| !is_sanitized_failure_kind(kind))
            {
                return Err(StorageLifetimeContractError::new(
                    "failed storage lifetime job requires sanitized failureKind",
                ));
            }
            require_timestamp(
                job.completed_at.as_deref().unwrap_or_default(),
                "job.completedAt",
            )?;
        }
    }
    Ok(job)
}

pub fn validate_storage_lifetime_materialization_pair_v1(
    value: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV1, StorageLifetimeContractError> {
    let raw: StorageLifetimeMaterializationSnapshotV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let job = raw
        .job
        .as_ref()
        .map(|job| {
            serde_json::to_value(job)
                .map_err(|error| StorageLifetimeContractError::new(error.to_string()))
                .and_then(|value| validate_storage_lifetime_materialization_job_v1(&value))
        })
        .transpose()?;
    let valid = matches!(
        raw.materialization_state,
        StorageMaterializationStateV1::Pending
    ) || (raw.materialization_state == StorageMaterializationStateV1::Ready
        && job.is_none());
    if !valid {
        return Err(StorageLifetimeContractError::new(
            "storage materialization meta/job pairing is fail-closed",
        ));
    }
    Ok(StorageLifetimeMaterializationSnapshotV1 {
        materialization_state: raw.materialization_state,
        job,
    })
}

pub fn validate_storage_lifetime_lifecycle_transition_v1(
    operation: StorageLifetimeLifecycleOperationV1,
    previous: &Value,
    next: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV1, StorageLifetimeContractError> {
    let previous = validate_storage_lifetime_materialization_pair_v1(previous)?;
    let next = validate_storage_lifetime_materialization_pair_v1(next)?;
    match operation {
        StorageLifetimeLifecycleOperationV1::Create => {
            let next_job = next
                .job
                .as_ref()
                .filter(|job| job.state == StorageLifetimeJobStateV1::Pending);
            if previous.materialization_state != StorageMaterializationStateV1::Pending
                || previous.job.is_some()
                || next.materialization_state != StorageMaterializationStateV1::Pending
                || next_job.is_none()
            {
                return lifecycle_error(
                    "storage lifetime create requires pending+absent to pending+pending",
                );
            }
            validate_storage_lifetime_cursor_transition_v1(
                None,
                &serde_json::to_value(job_cursor(next_job.expect("checked")))
                    .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
            )?;
        }
        StorageLifetimeLifecycleOperationV1::Cancel
        | StorageLifetimeLifecycleOperationV1::Resume => {
            if previous.job.as_ref().map(|job| job.state)
                != Some(StorageLifetimeJobStateV1::Pending)
                || previous != next
            {
                return lifecycle_error(
                    "storage lifetime cancel/resume must preserve the exact pending snapshot",
                );
            }
        }
        StorageLifetimeLifecycleOperationV1::CommitBatch => {
            let (previous_job, next_job) = same_pending_jobs(&previous, &next, "commitBatch")?;
            require_same_job_identity(previous_job, next_job)?;
            if previous_job.cutoff_event_rowid != next_job.cutoff_event_rowid {
                return lifecycle_error(
                    "storage lifetime batch commit cannot change the frozen cutoff",
                );
            }
            validate_storage_lifetime_cursor_transition_v1(
                Some(
                    &serde_json::to_value(job_cursor(previous_job))
                        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
                ),
                &serde_json::to_value(job_cursor(next_job))
                    .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
            )?;
        }
        StorageLifetimeLifecycleOperationV1::Fail => {
            let previous_job = previous.job.as_ref();
            let next_job = next.job.as_ref();
            if previous.materialization_state != StorageMaterializationStateV1::Pending
                || next.materialization_state != StorageMaterializationStateV1::Pending
                || previous_job.map(|job| job.state) != Some(StorageLifetimeJobStateV1::Pending)
                || next_job.map(|job| job.state) != Some(StorageLifetimeJobStateV1::Failed)
            {
                return lifecycle_error(
                    "storage lifetime failure must transition the same pending singleton to failed",
                );
            }
            let previous_job = previous_job.expect("checked");
            let next_job = next_job.expect("checked");
            require_same_job_identity(previous_job, next_job)?;
            if job_cursor(previous_job) != job_cursor(next_job) {
                return lifecycle_error(
                    "storage lifetime failure must preserve the canonical cursor",
                );
            }
        }
        StorageLifetimeLifecycleOperationV1::Complete => {
            let completed = previous.job.as_ref().is_some_and(|job| {
                job.state == StorageLifetimeJobStateV1::Pending
                    && job.phase == StorageLifetimePhaseV1::Verify
                    && job.after_event_rowid == job.cutoff_event_rowid
            });
            if previous.materialization_state != StorageMaterializationStateV1::Pending
                || !completed
                || next.materialization_state != StorageMaterializationStateV1::Ready
                || next.job.is_some()
            {
                return lifecycle_error(
                    "storage lifetime completion requires completed verify and atomically removes the job",
                );
            }
        }
        StorageLifetimeLifecycleOperationV1::Append => {
            let ready_invalidation = previous.materialization_state
                == StorageMaterializationStateV1::Ready
                && previous.job.is_none()
                && next.materialization_state == StorageMaterializationStateV1::Pending
                && next.job.is_none();
            let pending_preserved = previous.materialization_state
                == StorageMaterializationStateV1::Pending
                && previous == next;
            if !ready_invalidation && !pending_preserved {
                return lifecycle_error(
                    "storage lifetime append must invalidate ready or preserve the frozen pending snapshot",
                );
            }
        }
        StorageLifetimeLifecycleOperationV1::CatchUpReset => {
            let (previous_job, next_job) = same_pending_jobs(&previous, &next, "catchUpReset")?;
            require_same_job_identity(previous_job, next_job)?;
            if previous_job.phase != StorageLifetimePhaseV1::Verify
                || previous_job.after_event_rowid != previous_job.cutoff_event_rowid
                || next_job.cutoff_event_rowid <= previous_job.cutoff_event_rowid
                || next_job.phase != StorageLifetimePhaseV1::Receipts
                || next_job.after_event_rowid != 0
            {
                return lifecycle_error(
                    "storage lifetime catch-up reset requires completed verify and a larger cutoff",
                );
            }
        }
    }
    Ok(next)
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
        && policy.job.table == "lifetime_materialization_job"
        && policy.job.maximum_rows == 1
        && policy.job.singleton_value == 1
        && policy.job.unique_job_id
        && strings_equal(&policy.job.states, STORAGE_LIFETIME_JOB_STATES_V1)
        && strings_equal(
            &policy.job.cursor_columns,
            &["cutoff_event_rowid", "phase", "after_event_rowid"],
        )
        && policy.job.maintenance_jobs_role == "losslessCompactionOnlyUnchanged"
        && policy.job.single_sqlite_owner_required
        && policy.job.transaction_serialization == "beginImmediate"
        && policy.job.transaction_boundary_validation == "exactMetaJobPairAndValidSingletonCursor"
        && policy.job.batch_commit == "derivedRowsAndAdvancedPendingSingletonAtomic"
        && policy.job.cancellation == "rollbackLeavesSamePendingSingletonAndCursor"
        && policy.job.terminal_failure == "sameSingletonPendingToFailedPreserveCursorAfterRollback"
        && policy.job.terminal_failure_meta_state == "pending"
        && policy.job.failure_record_unavailable
            == "failClosedRestartRevalidatesBeforeMutationNeverSuccess"
        && !policy.job.automatic_retry_enabled
        && policy.job.completion == "deleteSingletonAndSetMetaReadySameTransaction"
        && policy.job.append_after_ready == "setMetaPendingKeepJobAbsentSameAppendTransaction"
        && policy.job.append_during_pending == "doNotAlterFrozenSingletonCursor"
        && policy.job.catch_up == "samePendingSingletonResetToNewCutoffReceiptsZero"
        && !policy.job.scheduler_creation_implemented
        && strings_equal(
            &policy.job.forbidden_committed_states,
            &["running", "succeeded", "cancelled"],
        )
        && policy.pairing.canonical_readiness_authority
            == "storage_schema_meta.materialization_state"
        && strings_equal(
            &policy.pairing.valid_pairs,
            &[
                "pending+absent",
                "pending+pending",
                "pending+failed",
                "ready+absent",
            ],
        )
        && strings_equal(
            &policy.pairing.fail_closed_pairs,
            &["materializing+any", "failed+any", "ready+present"],
        )
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
            == "remainPendingResetSameSingletonAtObservedCutoffNeverPublishReady"
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

pub fn validate_storage_lifetime_batch_policy_v2(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let expected = serde_json::json!({
        "schemaVersion": STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V2,
        "casesFile": "cases.json",
        "guards": {
            "maxRowsPerBatch": STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
            "maxCanonicalPayloadBytesPerBatch":
                STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
            "callerConfigurable": false,
            "appliesToPhases": STORAGE_LIFETIME_PHASES_V2,
        },
        "cursor": {
            "schemaVersion": STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2,
            "column": "after_scan_rowid",
            "phases": STORAGE_LIFETIME_PHASES_V2,
            "eventRowidPhases": STORAGE_LIFETIME_EVENT_ROWID_PHASES_V2,
            "derivedTableRowidPhases": STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V2,
            "phaseStartAfterScanRowid": 0,
            "phaseStartScanExhausted": false,
            "eventRowidBound": "afterScanRowidLessThanOrEqualToCutoff",
            "derivedRowidBound": "jsSafeUnsignedIndependentOfCutoff",
            "scanExhaustedMutationAuthority": "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
            "scanExhaustedProofBoundary": "implementationOwnedAndTestedInSidecarNotRepresentableAsPublicJson",
            "phaseTransition": "scanExhaustedTrueThenExactNextAtZeroAndFalse",
        },
        "scan": {
            "sourcePredicate": "rowid > afterScanRowid AND rowid <= cutoffEventRowid",
            "derivedPredicate": "rowid > afterScanRowid",
            "orderBy": "rowid",
            "limitSql": STORAGE_LIFETIME_KEYSET_LIMIT_SQL_V2,
            "forbiddenWithinBatch": ["OFFSET", "COUNT", "window", "GROUP BY", "unboundedFullTableScan"],
        },
        "transaction": {
            "wordingRevision": "shortArmTransactionThenOneBoundedBatchTransaction",
            "arm": "pendingToAttemptingCommittedBeforeBatch",
            "success": "derivedRowsCursorAndAttemptingToPendingAtomic",
            "nonemptyBatchScanExhausted": false,
            "emptyBatchScanExhausted": "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
            "cancellation": "rollbackBatchOrSavepointThenCommitAttemptingToPending",
            "terminalValidationFailure": "rollbackBatchThenCommitAttemptingToFailed",
            "failureRecordUnavailable": "durableAttemptingSentinelReopenRefusesRetryOrSuccess",
            "reopenAttempting": "failClosedNonRunnable",
            "reopenFailed": "failClosedNonRunnable",
        },
        "verification": {
            "verifySource": "eachEventAtOrBelowCutoffHasExactArtifactsCurrentProjectionAndSequence",
            "verifyDerived": "eachDerivedRowHasExactSourceBackingAndNoSourceLessExtra",
            "derivedPhaseTables": {
                "verifyReceipts": "operation_receipts",
                "verifyMessages": "chat_messages",
                "verifyWorkspaces": "prepared_workspaces",
                "verifyBaselines": "ledger_baselines",
            },
            "historicalBaseline": "allowedWhenDeterministicDigestValidAndSourceBacked",
            "newestBaseline": "requiredForEveryCompactableAggregate",
        },
        "cancellation": {
            "afterCommittedVerifyBatch": "returnSuspended",
            "finalPublicationInSameCall": false,
            "committedBatchWinsScope": "committedBatchTransactionOnly",
        },
        "finalPublication": {
            "beforeBegin": "cancelReturnsSuspendedNoMutation",
            "transaction": "shortBeginImmediateOnly",
            "preconditions": [
                "exactMetaJobPairAndCursor",
                "verifyBaselinesTableEndProvedByExactEmptyKeysetBatch",
                "scanExhaustedTrue",
                "maxEventRowidEqualsCutoff",
                "cancelBeforeMutation",
                "precommitCancel",
            ],
            "mutation": "deleteJobAndSetMetaReady",
            "forbiddenWork": ["artifactScan", "tableScan", "count"],
            "maxRowidChanged": "pendingReceiptsObservedCutoffAfterScanRowidZero",
        },
        "activation": {
            "cleanupEnabled": false,
            "deletionEnabled": false,
            "compactionEnabled": false,
            "wireEnabled": false,
            "schedulerEnabled": false,
        },
    });
    if value != &expected {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 policy contradicts the fixed bounded verification contract",
        ));
    }
    Ok(())
}

pub fn validate_storage_lifetime_cursor_v2(
    value: &Value,
) -> Result<StorageLifetimeCursorV2, StorageLifetimeContractError> {
    let cursor: StorageLifetimeCursorV2 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_storage_lifetime_cursor_fields_v2(&cursor)?;
    Ok(cursor)
}

pub fn validate_storage_lifetime_cursor_transition_v2(
    previous: Option<&Value>,
    next: &Value,
) -> Result<StorageLifetimeCursorV2, StorageLifetimeContractError> {
    let next = validate_storage_lifetime_cursor_v2(next)?;
    let Some(previous) = previous else {
        if next.phase != StorageLifetimePhaseV2::Receipts
            || next.after_scan_rowid != 0
            || next.scan_exhausted
        {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime v4 initial cursor must start unexhausted receipts at zero",
            ));
        }
        return Ok(next);
    };
    let previous = validate_storage_lifetime_cursor_v2(previous)?;
    if next.cutoff_event_rowid != previous.cutoff_event_rowid {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 batch transition cannot change the frozen cutoff",
        ));
    }
    if next.phase == previous.phase {
        if previous.scan_exhausted
            || next.scan_exhausted
            || next.after_scan_rowid <= previous.after_scan_rowid
        {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime v4 same-phase cursor must advance afterScanRowid",
            ));
        }
        return Ok(next);
    }
    if !previous.scan_exhausted
        || next.scan_exhausted
        || next_phase_v2(previous.phase) != Some(next.phase)
        || next.after_scan_rowid != 0
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 phase transition requires exhausted current phase and unexhausted exact next phase at zero",
        ));
    }
    Ok(next)
}

pub fn validate_storage_lifetime_materialization_job_v2(
    value: &Value,
) -> Result<StorageLifetimeMaterializationJobV2, StorageLifetimeContractError> {
    let job: StorageLifetimeMaterializationJobV2 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if job.singleton != 1 || job.job_id.is_empty() {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 job requires singleton 1 and non-empty jobId",
        ));
    }
    validate_storage_lifetime_cursor_fields_v2(&job_cursor_v2(&job))?;
    require_timestamp(&job.created_at, "job.createdAt")?;
    require_timestamp(&job.updated_at, "job.updatedAt")?;
    match job.state {
        StorageLifetimeJobStateV2::Failed => {
            if job
                .failure_kind
                .as_deref()
                .is_none_or(|kind| !is_sanitized_failure_kind(kind))
            {
                return Err(StorageLifetimeContractError::new(
                    "failed storage lifetime v4 job requires sanitized failureKind",
                ));
            }
            require_timestamp(
                job.completed_at.as_deref().unwrap_or_default(),
                "job.completedAt",
            )?;
        }
        StorageLifetimeJobStateV2::Pending | StorageLifetimeJobStateV2::Attempting => {
            if job.failure_kind.is_some() || job.completed_at.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "pending or attempting storage lifetime v4 job cannot carry terminal evidence",
                ));
            }
        }
    }
    Ok(job)
}

pub fn validate_storage_lifetime_materialization_pair_v2(
    value: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV2, StorageLifetimeContractError> {
    let raw: StorageLifetimeMaterializationSnapshotV2 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let job = raw
        .job
        .as_ref()
        .map(|job| {
            serde_json::to_value(job)
                .map_err(|error| StorageLifetimeContractError::new(error.to_string()))
                .and_then(|value| validate_storage_lifetime_materialization_job_v2(&value))
        })
        .transpose()?;
    let valid = raw.materialization_state == StorageMaterializationStateV1::Pending
        || (raw.materialization_state == StorageMaterializationStateV1::Ready && job.is_none());
    if !valid {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 meta/job pairing is fail-closed",
        ));
    }
    Ok(StorageLifetimeMaterializationSnapshotV2 {
        materialization_state: raw.materialization_state,
        job,
    })
}

pub fn validate_storage_lifetime_reopen_pair_v2(
    value: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV2, StorageLifetimeContractError> {
    let snapshot = validate_storage_lifetime_materialization_pair_v2(value)?;
    if snapshot.job.as_ref().is_some_and(|job| {
        matches!(
            job.state,
            StorageLifetimeJobStateV2::Attempting | StorageLifetimeJobStateV2::Failed
        )
    }) {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 reopen refuses attempting or failed job",
        ));
    }
    Ok(snapshot)
}

pub fn validate_storage_lifetime_lifecycle_transition_v2(
    operation: StorageLifetimeLifecycleOperationV2,
    previous: &Value,
    next: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV2, StorageLifetimeContractError> {
    let previous = validate_storage_lifetime_materialization_pair_v2(previous)?;
    let next = validate_storage_lifetime_materialization_pair_v2(next)?;
    let previous_job = previous.job.as_ref();
    let next_job = next.job.as_ref();
    match operation {
        StorageLifetimeLifecycleOperationV2::Create => {
            if previous.materialization_state != StorageMaterializationStateV1::Pending
                || previous_job.is_some()
                || next.materialization_state != StorageMaterializationStateV1::Pending
                || next_job.map(|job| job.state) != Some(StorageLifetimeJobStateV2::Pending)
            {
                return lifecycle_error(
                    "storage lifetime v4 create requires pending absent to pending job",
                );
            }
            validate_storage_lifetime_cursor_transition_v2(
                None,
                &serde_json::to_value(job_cursor_v2(next_job.expect("checked")))
                    .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
            )?;
        }
        StorageLifetimeLifecycleOperationV2::Append => {
            let invalidate_ready = previous.materialization_state
                == StorageMaterializationStateV1::Ready
                && previous_job.is_none()
                && next.materialization_state == StorageMaterializationStateV1::Pending
                && next_job.is_none();
            if !invalidate_ready && previous != next {
                return lifecycle_error(
                    "storage lifetime v4 append must invalidate ready or preserve pending exactly",
                );
            }
        }
        StorageLifetimeLifecycleOperationV2::FailureRecordUnavailable => {
            if previous_job.map(|job| job.state) != Some(StorageLifetimeJobStateV2::Attempting)
                || previous != next
            {
                return lifecycle_error(
                    "failure record unavailable must leave the durable attempting sentinel unchanged",
                );
            }
        }
        StorageLifetimeLifecycleOperationV2::FinalPublish => {
            if previous.materialization_state != StorageMaterializationStateV1::Pending
                || previous_job.map(|job| (job.state, job.phase))
                    != Some((
                        StorageLifetimeJobStateV2::Pending,
                        StorageLifetimePhaseV2::VerifyBaselines,
                    ))
                || !previous_job.is_some_and(|job| job.scan_exhausted)
                || next.materialization_state != StorageMaterializationStateV1::Ready
                || next_job.is_some()
            {
                return lifecycle_error(
                    "final publication requires pending verifyBaselines and atomically publishes ready",
                );
            }
        }
        operation => {
            if previous.materialization_state != StorageMaterializationStateV1::Pending
                || next.materialization_state != StorageMaterializationStateV1::Pending
            {
                return lifecycle_error(format!(
                    "storage lifetime v4 {operation:?} requires a pending meta/job pair"
                ));
            }
            let previous_job = previous_job.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "storage lifetime v4 transition requires a previous job",
                )
            })?;
            let next_job = next_job.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "storage lifetime v4 transition requires a next job",
                )
            })?;
            require_same_job_identity_v2(previous_job, next_job)?;
            match operation {
                StorageLifetimeLifecycleOperationV2::Arm => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.state != StorageLifetimeJobStateV2::Attempting
                        || previous_job.scan_exhausted
                        || next_job.scan_exhausted
                        || job_cursor_v2(previous_job) != job_cursor_v2(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v4 arm must commit pending to attempting at the same cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV2::BatchSuccess => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                    {
                        return lifecycle_error(
                            "storage lifetime v4 batch success must commit attempting to pending",
                        );
                    }
                    validate_storage_lifetime_cursor_transition_v2(
                        Some(&serde_json::to_value(job_cursor_v2(previous_job)).map_err(
                            |error| StorageLifetimeContractError::new(error.to_string()),
                        )?),
                        &serde_json::to_value(job_cursor_v2(next_job)).map_err(|error| {
                            StorageLifetimeContractError::new(error.to_string())
                        })?,
                    )?;
                }
                StorageLifetimeLifecycleOperationV2::PhaseAdvance => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                    {
                        return lifecycle_error(
                            "storage lifetime v4 phase advance requires pending jobs",
                        );
                    }
                    validate_storage_lifetime_cursor_transition_v2(
                        Some(&serde_json::to_value(job_cursor_v2(previous_job)).map_err(
                            |error| StorageLifetimeContractError::new(error.to_string()),
                        )?),
                        &serde_json::to_value(job_cursor_v2(next_job)).map_err(|error| {
                            StorageLifetimeContractError::new(error.to_string())
                        })?,
                    )?;
                }
                StorageLifetimeLifecycleOperationV2::Cancel => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                        || job_cursor_v2(previous_job) != job_cursor_v2(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v4 cancellation must restore pending at the same cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV2::Fail => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Failed
                        || job_cursor_v2(previous_job) != job_cursor_v2(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v4 terminal failure must preserve the attempting cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV2::FinalReset => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || previous_job.phase != StorageLifetimePhaseV2::VerifyBaselines
                        || !previous_job.scan_exhausted
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.cutoff_event_rowid <= previous_job.cutoff_event_rowid
                        || next_job.phase != StorageLifetimePhaseV2::Receipts
                        || next_job.after_scan_rowid != 0
                        || next_job.scan_exhausted
                    {
                        return lifecycle_error(
                            "storage lifetime v4 final reset requires a larger cutoff and receipts at zero",
                        );
                    }
                }
                _ => return lifecycle_error("storage lifetime v4 lifecycle operation is invalid"),
            }
        }
    }
    Ok(next)
}

pub fn validate_storage_lifetime_verification_evidence_v2(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let object = value.as_object().ok_or_else(|| {
        StorageLifetimeContractError::new(
            "storage lifetime v4 verification evidence must be object",
        )
    })?;
    let expected_keys = [
        "phase",
        "cutoffEventRowid",
        "scannedRowids",
        "exactExpectedArtifacts",
        "currentProjectionEquivalent",
        "sequenceEquivalent",
        "sourceBacked",
        "noSourceLessExtra",
        "deterministic",
        "digestValid",
        "newestBaselinePresent",
    ];
    if object.len() != expected_keys.len()
        || expected_keys.iter().any(|key| !object.contains_key(*key))
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 verification evidence has unexpected shape",
        ));
    }
    let phase = object
        .get("phase")
        .and_then(Value::as_str)
        .ok_or_else(|| StorageLifetimeContractError::new("verification phase must be string"))?;
    if phase != "verifySource" && !STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V2.contains(&phase) {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 verification evidence phase is invalid",
        ));
    }
    let cutoff = object
        .get("cutoffEventRowid")
        .and_then(Value::as_u64)
        .ok_or_else(|| StorageLifetimeContractError::new("verification cutoff must be unsigned"))?;
    require_js_safe_unsigned(cutoff, "evidence.cutoffEventRowid")?;
    let rowids = object
        .get("scannedRowids")
        .and_then(Value::as_array)
        .ok_or_else(|| StorageLifetimeContractError::new("verification rowids must be array"))?;
    if rowids.len() > STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1 {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 verification evidence exceeds LIMIT 100",
        ));
    }
    let mut previous = None;
    for value in rowids {
        let rowid = value.as_u64().ok_or_else(|| {
            StorageLifetimeContractError::new("verification rowid must be unsigned")
        })?;
        require_js_safe_unsigned(rowid, "evidence.scannedRowid")?;
        if previous.is_some_and(|previous| rowid <= previous) {
            return Err(StorageLifetimeContractError::new(
                "verification evidence rowids must be strictly increasing",
            ));
        }
        if phase == "verifySource" && rowid > cutoff {
            return Err(StorageLifetimeContractError::new(
                "verifySource rowid cannot exceed cutoff",
            ));
        }
        previous = Some(rowid);
    }
    let flag = |name: &str| object.get(name).and_then(Value::as_bool);
    if phase == "verifySource"
        && (flag("exactExpectedArtifacts") != Some(true)
            || flag("currentProjectionEquivalent") != Some(true)
            || flag("sequenceEquivalent") != Some(true))
    {
        return Err(StorageLifetimeContractError::new(
            "verifySource must prove artifacts, current projection, and sequence",
        ));
    }
    if phase != "verifySource"
        && (flag("sourceBacked") != Some(true) || flag("noSourceLessExtra") != Some(true))
    {
        return Err(StorageLifetimeContractError::new(
            "derived verification must reject every source-less extra",
        ));
    }
    if phase == "verifyBaselines"
        && (flag("deterministic") != Some(true)
            || flag("digestValid") != Some(true)
            || flag("newestBaselinePresent") != Some(true))
    {
        return Err(StorageLifetimeContractError::new(
            "baseline verification requires deterministic digest-valid source-backed history and newest baseline",
        ));
    }
    Ok(())
}

pub fn validate_storage_lifetime_batch_policy_v3(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let expected = serde_json::json!({
        "schemaVersion": STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V3,
        "casesFile": "cases.json",
        "guards": {
            "maxRowsPerBatch": STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
            "maxCanonicalPayloadBytesPerBatch":
                STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
            "callerConfigurable": false,
            "appliesToPhases": STORAGE_LIFETIME_PHASES_V3,
        },
        "cursor": {
            "schemaVersion": STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3,
            "column": "after_scan_rowid",
            "phases": STORAGE_LIFETIME_PHASES_V3,
            "eventRowidPhases": STORAGE_LIFETIME_EVENT_ROWID_PHASES_V3,
            "projectionRowidPhases": STORAGE_LIFETIME_PROJECTION_ROWID_PHASES_V3,
            "derivedRowidPhases": STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V3,
            "phaseStartAfterScanRowid": 0,
            "phaseStartScanExhausted": false,
            "scanExhaustedMutationAuthority":
                "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
            "phaseTransition": "scanExhaustedTrueThenExactNextAtZeroAndFalse",
        },
        "scan": {
            "sourcePredicate": "rowid > afterScanRowid AND rowid <= cutoffEventRowid",
            "tablePredicate": "rowid > afterScanRowid",
            "orderBy": "rowid",
            "limitSql": STORAGE_LIFETIME_KEYSET_LIMIT_SQL_V2,
            "privateEmptyWitness": "sameBeginImmediateTransaction",
            "forbidden": [
                "OFFSET", "COUNT", "window", "GROUP BY", "antiJoin",
                "unboundedFullTableScan", "unboundedNamespaceQuery",
            ],
        },
        "maxEventRowidFence": {
            "transaction": "BEGIN IMMEDIATE",
            "requiredBefore": [
                "enterVerifySource", "everyProjectionOrDerivedVerificationBatch",
                "finalPublication",
            ],
            "equal": "continue",
            "less": "contractError",
            "greater": "sameSingletonPendingReceiptsNewCutoffZeroUnexhaustedNoFailure",
            "frozenMaterializationPhases": "mayContinueAtExistingCutoff",
            "appendSerialization": "BEGIN IMMEDIATESerializesAppendDuringBatch",
            "normalWorkerCheckState": "pendingBeforeArm",
            "armToBatch": "sameSynchronousSidecarOwnerImmediatelyEntersBatch",
            "attemptingCatchUpReset": "forbidden",
            "attemptingOnReopen": "failClosed",
        },
        "verification": {
            "verifySource": {
                "boundedBy": "eventsRowidAtOrBelowCutoff",
                "requires": [
                    "exactAggregateSequenceAndPredecessor", "requiredDerivedPresence",
                    "latestEventHasProjectionRow", "appInstanceSourceHasAppInstancesRow",
                ],
                "forbidden": ["liveProjectionTableFullScan", "antiJoin"],
            },
            "projectionPhaseTables": {
                "verifyProjects": "projects",
                "verifyChats": "chats",
                "verifyRuns": "runs",
                "verifyLocks": "locks",
                "verifyAppInstances": "app_instances",
            },
            "projectionRule":
                "eachRowMatchesLatestExactSourceEventAtOrBelowCutoffIncludingMirroredAndIndexedColumns",
            "projectionColumns": {
                "projects": ["project_id", "canonical_path", "projection_json", "last_event_sequence"],
                "chats": ["chat_id", "project_id", "projection_json", "last_event_sequence"],
                "runs": [
                    "run_id", "chat_id", "state", "worktree_path", "projection_json",
                    "last_event_sequence",
                ],
                "locks": [
                    "lock_id", "project_id", "writer_run_id", "worktree_path", "poisoned",
                    "recovery_required", "evidence_json", "projection_json",
                    "last_event_sequence",
                ],
                "app_instances": [
                    "sidecar_instance_id", "pid", "platform", "started_at", "heartbeat_at",
                    "clean_shutdown_at", "last_event_sequence",
                ],
            },
            "derivedPhaseTables": {
                "verifyReceipts": "operation_receipts",
                "verifyMessages": "chat_messages",
                "verifyWorkspaces": "prepared_workspaces",
                "verifyBaselines": "ledger_baselines",
            },
            "missingProjectionDetection": "verifySourceLatestSourcePresence",
            "extraOrSourceLessDetection": "exactProjectionTablePhase",
            "historicalBaseline": "allowedWhenDeterministicDigestValidAndSourceBacked",
            "newestBaseline": "requiredForEveryCompactableAggregate",
        },
        "snapshotAggregates": {
            "includedProjectionTables": ["projects", "chats", "runs", "locks"],
            "excludedProjectionTables": ["app_instances"],
            "appInstancesExclusionReason": "lifecycleProjectionNotMaterializedSnapshotAggregate",
        },
        "transaction": {
            "arm": "pendingToAttemptingCommittedBeforeBatch",
            "success": "verifiedRowsCursorAndAttemptingToPendingAtomic",
            "nonemptyBatchScanExhausted": false,
            "emptyBatchScanExhausted": "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
            "cancellation": "rollbackBatchOrSavepointThenCommitAttemptingToPending",
            "terminalValidationFailure": "rollbackBatchThenCommitAttemptingToFailed",
            "failureRecordUnavailable": "durableAttemptingSentinelReopenRefusesRetryOrSuccess",
            "reopenAttempting": "failClosedNonRunnable",
            "reopenFailed": "failClosedNonRunnable",
        },
        "finalPublication": {
            "transaction": "shortBeginImmediateOnly",
            "preconditions": [
                "exactMetaJobPairAndCursor",
                "verifyBaselinesTableEndProvedByExactEmptyKeysetBatch",
                "scanExhaustedTrue", "maxEventRowidEqualsCutoff", "cancelBeforeMutation",
                "precommitCancel",
            ],
            "mutation": "deleteJobAndSetMetaReady",
            "forbiddenWork": ["artifactScan", "tableScan", "count"],
        },
        "activation": {
            "cleanupEnabled": false,
            "deletionEnabled": false,
            "compactionEnabled": false,
            "wireEnabled": false,
            "schedulerEnabled": false,
        },
    });
    if value != &expected {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 policy contradicts bounded projection verification",
        ));
    }
    Ok(())
}

pub fn validate_storage_lifetime_cursor_v3(
    value: &Value,
) -> Result<StorageLifetimeCursorV3, StorageLifetimeContractError> {
    let cursor: StorageLifetimeCursorV3 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_storage_lifetime_cursor_fields_v3(&cursor)?;
    Ok(cursor)
}

pub fn validate_storage_lifetime_cursor_transition_v3(
    previous: Option<&Value>,
    next: &Value,
) -> Result<StorageLifetimeCursorV3, StorageLifetimeContractError> {
    let next = validate_storage_lifetime_cursor_v3(next)?;
    let Some(previous) = previous else {
        if next.phase != StorageLifetimePhaseV3::Receipts
            || next.after_scan_rowid != 0
            || next.scan_exhausted
        {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime v5 initial cursor must start unexhausted receipts at zero",
            ));
        }
        return Ok(next);
    };
    let previous = validate_storage_lifetime_cursor_v3(previous)?;
    if next.cutoff_event_rowid != previous.cutoff_event_rowid {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 batch transition cannot change the frozen cutoff",
        ));
    }
    if next.phase == previous.phase {
        if previous.scan_exhausted
            || next.scan_exhausted
            || next.after_scan_rowid <= previous.after_scan_rowid
        {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime v5 same-phase cursor must advance afterScanRowid",
            ));
        }
        return Ok(next);
    }
    if !previous.scan_exhausted
        || next.scan_exhausted
        || next_phase_v3(previous.phase) != Some(next.phase)
        || next.after_scan_rowid != 0
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 phase transition requires exhausted current phase and unexhausted exact next phase at zero",
        ));
    }
    Ok(next)
}

pub fn validate_storage_lifetime_materialization_job_v3(
    value: &Value,
) -> Result<StorageLifetimeMaterializationJobV3, StorageLifetimeContractError> {
    let job: StorageLifetimeMaterializationJobV3 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if job.singleton != 1 || job.job_id.is_empty() {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 job requires singleton 1 and non-empty jobId",
        ));
    }
    validate_storage_lifetime_cursor_fields_v3(&job_cursor_v3(&job))?;
    require_timestamp(&job.created_at, "job.createdAt")?;
    require_timestamp(&job.updated_at, "job.updatedAt")?;
    match job.state {
        StorageLifetimeJobStateV2::Failed => {
            if job
                .failure_kind
                .as_deref()
                .is_none_or(|kind| !is_sanitized_failure_kind(kind))
            {
                return Err(StorageLifetimeContractError::new(
                    "failed storage lifetime v5 job requires sanitized failureKind",
                ));
            }
            require_timestamp(
                job.completed_at.as_deref().unwrap_or_default(),
                "job.completedAt",
            )?;
        }
        StorageLifetimeJobStateV2::Pending | StorageLifetimeJobStateV2::Attempting => {
            if job.failure_kind.is_some() || job.completed_at.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "pending or attempting storage lifetime v5 job cannot carry terminal evidence",
                ));
            }
        }
    }
    Ok(job)
}

pub fn validate_storage_lifetime_materialization_pair_v3(
    value: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV3, StorageLifetimeContractError> {
    let raw: StorageLifetimeMaterializationSnapshotV3 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let job = raw
        .job
        .as_ref()
        .map(|job| {
            serde_json::to_value(job)
                .map_err(|error| StorageLifetimeContractError::new(error.to_string()))
                .and_then(|value| validate_storage_lifetime_materialization_job_v3(&value))
        })
        .transpose()?;
    if raw.materialization_state != StorageMaterializationStateV1::Pending
        && !(raw.materialization_state == StorageMaterializationStateV1::Ready && job.is_none())
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 meta/job pairing is fail-closed",
        ));
    }
    Ok(StorageLifetimeMaterializationSnapshotV3 {
        materialization_state: raw.materialization_state,
        job,
    })
}

pub fn validate_storage_lifetime_reopen_pair_v3(
    value: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV3, StorageLifetimeContractError> {
    let snapshot = validate_storage_lifetime_materialization_pair_v3(value)?;
    if snapshot.job.as_ref().is_some_and(|job| {
        matches!(
            job.state,
            StorageLifetimeJobStateV2::Attempting | StorageLifetimeJobStateV2::Failed
        )
    }) {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 reopen refuses attempting or failed job",
        ));
    }
    Ok(snapshot)
}

pub fn validate_storage_lifetime_lifecycle_transition_v3(
    operation: StorageLifetimeLifecycleOperationV3,
    previous: &Value,
    next: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV3, StorageLifetimeContractError> {
    let previous = validate_storage_lifetime_materialization_pair_v3(previous)?;
    let next = validate_storage_lifetime_materialization_pair_v3(next)?;
    let previous_job = previous.job.as_ref();
    let next_job = next.job.as_ref();
    match operation {
        StorageLifetimeLifecycleOperationV3::Create => {
            if previous.materialization_state != StorageMaterializationStateV1::Pending
                || previous_job.is_some()
                || next.materialization_state != StorageMaterializationStateV1::Pending
                || next_job.map(|job| job.state) != Some(StorageLifetimeJobStateV2::Pending)
            {
                return lifecycle_error(
                    "storage lifetime v5 create requires pending absent to pending job",
                );
            }
            validate_storage_lifetime_cursor_transition_v3(
                None,
                &serde_json::to_value(job_cursor_v3(next_job.expect("checked")))
                    .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
            )?;
        }
        StorageLifetimeLifecycleOperationV3::Append => {
            let invalidate_ready = previous.materialization_state
                == StorageMaterializationStateV1::Ready
                && previous_job.is_none()
                && next.materialization_state == StorageMaterializationStateV1::Pending
                && next_job.is_none();
            if !invalidate_ready && previous != next {
                return lifecycle_error(
                    "storage lifetime v5 append must invalidate ready or preserve pending exactly",
                );
            }
        }
        StorageLifetimeLifecycleOperationV3::FailureRecordUnavailable => {
            if previous_job.map(|job| job.state) != Some(StorageLifetimeJobStateV2::Attempting)
                || previous != next
            {
                return lifecycle_error(
                    "failure record unavailable must leave the durable attempting sentinel unchanged",
                );
            }
        }
        StorageLifetimeLifecycleOperationV3::FinalPublish => {
            if previous_job.map(|job| (job.state, job.phase, job.scan_exhausted))
                != Some((
                    StorageLifetimeJobStateV2::Pending,
                    StorageLifetimePhaseV3::VerifyBaselines,
                    true,
                ))
                || next.materialization_state != StorageMaterializationStateV1::Ready
                || next_job.is_some()
            {
                return lifecycle_error(
                    "storage lifetime v5 final publication requires exhausted verifyBaselines",
                );
            }
        }
        operation => {
            let previous_job = previous_job.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "storage lifetime v5 transition requires a previous job",
                )
            })?;
            let next_job = next_job.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "storage lifetime v5 transition requires a next job",
                )
            })?;
            require_same_job_identity_v3(previous_job, next_job)?;
            match operation {
                StorageLifetimeLifecycleOperationV3::Arm => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.state != StorageLifetimeJobStateV2::Attempting
                        || previous_job.scan_exhausted
                        || next_job.scan_exhausted
                        || job_cursor_v3(previous_job) != job_cursor_v3(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v5 arm must preserve an unexhausted cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV3::BatchSuccess => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                    {
                        return lifecycle_error(
                            "storage lifetime v5 batch success must commit attempting to pending",
                        );
                    }
                    validate_storage_lifetime_cursor_transition_v3(
                        Some(&serde_json::to_value(job_cursor_v3(previous_job)).map_err(
                            |error| StorageLifetimeContractError::new(error.to_string()),
                        )?),
                        &serde_json::to_value(job_cursor_v3(next_job)).map_err(|error| {
                            StorageLifetimeContractError::new(error.to_string())
                        })?,
                    )?;
                }
                StorageLifetimeLifecycleOperationV3::PhaseAdvance => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                    {
                        return lifecycle_error(
                            "storage lifetime v5 phase advance requires pending jobs",
                        );
                    }
                    validate_storage_lifetime_cursor_transition_v3(
                        Some(&serde_json::to_value(job_cursor_v3(previous_job)).map_err(
                            |error| StorageLifetimeContractError::new(error.to_string()),
                        )?),
                        &serde_json::to_value(job_cursor_v3(next_job)).map_err(|error| {
                            StorageLifetimeContractError::new(error.to_string())
                        })?,
                    )?;
                }
                StorageLifetimeLifecycleOperationV3::Cancel => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                        || job_cursor_v3(previous_job) != job_cursor_v3(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v5 cancellation must restore the same cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV3::Fail => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Failed
                        || job_cursor_v3(previous_job) != job_cursor_v3(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v5 terminal failure must preserve the cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV3::CatchUpReset => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || !is_verification_phase_v3(previous_job.phase)
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.cutoff_event_rowid <= previous_job.cutoff_event_rowid
                        || next_job.phase != StorageLifetimePhaseV3::Receipts
                        || next_job.after_scan_rowid != 0
                        || next_job.scan_exhausted
                    {
                        return lifecycle_error(
                            "storage lifetime v5 catch-up reset requires verification drift and a larger cutoff",
                        );
                    }
                }
                _ => return lifecycle_error("storage lifetime v5 lifecycle operation is invalid"),
            }
        }
    }
    Ok(next)
}

pub fn validate_storage_lifetime_batch_policy_v4(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let expected = serde_json::json!({
        "schemaVersion": STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V4,
        "casesFile": "cases.json",
        "guards": {
            "maxRowsPerBatch": STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
            "maxCanonicalPayloadBytesPerBatch":
                STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
            "callerConfigurable": false,
            "appliesToPhases": STORAGE_LIFETIME_PHASES_V4,
        },
        "cursor": {
            "schemaVersion": STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4,
            "column": "after_scan_rowid",
            "phases": STORAGE_LIFETIME_PHASES_V4,
            "eventRowidPhases": STORAGE_LIFETIME_EVENT_ROWID_PHASES_V4,
            "projectionRowidPhases": STORAGE_LIFETIME_PROJECTION_ROWID_PHASES_V4,
            "derivedRowidPhases": STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V4,
            "phaseStartAfterScanRowid": 0,
            "phaseStartScanExhausted": false,
            "scanExhaustedMutationAuthority":
                "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
            "phaseTransition": "scanExhaustedTrueThenExactNextAtZeroAndFalse",
        },
        "sourceIndex": {
            "table": "event_source_index",
            "columns": STORAGE_EVENT_SOURCE_INDEX_COLUMNS_V1,
            "identity": "eventRowidEventIdOperationIdAndAggregateTupleExact",
            "introducedMessageId": "nullableOnlyWhenThisEventIntroducesExactTeamMessage",
            "chatDurableSequence": "nullableExactDurableEventSequenceFromCanonicalChatPayload",
            "chatSnapshotMessageId": "nullableExactLastMessageIdFromCanonicalChatPayload",
            "foreignKeys": [
                "event_id->events.event_id",
                "operation_id->events.operation_id",
                "aggregateTuple->events.aggregateTuple",
            ],
            "foreignKeyEnforcement": "PRAGMA foreign_keys=ONOnEveryConnection",
            "insertGuard": "event_source_index_requires_exact_event",
            "sameEventBinding":
                "eventRowidIdentityAggregateAndChatSnapshotFieldsMustMatchOneEventsRow",
            "separateForeignKeyCrossEventMix": "rejectedByInsertGuard",
            "introductionTransition": {
                "firstPredecessor": "durableZeroAndSnapshotNull",
                "message": "currentDurableEqualsPreviousPlusOneAndValidTeamMessageEventSequenceEqualsCurrentAndChatIdEqualsAggregateIdAndIntroducedEqualsSnapshotMessageId",
                "nonMessage": "durableUnchangedAndIntroducedNullAndSnapshotNullOrSameAsPreviousSnapshot",
                "reject": [
                    "durableDecrease", "durableJump", "increaseWithoutValidMessage",
                    "unchangedWithChangedSnapshotMessageId", "doubleIntroduction",
                ],
            },
            "predecessorLookup": {
                "indexed": "exactAggregateTypeIdAndAggregateSequenceMinusOne",
                "missingDuringMigrationPendingAppend":
                    "exactPredecessorEventsAggregateTupleLookupOnly",
                "insertGuardFallbackEnforcement":
                    "exactPredecessorEventMustExistAndBeValidThenSameTransitionOrReject",
                "forbidden": ["scan", "sort", "latestLookup"],
            },
            "historyBackfillByMigration": false,
            "historyBackfillPhase": "indexSource",
            "indexSourceWrite": "insertOrExactExistingNoOpOtherwiseConflict",
            "verificationPhase": "verifySourceIndex",
            "aggregateLatestIndex": "event_source_index_aggregate_latest",
            "introducedMessageUniqueIndex": "event_source_index_message_unique",
            "mutationGuards": [
                "event_source_index_requires_exact_event",
                "event_source_index_append_only_update",
                "event_source_index_append_only_delete",
            ],
        },
        "scan": {
            "sourcePredicate": "rowid > afterScanRowid AND rowid <= cutoffEventRowid",
            "sourceIndexPredicate":
                "event_rowid > afterScanRowid AND event_rowid <= cutoffEventRowid",
            "tablePredicate": "rowid > afterScanRowid",
            "orderBy": "rowid",
            "limitSql": STORAGE_LIFETIME_KEYSET_LIMIT_SQL_V2,
            "privateEmptyWitness": "sameBeginImmediateTransaction",
            "forbidden": [
                "OFFSET", "COUNT", "window", "GROUP BY", "antiJoin",
                "unboundedFullTableScan", "unboundedNamespaceQuery",
            ],
        },
        "appendAtomicity": {
            "transaction": "sameBeginImmediateAsEventProjectionAndReadyInvalidation",
            "eventInsert": "requiresExactSourceIndexInsertBeforeCommit",
            "duplicateNoOp": "noSourceIndexMutation",
            "conflict": "rollbackEventProjectionSourceIndexAndReadiness",
            "introducedMessageIdentityConflict": "rollbackWholeAppend",
        },
        "workspaceProvenance": {
            "lookup": "latestProjectSourceByAggregateLatestIndex",
            "payloadFetch": "singleEventsRowidLookup",
            "payloadLimitBytes": STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
            "comparison": "rustExactCanonicalProjectPayloadAndWorkspaceProjectId",
            "stopWhen":
                "aggregateRemovalOrTombstoneBecomesLegalUntilSourceIndexSemanticsAreVersioned",
        },
        "queryPlanAcceptance": {
            "mode": "exactExplainQueryPlanDetail",
            "indexSource": "SEARCH events USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)",
            "verifySourceIndex":
                "SEARCH event_source_index USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)",
            "latestAggregate":
                "SEARCH event_source_index USING COVERING INDEX event_source_index_aggregate_latest (aggregate_type=? AND aggregate_id=?)",
            "introducedMessageIdentity":
                "SEARCH event_source_index USING COVERING INDEX event_source_index_message_unique (introduced_message_id=?)",
            "indexedPredecessor":
                "SEARCH event_source_index USING INDEX sqlite_autoindex_event_source_index_3 (aggregate_type=? AND aggregate_id=? AND aggregate_sequence=?)",
            "missingIndexedPredecessorEvent":
                "SEARCH events USING INDEX sqlite_autoindex_events_3 (aggregate_type=? AND aggregate_id=? AND aggregate_sequence=?)",
            "payloadByRowid": "SEARCH events USING INTEGER PRIMARY KEY (rowid=?)",
            "reject": ["SCAN events", "SCAN event_source_index", "USE TEMP B-TREE"],
        },
        "maxEventRowidFence": {
            "transaction": "BEGIN IMMEDIATE",
            "requiredBefore": [
                "enterVerifySourceIndex", "enterVerifySource",
                "everyProjectionOrDerivedVerificationBatch", "finalPublication",
            ],
            "equal": "continue",
            "less": "contractError",
            "greater": "sameSingletonPendingIndexSourceNewCutoffZeroUnexhaustedNoFailure",
            "frozenMaterializationPhases": "mayContinueAtExistingCutoff",
            "appendSerialization": "BEGIN IMMEDIATESerializesAppendDuringBatch",
            "normalWorkerCheckState": "pendingBeforeArm",
            "armToBatch": "sameSynchronousSidecarOwnerImmediatelyEntersBatch",
            "attemptingCatchUpReset": "forbidden",
            "attemptingOnReopen": "failClosed",
        },
        "verification": {
            "verifySourceIndex": {
                "boundedBy": "eventSourceIndexEventRowidAtOrBelowCutoff",
                "requires": [
                    "oneExactIndexRowPerEvent", "exactIdentityAndAggregateTuple",
                    "exactNullableMessageId", "noSourceLessIndexRow",
                ],
            },
            "verifySource": {
                "boundedBy": "eventsRowidAtOrBelowCutoff",
                "requires": [
                    "exactAggregateSequenceAndPredecessor", "requiredDerivedPresence",
                    "latestEventHasProjectionRow", "appInstanceSourceHasAppInstancesRow",
                ],
                "forbidden": ["liveProjectionTableFullScan", "antiJoin"],
            },
            "projectionPhaseTables": {
                "verifyProjects": "projects",
                "verifyChats": "chats",
                "verifyRuns": "runs",
                "verifyLocks": "locks",
                "verifyAppInstances": "app_instances",
            },
            "projectionRule":
                "eachRowMatchesLatestExactSourceEventAtOrBelowCutoffIncludingMirroredAndIndexedColumns",
            "projectionColumns": {
                "projects": ["project_id", "canonical_path", "projection_json", "last_event_sequence"],
                "chats": ["chat_id", "project_id", "projection_json", "last_event_sequence"],
                "runs": [
                    "run_id", "chat_id", "state", "worktree_path", "projection_json",
                    "last_event_sequence",
                ],
                "locks": [
                    "lock_id", "project_id", "writer_run_id", "worktree_path", "poisoned",
                    "recovery_required", "evidence_json", "projection_json",
                    "last_event_sequence",
                ],
                "app_instances": [
                    "sidecar_instance_id", "pid", "platform", "started_at", "heartbeat_at",
                    "clean_shutdown_at", "last_event_sequence",
                ],
            },
            "derivedPhaseTables": {
                "verifyReceipts": "operation_receipts",
                "verifyMessages": "chat_messages",
                "verifyWorkspaces": "prepared_workspaces",
                "verifyBaselines": "ledger_baselines",
            },
            "missingProjectionDetection": "verifySourceLatestSourcePresence",
            "extraOrSourceLessDetection": "exactProjectionTablePhase",
            "historicalBaseline": "allowedWhenDeterministicDigestValidAndSourceBacked",
            "newestBaseline": "requiredForEveryCompactableAggregate",
        },
        "snapshotAggregates": {
            "includedProjectionTables": ["projects", "chats", "runs", "locks"],
            "excludedProjectionTables": ["app_instances", "event_source_index"],
            "exclusionReason": "lifecycleAndSourceIndexTablesAreNotMaterializedSnapshotAggregates",
        },
        "transaction": {
            "arm": "pendingToAttemptingCommittedBeforeBatch",
            "success": "verifiedRowsCursorAndAttemptingToPendingAtomic",
            "nonemptyBatchScanExhausted": false,
            "emptyBatchScanExhausted": "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
            "cancellation": "rollbackBatchOrSavepointThenCommitAttemptingToPending",
            "terminalValidationFailure": "rollbackBatchThenCommitAttemptingToFailed",
            "failureRecordUnavailable": "durableAttemptingSentinelReopenRefusesRetryOrSuccess",
            "reopenAttempting": "failClosedNonRunnable",
            "reopenFailed": "failClosedNonRunnable",
        },
        "finalPublication": {
            "transaction": "shortBeginImmediateOnly",
            "preconditions": [
                "exactMetaJobPairAndCursor",
                "verifyBaselinesTableEndProvedByExactEmptyKeysetBatch",
                "scanExhaustedTrue", "maxEventRowidEqualsCutoff", "cancelBeforeMutation",
                "precommitCancel",
            ],
            "mutation": "deleteJobAndSetMetaReady",
            "forbiddenWork": ["artifactScan", "tableScan", "count"],
        },
        "activation": {
            "cleanupEnabled": false,
            "deletionEnabled": false,
            "compactionEnabled": false,
            "wireEnabled": false,
            "schedulerEnabled": false,
        },
    });
    if value != &expected {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 policy contradicts indexed bounded verification",
        ));
    }
    Ok(())
}

pub fn validate_storage_event_source_index_entry_v1(
    value: &Value,
) -> Result<StorageEventSourceIndexEntryV1, StorageLifetimeContractError> {
    let entry: StorageEventSourceIndexEntryV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    require_js_safe_unsigned(entry.event_rowid, "sourceIndex.eventRowid")?;
    require_js_safe_unsigned(entry.aggregate_sequence, "sourceIndex.aggregateSequence")?;
    if entry.event_rowid == 0
        || entry.aggregate_sequence == 0
        || entry.event_id.is_empty()
        || entry.operation_id.is_empty()
        || entry.aggregate_type.is_empty()
        || entry.aggregate_id.is_empty()
        || entry
            .introduced_message_id
            .as_deref()
            .is_some_and(str::is_empty)
        || entry
            .chat_snapshot_message_id
            .as_deref()
            .is_some_and(str::is_empty)
    {
        return Err(StorageLifetimeContractError::new(
            "event source index identity and aggregate tuple must be non-empty and positive",
        ));
    }
    if let Some(sequence) = entry.chat_durable_sequence {
        require_js_safe_unsigned(sequence, "sourceIndex.chatDurableSequence")?;
    }
    Ok(entry)
}

fn storage_chat_source_state_v1(
    source: &LedgerEventV1,
) -> Result<(u64, Option<String>, Option<Value>), StorageLifetimeContractError> {
    let payload: Value = serde_json::from_str(&source.canonical_json_payload).map_err(|error| {
        StorageLifetimeContractError::new(format!(
            "source index event payload must be valid JSON: {error}"
        ))
    })?;
    let object = payload.as_object().ok_or_else(|| {
        StorageLifetimeContractError::new("chat source index payload must be an object")
    })?;
    let durable = object
        .get("durable_event_sequence")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            StorageLifetimeContractError::new(
                "chat source index requires a safe unsigned durable_event_sequence",
            )
        })?;
    require_js_safe_unsigned(durable, "sourceIndex.chatDurableSequence")?;
    let message = match object.get("last_message") {
        Some(Value::Null) => None,
        Some(message @ Value::Object(_)) => {
            validate_g5_payload_v1("teamMessage", message).map_err(|_| {
                StorageLifetimeContractError::new(
                    "chat source index requires a valid last_message TeamMessage",
                )
            })?;
            Some(message.clone())
        }
        _ => {
            return Err(StorageLifetimeContractError::new(
                "chat source index last_message must be an object or null",
            ));
        }
    };
    let snapshot_message_id = message
        .as_ref()
        .and_then(|value| value.get("messageId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok((durable, snapshot_message_id, message))
}

pub fn validate_storage_event_source_index_binding_v1(
    event_rowid: u64,
    entry_value: &Value,
    source: &LedgerEventV1,
) -> Result<StorageEventSourceIndexEntryV1, StorageLifetimeContractError> {
    let entry = validate_storage_event_source_index_entry_v1(entry_value)?;
    let (expected_durable, expected_snapshot) = if source.aggregate_type == "chat" {
        let (durable, snapshot, _) = storage_chat_source_state_v1(source)?;
        (Some(durable), snapshot)
    } else {
        (None, None)
    };
    if entry.event_rowid != event_rowid
        || entry.event_id != source.event_id
        || entry.operation_id != source.operation_id
        || entry.aggregate_type != source.aggregate_type
        || entry.aggregate_id != source.aggregate_id
        || entry.aggregate_sequence != source.aggregate_sequence
        || entry.chat_durable_sequence != expected_durable
        || entry.chat_snapshot_message_id != expected_snapshot
        || (source.aggregate_type != "chat" && entry.introduced_message_id.is_some())
    {
        return Err(StorageLifetimeContractError::new(
            "event source index must match one exact source event and mirrored chat snapshot",
        ));
    }
    Ok(entry)
}

pub fn validate_storage_event_source_index_transition_v1(
    event_rowid: u64,
    entry_value: &Value,
    source: &LedgerEventV1,
    predecessor_entry_value: Option<&Value>,
    predecessor_source: Option<&LedgerEventV1>,
) -> Result<StorageEventSourceIndexEntryV1, StorageLifetimeContractError> {
    let entry = validate_storage_event_source_index_binding_v1(event_rowid, entry_value, source)?;
    if source.aggregate_type != "chat" {
        return Ok(entry);
    }
    let (current_durable, current_snapshot, current_message) =
        storage_chat_source_state_v1(source)?;
    let (previous_durable, previous_snapshot) = if source.aggregate_sequence == 1 {
        if predecessor_entry_value.is_some() || predecessor_source.is_some() {
            return Err(StorageLifetimeContractError::new(
                "first chat source index row must use the synthetic zero predecessor",
            ));
        }
        (0, None)
    } else if let Some(predecessor_value) = predecessor_entry_value {
        if predecessor_source.is_some() {
            return Err(StorageLifetimeContractError::new(
                "chat source index predecessor must use exactly one lookup path",
            ));
        }
        let predecessor = validate_storage_event_source_index_entry_v1(predecessor_value)?;
        if predecessor.aggregate_type != "chat"
            || predecessor.aggregate_id != source.aggregate_id
            || predecessor.aggregate_sequence + 1 != source.aggregate_sequence
        {
            return Err(StorageLifetimeContractError::new(
                "indexed predecessor must be the exact aggregate sequence minus one",
            ));
        }
        (
            predecessor.chat_durable_sequence.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "indexed chat predecessor must mirror durable sequence",
                )
            })?,
            predecessor.chat_snapshot_message_id,
        )
    } else if let Some(predecessor) = predecessor_source {
        if predecessor.aggregate_type != "chat"
            || predecessor.aggregate_id != source.aggregate_id
            || predecessor.aggregate_sequence + 1 != source.aggregate_sequence
        {
            return Err(StorageLifetimeContractError::new(
                "fallback predecessor event must be the exact aggregate sequence minus one",
            ));
        }
        let (durable, snapshot, _) = storage_chat_source_state_v1(predecessor)?;
        (durable, snapshot)
    } else {
        return Err(StorageLifetimeContractError::new(
            "chat source index transition requires an exact predecessor lookup",
        ));
    };

    if previous_durable < MAX_SEQUENCE && current_durable == previous_durable + 1 {
        let message = current_message.as_ref().ok_or_else(|| {
            StorageLifetimeContractError::new(
                "durable sequence increase requires a valid introduced TeamMessage",
            )
        })?;
        let message_sequence = message.get("eventSequence").and_then(Value::as_u64);
        let message_chat_id = message.get("chatId").and_then(Value::as_str);
        if message_sequence != Some(current_durable)
            || message_chat_id != Some(source.aggregate_id.as_str())
            || entry.introduced_message_id != current_snapshot
        {
            return Err(StorageLifetimeContractError::new(
                "message introduction must match current durable sequence, chat, and snapshot",
            ));
        }
    } else if current_durable == previous_durable {
        if entry.introduced_message_id.is_some()
            || current_snapshot
                .as_ref()
                .is_some_and(|message_id| Some(message_id) != previous_snapshot.as_ref())
        {
            return Err(StorageLifetimeContractError::new(
                "non-message transition must keep durable sequence and null-or-same snapshot",
            ));
        }
    } else {
        return Err(StorageLifetimeContractError::new(
            "chat durable sequence must stay unchanged or increase by exactly one",
        ));
    }
    Ok(entry)
}

pub fn validate_storage_lifetime_workspace_project_source_v1(
    project_id: &str,
    source_aggregate_type: &str,
    source_aggregate_id: &str,
    canonical_payload: &str,
) -> Result<Value, StorageLifetimeContractError> {
    if project_id.is_empty()
        || source_aggregate_type != "project"
        || source_aggregate_id != project_id
    {
        return Err(StorageLifetimeContractError::new(
            "workspace provenance requires the indexed latest source for the exact project identity",
        ));
    }
    if canonical_payload.len()
        > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 as usize
    {
        return Err(StorageLifetimeContractError::new(
            "workspace project source payload exceeds 1 MiB",
        ));
    }
    let payload: Value = serde_json::from_str(canonical_payload).map_err(|error| {
        StorageLifetimeContractError::new(format!(
            "workspace project source payload must be valid JSON: {error}"
        ))
    })?;
    let canonical = canonical_json_v1(payload.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let object = payload.as_object().ok_or_else(|| {
        StorageLifetimeContractError::new("workspace project source payload must be an object")
    })?;
    if canonical != canonical_payload
        || object.get("project_id").and_then(Value::as_str) != Some(project_id)
        || object
            .get("canonical_path")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(StorageLifetimeContractError::new(
            "workspace provenance requires an exact canonical project payload",
        ));
    }
    Ok(payload)
}

pub fn validate_storage_lifetime_cursor_v4(
    value: &Value,
) -> Result<StorageLifetimeCursorV4, StorageLifetimeContractError> {
    let cursor: StorageLifetimeCursorV4 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_storage_lifetime_cursor_fields_v4(&cursor)?;
    Ok(cursor)
}

pub fn validate_storage_lifetime_cursor_transition_v4(
    previous: Option<&Value>,
    next: &Value,
) -> Result<StorageLifetimeCursorV4, StorageLifetimeContractError> {
    let next = validate_storage_lifetime_cursor_v4(next)?;
    let Some(previous) = previous else {
        if next.phase != StorageLifetimePhaseV4::IndexSource
            || next.after_scan_rowid != 0
            || next.scan_exhausted
        {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime v6 initial cursor must start unexhausted indexSource at zero",
            ));
        }
        return Ok(next);
    };
    let previous = validate_storage_lifetime_cursor_v4(previous)?;
    if next.cutoff_event_rowid != previous.cutoff_event_rowid {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 batch transition cannot change the frozen cutoff",
        ));
    }
    if next.phase == previous.phase {
        if previous.scan_exhausted
            || next.scan_exhausted
            || next.after_scan_rowid <= previous.after_scan_rowid
        {
            return Err(StorageLifetimeContractError::new(
                "storage lifetime v6 same-phase cursor must advance afterScanRowid",
            ));
        }
        return Ok(next);
    }
    if !previous.scan_exhausted
        || next.scan_exhausted
        || next_phase_v4(previous.phase) != Some(next.phase)
        || next.after_scan_rowid != 0
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 phase transition requires exhausted current phase and unexhausted exact next phase at zero",
        ));
    }
    Ok(next)
}

pub fn validate_storage_lifetime_materialization_job_v4(
    value: &Value,
) -> Result<StorageLifetimeMaterializationJobV4, StorageLifetimeContractError> {
    let job: StorageLifetimeMaterializationJobV4 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if job.singleton != 1 || job.job_id.is_empty() {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 job requires singleton 1 and non-empty jobId",
        ));
    }
    validate_storage_lifetime_cursor_fields_v4(&job_cursor_v4(&job))?;
    require_timestamp(&job.created_at, "job.createdAt")?;
    require_timestamp(&job.updated_at, "job.updatedAt")?;
    match job.state {
        StorageLifetimeJobStateV2::Failed => {
            if job
                .failure_kind
                .as_deref()
                .is_none_or(|kind| !is_sanitized_failure_kind(kind))
            {
                return Err(StorageLifetimeContractError::new(
                    "failed storage lifetime v6 job requires sanitized failureKind",
                ));
            }
            require_timestamp(
                job.completed_at.as_deref().unwrap_or_default(),
                "job.completedAt",
            )?;
        }
        StorageLifetimeJobStateV2::Pending | StorageLifetimeJobStateV2::Attempting => {
            if job.failure_kind.is_some() || job.completed_at.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "pending or attempting storage lifetime v6 job cannot carry terminal evidence",
                ));
            }
        }
    }
    Ok(job)
}

pub fn validate_storage_lifetime_materialization_pair_v4(
    value: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV4, StorageLifetimeContractError> {
    let raw: StorageLifetimeMaterializationSnapshotV4 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let job = raw
        .job
        .as_ref()
        .map(|job| {
            serde_json::to_value(job)
                .map_err(|error| StorageLifetimeContractError::new(error.to_string()))
                .and_then(|value| validate_storage_lifetime_materialization_job_v4(&value))
        })
        .transpose()?;
    if raw.materialization_state != StorageMaterializationStateV1::Pending
        && !(raw.materialization_state == StorageMaterializationStateV1::Ready && job.is_none())
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 meta/job pairing is fail-closed",
        ));
    }
    Ok(StorageLifetimeMaterializationSnapshotV4 {
        materialization_state: raw.materialization_state,
        job,
    })
}

pub fn validate_storage_lifetime_reopen_pair_v4(
    value: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV4, StorageLifetimeContractError> {
    let snapshot = validate_storage_lifetime_materialization_pair_v4(value)?;
    if snapshot.job.as_ref().is_some_and(|job| {
        matches!(
            job.state,
            StorageLifetimeJobStateV2::Attempting | StorageLifetimeJobStateV2::Failed
        )
    }) {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 reopen refuses attempting or failed job",
        ));
    }
    Ok(snapshot)
}

pub fn validate_storage_lifetime_lifecycle_transition_v4(
    operation: StorageLifetimeLifecycleOperationV4,
    previous: &Value,
    next: &Value,
) -> Result<StorageLifetimeMaterializationSnapshotV4, StorageLifetimeContractError> {
    let previous = validate_storage_lifetime_materialization_pair_v4(previous)?;
    let next = validate_storage_lifetime_materialization_pair_v4(next)?;
    let previous_job = previous.job.as_ref();
    let next_job = next.job.as_ref();
    match operation {
        StorageLifetimeLifecycleOperationV4::Create => {
            if previous.materialization_state != StorageMaterializationStateV1::Pending
                || previous_job.is_some()
                || next.materialization_state != StorageMaterializationStateV1::Pending
                || next_job.map(|job| job.state) != Some(StorageLifetimeJobStateV2::Pending)
            {
                return lifecycle_error(
                    "storage lifetime v6 create requires pending absent to pending job",
                );
            }
            validate_storage_lifetime_cursor_transition_v4(
                None,
                &serde_json::to_value(job_cursor_v4(next_job.expect("checked")))
                    .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
            )?;
        }
        StorageLifetimeLifecycleOperationV4::Append => {
            let invalidate_ready = previous.materialization_state
                == StorageMaterializationStateV1::Ready
                && previous_job.is_none()
                && next.materialization_state == StorageMaterializationStateV1::Pending
                && next_job.is_none();
            if !invalidate_ready && previous != next {
                return lifecycle_error(
                    "storage lifetime v6 append must atomically index source and invalidate ready or preserve pending exactly",
                );
            }
        }
        StorageLifetimeLifecycleOperationV4::FailureRecordUnavailable => {
            if previous_job.map(|job| job.state) != Some(StorageLifetimeJobStateV2::Attempting)
                || previous != next
            {
                return lifecycle_error(
                    "failure record unavailable must leave the durable attempting sentinel unchanged",
                );
            }
        }
        StorageLifetimeLifecycleOperationV4::FinalPublish => {
            if previous_job.map(|job| (job.state, job.phase, job.scan_exhausted))
                != Some((
                    StorageLifetimeJobStateV2::Pending,
                    StorageLifetimePhaseV4::VerifyBaselines,
                    true,
                ))
                || next.materialization_state != StorageMaterializationStateV1::Ready
                || next_job.is_some()
            {
                return lifecycle_error(
                    "storage lifetime v6 final publication requires exhausted verifyBaselines",
                );
            }
        }
        operation => {
            let previous_job = previous_job.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "storage lifetime v6 transition requires a previous job",
                )
            })?;
            let next_job = next_job.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "storage lifetime v6 transition requires a next job",
                )
            })?;
            require_same_job_identity_v4(previous_job, next_job)?;
            match operation {
                StorageLifetimeLifecycleOperationV4::Arm => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.state != StorageLifetimeJobStateV2::Attempting
                        || previous_job.scan_exhausted
                        || next_job.scan_exhausted
                        || job_cursor_v4(previous_job) != job_cursor_v4(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v6 arm must preserve an unexhausted cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV4::BatchSuccess => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                    {
                        return lifecycle_error(
                            "storage lifetime v6 batch success must commit attempting to pending",
                        );
                    }
                    validate_storage_lifetime_cursor_transition_v4(
                        Some(&serde_json::to_value(job_cursor_v4(previous_job)).map_err(
                            |error| StorageLifetimeContractError::new(error.to_string()),
                        )?),
                        &serde_json::to_value(job_cursor_v4(next_job)).map_err(|error| {
                            StorageLifetimeContractError::new(error.to_string())
                        })?,
                    )?;
                }
                StorageLifetimeLifecycleOperationV4::PhaseAdvance => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                    {
                        return lifecycle_error(
                            "storage lifetime v6 phase advance requires pending jobs",
                        );
                    }
                    validate_storage_lifetime_cursor_transition_v4(
                        Some(&serde_json::to_value(job_cursor_v4(previous_job)).map_err(
                            |error| StorageLifetimeContractError::new(error.to_string()),
                        )?),
                        &serde_json::to_value(job_cursor_v4(next_job)).map_err(|error| {
                            StorageLifetimeContractError::new(error.to_string())
                        })?,
                    )?;
                }
                StorageLifetimeLifecycleOperationV4::Cancel => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                        || job_cursor_v4(previous_job) != job_cursor_v4(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v6 cancellation must restore the same cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV4::Fail => {
                    if previous_job.state != StorageLifetimeJobStateV2::Attempting
                        || next_job.state != StorageLifetimeJobStateV2::Failed
                        || job_cursor_v4(previous_job) != job_cursor_v4(next_job)
                    {
                        return lifecycle_error(
                            "storage lifetime v6 terminal failure must preserve the cursor",
                        );
                    }
                }
                StorageLifetimeLifecycleOperationV4::CatchUpReset => {
                    if previous_job.state != StorageLifetimeJobStateV2::Pending
                        || !is_verification_phase_v4(previous_job.phase)
                        || next_job.state != StorageLifetimeJobStateV2::Pending
                        || next_job.cutoff_event_rowid <= previous_job.cutoff_event_rowid
                        || next_job.phase != StorageLifetimePhaseV4::IndexSource
                        || next_job.after_scan_rowid != 0
                        || next_job.scan_exhausted
                    {
                        return lifecycle_error(
                            "storage lifetime v6 catch-up reset requires verification drift and a larger cutoff",
                        );
                    }
                }
                _ => return lifecycle_error("storage lifetime v6 lifecycle operation is invalid"),
            }
        }
    }
    Ok(next)
}

fn validate_storage_lifetime_cursor_fields_v4(
    cursor: &StorageLifetimeCursorV4,
) -> Result<(), StorageLifetimeContractError> {
    if cursor.schema_version != STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4 {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 cursor schemaVersion must be 4",
        ));
    }
    require_js_safe_unsigned(cursor.cutoff_event_rowid, "cursor.cutoffEventRowid")?;
    require_js_safe_unsigned(cursor.after_scan_rowid, "cursor.afterScanRowid")?;
    if is_event_rowid_phase_v4(cursor.phase) && cursor.after_scan_rowid > cursor.cutoff_event_rowid
    {
        return Err(StorageLifetimeContractError::new(
            "event-derived storage lifetime v6 cursor cannot exceed cutoffEventRowid",
        ));
    }
    Ok(())
}

fn is_event_rowid_phase_v4(phase: StorageLifetimePhaseV4) -> bool {
    matches!(
        phase,
        StorageLifetimePhaseV4::IndexSource
            | StorageLifetimePhaseV4::VerifySourceIndex
            | StorageLifetimePhaseV4::Receipts
            | StorageLifetimePhaseV4::Messages
            | StorageLifetimePhaseV4::Workspaces
            | StorageLifetimePhaseV4::Baselines
            | StorageLifetimePhaseV4::VerifySource
    )
}

fn is_verification_phase_v4(phase: StorageLifetimePhaseV4) -> bool {
    !matches!(
        phase,
        StorageLifetimePhaseV4::IndexSource
            | StorageLifetimePhaseV4::Receipts
            | StorageLifetimePhaseV4::Messages
            | StorageLifetimePhaseV4::Workspaces
            | StorageLifetimePhaseV4::Baselines
    )
}

fn next_phase_v4(phase: StorageLifetimePhaseV4) -> Option<StorageLifetimePhaseV4> {
    let index = [
        StorageLifetimePhaseV4::IndexSource,
        StorageLifetimePhaseV4::VerifySourceIndex,
        StorageLifetimePhaseV4::Receipts,
        StorageLifetimePhaseV4::Messages,
        StorageLifetimePhaseV4::Workspaces,
        StorageLifetimePhaseV4::Baselines,
        StorageLifetimePhaseV4::VerifySource,
        StorageLifetimePhaseV4::VerifyProjects,
        StorageLifetimePhaseV4::VerifyChats,
        StorageLifetimePhaseV4::VerifyRuns,
        StorageLifetimePhaseV4::VerifyLocks,
        StorageLifetimePhaseV4::VerifyAppInstances,
        StorageLifetimePhaseV4::VerifyReceipts,
        StorageLifetimePhaseV4::VerifyMessages,
        StorageLifetimePhaseV4::VerifyWorkspaces,
        StorageLifetimePhaseV4::VerifyBaselines,
    ]
    .iter()
    .position(|candidate| *candidate == phase)?;
    [
        StorageLifetimePhaseV4::IndexSource,
        StorageLifetimePhaseV4::VerifySourceIndex,
        StorageLifetimePhaseV4::Receipts,
        StorageLifetimePhaseV4::Messages,
        StorageLifetimePhaseV4::Workspaces,
        StorageLifetimePhaseV4::Baselines,
        StorageLifetimePhaseV4::VerifySource,
        StorageLifetimePhaseV4::VerifyProjects,
        StorageLifetimePhaseV4::VerifyChats,
        StorageLifetimePhaseV4::VerifyRuns,
        StorageLifetimePhaseV4::VerifyLocks,
        StorageLifetimePhaseV4::VerifyAppInstances,
        StorageLifetimePhaseV4::VerifyReceipts,
        StorageLifetimePhaseV4::VerifyMessages,
        StorageLifetimePhaseV4::VerifyWorkspaces,
        StorageLifetimePhaseV4::VerifyBaselines,
    ]
    .get(index + 1)
    .copied()
}

fn job_cursor_v4(job: &StorageLifetimeMaterializationJobV4) -> StorageLifetimeCursorV4 {
    StorageLifetimeCursorV4 {
        schema_version: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4,
        cutoff_event_rowid: job.cutoff_event_rowid,
        phase: job.phase,
        after_scan_rowid: job.after_scan_rowid,
        scan_exhausted: job.scan_exhausted,
    }
}

fn require_same_job_identity_v4(
    previous: &StorageLifetimeMaterializationJobV4,
    next: &StorageLifetimeMaterializationJobV4,
) -> Result<(), StorageLifetimeContractError> {
    if previous.singleton != next.singleton
        || previous.job_id != next.job_id
        || previous.created_at != next.created_at
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v6 transition cannot replace job identity",
        ));
    }
    Ok(())
}

fn validate_storage_lifetime_cursor_fields_v3(
    cursor: &StorageLifetimeCursorV3,
) -> Result<(), StorageLifetimeContractError> {
    if cursor.schema_version != STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3 {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 cursor schemaVersion must be 3",
        ));
    }
    require_js_safe_unsigned(cursor.cutoff_event_rowid, "cursor.cutoffEventRowid")?;
    require_js_safe_unsigned(cursor.after_scan_rowid, "cursor.afterScanRowid")?;
    if is_event_rowid_phase_v3(cursor.phase) && cursor.after_scan_rowid > cursor.cutoff_event_rowid
    {
        return Err(StorageLifetimeContractError::new(
            "event-derived storage lifetime v5 cursor cannot exceed cutoffEventRowid",
        ));
    }
    Ok(())
}

fn is_event_rowid_phase_v3(phase: StorageLifetimePhaseV3) -> bool {
    matches!(
        phase,
        StorageLifetimePhaseV3::Receipts
            | StorageLifetimePhaseV3::Messages
            | StorageLifetimePhaseV3::Workspaces
            | StorageLifetimePhaseV3::Baselines
            | StorageLifetimePhaseV3::VerifySource
    )
}

fn is_verification_phase_v3(phase: StorageLifetimePhaseV3) -> bool {
    !matches!(
        phase,
        StorageLifetimePhaseV3::Receipts
            | StorageLifetimePhaseV3::Messages
            | StorageLifetimePhaseV3::Workspaces
            | StorageLifetimePhaseV3::Baselines
    )
}

fn next_phase_v3(phase: StorageLifetimePhaseV3) -> Option<StorageLifetimePhaseV3> {
    match phase {
        StorageLifetimePhaseV3::Receipts => Some(StorageLifetimePhaseV3::Messages),
        StorageLifetimePhaseV3::Messages => Some(StorageLifetimePhaseV3::Workspaces),
        StorageLifetimePhaseV3::Workspaces => Some(StorageLifetimePhaseV3::Baselines),
        StorageLifetimePhaseV3::Baselines => Some(StorageLifetimePhaseV3::VerifySource),
        StorageLifetimePhaseV3::VerifySource => Some(StorageLifetimePhaseV3::VerifyProjects),
        StorageLifetimePhaseV3::VerifyProjects => Some(StorageLifetimePhaseV3::VerifyChats),
        StorageLifetimePhaseV3::VerifyChats => Some(StorageLifetimePhaseV3::VerifyRuns),
        StorageLifetimePhaseV3::VerifyRuns => Some(StorageLifetimePhaseV3::VerifyLocks),
        StorageLifetimePhaseV3::VerifyLocks => Some(StorageLifetimePhaseV3::VerifyAppInstances),
        StorageLifetimePhaseV3::VerifyAppInstances => Some(StorageLifetimePhaseV3::VerifyReceipts),
        StorageLifetimePhaseV3::VerifyReceipts => Some(StorageLifetimePhaseV3::VerifyMessages),
        StorageLifetimePhaseV3::VerifyMessages => Some(StorageLifetimePhaseV3::VerifyWorkspaces),
        StorageLifetimePhaseV3::VerifyWorkspaces => Some(StorageLifetimePhaseV3::VerifyBaselines),
        StorageLifetimePhaseV3::VerifyBaselines => None,
    }
}

fn job_cursor_v3(job: &StorageLifetimeMaterializationJobV3) -> StorageLifetimeCursorV3 {
    StorageLifetimeCursorV3 {
        schema_version: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3,
        cutoff_event_rowid: job.cutoff_event_rowid,
        phase: job.phase,
        after_scan_rowid: job.after_scan_rowid,
        scan_exhausted: job.scan_exhausted,
    }
}

fn require_same_job_identity_v3(
    previous: &StorageLifetimeMaterializationJobV3,
    next: &StorageLifetimeMaterializationJobV3,
) -> Result<(), StorageLifetimeContractError> {
    if previous.singleton != next.singleton
        || previous.job_id != next.job_id
        || previous.created_at != next.created_at
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v5 transition cannot replace job identity",
        ));
    }
    Ok(())
}

fn validate_storage_lifetime_cursor_fields_v2(
    cursor: &StorageLifetimeCursorV2,
) -> Result<(), StorageLifetimeContractError> {
    if cursor.schema_version != STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2 {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 cursor schemaVersion must be 2",
        ));
    }
    require_js_safe_unsigned(cursor.cutoff_event_rowid, "cursor.cutoffEventRowid")?;
    require_js_safe_unsigned(cursor.after_scan_rowid, "cursor.afterScanRowid")?;
    if is_event_rowid_phase_v2(cursor.phase) && cursor.after_scan_rowid > cursor.cutoff_event_rowid
    {
        return Err(StorageLifetimeContractError::new(
            "event-derived storage lifetime cursor cannot exceed cutoffEventRowid",
        ));
    }
    Ok(())
}

fn is_event_rowid_phase_v2(phase: StorageLifetimePhaseV2) -> bool {
    matches!(
        phase,
        StorageLifetimePhaseV2::Receipts
            | StorageLifetimePhaseV2::Messages
            | StorageLifetimePhaseV2::Workspaces
            | StorageLifetimePhaseV2::Baselines
            | StorageLifetimePhaseV2::VerifySource
    )
}

fn next_phase_v2(phase: StorageLifetimePhaseV2) -> Option<StorageLifetimePhaseV2> {
    match phase {
        StorageLifetimePhaseV2::Receipts => Some(StorageLifetimePhaseV2::Messages),
        StorageLifetimePhaseV2::Messages => Some(StorageLifetimePhaseV2::Workspaces),
        StorageLifetimePhaseV2::Workspaces => Some(StorageLifetimePhaseV2::Baselines),
        StorageLifetimePhaseV2::Baselines => Some(StorageLifetimePhaseV2::VerifySource),
        StorageLifetimePhaseV2::VerifySource => Some(StorageLifetimePhaseV2::VerifyReceipts),
        StorageLifetimePhaseV2::VerifyReceipts => Some(StorageLifetimePhaseV2::VerifyMessages),
        StorageLifetimePhaseV2::VerifyMessages => Some(StorageLifetimePhaseV2::VerifyWorkspaces),
        StorageLifetimePhaseV2::VerifyWorkspaces => Some(StorageLifetimePhaseV2::VerifyBaselines),
        StorageLifetimePhaseV2::VerifyBaselines => None,
    }
}

fn job_cursor_v2(job: &StorageLifetimeMaterializationJobV2) -> StorageLifetimeCursorV2 {
    StorageLifetimeCursorV2 {
        schema_version: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2,
        cutoff_event_rowid: job.cutoff_event_rowid,
        phase: job.phase,
        after_scan_rowid: job.after_scan_rowid,
        scan_exhausted: job.scan_exhausted,
    }
}

fn require_same_job_identity_v2(
    previous: &StorageLifetimeMaterializationJobV2,
    next: &StorageLifetimeMaterializationJobV2,
) -> Result<(), StorageLifetimeContractError> {
    if previous.singleton != next.singleton
        || previous.job_id != next.job_id
        || previous.created_at != next.created_at
    {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v4 transition cannot replace job identity",
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
    Ok(())
}

fn next_phase(phase: StorageLifetimePhaseV1) -> Option<StorageLifetimePhaseV1> {
    match phase {
        StorageLifetimePhaseV1::Receipts => Some(StorageLifetimePhaseV1::Messages),
        StorageLifetimePhaseV1::Messages => Some(StorageLifetimePhaseV1::Workspaces),
        StorageLifetimePhaseV1::Workspaces => Some(StorageLifetimePhaseV1::Baselines),
        StorageLifetimePhaseV1::Baselines => Some(StorageLifetimePhaseV1::Verify),
        StorageLifetimePhaseV1::Verify => None,
    }
}

fn job_cursor(job: &StorageLifetimeMaterializationJobV1) -> StorageLifetimeCursorV1 {
    StorageLifetimeCursorV1 {
        schema_version: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1,
        cutoff_event_rowid: job.cutoff_event_rowid,
        phase: job.phase,
        after_event_rowid: job.after_event_rowid,
    }
}

fn same_pending_jobs<'a>(
    previous: &'a StorageLifetimeMaterializationSnapshotV1,
    next: &'a StorageLifetimeMaterializationSnapshotV1,
    operation: &str,
) -> Result<
    (
        &'a StorageLifetimeMaterializationJobV1,
        &'a StorageLifetimeMaterializationJobV1,
    ),
    StorageLifetimeContractError,
> {
    let previous_job = previous.job.as_ref();
    let next_job = next.job.as_ref();
    if previous.materialization_state != StorageMaterializationStateV1::Pending
        || next.materialization_state != StorageMaterializationStateV1::Pending
        || previous_job.map(|job| job.state) != Some(StorageLifetimeJobStateV1::Pending)
        || next_job.map(|job| job.state) != Some(StorageLifetimeJobStateV1::Pending)
    {
        return lifecycle_error(format!(
            "storage lifetime {operation} requires pending meta and pending singleton"
        ));
    }
    Ok((previous_job.expect("checked"), next_job.expect("checked")))
}

fn require_same_job_identity(
    previous: &StorageLifetimeMaterializationJobV1,
    next: &StorageLifetimeMaterializationJobV1,
) -> Result<(), StorageLifetimeContractError> {
    if previous.singleton != next.singleton
        || previous.job_id != next.job_id
        || previous.created_at != next.created_at
    {
        return lifecycle_error("storage lifetime transition must preserve singleton job identity");
    }
    Ok(())
}

fn lifecycle_error<T>(detail: impl Into<String>) -> Result<T, StorageLifetimeContractError> {
    Err(StorageLifetimeContractError::new(detail))
}

fn is_sanitized_failure_kind(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && value.len() <= 64
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
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
