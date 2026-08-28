use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::fmt::{self, Display};

use crate::storage_v1::{classify_operation_v1, LedgerEventV1, OperationDecisionV1};
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
