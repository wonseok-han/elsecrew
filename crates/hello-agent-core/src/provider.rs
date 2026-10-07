//! G5C provider discovery and event boundary.
//!
//! This module never probes the host or starts a vendor process. The owning sidecar supplies
//! bounded observations and sanitized executable identities. Only exact, versioned manifests and
//! redacted classifications leave this boundary.

use crate::agent_adapter::{AgentAdapterError, LaunchSpec};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub mod vendor_observation;
mod unique_json;

pub const PROVIDER_OBSERVATION_LINE_MAX_BYTES: usize = 128 * 1024;
pub const PROVIDER_OBSERVATION_TOTAL_MAX_BYTES: usize = 1024 * 1024;
pub const PROVIDER_INTERNAL_EVENTS_MAX: usize = 1024;
pub const PROVIDER_PUBLIC_EVENTS_MAX: usize = 256;
pub const PROVIDER_PUBLIC_TEXT_TOTAL_MAX_BYTES: usize = 1024 * 1024;
pub const PROVIDER_PUBLIC_TEXT_MAX_BYTES: usize = 64 * 1024;

const CLAUDE_VERSION: &str = "2.1.241";
const CODEX_VERSION: &str = "0.150.0";
const CLAUDE_AUTH_PARSER: &str = "claude-code.auth-status.v1";
const CLAUDE_EVENT_PARSER: &str = "claude-code.stream-json.v1";
const CODEX_AUTH_PARSER: &str = "codex.auth-status.v1";
const CODEX_EVENT_PARSER: &str = "codex.jsonl.v1";
const CLAUDE_HANDSHAKE: &str = "claude-code.system-init.v1";

const CLAUDE_VERIFICATION: &[&str] = &[
    "version",
    "model",
    "cwd",
    "permission",
    "tools",
    "mcp",
    "plugins",
    "config",
];
const CODEX_VERIFICATION: &[&str] = &[
    "version",
    "model",
    "cwd",
    "permission",
    "sandbox",
    "approvalPolicy",
    "config",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError(pub String);

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ProviderError {}

impl From<AgentAdapterError> for ProviderError {
    fn from(value: AgentAdapterError) -> Self {
        Self(value.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderId {
    ClaudeCode,
    Codex,
}

impl ProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    fn version(self) -> &'static str {
        match self {
            Self::ClaudeCode => CLAUDE_VERSION,
            Self::Codex => CODEX_VERSION,
        }
    }

    fn binary_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }

    fn helper_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "node",
            Self::Codex => "rg",
        }
    }

    fn parser_ids(self) -> (&'static str, &'static str) {
        match self {
            Self::ClaudeCode => (CLAUDE_AUTH_PARSER, CLAUDE_EVENT_PARSER),
            Self::Codex => (CODEX_AUTH_PARSER, CODEX_EVENT_PARSER),
        }
    }

    fn verification(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => CLAUDE_VERIFICATION,
            Self::Codex => CODEX_VERIFICATION,
        }
    }
}

/// Transient vendor output. `Debug` and every public result deliberately omit the bytes.
pub struct BoundedObservation {
    bytes: Vec<u8>,
}

impl fmt::Debug for BoundedObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedObservation")
            .field("bytes", &"<redacted>")
            .field("length", &self.bytes.len())
            .finish()
    }
}

impl BoundedObservation {
    pub fn new(bytes: Vec<u8>) -> Result<Self, ProviderError> {
        if bytes.is_empty() || bytes.len() > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES {
            return fail("provider observation is empty or exceeds the total byte limit");
        }
        if bytes
            .split(|byte| *byte == b'\n')
            .any(|line| line.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES)
        {
            return fail("provider observation contains an oversized line");
        }
        Ok(Self { bytes })
    }

    fn text(&self) -> Result<&str, ProviderError> {
        std::str::from_utf8(&self.bytes)
            .map(str::trim)
            .map_err(|_| ProviderError("provider observation is not UTF-8".into()))
    }

