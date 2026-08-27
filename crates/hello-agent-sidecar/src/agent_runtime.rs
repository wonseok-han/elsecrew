//! Planned G5A actual-agent boundary with a deterministic fake adapter gate.
//!
//! Production construction is deliberately blocked: it can report capabilities but cannot create a
//! process. Tests may inject an exact fake manifest and the sidecar binary as the only executable.

use crate::chat::{G5Action, G5Event};
use crate::project::{ProjectFailure, ProjectManager, WorkspaceAddress};
use crate::storage::{ProjectionUpsert, Storage};
use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use hello_agent_core::agent_adapter::{AgentAdapterError, LaunchSpec, MAX_AGENT_EVENT_BYTES};
use hello_agent_core::proctree::{
    identity_of, ProbeResult, ProcIdentity, ProcTable, SystemClock, SystemProcTable,
    TerminationConfig, TreeTracker,
};
use hello_agent_core::provider::{
    BoundedObservation, ClaudeHandshakeContextV1, DiscoveryInputV1, InternalProviderEventV1,
    ModelCatalogEvidenceV1, ObservedExecutableClosureV1, ProviderDefinitionV1,
    ProviderEventClassifierV1, ProviderExecutableFileIdentityV1, ProviderId,
    ProviderProfileTupleV1, SanitizedDiscoveryObservationV1, PROVIDER_OBSERVATION_LINE_MAX_BYTES,
};
use hello_agent_protocol::agent_adapter_v1::{
    agent_public_events_snapshot_bytes_v1, compute_agent_capability_fingerprint_v1,
    validate_agent_adapter_payload_v1, validate_agent_runtime_attestation_against_accepted_v1,
    validate_provider_manifest_v1, validate_sanitized_discovery_observation_v1,
    AgentCapabilitySnapshotV1, AgentDiscoveryEvidenceV1, AgentExecutableIdentityV1,
    AgentModelCapabilityV1, AgentResolvedProfileV1, AgentRunStartRequestV1,
};
use hello_agent_protocol::run_recovery_v1::{validate_g5_payload_v1, G5_RUN_START_CHAT_STATES_V1};
use hello_agent_protocol::{
    AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1, AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1,
    AGENT_PUBLIC_LEDGER_CUMULATIVE_SNAPSHOT_BYTES_PER_RUN_MAX_V1,
    AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const SOURCE_KIND: &str = "actualAgent";
const RECOVERY_RESERVATION_FIELD: &str = "recovery_id_reservation";
const RECOVERY_INTENT_FIELD: &str = "recovery_resolution_intent";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderMode {
    Blocked,
    #[cfg_attr(not(test), allow(dead_code))]
    FakeExact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FakeScenario {
    Clean,
    #[cfg_attr(not(test), allow(dead_code))]
    NoOutput,
    #[cfg_attr(not(test), allow(dead_code))]
    FinalDelayed,
    #[cfg_attr(not(test), allow(dead_code))]
    CancelClean,
    CancelUncertain,
    #[cfg_attr(not(test), allow(dead_code))]
    Malformed,
    #[cfg_attr(not(test), allow(dead_code))]
    Oversized,
    #[cfg_attr(not(test), allow(dead_code))]
    Descendant,
    #[cfg_attr(not(test), allow(dead_code))]
    SpawnFailure,
    #[cfg_attr(not(test), allow(dead_code))]
    IdentityFailure,
    #[cfg_attr(not(test), allow(dead_code))]
    ObserveFailure,
    #[cfg_attr(not(test), allow(dead_code))]
    ReleaseFailure,
    #[cfg_attr(not(test), allow(dead_code))]
    PollObserveFailure,
    #[cfg_attr(not(test), allow(dead_code))]
    PollWaitFailure,
    #[cfg_attr(not(test), allow(dead_code))]
    TerminalConstructionFailure,
    #[cfg_attr(not(test), allow(dead_code))]
    TerminalUpsertFailure,
    DiscoveryMissingHelper,
    DiscoveryUnknownVersion,
    DiscoveryUnknownAuth,
    DiscoveryStaleClosure,
    DiscoveryProfileDrift,
    HandshakeBeforePublic,
    HandshakeVersionMismatch,
    HandshakeModelMismatch,
    HandshakeCwdMismatch,
    HandshakePermissionMismatch,
    HandshakeToolsMismatch,
    HandshakeMcpMismatch,
    HandshakePluginsMismatch,
    HandshakeConfigMismatch,
    AttestationPersistenceFailure,
    ProviderAuthFailure,
    ProviderTerminalMalformed,
    PublicTextExact,
    PublicAggregateExact,
    PublicAggregatePlusOne,
    ProviderTerminalFailed,
    ProviderTerminalRecoveryRequired,
}

struct ActiveAgentRun {
    launcher: Child,
    tracker: TreeTracker,
    receiver: Receiver<Result<Vec<u8>, String>>,
    identity: RunIdentity,
    profile: AgentResolvedProfileV1,
    capability_fingerprint: String,
    stream_id: String,
    next_wire_seq: u64,
    running_event_pending: bool,
    scenario: FakeScenario,
    evidence_failed: bool,
    provider_classifier: ProviderEventClassifierV1,
    provider_terminal_outcome: Option<String>,
}

enum PublicEventPersistError {
    BudgetExceeded,
    Fault(ProjectFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryResolutionFailurePoint {
    BeforeIntentCommit,
    AfterIntentBeforeFilesystem,
    AfterFilesystemBeforeFinalize,
    BeforeFinalizeCommit,
    AfterFinalizeBeforeResponse,
}

#[derive(Debug, Clone)]
pub(crate) struct RunIdentity {
    pub(crate) chat_id: String,
    pub(crate) task_id: String,
    pub(crate) project_id: String,
    pub(crate) worktree_id: String,
    pub(crate) workspace_identity: String,
    pub(crate) run_id: String,
}

pub(crate) struct AgentRuntime {
    provider: ProviderMode,
    fake_executable: Option<PathBuf>,
    scenario: FakeScenario,
    active: HashMap<String, ActiveAgentRun>,
    preparing_to_quit: bool,
    discovery_cache: HashMap<String, ObservedProvider>,
    observation_counts: HashMap<String, usize>,
}

#[derive(Clone)]
struct ObservedProvider {
    definition: ProviderDefinitionV1,
    discovery: SanitizedDiscoveryObservationV1,
    capability: AgentCapabilitySnapshotV1,
}

impl AgentRuntime {
    pub(crate) fn blocked() -> Self {
        Self {
            provider: ProviderMode::Blocked,
            fake_executable: None,
            scenario: FakeScenario::Clean,
            active: HashMap::new(),
            preparing_to_quit: false,
            discovery_cache: HashMap::new(),
            observation_counts: HashMap::new(),
        }
    }

    pub(crate) fn recover_unclean_runs(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Result<(), SidecarError> {
        for projection in storage.projection_records("run")? {
            if projection.get("source_kind").and_then(Value::as_str) != Some(SOURCE_KIND)
                || projection
                    .get("recovery")
                    .is_some_and(|value| !value.is_null())
                || !projection
                    .get("state")
                    .and_then(Value::as_str)
                    .is_some_and(|state| {
                        ["preparing", "running", "cancelling", "recoveryRequired"].contains(&state)
                    })
            {
                continue;
            }
            let projection = ensure_recovery_reservation(storage, &projection)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let recovery_id = reserved_recovery_id(&projection)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let public_events =
                projection["public_events"]
                    .as_array()
                    .cloned()
                    .ok_or_else(|| {
                        SidecarError::new("recoveryInvalid", "public event ledger is missing")
                    })?;
            public_budget_metrics(&public_events)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let run_id = projection["run_id"].as_str().unwrap_or_default();
            let chat_id = projection["chat_id"].as_str().unwrap_or_default();
            let chat_projection = actual_chat_projection(storage, chat_id)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let chat = chat_projection["record"].clone();
            let profile: AgentResolvedProfileV1 =
                serde_json::from_value(projection["record"]["selectedProfile"].clone())
                    .map_err(|error| SidecarError::new("recoveryInvalid", error.to_string()))?;
            let tracked = projection["barrier"]["trackedProcesses"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut value| {
                    value["observedGone"] = Value::Bool(false);
                    value
                })
                .collect::<Vec<_>>();
            let run = agent_run_record(
                run_id,
                &chat,
                &profile,
                projection["record"]["capabilityFingerprint"]
                    .as_str()
                    .unwrap_or_default(),
                "recoveryRequired",
                Some("recoveryRequired"),
                projection["record"]["createdAt"]
                    .as_str()
                    .unwrap_or_default(),
            )
            .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let identity_missing = tracked.is_empty();
            let (termination, recovery) = if identity_missing {
                let recovery = json!({"schemaVersion":1,"recoveryId":recovery_id,"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"source":"actualAgent","reason":"processIdentityUnavailable","state":"unresolved","trackedProcesses":[],"poisoned":true,"revision":1,"createdAt":now(),"resolvedAt":null});
                validate_agent_adapter_payload_v1("recoveryRecord", &recovery)
                    .map_err(|error| SidecarError::new("recoveryInvalid", error.to_string()))?;
                (Value::Null, recovery)
            } else {
                let termination = json!({"schemaVersion":1,"runId":run_id,"outcome":"recoveryRequired","processesProvenGone":false,"poisoned":true,"writerLockState":"recoveryRequired","chatState":"recoveryRequired","trackedProcesses":tracked,"evidenceDigest":digest_text("unclean-agent-recovery-v1"),"observedAt":now()});
                validate_agent_adapter_payload_v1("terminationTranscript", &termination)
                    .map_err(|error| SidecarError::new("recoveryInvalid", error.to_string()))?;
                let recovery = json!({"schemaVersion":1,"recoveryId":recovery_id,"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"source":"actualAgent","reason":"uncleanShutdown","state":"unresolved","trackedProcesses":tracked,"poisoned":true,"revision":1,"createdAt":now(),"resolvedAt":null});
                validate_agent_adapter_payload_v1("recoveryRecord", &recovery)
                    .map_err(|error| SidecarError::new("recoveryInvalid", error.to_string()))?;
                (termination, recovery)
            };
            let address = address_from_chat(&chat)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            projects
                .workspace_restore_recovery_required(&address)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let recovered_chat = updated_chat(&chat, "recoveryRequired")
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let mut run_payload = agent_run_projection(
                &run,
                &projection["workspace_manifest"],
                projection.get("barrier").cloned(),
                projection.get("process_identity_internal").cloned(),
                public_events,
            )
            .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            preserve_provider_evidence(&projection, &mut run_payload);
            preserve_recovery_internal(&projection, &mut run_payload);
            run_payload.insert("termination".into(), termination);
            run_payload.insert("recovery".into(), recovery);
            let mut upserts = vec![
                ProjectionUpsert {
                    aggregate_type: "run",
                    aggregate_id: run_id.into(),
                    operation_id: format!("op_agent_unclean_{run_id}"),
                    request_id: None,
                    payload: run_payload,
                },
                ProjectionUpsert {
                    aggregate_type: "chat",
                    aggregate_id: chat_id.into(),
                    operation_id: format!("op_agent_unclean_chat_{run_id}"),
                    request_id: None,
                    payload: chat_projection_with_record(&chat_projection, &recovered_chat, None)
                        .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?,
                },
            ];
            if profile.permission == "workspaceWrite" {
                upserts.push(
                    release_writer_lock(storage, &address, run_id, run_id, true)
                        .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?,
                );
            }
            storage.upsert_projection_records_atomic(upserts)?;
        }
        self.reconcile_recovery_intents(storage, projects)
    }

    fn reconcile_recovery_intents(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Result<(), SidecarError> {
        let pending = storage
            .projection_records("run")?
            .into_iter()
            .filter(|projection| {
                projection.get("source_kind").and_then(Value::as_str) == Some(SOURCE_KIND)
                    && projection
                        .get(RECOVERY_INTENT_FIELD)
                        .is_some_and(|value| !value.is_null())
            })
            .collect::<Vec<_>>();
        for projection in pending {
            let intent = projection[RECOVERY_INTENT_FIELD].clone();
            validate_recovery_intent(&projection, &intent)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let address = address_from_recovery_intent(&intent)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let run_id = identity_string(&intent, "runId")
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            projects
                .workspace_ensure_recovery_abandoned(
                    &address,
                    &intent["expectedManifest"],
                    intent["expectedManifestDigest"]
                        .as_str()
                        .unwrap_or_default(),
                )
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            finalize_recovery_intent(storage, projects, &address, &run_id, "startup", false)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
        }
        Ok(())
    }

    pub(crate) fn validate_persisted_runs(&self, storage: &Storage) -> Result<(), SidecarError> {
        for projection in storage.projection_records("run")? {
            if projection.get("source_kind").and_then(Value::as_str) == Some(SOURCE_KIND) {
                validate_projection_public_budget(&projection)
                    .map_err(|error| SidecarError::new("projectionInvalid", error.detail))?;
                validate_provider_projection(&projection)
                    .map_err(|error| SidecarError::new("projectionInvalid", error.detail))?;
                if projection
                    .get("recovery")
                    .is_some_and(|value| !value.is_null())
                {
                    let reserved = reserved_recovery_id(&projection)
                        .map_err(|error| SidecarError::new("projectionInvalid", error.detail))?;
                    if projection["recovery"]["recoveryId"] != reserved {
                        return Err(SidecarError::new(
                            "projectionInvalid",
                            "actual-agent recovery differs from its reserved ID",
                        ));
                    }
                }
                if let Some(intent) = projection
                    .get(RECOVERY_INTENT_FIELD)
                    .filter(|value| !value.is_null())
                {
                    validate_recovery_intent(&projection, intent)
                        .map_err(|error| SidecarError::new("projectionInvalid", error.detail))?;
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn has_active_run(&self, run_id: &str) -> bool {
        self.active.contains_key(run_id)
    }

    #[cfg(test)]
    pub(crate) fn reserve_legacy_recovery_for_test(
        &self,
        storage: &mut Storage,
        run_id: &str,
        fail_before_commit: bool,
    ) -> Result<Map<String, Value>, ProjectFailure> {
        let projection = actual_run_projection(storage, run_id)?;
        ensure_recovery_reservation_inner(storage, &projection, fail_before_commit)
    }

    pub(crate) fn fake(executable: PathBuf, scenario: FakeScenario) -> Self {
        Self {
            provider: ProviderMode::FakeExact,
            fake_executable: Some(executable),
            scenario,
            active: HashMap::new(),
            preparing_to_quit: false,
            discovery_cache: HashMap::new(),
            observation_counts: HashMap::new(),
        }
    }

    pub(crate) fn advertises_g5a(&self) -> bool {
        self.provider == ProviderMode::FakeExact
    }

    pub(crate) fn get_capabilities(
        &mut self,
        runtime: &str,
        refresh: bool,
    ) -> Result<Value, ProjectFailure> {
        let request = json!({"runtime":runtime,"refresh":refresh});
        validate_agent_adapter_payload_v1("capabilitiesRequest", &request)
            .map_err(contract_failure)?;
        let snapshot = if self.provider == ProviderMode::Blocked {
            self.blocked_capability(runtime)?
        } else if !refresh {
            match self.discovery_cache.get(runtime) {
                Some(observed) => observed.capability.clone(),
                None => self.observe_provider(runtime)?.capability,
            }
        } else {
            self.observe_provider(runtime)?.capability
        };
        let value = serde_json::to_value(snapshot).map_err(json_failure)?;
        validate_agent_adapter_payload_v1("capabilitySnapshot", &value)
            .map_err(contract_failure)?;
        Ok(value)
    }

    pub(crate) fn start_agent(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        request: AgentRunStartRequestV1,
        request_id: &str,
    ) -> Result<G5Action, ProjectFailure> {
        validate_agent_adapter_payload_v1(
            "runStartRequest",
            &serde_json::to_value(&request).map_err(json_failure)?,
        )
        .map_err(contract_failure)?;
        if self.preparing_to_quit {
            return Err(ProjectFailure::recovery("sidecar is preparing to quit"));
        }
        if active_run_limit_reached(self.active.len()) {
            return Err(ProjectFailure::recovery(
                "actual-agent session active-run limit reached before spawn or lock",
            ));
        }
        let chat_projection = actual_chat_projection(storage, &request.chat_id)?;
        let chat = chat_projection["record"].clone();
        let profile: AgentResolvedProfileV1 = serde_json::from_value(
            chat["roleSnapshots"]
                .as_array()
                .and_then(|roles| roles.iter().find(|role| role["roleId"] == request.role_id))
                .cloned()
                .ok_or_else(|| ProjectFailure::manifest("chat role snapshot is missing"))?,
        )
        .map_err(json_failure)?;
        // Every start bypasses the UI cache. The exact executable closure, version, auth state,
        // catalog and immutable role tuple are re-observed before a run or writer lock exists.
        let observed_provider = if self.provider == ProviderMode::FakeExact {
            Some(self.observe_provider(&profile.runtime)?)
        } else {
            None
        };
        let capability = match observed_provider.as_ref() {
            Some(observed) => observed.capability.clone(),
            None => self.blocked_capability(&profile.runtime)?,
        };
        let outcome = preflight_outcome(&request, &profile, &capability);
        if outcome != "started" {
            return Err(ProjectFailure::manifest(format!(
                "agent preflight rejected before run, spawn, or lock: {outcome}"
            )));
        }
        let observed_provider = observed_provider.ok_or_else(|| {
            ProjectFailure::recovery("startable provider evidence is unavailable after preflight")
        })?;
        if !G5_RUN_START_CHAT_STATES_V1.contains(&chat["state"].as_str().unwrap_or_default()) {
            return Err(ProjectFailure::recovery(
                "chat state does not allow a new agent run",
            ));
        }
        if storage.projection_records("run")?.iter().any(|run| {
            run.get("chat_id").and_then(Value::as_str) == Some(request.chat_id.as_str())
                && run
                    .get("state")
                    .and_then(Value::as_str)
                    .is_some_and(|state| {
                        ["preparing", "running", "cancelling", "recoveryRequired"].contains(&state)
                    })
        }) {
            return Err(ProjectFailure::recovery(
                "chat already has an active or poisoned run",
            ));
        }
        let address = address_from_chat(&chat)?;
        let manifest = projects.verify_workspace(storage, &address)?;
        let prompt = resolve_prompt(storage, &request.chat_id, &request.input_message_ids)?;
        let executable = self
            .fake_executable
            .as_ref()
            .ok_or_else(|| ProjectFailure::manifest("verified runtime adapter is unavailable"))?;
        let launch_spec = LaunchSpec::new_internal(
            executable.clone(),
            vec![
                "--agent-fixture-launcher".into(),
                scenario_name(self.scenario).into(),
            ],
            PathBuf::from(
                manifest["worktreePath"]
                    .as_str()
                    .ok_or_else(|| ProjectFailure::manifest("workspace path is missing"))?,
            ),
            BTreeMap::new(),
            prompt,
            "claude-code.system-init.v1",
            "claude-code.stream-json.v1",
        )
        .map_err(adapter_failure)?;

        let run_id = format!("agent_run_{}", Uuid::new_v4().simple());
        let recovery_id = new_recovery_id();
        let created = now();
        let run = agent_run_record(
            &run_id,
            &chat,
            &profile,
            &capability.fingerprint,
            "preparing",
            None,
            &created,
        )?;
        let mut preparing_projection =
            agent_run_projection(&run, &manifest, None, None, Vec::new())?;
        preparing_projection.insert(
            RECOVERY_RESERVATION_FIELD.into(),
            Value::String(recovery_id),
        );
        let mut upserts = vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: format!("op_agent_run_prepare_{request_id}"),
            request_id: Some(request_id.to_string()),
            payload: preparing_projection,
        }];
        if profile.permission == "workspaceWrite" {
            upserts.push(acquire_writer_lock(storage, &chat, &run_id, request_id)?);
        }
        storage.upsert_projection_records_atomic(upserts)?;

        if self.scenario == FakeScenario::SpawnFailure {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                None,
                None,
                "injected spawn failure",
            ));
        }

        let mut command = Command::new(launch_spec.executable());
        command
            .args(launch_spec.argv())
            .current_dir(launch_spec.cwd())
            .env_clear()
            .envs(launch_spec.environment())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut launcher = match command.spawn() {
            Ok(launcher) => launcher,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    None,
                    None,
                    &format!("fake agent control launcher failed: {error}"),
                ))
            }
        };
        let launcher_identity = match identity_of(launcher.id()) {
            Ok(identity) => identity,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    None,
                    &format!("launcher identity unavailable: {error}"),
                ))
            }
        };
        let mut tracker = TreeTracker::new(launcher_identity);
        if let Err(error) = tracker.observe(&SystemProcTable) {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                &format!("launcher tree unavailable: {error}"),
            ));
        }
        let stdout = match launcher.stdout.take() {
            Some(stdout) => stdout,
            None => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    "launcher stdout barrier is missing",
                ))
            }
        };
        let mut reader = BufReader::new(stdout);
        let ready = match read_bounded_line(&mut reader) {
            Ok(ready) => ready,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &error,
                ))
            }
        };
        let agent_pid = match parse_ready(&ready) {
            Ok(pid) => pid,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &error.detail,
                ))
            }
        };
        let agent_identity = match identity_of(agent_pid) {
            Ok(identity) => identity,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &format!("agent identity unavailable before release: {error}"),
                ))
            }
        };
        tracker.track_identity(agent_identity);
        for _ in 0..10 {
            let observed = if self.scenario == FakeScenario::ObserveFailure {
                Err("injected attach observation failure".to_string())
            } else {
                tracker.observe(&SystemProcTable)
            };
            if let Err(error) = observed {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &format!("agent attach observation failed: {error}"),
                ));
            }
            if tracker.tracked_len() >= 2 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if tracker.tracked_len() < 2 {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                "agent child was not attached before release",
            ));
        }
        if self.scenario == FakeScenario::IdentityFailure {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                "injected identity failure",
            ));
        }
        let configured = (|| -> Result<_, ProjectFailure> {
            let public_launcher = process_value(launcher_identity, launch_spec.executable(), false);
            let public_agent = process_value(agent_identity, launch_spec.executable(), false);
            let barrier = json!({
                "launcherIdentity":public_launcher,"agentIdentity":public_agent,
                "trackedProcesses":[public_launcher,public_agent],
                "treeIdentityDigest":digest_text(&format!("{}:{}:{}:{}",launcher_identity.pid,launcher_identity.start_us,agent_identity.pid,agent_identity.start_us)),
                "launchSpecDigest":digest_bytes(&launch_spec.redacted_digest_material()),
                "evidenceDigest":digest_text("agent-fixture-barrier-v1"),
                "treeTrackerAttached":true,"runAndLockPersistedBeforeRelease":true,
                "identityPersistedBeforeRelease":true,"releaseGranted":false,"capturedAt":now(),
            });
            let internal = json!([
                {"pid":launcher_identity.pid,"start_us":launcher_identity.start_us},
                {"pid":agent_identity.pid,"start_us":agent_identity.start_us}
            ]);
            let mut preparing_payload = agent_run_projection(
                &run,
                &manifest,
                Some(barrier.clone()),
                Some(internal),
                Vec::new(),
            )?;
            preserve_recovery_internal(
                &actual_run_projection(storage, &run_id)?,
                &mut preparing_payload,
            );
            Ok((barrier, preparing_payload))
        })();
        let (barrier, preparing_payload) = match configured {
            Ok(configured) => configured,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &format!("pre-release evidence construction failed: {}", error.detail),
                ))
            }
        };
        if let Err(error) = storage.upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: format!("op_agent_identity_{run_id}"),
            request_id: None,
            payload: preparing_payload,
        }]) {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                &format!("identity barrier persistence failed: {error}"),
            ));
        }

        if self.scenario == FakeScenario::ReleaseFailure {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                "injected release failure",
            ));
        }

        let mut release = match launcher.stdin.take() {
            Some(release) => release,
            None => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    "launcher release pipe is missing",
                ))
            }
        };
        if let Err(error) = release.write_all(b"run\n") {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                &format!("launcher release failed: {error}"),
            ));
        }
        drop(release);
        let released = (|| -> Result<(), ProjectFailure> {
            let mut projection = actual_run_projection(storage, &run_id)?;
            let mut released_barrier = barrier;
            released_barrier["releaseGranted"] = Value::Bool(true);
            released_barrier["capturedAt"] = Value::String(now());
            projection.insert("barrier".into(), released_barrier);
            projection.remove("last_event_sequence");
            storage.upsert_projection_records_atomic(vec![ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run_id.clone(),
                operation_id: format!("op_agent_release_{run_id}"),
                request_id: None,
                payload: projection,
            }])?;
            Ok(())
        })();
        if let Err(error) = released {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                &format!("release evidence persistence failed: {}", error.detail),
            ));
        }
        let accepted = json!({"schemaVersion":1,"runId":run_id,"chatId":request.chat_id,"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"roleId":request.role_id,"capabilityFingerprint":capability.fingerprint,"state":"preparing"});
        if let Err(error) = validate_agent_adapter_payload_v1("runAccepted", &accepted) {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                &format!("run accepted evidence failed: {error}"),
            ));
        }
        let handshake = match read_bounded_line(&mut reader) {
            Ok(line) if !line.is_empty() => line,
            Ok(_) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    "provider handshake is missing after release",
                ))
            }
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &error,
                ))
            }
        };
        let mut provider_classifier = observed_provider.definition.event_classifier();
        let provider_profile = ProviderProfileTupleV1 {
            model_selector: profile.model_selector.clone(),
            reasoning_effort: profile.reasoning_effort.clone(),
            permission: profile.permission.clone(),
        };
        let managed_cwd_identity = digest_path(launch_spec.cwd());
        let handshake_observation = match BoundedObservation::new(handshake) {
            Ok(value) => value,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &format!("provider handshake observation rejected: {error}"),
                ))
            }
        };
        let attested = provider_classifier.accept_claude_handshake(
            &observed_provider.discovery,
            handshake_observation,
            ClaudeHandshakeContextV1 {
                launch_spec: &launch_spec,
                run_id: run_id.clone(),
                capability_fingerprint: capability.fingerprint.clone(),
                effective_profile: provider_profile,
                workspace_identity: chat["workspaceIdentity"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                managed_cwd_identity,
                expected_tools: vec!["Read".into(), "Edit".into()],
                expected_mcp_servers: Vec::new(),
                expected_plugins: Vec::new(),
                verified_at: now(),
            },
        );
        let (attestation, handshake_event) = match attested {
            Ok(value) => value,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &format!("provider handshake rejected: {error}"),
                ))
            }
        };
        if !matches!(
            handshake_event,
            InternalProviderEventV1::Handshake { sequence: 1, .. }
        ) {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                "provider handshake classifier did not start at sequence one",
            ));
        }
        let attestation_projection = (|| -> Result<_, ProjectFailure> {
            let manifest_value = serde_json::to_value(observed_provider.definition.manifest())
                .map_err(json_failure)?;
            let discovery_value =
                serde_json::to_value(&observed_provider.discovery).map_err(json_failure)?;
            let attestation_value = serde_json::to_value(&attestation).map_err(json_failure)?;
            validate_agent_runtime_attestation_against_accepted_v1(
                &attestation_value,
                &manifest_value,
                &discovery_value,
                &accepted,
                &serde_json::to_value(&profile).map_err(json_failure)?,
            )
            .map_err(contract_failure)?;
            let running = agent_run_record(
                &run_id,
                &chat,
                &profile,
                &capability.fingerprint,
                "running",
                None,
                &created,
            )?;
            let mut running_payload = actual_run_projection(storage, &run_id)?;
            running_payload.remove("last_event_sequence");
            running_payload.insert("record".into(), running);
            running_payload.insert("state".into(), Value::String("running".into()));
            running_payload.insert("provider_discovery".into(), discovery_value);
            running_payload.insert("runtime_attestation".into(), attestation_value);
            let running_chat = updated_chat(&chat, "running")?;
            let running_chat_payload =
                chat_projection_with_record(&chat_projection, &running_chat, None)?;
            Ok((running_payload, running_chat_payload))
        })();
        let (running_payload, running_chat_payload) = match attestation_projection {
            Ok(value) => value,
            Err(error) => {
                return Err(fail_before_release(
                    storage,
                    projects,
                    &run_id,
                    Some(launcher),
                    Some(tracker),
                    &format!("runtime attestation construction failed: {}", error.detail),
                ))
            }
        };
        if self.scenario == FakeScenario::AttestationPersistenceFailure {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                "injected runtime attestation persistence failure",
            ));
        }
        if let Err(error) = storage.upsert_projection_records_atomic(vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run_id.clone(),
                operation_id: format!("op_agent_attested_running_{run_id}"),
                request_id: None,
                payload: running_payload,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: request.chat_id.clone(),
                operation_id: format!("op_agent_chat_running_{run_id}"),
                request_id: None,
                payload: running_chat_payload,
            },
        ]) {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                &format!("runtime attestation persistence failed: {error}"),
            ));
        }
        let (sender, receiver) = mpsc::sync_channel(32);
        if let Err(error) = thread::Builder::new()
            .name("agent-fixture-drain".into())
            .spawn(move || loop {
                match read_bounded_line(&mut reader) {
                    Ok(line) if line.is_empty() => break,
                    Ok(line) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            })
        {
            return Err(fail_before_release(
                storage,
                projects,
                &run_id,
                Some(launcher),
                Some(tracker),
                &format!("agent drain thread failed: {error}"),
            ));
        }
        let stream_id = format!("agent_stream_{}", Uuid::new_v4().simple());
        self.active.insert(
            run_id.clone(),
            ActiveAgentRun {
                launcher,
                tracker,
                receiver,
                identity: RunIdentity {
                    chat_id: request.chat_id.clone(),
                    task_id: chat["taskId"].as_str().unwrap_or_default().to_string(),
                    project_id: chat["projectId"].as_str().unwrap_or_default().to_string(),
                    worktree_id: chat["worktreeId"].as_str().unwrap_or_default().to_string(),
                    workspace_identity: chat["workspaceIdentity"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    run_id: run_id.clone(),
                },
                profile: profile.clone(),
                capability_fingerprint: capability.fingerprint.clone(),
                stream_id: stream_id.clone(),
                next_wire_seq: 0,
                running_event_pending: true,
                scenario: self.scenario,
                evidence_failed: false,
                provider_classifier,
                provider_terminal_outcome: None,
            },
        );
        Ok(G5Action {
            result: accepted,
            events: Vec::new(),
        })
    }

    pub(crate) fn poll(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Result<Vec<G5Event>, SidecarError> {
        let ids = self.active.keys().cloned().collect::<Vec<_>>();
        let mut events = Vec::new();
        for run_id in ids {
            if self
                .active
                .get(&run_id)
                .is_some_and(|active| active.running_event_pending)
            {
                let event = self
                    .state_event(storage, &run_id)
                    .map_err(|error| self.fail_poll(storage, projects, &run_id, error))?;
                events.push(event);
            }
            let projection = actual_run_projection(storage, &run_id)
                .map_err(|error| self.fail_poll(storage, projects, &run_id, error))?;
            let persisted_budget = stored_public_budget(&projection)
                .map_err(|error| self.fail_poll(storage, projects, &run_id, error))?;
            let persisted_events = projection["public_events"].as_array().ok_or_else(|| {
                self.fail_poll(
                    storage,
                    projects,
                    &run_id,
                    ProjectFailure::recovery("actual-agent public event ledger is missing"),
                )
            })?;
            let mut public =
                PublicEventCollector::new(&projection, persisted_events, persisted_budget);
            let mut parser_failed = false;
            let mut budget_exceeded = false;
            let mut observed_identities = None;
            let mut observation_failure = None;
            let mut collection_failure = None;
            if let Some(active) = self.active.get_mut(&run_id) {
                let observed = if active.scenario == FakeScenario::PollObserveFailure {
                    Err("injected poll observation failure".to_string())
                } else {
                    active.tracker.observe(&SystemProcTable)
                };
                if let Err(error) = observed {
                    active.evidence_failed = true;
                    observation_failure = Some(ProjectFailure::recovery(format!(
                        "agent process-tree observation failed: {error}"
                    )));
                }
                observed_identities = Some(active.tracker.tracked_identities());
                loop {
                    match active.receiver.try_recv() {
                        Ok(Ok(line)) => match classify_provider_line(active, line) {
                            Ok(ProviderLine::Public { kind, text }) => {
                                match public.push(active, &kind, text) {
                                    Ok(true) => {}
                                    Ok(false) => {
                                        budget_exceeded = true;
                                        break;
                                    }
                                    Err(error) => {
                                        collection_failure = Some(error);
                                        break;
                                    }
                                }
                            }
                            Ok(ProviderLine::Private | ProviderLine::Terminal) => {}
                            Err(_) => {
                                parser_failed = true;
                                break;
                            }
                        },
                        Ok(Err(_)) => {
                            parser_failed = true;
                            break;
                        }
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                    }
                }
            }
            if let Some(error) = observation_failure {
                return Err(self.fail_poll(storage, projects, &run_id, error));
            }
            if let Some(error) = collection_failure.take() {
                return Err(self.fail_poll(storage, projects, &run_id, error));
            }
            if let Some(identities) = observed_identities {
                persist_tracker_snapshot(storage, &run_id, &identities)
                    .map_err(|error| self.fail_poll(storage, projects, &run_id, error))?;
            }
            let finished = match self.active.get_mut(&run_id) {
                Some(active) => match if active.scenario == FakeScenario::PollWaitFailure {
                    Err(std::io::Error::other("injected poll wait failure"))
                } else {
                    active.launcher.try_wait()
                } {
                    Ok(status) => status,
                    Err(error) => {
                        return Err(self.fail_poll(
                            storage,
                            projects,
                            &run_id,
                            ProjectFailure::recovery(format!(
                                "agent launcher wait failed: {error}"
                            )),
                        ))
                    }
                },
                None => {
                    return Err(SidecarError::new(
                        "agentRuntimeFailure",
                        "active agent run disappeared",
                    ))
                }
            };
            if finished.is_some() {
                if let Some(active) = self.active.get_mut(&run_id) {
                    loop {
                        match active.receiver.recv_timeout(Duration::from_millis(100)) {
                            Ok(Ok(line)) => match classify_provider_line(active, line) {
                                Ok(ProviderLine::Public { kind, text }) => {
                                    match public.push(active, &kind, text) {
                                        Ok(true) => {}
                                        Ok(false) => {
                                            budget_exceeded = true;
                                            break;
                                        }
                                        Err(error) => {
                                            collection_failure = Some(error);
                                            break;
                                        }
                                    }
                                }
                                Ok(ProviderLine::Private | ProviderLine::Terminal) => {}
                                Err(_) => {
                                    parser_failed = true;
                                    break;
                                }
                            },
                            Ok(Err(_)) | Err(RecvTimeoutError::Timeout) => {
                                parser_failed = true;
                                break;
                            }
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    }
                }
            }
            if let Some(error) = collection_failure {
                return Err(self.fail_poll(storage, projects, &run_id, error));
            }
            for event in public.into_events() {
                match self.persist_public_event(storage, &run_id, event) {
                    Ok(event) => events.push(event),
                    Err(PublicEventPersistError::BudgetExceeded) => {
                        budget_exceeded = true;
                        break;
                    }
                    Err(PublicEventPersistError::Fault(error)) => {
                        return Err(self.fail_poll(storage, projects, &run_id, error));
                    }
                }
            }
            if parser_failed || budget_exceeded || finished.is_some() {
                let provider_outcome = self
                    .active
                    .get(&run_id)
                    .and_then(|active| active.provider_terminal_outcome.as_deref());
                let state = if provider_outcome == Some("recoveryRequired") {
                    "recoveryRequired"
                } else if parser_failed
                    || budget_exceeded
                    || finished.is_some_and(|status| !status.success())
                {
                    "failed"
                } else {
                    match provider_outcome {
                        Some("failed") => "failed",
                        Some("canceled") => "canceled",
                        _ => "succeeded",
                    }
                };
                let terminal = self
                    .finish(storage, projects, &run_id, state, false)
                    .map_err(|error| {
                        SidecarError::new(
                            "agentRuntimeFailure",
                            format!("agent terminal persistence failed: {}", error.detail),
                        )
                    })?;
                events.push(terminal);
            }
        }
        Ok(events)
    }

    pub(crate) fn cancel(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        identity: &RunIdentity,
    ) -> Result<G5Action, ProjectFailure> {
        let active = self
            .active
            .get(&identity.run_id)
            .ok_or_else(|| ProjectFailure::manifest("active agent run is missing"))?;
        if !same_identity(&active.identity, identity) {
            return Err(ProjectFailure::manifest("agent run identity differs"));
        }
        let mut events = Vec::new();
        if active.running_event_pending {
            events.push(self.state_event(storage, &identity.run_id)?);
        }
        let terminal_event = self.finish(storage, projects, &identity.run_id, "canceled", true)?;
        events.push(terminal_event);
        let projection = actual_run_projection(storage, &identity.run_id)?;
        let run = projection["record"].clone();
        let termination = projection["termination"].clone();
        let recovery = projection.get("recovery").cloned().unwrap_or(Value::Null);
        let outcome = if run["state"] == "recoveryRequired" {
            "recoveryRequired"
        } else {
            "canceled"
        };
        let result = json!({"schemaVersion":1,"request":identity_value(identity),"run":run,"termination":termination,"recovery":recovery,"outcome":outcome});
        validate_agent_adapter_payload_v1("runCancelResult", &result).map_err(contract_failure)?;
        Ok(G5Action { result, events })
    }

    pub(crate) fn inspect_recovery(
        &self,
        storage: &Storage,
        request: &Value,
    ) -> Result<Value, ProjectFailure> {
        validate_agent_adapter_payload_v1("recoveryInspectRequest", request)
            .map_err(contract_failure)?;
        let recovery_id = request["recoveryId"].as_str().unwrap_or_default();
        let recovery = storage
            .projection_records("run")?
            .into_iter()
            .filter(|row| row.get("source_kind").and_then(Value::as_str) == Some(SOURCE_KIND))
            .find_map(|row| {
                row.get("recovery")
                    .filter(|value| value["recoveryId"] == recovery_id)
                    .cloned()
            })
            .ok_or_else(|| ProjectFailure::manifest("agent recovery is missing"))?;
        for field in [
            "chatId",
            "taskId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "runId",
        ] {
            if recovery[field] != request[field] {
                return Err(ProjectFailure::manifest("agent recovery identity differs"));
            }
        }
        Ok(recovery)
    }

    pub(crate) fn resolve_recovery(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        request: &Value,
        request_id: &str,
    ) -> Result<Value, ProjectFailure> {
        self.resolve_recovery_inner(storage, projects, request, request_id, None)
    }

    #[cfg(test)]
    pub(crate) fn resolve_recovery_with_failure(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        request: &Value,
        request_id: &str,
        failure: RecoveryResolutionFailurePoint,
    ) -> Result<Value, ProjectFailure> {
        self.resolve_recovery_inner(storage, projects, request, request_id, Some(failure))
    }

    fn resolve_recovery_inner(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        request: &Value,
        request_id: &str,
        failure: Option<RecoveryResolutionFailurePoint>,
    ) -> Result<Value, ProjectFailure> {
        validate_agent_adapter_payload_v1("recoveryResolveRequest", request)
            .map_err(contract_failure)?;
        let before = self.inspect_recovery(storage, &json!({
            "recoveryId":request["recoveryId"],"chatId":request["chatId"],"taskId":request["taskId"],
            "projectId":request["projectId"],"worktreeId":request["worktreeId"],
            "workspaceIdentity":request["workspaceIdentity"],"runId":request["runId"]
        }))?;
        if before["revision"] != request["expectedRevision"] {
            let result = json!({"schemaVersion":1,"before":before,"request":request,"after":null,"outcome":"revisionConflict"});
            validate_agent_adapter_payload_v1("recoveryResolveResult", &result)
                .map_err(contract_failure)?;
            return Ok(result);
        }
        if before["reason"] == "processIdentityUnavailable" {
            let result = json!({"schemaVersion":1,"before":before,"request":request,"after":null,"outcome":"identityUnavailable"});
            validate_agent_adapter_payload_v1("recoveryResolveResult", &result)
                .map_err(contract_failure)?;
            return Ok(result);
        }
        let projection =
            actual_run_projection(storage, request["runId"].as_str().unwrap_or_default())?;
        let projection = ensure_recovery_reservation(storage, &projection)?;
        let mut tracked = before["trackedProcesses"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let internal = projection["process_identity_internal"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut all_gone = tracked.len() == internal.len() && !tracked.is_empty();
        for (public, private) in tracked.iter_mut().zip(internal.iter()) {
            let identity = ProcIdentity {
                pid: private["pid"].as_u64().unwrap_or(0) as u32,
                start_us: private["start_us"].as_i64().unwrap_or(0),
            };
            let gone = matches!(SystemProcTable.probe(identity), ProbeResult::Gone);
            public["observedGone"] = Value::Bool(gone);
            all_gone &= gone;
        }
        if !all_gone {
            let mut current = before.clone();
            current["trackedProcesses"] = Value::Array(tracked);
            let result = json!({"schemaVersion":1,"before":current,"request":request,"after":null,"outcome":"processesRemain"});
            validate_agent_adapter_payload_v1("recoveryResolveResult", &result)
                .map_err(contract_failure)?;
            return Ok(result);
        }
        let mut observed_before = before.clone();
        observed_before["trackedProcesses"] = Value::Array(tracked.clone());
        let address = WorkspaceAddress {
            chat_id: request["chatId"].as_str().unwrap_or_default().into(),
            project_id: request["projectId"].as_str().unwrap_or_default().into(),
            worktree_id: request["worktreeId"].as_str().unwrap_or_default().into(),
            workspace_identity: request["workspaceIdentity"]
                .as_str()
                .unwrap_or_default()
                .into(),
        };
        let mut expected_manifest = projection["workspace_manifest"].clone();
        expected_manifest["lifecycle"] = Value::String("recoveryRequired".into());
        let expected_manifest_digest = recovery_manifest_digest(&expected_manifest)?;
        let intent = json!({
            "schemaVersion":1,"recoveryId":before["recoveryId"],
            "chatId":before["chatId"],"taskId":before["taskId"],
            "projectId":before["projectId"],"worktreeId":before["worktreeId"],
            "workspaceIdentity":before["workspaceIdentity"],"runId":before["runId"],
            "expectedRevision":before["revision"],"resolution":"abandon",
            "trackedProcesses":tracked,"expectedManifest":expected_manifest,
            "expectedManifestDigest":expected_manifest_digest,
        });
        if failure == Some(RecoveryResolutionFailurePoint::BeforeIntentCommit) {
            return Err(ProjectFailure::recovery(
                "injected failure before recovery intent commit",
            ));
        }
        persist_recovery_intent(storage, &projection, &intent)?;
        if failure == Some(RecoveryResolutionFailurePoint::AfterIntentBeforeFilesystem) {
            return Err(ProjectFailure::recovery(
                "injected failure after recovery intent before filesystem",
            ));
        }
        projects.workspace_ensure_recovery_abandoned(
            &address,
            &intent["expectedManifest"],
            intent["expectedManifestDigest"]
                .as_str()
                .unwrap_or_default(),
        )?;
        if failure == Some(RecoveryResolutionFailurePoint::AfterFilesystemBeforeFinalize) {
            return Err(ProjectFailure::recovery(
                "injected failure after filesystem before recovery finalize",
            ));
        }
        let after = finalize_recovery_intent(
            storage,
            projects,
            &address,
            request["runId"].as_str().unwrap_or_default(),
            request_id,
            failure == Some(RecoveryResolutionFailurePoint::BeforeFinalizeCommit),
        )?;
        if failure == Some(RecoveryResolutionFailurePoint::AfterFinalizeBeforeResponse) {
            return Err(ProjectFailure::recovery(
                "injected failure after recovery finalize before response",
            ));
        }
        let result = json!({"schemaVersion":1,"before":observed_before,"request":request,"after":after,"outcome":"resolvedAbandoned"});
        validate_agent_adapter_payload_v1("recoveryResolveResult", &result)
            .map_err(contract_failure)?;
        Ok(result)
    }

    pub(crate) fn prepare_quit(&mut self, storage: &mut Storage, projects: &mut ProjectManager) {
        self.preparing_to_quit = true;
        let ids = self.active.keys().cloned().collect::<Vec<_>>();
        for run_id in ids {
            if let Some(active) = self.active.get(&run_id) {
                let identity = active.identity.clone();
                let _ = self.cancel(storage, projects, &identity);
            }
        }
    }

    fn blocked_capability(
        &self,
        runtime: &str,
    ) -> Result<AgentCapabilitySnapshotV1, ProjectFailure> {
        let platform = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "win32"
        };
        let mut snapshot = AgentCapabilitySnapshotV1 {
            schema_version: 1,
            runtime: runtime.into(),
            platform: platform.into(),
            installation_state: "notInstalled".into(),
            auth_state: "unknown".into(),
            executable: None,
            models: Vec::new(),
            discovery: AgentDiscoveryEvidenceV1 {
                source: "unavailable".into(),
                verification_level: "unknown".into(),
                exact_cli_version: None,
                effective_model_evidence: "unknown".into(),
            },
            can_start: false,
            blocked_reason: Some(
                if platform == "darwin" {
                    "notInstalled"
                } else {
                    "platformUnsupported"
                }
                .into(),
            ),
            fingerprint: String::new(),
            observed_at: now(),
        };
        snapshot.fingerprint =
            compute_agent_capability_fingerprint_v1(&snapshot).map_err(contract_failure)?;
        Ok(snapshot)
    }

    fn observe_provider(&mut self, runtime: &str) -> Result<ObservedProvider, ProjectFailure> {
        let provider = provider_id(runtime)?;
        let executable = self
            .fake_executable
            .as_ref()
            .ok_or_else(|| ProjectFailure::manifest("fake executable missing"))?;
        let approved_closure = fake_observed_closure(provider, executable);
        let definition = ProviderDefinitionV1::new(provider, approved_closure.clone())
            .map_err(provider_failure)?;
        let observation_index = self
            .observation_counts
            .entry(runtime.to_string())
            .or_default();
        *observation_index += 1;
        let mut observed_closure = approved_closure;
        let mut version = match provider {
            ProviderId::ClaudeCode => b"Claude Code 2.1.241".to_vec(),
            ProviderId::Codex => b"codex-cli 0.150.0".to_vec(),
        };
        let mut auth = match provider {
            ProviderId::ClaudeCode => br#"{"loggedIn":true,"email":"private@example.invalid","organization":"private-org","token":"private-token"}"#.to_vec(),
            ProviderId::Codex => b"Logged in using ChatGPT".to_vec(),
        };
        let mut profiles = definition.manifest().exact_profiles.clone();
        if self.scenario == FakeScenario::DiscoveryMissingHelper {
            observed_closure.required_helpers.clear();
        }
        if self.scenario == FakeScenario::DiscoveryUnknownVersion {
            version = match provider {
                ProviderId::ClaudeCode => b"Claude Code 9.9.9".to_vec(),
                ProviderId::Codex => b"codex-cli 9.9.9".to_vec(),
            };
        }
        if self.scenario == FakeScenario::DiscoveryUnknownAuth {
            auth = match provider {
                ProviderId::ClaudeCode => {
                    br#"{"loggedIn":false,"email":"private@example.invalid"}"#.to_vec()
                }
                ProviderId::Codex => b"Account: private@example.invalid".to_vec(),
            };
        }
        if self.scenario == FakeScenario::DiscoveryStaleClosure && *observation_index > 1 {
            observed_closure.binary.file_identity_digest = digest_text("stale-fixture-binary");
        }
        if self.scenario == FakeScenario::DiscoveryProfileDrift && *observation_index > 1 {
            profiles[0].model_selector = "unapproved-model".into();
        }
        let evidence = match provider {
            ProviderId::ClaudeCode => ModelCatalogEvidenceV1::RuntimeVerified,
            ProviderId::Codex => ModelCatalogEvidenceV1::ArgvOnly,
        };
        let discovery = definition
            .discover(DiscoveryInputV1 {
                executable: Some(observed_closure),
                version: Some(BoundedObservation::new(version).map_err(provider_failure)?),
                auth: Some(BoundedObservation::new(auth).map_err(provider_failure)?),
                model_catalog_evidence: evidence,
                exact_profiles: profiles,
                observed_runtime_verification: definition
                    .manifest()
                    .launch_policy
                    .required_runtime_verification
                    .clone(),
                observed_at: now(),
            })
            .map_err(provider_failure)?;
        let manifest_value = serde_json::to_value(definition.manifest()).map_err(json_failure)?;
        let discovery_value = serde_json::to_value(&discovery).map_err(json_failure)?;
        validate_provider_manifest_v1(&manifest_value).map_err(contract_failure)?;
        validate_sanitized_discovery_observation_v1(&discovery_value, &manifest_value)
            .map_err(contract_failure)?;
        let capability = capability_from_discovery(&discovery)?;
        let observed = ObservedProvider {
            definition,
            discovery,
            capability,
        };
        self.discovery_cache
            .insert(runtime.to_string(), observed.clone());
        Ok(observed)
    }

    #[cfg(test)]
    pub(crate) fn observation_count(&self, runtime: &str) -> usize {
        self.observation_counts.get(runtime).copied().unwrap_or(0)
    }

    fn persist_public_event(
        &mut self,
        storage: &mut Storage,
        run_id: &str,
        event: Value,
    ) -> Result<G5Event, PublicEventPersistError> {
        let active = self.active.get_mut(run_id).ok_or_else(|| {
            PublicEventPersistError::Fault(ProjectFailure::manifest("active agent run is missing"))
        })?;
        let projection =
            actual_run_projection(storage, run_id).map_err(PublicEventPersistError::Fault)?;
        let current = stored_public_budget(&projection).map_err(PublicEventPersistError::Fault)?;
        let mut events = projection["public_events"]
            .as_array()
            .cloned()
            .ok_or_else(|| {
                PublicEventPersistError::Fault(ProjectFailure::recovery(
                    "actual-agent public event ledger is missing",
                ))
            })?;
        validate_agent_adapter_payload_v1("publicEvent", &event)
            .map_err(contract_failure)
            .map_err(PublicEventPersistError::Fault)?;
        validate_public_event_binding(&event, active).map_err(PublicEventPersistError::Fault)?;
        if projection["record"]["capabilityFingerprint"] != active.capability_fingerprint {
            return Err(PublicEventPersistError::Fault(ProjectFailure::recovery(
                "durable agent run fingerprint differs from the accepted run",
            )));
        }
        let event_text_bytes = event["text"]
            .as_str()
            .ok_or_else(|| {
                PublicEventPersistError::Fault(ProjectFailure::recovery(
                    "validated public event text is missing",
                ))
            })?
            .len();
        let event_count = current
            .event_count
            .checked_add(1)
            .ok_or(PublicEventPersistError::BudgetExceeded)?;
        let text_bytes = current
            .text_bytes
            .checked_add(event_text_bytes)
            .ok_or(PublicEventPersistError::BudgetExceeded)?;
        events.push(event.clone());
        let next_snapshot_bytes = agent_public_events_snapshot_bytes_v1(&events)
            .map_err(contract_failure)
            .map_err(PublicEventPersistError::Fault)?;
        let cumulative_snapshot_bytes = current
            .cumulative_snapshot_bytes
            .checked_add(next_snapshot_bytes)
            .ok_or(PublicEventPersistError::BudgetExceeded)?;
        let next_budget = PublicBudgetMetrics {
            event_count,
            text_bytes,
            cumulative_snapshot_bytes,
        };
        if enforce_public_budget(next_budget).is_err() {
            return Err(PublicEventPersistError::BudgetExceeded);
        }
        let mut updated = projection;
        updated.remove("last_event_sequence");
        updated.insert("public_events".into(), Value::Array(events));
        apply_public_budget(&mut updated, next_budget);
        let mut chat_projection = actual_chat_projection(storage, &active.identity.chat_id)
            .map_err(PublicEventPersistError::Fault)?;
        let sequence = chat_projection["durable_event_sequence"]
            .as_u64()
            .unwrap_or(0)
            + 1;
        let kind = event["kind"].as_str().unwrap_or_default();
        let text = event["text"].as_str().unwrap_or_default();
        let message_kind = if kind == "final" { "role" } else { "status" };
        let role_id = (kind == "final").then(|| active.profile.role_id.clone());
        let message = json!({"schemaVersion":1,"messageId":format!("message_{}",Uuid::new_v4().simple()),"chatId":active.identity.chat_id,"taskId":active.identity.task_id,"worktreeId":active.identity.worktree_id,"eventSequence":sequence,"kind":message_kind,"roleId":role_id,"runId":run_id,"text":text,"createdAt":now()});
        validate_g5_payload_v1("teamMessage", &message)
            .map_err(|error| {
                ProjectFailure::manifest(format!("invalid actual-agent public message: {error}"))
            })
            .map_err(PublicEventPersistError::Fault)?;
        chat_projection.remove("last_event_sequence");
        chat_projection.insert("last_message".into(), message);
        chat_projection.insert("durable_event_sequence".into(), Value::from(sequence));
        let operation = event["eventId"].as_str().unwrap_or_default();
        storage
            .upsert_projection_records_atomic(vec![
                ProjectionUpsert {
                    aggregate_type: "run",
                    aggregate_id: run_id.into(),
                    operation_id: format!("op_agent_public_{operation}"),
                    request_id: None,
                    payload: updated,
                },
                ProjectionUpsert {
                    aggregate_type: "chat",
                    aggregate_id: active.identity.chat_id.clone(),
                    operation_id: format!("op_agent_public_chat_{operation}"),
                    request_id: None,
                    payload: chat_projection,
                },
            ])
            .map_err(ProjectFailure::from)
            .map_err(PublicEventPersistError::Fault)?;
        active.next_wire_seq += 1;
        Ok(G5Event {
            stream_id: active.stream_id.clone(),
            seq: active.next_wire_seq,
            event: "agent.publicEvent",
            chat_id: Some(active.identity.chat_id.clone()),
            task_id: Some(active.identity.task_id.clone()),
            run_id: Some(run_id.into()),
            payload: event,
        })
    }

    fn state_event(&mut self, storage: &Storage, run_id: &str) -> Result<G5Event, ProjectFailure> {
        let active = self
            .active
            .get_mut(run_id)
            .ok_or_else(|| ProjectFailure::manifest("active agent run is missing"))?;
        let projection = actual_run_projection(storage, run_id)?;
        let run = projection["record"].clone();
        let event = json!({
            "schemaVersion":1,
            "eventId":format!("agent_state_event_{}",Uuid::new_v4().simple()),
            "run":run,
        });
        validate_agent_adapter_payload_v1("runStateChanged", &event).map_err(contract_failure)?;
        validate_state_event_binding(&event, active)?;
        active.next_wire_seq = active
            .next_wire_seq
            .checked_add(1)
            .ok_or_else(|| ProjectFailure::recovery("agent wire sequence overflow"))?;
        active.running_event_pending = false;
        Ok(G5Event {
            stream_id: active.stream_id.clone(),
            seq: active.next_wire_seq,
            event: "agent.runStateChanged",
            chat_id: Some(active.identity.chat_id.clone()),
            task_id: Some(active.identity.task_id.clone()),
            run_id: Some(run_id.into()),
            payload: event,
        })
    }

    fn finish(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        run_id: &str,
        requested_state: &str,
        cancelling: bool,
    ) -> Result<G5Event, ProjectFailure> {
        let active = self
            .active
            .get_mut(run_id)
            .ok_or_else(|| ProjectFailure::manifest("active agent run is missing"))?;
        let report = active.tracker.terminate(
            &SystemProcTable,
            &SystemClock::default(),
            termination_config(),
        );
        let _ = active.launcher.wait();
        let forced_uncertain =
            active.scenario == FakeScenario::CancelUncertain || active.evidence_failed;
        let observed_processes_gone = report.confirmed() && !forced_uncertain;
        let provider_requested_recovery = requested_state == "recoveryRequired";
        let recovery_required = provider_requested_recovery || !observed_processes_gone;
        // An explicit provider recovery verdict invalidates the public termination proof even
        // when the OS tree observation says every tracked identity is gone. The per-process
        // observations remain available so an explicit abandon resolution can re-probe them.
        let processes_proven_gone = observed_processes_gone && !provider_requested_recovery;
        let state = if recovery_required {
            "recoveryRequired"
        } else {
            requested_state
        };
        let outcome = if recovery_required {
            "recoveryRequired"
        } else {
            "cleanExit"
        };
        let projection = actual_run_projection(storage, run_id)?;
        let recovery_id = reserved_recovery_id(&projection)?;
        let created = projection["record"]["createdAt"]
            .as_str()
            .unwrap_or_default();
        let chat_projection = actual_chat_projection(storage, &active.identity.chat_id)?;
        let chat = chat_projection["record"].clone();
        let run = agent_run_record(
            run_id,
            &chat,
            &active.profile,
            projection["record"]["capabilityFingerprint"]
                .as_str()
                .unwrap_or_default(),
            state,
            Some(outcome),
            created,
        )?;
        if active.scenario == FakeScenario::TerminalConstructionFailure {
            return Err(ProjectFailure::recovery(
                "injected terminal construction failure",
            ));
        }
        let tracked = report
            .tracked
            .iter()
            .map(|identity| {
                process_value(
                    *identity,
                    Path::new("/redacted/agent"),
                    observed_processes_gone,
                )
            })
            .collect::<Vec<_>>();
        let lock_state = if recovery_required {
            "recoveryRequired"
        } else {
            if active.profile.permission == "workspaceWrite" {
                "released"
            } else {
                "none"
            }
        };
        let termination = json!({"schemaVersion":1,"runId":run_id,"outcome":outcome,"processesProvenGone":processes_proven_gone,"poisoned":recovery_required,"writerLockState":lock_state,"chatState":if recovery_required {"recoveryRequired"} else {requested_state},"trackedProcesses":tracked,"evidenceDigest":digest_text("agent-fixture-termination-v1"),"observedAt":now()});
        validate_agent_adapter_payload_v1("terminationTranscript", &termination)
            .map_err(contract_failure)?;
        let recovery = recovery_required.then(||json!({"schemaVersion":1,"recoveryId":recovery_id,"chatId":active.identity.chat_id,"taskId":active.identity.task_id,"projectId":active.identity.project_id,"worktreeId":active.identity.worktree_id,"workspaceIdentity":active.identity.workspace_identity,"runId":run_id,"source":"actualAgent","reason":if cancelling {"cancelUncertain"} else {"processIdentityUncertain"},"state":"unresolved","trackedProcesses":tracked,"poisoned":true,"revision":1,"createdAt":now(),"resolvedAt":null}));
        if let Some(value) = &recovery {
            validate_agent_adapter_payload_v1("recoveryRecord", value).map_err(contract_failure)?;
            projects.workspace_mark_recovery_required(storage, &address_from_chat(&chat)?)?;
        }
        let updated_chat = updated_chat(
            &chat,
            if recovery_required {
                "recoveryRequired"
            } else {
                match requested_state {
                    "succeeded" => {
                        if active.profile.role_id == "planning" {
                            "waitingForUser"
                        } else {
                            "reviewReady"
                        }
                    }
                    "canceled" => "canceled",
                    _ => "needsAttention",
                }
            },
        )?;
        let internal = report
            .tracked
            .iter()
            .map(|identity| json!({"pid":identity.pid,"start_us":identity.start_us}))
            .collect::<Vec<_>>();
        let mut barrier = projection.get("barrier").cloned().unwrap_or(Value::Null);
        if let Some(object) = barrier.as_object_mut() {
            object.insert("trackedProcesses".into(), Value::Array(tracked.clone()));
            object.insert("capturedAt".into(), Value::String(now()));
        }
        let mut upserts = vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run_id.into(),
                operation_id: format!("op_agent_terminal_{run_id}"),
                request_id: None,
                payload: agent_run_projection(
                    &run,
                    &projection["workspace_manifest"],
                    Some(barrier),
                    Some(Value::Array(internal)),
                    projection["public_events"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                )?,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: active.identity.chat_id.clone(),
                operation_id: format!("op_agent_chat_terminal_{run_id}"),
                request_id: None,
                payload: chat_projection_with_record(&chat_projection, &updated_chat, None)?,
            },
        ];
        upserts[0]
            .payload
            .insert("termination".into(), termination.clone());
        upserts[0]
            .payload
            .insert("recovery".into(), recovery.clone().unwrap_or(Value::Null));
        preserve_provider_evidence(&projection, &mut upserts[0].payload);
        preserve_recovery_internal(&projection, &mut upserts[0].payload);
        if active.profile.permission == "workspaceWrite" {
            upserts.push(release_writer_lock(
                storage,
                &address_from_chat(&chat)?,
                run_id,
                run_id,
                recovery_required,
            )?);
        }
        let state_event = json!({
            "schemaVersion":1,
            "eventId":format!("agent_state_event_{}",Uuid::new_v4().simple()),
            "run":run,
        });
        validate_agent_adapter_payload_v1("runStateChanged", &state_event)
            .map_err(contract_failure)?;
        validate_state_event_binding(&state_event, active)?;
        let seq = active
            .next_wire_seq
            .checked_add(1)
            .ok_or_else(|| ProjectFailure::recovery("agent wire sequence overflow"))?;
        let event = G5Event {
            stream_id: active.stream_id.clone(),
            seq,
            event: "agent.runStateChanged",
            chat_id: Some(active.identity.chat_id.clone()),
            task_id: Some(active.identity.task_id.clone()),
            run_id: Some(run_id.into()),
            payload: state_event,
        };
        if active.scenario == FakeScenario::TerminalUpsertFailure {
            return Err(ProjectFailure::recovery(
                "injected terminal projection upsert failure",
            ));
        }
        storage.upsert_projection_records_atomic(upserts)?;
        let removed = self.active.remove(run_id);
        debug_assert!(
            removed.is_some(),
            "committed active run must still be owned"
        );
        Ok(event)
    }

    fn fail_poll(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        run_id: &str,
        failure: ProjectFailure,
    ) -> SidecarError {
        if let Some(active) = self.active.get_mut(run_id) {
            active.evidence_failed = true;
        }
        let cleanup = if self.active.contains_key(run_id) {
            self.finish(storage, projects, run_id, "failed", false)
                .err()
                .map(|error| error.detail)
        } else {
            None
        };
        SidecarError::new(
            "agentRuntimeFailure",
            match cleanup {
                Some(cleanup) => format!(
                    "{}; fail-closed terminal persistence also failed: {cleanup}",
                    failure.detail
                ),
                None => failure.detail,
            },
        )
    }
}

fn persist_tracker_snapshot(
    storage: &mut Storage,
    run_id: &str,
    identities: &[ProcIdentity],
) -> Result<(), ProjectFailure> {
    let mut projection = actual_run_projection(storage, run_id)?;
    let existing = projection["process_identity_internal"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let internal = identities
        .iter()
        .map(|identity| json!({"pid":identity.pid,"start_us":identity.start_us}))
        .collect::<Vec<_>>();
    if existing == internal {
        return Ok(());
    }
    let public = identities
        .iter()
        .map(|identity| process_value(*identity, Path::new("/redacted/agent"), false))
        .collect::<Vec<_>>();
    let barrier = projection
        .get_mut("barrier")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ProjectFailure::recovery("durable agent barrier is missing"))?;
    barrier.insert("trackedProcesses".into(), Value::Array(public));
    barrier.insert("capturedAt".into(), Value::String(now()));
    projection.insert("process_identity_internal".into(), Value::Array(internal));
    projection.remove("last_event_sequence");
    storage.upsert_projection_records_atomic(vec![ProjectionUpsert {
        aggregate_type: "run",
        aggregate_id: run_id.into(),
        operation_id: format!("op_agent_tree_{}_{}", run_id, Uuid::new_v4().simple()),
        request_id: None,
        payload: projection,
    }])?;
    Ok(())
}

fn fail_before_release(
    storage: &mut Storage,
    projects: &mut ProjectManager,
    run_id: &str,
    mut launcher: Option<Child>,
    mut tracker: Option<TreeTracker>,
    detail: &str,
) -> ProjectFailure {
    if let Some(child) = launcher.as_mut() {
        drop(child.stdin.take());
    }
    let report = tracker.as_mut().map(|value| {
        value.terminate(
            &SystemProcTable,
            &SystemClock::default(),
            termination_config(),
        )
    });
    if let Some(child) = launcher.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let clean = launcher.is_none() || report.as_ref().is_some_and(|value| value.confirmed());
    let identities = report
        .as_ref()
        .map(|value| value.tracked.clone())
        .or_else(|| tracker.as_ref().map(TreeTracker::tracked_identities))
        .unwrap_or_default();

    let persisted = (|| -> Result<(), ProjectFailure> {
        let projection = actual_run_projection(storage, run_id)?;
        let recovery_id = reserved_recovery_id(&projection)?;
        let chat_id = identity_string(&projection["record"], "chatId")?;
        let chat_projection = actual_chat_projection(storage, &chat_id)?;
        let chat = chat_projection["record"].clone();
        let profile: AgentResolvedProfileV1 =
            serde_json::from_value(projection["record"]["selectedProfile"].clone())
                .map_err(json_failure)?;
        let outcome = if clean {
            "cleanExit"
        } else {
            "recoveryRequired"
        };
        let state = if clean { "failed" } else { "recoveryRequired" };
        let run = agent_run_record(
            run_id,
            &chat,
            &profile,
            projection["record"]["capabilityFingerprint"]
                .as_str()
                .unwrap_or_default(),
            state,
            Some(outcome),
            projection["record"]["createdAt"]
                .as_str()
                .unwrap_or_default(),
        )?;
        let public = identities
            .iter()
            .map(|identity| process_value(*identity, Path::new("/redacted/agent"), clean))
            .collect::<Vec<_>>();
        let internal = identities
            .iter()
            .map(|identity| json!({"pid":identity.pid,"start_us":identity.start_us}))
            .collect::<Vec<_>>();
        let mut payload = agent_run_projection(
            &run,
            &projection["workspace_manifest"],
            projection.get("barrier").cloned(),
            Some(Value::Array(internal)),
            projection["public_events"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        )?;
        preserve_provider_evidence(&projection, &mut payload);
        preserve_recovery_internal(&projection, &mut payload);
        payload.insert(
            "pre_release_failure".into(),
            json!({"stage":"beforeRelease","evidenceDigest":digest_text(detail),"processesProvenGone":clean,"observedAt":now()}),
        );
        let recovery = if clean {
            None
        } else {
            Some(
                json!({"schemaVersion":1,"recoveryId":recovery_id,"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"source":"actualAgent","reason":if public.is_empty() {"processIdentityUnavailable"} else {"processIdentityUncertain"},"state":"unresolved","trackedProcesses":public,"poisoned":true,"revision":1,"createdAt":now(),"resolvedAt":null}),
            )
        };
        if !public.is_empty() {
            let termination = json!({"schemaVersion":1,"runId":run_id,"outcome":outcome,"processesProvenGone":clean,"poisoned":!clean,"writerLockState":if clean {if profile.permission == "workspaceWrite" {"released"} else {"none"}} else {"recoveryRequired"},"chatState":if clean {"failed"} else {"recoveryRequired"},"trackedProcesses":public,"evidenceDigest":digest_text("agent-pre-release-termination-v1"),"observedAt":now()});
            validate_agent_adapter_payload_v1("terminationTranscript", &termination)
                .map_err(contract_failure)?;
            payload.insert("termination".into(), termination);
        }
        if let Some(value) = &recovery {
            validate_agent_adapter_payload_v1("recoveryRecord", value).map_err(contract_failure)?;
            payload.insert("recovery".into(), value.clone());
        }
        let address = address_from_chat(&chat)?;
        if !clean {
            projects.workspace_mark_recovery_required(storage, &address)?;
        }
        let updated = updated_chat(
            &chat,
            if clean {
                "needsAttention"
            } else {
                "recoveryRequired"
            },
        )?;
        let mut upserts = vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run_id.into(),
                operation_id: format!("op_agent_pre_release_terminal_{run_id}"),
                request_id: None,
                payload,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: chat_id,
                operation_id: format!("op_agent_pre_release_chat_{run_id}"),
                request_id: None,
                payload: chat_projection_with_record(&chat_projection, &updated, None)?,
            },
        ];
        if profile.permission == "workspaceWrite" {
            upserts.push(release_writer_lock(
                storage, &address, run_id, run_id, !clean,
            )?);
        }
        storage.upsert_projection_records_atomic(upserts)?;
        Ok(())
    })();
    match persisted {
        Ok(()) => ProjectFailure::recovery(detail),
        Err(error) => ProjectFailure::recovery(format!(
            "{detail}; fail-before-release persistence also failed: {}",
            error.detail
        )),
    }
}

enum ProviderLine {
    Public { kind: String, text: String },
    Private,
    Terminal,
}

fn classify_provider_line(
    active: &mut ActiveAgentRun,
    line: Vec<u8>,
) -> Result<ProviderLine, ProjectFailure> {
    let observation = BoundedObservation::new(line).map_err(provider_failure)?;
    match active
        .provider_classifier
        .classify(observation)
        .map_err(provider_failure)?
    {
        InternalProviderEventV1::Public { kind, text, .. } => {
            Ok(ProviderLine::Public { kind, text })
        }
        InternalProviderEventV1::Private { .. } => Ok(ProviderLine::Private),
        InternalProviderEventV1::Terminal { outcome, .. } => {
            active.provider_terminal_outcome = Some(outcome);
            Ok(ProviderLine::Terminal)
        }
        InternalProviderEventV1::AuthFailure { .. } | InternalProviderEventV1::Handshake { .. } => {
            Err(ProjectFailure::recovery(
                "provider emitted a forbidden runtime event",
            ))
        }
    }
}

fn provider_id(runtime: &str) -> Result<ProviderId, ProjectFailure> {
    match runtime {
        "claude-code" => Ok(ProviderId::ClaudeCode),
        "codex" => Ok(ProviderId::Codex),
        _ => Err(ProjectFailure::manifest("unsupported provider runtime")),
    }
}

fn fake_observed_closure(provider: ProviderId, executable: &Path) -> ObservedExecutableClosureV1 {
    let (binary, helper) = match provider {
        ProviderId::ClaudeCode => ("claude", "node"),
        ProviderId::Codex => ("codex", "rg"),
    };
    ObservedExecutableClosureV1 {
        binary: ProviderExecutableFileIdentityV1 {
            logical_name: binary.into(),
            path_digest: digest_path(executable),
            file_identity_digest: digest_text("g5c-fixture-provider-binary-v1"),
        },
        required_helpers: vec![ProviderExecutableFileIdentityV1 {
            logical_name: helper.into(),
            path_digest: digest_text(&format!("g5c-fixture-helper-path-{helper}-v1")),
            file_identity_digest: digest_text(&format!("g5c-fixture-helper-file-{helper}-v1")),
        }],
    }
}

fn capability_from_discovery(
    observation: &SanitizedDiscoveryObservationV1,
) -> Result<AgentCapabilitySnapshotV1, ProjectFailure> {
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "win32"
    };
    let mut grouped: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for profile in &observation.exact_profiles {
        let entry = grouped.entry(profile.model_selector.clone()).or_default();
        if let Some(reasoning) = &profile.reasoning_effort {
            entry.0.insert(reasoning.clone());
        }
        entry.1.insert(profile.permission.clone());
    }
    let models = grouped
        .into_iter()
        .map(
            |(model_selector, (reasoning_efforts, permissions))| AgentModelCapabilityV1 {
                model_selector,
                reasoning_efforts: reasoning_efforts.into_iter().collect(),
                permissions: permissions.into_iter().collect(),
            },
        )
        .collect();
    let provider_blocker = observation.blocked_reason.as_deref();
    let mut blocked_reason = provider_blocker
        .map(|reason| match reason {
            "notInstalled" => "notInstalled",
            "notAuthenticated" => "notAuthenticated",
            "authExpired" => "authExpired",
            "closureMismatch" => "installationIncomplete",
            "unsupportedVersion" => "unsupportedVersion",
            "unsupportedProfile" | "modelCatalogUnverified" => "exactModelUnavailable",
            "runtimeVerificationIncomplete" => "capabilityUnknown",
            _ => "capabilityUnknown",
        })
        .map(str::to_string);
    if platform != "darwin" {
        blocked_reason = Some("platformUnsupported".into());
    }
    let evidence = if matches!(
        provider_blocker,
        Some("unsupportedProfile" | "modelCatalogUnverified")
    ) {
        ("exactVersionManifest", "measuredExactVersion", "argvOnly")
    } else {
        match observation.model_catalog_evidence.as_str() {
            "runtimeVerified" => (
                "machineReadableCatalog",
                "runtimeVerified",
                "runtimeVerified",
            ),
            "exactVersionManifest" => (
                "exactVersionManifest",
                "measuredExactVersion",
                "measuredExactVersion",
            ),
            "argvOnly" => ("exactVersionManifest", "measuredExactVersion", "argvOnly"),
            _ => ("unavailable", "unknown", "unknown"),
        }
    };
    let public_incomplete = provider_blocker == Some("closureMismatch");
    let executable = (!public_incomplete)
        .then_some(observation.executable_closure.as_ref())
        .flatten()
        .map(|closure| AgentExecutableIdentityV1 {
            path_digest: closure.binary.path_digest.clone(),
            file_identity_digest: closure.binary.file_identity_digest.clone(),
            cli_version: closure.binary.cli_version.clone(),
        });
    let mut snapshot = AgentCapabilitySnapshotV1 {
        schema_version: 1,
        runtime: observation.runtime.clone(),
        platform: platform.into(),
        installation_state: if public_incomplete {
            "incomplete".into()
        } else {
            observation.installation_state.clone()
        },
        auth_state: observation.auth_state.clone(),
        executable,
        models,
        discovery: AgentDiscoveryEvidenceV1 {
            source: evidence.0.into(),
            verification_level: evidence.1.into(),
            exact_cli_version: if public_incomplete {
                None
            } else {
                observation.exact_cli_version.clone()
            },
            effective_model_evidence: evidence.2.into(),
        },
        can_start: blocked_reason.is_none(),
        blocked_reason,
        fingerprint: String::new(),
        observed_at: observation.observed_at.clone(),
    };
    snapshot.fingerprint =
        compute_agent_capability_fingerprint_v1(&snapshot).map_err(contract_failure)?;
    validate_agent_adapter_payload_v1(
        "capabilitySnapshot",
        &serde_json::to_value(&snapshot).map_err(json_failure)?,
    )
    .map_err(contract_failure)?;
    Ok(snapshot)
}

fn preflight_outcome(
    request: &AgentRunStartRequestV1,
    profile: &AgentResolvedProfileV1,
    capability: &AgentCapabilitySnapshotV1,
) -> &'static str {
    if let Some(reason) = capability.blocked_reason.as_deref() {
        return match reason {
            "platformUnsupported" => "platformUnsupported",
            "notInstalled" => "notInstalled",
            "installationIncomplete" => "installationIncomplete",
            "unsupportedVersion" => "unsupportedVersion",
            "notAuthenticated" => "notAuthenticated",
            "authExpired" => "authExpired",
            "exactModelUnavailable" => "exactModelUnavailable",
            _ => "capabilityUnknown",
        };
    }
    if request.expected_capability_fingerprint != capability.fingerprint
        || request.role_id != profile.role_id
        || profile.runtime != capability.runtime
    {
        return "profileChanged";
    }
    let Some(model) = capability
        .models
        .iter()
        .find(|model| model.model_selector == profile.model_selector)
    else {
        return "exactModelUnavailable";
    };
    if profile
        .reasoning_effort
        .as_ref()
        .is_some_and(|effort| !model.reasoning_efforts.contains(effort))
    {
        return "reasoningUnsupported";
    }
    if !model.permissions.contains(&profile.permission) {
        return "permissionUnsupported";
    }
    "started"
}

fn active_run_limit_reached(active_runs: usize) -> bool {
    active_runs >= AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1
}

fn resolve_prompt(
    storage: &Storage,
    chat_id: &str,
    ids: &[String],
) -> Result<Vec<u8>, ProjectFailure> {
    let selected: HashSet<_> = ids.iter().map(String::as_str).collect();
    let mut found = HashSet::new();
    let mut prompt = Vec::new();
    for payload in storage.aggregate_event_payloads("chat", chat_id)? {
        if let Some(message) = payload.get("last_message").filter(|value| !value.is_null()) {
            if message["messageId"]
                .as_str()
                .is_some_and(|id| selected.contains(id))
            {
                let text = message["text"]
                    .as_str()
                    .ok_or_else(|| ProjectFailure::manifest("input message text is missing"))?;
                found.insert(
                    message["messageId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                );
                prompt.extend_from_slice(text.as_bytes());
                prompt.push(b'\n');
            }
        }
    }
    if found.len() != selected.len() {
        return Err(ProjectFailure::manifest(
            "input message identity is missing or belongs to another chat",
        ));
    }
    Ok(prompt)
}

fn agent_run_record(
    run_id: &str,
    chat: &Value,
    profile: &AgentResolvedProfileV1,
    fingerprint: &str,
    state: &str,
    termination: Option<&str>,
    created: &str,
) -> Result<Value, ProjectFailure> {
    let stage = match profile.role_id.as_str() {
        "planning" => "planning",
        "coding" => "implementing",
        "review" => "verifying",
        _ => return Err(ProjectFailure::manifest("unknown agent role")),
    };
    let value = json!({"schemaVersion":1,"runId":run_id,"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"selectedProfile":profile,"source":"actualAgent","capabilityFingerprint":fingerprint,"state":state,"stage":stage,"terminationOutcome":termination,"createdAt":created,"updatedAt":now()});
    validate_agent_adapter_payload_v1("runRecord", &value).map_err(contract_failure)?;
    Ok(value)
}

fn agent_run_projection(
    run: &Value,
    manifest: &Value,
    barrier: Option<Value>,
    internal: Option<Value>,
    events: Vec<Value>,
) -> Result<Map<String, Value>, ProjectFailure> {
    let budget = public_budget_metrics(&events)?;
    Ok(json!({"run_id":run["runId"],"chat_id":run["chatId"],"state":run["state"],"worktree_path":manifest["worktreePath"],"source_kind":SOURCE_KIND,"record":run,"workspace_manifest":manifest,"barrier":barrier,"process_identity_internal":internal,"public_events":events,"public_event_count":budget.event_count,"public_text_bytes":budget.text_bytes,"public_ledger_cumulative_snapshot_bytes":budget.cumulative_snapshot_bytes,"termination":null,"recovery":null}).as_object().expect("agent projection").clone())
}
fn actual_run_projection(
    storage: &Storage,
    run_id: &str,
) -> Result<Map<String, Value>, ProjectFailure> {
    let value = storage
        .projection_record("run", run_id)?
        .ok_or_else(|| ProjectFailure::manifest("agent run is missing"))?;
    if value.get("source_kind").and_then(Value::as_str) != Some(SOURCE_KIND) {
        return Err(ProjectFailure::manifest(
            "run is not an actual-agent ledger record",
        ));
    }
    stored_public_budget(&value)?;
    Ok(value)
}

fn preserve_provider_evidence(source: &Map<String, Value>, target: &mut Map<String, Value>) {
    for field in ["provider_discovery", "runtime_attestation"] {
        if let Some(value) = source.get(field) {
            target.insert(field.into(), value.clone());
        }
    }
}

fn new_recovery_id() -> String {
    format!("agent_recovery_{}", Uuid::new_v4().simple())
}

fn reserved_recovery_id(projection: &Map<String, Value>) -> Result<String, ProjectFailure> {
    let recovery_id = projection
        .get(RECOVERY_RESERVATION_FIELD)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProjectFailure::recovery("actual-agent recovery ID reservation is missing")
        })?;
    if !recovery_id.starts_with("agent_recovery_") || recovery_id.len() <= "agent_recovery_".len() {
        return Err(ProjectFailure::recovery(
            "actual-agent recovery ID reservation is invalid",
        ));
    }
    Ok(recovery_id.to_string())
}

fn ensure_recovery_reservation(
    storage: &mut Storage,
    projection: &Map<String, Value>,
) -> Result<Map<String, Value>, ProjectFailure> {
    ensure_recovery_reservation_inner(storage, projection, false)
}

fn ensure_recovery_reservation_inner(
    storage: &mut Storage,
    projection: &Map<String, Value>,
    fail_before_commit: bool,
) -> Result<Map<String, Value>, ProjectFailure> {
    if projection
        .get(RECOVERY_RESERVATION_FIELD)
        .is_some_and(Value::is_string)
    {
        reserved_recovery_id(projection)?;
        return Ok(projection.clone());
    }
    let run_id = identity_string(&projection["record"], "runId")?;
    let recovery_id = projection
        .get("recovery")
        .filter(|value| !value.is_null())
        .and_then(|value| value.get("recoveryId"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(new_recovery_id);
    let mut reserved = projection.clone();
    reserved.remove("last_event_sequence");
    reserved.insert(
        RECOVERY_RESERVATION_FIELD.into(),
        Value::String(recovery_id),
    );
    if fail_before_commit {
        return Err(ProjectFailure::recovery(
            "injected recovery reservation persistence failure",
        ));
    }
    storage.upsert_projection_records_atomic(vec![ProjectionUpsert {
        aggregate_type: "run",
        aggregate_id: run_id.clone(),
        operation_id: format!("op_agent_recovery_reserve_{}", Uuid::new_v4().simple()),
        request_id: None,
        payload: reserved,
    }])?;
    actual_run_projection(storage, &run_id)
}

fn preserve_recovery_internal(source: &Map<String, Value>, target: &mut Map<String, Value>) {
    for field in [RECOVERY_RESERVATION_FIELD, RECOVERY_INTENT_FIELD] {
        if let Some(value) = source.get(field) {
            target.insert(field.into(), value.clone());
        }
    }
}

fn validate_provider_projection(projection: &Map<String, Value>) -> Result<(), ProjectFailure> {
    let discovery = projection.get("provider_discovery");
    let attestation = projection.get("runtime_attestation");
    if discovery.is_none() && attestation.is_none() {
        // Pre-G5C actual-agent fixture projections remain readable.
        return Ok(());
    }
    let (Some(discovery), Some(attestation)) = (discovery, attestation) else {
        return Err(ProjectFailure::recovery(
            "provider discovery and runtime attestation must persist together",
        ));
    };
    let closure = discovery["executableClosure"]
        .as_object()
        .ok_or_else(|| ProjectFailure::recovery("provider executable closure is missing"))?;
    let provider = provider_id(
        discovery["providerId"]
            .as_str()
            .ok_or_else(|| ProjectFailure::recovery("provider identity is missing"))?,
    )?;
    let binary = &closure["binary"];
    let helpers = closure["requiredHelpers"]
        .as_array()
        .ok_or_else(|| ProjectFailure::recovery("provider helper closure is missing"))?;
    let observed = ObservedExecutableClosureV1 {
        binary: provider_file_identity_from_value(binary)?,
        required_helpers: helpers
            .iter()
            .map(provider_file_identity_from_value)
            .collect::<Result<Vec<_>, _>>()?,
    };
    let definition = ProviderDefinitionV1::new(provider, observed).map_err(provider_failure)?;
    let manifest = serde_json::to_value(definition.manifest()).map_err(json_failure)?;
    let record = projection
        .get("record")
        .ok_or_else(|| ProjectFailure::recovery("actual-agent run record is missing"))?;
    let accepted = json!({
        "schemaVersion":1,
        "runId":record["runId"],
        "chatId":record["chatId"],
        "taskId":record["taskId"],
        "projectId":record["projectId"],
        "worktreeId":record["worktreeId"],
        "workspaceIdentity":record["workspaceIdentity"],
        "roleId":record["selectedProfile"]["roleId"],
        "capabilityFingerprint":record["capabilityFingerprint"],
        "state":"preparing"
    });
    validate_agent_runtime_attestation_against_accepted_v1(
        attestation,
        &manifest,
        discovery,
        &accepted,
        &record["selectedProfile"],
    )
    .map_err(contract_failure)?;
    Ok(())
}

fn provider_file_identity_from_value(
    value: &Value,
) -> Result<ProviderExecutableFileIdentityV1, ProjectFailure> {
    Ok(ProviderExecutableFileIdentityV1 {
        logical_name: identity_string(value, "logicalName")?,
        path_digest: identity_string(value, "pathDigest")?,
        file_identity_digest: identity_string(value, "fileIdentityDigest")?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PublicBudgetMetrics {
    event_count: usize,
    text_bytes: usize,
    cumulative_snapshot_bytes: usize,
}

struct PublicEventCollector<'a> {
    projection: &'a Map<String, Value>,
    persisted_events: &'a [Value],
    persisted_budget: PublicBudgetMetrics,
    events: Vec<Value>,
    text_bytes: usize,
    snapshot_bytes: usize,
}

impl<'a> PublicEventCollector<'a> {
    fn new(
        projection: &'a Map<String, Value>,
        persisted_events: &'a [Value],
        persisted_budget: PublicBudgetMetrics,
    ) -> Self {
        Self {
            projection,
            persisted_events,
            persisted_budget,
            events: Vec::new(),
            text_bytes: 0,
            snapshot_bytes: 0,
        }
    }

    fn push(
        &mut self,
        active: &ActiveAgentRun,
        kind: &str,
        text: String,
    ) -> Result<bool, ProjectFailure> {
        let event_count = self
            .persisted_budget
            .event_count
            .checked_add(self.events.len())
            .and_then(|value| value.checked_add(1));
        let text_bytes = self
            .persisted_budget
            .text_bytes
            .checked_add(self.text_bytes)
            .and_then(|value| value.checked_add(text.len()));
        if event_count.is_none_or(|value| value > AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1)
            || text_bytes.is_none_or(|value| value > AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1)
        {
            return Ok(false);
        }
        let event_text_bytes = text.len();
        let event = json!({"schemaVersion":1,"eventId":format!("agent_event_{}",Uuid::new_v4().simple()),"chatId":active.identity.chat_id,"taskId":active.identity.task_id,"projectId":active.identity.project_id,"worktreeId":active.identity.worktree_id,"workspaceIdentity":active.identity.workspace_identity,"runId":active.identity.run_id,"roleId":active.profile.role_id,"capabilityFingerprint":active.capability_fingerprint,"kind":kind,"text":text,"createdAt":now()});
        validate_agent_adapter_payload_v1("publicEvent", &event).map_err(contract_failure)?;
        validate_public_event_binding(&event, active)?;
        if self.projection["record"]["capabilityFingerprint"] != active.capability_fingerprint {
            return Err(ProjectFailure::recovery(
                "durable agent run fingerprint differs from the accepted run",
            ));
        }
        let mut prospective =
            Vec::with_capacity(self.persisted_events.len() + self.events.len() + 1);
        prospective.extend_from_slice(self.persisted_events);
        prospective.extend_from_slice(&self.events);
        prospective.push(event.clone());
        let next_snapshot =
            agent_public_events_snapshot_bytes_v1(&prospective).map_err(contract_failure)?;
        let cumulative = self
            .persisted_budget
            .cumulative_snapshot_bytes
            .checked_add(self.snapshot_bytes)
            .and_then(|value| value.checked_add(next_snapshot));
        if cumulative.is_none_or(|value| {
            value > AGENT_PUBLIC_LEDGER_CUMULATIVE_SNAPSHOT_BYTES_PER_RUN_MAX_V1
        }) {
            return Ok(false);
        }
        self.text_bytes = self
            .text_bytes
            .checked_add(event_text_bytes)
            .ok_or_else(|| ProjectFailure::recovery("public event text byte counter overflow"))?;
        self.snapshot_bytes = self
            .snapshot_bytes
            .checked_add(next_snapshot)
            .ok_or_else(|| ProjectFailure::recovery("public event snapshot counter overflow"))?;
        self.events.push(event);
        Ok(true)
    }

    fn into_events(self) -> Vec<Value> {
        self.events
    }
}

fn public_budget_metrics(events: &[Value]) -> Result<PublicBudgetMetrics, ProjectFailure> {
    let event_count = events.len();
    let text_bytes = events.iter().try_fold(0_usize, |total, event| {
        validate_agent_adapter_payload_v1("publicEvent", event).map_err(contract_failure)?;
        let text = event["text"]
            .as_str()
            .ok_or_else(|| ProjectFailure::manifest("public event text is missing"))?;
        total
            .checked_add(text.len())
            .ok_or_else(|| ProjectFailure::recovery("public event text byte counter overflow"))
    })?;
    let cumulative_snapshot_bytes =
        hello_agent_protocol::agent_adapter_v1::agent_public_ledger_cumulative_snapshot_bytes_v1(
            events,
        )
        .map_err(contract_failure)?;
    let metrics = PublicBudgetMetrics {
        event_count,
        text_bytes,
        cumulative_snapshot_bytes,
    };
    enforce_public_budget(metrics)?;
    Ok(metrics)
}

fn enforce_public_budget(metrics: PublicBudgetMetrics) -> Result<(), ProjectFailure> {
    if metrics.event_count > AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1
        || metrics.text_bytes > AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1
        || metrics.cumulative_snapshot_bytes
            > AGENT_PUBLIC_LEDGER_CUMULATIVE_SNAPSHOT_BYTES_PER_RUN_MAX_V1
    {
        return Err(ProjectFailure::recovery(
            "actual-agent public event resource budget exceeded",
        ));
    }
    Ok(())
}

fn validate_projection_public_budget(
    projection: &Map<String, Value>,
) -> Result<PublicBudgetMetrics, ProjectFailure> {
    let events = projection["public_events"]
        .as_array()
        .ok_or_else(|| ProjectFailure::recovery("actual-agent public event ledger is missing"))?;
    let metrics = public_budget_metrics(events)?;
    let stored = [
        ("public_event_count", metrics.event_count),
        ("public_text_bytes", metrics.text_bytes),
        (
            "public_ledger_cumulative_snapshot_bytes",
            metrics.cumulative_snapshot_bytes,
        ),
    ];
    let present = stored
        .iter()
        .filter(|(name, _)| projection.get(*name).is_some())
        .count();
    if present == 0 {
        // G5A projections created before the G5B counters remain readable and are upgraded on
        // their next write. A partially present or mismatched counter set is never accepted.
        return Ok(metrics);
    }
    if present != stored.len()
        || stored.iter().any(|(name, expected)| {
            projection.get(*name).and_then(Value::as_u64) != Some(*expected as u64)
        })
    {
        return Err(ProjectFailure::recovery(
            "actual-agent public event durable counters do not match the ledger",
        ));
    }
    Ok(metrics)
}

fn stored_public_budget(
    projection: &Map<String, Value>,
) -> Result<PublicBudgetMetrics, ProjectFailure> {
    validate_projection_public_budget(projection)
}

fn apply_public_budget(projection: &mut Map<String, Value>, metrics: PublicBudgetMetrics) {
    projection.insert(
        "public_event_count".into(),
        Value::from(metrics.event_count as u64),
    );
    projection.insert(
        "public_text_bytes".into(),
        Value::from(metrics.text_bytes as u64),
    );
    projection.insert(
        "public_ledger_cumulative_snapshot_bytes".into(),
        Value::from(metrics.cumulative_snapshot_bytes as u64),
    );
}
fn actual_chat_projection(
    storage: &Storage,
    chat_id: &str,
) -> Result<Map<String, Value>, ProjectFailure> {
    storage
        .projection_record("chat", chat_id)?
        .ok_or_else(|| ProjectFailure::manifest("chat projection is missing"))
}

fn recovery_manifest_digest(manifest: &Value) -> Result<String, ProjectFailure> {
    let bytes = serde_json::to_vec(manifest).map_err(json_failure)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn address_from_recovery_intent(intent: &Value) -> Result<WorkspaceAddress, ProjectFailure> {
    Ok(WorkspaceAddress {
        chat_id: identity_string(intent, "chatId")?,
        project_id: identity_string(intent, "projectId")?,
        worktree_id: identity_string(intent, "worktreeId")?,
        workspace_identity: identity_string(intent, "workspaceIdentity")?,
    })
}

fn validate_recovery_intent(
    projection: &Map<String, Value>,
    intent: &Value,
) -> Result<(), ProjectFailure> {
    let recovery = projection
        .get("recovery")
        .filter(|value| !value.is_null())
        .ok_or_else(|| ProjectFailure::recovery("recovery intent has no unresolved recovery"))?;
    for field in [
        "recoveryId",
        "chatId",
        "taskId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
        "runId",
    ] {
        if intent.get(field) != recovery.get(field) {
            return Err(ProjectFailure::recovery(
                "recovery resolution intent identity differs",
            ));
        }
    }
    if intent["expectedRevision"] != recovery["revision"]
        || intent["resolution"] != "abandon"
        || recovery["state"] != "unresolved"
        || recovery["poisoned"] != true
        || intent["recoveryId"]
            != projection
                .get(RECOVERY_RESERVATION_FIELD)
                .cloned()
                .unwrap_or(Value::Null)
    {
        return Err(ProjectFailure::recovery(
            "recovery resolution intent state differs",
        ));
    }
    let digest = identity_string(intent, "expectedManifestDigest")?;
    if recovery_manifest_digest(&intent["expectedManifest"])? != digest {
        return Err(ProjectFailure::recovery(
            "recovery resolution intent manifest digest differs",
        ));
    }
    Ok(())
}

fn persist_recovery_intent(
    storage: &mut Storage,
    projection: &Map<String, Value>,
    intent: &Value,
) -> Result<(), ProjectFailure> {
    validate_recovery_intent(projection, intent)?;
    if let Some(existing) = projection
        .get(RECOVERY_INTENT_FIELD)
        .filter(|value| !value.is_null())
    {
        return if existing == intent {
            Ok(())
        } else {
            Err(ProjectFailure::recovery(
                "another recovery resolution intent is already pending",
            ))
        };
    }
    let run_id = identity_string(intent, "runId")?;
    let recovery_id = identity_string(intent, "recoveryId")?;
    let mut payload = projection.clone();
    payload.remove("last_event_sequence");
    payload.insert(RECOVERY_INTENT_FIELD.into(), intent.clone());
    storage.upsert_projection_records_atomic(vec![ProjectionUpsert {
        aggregate_type: "run",
        aggregate_id: run_id,
        operation_id: format!("op_agent_recovery_intent_{recovery_id}"),
        request_id: None,
        payload,
    }])?;
    Ok(())
}

fn finalize_recovery_intent(
    storage: &mut Storage,
    projects: &ProjectManager,
    address: &WorkspaceAddress,
    run_id: &str,
    operation: &str,
    fail_before_commit: bool,
) -> Result<Value, ProjectFailure> {
    let projection = actual_run_projection(storage, run_id)?;
    if projection
        .get(RECOVERY_INTENT_FIELD)
        .is_none_or(Value::is_null)
    {
        return Err(ProjectFailure::recovery(
            "recovery resolution intent is missing",
        ));
    }
    let intent = projection[RECOVERY_INTENT_FIELD].clone();
    validate_recovery_intent(&projection, &intent)?;
    if address_from_recovery_intent(&intent)? != *address {
        return Err(ProjectFailure::recovery(
            "recovery resolution address differs",
        ));
    }
    projects.workspace_verify_recovery_abandoned(
        address,
        &intent["expectedManifest"],
        intent["expectedManifestDigest"]
            .as_str()
            .unwrap_or_default(),
    )?;
    let before = projection["recovery"].clone();
    let mut after = before.clone();
    after["trackedProcesses"] = intent["trackedProcesses"].clone();
    after["poisoned"] = Value::Bool(false);
    after["state"] = Value::String("resolvedAbandoned".into());
    after["revision"] = Value::from(before["revision"].as_u64().unwrap_or(0) + 1);
    after["resolvedAt"] = Value::String(now());
    validate_agent_adapter_payload_v1("recoveryRecord", &after).map_err(contract_failure)?;

    let run_id = identity_string(&intent, "runId")?;
    let chat_projection = actual_chat_projection(storage, &address.chat_id)?;
    let chat = chat_projection["record"].clone();
    let profile: AgentResolvedProfileV1 =
        serde_json::from_value(projection["record"]["selectedProfile"].clone())
            .map_err(json_failure)?;
    let resolved_run = agent_run_record(
        &run_id,
        &chat,
        &profile,
        projection["record"]["capabilityFingerprint"]
            .as_str()
            .unwrap_or_default(),
        "canceled",
        Some("cleanExit"),
        projection["record"]["createdAt"]
            .as_str()
            .unwrap_or_default(),
    )?;
    let termination = json!({
        "schemaVersion":1,"runId":run_id,"outcome":"cleanExit",
        "processesProvenGone":true,"poisoned":false,
        "writerLockState":if profile.permission == "workspaceWrite" {"released"} else {"none"},
        "chatState":"canceled","trackedProcesses":after["trackedProcesses"],
        "evidenceDigest":digest_text("agent-recovery-abandoned-v1"),"observedAt":now()
    });
    validate_agent_adapter_payload_v1("terminationTranscript", &termination)
        .map_err(contract_failure)?;
    let discarded_chat = updated_chat(&chat, "discarded")?;
    let mut updated_projection = projection.clone();
    updated_projection.remove("last_event_sequence");
    updated_projection.remove(RECOVERY_INTENT_FIELD);
    updated_projection.insert("record".into(), resolved_run);
    updated_projection.insert("state".into(), Value::String("canceled".into()));
    updated_projection.insert("termination".into(), termination);
    updated_projection.insert("recovery".into(), after.clone());
    let recovery_id = identity_string(&intent, "recoveryId")?;
    let mut upserts = vec![
        ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: format!("op_agent_recovery_finalize_{recovery_id}"),
            request_id: None,
            payload: updated_projection,
        },
        ProjectionUpsert {
            aggregate_type: "chat",
            aggregate_id: address.chat_id.clone(),
            operation_id: format!("op_agent_recovery_chat_finalize_{recovery_id}"),
            request_id: None,
            payload: chat_projection_with_record(&chat_projection, &discarded_chat, None)?,
        },
    ];
    if profile.permission == "workspaceWrite" {
        upserts.push(release_writer_lock(
            storage,
            address,
            &run_id,
            &format!("recovery_finalize_{recovery_id}_{operation}"),
            false,
        )?);
    }
    if fail_before_commit {
        return Err(ProjectFailure::recovery(
            "injected failure before recovery finalize commit",
        ));
    }
    storage.upsert_projection_records_atomic(upserts)?;
    Ok(after)
}
fn address_from_chat(chat: &Value) -> Result<WorkspaceAddress, ProjectFailure> {
    Ok(WorkspaceAddress {
        chat_id: identity_string(chat, "chatId")?,
        project_id: identity_string(chat, "projectId")?,
        worktree_id: identity_string(chat, "worktreeId")?,
        workspace_identity: identity_string(chat, "workspaceIdentity")?,
    })
}
fn identity_string(value: &Value, key: &str) -> Result<String, ProjectFailure> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ProjectFailure::manifest(format!("{key} is missing")))
}
fn updated_chat(chat: &Value, state: &str) -> Result<Value, ProjectFailure> {
    let mut value = chat.clone();
    value["state"] = Value::String(state.into());
    value["updatedAt"] = Value::String(now());
    validate_g5_payload_v1("chatRecord", &value)
        .map_err(|error| ProjectFailure::manifest(format!("invalid chat state: {error}")))?;
    Ok(value)
}
fn chat_projection_with_record(
    existing: &Map<String, Value>,
    chat: &Value,
    message: Option<Value>,
) -> Result<Map<String, Value>, ProjectFailure> {
    let mut value = existing.clone();
    value.remove("last_event_sequence");
    value.insert("record".into(), chat.clone());
    value.insert("state".into(), chat["state"].clone());
    value.insert("last_message".into(), message.unwrap_or(Value::Null));
    Ok(value)
}

fn acquire_writer_lock(
    storage: &Storage,
    chat: &Value,
    run_id: &str,
    request_id: &str,
) -> Result<ProjectionUpsert, ProjectFailure> {
    let lock_id = format!("lock_{}", chat["worktreeId"].as_str().unwrap_or_default());
    let mut lock = storage
        .projection_record("lock", &lock_id)?
        .ok_or_else(|| ProjectFailure::manifest("workspace writer lock is missing"))?;
    if lock
        .get("writer_run_id")
        .is_some_and(|value| !value.is_null())
        || lock.get("poisoned").and_then(Value::as_bool) != Some(false)
        || lock.get("recovery_required").and_then(Value::as_bool) != Some(false)
    {
        return Err(ProjectFailure::recovery(
            "workspace already has a writer lock",
        ));
    }
    lock.insert("writer_run_id".into(), Value::String(run_id.into()));
    lock.insert(
        "evidence".into(),
        json!({"phase":"agentWriterHeld","run_id":run_id}),
    );
    Ok(ProjectionUpsert {
        aggregate_type: "lock",
        aggregate_id: lock_id,
        operation_id: format!("op_agent_writer_{request_id}"),
        request_id: Some(request_id.into()),
        payload: lock,
    })
}
fn release_writer_lock(
    storage: &Storage,
    address: &WorkspaceAddress,
    run_id: &str,
    operation: &str,
    poisoned: bool,
) -> Result<ProjectionUpsert, ProjectFailure> {
    let lock_id = format!("lock_{}", address.worktree_id);
    let mut lock = storage
        .projection_record("lock", &lock_id)?
        .ok_or_else(|| ProjectFailure::manifest("workspace writer lock is missing"))?;
    if lock["writer_run_id"].as_str() != Some(run_id) {
        return Err(ProjectFailure::manifest(
            "writer lock belongs to another run",
        ));
    }
    lock.insert(
        "writer_run_id".into(),
        if poisoned {
            Value::String(run_id.into())
        } else {
            Value::Null
        },
    );
    lock.insert("poisoned".into(), Value::Bool(poisoned));
    lock.insert("recovery_required".into(), Value::Bool(poisoned));
    lock.insert("evidence".into(),json!({"phase":if poisoned{"agentRecoveryRequired"}else{"agentWriterReleased"},"run_id":run_id}));
    Ok(ProjectionUpsert {
        aggregate_type: "lock",
        aggregate_id: lock_id,
        operation_id: format!("op_agent_writer_terminal_{operation}"),
        request_id: None,
        payload: lock,
    })
}

fn process_value(identity: ProcIdentity, path: &Path, gone: bool) -> Value {
    json!({"pid":identity.pid,"startIdentityDigest":digest_text(&identity.start_us.to_string()),"executablePathDigest":digest_path(path),"observedGone":gone})
}
fn identity_value(identity: &RunIdentity) -> Value {
    json!({"chatId":identity.chat_id,"taskId":identity.task_id,"projectId":identity.project_id,"worktreeId":identity.worktree_id,"workspaceIdentity":identity.workspace_identity,"runId":identity.run_id})
}
fn validate_state_event_binding(
    event: &Value,
    active: &ActiveAgentRun,
) -> Result<(), ProjectFailure> {
    let run = &event["run"];
    if run["runId"] != active.identity.run_id
        || run["chatId"] != active.identity.chat_id
        || run["taskId"] != active.identity.task_id
        || run["projectId"] != active.identity.project_id
        || run["worktreeId"] != active.identity.worktree_id
        || run["workspaceIdentity"] != active.identity.workspace_identity
        || run["selectedProfile"]["roleId"] != active.profile.role_id
        || run["capabilityFingerprint"] != active.capability_fingerprint
    {
        return Err(ProjectFailure::recovery(
            "agent run state event identity differs from the accepted run",
        ));
    }
    Ok(())
}
fn validate_public_event_binding(
    event: &Value,
    active: &ActiveAgentRun,
) -> Result<(), ProjectFailure> {
    if event["runId"] != active.identity.run_id
        || event["chatId"] != active.identity.chat_id
        || event["taskId"] != active.identity.task_id
        || event["projectId"] != active.identity.project_id
        || event["worktreeId"] != active.identity.worktree_id
        || event["workspaceIdentity"] != active.identity.workspace_identity
        || event["roleId"] != active.profile.role_id
        || event["capabilityFingerprint"] != active.capability_fingerprint
    {
        return Err(ProjectFailure::recovery(
            "agent public event identity differs from the accepted run",
        ));
    }
    Ok(())
}
fn same_identity(left: &RunIdentity, right: &RunIdentity) -> bool {
    left.chat_id == right.chat_id
        && left.task_id == right.task_id
        && left.project_id == right.project_id
        && left.worktree_id == right.worktree_id
        && left.workspace_identity == right.workspace_identity
        && left.run_id == right.run_id
}
fn digest_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}
fn digest_bytes(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    format!("sha256:{:x}", hasher.finalize())
}
fn digest_path(path: &Path) -> String {
    digest_text(&path.to_string_lossy())
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
fn scenario_name(value: FakeScenario) -> &'static str {
    match value {
        FakeScenario::Clean => "clean",
        FakeScenario::NoOutput => "no-output",
        FakeScenario::FinalDelayed => "final-delayed",
        FakeScenario::CancelClean => "cancel-clean",
        FakeScenario::CancelUncertain => "cancel-uncertain",
        FakeScenario::Malformed => "malformed",
        FakeScenario::Oversized => "oversized",
        FakeScenario::Descendant => "descendant",
        FakeScenario::SpawnFailure => "clean",
        FakeScenario::IdentityFailure => "clean",
        FakeScenario::ObserveFailure => "clean",
        FakeScenario::ReleaseFailure => "clean",
        FakeScenario::PollObserveFailure => "cancel-clean",
        FakeScenario::PollWaitFailure => "cancel-clean",
        FakeScenario::TerminalConstructionFailure => "no-output",
        FakeScenario::TerminalUpsertFailure => "no-output",
        FakeScenario::DiscoveryMissingHelper
        | FakeScenario::DiscoveryUnknownVersion
        | FakeScenario::DiscoveryUnknownAuth
        | FakeScenario::DiscoveryStaleClosure
        | FakeScenario::DiscoveryProfileDrift => "clean",
        FakeScenario::HandshakeBeforePublic => "handshake-before-public",
        FakeScenario::HandshakeVersionMismatch => "handshake-version-mismatch",
        FakeScenario::HandshakeModelMismatch => "handshake-model-mismatch",
        FakeScenario::HandshakeCwdMismatch => "handshake-cwd-mismatch",
        FakeScenario::HandshakePermissionMismatch => "handshake-permission-mismatch",
        FakeScenario::HandshakeToolsMismatch => "handshake-tools-mismatch",
        FakeScenario::HandshakeMcpMismatch => "handshake-mcp-mismatch",
        FakeScenario::HandshakePluginsMismatch => "handshake-plugins-mismatch",
        FakeScenario::HandshakeConfigMismatch => "handshake-config-mismatch",
        FakeScenario::AttestationPersistenceFailure => "clean",
        FakeScenario::ProviderAuthFailure => "provider-auth-failure",
        FakeScenario::ProviderTerminalMalformed => "provider-terminal-malformed",
        FakeScenario::PublicTextExact => "public-text-exact",
        FakeScenario::PublicAggregateExact => "public-aggregate-exact",
        FakeScenario::PublicAggregatePlusOne => "public-aggregate-plus-one",
        FakeScenario::ProviderTerminalFailed => "provider-terminal-failed",
        FakeScenario::ProviderTerminalRecoveryRequired => "provider-terminal-recovery-required",
    }
}
fn contract_failure(error: impl std::fmt::Display) -> ProjectFailure {
    ProjectFailure::manifest(format!("agent adapter contract: {error}"))
}
fn provider_failure(_error: impl std::fmt::Display) -> ProjectFailure {
    ProjectFailure::manifest("provider verification failed closed")
}
fn adapter_failure(error: AgentAdapterError) -> ProjectFailure {
    ProjectFailure::manifest(error.to_string())
}
fn json_failure(error: impl std::fmt::Display) -> ProjectFailure {
    ProjectFailure::manifest(format!("agent JSON: {error}"))
}
fn termination_config() -> TerminationConfig {
    TerminationConfig {
        term_grace_ms: 1_000,
        kill_grace_ms: 1_000,
        poll_interval_ms: 25,
    }
}

fn parse_ready(line: &[u8]) -> Result<u32, ProjectFailure> {
    let text = std::str::from_utf8(line)
        .map_err(|_| ProjectFailure::recovery("launcher handshake is not UTF-8"))?;
    text.strip_prefix("ready:")
        .and_then(|value| value.trim().parse().ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| ProjectFailure::recovery("launcher handshake is invalid"))
}
fn read_bounded_line<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, String> {
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| format!("agent event read failed: {error}"))?;
        if available.is_empty() {
            return Ok(line);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        let framed = line
            .len()
            .saturating_add(take)
            .saturating_add(usize::from(newline.is_some()));
        if framed > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
            return Err("agent event exceeds the bounded line size".into());
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(line);
        }
    }
}

pub(crate) fn run_agent_fixture_launcher(scenario: &str) -> Result<(), SidecarError> {
    if ![
        "clean",
        "no-output",
        "final-delayed",
        "cancel-clean",
        "cancel-uncertain",
        "malformed",
        "oversized",
        "descendant",
        "handshake-before-public",
        "handshake-version-mismatch",
        "handshake-model-mismatch",
        "handshake-cwd-mismatch",
        "handshake-permission-mismatch",
        "handshake-tools-mismatch",
        "handshake-mcp-mismatch",
        "handshake-plugins-mismatch",
        "handshake-config-mismatch",
        "provider-auth-failure",
        "provider-terminal-malformed",
        "public-text-exact",
        "public-aggregate-exact",
        "public-aggregate-plus-one",
        "provider-terminal-failed",
        "provider-terminal-recovery-required",
    ]
    .contains(&scenario)
    {
        return Err(SidecarError::new(
            "invalidArguments",
            "unknown agent fixture scenario",
        ));
    }
    let executable = std::env::current_exe().map_err(|error| {
        SidecarError::new("fixtureFailure", format!("fixture executable: {error}"))
    })?;
    let mut child = Command::new(executable)
        .args(["--agent-fixture-child", scenario])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            SidecarError::new("fixtureFailure", format!("agent fixture child: {error}"))
        })?;
    writeln!(std::io::stdout(), "ready:{}", child.id()).map_err(|error| {
        SidecarError::new("fixtureFailure", format!("launcher handshake: {error}"))
    })?;
    let mut release = String::new();
    std::io::stdin().read_line(&mut release).map_err(|error| {
        SidecarError::new("fixtureFailure", format!("launcher release: {error}"))
    })?;
    if release.trim() != "run" {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(());
    }
    child
        .stdin
        .take()
        .ok_or_else(|| SidecarError::new("fixtureFailure", "child release pipe missing"))?
        .write_all(b"run\n")
        .map_err(|error| SidecarError::new("fixtureFailure", format!("child release: {error}")))?;
    if let Some(mut output) = child.stdout.take() {
        std::io::copy(&mut output, &mut std::io::stdout()).map_err(|error| {
            SidecarError::new("fixtureFailure", format!("fixture copy: {error}"))
        })?;
    }
    let status = child
        .wait()
        .map_err(|error| SidecarError::new("fixtureFailure", format!("fixture wait: {error}")))?;
    if !status.success() {
        return Err(SidecarError::new(
            "fixtureFailure",
            "agent fixture child failed",
        ));
    }
    Ok(())
}

