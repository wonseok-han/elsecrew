//! Planned G5A actual-agent boundary with a deterministic fake adapter gate.
//!
//! Production construction is deliberately blocked: it can report capabilities but cannot create a
//! process. Tests may inject an exact fake manifest and the sidecar binary as the only executable.

use crate::chat::{G5Action, G5Event};
use crate::project::{ProjectFailure, ProjectManager, WorkspaceAddress};
use crate::storage::{ProjectionUpsert, Storage};
use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use hello_agent_core::agent_adapter::{
    normalize_vendor_event, AgentAdapterError, LaunchSpec, NormalizedVendorEvent,
    MAX_AGENT_EVENT_BYTES,
};
use hello_agent_core::proctree::{
    identity_of, ProbeResult, ProcIdentity, ProcTable, SystemClock, SystemProcTable,
    TerminationConfig, TreeTracker,
};
use hello_agent_protocol::agent_adapter_v1::{
    compute_agent_capability_fingerprint_v1, validate_agent_adapter_payload_v1,
    AgentCapabilitySnapshotV1, AgentDiscoveryEvidenceV1, AgentExecutableIdentityV1,
    AgentModelCapabilityV1, AgentResolvedProfileV1, AgentRunStartRequestV1,
};
use hello_agent_protocol::run_recovery_v1::{validate_g5_payload_v1, G5_RUN_START_CHAT_STATES_V1};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const SOURCE_KIND: &str = "actualAgent";

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
}

struct ActiveAgentRun {
    launcher: Child,
    tracker: TreeTracker,
    receiver: Receiver<Result<Vec<u8>, String>>,
    identity: RunIdentity,
    profile: AgentResolvedProfileV1,
    stream_id: String,
    next_wire_seq: u64,
    scenario: FakeScenario,
    evidence_failed: bool,
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
}

impl AgentRuntime {
    pub(crate) fn blocked() -> Self {
        Self {
            provider: ProviderMode::Blocked,
            fake_executable: None,
            scenario: FakeScenario::Clean,
            active: HashMap::new(),
            preparing_to_quit: false,
        }
    }