    fn json(&self) -> Result<Value, ProviderError> {
        unique_json::parse(&self.bytes)
            .map_err(|_| ProviderError("provider observation is malformed JSON".into()))
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedExecutableClosureV1 {
    pub binary: ProviderExecutableFileIdentityV1,
    pub required_helpers: Vec<ProviderExecutableFileIdentityV1>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCatalogEvidenceV1 {
    RuntimeVerified,
    ExactVersionManifest,
    ArgvOnly,
    Unknown,
}

impl ModelCatalogEvidenceV1 {
    fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeVerified => "runtimeVerified",
            Self::ExactVersionManifest => "exactVersionManifest",
            Self::ArgvOnly => "argvOnly",
            Self::Unknown => "unknown",
        }
    }
}

pub struct DiscoveryInputV1 {
    pub executable: Option<ObservedExecutableClosureV1>,
    pub version: Option<BoundedObservation>,
    pub auth: Option<BoundedObservation>,
    pub model_catalog_evidence: ModelCatalogEvidenceV1,
    pub exact_profiles: Vec<ProviderProfileTupleV1>,
    pub observed_runtime_verification: Vec<String>,
    pub observed_at: String,
}

impl fmt::Debug for DiscoveryInputV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiscoveryInputV1")
            .field("executable", &self.executable)
            .field("version", &self.version)
            .field("auth", &self.auth)
            .field("modelCatalogEvidence", &self.model_catalog_evidence)
            .field("exactProfiles", &self.exact_profiles)
            .field(
                "observedRuntimeVerification",
                &self.observed_runtime_verification,
            )
            .field("observedAt", &self.observed_at)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct ProviderDefinitionV1 {
    provider: ProviderId,
    manifest: ProviderManifestV1,
}

impl ProviderDefinitionV1 {
    /// Builds the exact supported manifest from sanitized, injected file identities.
    pub fn new(
        provider: ProviderId,
        executable: ObservedExecutableClosureV1,
    ) -> Result<Self, ProviderError> {
        validate_observed_closure_shape(provider, &executable)?;
        let closure = build_closure(provider, executable, provider.version())?;
        let profiles = exact_profiles(provider);
        let (auth_parser, event_parser) = provider.parser_ids();
        let mut manifest = ProviderManifestV1 {
            schema_version: 1,
            provider_id: provider.as_str().into(),
            runtime: provider.as_str().into(),
            manifest_version: format!("{}-{}-v1", provider.as_str(), provider.version()),
            executable_closure: closure,
            exact_profiles: profiles,
            auth_parser_id: auth_parser.into(),
            event_parser_id: event_parser.into(),
            launch_policy: ProviderLaunchPolicyV1 {
                invocation: "oneShot".into(),
                prompt_transport: "serverResolvedMessages".into(),
                cwd_policy: "managedWorkspaceExact".into(),
                environment_policy: "allowlistedNoSecrets".into(),
                fallback_policy: "forbidden".into(),
                required_runtime_verification: strings(provider.verification()),
            },
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = digest_struct_without(&manifest, "manifestDigest")?;
        Ok(Self { provider, manifest })
    }

    pub fn manifest(&self) -> &ProviderManifestV1 {
        &self.manifest
    }

    pub fn discover(
        &self,
        input: DiscoveryInputV1,
    ) -> Result<SanitizedDiscoveryObservationV1, ProviderError> {
        validate_profiles(&input.exact_profiles, true)?;
        validate_verification_subset(
            &input.observed_runtime_verification,
            self.provider.verification(),
        )?;
        validate_timestamp(&input.observed_at, "observedAt")?;

        let (installation_state, executable_closure, exact_cli_version) =
            match (input.executable, input.version.as_ref()) {
                (None, _) => ("notInstalled", None, None),
                (Some(_), None) => ("incomplete", None, None),
                (Some(executable), Some(version_observation)) => {
                    if validate_observed_closure_shape(self.provider, &executable).is_err() {
                        ("incomplete", None, None)
                    } else {
                        let version = parse_cli_version(self.provider, version_observation)?;
                        let closure = build_closure(self.provider, executable, &version)?;
                        ("installed", Some(closure), Some(version))
                    }
                }
            };
        let auth_state = if installation_state == "installed" {
            match input.auth.as_ref() {
                Some(observation) => parse_auth_state(self.provider, observation)?,
                None => "unknown",
            }
        } else {
            "unknown"
        };

        let evidence = input.model_catalog_evidence.as_str();
        // Unknown profile strings are never persisted. An exact mismatch is represented by the
        // empty sanitized tuple set and the deterministic `unsupportedProfile` blocker.
        let sanitized_profiles = if input.exact_profiles == self.manifest.exact_profiles {
            input.exact_profiles
        } else {
            Vec::new()
        };
        let blocker = discovery_blocker(
            installation_state,
            auth_state,
            executable_closure.as_ref(),
            exact_cli_version.as_deref(),
            evidence,
            &sanitized_profiles,
            &input.observed_runtime_verification,
            &self.manifest,
        );

        Ok(SanitizedDiscoveryObservationV1 {
            schema_version: 1,
            provider_id: self.provider.as_str().into(),
            runtime: self.provider.as_str().into(),
            manifest_digest: self.manifest.manifest_digest.clone(),
            installation_state: installation_state.into(),
            auth_state: auth_state.into(),
            executable_closure,
            exact_cli_version,
            model_catalog_evidence: evidence.into(),
            exact_profiles: sanitized_profiles,
            auth_parser_id: self.manifest.auth_parser_id.clone(),
            event_parser_id: self.manifest.event_parser_id.clone(),
            observed_runtime_verification: input.observed_runtime_verification,
            can_start: blocker.is_none(),
            blocked_reason: blocker.map(str::to_owned),
            observed_at: input.observed_at,
        })
    }

    pub fn event_classifier(&self) -> ProviderEventClassifierV1 {
        ProviderEventClassifierV1::new(self.clone())
    }
}

pub struct ClaudeHandshakeContextV1<'a> {
    pub launch_spec: &'a LaunchSpec,
    pub run_id: String,
    pub capability_fingerprint: String,
    pub effective_profile: ProviderProfileTupleV1,
    pub workspace_identity: String,
    pub managed_cwd_identity: String,
    pub expected_tools: Vec<String>,
    pub expected_mcp_servers: Vec<String>,
    pub expected_plugins: Vec<String>,
    pub verified_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaudeHandshakeWire {
    #[serde(rename = "type")]
    kind: String,
    subtype: String,
    claude_code_version: String,
    model: String,
    cwd: String,
    #[serde(rename = "permissionMode")]
    permission_mode: String,
    tools: Vec<String>,
    mcp_servers: Vec<String>,
    plugins: Vec<String>,
    #[serde(rename = "configIsolation")]
    config_isolation: String,
}

#[derive(Debug)]
pub struct ProviderEventClassifierV1 {
    definition: ProviderDefinitionV1,
    handshake_seen: bool,
    final_seen: bool,
    terminal_seen: bool,
    sequence: u64,
    public_events: usize,
    public_text_bytes: usize,
}

impl ProviderEventClassifierV1 {
    fn new(definition: ProviderDefinitionV1) -> Self {
        Self {
            definition,
            handshake_seen: false,
            final_seen: false,
            terminal_seen: false,
            sequence: 0,
            public_events: 0,
            public_text_bytes: 0,
        }
    }