pub(crate) fn run_agent_fixture_child(scenario: &str) -> Result<(), SidecarError> {
    let mut release = String::new();
    std::io::stdin()
        .read_line(&mut release)
        .map_err(|error| SidecarError::new("fixtureFailure", format!("child release: {error}")))?;
    if release.trim() != "run" {
        return Ok(());
    }
    if scenario == "handshake-before-public" {
        println!("{{\"type\":\"status\",\"text\":\"must-not-escape\"}}");
        return Ok(());
    }
    if scenario == "provider-auth-failure" {
        println!("{{\"type\":\"authFailure\",\"authState\":\"expired\"}}");
        return Ok(());
    }
    let cwd = std::env::current_dir()
        .map_err(|error| SidecarError::new("fixtureFailure", format!("fixture cwd: {error}")))?;
    let mut handshake = json!({
        "type":"system",
        "subtype":"init",
        "claude_code_version":"2.1.241",
        "model":"claude-opus-5",
        "cwd":cwd.to_string_lossy(),
        "permissionMode":"workspaceWrite",
        "tools":["Edit","Read"],
        "mcp_servers":[],
        "plugins":[],
        "configIsolation":"managedOnly"
    });
    match scenario {
        "handshake-version-mismatch" => handshake["claude_code_version"] = json!("2.1.242"),
        "handshake-model-mismatch" => handshake["model"] = json!("claude-unknown"),
        "handshake-cwd-mismatch" => handshake["cwd"] = json!("/not/managed"),
        "handshake-permission-mismatch" => handshake["permissionMode"] = json!("readOnly"),
        "handshake-tools-mismatch" => handshake["tools"] = json!(["Read"]),
        "handshake-mcp-mismatch" => handshake["mcp_servers"] = json!(["unsafe"]),
        "handshake-plugins-mismatch" => handshake["plugins"] = json!(["unsafe"]),
        "handshake-config-mismatch" => handshake["configIsolation"] = json!("userAndProject"),
        _ => {}
    }
    println!(
        "{}",
        serde_json::to_string(&handshake).map_err(|error| {
            SidecarError::new("fixtureFailure", format!("fixture handshake: {error}"))
        })?
    );
    if scenario.starts_with("handshake-") {
        return Ok(());
    }
    match scenario {
        "no-output" => {}
        "malformed" => println!("not-json"),
        "oversized" => println!(
            "{{\"type\":\"status\",\"text\":\"{}\"}}",
            "x".repeat(MAX_AGENT_EVENT_BYTES + 1)
        ),
        "provider-terminal-malformed" => {
            println!("{{\"type\":\"terminal\",\"outcome\":\"unknown\"}}")
        }
        "public-text-exact" => {
            println!(
                "{}",
                json!({"type":"status","text":"x".repeat(MAX_AGENT_EVENT_BYTES)})
            );
            println!("{{\"type\":\"terminal\",\"outcome\":\"succeeded\"}}")
        }
        "public-aggregate-exact" | "public-aggregate-plus-one" => {
            for _ in 0..16 {
                println!(
                    "{}",
                    json!({"type":"status","text":"x".repeat(MAX_AGENT_EVENT_BYTES)})
                );
            }
            if scenario == "public-aggregate-plus-one" {
                println!("{{\"type\":\"status\",\"text\":\"x\"}}")
            }
            println!("{{\"type\":\"terminal\",\"outcome\":\"succeeded\"}}")
        }
        "provider-terminal-recovery-required" => {
            println!("{{\"type\":\"terminal\",\"outcome\":\"recoveryRequired\"}}")
        }
        "provider-terminal-failed" => {
            println!("{{\"type\":\"terminal\",\"outcome\":\"failed\"}}")
        }
        _ => {
            println!("{{\"type\":\"reasoning\",\"secret\":\"token-never-persist\"}}");
            println!("{{\"type\":\"status\",\"text\":\"Working\"}}");
            println!("{{\"type\":\"question\",\"text\":\"Continue with the reviewed change?\"}}");
            if scenario == "descendant" {
                let executable = std::env::current_exe().map_err(|error| {
                    SidecarError::new("fixtureFailure", format!("fixture executable: {error}"))
                })?;
                let mut grandchild = Command::new(executable)
                    .arg("--agent-fixture-grandchild")
                    .env_clear()
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|error| {
                        SidecarError::new("fixtureFailure", format!("fixture grandchild: {error}"))
                    })?;
                thread::sleep(Duration::from_secs(30));
                let _ = grandchild.kill();
                let _ = grandchild.wait();
            } else if scenario == "final-delayed" {
                println!("{{\"type\":\"final\",\"text\":\"Done\"}}");
                std::io::stdout().flush().map_err(|error| {
                    SidecarError::new("fixtureFailure", format!("fixture flush: {error}"))
                })?;
                thread::sleep(Duration::from_millis(500));
            } else if scenario.starts_with("cancel-") {
                thread::sleep(Duration::from_secs(30));
            } else {
                println!("{{\"type\":\"final\",\"text\":\"Done\"}}");
                println!("{{\"type\":\"terminal\",\"outcome\":\"succeeded\"}}");
            }
        }
    }
    Ok(())
}

