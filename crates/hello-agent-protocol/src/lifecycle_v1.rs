use super::{ready_payload, ErrorV1, EventV1, PrepareQuitResultV1, ReadyPayloadV1};
use serde::{Deserialize, Serialize};
use serde_json::to_value;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display};

pub const G3_CAPABILITIES_V1: &[&str] = &["system.status", "app.prepareQuit"];
pub const CHANNEL_SYSTEM_GET_STATUS_V1: &str = "m5:v1:system:get-status";
pub const CHANNEL_APP_PREPARE_QUIT_V1: &str = "m5:v1:app:prepare-quit";
pub const CHANNEL_SIDECAR_EVENT_V1: &str = "m5:v1:sidecar:event";
pub const DATA_DIRECTORY_SEGMENTS_V1: &[&str] = &["com.wonseokhan.helloagent", "m5"];
pub const SQLITE_FILE_V1: &str = "hello-agent.sqlite3";
pub const PRODUCTION_SIDECAR_SEGMENTS_V1: &[&str] = &["sidecar", "hello-agent-sidecar"];
pub const PRODUCTION_SIDECAR_WINDOWS_SUFFIX_V1: &str = ".exe";
pub const PRODUCTION_SIDECAR_ARGV_V1: &[&str] = &["--stdio"];

pub const STARTUP_STEPS_V1: &[&str] = &[
    "sidecarStarted",
    "storageConnectionVerified",
    "migrationComplete",
    "integrityCheckComplete",
    "projectionReplayComplete",
    "recoveryComplete",
    "readyEmitted",
];