    pub fn accept_claude_handshake(
        &mut self,
        discovery: &SanitizedDiscoveryObservationV1,
        observation: BoundedObservation,
        context: ClaudeHandshakeContextV1<'_>,
    ) -> Result<(AgentRuntimeAttestationV1, InternalProviderEventV1), ProviderError> {
        if self.definition.provider != ProviderId::ClaudeCode
            || self.handshake_seen
            || self.sequence != 0
            || self.terminal_seen
        {
            return fail("Claude handshake is not valid in the current provider state");
        }
        validate_startable_discovery(&self.definition.manifest, discovery)?;
        validate_profile(&context.effective_profile)?;
        if !self
            .definition
            .manifest
            .exact_profiles
            .contains(&context.effective_profile)
        {
            return fail("Claude handshake profile is not an exact supported tuple");
        }
        validate_digest(&context.capability_fingerprint, "capabilityFingerprint")?;
        validate_digest(&context.managed_cwd_identity, "managedCwdIdentity")?;
        validate_identifier(&context.run_id, "runId")?;
        validate_identifier(&context.workspace_identity, "workspaceIdentity")?;
        validate_timestamp(&context.verified_at, "verifiedAt")?;
        if context.launch_spec.parser() != self.definition.manifest.event_parser_id
            || context.launch_spec.expected_handshake() != CLAUDE_HANDSHAKE
        {
            return fail("Claude launch spec parser or handshake identity drifted");
        }

        let wire: ClaudeHandshakeWire = serde_json::from_value(observation.json()?)
            .map_err(|_| ProviderError("Claude handshake shape is malformed".into()))?;
        let expected_cwd = context
            .launch_spec
            .cwd()
            .to_str()
            .ok_or_else(|| ProviderError("managed cwd is not UTF-8".into()))?;
        if wire.kind != "system"
            || wire.subtype != "init"
            || wire.claude_code_version != self.definition.provider.version()
            || wire.model != context.effective_profile.model_selector
            || wire.cwd != expected_cwd
            || wire.permission_mode != context.effective_profile.permission
            || !same_exact_set(&wire.tools, &context.expected_tools)
            || !same_exact_set(&wire.mcp_servers, &context.expected_mcp_servers)
            || !same_exact_set(&wire.plugins, &context.expected_plugins)
            || wire.config_isolation != "managedOnly"
        {
            return fail("Claude runtime handshake does not match the exact launch context");
        }

        let closure = discovery.executable_closure.as_ref().ok_or_else(|| {
            ProviderError("startable discovery lost its executable closure".into())
        })?;
        let evidence = self
            .definition
            .manifest
            .launch_policy
            .required_runtime_verification
            .clone();
        let attestation = AgentRuntimeAttestationV1 {
            schema_version: 1,
            run_id: context.run_id,
            provider_id: self.definition.provider.as_str().into(),
            runtime: self.definition.provider.as_str().into(),
            capability_fingerprint: context.capability_fingerprint,
            manifest_digest: self.definition.manifest.manifest_digest.clone(),
            executable_closure_digest: closure.closure_digest.clone(),
            effective_profile: context.effective_profile,
            workspace_identity: context.workspace_identity,
            managed_cwd_identity: context.managed_cwd_identity,
            event_parser_id: self.definition.manifest.event_parser_id.clone(),
            verified_runtime_evidence: evidence.clone(),
            verified_at: context.verified_at,
        };
        self.sequence = 1;
        self.handshake_seen = true;
        Ok((
            attestation,
            InternalProviderEventV1::Handshake {
                schema_version: 1,
                sequence: 1,
                verified_runtime_evidence: evidence,
            },
        ))
    }

