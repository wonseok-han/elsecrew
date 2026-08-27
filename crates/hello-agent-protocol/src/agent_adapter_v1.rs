use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display};

use crate::run_recovery_v1::{
    G5_CAPABILITIES_V1, G5_EVENT_PAGE_MAX_ITEMS_V1, G5_ID_MAX_BYTES_V1,
    G5_MODEL_SELECTOR_MAX_BYTES_V1, G5_PERMISSIONS_V1, G5_PUBLIC_MESSAGE_MAX_BYTES_V1,
    G5_REASONING_EFFORT_MAX_BYTES_V1, G5_ROLE_IDS_V1, G5_RUNTIMES_V1,
};

pub const G5A_SCHEMA_VERSION_V1: u64 = 1;
pub const G5A_CAPABILITIES_V1: &[&str] = &[
    "system.status",
    "app.prepareQuit",
    "project.open",
    "project.getTree",
    "project.createWorktree",
    "project.close",
    "file.read",
    "file.save",
    "file.watch",
    "diff.getSummary",
    "diff.getFile",
    "workspace.getTree",
    "workspace.close",
    "workspace.file.read",
    "workspace.file.save",
    "workspace.file.watch",
    "workspace.diff.getSummary",
    "workspace.diff.getFile",
    "workspace.prepareChat",
    "workspace.abandonPrepared",
    "project.getRoleSettings",
    "project.updateRoleSettings",
    "chat.create",
    "chat.list",
    "chat.get",
    "run.startFixture",
    "run.cancel",
    "recovery.list",
    "recovery.inspect",
    "recovery.markResolved",
    "agent.getCapabilities",
    "run.startAgent",
    "run.cancelAgent",
    "recovery.inspectAgent",
    "recovery.resolveAgent",
];
pub const AGENT_INSTALLATION_STATES_V1: &[&str] = &["installed", "notInstalled", "incomplete"];
pub const AGENT_AUTH_STATES_V1: &[&str] =
    &["authenticated", "notAuthenticated", "expired", "unknown"];
pub const AGENT_DISCOVERY_SOURCES_V1: &[&str] = &[
    "machineReadableCatalog",
    "exactVersionManifest",
    "unavailable",
];
pub const AGENT_VERIFICATION_LEVELS_V1: &[&str] =
    &["runtimeVerified", "measuredExactVersion", "unknown"];
pub const AGENT_MODEL_EVIDENCE_V1: &[&str] = &[
    "runtimeVerified",
    "measuredExactVersion",
    "argvOnly",
    "unknown",
];
pub const AGENT_START_OUTCOMES_V1: &[&str] = &[
    "started",
    "platformUnsupported",
    "notInstalled",
    "installationIncomplete",
    "unsupportedVersion",
    "notAuthenticated",
    "authExpired",
    "capabilityUnknown",
    "exactModelUnavailable",
    "reasoningUnsupported",
    "permissionUnsupported",
    "profileChanged",
    "profileUnavailable",
    "workspaceAbandoned",
    "recoveryBlocked",
    "chatStateBlocked",
    "runActive",
    "writerLocked",
];
pub const AGENT_CAPABILITY_BLOCK_REASONS_V1: &[&str] = &[
    "platformUnsupported",
    "notInstalled",
    "installationIncomplete",
    "unsupportedVersion",
    "notAuthenticated",
    "authExpired",
    "capabilityUnknown",
    "exactModelUnavailable",
];
pub const AGENT_PUBLIC_EVENT_KINDS_V1: &[&str] = &["status", "question", "final"];
pub const AGENT_RUN_STATE_EVENT_STATES_V1: &[&str] = &[
    "running",
    "succeeded",
    "failed",
    "canceled",
    "recoveryRequired",
];
pub const G5C_PROVIDER_IDS_V1: &[&str] = &["claude-code", "codex"];
pub const G5C_MODEL_CATALOG_EVIDENCE_V1: &[&str] = &[
    "runtimeVerified",
    "exactVersionManifest",
    "argvOnly",
    "unknown",
];
pub const G5C_RUNTIME_VERIFICATION_IDS_V1: &[&str] = &[
    "version",
    "model",
    "cwd",
    "permission",
    "tools",
    "mcp",
    "plugins",
    "config",
    "sandbox",
    "approvalPolicy",
];
pub const G5C_CLAUDE_RUNTIME_VERIFICATION_V1: &[&str] = &[
    "version",
    "model",
    "cwd",
    "permission",
    "tools",
    "mcp",
    "plugins",
    "config",
];
pub const G5C_CODEX_RUNTIME_VERIFICATION_V1: &[&str] = &[
    "version",
    "model",
    "cwd",
    "permission",
    "sandbox",
    "approvalPolicy",
    "config",
];
pub const G5C_DISCOVERY_BLOCK_REASONS_V1: &[&str] = &[
    "notInstalled",
    "notAuthenticated",
    "authExpired",
    "closureMismatch",
    "unsupportedVersion",
    "modelCatalogUnverified",
    "unsupportedProfile",
    "runtimeVerificationIncomplete",
];
pub const G5C_PROVIDER_EVENT_CLASSIFICATIONS_V1: &[&str] =
    &["Handshake", "Public", "Private", "Terminal", "AuthFailure"];
pub const G5C_INTERNAL_EVENTS_PER_RUN_MAX_V1: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAdapterContractError {
    detail: String,
}

