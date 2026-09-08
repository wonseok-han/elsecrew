use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
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
pub const STORAGE_LIFETIME_SCHEMA_VERSION_V7: u32 = 7;
pub const STORAGE_LIFETIME_SCHEMA_VERSION_V8: u32 = 8;
pub const STORAGE_LIFETIME_SCHEMA_VERSION_V9: u32 = 9;
pub const STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V1: u32 = 1;
pub const STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V2: u32 = 2;
pub const STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V3: u32 = 3;
pub const STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3: u32 = 3;
pub const STORAGE_COMPACTION_PLAN_SCHEMA_VERSION_V1: u32 = 1;
pub const STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1: u32 = 1_000;
pub const STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1: u32 = 1_001;
pub const STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1: u32 = 100;
pub const STORAGE_COMPACTION_MAX_DELETE_ROWS_V1: usize = 100;
pub const STORAGE_COMPACTION_PHASES_V1: &[&str] = &[
    "freezeMessages",
    "verifyPrefix",
    "authorizeDelete",
    "deletePrefix",
    "verifyEquivalence",
    "finalize",
];
pub const STORAGE_COMPACTION_PHASES_V2: &[&str] = &[
    "freezeOperations",
    "freezeMessages",
    "verifyPrefix",
    "authorizeDelete",
    "deletePrefix",
    "verifyOperations",
    "verifyEquivalence",
    "finalize",
];
pub const STORAGE_COMPACTION_PHASES_V3: &[&str] = &[
    "planEligibility",
    "freezeOperations",
    "freezeMessages",
    "verifyPrefix",
    "authorizeDelete",
    "deletePrefix",
    "verifyOperations",
    "verifyEquivalence",
    "releasePlan",
    "finalize",
];
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
pub const STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1: u64 = 8_388_608;
pub const STORAGE_LOSSLESS_COMPACTION_MIN_ELIGIBLE_LEDGER_EVENTS_V1: u64 = 1_000;
pub const STORAGE_LIFETIME_ACTIVATION_PHASES_V1: &[&str] = &[
    "pending",
    "armRunning",
    "freeze",
    "materialize",
    "verify",
    "delete",
    "finalize",
];
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeV7PreflightPolicyV1 {
    pub schema: String,
    pub admissible_materialization_pairs: Vec<String>,
    pub rejected_materialization_pairs: Vec<String>,
    pub maintenance_requirement: String,
    pub delete_authority_requirement: String,
    pub partial_extra_or_unknown_schema: String,
    pub mapping: Vec<String>,
    pub timing: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageLifetimeV8PreflightPolicyV1 {
    pub schema: String,
    pub required_ledger_state: String,
    pub admissible_materialization_pairs: Vec<String>,
    pub rejected_materialization_pairs: Vec<String>,
    pub maintenance_requirement: String,
    pub empty_tables: Vec<String>,
    pub target_object_requirement: String,
    pub partial_extra_or_tampered_schema: String,
    pub mapping: Vec<String>,
    pub timing: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionPhaseV1 {
    FreezeMessages,
    VerifyPrefix,
    AuthorizeDelete,
    DeletePrefix,
    VerifyEquivalence,
    Finalize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionCursorV1 {
    pub schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub phase: StorageCompactionPhaseV1,
    pub after_event_rowid: u64,
    pub scan_exhausted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionPhaseV2 {
    FreezeOperations,
    FreezeMessages,
    VerifyPrefix,
    AuthorizeDelete,
    DeletePrefix,
    VerifyOperations,
    VerifyEquivalence,
    Finalize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionCursorV2 {
    pub schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub phase: StorageCompactionPhaseV2,
    pub after_event_rowid: u64,
    pub scan_exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionProgressV2 {
    pub rows_in_batch: u32,
    pub canonical_payload_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionPhaseV3 {
    PlanEligibility,
    FreezeOperations,
    FreezeMessages,
    VerifyPrefix,
    AuthorizeDelete,
    DeletePrefix,
    VerifyOperations,
    VerifyEquivalence,
    ReleasePlan,
    Finalize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionCursorV3 {
    pub schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub phase: StorageCompactionPhaseV3,
    pub after_plan_ordinal: u32,
    pub scan_exhausted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionPlanDecisionV1 {
    Delete,
    RetainAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionPlanV1 {
    pub job_id: String,
    pub plan_schema_version: u32,
    pub cutoff_event_rowid: u64,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub retained_boundary_sequence: u64,
    pub delete_through_sequence: u64,
    pub anchor_aggregate_sequence: u64,
    pub expected_rows: u32,
    pub baseline_id: String,
    pub baseline_projection_digest: String,
    pub previous_successful_job_id: Option<String>,
    pub created_at: String,
    pub planned_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionPlanRowV1 {
    pub job_id: String,
    pub ordinal: u32,
    pub event_rowid: u64,
    pub event_id: String,
    pub operation_id: String,
    pub request_id: Option<String>,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_sequence: u64,
    pub event_type: String,
    pub payload_schema_version: u32,
    pub event_timestamp: String,
    pub canonical_payload_json: String,
    pub canonical_payload_digest: String,
    pub introduced_message_id: Option<String>,
    pub introduced_message_digest: Option<String>,
    pub decision: StorageCompactionPlanDecisionV1,
    pub baseline_id: Option<String>,
    pub baseline_projection_digest: Option<String>,
    pub operation_frozen_before_plan: bool,
    pub message_frozen_before_plan: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionPrivateStateV3 {
    pub schema_version: u32,
    pub cursor: StorageCompactionCursorV3,
    pub plan_rows_complete: bool,
    pub operations_verified: bool,
    pub equivalence_verified: bool,
    pub plan_rows_released: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionAdmissionStateV3 {
    pub job_id: String,
    pub state: StorageCompactionJobStateV1,
    pub private_state: StorageCompactionPrivateStateV3,
    pub authority_count: u32,
    pub plan_header_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionLifecycleOperationV3 {
    Arm,
    BatchSuccess,
    CancelBeforeArm,
    CancelAfterArm,
    CommitUncertain,
    Complete,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionJobV3 {
    pub job_id: String,
    pub state: StorageCompactionJobStateV1,
    pub private_state: StorageCompactionPrivateStateV3,
    pub authority_count: u32,
    pub failure_kind: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionLifecycleOperationV2 {
    Arm,
    BatchSuccess,
    CancelBeforeArm,
    CancelAfterArm,
    CommitUncertain,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionJobV2 {
    pub job_id: String,
    pub state: StorageCompactionJobStateV1,
    pub cursor: StorageCompactionCursorV2,
    pub authority_count: u32,
    pub operations_verified: bool,
    pub equivalence_verified: bool,
    pub failure_kind: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg(test)]
struct StorageCompactionEquivalenceEvidenceV2 {
    pub rows_in_batch: u32,
    pub operations_verified: bool,
    pub baseline_tail_before: bool,
    pub anchor_tail_after: bool,
    pub preserved_artifacts_byte_exact: bool,
    pub receipt_replay_equivalent: bool,
    pub namespace_decisions_equivalent: bool,
    pub frozen_message_ids_reserved: bool,
    pub authority_empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FrozenOperationSourceV1 {
    pub operation_id: String,
    pub source_event_rowid: u64,
    pub source_event_id: String,
    pub request_id: Option<String>,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_sequence: u64,
    pub event_type: String,
    pub payload_schema_version: u32,
    pub canonical_payload_json: String,
    pub canonical_payload_digest: String,
    pub event_timestamp: String,
    pub canonical_record_json: String,
    pub record_digest: String,
    pub frozen_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageRuntimeSourceAuthorityV1 {
    pub aggregate_type: String,
    pub event_present: bool,
    pub live_source_index_present: bool,
    pub frozen_operation_present: bool,
    pub receipt_present: bool,
    pub live_source_matches: bool,
    pub frozen_operation_matches: bool,
    pub receipt_matches: bool,
    pub chat_predecessor_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionPrefixV1 {
    pub prefix_id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub deleted_through_sequence: u64,
    pub anchor_event_rowid: u64,
    pub anchor_event_id: String,
    pub anchor_operation_id: String,
    pub anchor_aggregate_sequence: u64,
    pub baseline_id: String,
    pub baseline_projection_digest: String,
    pub created_at: String,
    pub verified_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FrozenMessageSourceV1 {
    pub message_id: String,
    pub source_event_rowid: u64,
    pub source_event_id: String,
    pub source_operation_id: String,
    pub chat_id: String,
    pub source_aggregate_sequence: u64,
    pub durable_sequence: u64,
    pub canonical_message_json: String,
    pub message_digest: String,
    pub frozen_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionDeleteAuthorityV1 {
    pub job_id: String,
    pub ordinal: u32,
    pub prefix_id: String,
    pub prefix_deleted_through_sequence: u64,
    pub event_rowid: u64,
    pub event_id: String,
    pub operation_id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_sequence: u64,
    pub introduced_message_id: Option<String>,
    pub frozen_message_id: Option<String>,
    pub authorized_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionJobStateV1 {
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionJobV1 {
    pub job_id: String,
    pub state: StorageCompactionJobStateV1,
    pub cursor: StorageCompactionCursorV1,
    pub authority_count: u32,
    pub equivalence_verified: bool,
    pub failure_kind: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageCompactionLifecycleOperationV1 {
    Arm,
    BatchSuccess,
    CancelBeforeArm,
    CancelAfterArm,
    Fail,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageCompactionEquivalenceV1 {
    pub baseline_tail_before: bool,
    pub anchor_tail_after: bool,
    pub preserved_artifacts_byte_exact: bool,
    pub receipt_replay_equivalent: bool,
    pub namespace_decisions_equivalent: bool,
    pub frozen_message_ids_reserved: bool,
    pub authority_empty: bool,
    pub rows_in_batch: u32,
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

pub fn validate_storage_lifetime_v7_preflight_policy_v1(
    value: &Value,
) -> Result<StorageLifetimeV7PreflightPolicyV1, StorageLifetimeContractError> {
    let policy: StorageLifetimeV7PreflightPolicyV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let exact = policy.schema == "exactStorageV6"
        && strings_equal(
            &policy.admissible_materialization_pairs,
            &[
                "ready+absent",
                "pending+absent",
                "pending+pending",
                "pending+failed",
            ],
        )
        && strings_equal(
            &policy.rejected_materialization_pairs,
            &["pending+attempting"],
        )
        && policy.maintenance_requirement == "noPendingOrRunningLosslessCompaction"
        && policy.delete_authority_requirement == "storageV7AuthorityObjectsAbsent"
        && policy.partial_extra_or_unknown_schema == "failClosedBeforeMutation"
        && strings_equal(
            &policy.mapping,
            &[
                "materializationStatePreserved",
                "materializationJobPreservedByteExact",
                "completedAndFailedMaintenanceHistoryPreserved",
                "existingRowsNotBackfilled",
                "existingLedgerAndDerivedRowsNotDeleted",
            ],
        )
        && policy.timing == "beforeAnyDdlOrMutation";
    if !exact {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v7 preflight policy is not fail-closed",
        ));
    }
    Ok(policy)
}

pub fn validate_storage_lifetime_v8_preflight_policy_v1(
    value: &Value,
) -> Result<StorageLifetimeV8PreflightPolicyV1, StorageLifetimeContractError> {
    let policy: StorageLifetimeV8PreflightPolicyV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let exact = policy.schema == "exactStorageV7"
        && policy.required_ledger_state == "uncompactedOnly"
        && strings_equal(
            &policy.admissible_materialization_pairs,
            &[
                "ready+absent",
                "pending+absent",
                "pending+pending",
                "pending+failed",
            ],
        )
        && strings_equal(
            &policy.rejected_materialization_pairs,
            &["pending+attempting"],
        )
        && policy.maintenance_requirement == "noPendingOrRunningLosslessCompaction"
        && strings_equal(
            &policy.empty_tables,
            &[
                "ledger_compaction_prefixes",
                "frozen_message_sources",
                "ledger_compaction_delete_authority",
            ],
        )
        && policy.target_object_requirement == "storageV8ObjectsAbsent"
        && policy.partial_extra_or_tampered_schema == "failClosedBeforeMutation"
        && strings_equal(
            &policy.mapping,
            &[
                "materializationPairPreservedByteExact",
                "terminalMaintenanceHistoryPreservedByteExact",
                "allExistingRowsAndTriggersPreservedExceptNamedVersionedGuards",
                "existingHistoryNotBackfilled",
            ],
        )
        && policy.timing == "beforeAnyDdlOrMutation";
    if !exact {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime v8 preflight policy is not fail-closed",
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
                "indexedNonNullSnapshotProvenance":
                    "exactEventsRowidLookupMustBindSnapshotChatIdAndEventSequence",
                "missingDuringMigrationPendingAppend":
                    "rejectOrSuspendUntilExactPredecessorSourceIndexPresent",
                "rawPredecessorEventFallback": "forbidden",
                "forbidden": ["scan", "sort", "latestLookup", "rawEventFallback"],
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

pub fn validate_storage_compaction_cursor_v1(
    value: &Value,
) -> Result<StorageCompactionCursorV1, StorageLifetimeContractError> {
    let cursor: StorageCompactionCursorV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if cursor.schema_version != STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V1 {
        return Err(StorageLifetimeContractError::new(
            "storage compaction cursor schemaVersion must be 1",
        ));
    }
    require_js_safe_unsigned(cursor.cutoff_event_rowid, "compaction.cutoffEventRowid")?;
    require_js_safe_unsigned(cursor.after_event_rowid, "compaction.afterEventRowid")?;
    if cursor.after_event_rowid > cursor.cutoff_event_rowid {
        return Err(StorageLifetimeContractError::new(
            "storage compaction cursor cannot exceed its frozen cutoff",
        ));
    }
    Ok(cursor)
}

pub fn validate_storage_lossless_compaction_policy_v1(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let expected = serde_json::json!({
        "schemaVersion": 1,
        "casesFile": "cases.json",
        "activation": {
            "productExecutionEnabled": false,
            "automaticSchedulingEnabled": false,
            "actualDeleteEnabledBeforeC1Consumer": false,
            "wireStatusOrControlEnabled": false,
        },
        "guardVersioning": {
            "schemaVersion": 7,
            "migrationReplacement": "oneTimeExactV6ToV7Transaction",
            "runtimeDropRecreate": "forbidden",
            "deleteBatchDdl": "forbidden",
            "deleteBatchMechanism": "durableRunningJobPlusEphemeralExactAuthorityRows",
        },
        "prefix": {
            "table": "ledger_compaction_prefixes",
            "identity": "appendOnlyPrefixIdAndAggregateDeletedThroughUnique",
            "range": "aggregateSequenceOneThroughDeletedThroughContinuousOnly",
            "anchor": "exactSameAggregateAtDeletedThroughPlusOneRemainsInEventsAndSourceIndex",
            "baseline": "immutableSameAggregateAtDeletedThroughWithExactDigest",
            "progression": "newPrefixStrictlyExtendsLatestVerifiedPrefix",
            "appInstance": "forbidden",
            "proof": "boundedPreDeleteAndPostDeleteEquivalenceBeforePrefixInsert",
        },
        "frozenMessageSource": {
            "table": "frozen_message_sources",
            "whenRequired": "beforeDeletingAnyEventThatIntroducedMessage",
            "fields": [
                "messageId", "sourceEventRowid", "sourceEventId", "sourceOperationId",
                "chatId", "sourceAggregateSequence", "durableSequence",
                "canonicalMessageJson", "messageDigest", "frozenAt",
            ],
            "binding": "exactEventSourceIndexIntroductionAndExactChatMessage",
            "appendOnly": true,
            "messageIdReuse": "forbiddenForeverWhileFrozenRecordExists",
        },
        "deleteAuthority": {
            "table": "ledger_compaction_delete_authority",
            "jobState": "running",
            "maxExactTuples": STORAGE_COMPACTION_MAX_DELETE_ROWS_V1,
            "globalAuthorityOwner": "oneRunningJobAtATime",
            "identity": "eventRowidEventIdOperationIdAndAggregateTupleExact",
            "prefixBound": "sameAggregateSequenceAtOrBelowDeletedThrough",
            "introducedMessage": "requiresExactFrozenMessageSource",
            "appInstance": "forbidden",
            "sourceIndexDeleteBeforeEventDelete": true,
            "eventDeleteConsumesAuthority": true,
            "mustBeEmptyBeforeCommit": true,
            "mustBeEmptyOnOpen": true,
            "updates": "forbidden",
        },
        "cursor": {
            "schemaVersion": STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V1,
            "phases": STORAGE_COMPACTION_PHASES_V1,
            "columns": ["cutoffEventRowid", "phase", "afterEventRowid", "scanExhausted"],
            "keyset": "eventRowidGreaterThanAfterAndAtOrBelowCutoffOrderByEventRowidLimit100",
            "phaseTransition": "exhaustedCurrentThenNextAtZeroUnexhausted",
            "callerConfigurable": false,
            "maxRowsPerBatch": STORAGE_COMPACTION_MAX_DELETE_ROWS_V1,
            "unboundedScanOrTransaction": "forbidden",
        },
        "lifecycle": {
            "durableTable": "maintenance_jobs",
            "jobKind": "losslessCompaction",
            "runningUniqueness": "schemaPartialUniqueBeforeAuthority",
            "c1Consumer": "requiresExactSchemaGuardNeverApplicationOnlyCheck",
            "arm": "pendingToRunningSentinelCommittedBeforeDeleteBatch",
            "batchSuccess": "runningToPendingWithAdvancedCursorSameDeleteTransaction",
            "cancelBeforeArm": "pendingUnchangedNoAuthority",
            "cancelAfterArm": "rollbackDeleteBatchLeavesRunningSentinelFailClosed",
            "commitUncertain": "reopenPendingAdvancedMeansCommittedRunningMeansManualRecoveryRequired",
            "terminalSuccess": "authorityEmptyAndEquivalenceVerifiedThenSucceeded",
            "terminalFailure": "rollbackBatchThenRunningToFailedWithSanitizedEvidence",
            "automaticRetryRunning": false,
        },
        "deletion": {
            "aggregateRule": "retainOneExactAnchorPerCompactedAggregate",
            "eligible": "onlyContiguousVerifiedPrefixBeforeAnchor",
            "appInstance": "never",
            "unchangedTables": [
                "operation_receipts", "chat_messages", "prepared_workspaces", "ledger_baselines",
                "projects", "chats", "runs", "locks", "app_instances",
            ],
            "snapshotFiles": "neverMutated",
            "checkpointOrVacuumRequiredForSuccess": false,
        },
        "equivalence": {
            "maxRowsPerBatch": STORAGE_COMPACTION_MAX_DELETE_ROWS_V1,
            "before": [
                "baselinePlusTailReplaysCurrentProjection",
                "operationReceiptsClassifyEveryDeletedOperation",
                "introducedMessagesHaveFrozenSources",
                "namespaceDecisionsMatchLegacyActualRecovery",
            ],
            "after": [
                "anchorPlusTailReplaysCurrentProjection",
                "preservedArtifactsByteExact",
                "deletedOperationsRemainDuplicateOrConflictByReceipt",
                "deletedIntroducedMessageIdsRemainReserved",
            ],
            "failure": "rollbackWholeAtMost100RowDeleteBatch",
            "finalization": "exactEmptyAuthorityQueryThenSucceededSameTransaction",
        },
        "replay": {
            "deletedOperationAuthority": "operationReceiptsCanonicalPayloadAndIdentity",
            "duplicate": "exactCanonicalReceiptMatchIsDuplicateNoOp",
            "conflict": "sameOperationIdAnyImmutableDifferenceIsConflict",
            "namespaces": ["legacy", "actualAgent", "recovery"],
            "namespaceDecisionEquality": true,
            "deletedMessageIntroduction": "frozenMessageSourceReservesMessageId",
        },
        "queryPlanAcceptance": {
            "mode": "exactExplainQueryPlanDetail",
            "prefixLatest": "SEARCH ledger_compaction_prefixes USING INDEX ledger_compaction_prefixes_latest (aggregate_type=? AND aggregate_id=?)",
            "deleteBatch": "SEARCH event_source_index USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)",
            "authorityEmpty": "SCAN ledger_compaction_delete_authority USING COVERING INDEX sqlite_autoindex_ledger_compaction_delete_authority_1",
            "frozenMessage": "SEARCH frozen_message_sources USING INDEX sqlite_autoindex_frozen_message_sources_1 (message_id=?)",
            "reject": ["USE TEMP B-TREE", "SCAN events", "SCAN event_source_index"],
        },
    });
    if value != &expected {
        return Err(StorageLifetimeContractError::new(
            "storage v7 lossless compaction policy is not exact",
        ));
    }
    Ok(())
}

pub fn validate_storage_frozen_operation_source_policy_v1(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let expected = serde_json::json!({
        "schemaVersion": 1,
        "casesFile": "cases.json",
        "table": "frozen_operation_sources",
        "identity": "operationIdPrimaryKey",
        "allowedAggregateTypes": ["project", "chat", "run", "lock"],
        "appInstance": "forbidden",
        "requestId": "nullableAndExcludedFromDuplicateConflictAuthority",
        "sourceAuthority": "exactLiveEventAndExactSourceIndex",
        "receiptRole": "immutableCrossCheckNeverSourceReconstruction",
        "consumerIntegration": "requiredBeforeAnyProductActivation",
        "immutableFields": [
            "operationId", "sourceEventRowid", "sourceEventId", "requestId",
            "aggregateType", "aggregateId", "aggregateSequence", "eventType",
            "payloadSchemaVersion", "canonicalPayloadJson", "canonicalPayloadDigest",
            "eventTimestamp", "canonicalRecordJson", "recordDigest", "frozenAt",
        ],
        "payloadDigest": {
            "algorithm": "sha256",
            "format": "lowercaseHex64",
            "sqlAuthority": "exactImmutableReceiptCrossCheck",
            "consumerValidation": "recomputeBeforeInsert",
        },
        "recordDigest": {
            "schemaVersion": 1,
            "algorithm": "sha256",
            "format": "lowercaseHex64",
            "canonicalEnvelope": "canonicalJsonV1AllFieldsExceptRecordDigest",
            "sqlAuthority": "exactCanonicalEnvelopeTextAndDigestShape",
            "consumerValidation": "recomputeBeforeInsertRequired",
            "reopenValidation": "boundedVerifyOperationsBeforeRunnable",
        },
        "appendOnly": true,
        "receiptMutation": "updateAndDeleteForbidden",
        "dependentGuards": [
            "prefixAnchorRequiresFrozenOperation",
            "frozenMessageRequiresSameFrozenOperation",
            "deleteAuthorityRequiresSameFrozenOperation",
        ],
        "queryPlanAcceptance": {
            "mode": "exactExplainQueryPlanDetail",
            "sourceEventRowid": "SEARCH frozen_operation_sources USING COVERING INDEX frozen_operation_sources_source_rowid (source_event_rowid=?)",
            "operationId": "SEARCH frozen_operation_sources USING PRIMARY KEY (operation_id=?)",
            "aggregateTuple": "SEARCH frozen_operation_sources USING COVERING INDEX frozen_operation_sources_aggregate (aggregate_type=? AND aggregate_id=? AND aggregate_sequence=?)",
            "reject": ["SCAN frozen_operation_sources", "USE TEMP B-TREE"],
        },
    });
    if value != &expected {
        return Err(StorageLifetimeContractError::new(
            "storage frozen operation source policy is not exact",
        ));
    }
    Ok(())
}

pub fn validate_storage_runtime_source_authority_policy_v1(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let expected = serde_json::json!({
        "schemaVersion": 1,
        "liveEvent": "exactLiveEventPlusSourceIndexPlusReceipt",
        "deletedEvent": "exactFrozenOperationPlusReceipt",
        "receiptOnly": "failClosed",
        "requestIdComparison": "excluded",
        "deletedIntroducingMessageIdReuse": "rejectedByPreservedV7FrozenMessageGuard",
        "deletedOperationDuplicateConflict": "frozenOperationPlusReceiptExactV1Semantics",
        "chatPredecessor": "frozenOperationCanonicalPayloadNeverReceiptSelfReconstruction",
        "namespaces": ["legacy", "actualAgent", "recovery"],
        "namespaceDecisionEquality": true,
    });
    if value != &expected {
        return Err(StorageLifetimeContractError::new(
            "storage runtime source authority policy is not exact",
        ));
    }
    Ok(())
}

pub fn validate_storage_compaction_policy_v2(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    let expected = serde_json::json!({
        "schemaVersion": 2,
        "activation": {
            "productExecutionEnabled": false,
            "actualDeleteEnabled": false,
            "automaticSchedulingEnabled": false,
            "wireEnabled": false,
        },
        "privateDurableState": {
            "location": "maintenance_jobs.cutoff_json.storageCompactionV2PrivateState",
            "schemaVersion": 1,
            "fields": ["schemaVersion", "cursor", "operationsVerified", "equivalenceVerified"],
            "writer": "sidecarOnlySameTransaction",
            "publicInput": "forbidden",
        },
        "cursor": {
            "schemaVersion": STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V2,
            "phases": STORAGE_COMPACTION_PHASES_V2,
            "keyset": "eventRowidGreaterThanAfterAndAtOrBelowCutoffOrderByEventRowidLimit100",
            "maxRowsPerBatch": STORAGE_COMPACTION_MAX_DELETE_ROWS_V1,
            "maxCanonicalPayloadBytesPerBatch": STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
            "scanExhaustedMutationAuthority": "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
            "scanExhaustedProofBoundary": "implementationOwnedNotRepresentableAsPublicJsonOrWire",
            "durablePrivateResult": "scanExhaustedPersistedInMaintenanceJobCutoffJsonOnlyAfterPrivateTransaction",
            "publicProgressInput": "positiveBoundedRowsAndBytesNoExhaustionAuthority",
            "phaseTransition": "durablePrivateExhaustionThenExactNextAtZeroAndFalse",
            "callerConfigurable": false,
        },
        "lifecycle": {
            "arm": "pendingToRunningSentinelCommittedBeforeBatch",
            "batchSuccess": "runningToPendingWithCursorAndEmptyAuthoritySameTransaction",
            "cancelBeforeArm": "pendingUnchanged",
            "cancelAfterArm": "rollbackLeavesRunningSentinel",
            "commitUncertain": "runningRequiresManualRecoveryPendingAdvancedMeansCommitted",
            "runtimeGuardDdl": "forbidden",
            "terminalSuccessMutationAuthority": "sidecarPrivateBoundedOperationsAndEquivalenceVerificationSameTransaction",
            "terminalSuccessProofBoundary": "implementationOwnedNotRepresentableAsPublicJsonOrWire",
            "durablePrivateResult": "verifiedBooleansAndSucceededPersistedInMaintenanceJobSamePrivateTransaction",
            "publicCompleteTransition": "alwaysRejected",
        },
        "operationVerification": {
            "everyAuthorizedDeleteHasFrozenOperation": true,
            "deletedOperationReplayUsesFrozenOperationAndReceipt": true,
            "receiptOnlyRejected": true,
            "chatPredecessorUsesFrozenCanonicalPayload": true,
            "publicBooleanAuthority": "forbidden",
            "durablePrivateResult": "operationsVerifiedAndEquivalenceVerifiedPersistedOnlyBySidecarSameTransaction",
        },
    });
    if value != &expected {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v2 policy is not exact",
        ));
    }
    Ok(())
}

pub fn storage_lifetime_activation_policy_v1() -> Value {
    serde_json::json!({
        "schemaVersion": 1,
        "generation": "storageLifetimeActivationV1",
        "databaseMigration": "none",
        "publicWire": {
            "g5dLifecycleShape": "unchanged",
            "storageStatusShape": "unchanged",
            "capabilityOrder": "unchanged",
            "readyToFailedTransition": false,
            "physicalCleanupEnabled": false,
        },
        "eligibility": {
            "exactSafePrefixMinLedgerEvents":
                STORAGE_LOSSLESS_COMPACTION_MIN_ELIGIBLE_LEDGER_EVENTS_V1,
            "countBoundary": "sinceRetainedBoundary",
            "belowThresholdAllowed": false,
            "ageTrigger": false,
            "byteTrigger": false,
            "preconditions": [
                "allExistingSafetyExclusionsPass",
                "completeMaterialization",
                "exactBeforeAfterEquivalence",
            ],
        },
        "preservation": {
            "logicalDeleteRows": ["events", "event_source_index"],
            "preservedRecords": [
                "userChats",
                "chatMessages",
                "userEvidence",
                "recoveries",
                "workspaces",
                "snapshots",
                "baselines",
                "projections",
                "operationReceipts",
            ],
            "physicalCleanup": "disabled",
        },
        "restart": {
            "pendingEmptyAuthority":
                "exactVerifyDuringAdmissionThenReadyAndResumeAfterward",
            "runningSentinel": "failAdmissionWithoutMutationManualRecovery",
            "nonEmptyAuthority": "failAdmissionWithoutMutationManualRecovery",
            "ambiguousOrCommitUncertain": "failAdmissionWithoutMutationManualRecovery",
            "automaticResetOrResumeUnsafeState": false,
        },
        "runtime": {
            "owner": "singleSQLiteOwnerInSidecarEventLoop",
            "writerConnections": 1,
            "backgroundWriterConnection": false,
            "backgroundWriterThread": false,
            "ticks": "readyOnly",
            "zeroTickStates": ["verifying", "failed", "quitting"],
            "step": "oneBoundedCommittedStepPerTick",
        },
        "fatalAfterReady": {
            "causes": [
                "semanticCorruption",
                "commitUncertainty",
                "materializerFatal",
                "compactorFatal",
            ],
            "action": "poisonStopMaintenanceAndTerminateSidecar",
            "furtherMutationOrRun": "prohibited",
            "nextProcess": "reenterVerifyingAndRunAdmission",
            "wireTransition": "none",
        },
        "budgets": {
            "maxWorkUnitsPerStep": STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
            "maxCanonicalPrimaryInputBytesPerStep":
                STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
            "maxRetainedAndDependencyInputBytesPerStep":
                STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1,
            "callerConfigurable": false,
        },
        "phases": STORAGE_LIFETIME_ACTIVATION_PHASES_V1,
        "quit": "stopNewTicksBeforeCheckpointAndCleanMarker",
    })
}

pub fn validate_storage_lifetime_activation_policy_v1(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    if value != &storage_lifetime_activation_policy_v1() {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime activation policy is not exact",
        ));
    }
    Ok(())
}

pub fn storage_compaction_resume_witness_policy_v1() -> Value {
    serde_json::json!({
        "schemaVersion": 1,
        "generation": "storageCompactionResumeWitnessV1",
        "storageSchema": {
            "fromVersion": STORAGE_LIFETIME_SCHEMA_VERSION_V8,
            "toVersion": STORAGE_LIFETIME_SCHEMA_VERSION_V9,
            "precondition": "exactV8NoPendingOrRunningLosslessJobAndEmptyDeleteAuthority",
            "mode": "additivePlanWitnessAndVersionedGuardsNoExistingRowDeletion",
            "runtimeDigestAuthority": "rustRecomputesCanonicalPayloadAndMessageDigests",
        },
        "plan": {
            "headerTable": "storage_compaction_plans",
            "rowTable": "storage_compaction_plan_rows",
            "rowTableWithoutRowid": true,
            "planSchemaVersion": STORAGE_COMPACTION_PLAN_SCHEMA_VERSION_V1,
            "aggregateTypes": ["project", "chat", "run", "lock"],
            "deleteRows": STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1,
            "expectedRows": STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1,
            "boundary": "deleteThroughEqualsRetainedPlus1000AndAnchorEqualsDeleteThroughPlusOne",
            "headerMutation": "forbidden",
            "rowMutation": "forbidden",
            "rowIdentity": "jobIdAndOrdinalPrimaryKeyWithUniqueSourceRowidOperationAndAggregateTuple",
            "baseline": "everyDeleteRowMatchesHeaderBaselineIdentityAndDigest",
            "anchor": "ordinal1001IsRetainAnchorAndRemainsLive",
            "message": "nullableIdentityAndDigestAppearTogether",
        },
        "discovery": {
            "targetScan": "volatileBoundedAggregateKeyset",
            "forbidden": ["OFFSET", "fullVec", "tempTable", "unboundedScan"],
            "creationTransaction": "beginImmediateRecheckCutoffMaterializationSafetyBoundariesAndExact1001Rows",
            "planningWrite": "atMost100ConsecutiveOrdinalsAndCursorSameTransaction",
            "largerPrefix": "oneExact1000DeleteJobThenRepeat",
        },
        "privateState": {
            "schemaVersion": STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3,
            "fields": [
                "schemaVersion", "cursor", "planRowsComplete", "operationsVerified",
                "equivalenceVerified", "planRowsReleased",
            ],
            "cursorSchemaVersion": STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V3,
            "phases": STORAGE_COMPACTION_PHASES_V3,
            "cursorColumn": "afterPlanOrdinal",
            "finalizeCursor": "afterPlanOrdinalZeroAndTerminalEvidenceExhausted",
            "resumeVerification": "beforeReleaseBoundedKeysetProvesOrdinalsOneThroughCursorNoGapsExtrasAndRehashesSourceReceiptMessageBaseline",
            "exhausted": "trueOnlyAtExactly1001WitnessedRows",
            "activeV2Pending": "failAdmissionUnsupportedNoMutation",
        },
        "lifecycle": {
            "arm": "pendingToRunningSeparateCommittedSentinel",
            "success": "runningToPendingWithAdvancedCursorAndEmptyAuthorityAtomic",
            "crash": "pendingAdvancedCommittedRunningOrNonemptyAuthorityOrUncertainFailsAdmissionNoMutation",
            "reset": "forbidden",
            "appendAfterCutoff": "ignoredByPlanAndIncludedInRetainedTailEquivalence",
            "currentStateSafetyBeforeDelete": "failureLeavesZeroMutationPending",
        },
        "authority": {
            "source": "onlyDecisionDeletePlanRows",
            "joins": "planHeaderCurrentJobExactPlanRowAndExistingPrefix",
            "singleActiveJob": "schemaPartialUniquePendingOrRunning",
            "deleteBatchRows": STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1,
            "mustBeEmptyAtPending": true,
        },
        "release": {
            "planRows": "boundedAfterPrivateEquivalenceOnly",
            "resumeVerification": "releasedOrdinalPrefixAbsentAndRetainedSuffixExactAgainstTerminalEvidence",
            "header": "retainedAfterSuccess",
            "terminalEvidence": "retainedAfterSuccessOrFailure",
            "beforeEquivalence": "forbidden",
        },
        "multiJob": {
            "previousSuccessfulJob": "optionalExactImmediatePredecessor",
            "verification": "boundedPrimaryKeyLookupNoChainTraversal",
            "progression": "retainedBoundaryEqualsPreviousDeleteThroughAndNewJobDeletesNextExact1000",
            "activationWithoutBoundedTerminalEvidence": "forbidden",
        },
        "budgets": {
            "maxWorkUnitsPerStep": STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
            "maxCanonicalPrimaryInputBytesPerStep":
                STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
            "maxRetainedAndDependencyInputBytesPerStep":
                STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1,
            "callerConfigurable": false,
            "writerConnections": 1,
        },
        "activation": {
            "contractOnly": true,
            "runtimeImplemented": false,
            "productExecutionEnabled": false,
            "automaticSchedulingEnabled": false,
            "publicWireChanged": false,
        },
    })
}

pub fn validate_storage_compaction_resume_witness_policy_v1(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    if value != &storage_compaction_resume_witness_policy_v1() {
        return Err(StorageLifetimeContractError::new(
            "storage compaction resume witness policy is not exact",
        ));
    }
    Ok(())
}

pub fn validate_storage_lifetime_v9_migration_preflight(
    exact_storage_v8: bool,
    active_lossless_jobs: u32,
    delete_authority_rows: u32,
) -> Result<(), StorageLifetimeContractError> {
    if !exact_storage_v8 || active_lossless_jobs != 0 || delete_authority_rows != 0 {
        return Err(StorageLifetimeContractError::new(
            "storage v9 migration requires exact v8 with no active compaction or authority",
        ));
    }
    Ok(())
}

pub fn storage_lifetime_activation_policy_v2() -> Value {
    let mut policy = storage_lifetime_activation_policy_v1();
    let object = policy
        .as_object_mut()
        .expect("activation policy v1 is an object");
    object.insert("schemaVersion".into(), serde_json::json!(2));
    object.insert(
        "generation".into(),
        serde_json::json!("storageLifetimeActivationV2"),
    );
    object.insert(
        "databaseMigration".into(),
        serde_json::json!({
            "fromVersion": STORAGE_LIFETIME_SCHEMA_VERSION_V8,
            "toVersion": STORAGE_LIFETIME_SCHEMA_VERSION_V9,
            "precondition": "noPendingOrRunningLosslessJobAndEmptyDeleteAuthority",
            "effect": "additiveResumeWitnessTablesAndVersionedGuards",
            "existingRowsDeleted": false,
            "activeV2Pending": "failAdmissionUnsupportedNoMutation",
        }),
    );
    object.insert(
        "resumeWitness".into(),
        serde_json::json!({
            "generation": "storageCompactionResumeWitnessV1",
            "privateStateSchemaVersion": STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3,
            "pendingAdmission": "exactWitnessVerificationThenReadyResumeAfterward",
            "runningOrAuthorityOrUncertain": "failAdmissionNoMutation",
            "publicWire": "unchanged",
        }),
    );
    policy
}

pub fn validate_storage_lifetime_activation_policy_v2(
    value: &Value,
) -> Result<(), StorageLifetimeContractError> {
    if value != &storage_lifetime_activation_policy_v2() {
        return Err(StorageLifetimeContractError::new(
            "storage lifetime activation policy v2 is not exact",
        ));
    }
    Ok(())
}

pub fn validate_storage_compaction_plan_v1(
    value: &Value,
) -> Result<StorageCompactionPlanV1, StorageLifetimeContractError> {
    let plan: StorageCompactionPlanV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if plan.plan_schema_version != STORAGE_COMPACTION_PLAN_SCHEMA_VERSION_V1
        || plan.job_id.is_empty()
        || !matches!(
            plan.aggregate_type.as_str(),
            "project" | "chat" | "run" | "lock"
        )
        || plan.aggregate_id.is_empty()
        || plan.baseline_id.is_empty()
        || !is_lowercase_hex_digest(&plan.baseline_projection_digest)
        || plan.expected_rows != STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan header is invalid",
        ));
    }
    for (value, name) in [
        (plan.cutoff_event_rowid, "plan.cutoffEventRowid"),
        (
            plan.retained_boundary_sequence,
            "plan.retainedBoundarySequence",
        ),
        (plan.delete_through_sequence, "plan.deleteThroughSequence"),
        (
            plan.anchor_aggregate_sequence,
            "plan.anchorAggregateSequence",
        ),
    ] {
        require_js_safe_unsigned(value, name)?;
    }
    let expected_delete = plan
        .retained_boundary_sequence
        .checked_add(u64::from(STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1));
    let expected_anchor = expected_delete.and_then(|value| value.checked_add(1));
    if Some(plan.delete_through_sequence) != expected_delete
        || Some(plan.anchor_aggregate_sequence) != expected_anchor
        || plan.anchor_aggregate_sequence > MAX_SEQUENCE
        || plan.cutoff_event_rowid == 0
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan boundary must be exact 1000 delete rows plus one anchor",
        ));
    }
    if plan.previous_successful_job_id.as_deref() == Some(plan.job_id.as_str())
        || plan
            .previous_successful_job_id
            .as_deref()
            .is_some_and(str::is_empty)
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction previous successful job identity is invalid",
        ));
    }
    require_timestamp(&plan.created_at, "plan.createdAt")?;
    require_timestamp(&plan.planned_at, "plan.plannedAt")?;
    if plan.planned_at < plan.created_at {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plannedAt cannot precede createdAt",
        ));
    }
    Ok(plan)
}

pub fn validate_storage_compaction_plan_row_v1(
    plan: &StorageCompactionPlanV1,
    value: &Value,
) -> Result<StorageCompactionPlanRowV1, StorageLifetimeContractError> {
    let row: StorageCompactionPlanRowV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if row.job_id != plan.job_id
        || row.ordinal == 0
        || row.ordinal > STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1
        || row.event_rowid == 0
        || row.event_rowid > plan.cutoff_event_rowid
        || row.event_id.is_empty()
        || row.operation_id.is_empty()
        || row.aggregate_type != plan.aggregate_type
        || row.aggregate_id != plan.aggregate_id
        || row.event_type.is_empty()
        || row.payload_schema_version != 1
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan row identity is invalid",
        ));
    }
    require_js_safe_unsigned(row.event_rowid, "planRow.eventRowid")?;
    require_js_safe_unsigned(row.aggregate_sequence, "planRow.aggregateSequence")?;
    require_timestamp(&row.event_timestamp, "planRow.eventTimestamp")?;
    let digest = sha256_file_version_v1(row.canonical_payload_json.as_bytes());
    if digest.strip_prefix("sha256:") != Some(row.canonical_payload_digest.as_str()) {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan row canonical payload digest must be recomputed",
        ));
    }
    let expected_sequence = plan
        .retained_boundary_sequence
        .checked_add(u64::from(row.ordinal))
        .ok_or_else(|| StorageLifetimeContractError::new("plan row sequence overflow"))?;
    if row.aggregate_sequence != expected_sequence {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan row aggregate sequence must match ordinal",
        ));
    }
    let message_pair_valid = match (
        row.introduced_message_id.as_deref(),
        row.introduced_message_digest.as_deref(),
    ) {
        (None, None) => !row.message_frozen_before_plan,
        (Some(id), Some(digest)) => !id.is_empty() && is_lowercase_hex_digest(digest),
        _ => false,
    };
    if !message_pair_valid {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan row message identity and digest must be exact",
        ));
    }
    let is_anchor = row.ordinal == STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1;
    let decision_valid = if is_anchor {
        row.decision == StorageCompactionPlanDecisionV1::RetainAnchor
            && row.aggregate_sequence == plan.anchor_aggregate_sequence
            && row.baseline_id.is_none()
            && row.baseline_projection_digest.is_none()
    } else {
        row.decision == StorageCompactionPlanDecisionV1::Delete
            && row.aggregate_sequence <= plan.delete_through_sequence
            && row.baseline_id.as_deref() == Some(plan.baseline_id.as_str())
            && row.baseline_projection_digest.as_deref()
                == Some(plan.baseline_projection_digest.as_str())
    };
    if !decision_valid {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan row decision or baseline binding is invalid",
        ));
    }
    Ok(row)
}

pub fn validate_storage_compaction_plan_row_binding_v1(
    plan: &StorageCompactionPlanV1,
    value: &Value,
    expected_operation_frozen_before_plan: bool,
    expected_message_digest: Option<&str>,
    expected_message_frozen_before_plan: bool,
) -> Result<StorageCompactionPlanRowV1, StorageLifetimeContractError> {
    let row = validate_storage_compaction_plan_row_v1(plan, value)?;
    if row.operation_frozen_before_plan != expected_operation_frozen_before_plan
        || row.message_frozen_before_plan != expected_message_frozen_before_plan
        || row.introduced_message_digest.as_deref() != expected_message_digest
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan row frozen source binding is invalid",
        ));
    }
    Ok(row)
}