    pub(crate) fn recover_unclean_runs(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Result<(), SidecarError> {
        for projection in storage.projection_records("run")? {
            if projection.get("source_kind").and_then(Value::as_str) != Some(SOURCE_KIND)
                || projection.get("state").and_then(Value::as_str) != Some("recoveryRequired")
                || projection
                    .get("recovery")
                    .is_some_and(|value| !value.is_null())
            {
                continue;
            }
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
            if tracked.is_empty() {
                return Err(SidecarError::new(
                    "recoveryInvalid",
                    "unclean agent run has no durable process identity barrier",
                ));
            }
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
            let termination = json!({"schemaVersion":1,"runId":run_id,"outcome":"recoveryRequired","processesProvenGone":false,"poisoned":true,"writerLockState":"recoveryRequired","chatState":"recoveryRequired","trackedProcesses":tracked,"evidenceDigest":digest_text("unclean-agent-recovery-v1"),"observedAt":now()});
            validate_agent_adapter_payload_v1("terminationTranscript", &termination)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.to_string()))?;
            let recovery = json!({"schemaVersion":1,"recoveryId":format!("agent_recovery_unclean_{run_id}"),"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"source":"actualAgent","reason":"uncleanShutdown","state":"unresolved","trackedProcesses":tracked,"poisoned":true,"revision":1,"createdAt":now(),"resolvedAt":null});
            validate_agent_adapter_payload_v1("recoveryRecord", &recovery)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.to_string()))?;
            let address = address_from_chat(&chat)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            projects
                .workspace_mark_recovery_required(storage, &address)
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let recovered_chat = updated_chat(&chat, "recoveryRequired")
                .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
            let mut run_payload = agent_run_projection(
                &run,
                &projection["workspace_manifest"],
                projection.get("barrier").cloned(),
                projection.get("process_identity_internal").cloned(),
                projection["public_events"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            )
            .map_err(|error| SidecarError::new("recoveryInvalid", error.detail))?;
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
        Ok(())
    }

    pub(crate) fn fake(executable: PathBuf, scenario: FakeScenario) -> Self {
        Self {
            provider: ProviderMode::FakeExact,
            fake_executable: Some(executable),
            scenario,
            active: HashMap::new(),
            preparing_to_quit: false,
        }
    }

    pub(crate) fn advertises_g5a(&self) -> bool {
        self.provider == ProviderMode::FakeExact
    }

    pub(crate) fn get_capabilities(
        &self,
        runtime: &str,
        refresh: bool,
    ) -> Result<Value, ProjectFailure> {
        let request = json!({"runtime":runtime,"refresh":refresh});
        validate_agent_adapter_payload_v1("capabilitiesRequest", &request)
            .map_err(contract_failure)?;
        let snapshot = self.capability(runtime)?;
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
        let capability = self.capability(&profile.runtime)?;
        let outcome = preflight_outcome(&request, &profile, &capability);
        if outcome != "started" {
            return Err(ProjectFailure::manifest(format!(
                "agent preflight rejected before run, spawn, or lock: {outcome}"
            )));
        }
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
            "agent-fixture-ready-v1",
            "agent-fixture-jsonl-v1",
        )
        .map_err(adapter_failure)?;

        let run_id = format!("agent_run_{}", Uuid::new_v4().simple());
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
        let mut upserts = vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: format!("op_agent_run_prepare_{request_id}"),
            request_id: Some(request_id.to_string()),
            payload: agent_run_projection(&run, &manifest, None, None, Vec::new())?,
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
            let running = agent_run_record(
                &run_id,
                &chat,
                &profile,
                &capability.fingerprint,
                "running",
                None,
                &created,
            )?;
            let internal = json!([
                {"pid":launcher_identity.pid,"start_us":launcher_identity.start_us},
                {"pid":agent_identity.pid,"start_us":agent_identity.start_us}
            ]);
            let running_payload = agent_run_projection(
                &running,
                &manifest,
                Some(barrier.clone()),
                Some(internal),
                Vec::new(),
            )?;
            let running_chat = updated_chat(&chat, "running")?;
            let running_chat_payload =
                chat_projection_with_record(&chat_projection, &running_chat, None)?;
            Ok((barrier, running_payload, running_chat_payload))
        })();
        let (barrier, running_payload, running_chat_payload) = match configured {
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
        if let Err(error) = storage.upsert_projection_records_atomic(vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: run_id.clone(),
                operation_id: format!("op_agent_identity_{run_id}"),
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
                stream_id: stream_id.clone(),
                next_wire_seq: 0,
                scenario: self.scenario,
                evidence_failed: false,
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
    ) -> Vec<G5Event> {
        let ids = self.active.keys().cloned().collect::<Vec<_>>();
        let mut events = Vec::new();
        for run_id in ids {
            let mut public = Vec::new();
            let mut parser_failed = false;
            let mut observed_identities = None;
            if let Some(active) = self.active.get_mut(&run_id) {
                if active.tracker.observe(&SystemProcTable).is_err() {
                    active.evidence_failed = true;
                }
                observed_identities = Some(active.tracker.tracked_identities());
                loop {
                    match active.receiver.try_recv() {
                        Ok(Ok(line)) => match normalize_vendor_event(&line) {
                            Ok(NormalizedVendorEvent::Public { kind, text }) => {
                                public.push((kind, text))
                            }
                            Ok(NormalizedVendorEvent::Private) => {}
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
            if let Some(identities) = observed_identities {
                if let Err(_error) = persist_tracker_snapshot(storage, &run_id, &identities) {
                    if let Some(active) = self.active.get_mut(&run_id) {
                        active.evidence_failed = true;
                    }
                }
            }
            let finished = self
                .active
                .get_mut(&run_id)
                .and_then(|active| active.launcher.try_wait().ok())
                .flatten();
            if finished.is_some() {
                if let Some(active) = self.active.get_mut(&run_id) {
                    loop {
                        match active.receiver.recv_timeout(Duration::from_millis(100)) {
                            Ok(Ok(line)) => match normalize_vendor_event(&line) {
                                Ok(NormalizedVendorEvent::Public { kind, text }) => {
                                    public.push((kind, text))
                                }
                                Ok(NormalizedVendorEvent::Private) => {}
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
            for (kind, text) in public {
                if let Ok(event) = self.persist_public_event(storage, &run_id, kind, &text) {
                    events.push(event);
                } else {
                    parser_failed = true;
                    break;
                }
            }
            if parser_failed || finished.is_some() {
                let state = if parser_failed || finished.is_some_and(|status| !status.success()) {
                    "failed"
                } else {
                    "succeeded"
                };
                let _ = self.finish(storage, projects, &run_id, state, false);
            }
        }
        events
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
        self.finish(storage, projects, &identity.run_id, "canceled", true)?;
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
        Ok(G5Action {
            result,
            events: Vec::new(),
        })
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
        let projection =
            actual_run_projection(storage, request["runId"].as_str().unwrap_or_default())?;
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
        projects.workspace_abandon_recovery(&address)?;
        let mut after = observed_before.clone();
        after["trackedProcesses"] = Value::Array(tracked);
        after["poisoned"] = Value::Bool(false);
        after["state"] = Value::String("resolvedAbandoned".into());
        after["revision"] = Value::from(before["revision"].as_u64().unwrap_or(0) + 1);
        after["resolvedAt"] = Value::String(now());
        let mut updated_projection = projection.clone();
        updated_projection.remove("last_event_sequence");
        updated_projection.insert("recovery".into(), after.clone());
        let mut upserts = vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: identity_string(request, "runId")?,
            operation_id: format!("op_agent_recovery_resolve_{request_id}"),
            request_id: Some(request_id.into()),
            payload: updated_projection,
        }];
        if projection["record"]["selectedProfile"]["permission"] == "workspaceWrite" {
            upserts.push(release_writer_lock(
                storage,
                &address,
                request["runId"].as_str().unwrap_or_default(),
                request_id,
                false,
            )?);
        }
        storage.upsert_projection_records_atomic(upserts)?;
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

    fn capability(&self, runtime: &str) -> Result<AgentCapabilitySnapshotV1, ProjectFailure> {
        let platform = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "win32"
        };
        let mut snapshot = match self.provider {
            ProviderMode::Blocked => AgentCapabilitySnapshotV1 {
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
            },
            ProviderMode::FakeExact => {
                let executable = self
                    .fake_executable
                    .as_ref()
                    .ok_or_else(|| ProjectFailure::manifest("fake executable missing"))?;
                let models = if runtime == "codex" {
                    vec![AgentModelCapabilityV1 {
                        model_selector: "sol".into(),
                        reasoning_efforts: vec!["medium".into(), "high".into()],
                        permissions: vec!["readOnly".into()],
                    }]
                } else {
                    vec![AgentModelCapabilityV1 {
                        model_selector: "opus".into(),
                        reasoning_efforts: Vec::new(),
                        permissions: vec!["workspaceWrite".into()],
                    }]
                };
                AgentCapabilitySnapshotV1 {
                    schema_version: 1,
                    runtime: runtime.into(),
                    platform: platform.into(),
                    installation_state: "installed".into(),
                    auth_state: "authenticated".into(),
                    executable: Some(AgentExecutableIdentityV1 {
                        path_digest: digest_path(executable),
                        file_identity_digest: digest_text("agent-fixture-binary-v1"),
                        cli_version: "fixture-1".into(),
                    }),
                    models,
                    discovery: AgentDiscoveryEvidenceV1 {
                        source: "exactVersionManifest".into(),
                        verification_level: "measuredExactVersion".into(),
                        exact_cli_version: Some("fixture-1".into()),
                        effective_model_evidence: "measuredExactVersion".into(),
                    },
                    can_start: platform == "darwin",
                    blocked_reason: (platform != "darwin").then(|| "platformUnsupported".into()),
                    fingerprint: String::new(),
                    observed_at: now(),
                }
            }
        };
        snapshot.fingerprint =
            compute_agent_capability_fingerprint_v1(&snapshot).map_err(contract_failure)?;
        Ok(snapshot)
    }

    fn persist_public_event(
        &mut self,
        storage: &mut Storage,
        run_id: &str,
        kind: &str,
        text: &str,
    ) -> Result<G5Event, ProjectFailure> {
        let active = self
            .active
            .get_mut(run_id)
            .ok_or_else(|| ProjectFailure::manifest("active agent run is missing"))?;
        let projection = actual_run_projection(storage, run_id)?;
        let mut events = projection["public_events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let event = json!({"schemaVersion":1,"eventId":format!("agent_event_{}",Uuid::new_v4().simple()),"chatId":active.identity.chat_id,"taskId":active.identity.task_id,"projectId":active.identity.project_id,"worktreeId":active.identity.worktree_id,"workspaceIdentity":active.identity.workspace_identity,"runId":run_id,"roleId":active.profile.role_id,"capabilityFingerprint":projection["record"]["capabilityFingerprint"],"kind":kind,"text":text,"createdAt":now()});
        validate_agent_adapter_payload_v1("publicEvent", &event).map_err(contract_failure)?;
        events.push(event.clone());
        let mut updated = projection;
        updated.remove("last_event_sequence");
        updated.insert("public_events".into(), Value::Array(events));
        let mut chat_projection = actual_chat_projection(storage, &active.identity.chat_id)?;
        let sequence = chat_projection["durable_event_sequence"]
            .as_u64()
            .unwrap_or(0)
            + 1;
        let message_kind = if kind == "final" { "role" } else { "status" };
        let role_id = (kind == "final").then(|| active.profile.role_id.clone());
        let message = json!({"schemaVersion":1,"messageId":format!("message_{}",Uuid::new_v4().simple()),"chatId":active.identity.chat_id,"taskId":active.identity.task_id,"worktreeId":active.identity.worktree_id,"eventSequence":sequence,"kind":message_kind,"roleId":role_id,"runId":run_id,"text":text,"createdAt":now()});
        validate_g5_payload_v1("teamMessage", &message).map_err(|error| {
            ProjectFailure::manifest(format!("invalid actual-agent public message: {error}"))
        })?;
        chat_projection.remove("last_event_sequence");
        chat_projection.insert("last_message".into(), message);
        chat_projection.insert("durable_event_sequence".into(), Value::from(sequence));
        let operation = event["eventId"].as_str().unwrap_or_default();
        storage.upsert_projection_records_atomic(vec![
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
        ])?;
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

    fn finish(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        run_id: &str,
        requested_state: &str,
        cancelling: bool,
    ) -> Result<(), ProjectFailure> {
        let mut active = self
            .active
            .remove(run_id)
            .ok_or_else(|| ProjectFailure::manifest("active agent run is missing"))?;
        let report = active.tracker.terminate(
            &SystemProcTable,
            &SystemClock::default(),
            termination_config(),
        );
        let _ = active.launcher.wait();
        let forced_uncertain =
            active.scenario == FakeScenario::CancelUncertain || active.evidence_failed;
        let clean = report.confirmed() && !forced_uncertain;
        let state = if clean {
            requested_state
        } else {
            "recoveryRequired"
        };
        let outcome = if clean {
            "cleanExit"
        } else {
            "recoveryRequired"
        };
        let projection = actual_run_projection(storage, run_id)?;
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
        let tracked = report
            .tracked
            .iter()
            .map(|identity| process_value(*identity, Path::new("/redacted/agent"), clean))
            .collect::<Vec<_>>();
        let lock_state = if clean {
            if active.profile.permission == "workspaceWrite" {
                "released"
            } else {
                "none"
            }
        } else {
            "recoveryRequired"
        };
        let termination = json!({"schemaVersion":1,"runId":run_id,"outcome":outcome,"processesProvenGone":clean,"poisoned":!clean,"writerLockState":lock_state,"chatState":if clean {requested_state} else {"recoveryRequired"},"trackedProcesses":tracked,"evidenceDigest":digest_text("agent-fixture-termination-v1"),"observedAt":now()});
        validate_agent_adapter_payload_v1("terminationTranscript", &termination)
            .map_err(contract_failure)?;
        let recovery = (!clean).then(||json!({"schemaVersion":1,"recoveryId":format!("agent_recovery_{}",Uuid::new_v4().simple()),"chatId":active.identity.chat_id,"taskId":active.identity.task_id,"projectId":active.identity.project_id,"worktreeId":active.identity.worktree_id,"workspaceIdentity":active.identity.workspace_identity,"runId":run_id,"source":"actualAgent","reason":if cancelling {"cancelUncertain"} else {"processIdentityUncertain"},"state":"unresolved","trackedProcesses":tracked,"poisoned":true,"revision":1,"createdAt":now(),"resolvedAt":null}));
        if let Some(value) = &recovery {
            validate_agent_adapter_payload_v1("recoveryRecord", value).map_err(contract_failure)?;
            projects.workspace_mark_recovery_required(storage, &address_from_chat(&chat)?)?;
        }
        let updated_chat = updated_chat(
            &chat,
            if clean {
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
            } else {
                "recoveryRequired"
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
        if active.profile.permission == "workspaceWrite" {
            upserts.push(release_writer_lock(
                storage,
                &address_from_chat(&chat)?,
                run_id,
                run_id,
                !clean,
            )?);
        }
        storage.upsert_projection_records_atomic(upserts)?;
        Ok(())
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
        payload.insert(
            "pre_release_failure".into(),
            json!({"stage":"beforeRelease","evidenceDigest":digest_text(detail),"processesProvenGone":clean,"observedAt":now()}),
        );
        let recovery = if public.is_empty() || clean {
            None
        } else {
            Some(
                json!({"schemaVersion":1,"recoveryId":format!("agent_recovery_{}",Uuid::new_v4().simple()),"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"source":"actualAgent","reason":"processIdentityUncertain","state":"unresolved","trackedProcesses":public,"poisoned":true,"revision":1,"createdAt":now(),"resolvedAt":null}),
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
    Ok(json!({"run_id":run["runId"],"chat_id":run["chatId"],"state":run["state"],"worktree_path":manifest["worktreePath"],"source_kind":SOURCE_KIND,"record":run,"workspace_manifest":manifest,"barrier":barrier,"process_identity_internal":internal,"public_events":events,"termination":null,"recovery":null}).as_object().expect("agent projection").clone())
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
    Ok(value)
}
fn actual_chat_projection(
    storage: &Storage,
    chat_id: &str,
) -> Result<Map<String, Value>, ProjectFailure> {
    storage
        .projection_record("chat", chat_id)?
        .ok_or_else(|| ProjectFailure::manifest("chat projection is missing"))
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
        FakeScenario::CancelClean => "cancel-clean",
        FakeScenario::CancelUncertain => "cancel-uncertain",
        FakeScenario::Malformed => "malformed",
        FakeScenario::Oversized => "oversized",
        FakeScenario::Descendant => "descendant",
        FakeScenario::SpawnFailure => "clean",
        FakeScenario::IdentityFailure => "clean",
        FakeScenario::ObserveFailure => "clean",
        FakeScenario::ReleaseFailure => "clean",
    }
}
fn contract_failure(error: impl std::fmt::Display) -> ProjectFailure {
    ProjectFailure::manifest(format!("agent adapter contract: {error}"))
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
        if line.len().saturating_add(take) > MAX_AGENT_EVENT_BYTES {
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
        "cancel-clean",
        "cancel-uncertain",
        "malformed",
        "oversized",
        "descendant",
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
    match scenario {
        "malformed" => println!("not-json"),
        "oversized" => println!(
            "{{\"type\":\"status\",\"text\":\"{}\"}}",
            "x".repeat(MAX_AGENT_EVENT_BYTES)
        ),
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
            } else if scenario.starts_with("cancel-") {
                thread::sleep(Duration::from_secs(30));
            } else {
                println!("{{\"type\":\"final\",\"text\":\"Done\"}}");
            }
        }
    }
    Ok(())
}

pub(crate) fn run_agent_fixture_grandchild() {
    thread::sleep(Duration::from_secs(30));
}
