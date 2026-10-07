use crate::{ErrorV1, ReadyPayloadV1, APP_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display};

pub const G5D_ALLOWED_WHILE_STORAGE_NOT_READY_V1: &[&str] = &["system.status", "app.prepareQuit"];
pub const G5D_STORAGE_READY_ONLY_METHODS_V1: &[&str] = &["storage.status"];
pub const G5D_STARTUP_STEPS_V1: &[&str] = &[
    "sidecarStarted",
    "shellReadyEmitted",
    "storageConnectionVerified",
    "migrationComplete",
    "integrityCheckComplete",
    "projectionReplayComplete",
    "recoveryComplete",
    "verifiedForStartup",
    "appInstanceRegistered",
    "storageAdmissionReady",
];
pub const STORAGE_ADMISSION_FAILED_DETAIL_V1: &str = "storage admission failed";
pub const STORAGE_VERIFYING_BLOCKED_DETAIL_V1: &str = "storage admission is still verifying";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct G5DAdmissionContractError {
    detail: String,
}

impl G5DAdmissionContractError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for G5DAdmissionContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for G5DAdmissionContractError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SystemStateG5DV1 {
    ShellReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageAdmissionStateV1 {
    Verifying,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageAdmissionV1 {
    pub schema_version: u32,
    pub state: StorageAdmissionStateV1,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error: Option<ErrorV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "health",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DatabaseStatusG5DV1 {
    Verifying { schema_version: Option<u32> },
    Healthy { schema_version: u32 },
    Unavailable { schema_version: Option<u32> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SystemStatusG5DV1 {
    pub schema_version: u32,
    pub state: SystemStateG5DV1,
    pub sidecar: ReadyPayloadV1,
    pub storage_admission: StorageAdmissionV1,
    pub database: DatabaseStatusG5DV1,
    pub recovery_required: Option<bool>,
    pub blocker_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StartupTraceG5DV1 {
    pub schema_version: u32,
    pub steps: Vec<String>,
}

pub fn parse_g5d_product_system_status_v1(
    value: &Value,
) -> Result<SystemStatusG5DV1, G5DAdmissionContractError> {
    parse_system_status(value, |sidecar| {
        crate::run_recovery_v1::validate_g5d_product_ready_payload_v1(sidecar)
            .map_err(|error| G5DAdmissionContractError::new(error.to_string()))
    })
}

/// Preserve the exact D1 baseline while allowing the negotiated follow-up suffix.
pub fn parse_current_product_system_status_v1(
    value: &Value,
) -> Result<SystemStatusG5DV1, G5DAdmissionContractError> {
    parse_followup_status(value, parse_g5d_product_system_status_v1)
}

pub fn parse_current_agent_fixture_system_status_v1(
    value: &Value,
) -> Result<SystemStatusG5DV1, G5DAdmissionContractError> {
    parse_followup_status(value, parse_g5d_agent_fixture_system_status_v1)
}

pub fn parse_current_native_system_status_v1(value: &Value) -> Result<SystemStatusG5DV1, G5DAdmissionContractError> {
    parse_followup_status(value, |value| parse_system_status(value, |sidecar| {
        crate::agent_adapter_v1::validate_g5d_native_ready_payload_v1(sidecar)
            .map_err(|error| G5DAdmissionContractError::new(error.to_string()))
    }))
}

fn parse_followup_status(
    value: &Value,
    baseline: fn(&Value) -> Result<SystemStatusG5DV1, G5DAdmissionContractError>,
) -> Result<SystemStatusG5DV1, G5DAdmissionContractError> {
    let mut normalized = value.clone();
    let capabilities = normalized
        .pointer_mut("/sidecar/capabilities")
        .and_then(Value::as_array_mut);
    let mut login = false;
    let mut approval = false;
    let mut catalog = false;
    let extended = capabilities.is_some_and(|capabilities| {
        if capabilities.last().and_then(Value::as_str) == Some("project.catalog") {
            capabilities.pop();
            catalog = true;
        }
        if capabilities.last().and_then(Value::as_str) == Some("run.approval") {
            capabilities.pop();
            approval = true;
        }
        if capabilities.last().and_then(Value::as_str) == Some("provider.login") {
            capabilities.pop();
            login = true;
        }
        if capabilities.last().and_then(Value::as_str) == Some("chat.appendMessage") {
            capabilities.pop();
            true
        } else {
            false
        }
    });
    let mut parsed = baseline(&normalized)?;
    if catalog && !approval {
        return Err(G5DAdmissionContractError::new(
            "catalog requires approval baseline",
        ));
    }
    if approval && !login {
        return Err(G5DAdmissionContractError::new(
            "approval requires login baseline",
        ));
    }
    if login && !extended {
        return Err(G5DAdmissionContractError::new(
            "login requires followup baseline",
        ));
    }
    if extended {
        parsed
            .sidecar
            .capabilities
            .push("chat.appendMessage".to_owned());
    }
    if login {
        parsed
            .sidecar
            .capabilities
            .push("provider.login".to_owned());
    }
    if approval {
        parsed.sidecar.capabilities.push("run.approval".to_owned());
    }
    if catalog {
        parsed
            .sidecar
            .capabilities
            .push("project.catalog".to_owned());
    }
    Ok(parsed)
}

pub fn parse_g5d_agent_fixture_system_status_v1(
    value: &Value,
) -> Result<SystemStatusG5DV1, G5DAdmissionContractError> {
    parse_system_status(value, |sidecar| {
        crate::agent_adapter_v1::validate_g5d_agent_fixture_ready_payload_v1(sidecar)
            .map_err(|error| G5DAdmissionContractError::new(error.to_string()))
    })
}

pub fn validate_g5d_blocked_request_error_v1(
    state: StorageAdmissionStateV1,
    error: &ErrorV1,
) -> Result<(), G5DAdmissionContractError> {
    let expected = match state {
        StorageAdmissionStateV1::Verifying => ErrorV1 {
            code: "storageVerifying".to_string(),
            kind: "requestBlocked".to_string(),
            detail: STORAGE_VERIFYING_BLOCKED_DETAIL_V1.to_string(),
            retryable: true,
            outcome_unknown: Some(false),
        },
        StorageAdmissionStateV1::Failed => ErrorV1 {
            code: "storageAdmissionFailed".to_string(),
            kind: "requestBlocked".to_string(),
            detail: STORAGE_ADMISSION_FAILED_DETAIL_V1.to_string(),
            retryable: false,
            outcome_unknown: Some(false),
        },
        StorageAdmissionStateV1::Ready => {
            return Err(G5DAdmissionContractError::new(
                "ready storage does not produce an admission-blocked error",
            ));
        }
    };
    if error != &expected {
        return Err(G5DAdmissionContractError::new(
            "blocked request error does not match storage admission state",
        ));
    }
    Ok(())
}

pub fn validate_g5d_startup_trace_v1(
    trace: &StartupTraceG5DV1,
) -> Result<(), G5DAdmissionContractError> {
    if trace.schema_version != 1 {
        return Err(G5DAdmissionContractError::new(
            "G5D startup trace schemaVersion must be 1",
        ));
    }
    if !same_strings(&trace.steps, G5D_STARTUP_STEPS_V1) {
        return Err(G5DAdmissionContractError::new(
            "G5D startup must emit shell ready before storage verification and admit only after verifiedForStartup and app registration",
        ));
    }
    Ok(())
}

fn parse_system_status(
    value: &Value,
    validate_sidecar: impl FnOnce(&Value) -> Result<(), G5DAdmissionContractError>,
) -> Result<SystemStatusG5DV1, G5DAdmissionContractError> {
    exact_fields(
        value,
        &[
            "schemaVersion",
            "state",
            "sidecar",
            "storageAdmission",
            "database",
            "recoveryRequired",
            "blockerIds",
        ],
        "G5D system status",
    )?;
    let object = value
        .as_object()
        .ok_or_else(|| G5DAdmissionContractError::new("G5D system status must be an object"))?;
    exact_fields(
        object
            .get("storageAdmission")
            .ok_or_else(|| G5DAdmissionContractError::new("storageAdmission is required"))?,
        &[
            "schemaVersion",
            "state",
            "startedAt",
            "completedAt",
            "error",
        ],
        "storageAdmission",
    )?;
    exact_fields(
        object
            .get("database")
            .ok_or_else(|| G5DAdmissionContractError::new("database is required"))?,
        &["health", "schemaVersion"],
        "database",
    )?;
    let sidecar = object
        .get("sidecar")
        .ok_or_else(|| G5DAdmissionContractError::new("sidecar is required"))?;
    validate_sidecar(sidecar)?;
    let status: SystemStatusG5DV1 = serde_json::from_value(value.clone())
        .map_err(|error| G5DAdmissionContractError::new(error.to_string()))?;
    validate_system_status(&status)?;
    Ok(status)
}

fn validate_system_status(status: &SystemStatusG5DV1) -> Result<(), G5DAdmissionContractError> {
    if status.schema_version != 1 || !matches!(status.state, SystemStateG5DV1::ShellReady) {
        return Err(G5DAdmissionContractError::new(
            "G5D status must describe the schema v1 shellReady state",
        ));
    }
    if status.storage_admission.schema_version != 1 {
        return Err(G5DAdmissionContractError::new(
            "storageAdmission schemaVersion must be 1",
        ));
    }
    require_timestamp(&status.storage_admission.started_at, "startedAt")?;

    match status.storage_admission.state {
        StorageAdmissionStateV1::Verifying => {
            if status.storage_admission.completed_at.is_some()
                || status.storage_admission.error.is_some()
                || !matches!(
                    status.database,
                    DatabaseStatusG5DV1::Verifying {
                        schema_version: None
                    }
                )
                || status.recovery_required.is_some()
                || !status.blocker_ids.is_empty()
            {
                return Err(G5DAdmissionContractError::new(
                    "verifying storage admission has an invalid cross-state combination",
                ));
            }
        }
        StorageAdmissionStateV1::Ready => {
            validate_completed_at(&status.storage_admission)?;
            if status.storage_admission.error.is_some()
                || !matches!(
                    status.database,
                    DatabaseStatusG5DV1::Healthy { schema_version }
                        if schema_version == APP_SCHEMA_VERSION
                )
            {
                return Err(G5DAdmissionContractError::new(
                    "ready storage admission requires a healthy v1 database and no error",
                ));
            }
            validate_blockers(status.recovery_required, &status.blocker_ids)?;
        }
        StorageAdmissionStateV1::Failed => {
            validate_completed_at(&status.storage_admission)?;
            let Some(error) = &status.storage_admission.error else {
                return Err(G5DAdmissionContractError::new(
                    "failed storage admission requires its fixed public error",
                ));
            };
            let expected = ErrorV1 {
                code: "storageAdmissionFailed".to_string(),
                kind: "storageUnavailable".to_string(),
                detail: STORAGE_ADMISSION_FAILED_DETAIL_V1.to_string(),
                retryable: false,
                outcome_unknown: Some(true),
            };
            if error != &expected
                || !matches!(
                    status.database,
                    DatabaseStatusG5DV1::Unavailable { schema_version }
                        if schema_version.is_none() || schema_version == Some(APP_SCHEMA_VERSION)
                )
                || status.recovery_required.is_some()
                || !status.blocker_ids.is_empty()
            {
                return Err(G5DAdmissionContractError::new(
                    "failed storage admission has an invalid cross-state combination",
                ));
            }
        }
    }
    Ok(())
}

fn validate_completed_at(admission: &StorageAdmissionV1) -> Result<(), G5DAdmissionContractError> {
    let completed_at = admission
        .completed_at
        .as_deref()
        .ok_or_else(|| G5DAdmissionContractError::new("completedAt is required"))?;
    require_timestamp(completed_at, "completedAt")?;
    if completed_at < admission.started_at.as_str() {
        return Err(G5DAdmissionContractError::new(
            "completedAt cannot precede startedAt",
        ));
    }
    Ok(())
}

fn validate_blockers(
    recovery_required: Option<bool>,
    blocker_ids: &[String],
) -> Result<(), G5DAdmissionContractError> {
    let Some(recovery_required) = recovery_required else {
        return Err(G5DAdmissionContractError::new(
            "ready storage admission requires recoveryRequired",
        ));
    };
    if blocker_ids.iter().any(String::is_empty) {
        return Err(G5DAdmissionContractError::new(
            "blockerIds cannot contain an empty identity",
        ));
    }
    let unique: HashSet<_> = blocker_ids.iter().collect();
    if unique.len() != blocker_ids.len() || recovery_required != !blocker_ids.is_empty() {
        return Err(G5DAdmissionContractError::new(
            "recoveryRequired must match unique blockerIds",
        ));
    }
    Ok(())
}

fn exact_fields(
    value: &Value,
    expected: &[&str],
    name: &str,
) -> Result<(), G5DAdmissionContractError> {
    let object = value
        .as_object()
        .ok_or_else(|| G5DAdmissionContractError::new(format!("{name} must be an object")))?;
    if object.len() != expected.len() || expected.iter().any(|field| !object.contains_key(*field)) {
        return Err(G5DAdmissionContractError::new(format!(
            "{name} must contain the exact contract fields"
        )));
    }
    Ok(())
}

fn require_timestamp(value: &str, name: &str) -> Result<(), G5DAdmissionContractError> {
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
        return Err(G5DAdmissionContractError::new(format!(
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
        return Err(G5DAdmissionContractError::new(format!(
            "{name} must be an RFC 3339 UTC second timestamp"
        )));
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
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
    if year < 1
        || !(1..=12).contains(&month)
        || day < 1
        || day > days[month as usize]
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(G5DAdmissionContractError::new(format!(
            "{name} must be a real RFC 3339 UTC calendar timestamp"
        )));
    }
    Ok(())
}

fn same_strings<T: AsRef<str>, U: AsRef<str>>(left: &[T], right: &[U]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.as_ref() == right.as_ref())
}

#[cfg(test)]
mod tests;