pub fn validate_storage_compaction_plan_batch_v1(
    plan_value: &Value,
    rows_value: &Value,
    previous_after_plan_ordinal: u32,
    scan_exhausted: bool,
) -> Result<Vec<StorageCompactionPlanRowV1>, StorageLifetimeContractError> {
    let plan = validate_storage_compaction_plan_v1(plan_value)?;
    let rows = rows_value
        .as_array()
        .ok_or_else(|| StorageLifetimeContractError::new("plan row batch must be an array"))?;
    if rows.is_empty()
        || rows.len() > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1 as usize
        || previous_after_plan_ordinal >= STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan row batch must be positive and bounded",
        ));
    }
    let next_after_plan_ordinal = previous_after_plan_ordinal
        .checked_add(rows.len() as u32)
        .ok_or_else(|| StorageLifetimeContractError::new("plan row cursor overflow"))?;
    if next_after_plan_ordinal > STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1
        || scan_exhausted != (next_after_plan_ordinal == STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1)
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction plan exhaustion requires exactly ordinal 1001",
        ));
    }
    let mut source_rowids = BTreeSet::new();
    let mut operations = BTreeSet::new();
    let mut aggregate_sequences = BTreeSet::new();
    rows.iter()
        .enumerate()
        .map(|(index, value)| {
            let row = validate_storage_compaction_plan_row_v1(&plan, value)?;
            if row.ordinal != previous_after_plan_ordinal + index as u32 + 1
                || !source_rowids.insert(row.event_rowid)
                || !operations.insert(row.operation_id.clone())
                || !aggregate_sequences.insert(row.aggregate_sequence)
            {
                return Err(StorageLifetimeContractError::new(
                    "storage compaction plan rows must be gapless and unique",
                ));
            }
            Ok(row)
        })
        .collect()
}