    pub fn classify(
        &mut self,
        observation: BoundedObservation,
    ) -> Result<InternalProviderEventV1, ProviderError> {
        if self.terminal_seen || self.sequence as usize >= PROVIDER_INTERNAL_EVENTS_MAX {
            return fail("provider event arrived after terminal or exceeded the run limit");
        }
        let value = observation.json()?;
        let object = value
            .as_object()
            .ok_or_else(|| ProviderError("provider event must be an object".into()))?;
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError("provider event type is missing".into()))?;
        let sequence = self.sequence + 1;
        let event = match kind {
            "status" | "question" | "final" => {
                if !self.handshake_seen {
                    return fail("provider handshake must precede public events");
                }
                if self.final_seen {
                    return fail("no public event may follow final");
                }
                exact_keys(object, &["type", "text"], "public provider event")?;
                let text = object
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ProviderError("public provider text is missing".into()))?;
                validate_nonempty_bounded(text, PROVIDER_PUBLIC_TEXT_MAX_BYTES, "public text")?;
                let next_events = self.public_events + 1;
                let next_text = self
                    .public_text_bytes
                    .checked_add(text.len())
                    .ok_or_else(|| ProviderError("public provider text bytes overflow".into()))?;
                if next_events > PROVIDER_PUBLIC_EVENTS_MAX
                    || next_text > PROVIDER_PUBLIC_TEXT_TOTAL_MAX_BYTES
                {
                    return fail("public provider event exceeds the G5B run budget");
                }
                self.public_events = next_events;
                self.public_text_bytes = next_text;
                self.final_seen = kind == "final";
                InternalProviderEventV1::Public {
                    schema_version: 1,
                    sequence,
                    kind: kind.into(),
                    text: text.into(),
                }
            }
            "reasoning" | "toolCall" | "toolResult" | "transport" => {
                if !self.handshake_seen {
                    return fail("provider handshake must precede private events");
                }
                InternalProviderEventV1::Private {
                    schema_version: 1,
                    sequence,
                    private_kind: kind.into(),
                }
            }
            "terminal" => {
                if !self.handshake_seen {
                    return fail("provider handshake must precede terminal events");
                }
                exact_keys(object, &["type", "outcome"], "terminal provider event")?;
                let outcome = object
                    .get("outcome")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ProviderError("terminal outcome is missing".into()))?;
                if !matches!(
                    outcome,
                    "succeeded" | "failed" | "canceled" | "recoveryRequired"
                ) {
                    return fail("terminal provider outcome is unknown");
                }
                self.terminal_seen = true;
                InternalProviderEventV1::Terminal {
                    schema_version: 1,
                    sequence,
                    outcome: outcome.into(),
                }
            }
            "authFailure" => {
                if self.handshake_seen || self.sequence != 0 {
                    return fail("auth failure must be the sole pre-handshake event");
                }
                exact_keys(object, &["type", "authState"], "auth failure event")?;
                let auth_state = object
                    .get("authState")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ProviderError("auth failure state is missing".into()))?;
                if !matches!(auth_state, "notAuthenticated" | "expired") {
                    return fail("auth failure state is unknown");
                }
                self.terminal_seen = true;
                InternalProviderEventV1::AuthFailure {
                    schema_version: 1,
                    sequence,
                    auth_state: auth_state.into(),
                }
            }
            "handshake" | "system" => {
                return fail("provider handshake must pass exact runtime verification")
            }
            _ => return fail("unknown provider event type"),
        };
        self.sequence = sequence;
        Ok(event)
    }
}