pub const PREPARE_QUIT_STEPS_V1: &[&str] = &[
    "mainQuitRequested",
    "mainQuitRequestCoalesced",
    "sidecarNewRequestsBlocked",
    "sidecarWorkStopped",
    "databaseCheckpointed",
    "cleanShutdownMarked",
    "sidecarSafeResponse",
    "mainExitAllowed",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleContractError {
    detail: String,
}

impl LifecycleContractError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for LifecycleContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for LifecycleContractError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "health",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DatabaseStatusV1 {
    Healthy { schema_version: u32 },
    Unavailable { schema_version: Option<u32> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SystemStatusV1 {
    Ready {
        sidecar: ReadyPayloadV1,
        database: DatabaseStatusV1,
        recovery_required: bool,
        blocker_ids: Vec<String>,
    },
    Unavailable {
        last_ready_sidecar: Option<ReadyPayloadV1>,
        database: DatabaseStatusV1,
        recovery_required: bool,
        blocker_ids: Vec<String>,
        error: ErrorV1,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StartupTraceV1 {
    Ready {
        steps: Vec<String>,
        ready: ReadyPayloadV1,
    },
    FailedBeforeReady {
        steps: Vec<String>,
        stdout_lines: Vec<String>,
        stderr: String,
        exit_code: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PrepareQuitTraceV1 {
    pub renderer_call_count: u32,
    pub sidecar_request_count: u32,
    pub steps: Vec<String>,
    pub response: PrepareQuitResultV1,
}

pub fn validate_system_status_v1(status: &SystemStatusV1) -> Result<(), LifecycleContractError> {
    let (database, recovery_required, blocker_ids) = match status {
        SystemStatusV1::Ready {
            sidecar,
            database,
            recovery_required,
            blocker_ids,
        } => {
            validate_ready_identity(sidecar)?;
            if !matches!(
                database,
                DatabaseStatusV1::Healthy { schema_version }
                    if *schema_version == super::APP_SCHEMA_VERSION
            ) {
                return Err(LifecycleContractError::new(
                    "ready status requires a healthy v1 database",
                ));
            }
            (database, recovery_required, blocker_ids)
        }
        SystemStatusV1::Unavailable {
            last_ready_sidecar,
            database,
            recovery_required,
            blocker_ids,
            error,
        } => {
            if let Some(sidecar) = last_ready_sidecar {
                validate_ready_identity(sidecar)?;
            }
            if !matches!(database, DatabaseStatusV1::Unavailable { schema_version } if schema_version.is_none() || *schema_version == Some(super::APP_SCHEMA_VERSION))
            {
                return Err(LifecycleContractError::new(
                    "unavailable status has an invalid database state",
                ));
            }
            validate_error(error)?;
            (database, recovery_required, blocker_ids)
        }
    };
    let _ = database;
    validate_blockers(*recovery_required, blocker_ids)
}

pub fn validate_startup_trace_v1(trace: &StartupTraceV1) -> Result<(), LifecycleContractError> {
    let steps = match trace {
        StartupTraceV1::Ready { steps, ready } => {
            if !same_strings(steps, STARTUP_STEPS_V1) {
                return Err(LifecycleContractError::new(
                    "ready requires verified storage connection, migration, integrity, replay, and recovery",
                ));
            }
            validate_ready_identity(ready)?;
            return Ok(());
        }
        StartupTraceV1::FailedBeforeReady {
            steps,
            stdout_lines,
            stderr,
            exit_code,
        } => {
            if !stdout_lines.is_empty() {
                return Err(LifecycleContractError::new(
                    "pre-ready failure cannot write a protocol envelope to stdout",
                ));
            }
            if *exit_code == 0 {
                return Err(LifecycleContractError::new(
                    "pre-ready failure must use a nonzero exit code",
                ));
            }
            if stderr.trim().is_empty() {
                return Err(LifecycleContractError::new(
                    "pre-ready failure must write diagnostic evidence to stderr",
                ));
            }
            steps
        }
    };
    if steps.iter().any(|step| step == "readyEmitted") {
        return Err(LifecycleContractError::new(
            "pre-ready failure cannot emit ready",
        ));
    }
    validate_ordered_prefix(steps, STARTUP_STEPS_V1, "startup")
}

pub fn validate_prepare_quit_trace_v1(
    trace: &PrepareQuitTraceV1,
) -> Result<(), LifecycleContractError> {
    if trace.renderer_call_count == 0 {
        return Err(LifecycleContractError::new(
            "prepareQuit must originate from at least one renderer call",
        ));
    }
    if trace.sidecar_request_count != 1 {
        return Err(LifecycleContractError::new(
            "main must coordinate prepareQuit as one sidecar request",
        ));
    }
    let expected: Vec<&str> = PREPARE_QUIT_STEPS_V1
        .iter()
        .copied()
        .filter(|step| trace.renderer_call_count > 1 || *step != "mainQuitRequestCoalesced")
        .collect();
    if !same_strings(&trace.steps, &expected) {
        return Err(LifecycleContractError::new(
            "prepareQuit steps do not prove main-owned single-flight safe shutdown",
        ));
    }
    if !trace.response.safe_to_quit || !trace.response.blockers.is_empty() {
        return Err(LifecycleContractError::new(
            "a completed prepareQuit trace must end in a safe response without blockers",
        ));
    }
    Ok(())
}

fn validate_ready_identity(ready: &ReadyPayloadV1) -> Result<(), LifecycleContractError> {
    let event = EventV1 {
        protocol_version: super::PROTOCOL_VERSION,
        kind: "event".to_string(),
        sidecar_instance_id: ready.sidecar_instance_id.clone(),
        stream_id: "system".to_string(),
        seq: 1,
        event: "system.ready".to_string(),
        chat_id: None,
        task_id: None,
        run_id: None,
        payload: to_value(ready).map_err(|error| {
            LifecycleContractError::new(format!("could not encode ready identity: {error}"))
        })?,
    };
    ready_payload(&event)
        .map_err(|error| LifecycleContractError::new(format!("invalid ready identity: {error}")))?;
    if !same_strings(&ready.capabilities, G3_CAPABILITIES_V1) {
        return Err(LifecycleContractError::new(
            "G3 ready capabilities must contain only system.status and app.prepareQuit",
        ));
    }
    Ok(())
}

fn validate_error(error: &ErrorV1) -> Result<(), LifecycleContractError> {
    for (name, value) in [
        ("code", error.code.as_str()),
        ("kind", error.kind.as_str()),
        ("detail", error.detail.as_str()),
    ] {
        if value.is_empty() {
            return Err(LifecycleContractError::new(format!(
                "system status error {name} is required"
            )));
        }
    }
    if error.outcome_unknown == Some(true) && error.retryable {
        return Err(LifecycleContractError::new(
            "unknown dangerous outcome cannot be auto-retryable",
        ));
    }
    Ok(())
}

fn validate_blockers(
    recovery_required: bool,
    blocker_ids: &[String],
) -> Result<(), LifecycleContractError> {
    if blocker_ids.iter().any(String::is_empty) {
        return Err(LifecycleContractError::new(
            "system status blockerIds cannot be empty",
        ));
    }
    let unique: HashSet<_> = blocker_ids.iter().collect();
    if unique.len() != blocker_ids.len() {
        return Err(LifecycleContractError::new(
            "system status blockerIds must be unique",
        ));
    }
    if recovery_required != !blocker_ids.is_empty() {
        return Err(LifecycleContractError::new(
            "recoveryRequired must agree with blockerIds",
        ));
    }
    Ok(())
}

fn validate_ordered_prefix(
    actual: &[String],
    expected: &[&str],
    name: &str,
) -> Result<(), LifecycleContractError> {
    if actual
        .iter()
        .enumerate()
        .any(|(index, step)| expected.get(index).copied() != Some(step.as_str()))
    {
        Err(LifecycleContractError::new(format!(
            "{name} steps must be an ordered prefix without gaps or duplicates"
        )))
    } else {
        Ok(())
    }
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