pub fn validate_storage_compaction_cursor_v3(
    value: &Value,
) -> Result<StorageCompactionCursorV3, StorageLifetimeContractError> {
    let cursor: StorageCompactionCursorV3 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let position_valid = if cursor.phase == StorageCompactionPhaseV3::Finalize {
        cursor.after_plan_ordinal == 0 && cursor.scan_exhausted
    } else {
        cursor.after_plan_ordinal <= STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1
            && cursor.scan_exhausted
                == (cursor.after_plan_ordinal == STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1)
    };
    if cursor.schema_version != STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V3
        || cursor.cutoff_event_rowid == 0
        || !position_valid
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v3 cursor is invalid",
        ));
    }
    require_js_safe_unsigned(cursor.cutoff_event_rowid, "compactionV3.cutoffEventRowid")?;
    Ok(cursor)
}

pub fn validate_storage_compaction_cursor_transition_v3(
    previous_value: Option<&Value>,
    next_value: &Value,
    rows_in_step: u32,
) -> Result<StorageCompactionCursorV3, StorageLifetimeContractError> {
    let next = validate_storage_compaction_cursor_v3(next_value)?;
    if previous_value.is_none() {
        if next.phase != StorageCompactionPhaseV3::PlanEligibility
            || next.after_plan_ordinal != 0
            || next.scan_exhausted
            || rows_in_step != 0
        {
            return Err(StorageLifetimeContractError::new(
                "storage compaction v3 cursor must start at planEligibility zero",
            ));
        }
        return Ok(next);
    }
    let previous = validate_storage_compaction_cursor_v3(previous_value.expect("checked"))?;
    if previous.cutoff_event_rowid != next.cutoff_event_rowid
        || rows_in_step > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v3 cursor transition exceeds fixed boundary",
        ));
    }
    if previous.phase == next.phase {
        if previous.scan_exhausted
            || rows_in_step == 0
            || next.after_plan_ordinal != previous.after_plan_ordinal + rows_in_step
        {
            return Err(StorageLifetimeContractError::new(
                "storage compaction v3 cursor progress must be positive and consecutive",
            ));
        }
        return Ok(next);
    }
    let previous_index = storage_compaction_phase_v3_index(previous.phase);
    let next_index = storage_compaction_phase_v3_index(next.phase);
    let next_position_valid = if next.phase == StorageCompactionPhaseV3::Finalize {
        next.after_plan_ordinal == 0 && next.scan_exhausted
    } else {
        next.after_plan_ordinal == 0 && !next.scan_exhausted
    };
    if !previous.scan_exhausted
        || next_index != previous_index + 1
        || !next_position_valid
        || rows_in_step != 0
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v3 phases must advance exactly in order",
        ));
    }
    Ok(next)
}