fn exact_profiles(provider: ProviderId) -> Vec<ProviderProfileTupleV1> {
    let (model, reasoning) = match provider {
        ProviderId::ClaudeCode => ("claude-opus-5", None),
        ProviderId::Codex => ("gpt-5.6-sol", Some("medium")),
    };
    ["readOnly", "workspaceWrite"]
        .into_iter()
        .map(|permission| ProviderProfileTupleV1 {
            model_selector: model.into(),
            reasoning_effort: reasoning.map(str::to_owned),
            permission: permission.into(),
        })
        .collect()
}

fn validate_observed_closure_shape(
    provider: ProviderId,
    closure: &ObservedExecutableClosureV1,
) -> Result<(), ProviderError> {
    validate_file_identity(&closure.binary)?;
    if closure.binary.logical_name != provider.binary_name()
        || closure.required_helpers.len() != 1
        || closure.required_helpers[0].logical_name != provider.helper_name()
    {
        return fail("provider executable closure does not match the exact required components");
    }
    validate_file_identity(&closure.required_helpers[0])
}

fn validate_file_identity(file: &ProviderExecutableFileIdentityV1) -> Result<(), ProviderError> {
    validate_nonempty_bounded(&file.logical_name, 64, "logicalName")?;
    validate_digest(&file.path_digest, "pathDigest")?;
    validate_digest(&file.file_identity_digest, "fileIdentityDigest")
}

fn build_closure(
    provider: ProviderId,
    observed: ObservedExecutableClosureV1,
    version: &str,
) -> Result<ProviderExecutableClosureV1, ProviderError> {
    validate_observed_closure_shape(provider, &observed)?;
    validate_semver(version)?;
    let mut closure = ProviderExecutableClosureV1 {
        binary: ProviderExecutableBinaryIdentityV1 {
            logical_name: observed.binary.logical_name,
            path_digest: observed.binary.path_digest,
            file_identity_digest: observed.binary.file_identity_digest,
            cli_version: version.into(),
        },
        required_helpers: observed.required_helpers,
        closure_digest: String::new(),
    };
    closure.closure_digest = digest_struct_without(&closure, "closureDigest")?;
    Ok(closure)
}

/// Parse bounded version output. This identifies a reported version only;
/// it does not authenticate the executable or grant runtime admission.
pub fn parse_cli_version(
    provider: ProviderId,
    observation: &BoundedObservation,
) -> Result<String, ProviderError> {
    let text = observation.text()?;
    let version = match provider {
        ProviderId::ClaudeCode => text
            .strip_prefix("Claude Code ")
            .or_else(|| text.strip_suffix(" (Claude Code)")),
        ProviderId::Codex => text
            .strip_prefix("codex-cli ")
            .or_else(|| text.strip_prefix("codex ")),
    }
    .ok_or_else(|| ProviderError("provider version output is malformed".into()))?;
    validate_semver(version)?;
    Ok(version.into())
}