pub(crate) fn run_agent_fixture_grandchild() {
    thread::sleep(Duration::from_secs(30));
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    fn public_event(index: usize, text: String) -> Value {
        json!({
            "schemaVersion":1,
            "eventId":format!("agent_event_{index}"),
            "chatId":"chat_a",
            "taskId":"task_a",
            "projectId":"project_a",
            "worktreeId":"worktree_a",
            "workspaceIdentity":"workspace_a",
            "runId":"agent_run_a",
            "roleId":"planning",
            "capabilityFingerprint":format!("sha256:{}", "0".repeat(64)),
            "kind":"status",
            "text":text,
            "createdAt":"2026-08-27T00:00:00Z",
        })
    }

    #[test]
    fn provider_raw_line_limit_counts_the_newline_framing_byte() {
        let mut exact = vec![b'x'; PROVIDER_OBSERVATION_LINE_MAX_BYTES - 1];
        exact.push(b'\n');
        assert_eq!(
            read_bounded_line(&mut std::io::Cursor::new(exact))
                .unwrap()
                .len(),
            PROVIDER_OBSERVATION_LINE_MAX_BYTES - 1
        );

        let mut over = vec![b'x'; PROVIDER_OBSERVATION_LINE_MAX_BYTES];
        over.push(b'\n');
        assert!(read_bounded_line(&mut std::io::Cursor::new(over)).is_err());
        assert_eq!(
            read_bounded_line(&mut std::io::Cursor::new(vec![
                b'x';
                PROVIDER_OBSERVATION_LINE_MAX_BYTES
            ]))
            .unwrap()
            .len(),
            PROVIDER_OBSERVATION_LINE_MAX_BYTES
        );
    }

    #[test]
    fn exact_resource_boundaries_pass_and_plus_one_fails() {
        for (exact, over) in [
            (
                PublicBudgetMetrics {
                    event_count: AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1,
                    text_bytes: 0,
                    cumulative_snapshot_bytes: 0,
                },
                PublicBudgetMetrics {
                    event_count: AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1 + 1,
                    text_bytes: 0,
                    cumulative_snapshot_bytes: 0,
                },
            ),
            (
                PublicBudgetMetrics {
                    event_count: 0,
                    text_bytes: AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1,
                    cumulative_snapshot_bytes: 0,
                },
                PublicBudgetMetrics {
                    event_count: 0,
                    text_bytes: AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1 + 1,
                    cumulative_snapshot_bytes: 0,
                },
            ),
            (
                PublicBudgetMetrics {
                    event_count: 0,
                    text_bytes: 0,
                    cumulative_snapshot_bytes:
                        AGENT_PUBLIC_LEDGER_CUMULATIVE_SNAPSHOT_BYTES_PER_RUN_MAX_V1,
                },
                PublicBudgetMetrics {
                    event_count: 0,
                    text_bytes: 0,
                    cumulative_snapshot_bytes:
                        AGENT_PUBLIC_LEDGER_CUMULATIVE_SNAPSHOT_BYTES_PER_RUN_MAX_V1 + 1,
                },
            ),
        ] {
            enforce_public_budget(exact).expect("exact boundary");
            assert!(enforce_public_budget(over).is_err(), "+1 must fail");
        }
    }

    #[test]
    fn active_actual_run_limit_is_checked_at_exact_boundary() {
        assert!(!active_run_limit_reached(
            AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1 - 1
        ));
        assert!(active_run_limit_reached(
            AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1
        ));
    }

    #[test]
    fn valid_public_ledgers_reach_count_and_text_boundaries_but_reject_plus_one() {
        let mut count_events = (0..AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1)
            .map(|index| public_event(index, "x".into()))
            .collect::<Vec<_>>();
        let count_metrics = public_budget_metrics(&count_events).expect("exact count boundary");
        assert_eq!(
            count_metrics.event_count,
            AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1
        );
        count_events.push(public_event(count_events.len(), "x".into()));
        assert!(public_budget_metrics(&count_events).is_err());

        let mut text_events = (0..16)
            .map(|index| public_event(index, "x".repeat(64 * 1024)))
            .collect::<Vec<_>>();
        let text_metrics = public_budget_metrics(&text_events).expect("exact text boundary");
        assert_eq!(
            text_metrics.text_bytes,
            AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1
        );
        text_events.push(public_event(text_events.len(), "x".into()));
        assert!(public_budget_metrics(&text_events).is_err());
    }
}