pub fn validate_storage_compaction_admission_state_v3(
    value: &Value,
) -> Result<Option<StorageCompactionAdmissionStateV3>, StorageLifetimeContractError> {
    if value.is_null() {
        return Ok(None);
    }
    let state: StorageCompactionAdmissionStateV3 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_storage_compaction_private_state_v3(
        &serde_json::to_value(&state.private_state)
            .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
    )?;
    if state.job_id.is_empty()
        || state.state != StorageCompactionJobStateV1::Pending
        || state.authority_count != 0
        || !state.plan_header_present
    {
        return Err(StorageLifetimeContractError::new(
            "only exact active v3 pending compaction is admission-runnable",
        ));
    }
    Ok(Some(state))
}

pub fn validate_storage_compaction_private_state_v3(
    value: &Value,
) -> Result<StorageCompactionPrivateStateV3, StorageLifetimeContractError> {
    let state: StorageCompactionPrivateStateV3 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    let cursor = validate_storage_compaction_cursor_v3(
        &serde_json::to_value(&state.cursor)
            .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
    )?;
    let phase = storage_compaction_phase_v3_index(cursor.phase);
    let expected_plan_complete = phase > 0 || cursor.scan_exhausted;
    let expected_operations_verified = phase > 6 || (phase == 6 && cursor.scan_exhausted);
    let expected_equivalence_verified = phase > 7 || (phase == 7 && cursor.scan_exhausted);
    let expected_rows_released = phase > 8 || (phase == 8 && cursor.scan_exhausted);
    if state.schema_version != STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3
        || state.plan_rows_complete != expected_plan_complete
        || state.operations_verified != expected_operations_verified
        || state.equivalence_verified != expected_equivalence_verified
        || state.plan_rows_released != expected_rows_released
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v3 private state contradicts its cursor phase",
        ));
    }
    Ok(state)
}