fn parse_auth_state(
    provider: ProviderId,
    observation: &BoundedObservation,
) -> Result<&'static str, ProviderError> {
    match provider {
        ProviderId::ClaudeCode => {
            let value = observation.json()?;
            let object = value
                .as_object()
                .ok_or_else(|| ProviderError("Claude auth output must be an object".into()))?;
            if object.get("expired").and_then(Value::as_bool) == Some(true) {
                return Ok("expired");
            }
            Ok(match object.get("loggedIn").and_then(Value::as_bool) {
                Some(true) => "authenticated",
                Some(false) => "notAuthenticated",
                None => "unknown",
            })
        }
        ProviderId::Codex => Ok(match observation.text()? {
            "Logged in using ChatGPT" | "Logged in using an API key" => "authenticated",
            "Not logged in" => "notAuthenticated",
            "Authentication expired" => "expired",
            _ => "unknown",
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn discovery_blocker<'a>(
    installation: &str,
    auth: &str,
    closure: Option<&ProviderExecutableClosureV1>,
    version: Option<&str>,
    catalog: &str,
    profiles: &[ProviderProfileTupleV1],
    verification: &[String],
    manifest: &'a ProviderManifestV1,
) -> Option<&'a str> {
    if installation == "notInstalled" {
        return Some("notInstalled");
    }
    let Some(closure) = closure else {
        return Some("closureMismatch");
    };
    if installation == "incomplete" {
        return Some("closureMismatch");
    }
    if auth == "expired" {
        return Some("authExpired");
    }
    if auth != "authenticated" {
        return Some("notAuthenticated");
    }
    if closure.closure_digest != manifest.executable_closure.closure_digest {
        return Some("closureMismatch");
    }
    if version != Some(manifest.executable_closure.binary.cli_version.as_str()) {
        return Some("unsupportedVersion");
    }
    if matches!(catalog, "argvOnly" | "unknown") {
        return Some("modelCatalogUnverified");
    }
    if profiles != manifest.exact_profiles {
        return Some("unsupportedProfile");
    }
    if verification != manifest.launch_policy.required_runtime_verification {
        return Some("runtimeVerificationIncomplete");
    }
    None
}

fn validate_startable_discovery(
    manifest: &ProviderManifestV1,
    discovery: &SanitizedDiscoveryObservationV1,
) -> Result<(), ProviderError> {
    let closure = discovery
        .executable_closure
        .as_ref()
        .ok_or_else(|| ProviderError("startable discovery has no executable closure".into()))?;
    if discovery.schema_version != 1
        || !discovery.can_start
        || discovery.blocked_reason.is_some()
        || discovery.provider_id != manifest.provider_id
        || discovery.runtime != manifest.runtime
        || discovery.manifest_digest != manifest.manifest_digest
        || discovery.installation_state != "installed"
        || discovery.auth_state != "authenticated"
        || closure != &manifest.executable_closure
        || discovery.exact_cli_version.as_deref()
            != Some(manifest.executable_closure.binary.cli_version.as_str())
        || !matches!(
            discovery.model_catalog_evidence.as_str(),
            "runtimeVerified" | "exactVersionManifest"
        )
        || discovery.exact_profiles != manifest.exact_profiles
        || discovery.auth_parser_id != manifest.auth_parser_id
        || discovery.event_parser_id != manifest.event_parser_id
        || discovery.observed_runtime_verification
            != manifest.launch_policy.required_runtime_verification
    {
        return fail("runtime handshake requires exact startable discovery evidence");
    }
    Ok(())
}

fn validate_profiles(
    profiles: &[ProviderProfileTupleV1],
    allow_empty: bool,
) -> Result<(), ProviderError> {
    if profiles.len() > 128 || (!allow_empty && profiles.is_empty()) {
        return fail("provider profiles must be bounded");
    }
    let mut seen = BTreeSet::new();
    for profile in profiles {
        validate_profile(profile)?;
        if !seen.insert((
            profile.model_selector.as_str(),
            profile.reasoning_effort.as_deref(),
            profile.permission.as_str(),
        )) {
            return fail("provider profiles must be unique exact tuples");
        }
    }
    Ok(())
}

fn validate_profile(profile: &ProviderProfileTupleV1) -> Result<(), ProviderError> {
    validate_nonempty_bounded(&profile.model_selector, 128, "modelSelector")?;
    if let Some(reasoning) = profile.reasoning_effort.as_deref() {
        validate_nonempty_bounded(reasoning, 32, "reasoningEffort")?;
    }
    if !matches!(profile.permission.as_str(), "readOnly" | "workspaceWrite") {
        return fail("provider profile permission is unsupported");
    }
    Ok(())
}

fn validate_verification_subset(
    observed: &[String],
    required: &[&str],
) -> Result<(), ProviderError> {
    if observed.len() > required.len() {
        return fail("runtime verification evidence exceeds the manifest set");
    }
    let mut unique = BTreeSet::new();
    for value in observed {
        if !required.contains(&value.as_str()) || !unique.insert(value.as_str()) {
            return fail("runtime verification evidence is unknown or duplicated");
        }
    }
    Ok(())
}

fn same_exact_set(actual: &[String], expected: &[String]) -> bool {
    let actual_count = actual.len();
    let expected_count = expected.len();
    if actual_count != expected_count {
        return false;
    }
    let actual: BTreeSet<&str> = actual.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = expected.iter().map(String::as_str).collect();
    actual.len() == actual_count && expected.len() == expected_count && actual == expected
}

fn validate_semver(value: &str) -> Result<(), ProviderError> {
    // Preserve the exact vendor version, including prerelease/build identity.
    // A reported version never substitutes for signature or protocol admission.
    let (release, build) = value.split_once('+').map_or((value, None), |(a, b)| (a, Some(b)));
    let (core, prerelease) = release.split_once('-').map_or((release, None), |(a, b)| (a, Some(b)));
    let identifiers_valid = |suffix: &str, prerelease: bool| suffix.split('.').all(|part| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !(prerelease && part.len() > 1 && part.starts_with('0') && part.bytes().all(|byte| byte.is_ascii_digit()))
    });
    let parts: Vec<&str> = core.split('.').collect();
    if value.len() > 128
        || build.is_some_and(|suffix| !identifiers_valid(suffix, false))
        || prerelease.is_some_and(|suffix| !identifiers_valid(suffix, true))
        || parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return fail("provider CLI version is not an exact semantic version");
    }
    Ok(())
}