impl AgentAdapterContractError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for AgentAdapterContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for AgentAdapterContractError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderExecutableFileIdentityV1 {
    pub logical_name: String,
    pub path_digest: String,
    pub file_identity_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderExecutableBinaryIdentityV1 {
    pub logical_name: String,
    pub path_digest: String,
    pub file_identity_digest: String,
    pub cli_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderExecutableClosureV1 {
    pub binary: ProviderExecutableBinaryIdentityV1,
    pub required_helpers: Vec<ProviderExecutableFileIdentityV1>,
    pub closure_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderProfileTupleV1 {
    pub model_selector: String,
    pub reasoning_effort: Option<String>,
    pub permission: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderLaunchPolicyV1 {
    pub invocation: String,
    pub prompt_transport: String,
    pub cwd_policy: String,
    pub environment_policy: String,
    pub fallback_policy: String,
    pub required_runtime_verification: Vec<String>,
}

/// Internal canonical provider definition. It is not a protocol-v1 request or event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderManifestV1 {
    pub schema_version: u64,
    pub provider_id: String,
    pub runtime: String,
    pub manifest_version: String,
    pub executable_closure: ProviderExecutableClosureV1,
    pub exact_profiles: Vec<ProviderProfileTupleV1>,
    pub auth_parser_id: String,
    pub event_parser_id: String,
    pub launch_policy: ProviderLaunchPolicyV1,
    pub manifest_digest: String,
}

/// Persistable discovery evidence with an exact redacted field set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SanitizedDiscoveryObservationV1 {
    pub schema_version: u64,
    pub provider_id: String,
    pub runtime: String,
    pub manifest_digest: String,
    pub installation_state: String,
    pub auth_state: String,
    pub executable_closure: Option<ProviderExecutableClosureV1>,
    pub exact_cli_version: Option<String>,
    pub model_catalog_evidence: String,
    pub exact_profiles: Vec<ProviderProfileTupleV1>,
    pub auth_parser_id: String,
    pub event_parser_id: String,
    pub observed_runtime_verification: Vec<String>,
    pub can_start: bool,
    pub blocked_reason: Option<String>,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRuntimeAttestationV1 {
    pub schema_version: u64,
    pub run_id: String,
    pub provider_id: String,
    pub runtime: String,
    pub capability_fingerprint: String,
    pub manifest_digest: String,
    pub executable_closure_digest: String,
    pub effective_profile: ProviderProfileTupleV1,
    pub workspace_identity: String,
    pub managed_cwd_identity: String,
    pub event_parser_id: String,
    pub verified_runtime_evidence: Vec<String>,
    pub verified_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "classification", deny_unknown_fields)]
pub enum InternalProviderEventV1 {
    Handshake {
        #[serde(rename = "schemaVersion")]
        schema_version: u64,
        sequence: u64,
        #[serde(rename = "verifiedRuntimeEvidence")]
        verified_runtime_evidence: Vec<String>,
    },
    Public {
        #[serde(rename = "schemaVersion")]
        schema_version: u64,
        sequence: u64,
        kind: String,
        text: String,
    },
    Private {
        #[serde(rename = "schemaVersion")]
        schema_version: u64,
        sequence: u64,
        #[serde(rename = "privateKind")]
        private_kind: String,
    },
    Terminal {
        #[serde(rename = "schemaVersion")]
        schema_version: u64,
        sequence: u64,
        outcome: String,
    },
    AuthFailure {
        #[serde(rename = "schemaVersion")]
        schema_version: u64,
        sequence: u64,
        #[serde(rename = "authState")]
        auth_state: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentCapabilitiesRequestV1 {
    pub runtime: String,
    pub refresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentExecutableIdentityV1 {
    pub path_digest: String,
    pub file_identity_digest: String,
    pub cli_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentModelCapabilityV1 {
    pub model_selector: String,
    pub reasoning_efforts: Vec<String>,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentDiscoveryEvidenceV1 {
    pub source: String,
    pub verification_level: String,
    pub exact_cli_version: Option<String>,
    pub effective_model_evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentCapabilitySnapshotV1 {
    pub schema_version: u64,
    pub runtime: String,
    pub platform: String,
    pub installation_state: String,
    pub auth_state: String,
    pub executable: Option<AgentExecutableIdentityV1>,
    pub models: Vec<AgentModelCapabilityV1>,
    pub discovery: AgentDiscoveryEvidenceV1,
    pub can_start: bool,
    pub blocked_reason: Option<String>,
    /// SHA-256 of canonical snapshot fields except fingerprint and observedAt.
    pub fingerprint: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunStartRequestV1 {
    pub chat_id: String,
    pub role_id: String,
    pub input_message_ids: Vec<String>,
    pub expected_capability_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunContextV1 {
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub role_id: String,
    pub capability_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunBindingV1 {
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub run_id: String,
    pub role_id: String,
    pub capability_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunAcceptedV1 {
    pub schema_version: u64,
    pub run_id: String,
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub role_id: String,
    pub capability_fingerprint: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunRecordV1 {
    pub schema_version: u64,
    pub run_id: String,
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub selected_profile: AgentResolvedProfileV1,
    pub source: String,
    pub capability_fingerprint: String,
    pub state: String,
    pub stage: String,
    pub termination_outcome: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentResolvedProfileV1 {
    pub role_id: String,
    pub runtime: String,
    pub model_selector: String,
    pub reasoning_effort: Option<String>,
    pub permission: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentExecutionModeV1 {
    pub invocation: String,
    pub prompt_transport: String,
    pub follow_up: String,
    pub resume: String,
    pub active_stdin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentProcessIdentityV1 {
    pub pid: u64,
    pub start_identity_digest: String,
    pub executable_path_digest: String,
    pub observed_gone: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentLaunchBarrierEvidenceV1 {
    pub launcher_identity: AgentProcessIdentityV1,
    pub agent_identity: AgentProcessIdentityV1,
    pub tracked_processes: Vec<AgentProcessIdentityV1>,
    pub tree_identity_digest: String,
    pub launch_spec_digest: String,
    pub evidence_digest: String,
    pub tree_tracker_attached: bool,
    pub run_and_lock_persisted_before_release: bool,
    pub identity_persisted_before_release: bool,
    pub release_granted: bool,
    pub captured_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunStartTranscriptV1 {
    pub schema_version: u64,
    pub request: AgentRunStartRequestV1,
    pub capability: AgentCapabilitySnapshotV1,
    pub resolved_profile: AgentResolvedProfileV1,
    pub execution_mode: AgentExecutionModeV1,
    pub run_id: Option<String>,
    pub preflight_outcome: String,
    pub persisted_run_count: u64,
    pub spawn_count: u64,
    pub lock_count: u64,
    pub barrier: Option<AgentLaunchBarrierEvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentPublicEventV1 {
    pub schema_version: u64,
    pub event_id: String,
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub run_id: String,
    pub role_id: String,
    pub capability_fingerprint: String,
    pub kind: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunStateChangedV1 {
    pub schema_version: u64,
    pub event_id: String,
    pub run: AgentRunRecordV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentTerminationTranscriptV1 {
    pub schema_version: u64,
    pub run_id: String,
    pub outcome: String,
    pub processes_proven_gone: bool,
    pub poisoned: bool,
    pub writer_lock_state: String,
    pub chat_state: String,
    pub tracked_processes: Vec<AgentProcessIdentityV1>,
    pub evidence_digest: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunIdentityV1 {
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub run_id: String,
}

pub type AgentRunCancelRequestV1 = AgentRunIdentityV1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRecoveryRecordV1 {
    pub schema_version: u64,
    pub recovery_id: String,
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub run_id: String,
    pub source: String,
    pub reason: String,
    pub state: String,
    pub tracked_processes: Vec<AgentProcessIdentityV1>,
    pub poisoned: bool,
    pub revision: u64,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunCancelResultV1 {
    pub schema_version: u64,
    pub request: AgentRunCancelRequestV1,
    pub run: AgentRunRecordV1,
    pub termination: AgentTerminationTranscriptV1,
    pub recovery: Option<AgentRecoveryRecordV1>,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRunTerminationPersistenceV1 {
    pub schema_version: u64,
    pub run: AgentRunRecordV1,
    pub termination: AgentTerminationTranscriptV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRecoveryInspectRequestV1 {
    pub recovery_id: String,
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRecoveryResolveRequestV1 {
    pub recovery_id: String,
    pub chat_id: String,
    pub task_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub run_id: String,
    pub expected_revision: u64,
    pub resolution: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentRecoveryResolveResultV1 {
    pub schema_version: u64,
    pub before: AgentRecoveryRecordV1,
    pub request: AgentRecoveryResolveRequestV1,
    pub after: Option<AgentRecoveryRecordV1>,
    pub outcome: String,
}

pub fn validate_agent_adapter_payload_v1(
    validator: &str,
    value: &Value,
) -> Result<(), AgentAdapterContractError> {
    match validator {
        "capabilitiesRequest" => validate_capabilities_request(value).map(|_| ()),
        "capabilitySnapshot" => validate_capability_snapshot(value).map(|_| ()),
        "runStartRequest" => validate_run_start_request(value).map(|_| ()),
        "runAccepted" => validate_run_accepted(value).map(|_| ()),
        "runRecord" => validate_run_record(value).map(|_| ()),
        "runStartTranscript" => validate_run_start_transcript(value),
        "publicEvent" => validate_agent_public_event_v1(value).map(|_| ()),
        "runStateChanged" => validate_agent_run_state_changed_v1(value).map(|_| ()),
        "terminationTranscript" => validate_termination_transcript(value),
        "agentChatDetailPage" => validate_agent_chat_detail_page(value),
        "runCancelRequest" => validate_agent_run_cancel_request(value).map(|_| ()),
        "runCancelResult" => validate_agent_run_cancel_result(value),
        "runTerminationPersistence" => validate_agent_run_termination_persistence(value),
        "recoveryRecord" => validate_agent_recovery_record(value).map(|_| ()),
        "recoveryInspectRequest" => validate_agent_recovery_inspect_request(value).map(|_| ()),
        "recoveryResolveRequest" => validate_agent_recovery_resolve_request(value).map(|_| ()),
        "recoveryResolveResult" => validate_agent_recovery_resolve_result(value),
        other => fail(format!("unknown agent adapter payload validator: {other}")),
    }
}

pub fn validate_provider_manifest_v1(
    value: &Value,
) -> Result<ProviderManifestV1, AgentAdapterContractError> {
    let manifest: ProviderManifestV1 = from_value(value, "provider manifest")?;
    schema(manifest.schema_version)?;
    one_of(&manifest.provider_id, G5C_PROVIDER_IDS_V1, "providerId")?;
    one_of(&manifest.runtime, G5_RUNTIMES_V1, "runtime")?;
    if manifest.provider_id != manifest.runtime {
        return fail("providerId and runtime must match exactly");
    }
    bounded(&manifest.manifest_version, "manifestVersion", 64)?;
    validate_provider_executable_closure_v1(&manifest.executable_closure)?;
    validate_provider_profiles_v1(&manifest.exact_profiles, "exactProfiles", false)?;
    let (auth_parser, event_parser) = provider_parser_ids_v1(&manifest.provider_id);
    bounded(&manifest.auth_parser_id, "authParserId", 128)?;
    bounded(&manifest.event_parser_id, "eventParserId", 128)?;
    if manifest.auth_parser_id != auth_parser || manifest.event_parser_id != event_parser {
        return fail("provider parser IDs must match the exact provider contract");
    }
    validate_provider_launch_policy_v1(&manifest.launch_policy, &manifest.provider_id)?;
    fingerprint_digest(&manifest.manifest_digest, "manifestDigest")?;
    if manifest.manifest_digest != compute_provider_manifest_digest_v1(&manifest)? {
        return fail("manifestDigest must bind the canonical provider manifest");
    }
    Ok(manifest)
}

/// UTF-8 compact JSON with recursive ASCII key ordering and manifestDigest excluded.
pub fn canonical_provider_manifest_v1(
    manifest: &ProviderManifestV1,
) -> Result<String, AgentAdapterContractError> {
    let mut value = serde_json::to_value(manifest).map_err(contract_json)?;
    value
        .as_object_mut()
        .ok_or_else(|| {
            AgentAdapterContractError::new("provider manifest must serialize as object")
        })?
        .remove("manifestDigest");
    serde_json::to_string(&canonicalize_fingerprint_value(value)).map_err(contract_json)
}

pub fn compute_provider_manifest_digest_v1(
    manifest: &ProviderManifestV1,
) -> Result<String, AgentAdapterContractError> {
    Ok(crate::project_v1::sha256_file_version_v1(
        canonical_provider_manifest_v1(manifest)?.as_bytes(),
    ))
}

/// UTF-8 compact JSON with recursive ASCII key ordering and closureDigest excluded.
pub fn canonical_provider_executable_closure_v1(
    closure: &ProviderExecutableClosureV1,
) -> Result<String, AgentAdapterContractError> {
    let mut value = serde_json::to_value(closure).map_err(contract_json)?;
    value
        .as_object_mut()
        .ok_or_else(|| AgentAdapterContractError::new("provider closure must serialize as object"))?
        .remove("closureDigest");
    serde_json::to_string(&canonicalize_fingerprint_value(value)).map_err(contract_json)
}

pub fn compute_provider_executable_closure_digest_v1(
    closure: &ProviderExecutableClosureV1,
) -> Result<String, AgentAdapterContractError> {
    Ok(crate::project_v1::sha256_file_version_v1(
        canonical_provider_executable_closure_v1(closure)?.as_bytes(),
    ))
}

pub fn validate_sanitized_discovery_observation_v1(
    value: &Value,
    manifest_value: &Value,
) -> Result<SanitizedDiscoveryObservationV1, AgentAdapterContractError> {
    let manifest = validate_provider_manifest_v1(manifest_value)?;
    let observation: SanitizedDiscoveryObservationV1 =
        from_value(value, "sanitized discovery observation")?;
    schema(observation.schema_version)?;
    if observation.provider_id != manifest.provider_id || observation.runtime != manifest.runtime {
        return fail("discovery provider identity must match the manifest");
    }
    fingerprint_digest(&observation.manifest_digest, "manifestDigest")?;
    if observation.manifest_digest != manifest.manifest_digest {
        return fail("discovery manifestDigest mismatch");
    }
    one_of(
        &observation.installation_state,
        AGENT_INSTALLATION_STATES_V1,
        "installationState",
    )?;
    one_of(&observation.auth_state, AGENT_AUTH_STATES_V1, "authState")?;
    if (observation.installation_state == "installed") != observation.executable_closure.is_some() {
        return fail(
            "installed discovery must include one executable closure and other states must not",
        );
    }
    if let Some(closure) = &observation.executable_closure {
        validate_provider_executable_closure_v1(closure)?;
    }
    if observation.executable_closure.is_none() != observation.exact_cli_version.is_none() {
        return fail("exactCliVersion and executable closure must be observed together");
    }
    if let (Some(closure), Some(version)) = (
        observation.executable_closure.as_ref(),
        observation.exact_cli_version.as_deref(),
    ) {
        bounded(version, "exactCliVersion", 128)?;
        if version != closure.binary.cli_version {
            return fail("exactCliVersion must match the observed binary");
        }
    }
    one_of(
        &observation.model_catalog_evidence,
        G5C_MODEL_CATALOG_EVIDENCE_V1,
        "modelCatalogEvidence",
    )?;
    validate_provider_profiles_v1(&observation.exact_profiles, "exactProfiles", true)?;
    if observation.auth_parser_id != manifest.auth_parser_id
        || observation.event_parser_id != manifest.event_parser_id
    {
        return fail("discovery parser IDs must match the manifest without fallback");
    }
    validate_runtime_verification_list_v1(
        &observation.observed_runtime_verification,
        "observedRuntimeVerification",
        true,
    )?;
    if observation
        .observed_runtime_verification
        .iter()
        .any(|evidence| {
            !manifest
                .launch_policy
                .required_runtime_verification
                .contains(evidence)
        })
    {
        return fail(
            "discovery runtime verification must be an exact subset of manifest requirements",
        );
    }
    if let Some(reason) = &observation.blocked_reason {
        one_of(reason, G5C_DISCOVERY_BLOCK_REASONS_V1, "blockedReason")?;
    }
    timestamp(&observation.observed_at, "observedAt")?;
    let expected = provider_discovery_blocker_v1(&observation, &manifest);
    if observation.can_start != expected.is_none()
        || observation.blocked_reason.as_deref() != expected
    {
        return fail("canStart and blockedReason must match exact sanitized discovery evidence");
    }
    Ok(observation)
}

pub fn validate_agent_runtime_attestation_v1(
    value: &Value,
    manifest_value: &Value,
    observation_value: &Value,
) -> Result<AgentRuntimeAttestationV1, AgentAdapterContractError> {
    let manifest = validate_provider_manifest_v1(manifest_value)?;
    let observation =
        validate_sanitized_discovery_observation_v1(observation_value, manifest_value)?;
    let closure = observation.executable_closure.as_ref().ok_or_else(|| {
        AgentAdapterContractError::new("runtime attestation requires executable closure")
    })?;
    if !observation.can_start {
        return fail("runtime attestation requires a startable sanitized observation");
    }
    let attestation: AgentRuntimeAttestationV1 = from_value(value, "agent runtime attestation")?;
    schema(attestation.schema_version)?;
    agent_run_id(&attestation.run_id, "runId")?;
    if attestation.provider_id != manifest.provider_id || attestation.runtime != manifest.runtime {
        return fail("runtime attestation provider identity must match the manifest");
    }
    fingerprint_digest(&attestation.capability_fingerprint, "capabilityFingerprint")?;
    fingerprint_digest(&attestation.manifest_digest, "manifestDigest")?;
    fingerprint_digest(
        &attestation.executable_closure_digest,
        "executableClosureDigest",
    )?;
    if attestation.manifest_digest != manifest.manifest_digest {
        return fail("runtime attestation manifestDigest mismatch");
    }
    if attestation.executable_closure_digest != closure.closure_digest {
        return fail("runtime attestation executable closure mismatch");
    }
    validate_provider_profile_tuple_v1(&attestation.effective_profile, "effectiveProfile")?;
    if !manifest
        .exact_profiles
        .contains(&attestation.effective_profile)
    {
        return fail("runtime attestation effective profile is not an exact supported tuple");
    }
    id(&attestation.workspace_identity, "workspaceIdentity")?;
    fingerprint_digest(&attestation.managed_cwd_identity, "managedCwdIdentity")?;
    if attestation.event_parser_id != manifest.event_parser_id {
        return fail("runtime attestation parser mismatch");
    }
    validate_runtime_verification_list_v1(
        &attestation.verified_runtime_evidence,
        "verifiedRuntimeEvidence",
        false,
    )?;
    if attestation.verified_runtime_evidence != manifest.launch_policy.required_runtime_verification
    {
        return fail(
            "runtime attestation must bind every required verification in canonical order",
        );
    }
    timestamp(&attestation.verified_at, "verifiedAt")?;
    Ok(attestation)
}

pub fn validate_agent_runtime_attestation_against_accepted_v1(
    value: &Value,
    manifest_value: &Value,
    observation_value: &Value,
    accepted_value: &Value,
    profile_value: &Value,
) -> Result<AgentRuntimeAttestationV1, AgentAdapterContractError> {
    let attestation =
        validate_agent_runtime_attestation_v1(value, manifest_value, observation_value)?;
    let accepted = validate_run_accepted(accepted_value)?;
    let profile: AgentResolvedProfileV1 = from_value(profile_value, "resolved provider profile")?;
    validate_profile(&profile)?;
    if accepted.role_id != profile.role_id {
        return fail("accepted run and resolved profile role mismatch");
    }
    if attestation.run_id != accepted.run_id {
        return fail("runtime attestation changed accepted runId");
    }
    if attestation.capability_fingerprint != accepted.capability_fingerprint {
        return fail("runtime attestation changed accepted capabilityFingerprint");
    }
    if attestation.workspace_identity != accepted.workspace_identity {
        return fail("runtime attestation changed accepted workspaceIdentity");
    }
    if attestation.runtime != profile.runtime
        || attestation.effective_profile.model_selector != profile.model_selector
        || attestation.effective_profile.reasoning_effort != profile.reasoning_effort
        || attestation.effective_profile.permission != profile.permission
    {
        return fail("runtime attestation changed the exact resolved provider profile");
    }
    Ok(attestation)
}

pub fn validate_internal_provider_event_sequence_v1(
    value: &Value,
    manifest_value: &Value,
) -> Result<Vec<InternalProviderEventV1>, AgentAdapterContractError> {
    let manifest = validate_provider_manifest_v1(manifest_value)?;
    let entries = value.as_array().ok_or_else(|| {
        AgentAdapterContractError::new("internal provider events must be a bounded array")
    })?;
    if entries.is_empty() || entries.len() > G5C_INTERNAL_EVENTS_PER_RUN_MAX_V1 {
        return fail("internal provider events must be nonempty and bounded");
    }
    let mut handshake_seen = false;
    let mut final_seen = false;
    let mut terminal_seen = false;
    let mut public_event_count = 0_usize;
    let mut public_text_bytes = 0_usize;
    let mut validated = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let object = entry.as_object().ok_or_else(|| {
            AgentAdapterContractError::new("internal provider event must be an object")
        })?;
        let classification = object
            .get("classification")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentAdapterContractError::new("invalid classification"))?;
        one_of(
            classification,
            G5C_PROVIDER_EVENT_CLASSIFICATIONS_V1,
            "classification",
        )?;
        let keys: &[&str] = match classification {
            "Handshake" => &[
                "schemaVersion",
                "sequence",
                "classification",
                "verifiedRuntimeEvidence",
            ],
            "Public" => &[
                "schemaVersion",
                "sequence",
                "classification",
                "kind",
                "text",
            ],
            "Private" => &["schemaVersion", "sequence", "classification", "privateKind"],
            "Terminal" => &["schemaVersion", "sequence", "classification", "outcome"],
            _ => &["schemaVersion", "sequence", "classification", "authState"],
        };
        exact_value_keys(object, keys, "internal provider event")?;
        schema(
            object
                .get("schemaVersion")
                .and_then(Value::as_u64)
                .ok_or_else(|| AgentAdapterContractError::new("schemaVersion must be 1"))?,
        )?;
        let sequence = object
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                AgentAdapterContractError::new("provider event sequence must be positive")
            })?;
        if sequence != (index + 1) as u64 {
            return fail("internal provider event sequence must be contiguous from one");
        }
        if terminal_seen {
            return fail("terminal or auth failure must be the last provider event");
        }
        match classification {
            "Handshake" => {
                let evidence = value_string_array(object, "verifiedRuntimeEvidence")?;
                validate_runtime_verification_list_v1(&evidence, "verifiedRuntimeEvidence", false)?;
                if index != 0
                    || handshake_seen
                    || evidence != manifest.launch_policy.required_runtime_verification
                {
                    return fail("one exact provider handshake must be the first event");
                }
                handshake_seen = true;
            }
            "AuthFailure" => {
                let auth_state = object
                    .get("authState")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AgentAdapterContractError::new("invalid auth failure state"))?;
                one_of(
                    auth_state,
                    &["notAuthenticated", "expired"],
                    "auth failure state",
                )?;
                if index != 0 || entries.len() != 1 || handshake_seen {
                    return fail("pre-handshake auth failure must be the sole terminal event");
                }
                terminal_seen = true;
            }
            "Public" => {
                if !handshake_seen {
                    return fail("provider handshake must precede public events");
                }
                let kind = object
                    .get("kind")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AgentAdapterContractError::new("invalid public kind"))?;
                one_of(kind, AGENT_PUBLIC_EVENT_KINDS_V1, "public kind")?;
                let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                    AgentAdapterContractError::new("public text must be a string")
                })?;
                bounded(text, "public text", G5_PUBLIC_MESSAGE_MAX_BYTES_V1)?;
                public_event_count += 1;
                public_text_bytes = public_text_bytes
                    .checked_add(text.len())
                    .ok_or_else(|| AgentAdapterContractError::new("public text bytes overflow"))?;
                if public_event_count > crate::AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1 {
                    return fail("internal provider public event count exceeds the G5B run budget");
                }
                if public_text_bytes > crate::AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1 {
                    return fail("internal provider public text exceeds the G5B run budget");
                }
                if final_seen {
                    return fail("no public event may follow final");
                }
                final_seen = kind == "final";
            }
            "Private" => {
                if !handshake_seen {
                    return fail("provider handshake must precede private events");
                }
                let private_kind = object
                    .get("privateKind")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AgentAdapterContractError::new("invalid privateKind"))?;
                one_of(
                    private_kind,
                    &["reasoning", "toolCall", "toolResult", "transport"],
                    "privateKind",
                )?;
            }
            "Terminal" => {
                if !handshake_seen {
                    return fail("provider handshake must precede terminal events");
                }
                let outcome = object
                    .get("outcome")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AgentAdapterContractError::new("invalid terminal outcome"))?;
                one_of(
                    outcome,
                    &["succeeded", "failed", "canceled", "recoveryRequired"],
                    "terminal outcome",
                )?;
                if index + 1 != entries.len() {
                    return fail("terminal provider event must be last");
                }
                terminal_seen = true;
            }
            _ => unreachable!("classification checked"),
        }
        validated.push(from_value(entry, "internal provider event")?);
    }
    Ok(validated)
}

pub fn assert_g5a_contract_v1() -> Result<(), AgentAdapterContractError> {
    if ![
        "agent.getCapabilities",
        "run.startAgent",
        "run.cancelAgent",
        "recovery.inspectAgent",
        "recovery.resolveAgent",
    ]
    .iter()
    .all(|method| G5A_CAPABILITIES_V1.contains(method))
    {
        return fail("G5A actual-agent capabilities are missing");
    }
    if !G5A_CAPABILITIES_V1
        .iter()
        .all(|capability| crate::METHODS_V1.contains(capability))
    {
        return fail("G5A capability is not reserved by protocol v1");
    }
    if G5A_CAPABILITIES_V1.contains(&"run.startAgentSmoke") {
        return fail("agent smoke must remain unadvertised");
    }
    if G5A_CAPABILITIES_V1[..G5_CAPABILITIES_V1.len()] != *G5_CAPABILITIES_V1 {
        return fail("G5A capabilities must preserve the exact G5 prefix");
    }
    let unique: HashSet<_> = G5A_CAPABILITIES_V1.iter().collect();
    if unique.len() != G5A_CAPABILITIES_V1.len() {
        return fail("G5A capabilities must be unique");
    }
    Ok(())
}

pub fn validate_planned_g5a_ready_payload_v1(
    value: &Value,
) -> Result<(), AgentAdapterContractError> {
    let ready: crate::ReadyPayloadV1 = from_value(value, "G5A ready payload")?;
    if ready.protocol_version != crate::PROTOCOL_VERSION
        || ready.sidecar_schema_version != crate::SIDECAR_SCHEMA_VERSION
        || ready.app_schema_version != crate::APP_SCHEMA_VERSION
        || ready.pid == 0
        || !ready
            .capabilities
            .iter()
            .map(String::as_str)
            .eq(G5A_CAPABILITIES_V1.iter().copied())
    {
        return fail("G5A ready payload does not match the exact planned contract");
    }
    bounded(&ready.platform, "platform", 64)?;
    id(&ready.sidecar_instance_id, "sidecarInstanceId")
}

fn validate_capabilities_request(
    value: &Value,
) -> Result<AgentCapabilitiesRequestV1, AgentAdapterContractError> {
    let request: AgentCapabilitiesRequestV1 = from_value(value, "agent capabilities request")?;
    one_of(&request.runtime, G5_RUNTIMES_V1, "runtime")?;
    Ok(request)
}

fn validate_capability_snapshot(
    value: &Value,
) -> Result<AgentCapabilitySnapshotV1, AgentAdapterContractError> {
    let snapshot: AgentCapabilitySnapshotV1 = from_value(value, "agent capability snapshot")?;
    schema(snapshot.schema_version)?;
    one_of(&snapshot.runtime, G5_RUNTIMES_V1, "runtime")?;
    one_of(&snapshot.platform, &["darwin", "win32"], "platform")?;
    one_of(
        &snapshot.installation_state,
        AGENT_INSTALLATION_STATES_V1,
        "installationState",
    )?;
    one_of(&snapshot.auth_state, AGENT_AUTH_STATES_V1, "authState")?;
    if (snapshot.installation_state == "installed") != snapshot.executable.is_some() {
        return fail("installed state and executable identity must agree");
    }
    if let Some(executable) = &snapshot.executable {
        validate_executable(executable)?;
    }
    if snapshot.models.len() > 128 {
        return fail("models must be bounded");
    }
    let mut selectors = HashSet::new();
    for model in &snapshot.models {
        validate_model(model)?;
        if !selectors.insert(model.model_selector.as_str()) {
            return fail("model selectors must be unique");
        }
    }
    validate_discovery(&snapshot.discovery)?;
    if snapshot.discovery.source == "unavailable"
        && (!snapshot.models.is_empty()
            || snapshot.discovery.exact_cli_version.is_some()
            || snapshot.discovery.verification_level != "unknown"
            || snapshot.discovery.effective_model_evidence != "unknown")
    {
        return fail("unavailable discovery cannot claim model or version evidence");
    }
    if let (Some(exact), Some(executable)) = (
        snapshot.discovery.exact_cli_version.as_deref(),
        snapshot.executable.as_ref(),
    ) {
        if exact != executable.cli_version
            && snapshot.blocked_reason.as_deref() != Some("unsupportedVersion")
        {
            return fail("CLI version mismatch must be unsupportedVersion");
        }
    }
    if let Some(reason) = &snapshot.blocked_reason {
        one_of(reason, AGENT_CAPABILITY_BLOCK_REASONS_V1, "blockedReason")?;
    }
    fingerprint_digest(&snapshot.fingerprint, "fingerprint")?;
    timestamp(&snapshot.observed_at, "observedAt")?;
    let expected = capability_blocker(&snapshot);
    if snapshot.can_start != expected.is_none() || snapshot.blocked_reason.as_deref() != expected {
        return fail("canStart and blockedReason must match verified capability evidence");
    }
    if snapshot.can_start && snapshot.models.is_empty() {
        return fail("startable capability must have an exact model set");
    }
    if snapshot.fingerprint != compute_agent_capability_fingerprint_v1(&snapshot)? {
        return fail("fingerprint must bind the canonical capability snapshot");
    }
    Ok(snapshot)
}

pub fn canonical_agent_capability_snapshot_v1(
    snapshot: &AgentCapabilitySnapshotV1,
) -> Result<String, AgentAdapterContractError> {
    let mut value = serde_json::to_value(snapshot).map_err(contract_json)?;
    let object = value.as_object_mut().ok_or_else(|| {
        AgentAdapterContractError::new("capability snapshot must serialize as object")
    })?;
    object.remove("fingerprint");
    object.remove("observedAt");
    serde_json::to_string(&canonicalize_fingerprint_value(value)).map_err(contract_json)
}

pub fn compute_agent_capability_fingerprint_v1(
    snapshot: &AgentCapabilitySnapshotV1,
) -> Result<String, AgentAdapterContractError> {
    let canonical = canonical_agent_capability_snapshot_v1(snapshot)?;
    Ok(crate::project_v1::sha256_file_version_v1(
        canonical.as_bytes(),
    ))
}

fn validate_run_start_request(
    value: &Value,
) -> Result<AgentRunStartRequestV1, AgentAdapterContractError> {
    let request: AgentRunStartRequestV1 = from_value(value, "agent run start request")?;
    id(&request.chat_id, "chatId")?;
    one_of(&request.role_id, G5_ROLE_IDS_V1, "roleId")?;
    if request.input_message_ids.is_empty() || request.input_message_ids.len() > 100 {
        return fail("inputMessageIds must be nonempty and bounded");
    }
    let mut unique = HashSet::new();
    for message_id in &request.input_message_ids {
        id(message_id, "inputMessageId")?;
        if !unique.insert(message_id.as_str()) {
            return fail("inputMessageIds must be unique");
        }
    }
    fingerprint_digest(
        &request.expected_capability_fingerprint,
        "expectedCapabilityFingerprint",
    )?;
    Ok(request)
}

fn validate_run_accepted(value: &Value) -> Result<AgentRunAcceptedV1, AgentAdapterContractError> {
    let accepted: AgentRunAcceptedV1 = from_value(value, "agent run accepted result")?;
    schema(accepted.schema_version)?;
    validate_run_context(&context_from_accepted(&accepted))?;
    agent_run_id(&accepted.run_id, "runId")?;
    if accepted.state != "preparing" {
        return fail("accepted agent run state must be preparing");
    }
    Ok(accepted)
}

pub fn validate_agent_run_accepted_against_context_v1(
    value: &Value,
    expected: &AgentRunContextV1,
) -> Result<AgentRunAcceptedV1, AgentAdapterContractError> {
    validate_run_context(expected)?;
    let accepted = validate_run_accepted(value)?;
    assert_same_run_context(
        expected,
        &context_from_accepted(&accepted),
        "accepted agent run",
    )?;
    Ok(accepted)
}

fn validate_run_record(value: &Value) -> Result<AgentRunRecordV1, AgentAdapterContractError> {
    let record: AgentRunRecordV1 = from_value(value, "agent run record")?;
    schema(record.schema_version)?;
    for (value, name) in [
        (&record.run_id, "runId"),
        (&record.chat_id, "chatId"),
        (&record.task_id, "taskId"),
        (&record.project_id, "projectId"),
        (&record.worktree_id, "worktreeId"),
        (&record.workspace_identity, "workspaceIdentity"),
    ] {
        if name == "runId" {
            agent_run_id(value, name)?;
        } else {
            id(value, name)?;
        }
    }
    validate_profile(&record.selected_profile)?;
    if record.source != "actualAgent" {
        return fail("agent run source must be actualAgent");
    }
    fingerprint_digest(&record.capability_fingerprint, "capabilityFingerprint")?;
    one_of(
        &record.state,
        crate::run_recovery_v1::G5_RUN_STATES_V1,
        "state",
    )?;
    one_of(
        &record.stage,
        crate::run_recovery_v1::G5_RUN_STAGES_V1,
        "stage",
    )?;
    let expected_stage = match record.selected_profile.role_id.as_str() {
        "planning" => "planning",
        "coding" => "implementing",
        "review" => "verifying",
        _ => return fail("invalid roleId"),
    };
    if record.stage != expected_stage {
        return fail("agent run stage does not match selected role snapshot");
    }
    if let Some(outcome) = &record.termination_outcome {
        one_of(
            outcome,
            &["cleanExit", "recoveryRequired"],
            "terminationOutcome",
        )?;
    }
    timestamp(&record.created_at, "createdAt")?;
    timestamp(&record.updated_at, "updatedAt")?;
    let active = matches!(
        record.state.as_str(),
        "preparing" | "running" | "cancelling"
    );
    if active != record.termination_outcome.is_none() {
        return fail("active and terminal run evidence must agree");
    }
    if (record.state == "recoveryRequired")
        != (record.termination_outcome.as_deref() == Some("recoveryRequired"))
    {
        return fail("recoveryRequired run must keep recovery termination evidence");
    }
    Ok(record)
}

fn validate_run_start_transcript(value: &Value) -> Result<(), AgentAdapterContractError> {
    let transcript: AgentRunStartTranscriptV1 = from_value(value, "agent run start transcript")?;
    schema(transcript.schema_version)?;
    validate_run_start_request(&serde_json::to_value(&transcript.request).map_err(contract_json)?)?;
    let capability = validate_capability_snapshot(
        &serde_json::to_value(&transcript.capability).map_err(contract_json)?,
    )?;
    validate_profile(&transcript.resolved_profile)?;
    validate_execution_mode(&transcript.execution_mode)?;
    if let Some(run_id) = &transcript.run_id {
        agent_run_id(run_id, "runId")?;
    }
    one_of(
        &transcript.preflight_outcome,
        AGENT_START_OUTCOMES_V1,
        "preflightOutcome",
    )?;
    if transcript.persisted_run_count > 1 || transcript.spawn_count > 1 || transcript.lock_count > 1
    {
        return fail("run, spawn, and lock counts must be zero or one");
    }
    let mut expected = capability
        .blocked_reason
        .clone()
        .unwrap_or_else(|| "started".to_string());
    if expected == "started"
        && transcript.request.expected_capability_fingerprint != capability.fingerprint
    {
        expected = "profileChanged".to_string();
    }
    if expected == "started"
        && (transcript.request.role_id != transcript.resolved_profile.role_id
            || transcript.resolved_profile.runtime != capability.runtime)
    {
        expected = "profileChanged".to_string();
    }
    let model = capability
        .models
        .iter()
        .find(|model| model.model_selector == transcript.resolved_profile.model_selector);
    if expected == "started" && model.is_none() {
        expected = "exactModelUnavailable".to_string();
    }
    if expected == "started"
        && transcript
            .resolved_profile
            .reasoning_effort
            .as_ref()
            .is_some_and(|effort| {
                !model
                    .expect("checked model")
                    .reasoning_efforts
                    .contains(effort)
            })
    {
        expected = "reasoningUnsupported".to_string();
    }
    if expected == "started"
        && !model
            .expect("checked model")
            .permissions
            .contains(&transcript.resolved_profile.permission)
    {
        expected = "permissionUnsupported".to_string();
    }
    if transcript.preflight_outcome != expected {
        return fail("preflightOutcome must match exact capability and profile evidence");
    }
    if expected == "started" {
        let expected_locks = u64::from(transcript.resolved_profile.permission == "workspaceWrite");
        if transcript.run_id.is_none()
            || transcript.persisted_run_count != 1
            || transcript.spawn_count != 1
            || transcript.lock_count != expected_locks
            || transcript.barrier.is_none()
        {
            return fail("started run requires persisted run, spawn, exact lock, and barrier");
        }
        validate_barrier(transcript.barrier.as_ref().expect("checked barrier"))?;
    } else if transcript.run_id.is_some()
        || transcript.persisted_run_count != 0
        || transcript.spawn_count != 0
        || transcript.lock_count != 0
        || transcript.barrier.is_some()
    {
        return fail("preflight rejection must create no run, process, lock, or barrier evidence");
    }
    Ok(())
}

pub fn validate_agent_public_event_v1(
    value: &Value,
) -> Result<AgentPublicEventV1, AgentAdapterContractError> {
    let event: AgentPublicEventV1 = from_value(value, "agent public event")?;
    schema(event.schema_version)?;
    agent_event_id(&event.event_id, "eventId")?;
    validate_run_binding(&binding_from_event(&event))?;
    one_of(&event.kind, AGENT_PUBLIC_EVENT_KINDS_V1, "kind")?;
    bounded(&event.text, "text", G5_PUBLIC_MESSAGE_MAX_BYTES_V1)?;
    timestamp(&event.created_at, "createdAt")?;
    Ok(event)
}

/// Canonical UTF-8 bytes of the prospective full public_events array snapshot.
pub fn agent_public_events_snapshot_bytes_v1(
    values: &[Value],
) -> Result<usize, AgentAdapterContractError> {
    canonical_public_events_snapshot_bytes_v1(&validate_public_ledger_events_v1(values)?)
}

/// Sum of the full public_events array snapshot bytes persisted after each append.
pub fn agent_public_ledger_cumulative_snapshot_bytes_v1(
    values: &[Value],
) -> Result<usize, AgentAdapterContractError> {
    let events = validate_public_ledger_events_v1(values)?;
    (1..=events.len()).try_fold(0_usize, |cumulative, length| {
        let snapshot = canonical_public_events_snapshot_bytes_v1(&events[..length])?;
        cumulative.checked_add(snapshot).ok_or_else(|| {
            AgentAdapterContractError::new("public event ledger cumulative bytes overflow")
        })
    })
}

pub fn validate_agent_public_event_against_run_v1(
    value: &Value,
    expected: &AgentRunBindingV1,
) -> Result<AgentPublicEventV1, AgentAdapterContractError> {
    validate_run_binding(expected)?;
    let event = validate_agent_public_event_v1(value)?;
    assert_same_run_binding(expected, &binding_from_event(&event), "agent public event")?;
    Ok(event)
}

pub fn validate_agent_run_state_changed_v1(
    value: &Value,
) -> Result<AgentRunStateChangedV1, AgentAdapterContractError> {
    let event: AgentRunStateChangedV1 = from_value(value, "agent run state event")?;
    schema(event.schema_version)?;
    agent_state_event_id(&event.event_id, "eventId")?;
    let run = validate_run_record(&serde_json::to_value(&event.run).map_err(contract_json)?)?;
    one_of(
        &run.state,
        AGENT_RUN_STATE_EVENT_STATES_V1,
        "wire run state",
    )?;
    Ok(event)
}

pub fn validate_agent_run_state_changed_against_accepted_v1(
    value: &Value,
    expected: &AgentRunAcceptedV1,
) -> Result<AgentRunStateChangedV1, AgentAdapterContractError> {
    let expected = validate_run_accepted(&serde_json::to_value(expected).map_err(contract_json)?)?;
    let event = validate_agent_run_state_changed_v1(value)?;
    assert_same_run_binding(
        &binding_from_accepted(&expected),
        &binding_from_run(&event.run),
        "agent run state event",
    )?;
    Ok(event)
}

fn validate_termination_transcript(value: &Value) -> Result<(), AgentAdapterContractError> {
    let transcript: AgentTerminationTranscriptV1 =
        from_value(value, "agent termination transcript")?;
    schema(transcript.schema_version)?;
    agent_run_id(&transcript.run_id, "runId")?;
    one_of(
        &transcript.outcome,
        &["cleanExit", "recoveryRequired"],
        "outcome",
    )?;
    one_of(
        &transcript.writer_lock_state,
        &["none", "released", "recoveryRequired"],
        "writerLockState",
    )?;
    one_of(
        &transcript.chat_state,
        &["succeeded", "failed", "canceled", "recoveryRequired"],
        "chatState",
    )?;
    if transcript.tracked_processes.is_empty() || transcript.tracked_processes.len() > 256 {
        return fail("trackedProcesses must be nonempty and bounded");
    }
    let mut identities = HashSet::new();
    for process in &transcript.tracked_processes {
        validate_process(process)?;
        if !identities.insert((process.pid, process.start_identity_digest.as_str())) {
            return fail("tracked process identities must be unique");
        }
    }
    digest(&transcript.evidence_digest, "evidenceDigest")?;
    timestamp(&transcript.observed_at, "observedAt")?;
    if transcript.outcome == "recoveryRequired" {
        if transcript.processes_proven_gone
            || !transcript.poisoned
            || transcript.writer_lock_state != "recoveryRequired"
            || transcript.chat_state != "recoveryRequired"
        {
            return fail("uncertain termination must remain poisoned and recoveryRequired");
        }
    } else if !transcript.processes_proven_gone
        || transcript
            .tracked_processes
            .iter()
            .any(|process| !process.observed_gone)
        || transcript.poisoned
        || transcript.writer_lock_state == "recoveryRequired"
        || transcript.chat_state == "recoveryRequired"
    {
        return fail(
            "clean termination must prove every tracked process gone and release any writer lock",
        );
    }
    Ok(())
}

fn validate_agent_chat_detail_page(value: &Value) -> Result<(), AgentAdapterContractError> {
    let object = value.as_object().ok_or_else(|| {
        AgentAdapterContractError::new("agent chat detail page must be an object")
    })?;
    exact_value_keys(
        object,
        &[
            "chat",
            "afterEventSequence",
            "messages",
            "runs",
            "nextEventSequence",
            "agentRuns",
            "agentRecovery",
        ],
        "agent chat detail page",
    )?;
    let mut base = object.clone();
    let agent_runs = base
        .remove("agentRuns")
        .and_then(|entry| entry.as_array().cloned())
        .ok_or_else(|| AgentAdapterContractError::new("agentRuns must be an array"))?;
    let agent_recovery = match base
        .remove("agentRecovery")
        .ok_or_else(|| AgentAdapterContractError::new("agentRecovery is required"))?
    {
        Value::Null => None,
        value => Some(validate_agent_recovery_record(&value)?),
    };
    if agent_runs.len() > G5_EVENT_PAGE_MAX_ITEMS_V1 {
        return fail("agentRuns must be bounded");
    }
    crate::run_recovery_v1::validate_g5_payload_v1("chatDetailPage", &Value::Object(base.clone()))
        .map_err(|error| AgentAdapterContractError::new(error.to_string()))?;
    let chat = base
        .get("chat")
        .and_then(Value::as_object)
        .ok_or_else(|| AgentAdapterContractError::new("chat detail chat must be an object"))?;
    let fixture_runs = base
        .get("runs")
        .and_then(Value::as_array)
        .ok_or_else(|| AgentAdapterContractError::new("fixture runs must be an array"))?;
    let mut fixture_ids = HashSet::new();
    for run in fixture_runs {
        let run_id = value_string(run, "runId", "fixture run")?;
        if run_id.starts_with("agent_run_") || !fixture_ids.insert(run_id.to_string()) {
            return fail("fixture run ledger cannot use actual-agent or duplicate run ids");
        }
    }
    let mut agent_ids = HashSet::new();
    let mut recovery_runs = Vec::new();
    for run in &agent_runs {
        let run = validate_run_record(run)?;
        for (expected, actual, name) in [
            (chat_string(chat, "chatId")?, run.chat_id.as_str(), "chatId"),
            (chat_string(chat, "taskId")?, run.task_id.as_str(), "taskId"),
            (
                chat_string(chat, "projectId")?,
                run.project_id.as_str(),
                "projectId",
            ),
            (
                chat_string(chat, "worktreeId")?,
                run.worktree_id.as_str(),
                "worktreeId",
            ),
            (
                chat_string(chat, "workspaceIdentity")?,
                run.workspace_identity.as_str(),
                "workspaceIdentity",
            ),
        ] {
            if expected != actual {
                return fail(format!("agent run changed exact chat/workspace {name}"));
            }
        }
        if !agent_ids.insert(run.run_id.clone()) || fixture_ids.contains(run.run_id.as_str()) {
            return fail("fixture and actual run ledgers must have distinct unique run ids");
        }
        if run.state == "recoveryRequired" {
            recovery_runs.push(run);
        }
    }
    let messages = base
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AgentAdapterContractError::new("messages must be an array"))?;
    for message in messages {
        let Some(run_id) = message.get("runId").and_then(Value::as_str) else {
            continue;
        };
        if fixture_ids.contains(run_id) == agent_ids.contains(run_id) {
            return fail("message runId must resolve to exactly one fixture or actual run ledger");
        }
    }
    match agent_recovery {
        None if !recovery_runs.is_empty() => {
            return fail("recoveryRequired agent run requires one discoverable recovery")
        }
        Some(_) if recovery_runs.len() != 1 => {
            return fail(
                "agent chat detail must expose exactly one recoveryRequired run for its recovery",
            )
        }
        Some(recovery) => {
            if recovery.state != "unresolved" || !recovery.poisoned {
                return fail("agent chat detail recovery must remain unresolved and poisoned");
            }
            let run = &recovery_runs[0];
            let identity = AgentRunIdentityV1 {
                chat_id: run.chat_id.clone(),
                task_id: run.task_id.clone(),
                project_id: run.project_id.clone(),
                worktree_id: run.worktree_id.clone(),
                workspace_identity: run.workspace_identity.clone(),
                run_id: run.run_id.clone(),
            };
            assert_same_recovery_identity(&identity, None, &recovery, "agent chat recovery")?;
        }
        None => {}
    }
    Ok(())
}

fn validate_agent_run_cancel_request(
    value: &Value,
) -> Result<AgentRunCancelRequestV1, AgentAdapterContractError> {
    let request: AgentRunCancelRequestV1 = from_value(value, "agent run cancel request")?;
    validate_agent_run_identity(&request)?;
    Ok(request)
}

fn validate_agent_recovery_record(
    value: &Value,
) -> Result<AgentRecoveryRecordV1, AgentAdapterContractError> {
    let recovery: AgentRecoveryRecordV1 = from_value(value, "agent recovery record")?;
    schema(recovery.schema_version)?;
    agent_recovery_id(&recovery.recovery_id, "recoveryId")?;
    validate_agent_run_identity(&AgentRunIdentityV1 {
        chat_id: recovery.chat_id.clone(),
        task_id: recovery.task_id.clone(),
        project_id: recovery.project_id.clone(),
        worktree_id: recovery.worktree_id.clone(),
        workspace_identity: recovery.workspace_identity.clone(),
        run_id: recovery.run_id.clone(),
    })?;
    if recovery.source != "actualAgent" {
        return fail("agent recovery source must be actualAgent");
    }
    one_of(
        &recovery.reason,
        &[
            "uncleanShutdown",
            "cancelUncertain",
            "processIdentityUncertain",
            "processIdentityUnavailable",
        ],
        "reason",
    )?;
    one_of(
        &recovery.state,
        &["unresolved", "resolvedAbandoned"],
        "state",
    )?;
    if recovery.tracked_processes.len() > 256
        || (recovery.reason == "processIdentityUnavailable")
            != recovery.tracked_processes.is_empty()
    {
        return fail("only processIdentityUnavailable recovery may omit process evidence");
    }
    validate_unique_processes(&recovery.tracked_processes)?;
    if recovery.poisoned != (recovery.state == "unresolved") || recovery.revision == 0 {
        return fail("only unresolved positive-revision recovery remains poisoned");
    }
    timestamp(&recovery.created_at, "createdAt")?;
    if recovery.state == "unresolved" {
        if recovery.resolved_at.is_some() {
            return fail("unresolved recovery cannot have resolvedAt");
        }
    } else {
        if recovery.reason == "processIdentityUnavailable" {
            return fail("processIdentityUnavailable recovery cannot be resolvedAbandoned");
        }
        timestamp(
            recovery.resolved_at.as_deref().ok_or_else(|| {
                AgentAdapterContractError::new("resolved recovery requires resolvedAt")
            })?,
            "resolvedAt",
        )?;
        if recovery
            .tracked_processes
            .iter()
            .any(|process| !process.observed_gone)
        {
            return fail("resolved recovery must prove every tracked process gone");
        }
    }
    Ok(recovery)
}

fn validate_agent_run_cancel_result(value: &Value) -> Result<(), AgentAdapterContractError> {
    let result: AgentRunCancelResultV1 = from_value(value, "agent run cancel result")?;
    schema(result.schema_version)?;
    validate_agent_run_identity(&result.request)?;
    validate_run_record(&serde_json::to_value(&result.run).map_err(contract_json)?)?;
    validate_termination_transcript(
        &serde_json::to_value(&result.termination).map_err(contract_json)?,
    )?;
    validate_agent_run_termination_persistence(
        &serde_json::to_value(AgentRunTerminationPersistenceV1 {
            schema_version: G5A_SCHEMA_VERSION_V1,
            run: result.run.clone(),
            termination: result.termination.clone(),
        })
        .map_err(contract_json)?,
    )?;
    let recovery = result
        .recovery
        .as_ref()
        .map(|entry| {
            validate_agent_recovery_record(&serde_json::to_value(entry).map_err(contract_json)?)
        })
        .transpose()?;
    one_of(
        &result.outcome,
        &["canceled", "recoveryRequired"],
        "outcome",
    )?;
    assert_same_record_identity(&result.request, &result.run, "cancel result run")?;
    if result.termination.run_id != result.request.run_id {
        return fail("cancel termination changed run identity");
    }
    if result.outcome == "canceled" {
        if result.run.state != "canceled"
            || result.run.termination_outcome.as_deref() != Some("cleanExit")
            || result.termination.outcome != "cleanExit"
            || recovery.is_some()
        {
            return fail("canceled outcome requires clean terminal run without recovery");
        }
    } else {
        let recovery = recovery.ok_or_else(|| {
            AgentAdapterContractError::new("recoveryRequired outcome must persist recovery")
        })?;
        if result.run.state != "recoveryRequired"
            || result.run.termination_outcome.as_deref() != Some("recoveryRequired")
            || result.termination.outcome != "recoveryRequired"
            || recovery.state != "unresolved"
        {
            return fail("recoveryRequired outcome must keep run and recovery poisoned");
        }
        assert_same_recovery_identity(&result.request, None, &recovery, "cancel recovery")?;
        if recovery.tracked_processes != result.termination.tracked_processes {
            return fail("cancel recovery must persist the exact termination process evidence");
        }
    }
    Ok(())
}

fn validate_agent_run_termination_persistence(
    value: &Value,
) -> Result<(), AgentAdapterContractError> {
    let persistence: AgentRunTerminationPersistenceV1 =
        from_value(value, "agent run termination persistence")?;
    schema(persistence.schema_version)?;
    let run = validate_run_record(&serde_json::to_value(&persistence.run).map_err(contract_json)?)?;
    validate_termination_transcript(
        &serde_json::to_value(&persistence.termination).map_err(contract_json)?,
    )?;
    if run.run_id != persistence.termination.run_id
        || run.termination_outcome.as_deref() != Some(persistence.termination.outcome.as_str())
    {
        return fail("persisted run and termination identity/outcome must agree");
    }
    if matches!(run.state.as_str(), "preparing" | "running" | "cancelling") {
        return fail("termination persistence requires a terminal run");
    }
    if persistence.termination.outcome == "cleanExit" {
        let expected_lock = if run.selected_profile.permission == "workspaceWrite" {
            "released"
        } else {
            "none"
        };
        if persistence.termination.writer_lock_state != expected_lock {
            return fail("clean termination writer lock state must match selected permission");
        }
    }
    Ok(())
}

fn validate_agent_recovery_inspect_request(
    value: &Value,
) -> Result<AgentRecoveryInspectRequestV1, AgentAdapterContractError> {
    let request: AgentRecoveryInspectRequestV1 =
        from_value(value, "agent recovery inspect request")?;
    agent_recovery_id(&request.recovery_id, "recoveryId")?;
    validate_inspect_run_identity(&request)?;
    Ok(request)
}

fn validate_agent_recovery_resolve_request(
    value: &Value,
) -> Result<AgentRecoveryResolveRequestV1, AgentAdapterContractError> {
    let request: AgentRecoveryResolveRequestV1 =
        from_value(value, "agent recovery resolve request")?;
    agent_recovery_id(&request.recovery_id, "recoveryId")?;
    validate_resolve_run_identity(&request)?;
    if request.expected_revision == 0 || request.resolution != "abandon" {
        return fail("agent recovery resolution requires positive revision and abandon");
    }
    Ok(request)
}

fn validate_agent_recovery_resolve_result(value: &Value) -> Result<(), AgentAdapterContractError> {
    let result: AgentRecoveryResolveResultV1 = from_value(value, "agent recovery resolve result")?;
    schema(result.schema_version)?;
    let before = validate_agent_recovery_record(
        &serde_json::to_value(&result.before).map_err(contract_json)?,
    )?;
    validate_agent_recovery_resolve_request(
        &serde_json::to_value(&result.request).map_err(contract_json)?,
    )?;
    let after = result
        .after
        .as_ref()
        .map(|entry| {
            validate_agent_recovery_record(&serde_json::to_value(entry).map_err(contract_json)?)
        })
        .transpose()?;
    one_of(
        &result.outcome,
        &[
            "resolvedAbandoned",
            "processesRemain",
            "revisionConflict",
            "identityUnavailable",
        ],
        "outcome",
    )?;
    assert_same_recovery_identity_from_resolve(&result.request, &before, "recovery before")?;
    if before.state != "unresolved" {
        return fail("only unresolved recovery can be resolved");
    }
    let all_gone = before
        .tracked_processes
        .iter()
        .all(|process| process.observed_gone);
    let expected = if result.request.expected_revision != before.revision {
        "revisionConflict"
    } else if before.reason == "processIdentityUnavailable" {
        "identityUnavailable"
    } else if !all_gone {
        "processesRemain"
    } else {
        "resolvedAbandoned"
    };
    if result.outcome != expected {
        return fail("recovery resolution outcome must match revision and process evidence");
    }
    if result.outcome == "resolvedAbandoned" {
        let after = after.ok_or_else(|| {
            AgentAdapterContractError::new("resolved recovery must persist an after record")
        })?;
        if after.state != "resolvedAbandoned" || after.revision != before.revision + 1 {
            return fail("resolved recovery must persist the next revision");
        }
        assert_same_recovery_records(&before, &after, "recovery after")?;
    } else if after.is_some() {
        return fail("failed recovery resolution cannot persist an after record");
    }
    Ok(())
}

fn validate_provider_executable_closure_v1(
    closure: &ProviderExecutableClosureV1,
) -> Result<(), AgentAdapterContractError> {
    validate_provider_executable_binary_identity_v1(&closure.binary)?;
    if closure.required_helpers.is_empty() || closure.required_helpers.len() > 16 {
        return fail("provider executable closure must include bounded required helpers");
    }
    let mut logical_names = HashSet::new();
    logical_names.insert(closure.binary.logical_name.as_str());
    for helper in &closure.required_helpers {
        validate_provider_executable_file_identity_v1(helper)?;
        if !logical_names.insert(helper.logical_name.as_str()) {
            return fail("executable closure logical names must be unique");
        }
    }
    fingerprint_digest(&closure.closure_digest, "closureDigest")?;
    if closure.closure_digest != compute_provider_executable_closure_digest_v1(closure)? {
        return fail("closureDigest must bind the canonical executable closure");
    }
    Ok(())
}

fn validate_provider_executable_binary_identity_v1(
    binary: &ProviderExecutableBinaryIdentityV1,
) -> Result<(), AgentAdapterContractError> {
    bounded(&binary.logical_name, "logicalName", 64)?;
    fingerprint_digest(&binary.path_digest, "pathDigest")?;
    fingerprint_digest(&binary.file_identity_digest, "fileIdentityDigest")?;
    bounded(&binary.cli_version, "cliVersion", 128)
}

fn validate_provider_executable_file_identity_v1(
    file: &ProviderExecutableFileIdentityV1,
) -> Result<(), AgentAdapterContractError> {
    bounded(&file.logical_name, "logicalName", 64)?;
    fingerprint_digest(&file.path_digest, "pathDigest")?;
    fingerprint_digest(&file.file_identity_digest, "fileIdentityDigest")
}

fn validate_provider_profiles_v1(
    profiles: &[ProviderProfileTupleV1],
    name: &str,
    allow_empty: bool,
) -> Result<(), AgentAdapterContractError> {
    if profiles.len() > 128 || (!allow_empty && profiles.is_empty()) {
        return fail(format!("{name} must be a bounded nonempty tuple set"));
    }
    let mut unique = HashSet::new();
    for profile in profiles {
        validate_provider_profile_tuple_v1(profile, "provider profile tuple")?;
        if !unique.insert((
            profile.model_selector.as_str(),
            profile.reasoning_effort.as_deref(),
            profile.permission.as_str(),
        )) {
            return fail(format!("{name} tuples must be unique"));
        }
    }
    Ok(())
}

fn validate_provider_profile_tuple_v1(
    profile: &ProviderProfileTupleV1,
    _name: &str,
) -> Result<(), AgentAdapterContractError> {
    bounded(
        &profile.model_selector,
        "modelSelector",
        G5_MODEL_SELECTOR_MAX_BYTES_V1,
    )?;
    if let Some(reasoning_effort) = &profile.reasoning_effort {
        bounded(
            reasoning_effort,
            "reasoningEffort",
            G5_REASONING_EFFORT_MAX_BYTES_V1,
        )?;
    }
    one_of(&profile.permission, G5_PERMISSIONS_V1, "permission")?;
    Ok(())
}

fn validate_provider_launch_policy_v1(
    policy: &ProviderLaunchPolicyV1,
    provider_id: &str,
) -> Result<(), AgentAdapterContractError> {
    if policy.invocation != "oneShot"
        || policy.prompt_transport != "serverResolvedMessages"
        || policy.cwd_policy != "managedWorkspaceExact"
        || policy.environment_policy != "allowlistedNoSecrets"
        || policy.fallback_policy != "forbidden"
    {
        return fail(
            "provider launch policy must keep one-shot managed-cwd no-secret no-fallback safety",
        );
    }
    validate_runtime_verification_list_v1(
        &policy.required_runtime_verification,
        "requiredRuntimeVerification",
        false,
    )?;
    let expected = if provider_id == "claude-code" {
        G5C_CLAUDE_RUNTIME_VERIFICATION_V1
    } else {
        G5C_CODEX_RUNTIME_VERIFICATION_V1
    };
    if !policy
        .required_runtime_verification
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        return fail("provider launch policy must use the exact provider verification set");
    }
    Ok(())
}

fn validate_runtime_verification_list_v1(
    values: &[String],
    name: &str,
    allow_empty: bool,
) -> Result<(), AgentAdapterContractError> {
    if values.len() > G5C_RUNTIME_VERIFICATION_IDS_V1.len() || (!allow_empty && values.is_empty()) {
        return fail(format!("{name} must be bounded and nonempty"));
    }
    let mut unique = HashSet::new();
    for value in values {
        one_of(value, G5C_RUNTIME_VERIFICATION_IDS_V1, name)?;
        if !unique.insert(value.as_str()) {
            return fail(format!("{name} must be unique"));
        }
    }
    Ok(())
}

fn provider_parser_ids_v1(provider_id: &str) -> (&'static str, &'static str) {
    if provider_id == "claude-code" {
        ("claude-code.auth-status.v1", "claude-code.stream-json.v1")
    } else {
        ("codex.auth-status.v1", "codex.jsonl.v1")
    }
}

fn provider_discovery_blocker_v1(
    observation: &SanitizedDiscoveryObservationV1,
    manifest: &ProviderManifestV1,
) -> Option<&'static str> {
    if observation.installation_state == "notInstalled" {
        return Some("notInstalled");
    }
    let Some(closure) = observation.executable_closure.as_ref() else {
        return Some("closureMismatch");
    };
    if observation.installation_state == "incomplete" {
        return Some("closureMismatch");
    }
    if observation.auth_state == "expired" {
        return Some("authExpired");
    }
    if observation.auth_state != "authenticated" {
        return Some("notAuthenticated");
    }
    if closure.closure_digest != manifest.executable_closure.closure_digest {
        return Some("closureMismatch");
    }
    if observation.exact_cli_version.as_deref()
        != Some(manifest.executable_closure.binary.cli_version.as_str())
    {
        return Some("unsupportedVersion");
    }
    if matches!(
        observation.model_catalog_evidence.as_str(),
        "argvOnly" | "unknown"
    ) {
        return Some("modelCatalogUnverified");
    }
    if observation.exact_profiles != manifest.exact_profiles {
        return Some("unsupportedProfile");
    }
    if observation.observed_runtime_verification
        != manifest.launch_policy.required_runtime_verification
    {
        return Some("runtimeVerificationIncomplete");
    }
    None
}

fn value_string_array(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, AgentAdapterContractError> {
    object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| AgentAdapterContractError::new(format!("{key} must be an array")))?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                AgentAdapterContractError::new(format!("{key} must contain strings"))
            })
        })
        .collect()
}

fn capability_blocker(snapshot: &AgentCapabilitySnapshotV1) -> Option<&'static str> {
    if snapshot.platform != "darwin" {
        return Some("platformUnsupported");
    }
    match snapshot.installation_state.as_str() {
        "notInstalled" => return Some("notInstalled"),
        "incomplete" => return Some("installationIncomplete"),
        _ => {}
    }
    let Some(executable) = snapshot.executable.as_ref() else {
        return Some("installationIncomplete");
    };
    match snapshot.auth_state.as_str() {
        "notAuthenticated" | "unknown" => return Some("notAuthenticated"),
        "expired" => return Some("authExpired"),
        _ => {}
    }
    let discovery = &snapshot.discovery;
    if discovery.source == "unavailable"
        || discovery.verification_level == "unknown"
        || discovery.effective_model_evidence == "unknown"
    {
        return Some("capabilityUnknown");
    }
    if discovery.exact_cli_version.is_none() {
        return Some("capabilityUnknown");
    }
    if discovery.exact_cli_version.as_deref() != Some(executable.cli_version.as_str()) {
        return Some("unsupportedVersion");
    }
    if discovery.effective_model_evidence == "argvOnly" {
        return Some("exactModelUnavailable");
    }
    if discovery.source == "machineReadableCatalog"
        && (discovery.verification_level != "runtimeVerified"
            || discovery.effective_model_evidence != "runtimeVerified")
    {
        return Some("capabilityUnknown");
    }
    if discovery.source == "exactVersionManifest"
        && (discovery.verification_level != "measuredExactVersion"
            || discovery.effective_model_evidence != "measuredExactVersion")
    {
        return Some("capabilityUnknown");
    }
    snapshot
        .models
        .is_empty()
        .then_some("exactModelUnavailable")
}

fn validate_executable(value: &AgentExecutableIdentityV1) -> Result<(), AgentAdapterContractError> {
    digest(&value.path_digest, "pathDigest")?;
    digest(&value.file_identity_digest, "fileIdentityDigest")?;
    bounded(&value.cli_version, "cliVersion", 128)
}

fn validate_model(value: &AgentModelCapabilityV1) -> Result<(), AgentAdapterContractError> {
    bounded(
        &value.model_selector,
        "modelSelector",
        G5_MODEL_SELECTOR_MAX_BYTES_V1,
    )?;
    if value.reasoning_efforts.len() > 32 || value.permissions.is_empty() {
        return fail("model capability arrays are invalid");
    }
    let mut efforts = HashSet::new();
    for effort in &value.reasoning_efforts {
        bounded(effort, "reasoningEffort", G5_REASONING_EFFORT_MAX_BYTES_V1)?;
        if !efforts.insert(effort.as_str()) {
            return fail("reasoningEfforts must be unique");
        }
    }
    let mut permissions = HashSet::new();
    for permission in &value.permissions {
        one_of(permission, G5_PERMISSIONS_V1, "permission")?;
        if !permissions.insert(permission.as_str()) {
            return fail("permissions must be unique");
        }
    }
    Ok(())
}

fn validate_discovery(value: &AgentDiscoveryEvidenceV1) -> Result<(), AgentAdapterContractError> {
    one_of(&value.source, AGENT_DISCOVERY_SOURCES_V1, "source")?;
    one_of(
        &value.verification_level,
        AGENT_VERIFICATION_LEVELS_V1,
        "verificationLevel",
    )?;
    if let Some(version) = &value.exact_cli_version {
        bounded(version, "exactCliVersion", 128)?;
    }
    one_of(
        &value.effective_model_evidence,
        AGENT_MODEL_EVIDENCE_V1,
        "effectiveModelEvidence",
    )?;
    Ok(())
}

fn validate_profile(value: &AgentResolvedProfileV1) -> Result<(), AgentAdapterContractError> {
    one_of(&value.role_id, G5_ROLE_IDS_V1, "roleId")?;
    one_of(&value.runtime, G5_RUNTIMES_V1, "runtime")?;
    bounded(
        &value.model_selector,
        "modelSelector",
        G5_MODEL_SELECTOR_MAX_BYTES_V1,
    )?;
    if let Some(effort) = &value.reasoning_effort {
        bounded(effort, "reasoningEffort", G5_REASONING_EFFORT_MAX_BYTES_V1)?;
    }
    one_of(&value.permission, G5_PERMISSIONS_V1, "permission")?;
    Ok(())
}

fn validate_execution_mode(value: &AgentExecutionModeV1) -> Result<(), AgentAdapterContractError> {
    if value.invocation != "oneShot"
        || value.prompt_transport != "serverResolvedMessages"
        || value.follow_up != "nextRun"
        || value.resume != "disabled"
        || value.active_stdin
    {
        return fail("agent v1 supports one-shot server prompts and next-run follow-up only");
    }
    Ok(())
}

fn validate_barrier(value: &AgentLaunchBarrierEvidenceV1) -> Result<(), AgentAdapterContractError> {
    validate_process(&value.launcher_identity)?;
    validate_process(&value.agent_identity)?;
    if value.tracked_processes.is_empty() || value.tracked_processes.len() > 256 {
        return fail("trackedProcesses must be nonempty and bounded");
    }
    let mut identities = HashSet::new();
    let mut launcher_found = false;
    let mut agent_found = false;
    for process in &value.tracked_processes {
        validate_process(process)?;
        if !identities.insert((process.pid, process.start_identity_digest.as_str())) {
            return fail("tracked process identities must be unique");
        }
        launcher_found |= same_process(process, &value.launcher_identity);
        agent_found |= same_process(process, &value.agent_identity);
    }
    if !launcher_found || !agent_found {
        return fail("trackedProcesses must contain launcher and agent identities");
    }
    digest(&value.tree_identity_digest, "treeIdentityDigest")?;
    digest(&value.launch_spec_digest, "launchSpecDigest")?;
    digest(&value.evidence_digest, "evidenceDigest")?;
    if !value.tree_tracker_attached
        || !value.run_and_lock_persisted_before_release
        || !value.identity_persisted_before_release
        || !value.release_granted
    {
        return fail("launcher identity barrier must complete before release");
    }
    timestamp(&value.captured_at, "capturedAt")
}

fn validate_process(value: &AgentProcessIdentityV1) -> Result<(), AgentAdapterContractError> {
    if value.pid == 0 || value.pid > crate::MAX_SEQUENCE {
        return fail("pid must be a positive safe integer");
    }
    digest(&value.start_identity_digest, "startIdentityDigest")?;
    digest(&value.executable_path_digest, "executablePathDigest")
}

fn same_process(left: &AgentProcessIdentityV1, right: &AgentProcessIdentityV1) -> bool {
    left.pid == right.pid
        && left.start_identity_digest == right.start_identity_digest
        && left.executable_path_digest == right.executable_path_digest
}

fn validate_run_context(value: &AgentRunContextV1) -> Result<(), AgentAdapterContractError> {
    for (entry, name) in [
        (&value.chat_id, "chatId"),
        (&value.task_id, "taskId"),
        (&value.project_id, "projectId"),
        (&value.worktree_id, "worktreeId"),
        (&value.workspace_identity, "workspaceIdentity"),
    ] {
        id(entry, name)?;
    }
    one_of(&value.role_id, G5_ROLE_IDS_V1, "roleId")?;
    fingerprint_digest(&value.capability_fingerprint, "capabilityFingerprint")
}

fn validate_run_binding(value: &AgentRunBindingV1) -> Result<(), AgentAdapterContractError> {
    let context = AgentRunContextV1 {
        chat_id: value.chat_id.clone(),
        task_id: value.task_id.clone(),
        project_id: value.project_id.clone(),
        worktree_id: value.worktree_id.clone(),
        workspace_identity: value.workspace_identity.clone(),
        role_id: value.role_id.clone(),
        capability_fingerprint: value.capability_fingerprint.clone(),
    };
    validate_run_context(&context)?;
    agent_run_id(&value.run_id, "runId")
}

fn context_from_accepted(value: &AgentRunAcceptedV1) -> AgentRunContextV1 {
    AgentRunContextV1 {
        chat_id: value.chat_id.clone(),
        task_id: value.task_id.clone(),
        project_id: value.project_id.clone(),
        worktree_id: value.worktree_id.clone(),
        workspace_identity: value.workspace_identity.clone(),
        role_id: value.role_id.clone(),
        capability_fingerprint: value.capability_fingerprint.clone(),
    }
}

fn binding_from_event(value: &AgentPublicEventV1) -> AgentRunBindingV1 {
    AgentRunBindingV1 {
        chat_id: value.chat_id.clone(),
        task_id: value.task_id.clone(),
        project_id: value.project_id.clone(),
        worktree_id: value.worktree_id.clone(),
        workspace_identity: value.workspace_identity.clone(),
        run_id: value.run_id.clone(),
        role_id: value.role_id.clone(),
        capability_fingerprint: value.capability_fingerprint.clone(),
    }
}

fn validate_public_ledger_events_v1(
    values: &[Value],
) -> Result<Vec<Value>, AgentAdapterContractError> {
    if values.len() > crate::AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1 {
        return fail("public event ledger must be a bounded array");
    }
    let mut event_ids = HashSet::new();
    let mut expected_binding = None;
    let mut events = Vec::with_capacity(values.len());
    for value in values {
        let event = validate_agent_public_event_v1(value)?;
        if !event_ids.insert(event.event_id.clone()) {
            return fail("public event ledger eventIds must be unique");
        }
        let binding = binding_from_event(&event);
        if let Some(expected) = &expected_binding {
            assert_same_run_binding(expected, &binding, "public event ledger")?;
        } else {
            expected_binding = Some(binding);
        }
        events.push(serde_json::to_value(event).map_err(contract_json)?);
    }
    Ok(events)
}

fn canonical_public_events_snapshot_bytes_v1(
    events: &[Value],
) -> Result<usize, AgentAdapterContractError> {
    let canonical = canonicalize_fingerprint_value(Value::Array(events.to_vec()));
    serde_json::to_vec(&canonical)
        .map(|encoded| encoded.len())
        .map_err(contract_json)
}

fn binding_from_accepted(value: &AgentRunAcceptedV1) -> AgentRunBindingV1 {
    AgentRunBindingV1 {
        chat_id: value.chat_id.clone(),
        task_id: value.task_id.clone(),
        project_id: value.project_id.clone(),
        worktree_id: value.worktree_id.clone(),
        workspace_identity: value.workspace_identity.clone(),
        run_id: value.run_id.clone(),
        role_id: value.role_id.clone(),
        capability_fingerprint: value.capability_fingerprint.clone(),
    }
}

fn binding_from_run(value: &AgentRunRecordV1) -> AgentRunBindingV1 {
    AgentRunBindingV1 {
        chat_id: value.chat_id.clone(),
        task_id: value.task_id.clone(),
        project_id: value.project_id.clone(),
        worktree_id: value.worktree_id.clone(),
        workspace_identity: value.workspace_identity.clone(),
        run_id: value.run_id.clone(),
        role_id: value.selected_profile.role_id.clone(),
        capability_fingerprint: value.capability_fingerprint.clone(),
    }
}

fn assert_same_run_context(
    expected: &AgentRunContextV1,
    actual: &AgentRunContextV1,
    name: &str,
) -> Result<(), AgentAdapterContractError> {
    if expected != actual {
        return fail(format!("{name} changed run context"));
    }
    Ok(())
}

fn assert_same_run_binding(
    expected: &AgentRunBindingV1,
    actual: &AgentRunBindingV1,
    name: &str,
) -> Result<(), AgentAdapterContractError> {
    if expected != actual {
        return fail(format!("{name} changed run binding"));
    }
    Ok(())
}

fn validate_agent_run_identity(
    value: &AgentRunIdentityV1,
) -> Result<(), AgentAdapterContractError> {
    for (entry, name) in [
        (&value.chat_id, "chatId"),
        (&value.task_id, "taskId"),
        (&value.project_id, "projectId"),
        (&value.worktree_id, "worktreeId"),
        (&value.workspace_identity, "workspaceIdentity"),
    ] {
        id(entry, name)?;
    }
    agent_run_id(&value.run_id, "runId")
}

fn validate_inspect_run_identity(
    value: &AgentRecoveryInspectRequestV1,
) -> Result<(), AgentAdapterContractError> {
    validate_agent_run_identity(&AgentRunIdentityV1 {
        chat_id: value.chat_id.clone(),
        task_id: value.task_id.clone(),
        project_id: value.project_id.clone(),
        worktree_id: value.worktree_id.clone(),
        workspace_identity: value.workspace_identity.clone(),
        run_id: value.run_id.clone(),
    })
}

fn validate_resolve_run_identity(
    value: &AgentRecoveryResolveRequestV1,
) -> Result<(), AgentAdapterContractError> {
    validate_agent_run_identity(&AgentRunIdentityV1 {
        chat_id: value.chat_id.clone(),
        task_id: value.task_id.clone(),
        project_id: value.project_id.clone(),
        worktree_id: value.worktree_id.clone(),
        workspace_identity: value.workspace_identity.clone(),
        run_id: value.run_id.clone(),
    })
}

fn assert_same_record_identity(
    expected: &AgentRunIdentityV1,
    actual: &AgentRunRecordV1,
    name: &str,
) -> Result<(), AgentAdapterContractError> {
    for (left, right, field) in [
        (expected.chat_id.as_str(), actual.chat_id.as_str(), "chatId"),
        (expected.task_id.as_str(), actual.task_id.as_str(), "taskId"),
        (
            expected.project_id.as_str(),
            actual.project_id.as_str(),
            "projectId",
        ),
        (
            expected.worktree_id.as_str(),
            actual.worktree_id.as_str(),
            "worktreeId",
        ),
        (
            expected.workspace_identity.as_str(),
            actual.workspace_identity.as_str(),
            "workspaceIdentity",
        ),
        (expected.run_id.as_str(), actual.run_id.as_str(), "runId"),
    ] {
        if left != right {
            return fail(format!("{name} changed {field}"));
        }
    }
    Ok(())
}

fn assert_same_recovery_identity(
    expected: &AgentRunIdentityV1,
    expected_recovery_id: Option<&str>,
    actual: &AgentRecoveryRecordV1,
    name: &str,
) -> Result<(), AgentAdapterContractError> {
    for (left, right, field) in [
        (expected.chat_id.as_str(), actual.chat_id.as_str(), "chatId"),
        (expected.task_id.as_str(), actual.task_id.as_str(), "taskId"),
        (
            expected.project_id.as_str(),
            actual.project_id.as_str(),
            "projectId",
        ),
        (
            expected.worktree_id.as_str(),
            actual.worktree_id.as_str(),
            "worktreeId",
        ),
        (
            expected.workspace_identity.as_str(),
            actual.workspace_identity.as_str(),
            "workspaceIdentity",
        ),
        (expected.run_id.as_str(), actual.run_id.as_str(), "runId"),
    ] {
        if left != right {
            return fail(format!("{name} changed {field}"));
        }
    }
    if expected_recovery_id.is_some_and(|id| id != actual.recovery_id) {
        return fail(format!("{name} changed recoveryId"));
    }
    Ok(())
}

fn assert_same_recovery_identity_from_resolve(
    expected: &AgentRecoveryResolveRequestV1,
    actual: &AgentRecoveryRecordV1,
    name: &str,
) -> Result<(), AgentAdapterContractError> {
    let identity = AgentRunIdentityV1 {
        chat_id: expected.chat_id.clone(),
        task_id: expected.task_id.clone(),
        project_id: expected.project_id.clone(),
        worktree_id: expected.worktree_id.clone(),
        workspace_identity: expected.workspace_identity.clone(),
        run_id: expected.run_id.clone(),
    };
    assert_same_recovery_identity(&identity, Some(&expected.recovery_id), actual, name)
}

fn assert_same_recovery_records(
    expected: &AgentRecoveryRecordV1,
    actual: &AgentRecoveryRecordV1,
    name: &str,
) -> Result<(), AgentAdapterContractError> {
    let identity = AgentRunIdentityV1 {
        chat_id: expected.chat_id.clone(),
        task_id: expected.task_id.clone(),
        project_id: expected.project_id.clone(),
        worktree_id: expected.worktree_id.clone(),
        workspace_identity: expected.workspace_identity.clone(),
        run_id: expected.run_id.clone(),
    };
    assert_same_recovery_identity(&identity, Some(&expected.recovery_id), actual, name)
}

fn validate_unique_processes(
    processes: &[AgentProcessIdentityV1],
) -> Result<(), AgentAdapterContractError> {
    let mut identities = HashSet::new();
    for process in processes {
        validate_process(process)?;
        if !identities.insert((process.pid, process.start_identity_digest.as_str())) {
            return fail("tracked process identities must be unique");
        }
    }
    Ok(())
}

fn exact_value_keys(
    object: &Map<String, Value>,
    expected: &[&str],
    name: &str,
) -> Result<(), AgentAdapterContractError> {
    if object.len() != expected.len() || !object.keys().all(|key| expected.contains(&key.as_str()))
    {
        return fail(format!("{name} has unknown or missing fields"));
    }
    Ok(())
}

fn chat_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, AgentAdapterContractError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| AgentAdapterContractError::new(format!("chat {key} must be a string")))
}

fn value_string<'a>(
    value: &'a Value,
    key: &str,
    name: &str,
) -> Result<&'a str, AgentAdapterContractError> {
    value
        .as_object()
        .and_then(|object| object.get(key))
        .and_then(Value::as_str)
        .ok_or_else(|| AgentAdapterContractError::new(format!("{name} {key} must be a string")))
}

fn from_value<T: for<'de> Deserialize<'de> + Serialize>(
    value: &Value,
    name: &str,
) -> Result<T, AgentAdapterContractError> {
    let decoded: T = serde_json::from_value(value.clone())
        .map_err(|error| AgentAdapterContractError::new(format!("invalid {name}: {error}")))?;
    let canonical = serde_json::to_value(&decoded).map_err(contract_json)?;
    if canonical != *value {
        return fail(format!("invalid {name}: optional fields must be explicit"));
    }
    Ok(decoded)
}

fn contract_json(error: serde_json::Error) -> AgentAdapterContractError {
    AgentAdapterContractError::new(format!("contract serialization failed: {error}"))
}

fn schema(value: u64) -> Result<(), AgentAdapterContractError> {
    if value != G5A_SCHEMA_VERSION_V1 {
        return fail("schemaVersion must be 1");
    }
    Ok(())
}

fn one_of<'a>(
    value: &'a str,
    values: &[&str],
    name: &str,
) -> Result<&'a str, AgentAdapterContractError> {
    if !values.contains(&value) {
        return fail(format!("invalid {name}"));
    }
    Ok(value)
}

fn bounded(value: &str, name: &str, maximum: usize) -> Result<(), AgentAdapterContractError> {
    if value.trim().is_empty() || value.len() > maximum {
        return fail(format!("{name} must be nonempty and bounded"));
    }
    Ok(())
}

fn id(value: &str, name: &str) -> Result<(), AgentAdapterContractError> {
    bounded(value, name, G5_ID_MAX_BYTES_V1)
}

fn agent_run_id(value: &str, name: &str) -> Result<(), AgentAdapterContractError> {
    id(value, name)?;
    if !value.starts_with("agent_run_") {
        return fail(format!("{name} must use the actual-agent run namespace"));
    }
    Ok(())
}

fn agent_recovery_id(value: &str, name: &str) -> Result<(), AgentAdapterContractError> {
    id(value, name)?;
    if !value.starts_with("agent_recovery_") {
        return fail(format!(
            "{name} must use the actual-agent recovery namespace"
        ));
    }
    Ok(())
}

fn agent_event_id(value: &str, name: &str) -> Result<(), AgentAdapterContractError> {
    id(value, name)?;
    if !value.starts_with("agent_event_") {
        return fail(format!("{name} must use the actual-agent event namespace"));
    }
    Ok(())
}

fn agent_state_event_id(value: &str, name: &str) -> Result<(), AgentAdapterContractError> {
    id(value, name)?;
    if !value.starts_with("agent_state_event_") {
        return fail(format!("{name} must use the agent state-event namespace"));
    }
    Ok(())
}

fn digest(value: &str, name: &str) -> Result<(), AgentAdapterContractError> {
    id(value, name)?;
    if !value.starts_with("sha256:") {
        return fail(format!("{name} must be a redacted sha256 digest"));
    }
    Ok(())
}

fn fingerprint_digest(value: &str, name: &str) -> Result<(), AgentAdapterContractError> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if !valid {
        return fail(format!("{name} must be an exact lowercase SHA-256 digest"));
    }
    Ok(())
}

fn canonicalize_fingerprint_value(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(canonicalize_fingerprint_value)
                .collect(),
        ),
        Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let mut canonical = Map::new();
            for (key, value) in entries {
                canonical.insert(key, canonicalize_fingerprint_value(value));
            }
            Value::Object(canonical)
        }
        scalar => scalar,
    }
}

fn timestamp(value: &str, name: &str) -> Result<(), AgentAdapterContractError> {
    let bytes = value.as_bytes();
    let separators = [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'Z'),
    ];
    let valid = bytes.len() == 20
        && separators
            .iter()
            .all(|(index, expected)| bytes.get(*index) == Some(expected))
        && bytes.iter().enumerate().all(|(index, byte)| {
            separators.iter().any(|(separator, _)| *separator == index) || byte.is_ascii_digit()
        });
    if !valid {
        return fail(format!("{name} must be an RFC 3339 UTC second timestamp"));
    }
    Ok(())
}

fn fail<T>(detail: impl Into<String>) -> Result<T, AgentAdapterContractError> {
    Err(AgentAdapterContractError::new(detail))
}

#[cfg(test)]
mod tests;