pub fn validate_storage_compaction_job_v3(
    value: &Value,
) -> Result<StorageCompactionJobV3, StorageLifetimeContractError> {
    let job: StorageCompactionJobV3 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if job.job_id.is_empty() || job.authority_count > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1 {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v3 job identity or authority is invalid",
        ));
    }
    validate_storage_compaction_private_state_v3(
        &serde_json::to_value(&job.private_state)
            .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
    )?;
    match job.state {
        StorageCompactionJobStateV1::Pending => {
            if job.authority_count != 0 || job.failure_kind.is_some() || job.completed_at.is_some()
            {
                return Err(StorageLifetimeContractError::new(
                    "storage compaction v3 pending job must have empty authority",
                ));
            }
        }
        StorageCompactionJobStateV1::Running => {
            if job.failure_kind.is_some() || job.completed_at.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "storage compaction v3 running job cannot be terminal",
                ));
            }
        }
        StorageCompactionJobStateV1::Succeeded => {
            if job.authority_count != 0
                || job.failure_kind.is_some()
                || job.private_state.cursor.phase != StorageCompactionPhaseV3::Finalize
                || !job.private_state.cursor.scan_exhausted
                || !job.private_state.plan_rows_released
                || job.completed_at.is_none()
            {
                return Err(StorageLifetimeContractError::new(
                    "storage compaction v3 success requires released final evidence",
                ));
            }
            require_optional_timestamp(job.completed_at.as_deref(), "compactionV3.completedAt")?;
        }
        StorageCompactionJobStateV1::Failed => {
            if job.authority_count != 0
                || job.failure_kind.as_deref().is_none_or(str::is_empty)
                || job.completed_at.is_none()
            {
                return Err(StorageLifetimeContractError::new(
                    "storage compaction v3 failed job requires terminal evidence",
                ));
            }
            require_optional_timestamp(job.completed_at.as_deref(), "compactionV3.completedAt")?;
        }
    }
    Ok(job)
}

pub fn validate_storage_compaction_lifecycle_transition_v3(
    operation: StorageCompactionLifecycleOperationV3,
    previous_value: &Value,
    next_value: &Value,
    rows_in_step: u32,
) -> Result<StorageCompactionJobV3, StorageLifetimeContractError> {
    let previous = validate_storage_compaction_job_v3(previous_value)?;
    let next = validate_storage_compaction_job_v3(next_value)?;
    if previous.job_id != next.job_id || rows_in_step > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v3 transition must preserve job and budget",
        ));
    }
    let same_private = previous.private_state == next.private_state;
    let same_nonstate = previous.authority_count == next.authority_count
        && previous.failure_kind == next.failure_kind
        && previous.completed_at == next.completed_at;
    let valid = match operation {
        StorageCompactionLifecycleOperationV3::Arm => {
            rows_in_step == 0
                && previous.state == StorageCompactionJobStateV1::Pending
                && next.state == StorageCompactionJobStateV1::Running
                && same_private
                && previous.authority_count == 0
                && next.authority_count == 0
        }
        StorageCompactionLifecycleOperationV3::BatchSuccess => {
            previous.state == StorageCompactionJobStateV1::Running
                && next.state == StorageCompactionJobStateV1::Pending
                && next.authority_count == 0
                && previous.failure_kind.is_none()
                && next.failure_kind.is_none()
                && previous.completed_at.is_none()
                && next.completed_at.is_none()
                && validate_storage_compaction_cursor_transition_v3(
                    Some(
                        &serde_json::to_value(&previous.private_state.cursor)
                            .expect("cursor serialization"),
                    ),
                    &serde_json::to_value(&next.private_state.cursor)
                        .expect("cursor serialization"),
                    rows_in_step,
                )
                .is_ok()
        }
        StorageCompactionLifecycleOperationV3::CancelBeforeArm => {
            rows_in_step == 0
                && previous.state == StorageCompactionJobStateV1::Pending
                && next == previous
        }
        StorageCompactionLifecycleOperationV3::CancelAfterArm => {
            rows_in_step == 0
                && previous.state == StorageCompactionJobStateV1::Running
                && next == previous
        }
        StorageCompactionLifecycleOperationV3::CommitUncertain => {
            (rows_in_step == 0
                && previous.state == StorageCompactionJobStateV1::Running
                && next == previous)
                || (previous.state == StorageCompactionJobStateV1::Running
                    && next.state == StorageCompactionJobStateV1::Pending
                    && next.authority_count == 0
                    && validate_storage_compaction_cursor_transition_v3(
                        Some(
                            &serde_json::to_value(&previous.private_state.cursor)
                                .expect("cursor serialization"),
                        ),
                        &serde_json::to_value(&next.private_state.cursor)
                            .expect("cursor serialization"),
                        rows_in_step,
                    )
                    .is_ok())
        }
        StorageCompactionLifecycleOperationV3::Complete => {
            rows_in_step == 0
                && previous.state == StorageCompactionJobStateV1::Pending
                && next.state == StorageCompactionJobStateV1::Succeeded
                && same_private
                && previous.authority_count == 0
                && next.authority_count == 0
                && next.failure_kind.is_none()
                && next.completed_at.is_some()
        }
        StorageCompactionLifecycleOperationV3::Fail => {
            rows_in_step == 0
                && previous.state == StorageCompactionJobStateV1::Running
                && next.state == StorageCompactionJobStateV1::Failed
                && same_private
                && next.authority_count == 0
                && next
                    .failure_kind
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                && next.completed_at.is_some()
        }
    };
    if !valid || (matches!(operation, StorageCompactionLifecycleOperationV3::Arm) && !same_nonstate)
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v3 lifecycle transition is not atomic",
        ));
    }
    Ok(next)
}

fn storage_compaction_phase_v3_index(phase: StorageCompactionPhaseV3) -> usize {
    match phase {
        StorageCompactionPhaseV3::PlanEligibility => 0,
        StorageCompactionPhaseV3::FreezeOperations => 1,
        StorageCompactionPhaseV3::FreezeMessages => 2,
        StorageCompactionPhaseV3::VerifyPrefix => 3,
        StorageCompactionPhaseV3::AuthorizeDelete => 4,
        StorageCompactionPhaseV3::DeletePrefix => 5,
        StorageCompactionPhaseV3::VerifyOperations => 6,
        StorageCompactionPhaseV3::VerifyEquivalence => 7,
        StorageCompactionPhaseV3::ReleasePlan => 8,
        StorageCompactionPhaseV3::Finalize => 9,
    }
}