fn validate_digest(value: &str, name: &str) -> Result<(), ProviderError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return fail(format!("{name} must be a SHA-256 digest"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return fail(format!("{name} must be a SHA-256 digest"));
    }
    Ok(())
}

fn validate_nonempty_bounded(value: &str, maximum: usize, name: &str) -> Result<(), ProviderError> {
    if value.trim().is_empty() || value.len() > maximum {
        return fail(format!("{name} must be nonempty and bounded"));
    }
    Ok(())
}

fn validate_identifier(value: &str, name: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return fail(format!("{name} must be a bounded identifier"));
    }
    Ok(())
}

fn validate_timestamp(value: &str, name: &str) -> Result<(), ProviderError> {
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

fn exact_keys(
    object: &Map<String, Value>,
    expected: &[&str],
    name: &str,
) -> Result<(), ProviderError> {
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return fail(format!("{name} contains unapproved fields"));
    }
    Ok(())
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

fn digest_struct_without<T: Serialize>(value: &T, field: &str) -> Result<String, ProviderError> {
    let mut value = serde_json::to_value(value)
        .map_err(|_| ProviderError("provider evidence could not be serialized".into()))?;
    value
        .as_object_mut()
        .ok_or_else(|| ProviderError("provider evidence must serialize as an object".into()))?
        .remove(field);
    let canonical = serde_json::to_vec(&canonicalize(value))
        .map_err(|_| ProviderError("provider evidence could not be canonicalized".into()))?;
    Ok(format!("sha256:{}", hex(&sha256(&canonical))))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted: BTreeMap<String, Value> = object
                .into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        other => other,
    }
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    text
}

// Small dependency-free SHA-256 used only for canonical sanitized manifests.
fn sha256(input: &[u8]) -> [u8; 32] {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let bit_len = (input.len() as u64) * 8;
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut state = INITIAL;
    for chunk in padded.chunks_exact(64) {
        let mut words = [0_u32; 64];
        for (index, bytes) in chunk.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes(bytes.try_into().expect("four bytes"));
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut output = [0_u8; 32];
    for (chunk, word) in output.chunks_exact_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    output
}

fn fail<T>(detail: impl Into<String>) -> Result<T, ProviderError> {
    Err(ProviderError(detail.into()))
}

#[cfg(test)]
mod tests;