fn is_lowercase_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn canonical_frozen_operation_record_v1(
    source: &FrozenOperationSourceV1,
) -> Result<String, StorageLifetimeContractError> {
    canonical_json_v1(serde_json::json!({
        "aggregateId": source.aggregate_id,
        "aggregateSequence": source.aggregate_sequence,
        "aggregateType": source.aggregate_type,
        "canonicalPayloadDigest": source.canonical_payload_digest,
        "canonicalPayloadJson": source.canonical_payload_json,
        "eventTimestamp": source.event_timestamp,
        "eventType": source.event_type,
        "frozenAt": source.frozen_at,
        "operationId": source.operation_id,
        "payloadSchemaVersion": source.payload_schema_version,
        "recordSchemaVersion": 1,
        "requestId": source.request_id,
        "sourceEventId": source.source_event_id,
        "sourceEventRowid": source.source_event_rowid,
    }))
    .map_err(|error| StorageLifetimeContractError::new(error.to_string()))
}

pub fn validate_frozen_operation_source_v1(
    value: &Value,
) -> Result<FrozenOperationSourceV1, StorageLifetimeContractError> {
    let source: FrozenOperationSourceV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    require_js_safe_unsigned(source.source_event_rowid, "frozen operation source rowid")?;
    require_js_safe_unsigned(
        source.aggregate_sequence,
        "frozen operation aggregate sequence",
    )?;
    let digest_is_lower_hex = |digest: &str| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if source.operation_id.is_empty()
        || source.source_event_rowid == 0
        || source.source_event_id.is_empty()
        || source.request_id.as_deref().is_some_and(str::is_empty)
        || !matches!(
            source.aggregate_type.as_str(),
            "project" | "chat" | "run" | "lock"
        )
        || source.aggregate_id.is_empty()
        || source.aggregate_sequence == 0
        || source.event_type.is_empty()
        || source.payload_schema_version != 1
        || source.canonical_payload_json.is_empty()
        || !digest_is_lower_hex(&source.canonical_payload_digest)
        || source.canonical_record_json.is_empty()
        || !digest_is_lower_hex(&source.record_digest)
    {
        return Err(StorageLifetimeContractError::new(
            "frozen operation source fields are invalid",
        ));
    }
    validate_frozen_operation_timestamps_v1(&source.event_timestamp, &source.frozen_at)?;
    let payload: Value = serde_json::from_str(&source.canonical_payload_json).map_err(|error| {
        StorageLifetimeContractError::new(format!(
            "frozen operation canonical payload must be valid JSON: {error}"
        ))
    })?;
    let canonical_payload = canonical_json_v1(payload.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if !payload.is_object() || canonical_payload != source.canonical_payload_json {
        return Err(StorageLifetimeContractError::new(
            "frozen operation canonical payload must be one canonical JSON object",
        ));
    }
    let computed_payload = sha256_file_version_v1(source.canonical_payload_json.as_bytes());
    if computed_payload.strip_prefix("sha256:") != Some(source.canonical_payload_digest.as_str()) {
        return Err(StorageLifetimeContractError::new(
            "frozen operation payload digest does not match canonical payload",
        ));
    }
    let expected_record = canonical_frozen_operation_record_v1(&source)?;
    if source.canonical_record_json != expected_record {
        return Err(StorageLifetimeContractError::new(
            "frozen operation canonical record is not exact",
        ));
    }
    let computed_record = sha256_file_version_v1(expected_record.as_bytes());
    if computed_record.strip_prefix("sha256:") != Some(source.record_digest.as_str()) {
        return Err(StorageLifetimeContractError::new(
            "frozen operation record digest does not match canonical envelope",
        ));
    }
    Ok(source)
}

pub fn validate_frozen_operation_timestamps_v1(
    event_timestamp: &str,
    frozen_at: &str,
) -> Result<(), StorageLifetimeContractError> {
    require_timestamp(event_timestamp, "frozen operation eventTimestamp")?;
    require_timestamp(frozen_at, "frozen operation frozenAt")
}

pub fn validate_frozen_operation_source_batch_v1(
    value: &Value,
) -> Result<Vec<FrozenOperationSourceV1>, StorageLifetimeContractError> {
    let rows = value.as_array().ok_or_else(|| {
        StorageLifetimeContractError::new("frozen operation batch must be an array")
    })?;
    if rows.is_empty() || rows.len() > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1 {
        return Err(StorageLifetimeContractError::new(
            "frozen operation batch must contain one through one hundred rows",
        ));
    }
    let mut operations = BTreeSet::new();
    let mut rowids = BTreeSet::new();
    let mut payload_bytes = 0_u64;
    let mut sources = Vec::with_capacity(rows.len());
    for row in rows {
        let source = validate_frozen_operation_source_v1(row)?;
        payload_bytes = payload_bytes
            .checked_add(source.canonical_payload_json.len() as u64)
            .ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "frozen operation batch canonical payload budget exceeded",
                )
            })?;
        if !operations.insert(source.operation_id.clone())
            || !rowids.insert(source.source_event_rowid)
        {
            return Err(StorageLifetimeContractError::new(
                "frozen operation batch identities must be distinct",
            ));
        }
        sources.push(source);
    }
    if payload_bytes > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 {
        return Err(StorageLifetimeContractError::new(
            "frozen operation batch canonical payload budget exceeded",
        ));
    }
    Ok(sources)
}

pub fn validate_frozen_operation_source_binding_v1(
    value: &Value,
    event_rowid: u64,
    event: &LedgerEventV1,
    source_index_value: &Value,
    receipt_value: &Value,
) -> Result<FrozenOperationSourceV1, StorageLifetimeContractError> {
    let frozen = validate_frozen_operation_receipt_binding_v1(value, receipt_value)?;
    let source_index =
        validate_storage_event_source_index_binding_v1(event_rowid, source_index_value, event)?;
    if frozen.source_event_rowid != event_rowid
        || frozen.source_event_id != event.event_id
        || frozen.operation_id != event.operation_id
        || frozen.request_id != event.request_id
        || frozen.aggregate_type != event.aggregate_type
        || frozen.aggregate_id != event.aggregate_id
        || frozen.aggregate_sequence != event.aggregate_sequence
        || frozen.event_type != event.event_type
        || frozen.payload_schema_version != event.payload_schema_version
        || frozen.canonical_payload_json != event.canonical_json_payload
        || frozen.event_timestamp != event.timestamp
        || source_index.operation_id != frozen.operation_id
    {
        return Err(StorageLifetimeContractError::new(
            "frozen operation must bind one exact live event, source index, and receipt cross-check",
        ));
    }
    Ok(frozen)
}

pub fn validate_frozen_operation_receipt_binding_v1(
    value: &Value,
    receipt_value: &Value,
) -> Result<FrozenOperationSourceV1, StorageLifetimeContractError> {
    let frozen = validate_frozen_operation_source_v1(value)?;
    let receipt: OperationReceiptV2 = serde_json::from_value(receipt_value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_operation_receipt_v2(&receipt)?;
    if receipt.operation_id != frozen.operation_id
        || receipt.event_id != frozen.source_event_id
        || receipt.aggregate_type != frozen.aggregate_type
        || receipt.aggregate_id != frozen.aggregate_id
        || receipt.aggregate_sequence != frozen.aggregate_sequence
        || receipt.event_type != frozen.event_type
        || receipt.payload_schema_version != frozen.payload_schema_version
        || receipt.canonical_payload_json != frozen.canonical_payload_json
        || receipt.canonical_payload_digest != frozen.canonical_payload_digest
        || receipt.event_timestamp != frozen.event_timestamp
    {
        return Err(StorageLifetimeContractError::new(
            "frozen operation must bind one exact immutable receipt",
        ));
    }
    Ok(frozen)
}

pub fn validate_storage_runtime_source_authority_v1(
    value: &Value,
) -> Result<StorageRuntimeSourceAuthorityV1, StorageLifetimeContractError> {
    let authority: StorageRuntimeSourceAuthorityV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if !matches!(
        authority.aggregate_type.as_str(),
        "project" | "chat" | "run" | "lock"
    ) {
        return Err(StorageLifetimeContractError::new(
            "runtime authority aggregateType is invalid",
        ));
    }
    if authority
        .chat_predecessor_source
        .as_deref()
        .is_some_and(|source| source != "frozenOperationCanonicalPayload")
    {
        return Err(StorageLifetimeContractError::new(
            "chat predecessor cannot be reconstructed from a receipt",
        ));
    }
    let exact_chat_predecessor = if authority.aggregate_type == "chat" && !authority.event_present {
        authority.chat_predecessor_source.as_deref() == Some("frozenOperationCanonicalPayload")
    } else {
        authority.chat_predecessor_source.is_none()
    };
    if !exact_chat_predecessor {
        return Err(StorageLifetimeContractError::new(
            "deleted chat predecessor requires frozen operation canonical payload only",
        ));
    }
    let valid = if authority.event_present {
        authority.live_source_index_present
            && authority.live_source_matches
            && authority.receipt_present
            && authority.receipt_matches
    } else {
        !authority.live_source_index_present
            && authority.frozen_operation_present
            && authority.frozen_operation_matches
            && authority.receipt_present
            && authority.receipt_matches
    };
    if !valid {
        return Err(StorageLifetimeContractError::new(
            "runtime source authority is incomplete",
        ));
    }
    Ok(authority)
}

pub fn validate_storage_compaction_cursor_v2(
    value: &Value,
) -> Result<StorageCompactionCursorV2, StorageLifetimeContractError> {
    let cursor: StorageCompactionCursorV2 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    require_js_safe_unsigned(cursor.cutoff_event_rowid, "compaction v2 cutoff")?;
    require_js_safe_unsigned(cursor.after_event_rowid, "compaction v2 after")?;
    if cursor.schema_version != STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V2
        || cursor.after_event_rowid > cursor.cutoff_event_rowid
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction cursor v2 is invalid",
        ));
    }
    Ok(cursor)
}

pub fn validate_storage_compaction_cursor_transition_v2(
    previous: Option<&Value>,
    next: &Value,
    progress: Option<&Value>,
) -> Result<StorageCompactionCursorV2, StorageLifetimeContractError> {
    let next = validate_storage_compaction_cursor_v2(next)?;
    let Some(previous) = previous else {
        if progress.is_some()
            || next.phase != StorageCompactionPhaseV2::FreezeOperations
            || next.after_event_rowid != 0
            || next.scan_exhausted
        {
            return Err(StorageLifetimeContractError::new(
                "storage compaction v2 must start freezeOperations without public progress",
            ));
        }
        return Ok(next);
    };
    let previous = validate_storage_compaction_cursor_v2(previous)?;
    if next.cutoff_event_rowid != previous.cutoff_event_rowid {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v2 cannot change frozen cutoff",
        ));
    }
    if next.phase != previous.phase {
        if progress.is_some()
            || !previous.scan_exhausted
            || next.scan_exhausted
            || compaction_phase_index_v2(next.phase)
                != compaction_phase_index_v2(previous.phase) + 1
            || next.after_event_rowid != 0
        {
            return Err(StorageLifetimeContractError::new(
                "storage compaction v2 phase advance requires exact exhausted predecessor",
            ));
        }
        return Ok(next);
    }
    if next.scan_exhausted != previous.scan_exhausted || previous.scan_exhausted {
        return Err(StorageLifetimeContractError::new(
            "public compaction v2 transition cannot mint private exhaustion authority",
        ));
    }
    let progress_value = progress.ok_or_else(|| {
        StorageLifetimeContractError::new("storage compaction v2 progress is required")
    })?;
    let progress: StorageCompactionProgressV2 = serde_json::from_value(progress_value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    require_js_safe_unsigned(progress.canonical_payload_bytes, "compaction v2 bytes")?;
    if progress.rows_in_batch as usize > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
        || progress.canonical_payload_bytes
            > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1
        || progress.rows_in_batch == 0
        || next.after_event_rowid <= previous.after_event_rowid
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v2 progress requires a nonempty bounded batch",
        ));
    }
    Ok(next)
}

pub fn validate_storage_compaction_job_v2(
    value: &Value,
) -> Result<StorageCompactionJobV2, StorageLifetimeContractError> {
    let job: StorageCompactionJobV2 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_storage_compaction_cursor_v2(
        &serde_json::to_value(&job.cursor)
            .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
    )?;
    if job.job_id.is_empty() || job.authority_count as usize > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction job v2 fields are invalid",
        ));
    }
    match job.state {
        StorageCompactionJobStateV1::Pending | StorageCompactionJobStateV1::Running => {
            if job.failure_kind.is_some() || job.completed_at.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "nonterminal compaction v2 job cannot carry terminal evidence",
                ));
            }
        }
        StorageCompactionJobStateV1::Succeeded => {
            if job.authority_count != 0
                || !job.operations_verified
                || !job.equivalence_verified
                || job.cursor.phase != StorageCompactionPhaseV2::Finalize
                || !job.cursor.scan_exhausted
                || job.failure_kind.is_some()
            {
                return Err(StorageLifetimeContractError::new(
                    "successful compaction v2 requires verified authority-empty finalization",
                ));
            }
            require_timestamp(
                job.completed_at.as_deref().unwrap_or_default(),
                "compaction v2 completedAt",
            )?;
        }
        StorageCompactionJobStateV1::Failed => {
            if job.authority_count != 0
                || job
                    .failure_kind
                    .as_deref()
                    .is_none_or(|kind| !is_sanitized_failure_kind(kind))
            {
                return Err(StorageLifetimeContractError::new(
                    "failed compaction v2 requires empty authority and failure evidence",
                ));
            }
            require_timestamp(
                job.completed_at.as_deref().unwrap_or_default(),
                "compaction v2 completedAt",
            )?;
        }
    }
    Ok(job)
}

pub fn validate_storage_compaction_lifecycle_transition_v2(
    operation: StorageCompactionLifecycleOperationV2,
    previous: &Value,
    next: &Value,
    progress: Option<&Value>,
) -> Result<StorageCompactionJobV2, StorageLifetimeContractError> {
    let previous = validate_storage_compaction_job_v2(previous)?;
    let next = validate_storage_compaction_job_v2(next)?;
    if previous.job_id != next.job_id {
        return Err(StorageLifetimeContractError::new(
            "compaction v2 job identity cannot change",
        ));
    }
    let same = previous == next;
    let same_cursor = previous.cursor == next.cursor;
    let batch_committed = || -> Result<bool, StorageLifetimeContractError> {
        validate_storage_compaction_cursor_transition_v2(
            Some(
                &serde_json::to_value(&previous.cursor)
                    .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
            ),
            &serde_json::to_value(&next.cursor)
                .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
            progress,
        )?;
        Ok(previous.state == StorageCompactionJobStateV1::Running
            && next.state == StorageCompactionJobStateV1::Pending
            && next.authority_count == 0
            && previous.operations_verified == next.operations_verified
            && previous.equivalence_verified == next.equivalence_verified)
    };
    let valid = match operation {
        StorageCompactionLifecycleOperationV2::Arm => {
            previous.state == StorageCompactionJobStateV1::Pending
                && next.state == StorageCompactionJobStateV1::Running
                && previous.authority_count == 0
                && next.authority_count == 0
                && same_cursor
                && previous.operations_verified == next.operations_verified
                && previous.equivalence_verified == next.equivalence_verified
                && progress.is_none()
        }
        StorageCompactionLifecycleOperationV2::BatchSuccess => batch_committed()?,
        StorageCompactionLifecycleOperationV2::CancelBeforeArm => {
            previous.state == StorageCompactionJobStateV1::Pending && progress.is_none() && same
        }
        StorageCompactionLifecycleOperationV2::CancelAfterArm => {
            previous.state == StorageCompactionJobStateV1::Running && progress.is_none() && same
        }
        StorageCompactionLifecycleOperationV2::CommitUncertain => {
            (previous.state == StorageCompactionJobStateV1::Running && progress.is_none() && same)
                || batch_committed()?
        }
        StorageCompactionLifecycleOperationV2::Complete => false,
    };
    if !valid {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v2 lifecycle transition is invalid",
        ));
    }
    Ok(next)
}

#[cfg(test)]
fn validate_storage_compaction_equivalence_v2(
    value: &Value,
) -> Result<StorageCompactionEquivalenceEvidenceV2, StorageLifetimeContractError> {
    let evidence: StorageCompactionEquivalenceEvidenceV2 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if evidence.rows_in_batch == 0
        || evidence.rows_in_batch as usize > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
        || !evidence.operations_verified
        || !evidence.baseline_tail_before
        || !evidence.anchor_tail_after
        || !evidence.preserved_artifacts_byte_exact
        || !evidence.receipt_replay_equivalent
        || !evidence.namespace_decisions_equivalent
        || !evidence.frozen_message_ids_reserved
        || !evidence.authority_empty
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction v2 equivalence evidence is incomplete",
        ));
    }
    Ok(evidence)
}

pub fn validate_storage_compaction_cursor_transition_v1(
    previous: Option<&Value>,
    next: &Value,
) -> Result<StorageCompactionCursorV1, StorageLifetimeContractError> {
    let next = validate_storage_compaction_cursor_v1(next)?;
    let Some(previous) = previous else {
        if next.phase != StorageCompactionPhaseV1::FreezeMessages
            || next.after_event_rowid != 0
            || next.scan_exhausted
        {
            return Err(StorageLifetimeContractError::new(
                "storage compaction must start freezeMessages at zero unexhausted",
            ));
        }
        return Ok(next);
    };
    let previous = validate_storage_compaction_cursor_v1(previous)?;
    if next.cutoff_event_rowid != previous.cutoff_event_rowid {
        return Err(StorageLifetimeContractError::new(
            "storage compaction cursor cannot change its frozen cutoff",
        ));
    }
    if next.phase == previous.phase {
        if previous.scan_exhausted
            || next.scan_exhausted
            || next.after_event_rowid <= previous.after_event_rowid
        {
            return Err(StorageLifetimeContractError::new(
                "storage compaction same-phase progress must increase within one bounded batch",
            ));
        }
        return Ok(next);
    }
    let previous_index = compaction_phase_index(previous.phase);
    if !previous.scan_exhausted
        || next.scan_exhausted
        || STORAGE_COMPACTION_PHASES_V1.get(previous_index + 1)
            != Some(&compaction_phase_name(next.phase))
        || next.after_event_rowid != 0
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction phase advance requires exhausted exact predecessor",
        ));
    }
    Ok(next)
}

pub fn validate_storage_compaction_prefix_v1(
    value: &Value,
) -> Result<StorageCompactionPrefixV1, StorageLifetimeContractError> {
    let prefix: StorageCompactionPrefixV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_storage_compaction_prefix_fields_v1(&prefix)?;
    Ok(prefix)
}

/// The same prefix predicate for an already typed, bounded metadata read. It
/// does not establish the referenced baseline/source/anchor's provenance.
pub fn validate_storage_compaction_prefix_fields_v1(
    prefix: &StorageCompactionPrefixV1,
) -> Result<(), StorageLifetimeContractError> {
    require_js_safe_unsigned(
        prefix.deleted_through_sequence,
        "prefix.deletedThroughSequence",
    )?;
    require_js_safe_unsigned(prefix.anchor_event_rowid, "prefix.anchorEventRowid")?;
    require_js_safe_unsigned(
        prefix.anchor_aggregate_sequence,
        "prefix.anchorAggregateSequence",
    )?;
    if prefix.prefix_id.is_empty()
        || prefix.aggregate_type.is_empty()
        || prefix.aggregate_type == "app_instance"
        || prefix.aggregate_id.is_empty()
        || prefix.deleted_through_sequence == 0
        || prefix.anchor_event_rowid == 0
        || prefix.anchor_event_id.is_empty()
        || prefix.anchor_operation_id.is_empty()
        || prefix.anchor_aggregate_sequence != prefix.deleted_through_sequence + 1
        || prefix.baseline_id.is_empty()
        || prefix.baseline_projection_digest.is_empty()
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction prefix must bind an exact successor anchor and baseline",
        ));
    }
    require_timestamp(&prefix.created_at, "prefix.createdAt")?;
    require_timestamp(&prefix.verified_at, "prefix.verifiedAt")?;
    Ok(())
}

pub fn validate_frozen_message_source_v1(
    value: &Value,
) -> Result<FrozenMessageSourceV1, StorageLifetimeContractError> {
    let source: FrozenMessageSourceV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    require_js_safe_unsigned(source.source_event_rowid, "frozen.sourceEventRowid")?;
    require_js_safe_unsigned(
        source.source_aggregate_sequence,
        "frozen.sourceAggregateSequence",
    )?;
    require_js_safe_unsigned(source.durable_sequence, "frozen.durableSequence")?;
    if source.message_id.is_empty()
        || source.source_event_rowid == 0
        || source.source_event_id.is_empty()
        || source.source_operation_id.is_empty()
        || source.chat_id.is_empty()
        || source.source_aggregate_sequence == 0
        || source.durable_sequence == 0
        || source.canonical_message_json.is_empty()
        || source.message_digest.is_empty()
    {
        return Err(StorageLifetimeContractError::new(
            "frozen message source must preserve exact introduction provenance",
        ));
    }
    require_timestamp(&source.frozen_at, "frozen.frozenAt")?;
    Ok(source)
}

pub fn validate_storage_compaction_delete_authority_batch_v1(
    value: &Value,
) -> Result<Vec<StorageCompactionDeleteAuthorityV1>, StorageLifetimeContractError> {
    let rows: Vec<StorageCompactionDeleteAuthorityV1> = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if rows.is_empty() || rows.len() > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1 {
        return Err(StorageLifetimeContractError::new(
            "storage compaction authority batch must contain one through one hundred rows",
        ));
    }
    let expected_job = rows[0].job_id.clone();
    let mut ordinals = BTreeSet::new();
    let mut rowids = BTreeSet::new();
    let mut event_ids = BTreeSet::new();
    let mut operation_ids = BTreeSet::new();
    let mut aggregate_tuples = BTreeSet::new();
    for row in &rows {
        require_js_safe_unsigned(row.prefix_deleted_through_sequence, "authority.prefix")?;
        require_js_safe_unsigned(row.event_rowid, "authority.eventRowid")?;
        require_js_safe_unsigned(row.aggregate_sequence, "authority.aggregateSequence")?;
        if row.job_id.is_empty()
            || row.job_id != expected_job
            || row.ordinal == 0
            || row.ordinal as usize > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
            || row.prefix_id.is_empty()
            || row.prefix_deleted_through_sequence == 0
            || row.event_rowid == 0
            || row.event_id.is_empty()
            || row.operation_id.is_empty()
            || row.aggregate_type.is_empty()
            || row.aggregate_type == "app_instance"
            || row.aggregate_id.is_empty()
            || row.aggregate_sequence == 0
            || row.aggregate_sequence > row.prefix_deleted_through_sequence
            || row.introduced_message_id != row.frozen_message_id
            || !ordinals.insert(row.ordinal)
            || !rowids.insert(row.event_rowid)
            || !event_ids.insert(row.event_id.clone())
            || !operation_ids.insert(row.operation_id.clone())
            || !aggregate_tuples.insert((
                row.aggregate_type.clone(),
                row.aggregate_id.clone(),
                row.aggregate_sequence,
            ))
        {
            return Err(StorageLifetimeContractError::new(
                "storage compaction authority must be one exact distinct prefix-bound tuple",
            ));
        }
        require_timestamp(&row.authorized_at, "authority.authorizedAt")?;
    }
    Ok(rows)
}

pub fn validate_storage_compaction_job_v1(
    value: &Value,
) -> Result<StorageCompactionJobV1, StorageLifetimeContractError> {
    let job: StorageCompactionJobV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    validate_storage_compaction_cursor_v1(
        &serde_json::to_value(&job.cursor)
            .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?,
    )?;
    if job.job_id.is_empty() || job.authority_count as usize > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction job identity or authority count is invalid",
        ));
    }
    match job.state {
        StorageCompactionJobStateV1::Pending | StorageCompactionJobStateV1::Running => {
            if job.failure_kind.is_some() || job.completed_at.is_some() {
                return Err(StorageLifetimeContractError::new(
                    "nonterminal compaction job cannot carry terminal evidence",
                ));
            }
        }
        StorageCompactionJobStateV1::Succeeded => {
            if job.authority_count != 0
                || !job.equivalence_verified
                || job.cursor.phase != StorageCompactionPhaseV1::Finalize
                || !job.cursor.scan_exhausted
                || job.failure_kind.is_some()
            {
                return Err(StorageLifetimeContractError::new(
                    "successful compaction requires empty authority and verified finalization",
                ));
            }
            require_timestamp(
                job.completed_at.as_deref().unwrap_or_default(),
                "compaction.completedAt",
            )?;
        }
        StorageCompactionJobStateV1::Failed => {
            if job.authority_count != 0
                || job
                    .failure_kind
                    .as_deref()
                    .is_none_or(|kind| !is_sanitized_failure_kind(kind))
            {
                return Err(StorageLifetimeContractError::new(
                    "failed compaction requires empty authority and sanitized evidence",
                ));
            }
            require_timestamp(
                job.completed_at.as_deref().unwrap_or_default(),
                "compaction.completedAt",
            )?;
        }
    }
    Ok(job)
}

pub fn validate_storage_compaction_lifecycle_transition_v1(
    operation: StorageCompactionLifecycleOperationV1,
    previous: &Value,
    next: &Value,
) -> Result<StorageCompactionJobV1, StorageLifetimeContractError> {
    let previous = validate_storage_compaction_job_v1(previous)?;
    let next = validate_storage_compaction_job_v1(next)?;
    if previous.job_id != next.job_id {
        return Err(StorageLifetimeContractError::new(
            "storage compaction lifecycle cannot change job identity",
        ));
    }
    let same_cursor = previous.cursor == next.cursor;
    let valid = match operation {
        StorageCompactionLifecycleOperationV1::Arm => {
            previous.state == StorageCompactionJobStateV1::Pending
                && next.state == StorageCompactionJobStateV1::Running
                && previous.authority_count == 0
                && next.authority_count == 0
                && same_cursor
                && previous.equivalence_verified == next.equivalence_verified
        }
        StorageCompactionLifecycleOperationV1::BatchSuccess => {
            previous.state == StorageCompactionJobStateV1::Running
                && previous.authority_count > 0
                && next.state == StorageCompactionJobStateV1::Pending
                && next.authority_count == 0
                && validate_storage_compaction_cursor_transition_v1(
                    Some(&serde_json::to_value(&previous.cursor).expect("serializable cursor")),
                    &serde_json::to_value(&next.cursor).expect("serializable cursor"),
                )
                .is_ok()
        }
        StorageCompactionLifecycleOperationV1::CancelBeforeArm => {
            previous.state == StorageCompactionJobStateV1::Pending
                && previous.authority_count == 0
                && previous == next
        }
        StorageCompactionLifecycleOperationV1::CancelAfterArm => {
            previous.state == StorageCompactionJobStateV1::Running
                && previous.authority_count == 0
                && previous == next
        }
        StorageCompactionLifecycleOperationV1::Fail => {
            previous.state == StorageCompactionJobStateV1::Running
                && previous.authority_count == 0
                && next.state == StorageCompactionJobStateV1::Failed
                && next.authority_count == 0
                && same_cursor
        }
        StorageCompactionLifecycleOperationV1::Complete => {
            previous.state == StorageCompactionJobStateV1::Pending
                && previous.authority_count == 0
                && previous.equivalence_verified
                && previous.cursor.phase == StorageCompactionPhaseV1::Finalize
                && previous.cursor.scan_exhausted
                && next.state == StorageCompactionJobStateV1::Succeeded
                && same_cursor
        }
    };
    if !valid {
        return Err(StorageLifetimeContractError::new(
            "storage compaction lifecycle transition is not fail-closed",
        ));
    }
    Ok(next)
}

pub fn validate_storage_compaction_equivalence_v1(
    value: &Value,
) -> Result<StorageCompactionEquivalenceV1, StorageLifetimeContractError> {
    let evidence: StorageCompactionEquivalenceV1 = serde_json::from_value(value.clone())
        .map_err(|error| StorageLifetimeContractError::new(error.to_string()))?;
    if evidence.rows_in_batch == 0
        || evidence.rows_in_batch as usize > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
        || !evidence.baseline_tail_before
        || !evidence.anchor_tail_after
        || !evidence.preserved_artifacts_byte_exact
        || !evidence.receipt_replay_equivalent
        || !evidence.namespace_decisions_equivalent
        || !evidence.frozen_message_ids_reserved
        || !evidence.authority_empty
    {
        return Err(StorageLifetimeContractError::new(
            "storage compaction equivalence evidence is incomplete",
        ));
    }
    Ok(evidence)
}

fn compaction_phase_index(phase: StorageCompactionPhaseV1) -> usize {
    match phase {
        StorageCompactionPhaseV1::FreezeMessages => 0,
        StorageCompactionPhaseV1::VerifyPrefix => 1,
        StorageCompactionPhaseV1::AuthorizeDelete => 2,
        StorageCompactionPhaseV1::DeletePrefix => 3,
        StorageCompactionPhaseV1::VerifyEquivalence => 4,
        StorageCompactionPhaseV1::Finalize => 5,
    }
}

fn compaction_phase_name(phase: StorageCompactionPhaseV1) -> &'static str {
    STORAGE_COMPACTION_PHASES_V1[compaction_phase_index(phase)]
}

fn compaction_phase_index_v2(phase: StorageCompactionPhaseV2) -> usize {
    match phase {
        StorageCompactionPhaseV2::FreezeOperations => 0,
        StorageCompactionPhaseV2::FreezeMessages => 1,
        StorageCompactionPhaseV2::VerifyPrefix => 2,
        StorageCompactionPhaseV2::AuthorizeDelete => 3,
        StorageCompactionPhaseV2::DeletePrefix => 4,
        StorageCompactionPhaseV2::VerifyOperations => 5,
        StorageCompactionPhaseV2::VerifyEquivalence => 6,
        StorageCompactionPhaseV2::Finalize => 7,
    }
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
    storage_chat_snapshot_fields_v1(&source.aggregate_id, durable, object.get("last_message"))
}

/// Validate the actual extracted chat fields without constructing a truncated
/// ledger payload. Canonical payload and exact source binding remain the
/// consumer's independent obligations.
pub fn storage_chat_snapshot_fields_v1(
    aggregate_id: &str,
    durable: u64,
    last_message: Option<&Value>,
) -> Result<(u64, Option<String>, Option<Value>), StorageLifetimeContractError> {
    require_js_safe_unsigned(durable, "sourceIndex.chatDurableSequence")?;
    let message = match last_message {
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
    if let Some(message) = message.as_ref() {
        if message.get("chatId").and_then(Value::as_str) != Some(aggregate_id)
            || message.get("eventSequence").and_then(Value::as_u64) != Some(durable)
        {
            return Err(StorageLifetimeContractError::new(
                "chat snapshot message must match aggregate id and durable sequence",
            ));
        }
    }
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
        let predecessor = validate_storage_event_source_index_entry_v1(predecessor_value)?;
        if predecessor.aggregate_type != "chat"
            || predecessor.aggregate_id != source.aggregate_id
            || predecessor.aggregate_sequence + 1 != source.aggregate_sequence
        {
            return Err(StorageLifetimeContractError::new(
                "indexed predecessor must be the exact aggregate sequence minus one",
            ));
        }
        if predecessor.chat_snapshot_message_id.is_some() {
            let predecessor_source = predecessor_source.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "indexed non-null snapshot predecessor requires exact source provenance",
                )
            })?;
            let bound = validate_storage_event_source_index_binding_v1(
                predecessor.event_rowid,
                predecessor_value,
                predecessor_source,
            )?;
            if bound != predecessor {
                return Err(StorageLifetimeContractError::new(
                    "indexed predecessor source provenance differs from the index row",
                ));
            }
        } else if let Some(predecessor_source) = predecessor_source {
            validate_storage_event_source_index_binding_v1(
                predecessor.event_rowid,
                predecessor_value,
                predecessor_source,
            )?;
        }
        (
            predecessor.chat_durable_sequence.ok_or_else(|| {
                StorageLifetimeContractError::new(
                    "indexed chat predecessor must mirror durable sequence",
                )
            })?,
            predecessor.chat_snapshot_message_id,
        )
    } else {
        return Err(StorageLifetimeContractError::new(
            "chat source index transition requires the exact predecessor source index row",
        ));
    };

    validate_storage_chat_transition_fields_v1(
        &source.aggregate_id,
        current_durable,
        current_snapshot.as_deref(),
        current_message.as_ref(),
        previous_durable,
        previous_snapshot.as_deref(),
        entry.introduced_message_id.as_deref(),
    )?;
    Ok(entry)
}

/// Pure transition predicate shared with the fragment consumer. It does not
/// validate a source/index/predecessor binding or grant mutation authority.
pub fn validate_storage_chat_transition_fields_v1(
    aggregate_id: &str,
    current_durable: u64,
    current_snapshot: Option<&str>,
    current_message: Option<&Value>,
    previous_durable: u64,
    previous_snapshot: Option<&str>,
    introduced_message_id: Option<&str>,
) -> Result<(), StorageLifetimeContractError> {
    if previous_durable < MAX_SEQUENCE && current_durable == previous_durable + 1 {
        let message = current_message.ok_or_else(|| {
            StorageLifetimeContractError::new(
                "durable sequence increase requires a valid introduced TeamMessage",
            )
        })?;
        let message_sequence = message.get("eventSequence").and_then(Value::as_u64);
        let message_chat_id = message.get("chatId").and_then(Value::as_str);
        if message_sequence != Some(current_durable)
            || message_chat_id != Some(aggregate_id)
            || introduced_message_id != current_snapshot
        {
            return Err(StorageLifetimeContractError::new(
                "message introduction must match current durable sequence, chat, and snapshot",
            ));
        }
    } else if current_durable == previous_durable {
        if introduced_message_id.is_some()
            || current_snapshot.is_some_and(|message_id| Some(message_id) != previous_snapshot)
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
    Ok(())
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
