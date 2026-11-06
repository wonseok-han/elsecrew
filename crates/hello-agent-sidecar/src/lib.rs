//! M5 Rust sidecar의 G4 프로젝트·파일 안전 경계와 bounded JSONL 전송 구현.
//!
//! 실제 유료 agent 실행과 원본 적용은 후속 게이트에서 추가한다.

mod agent_runtime;
mod application_cancel;
mod dispatch_timing;
mod chat;
mod project;
#[cfg(windows)]
mod git_preparation;
mod process_spawn;
#[cfg(target_os = "macos")]
mod provider_claude_login;
#[cfg(target_os = "macos")]
mod provider_claude_profile;
#[cfg(unix)]
#[allow(dead_code)] // Pin validation is not vendor activation or signature admission.
mod provider_executable;
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Internal gate; executable admission and vendor environment remain separate.
mod provider_login_gate;
#[cfg(unix)]
#[allow(dead_code)] // Internal login I/O; the isolated process owner is not activated yet.
mod provider_login_io;
#[cfg(target_os = "macos")]
mod provider_login_manager;
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Installer worker/IPC integration is not activated yet.
mod provider_install_owner;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
// Lifecycle owner; validated vendor launcher and IPC activation remain separate.
mod provider_login_process;
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Internal session, pending validated vendor launcher and IPC/UI activation.
mod provider_login_session;
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Internal broker; native compatibility/product admission pending.
mod provider_model_proxy;
#[cfg(all(test, target_os = "macos"))]
mod provider_permission_probe;
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Internal preflight; not product execution authority.
mod provider_preflight_session;
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Internal Claude control IO; launch policy and runtime activation remain separate.
mod provider_claude_session;
#[cfg(unix)]
#[allow(dead_code)] // Prepared profile capability; login transport is not activated yet.
mod provider_profile;
#[cfg(target_os = "macos")]
mod provider_runtime_sandbox;
mod provider_run_binding;
mod storage;
#[allow(dead_code)] // D1A keeps status internal until the later wire-activation wave.
mod storage_lifetime;

use chrono::{SecondsFormat, Utc};
use hello_agent_core as _;
use hello_agent_protocol::agent_adapter_v1::G5D_AGENT_FIXTURE_CAPABILITIES_V1;
use hello_agent_protocol::lifecycle_g5d_v1::{
    DatabaseStatusG5DV1, StorageAdmissionStateV1, StorageAdmissionV1, SystemStateG5DV1,
    SystemStatusG5DV1, G5D_ALLOWED_WHILE_STORAGE_NOT_READY_V1, STORAGE_ADMISSION_FAILED_DETAIL_V1,
    STORAGE_VERIFYING_BLOCKED_DETAIL_V1,
};
use hello_agent_protocol::lifecycle_v1::SQLITE_FILE_V1;
use hello_agent_protocol::run_recovery_v1::G5D_PRODUCT_CAPABILITIES_V1;
use hello_agent_protocol::{
    decode_jsonl, encode_jsonl, EnvelopeV1, ErrorV1, EventV1, PrepareQuitResultV1,
    ProtocolSessionV1, ReadyPayloadV1, RequestV1, ResponseV1, APP_SCHEMA_VERSION,
    MAX_CONTROL_LINE_BYTES, METHODS_V1, PROTOCOL_VERSION, SIDECAR_SCHEMA_VERSION,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::{self, Display};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub use project::{
    ProjectFailure, ProjectManager, SaveFailurePoint, WorkspaceAddress, WorktreeFailurePoint,
};
pub use storage::{AppendFailurePoint, AppendOutcome, ProjectionUpsert, Storage};

pub const STARTUP_FAILURE_EXIT_CODE: i32 = 70;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarError {
    kind: &'static str,
    detail: String,
}

impl SidecarError {
    pub(crate) fn new(kind: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl Display for SidecarError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for SidecarError {}

pub fn run_cli<I>(args: I) -> Result<(), SidecarError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let _program = args.next();
    let mode = args.next();
    if mode.as_deref() == Some(std::ffi::OsStr::new("--git-preparation")) {
        if args.next().is_some() { return Err(SidecarError::new("invalidArguments", "Git preparation accepts no path arguments")); }
        #[cfg(windows)]
        { return git_preparation::run(); }
        #[cfg(not(windows))]
        { return Err(SidecarError::new("unsupportedPlatform", "Git preparation is Windows-only")); }
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--fixture-child")) {
        let scenario = args.next().ok_or_else(|| {
            SidecarError::new("invalidArguments", "fixture child scenario is required")
        })?;
        if args.next().is_some() {
            return Err(SidecarError::new(
                "invalidArguments",
                "fixture child has extra arguments",
            ));
        }
        return chat::run_fixture_child(&scenario.to_string_lossy());
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--fixture-leaf")) {
        if args.next().is_some() {
            return Err(SidecarError::new(
                "invalidArguments",
                "fixture leaf has extra arguments",
            ));
        }
        chat::run_fixture_leaf();
        return Ok(());
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--agent-fixture-launcher")) {
        let scenario = args.next().ok_or_else(|| {
            SidecarError::new(
                "invalidArguments",
                "agent fixture launcher scenario is required",
            )
        })?;
        if args.next().is_some() {
            return Err(SidecarError::new(
                "invalidArguments",
                "agent fixture launcher has extra arguments",
            ));
        }
        return agent_runtime::run_agent_fixture_launcher(&scenario.to_string_lossy());
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--agent-fixture-child")) {
        let scenario = args.next().ok_or_else(|| {
            SidecarError::new(
                "invalidArguments",
                "agent fixture child scenario is required",
            )
        })?;
        if args.next().is_some() {
            return Err(SidecarError::new(
                "invalidArguments",
                "agent fixture child has extra arguments",
            ));
        }
        return agent_runtime::run_agent_fixture_child(&scenario.to_string_lossy());
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--agent-fixture-grandchild")) {
        if args.next().is_some() {
            return Err(SidecarError::new(
                "invalidArguments",
                "agent fixture grandchild has extra arguments",
            ));
        }
        agent_runtime::run_agent_fixture_grandchild();
        return Ok(());
    }
    let fixture_runtime = if mode.as_deref() == Some(std::ffi::OsStr::new("--stdio-agent-fixture"))
    {
        let scenario = args.next().ok_or_else(|| {
            SidecarError::new(
                "invalidArguments",
                "stdio agent fixture scenario is required",
            )
        })?;
        if args.next().is_some() {
            return Err(SidecarError::new(
                "invalidArguments",
                "stdio agent fixture has extra arguments",
            ));
        }
        let scenario = match scenario.to_string_lossy().as_ref() {
            "clean" => agent_runtime::FakeScenario::Clean,
            // Standard release builds retain the durable-fixture rejection.
            // QA can exercise the same optimized storage/transport code via an
            // explicitly selected build, without changing normal --stdio mode.
            #[cfg(any(debug_assertions, feature = "qa-output-fixtures"))]
            "long-stream" => agent_runtime::FakeScenario::LongStream,
            #[cfg(any(debug_assertions, feature = "qa-output-fixtures"))]
            "paced-long-stream" => agent_runtime::FakeScenario::PacedLongStream,
            "final-delayed" => agent_runtime::FakeScenario::FinalDelayed,
            "cancel-clean" => agent_runtime::FakeScenario::CancelClean,
            "cancel-uncertain" => agent_runtime::FakeScenario::CancelUncertain,
            "malformed" => agent_runtime::FakeScenario::Malformed,
            "oversized" => agent_runtime::FakeScenario::Oversized,
            "descendant" => agent_runtime::FakeScenario::Descendant,
            "discovery-missing-helper" => agent_runtime::FakeScenario::DiscoveryMissingHelper,
            "discovery-unknown-version" => agent_runtime::FakeScenario::DiscoveryUnknownVersion,
            "discovery-unknown-auth" => agent_runtime::FakeScenario::DiscoveryUnknownAuth,
            "discovery-stale-closure" => agent_runtime::FakeScenario::DiscoveryStaleClosure,
            "discovery-profile-drift" => agent_runtime::FakeScenario::DiscoveryProfileDrift,
            "handshake-before-public" => agent_runtime::FakeScenario::HandshakeBeforePublic,
            "handshake-version-mismatch" => agent_runtime::FakeScenario::HandshakeVersionMismatch,
            "handshake-model-mismatch" => agent_runtime::FakeScenario::HandshakeModelMismatch,
            "handshake-cwd-mismatch" => agent_runtime::FakeScenario::HandshakeCwdMismatch,
            "handshake-permission-mismatch" => {
                agent_runtime::FakeScenario::HandshakePermissionMismatch
            }
            "handshake-tools-mismatch" => agent_runtime::FakeScenario::HandshakeToolsMismatch,
            "handshake-mcp-mismatch" => agent_runtime::FakeScenario::HandshakeMcpMismatch,
            "handshake-plugins-mismatch" => agent_runtime::FakeScenario::HandshakePluginsMismatch,
            "handshake-config-mismatch" => agent_runtime::FakeScenario::HandshakeConfigMismatch,
            "attestation-persistence-failure" => {
                agent_runtime::FakeScenario::AttestationPersistenceFailure
            }
            "provider-auth-failure" => agent_runtime::FakeScenario::ProviderAuthFailure,
            "provider-terminal-malformed" => agent_runtime::FakeScenario::ProviderTerminalMalformed,
            "public-text-exact" => agent_runtime::FakeScenario::PublicTextExact,
            "public-aggregate-exact" => agent_runtime::FakeScenario::PublicAggregateExact,
            "public-aggregate-plus-one" => agent_runtime::FakeScenario::PublicAggregatePlusOne,
            "provider-terminal-failed" => agent_runtime::FakeScenario::ProviderTerminalFailed,
            "provider-terminal-recovery-required" => {
                agent_runtime::FakeScenario::ProviderTerminalRecoveryRequired
            }
            _ => {
                return Err(SidecarError::new(
                    "invalidArguments",
                    "unknown stdio agent fixture scenario",
                ))
            }
        };
        Some(agent_runtime::AgentRuntime::fake(
            std::env::current_exe().map_err(|error| {
                SidecarError::new("fixtureFailure", format!("fixture executable: {error}"))
            })?,
            scenario,
        ))
    } else {
        if mode.as_deref() != Some(std::ffi::OsStr::new("--stdio")) || args.next().is_some() {
            return Err(SidecarError::new(
                "invalidArguments",
                "sidecar requires exactly one argument: --stdio",
            ));
        }
        None
    };

    let current = std::env::current_dir().map_err(|error| {
        SidecarError::new(
            "dataDirectoryUnavailable",
            format!("could not read cwd: {error}"),
        )
    })?;
    if !current.is_dir() {
        return Err(SidecarError::new(
            "dataDirectoryUnavailable",
            "sidecar cwd is not a directory",
        ));
    }
    let canonical = fs::canonicalize(&current).map_err(|error| {
        SidecarError::new(
            "dataDirectoryUnavailable",
            format!("could not canonicalize sidecar cwd: {error}"),
        )
    })?;
    let database_path = canonical.join(SQLITE_FILE_V1);
    let stdin = io::stdin();
    let stdout = io::stdout();
    match fixture_runtime {
        Some(runtime) => serve_stdio_with_identity_and_runtime(
            &database_path,
            stdin,
            stdout.lock(),
            new_sidecar_instance_id(),
            runtime,
        ),
        None => run_stdio(&database_path, stdin, stdout.lock()),
    }
}

pub fn run_stdio<R: Read + Send + 'static, W: Write>(
    database_path: &Path,
    input: R,
    output: W,
) -> Result<(), SidecarError> {
    serve_stdio_with_identity(database_path, input, output, new_sidecar_instance_id())
}

fn serve_stdio_with_identity<R: Read + Send + 'static, W: Write>(
    database_path: &Path,
    input: R,
    output: W,
    sidecar_instance_id: String,
) -> Result<(), SidecarError> {
    serve_stdio_with_identity_and_runtime(
        database_path,
        input,
        output,
        sidecar_instance_id,
        if cfg!(target_os = "macos") {
            agent_runtime::AgentRuntime::native_candidate()
        } else {
            agent_runtime::AgentRuntime::blocked()
        },
    )
}

fn serve_stdio_with_identity_and_runtime<R: Read + Send + 'static, W: Write>(
    database_path: &Path,
    input: R,
    mut output: W,
    sidecar_instance_id: String,
    runtime: agent_runtime::AgentRuntime,
) -> Result<(), SidecarError> {
    g4_platform_gate()?;
    let mut timings = dispatch_timing::DispatchTiming::default();
    let mut sidecar = SidecarSession::new(sidecar_instance_id, runtime);
    let ready = EnvelopeV1::Event(sidecar.ready_event()?);
    let mut session = ProtocolSessionV1::default();
    session.observe(&ready).map_err(protocol_error)?;
    write_envelope(&mut output, &ready)?;
    sidecar.start_admission(database_path);

    let (sender, receiver) = mpsc::sync_channel(1);
    let application_cancel = sidecar.application_cancel.clone();
    let input_instance = sidecar.ready.sidecar_instance_id.clone();
    thread::spawn(move || {
        let mut input = BufReader::with_capacity(8 * 1024, input);
        loop {
            let message = match read_bounded_jsonl(&mut input) {
                Ok(Some(line)) => InputMessage::Line(line),
                Ok(None) => InputMessage::Eof,
                Err(error) => InputMessage::Error(error),
            };
            let finished = !matches!(message, InputMessage::Line(_));
            if let InputMessage::Line(line) = &message {
                application_cancel.observe(line, &input_instance);
            } else {
                application_cancel.stop();
            }
            let sent = match sender.try_send(message) {
                Ok(()) => true,
                Err(mpsc::TrySendError::Full(message)) => {
                    // Bound memory without letting a blocked reader hide a later
                    // stop request from an in-flight source copy.
                    application_cancel.stop();
                    sender.send(message).is_ok()
                }
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            };
            if !sent || finished {
                break;
            }
        }
    });
    let runtime_poll_interval = Duration::from_millis(100);
    let mut last_runtime_poll = Instant::now();
    loop {
        // Requests and polling work consume the same interval. Starting a fresh
        // 100ms wait after either would add idle latency to every output batch.
        let wait = runtime_poll_interval.saturating_sub(last_runtime_poll.elapsed());
        let line = match receiver.recv_timeout(wait) {
            Ok(InputMessage::Line(line)) => line,
            Ok(InputMessage::Eof) => {
                if sidecar.safe_to_quit() {
                    return Ok(());
                }
                return Err(SidecarError::new(
                    "unexpectedEof",
                    "stdin closed before app.prepareQuit; clean shutdown was not marked",
                ));
            }
            Ok(InputMessage::Error(error)) => return Err(error),
            Err(RecvTimeoutError::Timeout) => {
                last_runtime_poll = Instant::now();
                sidecar.poll_admission();
                let mut events = timings.measure("watch", || sidecar.poll_watch_events());
                let (responses, runtime_events) = timings.measure("runtime", || sidecar.poll_runtime_events())?;
                write_responses(&mut session, &mut output, responses)?;
                events.extend(runtime_events);
                write_events(&mut session, &mut output, events)?;
                timings.measure("maintenance", || sidecar.maintenance_tick())?;
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(SidecarError::new(
                    "stdioFailure",
                    "stdin reader stopped without a terminal message",
                ))
            }
        };
        sidecar.poll_admission();
        let envelope = match preflight_blocked_admission_request(&line, &sidecar) {
            Some(request) => EnvelopeV1::Request(request),
            None => decode_jsonl(&line).map_err(protocol_error)?,
        };
        session.observe(&envelope).map_err(protocol_error)?;
        let EnvelopeV1::Request(request) = envelope else {
            return Err(SidecarError::new(
                "protocolViolation",
                "sidecar stdin accepts request envelopes only",
            ));
        };
        let handled = timings.measure("request", || sidecar.handle_request(request))?;
        if sidecar.safe_to_quit() { timings.emit(); }
        write_responses(&mut session, &mut output, handled.response)?;
        write_events(&mut session, &mut output, handled.events)?;
        // A busy renderer must not starve agent/watch progression by keeping
        // recv_timeout from expiring. Preserve response-before-event ordering.
        if !sidecar.safe_to_quit() && last_runtime_poll.elapsed() >= runtime_poll_interval {
            last_runtime_poll = Instant::now();
            let mut events = timings.measure("watch", || sidecar.poll_watch_events());
            let (responses, runtime_events) = timings.measure("runtime", || sidecar.poll_runtime_events())?;
            write_responses(&mut session, &mut output, responses)?;
            events.extend(runtime_events);
            write_events(&mut session, &mut output, events)?;
        }
        timings.measure("maintenance", || sidecar.maintenance_tick())?;
    }
}

fn write_responses<W: Write>(
    session: &mut ProtocolSessionV1,
    output: &mut W,
    responses: impl IntoIterator<Item = ResponseV1>,
) -> Result<(), SidecarError> {
    for response in responses {
        let envelope = EnvelopeV1::Response(response);
        session.observe(&envelope).map_err(protocol_error)?;
        write_envelope(output, &envelope)?;
    }
    Ok(())
}

enum InputMessage {
    Line(Vec<u8>),
    Eof,
    Error(SidecarError),
}

fn write_events<W: Write>(
    session: &mut ProtocolSessionV1,
    output: &mut W,
    events: Vec<EventV1>,
) -> Result<(), SidecarError> {
    for event in events {
        let envelope = EnvelopeV1::Event(event);
        session.observe(&envelope).map_err(protocol_error)?;
        write_envelope(output, &envelope)?;
    }
    Ok(())
}

fn read_bounded_jsonl<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, SidecarError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(io_error)?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(SidecarError::new(
                    "partialJsonl",
                    "stdin ended in the middle of a JSONL record",
                ))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        if line.len().saturating_add(take) > MAX_CONTROL_LINE_BYTES {
            return Err(SidecarError::new(
                "controlLineTooLarge",
                "control line exceeds 1 MiB before newline",
            ));
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            line.push(b'\n');
            return Ok(Some(line));
        }
    }
}

fn write_envelope<W: Write>(output: &mut W, envelope: &EnvelopeV1) -> Result<(), SidecarError> {
    let line = encode_jsonl(envelope).map_err(protocol_error)?;
    output.write_all(line.as_bytes()).map_err(io_error)?;
    output.flush().map_err(io_error)
}

fn preflight_blocked_admission_request(line: &[u8], sidecar: &SidecarSession) -> Option<RequestV1> {
    if sidecar.storage_is_ready() {
        return None;
    }
    let request: RequestV1 = serde_json::from_slice(line).ok()?;
    let valid_identity = request.protocol_version == PROTOCOL_VERSION
        && request.kind == "request"
        && !request.sidecar_instance_id.is_empty()
        && !request.request_id.is_empty()
        && METHODS_V1.contains(&request.method.as_str());
    if !valid_identity
        || request.params.is_object()
        || G5D_ALLOWED_WHILE_STORAGE_NOT_READY_V1.contains(&request.method.as_str())
    {
        return None;
    }
    Some(request)
}

struct Sidecar {
    #[cfg(target_os = "macos")]
    login: provider_login_manager::LoginManager,
    #[cfg(target_os = "macos")]
    claude_login: provider_login_manager::LoginManager,
    #[cfg(target_os = "macos")]
    install: provider_install_owner::manager::InstallManager,
    storage: Storage,
    projects: ProjectManager,
    ready: ReadyPayloadV1,
    prepared_to_quit: bool,
    safe_to_quit: bool,
    g5: chat::G5Manager,
    agent_runtime: agent_runtime::AgentRuntime,
    pending_agent_cancels: BTreeMap<String, (RequestV1, agent_runtime::RunIdentity)>,
    deferred_runtime_error: Option<SidecarError>,
    pending_agent_capabilities: BTreeMap<String, RequestV1>,
    pending_agent_starts: BTreeMap<String, PendingAgentStart>,
    maintenance_scheduler: storage_lifetime::scheduler::CooperativeScheduler,
    maintenance_poisoned: bool,
}

struct PendingAgentStart {
    runtime: String,
    request: RequestV1,
    input: hello_agent_protocol::agent_adapter_v1::AgentRunStartRequestV1,
    reservation: Option<agent_runtime::preparation::RunReservation>,
    #[cfg(target_os = "macos")]
    process: Option<provider_login_process::LoginProcess<'static>>,
    result: Option<Result<chat::G5Action, SidecarError>>,
}

struct AdmissionWorker {
    cancellation: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<Sidecar, SidecarError>>>,
}

enum AdmissionState {
    NotStarted {
        runtime: Option<agent_runtime::AgentRuntime>,
    },
    Verifying(AdmissionWorker),
    Ready {
        completed_at: String,
        sidecar: Box<Sidecar>,
    },
    Failed {
        completed_at: String,
    },
}

struct SidecarSession {
    application_cancel: application_cancel::ApplicationCancel,
    ready: ReadyPayloadV1,
    started_at: String,
    admission: AdmissionState,
    safe_to_quit: bool,
}

impl SidecarSession {
    fn new(sidecar_instance_id: String, runtime: agent_runtime::AgentRuntime) -> Self {
        let native_capabilities = hello_agent_protocol::agent_adapter_v1::g5d_native_capabilities_v1();
        let capabilities = if runtime.uses_native_providers() {
            native_capabilities.as_slice()
        } else if runtime.advertises_g5a() {
            G5D_AGENT_FIXTURE_CAPABILITIES_V1
        } else {
            G5D_PRODUCT_CAPABILITIES_V1
        };
        Self {
            application_cancel: Default::default(),
            ready: ReadyPayloadV1 {
                protocol_version: PROTOCOL_VERSION,
                sidecar_schema_version: SIDECAR_SCHEMA_VERSION,
                app_schema_version: APP_SCHEMA_VERSION,
                pid: std::process::id(),
                platform: protocol_platform().to_string(),
                capabilities: capabilities
                    .iter()
                    .chain(std::iter::once(&"chat.appendMessage"))
                    .chain(["agent.outputHead", "agent.outputPage"].iter())
                    .chain(std::iter::once(&"provider.login"))
                    .chain(std::iter::once(&"run.approval"))
                    .chain(std::iter::once(&"project.catalog"))
                    .chain(std::iter::once(&"provider.install"))
                    .map(|value| (*value).to_string())
                    .collect(),
                sidecar_instance_id,
            },
            started_at: now_utc(),
            admission: AdmissionState::NotStarted {
                runtime: Some(runtime),
            },
            safe_to_quit: false,
        }
    }

    fn ready_event(&self) -> Result<EventV1, SidecarError> {
        Ok(EventV1 {
            protocol_version: PROTOCOL_VERSION,
            kind: "event".to_string(),
            sidecar_instance_id: self.ready.sidecar_instance_id.clone(),
            stream_id: "system".to_string(),
            seq: 1,
            event: "system.ready".to_string(),
            chat_id: None,
            task_id: None,
            run_id: None,
            payload: serde_json::to_value(&self.ready).map_err(json_error)?,
        })
    }

    fn start_admission(&mut self, database_path: &Path) {
        self.start_admission_with_gate(database_path, None);
    }

    fn start_admission_with_gate(
        &mut self,
        database_path: &Path,
        start_gate: Option<Arc<AtomicBool>>,
    ) {
        let AdmissionState::NotStarted { runtime } = &mut self.admission else {
            return;
        };
        let runtime = runtime.take().expect("admission runtime starts once");
        let database_path = database_path.to_path_buf();
        let ready = self.ready.clone();
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation);
        let handle = thread::spawn(move || {
            while start_gate
                .as_ref()
                .is_some_and(|gate| !gate.load(Ordering::Acquire))
            {
                admission_not_cancelled(&worker_cancellation)?;
                thread::sleep(Duration::from_millis(1));
            }
            Sidecar::initialize(database_path, ready, runtime, &worker_cancellation)
        });
        self.admission = AdmissionState::Verifying(AdmissionWorker {
            cancellation,
            handle: Some(handle),
        });
    }

    fn poll_admission(&mut self) {
        let finished = matches!(
            &self.admission,
            AdmissionState::Verifying(worker)
                if worker.handle.as_ref().is_some_and(JoinHandle::is_finished)
        );
        if !finished {
            return;
        }
        let AdmissionState::Verifying(mut worker) = std::mem::replace(
            &mut self.admission,
            AdmissionState::Failed {
                completed_at: now_utc(),
            },
        ) else {
            unreachable!("finished admission state changed")
        };
        let outcome = worker
            .handle
            .take()
            .expect("finished admission worker")
            .join();
        self.admission = match outcome {
            Ok(Ok(mut sidecar)) => {
                sidecar.projects.application_cancel = self.application_cancel.clone();
                AdmissionState::Ready {
                    completed_at: now_utc(),
                    sidecar: Box::new(sidecar),
                }
            }
            Ok(Err(_)) | Err(_) => AdmissionState::Failed {
                completed_at: now_utc(),
            },
        };
    }

    fn handle_request(&mut self, request: RequestV1) -> Result<HandledRequest, SidecarError> {
        match self.handle_validated_request(request.clone()) {
            Err(error) if error.kind() == "invalidParams" => Ok(HandledRequest::response(
                error_response(&request, "invalidParams", "invalidRequest", error.detail()),
            )),
            other => other,
        }
    }

    fn handle_validated_request(
        &mut self,
        request: RequestV1,
    ) -> Result<HandledRequest, SidecarError> {
        if request.method == "run.startFixture" && !self.ready.capabilities.iter().any(|method| method == "run.startFixture") {
            return Ok(HandledRequest::response(error_response(
                &request, "unsupportedMethod", "requestBlocked", "fixture execution is unavailable in native mode",
            )));
        }
        match &mut self.admission {
            AdmissionState::Ready { sidecar, .. } => {
                if request.method == "system.status" {
                    require_exact_params(&request.params, &[]).map_err(invalid_params)?;
                    return Ok(HandledRequest::response(success_response(
                        &request,
                        serde_json::to_value(self.system_status()?).map_err(json_error)?,
                    )));
                }
                let handled = sidecar.handle_request(request)?;
                self.safe_to_quit = sidecar.safe_to_quit;
                Ok(handled)
            }
            AdmissionState::NotStarted { .. } | AdmissionState::Verifying(_) => {
                self.handle_not_ready_request(request, StorageAdmissionStateV1::Verifying)
            }
            AdmissionState::Failed { .. } => {
                self.handle_not_ready_request(request, StorageAdmissionStateV1::Failed)
            }
        }
    }

    fn handle_not_ready_request(
        &mut self,
        request: RequestV1,
        state: StorageAdmissionStateV1,
    ) -> Result<HandledRequest, SidecarError> {
        match request.method.as_str() {
            "system.status" => {
                require_exact_params(&request.params, &[]).map_err(invalid_params)?;
                Ok(HandledRequest::response(success_response(
                    &request,
                    serde_json::to_value(self.system_status()?).map_err(json_error)?,
                )))
            }
            "app.prepareQuit" => {
                require_exact_params(&request.params, &[]).map_err(invalid_params)?;
                self.cancel_and_join_admission();
                self.safe_to_quit = true;
                Ok(HandledRequest::response(success_response(
                    &request,
                    serde_json::to_value(PrepareQuitResultV1 {
                        safe_to_quit: true,
                        blockers: Vec::new(),
                    })
                    .map_err(json_error)?,
                )))
            }
            _ => Ok(HandledRequest::response(admission_blocked_response(
                &request, state,
            ))),
        }
    }

    fn cancel_and_join_admission(&mut self) {
        let AdmissionState::Verifying(worker) = &mut self.admission else {
            return;
        };
        worker.cancellation.store(true, Ordering::Release);
        let mut worker = match std::mem::replace(
            &mut self.admission,
            AdmissionState::Failed {
                completed_at: now_utc(),
            },
        ) {
            AdmissionState::Verifying(worker) => worker,
            _ => unreachable!("verifying admission state changed"),
        };
        if let Some(handle) = worker.handle.take() {
            let _ = handle.join();
        }
    }

    fn system_status(&self) -> Result<SystemStatusG5DV1, SidecarError> {
        let (storage_admission, database, recovery_required, blocker_ids) = match &self.admission {
            AdmissionState::Ready {
                completed_at,
                sidecar,
            } => {
                let blocker_ids = sidecar.storage.blocker_ids()?;
                (
                    StorageAdmissionV1 {
                        schema_version: 1,
                        state: StorageAdmissionStateV1::Ready,
                        started_at: self.started_at.clone(),
                        completed_at: Some(completed_at.clone()),
                        error: None,
                    },
                    DatabaseStatusG5DV1::Healthy {
                        schema_version: APP_SCHEMA_VERSION,
                    },
                    Some(!blocker_ids.is_empty()),
                    blocker_ids,
                )
            }
            AdmissionState::Failed { completed_at } => (
                StorageAdmissionV1 {
                    schema_version: 1,
                    state: StorageAdmissionStateV1::Failed,
                    started_at: self.started_at.clone(),
                    completed_at: Some(completed_at.clone()),
                    error: Some(ErrorV1 {
                        code: "storageAdmissionFailed".to_string(),
                        kind: "storageUnavailable".to_string(),
                        detail: STORAGE_ADMISSION_FAILED_DETAIL_V1.to_string(),
                        retryable: false,
                        outcome_unknown: Some(true),
                    }),
                },
                DatabaseStatusG5DV1::Unavailable {
                    schema_version: None,
                },
                None,
                Vec::new(),
            ),
            AdmissionState::NotStarted { .. } | AdmissionState::Verifying(_) => (
                StorageAdmissionV1 {
                    schema_version: 1,
                    state: StorageAdmissionStateV1::Verifying,
                    started_at: self.started_at.clone(),
                    completed_at: None,
                    error: None,
                },
                DatabaseStatusG5DV1::Verifying {
                    schema_version: None,
                },
                None,
                Vec::new(),
            ),
        };
        Ok(SystemStatusG5DV1 {
            schema_version: 1,
            state: SystemStateG5DV1::ShellReady,
            sidecar: self.ready.clone(),
            storage_admission,
            database,
            recovery_required,
            blocker_ids,
        })
    }

    fn poll_watch_events(&mut self) -> Vec<EventV1> {
        match &mut self.admission {
            AdmissionState::Ready { sidecar, .. } => sidecar.poll_watch_events(),
            _ => Vec::new(),
        }
    }

    fn poll_runtime_events(&mut self) -> Result<(Vec<ResponseV1>, Vec<EventV1>), SidecarError> {
        match &mut self.admission {
            AdmissionState::Ready { sidecar, .. } => sidecar.poll_runtime_events(),
            _ => Ok((Vec::new(), Vec::new())),
        }
    }

    fn safe_to_quit(&self) -> bool {
        self.safe_to_quit
    }

    fn storage_is_ready(&self) -> bool {
        matches!(self.admission, AdmissionState::Ready { .. })
    }

    fn maintenance_tick(&mut self) -> Result<(), SidecarError> {
        let AdmissionState::Ready { sidecar, .. } = &mut self.admission else {
            return Ok(());
        };
        sidecar.maintenance_tick().map(|_| ())
    }
}

impl Drop for SidecarSession {
    fn drop(&mut self) {
        self.cancel_and_join_admission();
    }
}

impl Sidecar {
    fn initialize(
        database_path: std::path::PathBuf,
        ready: ReadyPayloadV1,
        agent_runtime: agent_runtime::AgentRuntime,
        cancellation: &AtomicBool,
    ) -> Result<Self, SidecarError> {
        // macOS native and explicit long-output fixtures use the same verified
        // migration/admission path. Blocked platforms keep their existing format.
        let mut storage = if agent_runtime.uses_durable_output() {
            Storage::open_for_durable_admission(&database_path, cancellation)?
        } else {
            Storage::open_for_admission(&database_path, cancellation)?
        };
        admission_not_cancelled(cancellation)?;
        let mut projects = ProjectManager::new(&database_path)?;
        admission_not_cancelled(cancellation)?;
        storage.recover_unclean_shutdown()?;
        let g5 = chat::G5Manager::default();
        agent_runtime.recover_unclean_runs(&mut storage, &mut projects)?;
        agent_runtime.reconcile_resolved_catalogs(&mut storage, &projects)?;
        g5.recover_unclean_runs(&mut storage, &mut projects)?;
        admission_not_cancelled(cancellation)?;
        storage.register_app_instance(&ready)?;
        admission_not_cancelled(cancellation)?;
        Ok(Self {
            storage,
            #[cfg(target_os = "macos")]
            login: provider_login_manager::LoginManager::default(),
            #[cfg(target_os = "macos")]
            claude_login: provider_login_manager::LoginManager::claude(),
            #[cfg(target_os = "macos")]
            install: provider_install_owner::manager::InstallManager::default(),
            projects,
            ready,
            prepared_to_quit: false,
            safe_to_quit: false,
            g5,
            agent_runtime,
            pending_agent_cancels: BTreeMap::new(),
            deferred_runtime_error: None,
            pending_agent_capabilities: BTreeMap::new(),
            pending_agent_starts: BTreeMap::new(),
            maintenance_scheduler: storage_lifetime::scheduler::CooperativeScheduler::default(),
            maintenance_poisoned: false,
        })
    }

    fn handle_request(&mut self, request: RequestV1) -> Result<HandledRequest, SidecarError> {
        match self.handle_validated_request(request.clone()) {
            Err(error) if error.kind() == "invalidParams" => Ok(HandledRequest::response(
                error_response(&request, "invalidParams", "invalidRequest", error.detail()),
            )),
            other => other,
        }
    }

    fn handle_validated_request(
        &mut self,
        request: RequestV1,
    ) -> Result<HandledRequest, SidecarError> {
        if self.maintenance_poisoned && request.method != "system.status" {
            return Err(maintenance_poisoned_error());
        }
        // This single dispatcher serializes admission before any asynchronous
        // login, discovery, native run, or installer worker can be reserved.
        #[cfg(target_os = "macos")]
        if self.installation_conflicts(&request) {
            return Ok(HandledRequest::response(error_response(
                &request, "providerBusy", "requestBlocked",
                "installation and provider work cannot overlap; check status or request cancellation",
            )));
        }
        if self.projects.application_running()
            && !matches!(request.method.as_str(), "system.status" | "app.prepareQuit")
            && !(request.method == "project.catalog" && matches!(request.params["action"].as_str(), Some("applicationStatus" | "chatApplicationStatus" | "applicationHistory" | "cancelApplication")))
        {
            return Ok(HandledRequest::response(error_response(&request, "applicationBusy", "requestBlocked", "source application owns the active write slot")));
        }
        if self.prepared_to_quit
            && !matches!(
                request.method.as_str(),
                "app.prepareQuit"
                    | "system.status"
                    | "recovery.list"
                    | "recovery.inspect"
                    | "recovery.markResolved"
                    | "recovery.inspectAgent"
                    | "recovery.resolveAgent"
            )
        {
            return Ok(HandledRequest::response(error_response(
                &request,
                "quitting",
                "requestBlocked",
                "sidecar is prepared to quit and blocks new requests",
            )));
        }

        if matches!(
            request.method.as_str(),
            "project.getRoleSettings"
                | "project.updateRoleSettings"
                | "workspace.prepareChat"
                | "workspace.abandonPrepared"
                | "chat.create"
                | "chat.list"
                | "chat.get"
                | "chat.appendMessage"
                | "run.startFixture"
                | "run.cancel"
                | "recovery.list"
                | "recovery.inspect"
                | "recovery.markResolved"
                | "agent.getCapabilities"
                | "agent.getModelChoices"
                | "agent.outputHead"
                | "agent.outputPage"
                | "run.startAgent"
                | "run.cancelAgent"
                | "recovery.inspectAgent"
                | "recovery.resolveAgent"
        ) {
            return if matches!(
                request.method.as_str(),
                "agent.getCapabilities"
                    | "agent.getModelChoices"
                    | "agent.outputHead"
                    | "agent.outputPage"
                    | "run.startAgent"
                    | "run.cancelAgent"
                    | "recovery.inspectAgent"
                    | "recovery.resolveAgent"
            ) {
                self.handle_agent_request(request)
            } else {
                self.handle_g5_request(request)
            };
        }

        let action = match request.method.as_str() {
            "run.approval" => {
                let result = match self.agent_runtime.approval_request(
                    &mut self.storage,
                    &self.projects,
                    &request.params,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        return Ok(HandledRequest::response(error_response(
                            &request,
                            "approvalUnavailable",
                            error.kind(),
                            error.detail(),
                        )))
                    }
                };
                Ok(project::ProjectAction {
                    result,
                    ..project::ProjectAction::default()
                })
            }
            "provider.install" => {
                // User-approved macOS activation. Each execution still requires
                // a fresh destination-bound consent plan; status never installs.
                #[cfg(target_os = "macos")]
                let result = self.install.request(&self.storage, &request.params, true)?;
                #[cfg(not(target_os = "macos"))]
                let result = serde_json::json!({"state":"unsupported","operationId":null,"provider":null,"plan":null,"error":null,"cancelRequested":false});
                Ok(project::ProjectAction { result, ..project::ProjectAction::default() })
            }
            "provider.login" => {
                #[cfg(target_os = "macos")]
                let result =
                    if request.params.get("provider").and_then(Value::as_str) == Some("claude") {
                        self.claude_login.request_with_projects(&self.storage, &request.params, &self.projects)?
                    } else {
                        self.login.request_with_projects(&self.storage, &request.params, &self.projects)?
                    };
                #[cfg(not(target_os = "macos"))]
                let result = serde_json::json!({"state":"unsupported","attemptId":null,"authUrl":null,"error":null});
                Ok(project::ProjectAction {
                    result,
                    ..project::ProjectAction::default()
                })
            }
            "system.status" => Ok({
                require_exact_params(&request.params, &[]).map_err(invalid_params)?;
                project::ProjectAction {
                    result: Value::Null,
                    ..project::ProjectAction::default()
                }
            }),
            "storage.status" => Ok({
                hello_agent_protocol::storage_lifetime_v1::validate_storage_status_request_v1(
                    &request.params,
                )
                .map_err(|error| invalid_params(error.to_string()))?;
                project::ProjectAction {
                    result: serde_json::to_value(self.storage.lifetime_status()?)
                        .map_err(json_error)?,
                    ..project::ProjectAction::default()
                }
            }),
            "app.prepareQuit" => Ok({
                require_exact_params(&request.params, &[]).map_err(invalid_params)?;
                if !self.prepared_to_quit {
                    self.projects.stop_application();
                    self.projects.stop_watchers();
                    self.prepared_to_quit = true;
                }
                self.agent_runtime
                    .prepare_quit(&mut self.storage, &mut self.projects);
                let blockers = self
                    .g5
                    .prepare_quit(&mut self.storage, &mut self.projects)
                    .map_err(|failure| SidecarError::new(failure.kind, failure.detail))?;
                #[cfg(target_os = "macos")]
                let login_blockers = {
                    let mut blockers = Vec::new();
                    if !self.login.prepare_quit() {
                        blockers.push("provider-login:codex".to_owned());
                    }
                    if !self.claude_login.prepare_quit() {
                        blockers.push("provider-login:claude".to_owned());
                    }
                    if !self.install.prepare_quit(&self.storage) {
                        blockers.push("provider-install".to_owned());
                    }
                    blockers
                };
                #[cfg(not(target_os = "macos"))]
                let login_blockers: Vec<String> = Vec::new();
                let login_stopped = login_blockers.is_empty();
                let runtime_blockers = self.agent_runtime.owned_run_ids();
                if blockers.is_empty()
                    && login_stopped
                    && runtime_blockers.is_empty()
                    && self.pending_agent_cancels.is_empty()
                    && self.pending_agent_capabilities.is_empty()
                    && self.pending_agent_starts.is_empty()
                    && !self.safe_to_quit
                {
                    self.storage.checkpoint()?;
                    self.storage
                        .mark_app_instance_clean(&self.ready.sidecar_instance_id)?;
                    self.safe_to_quit = true;
                }
                let mut blockers = self.storage.blocker_ids()?;
                blockers.extend(login_blockers);
                for run_id in runtime_blockers {
                    if !blockers.contains(&run_id) {
                        blockers.push(run_id);
                    }
                }
                for run_id in self.pending_agent_cancels.keys() {
                    if !blockers.contains(run_id) {
                        blockers.push(run_id.clone());
                    }
                }
                blockers.extend(self.pending_agent_starts.keys().map(|id| format!("agent-start:{id}")));
                blockers.extend(self.pending_agent_capabilities.keys().map(|id| format!("agent-discovery:{id}")));
                project::ProjectAction {
                    result: serde_json::to_value(PrepareQuitResultV1 {
                        safe_to_quit: self.safe_to_quit && blockers.is_empty(),
                        blockers,
                    })
                    .map_err(json_error)?,
                    ..project::ProjectAction::default()
                }
            }),
            "project.catalog" => match request.params.get("action").and_then(Value::as_str) {
                Some("cancelApplication") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "applicationId"]).map_err(invalid_params)?;
                    self.projects.cancel_application(param_string(params, "projectId").map_err(invalid_params)?, param_string(params, "applicationId").map_err(invalid_params)?)
                }
                Some("cancelApplicationConfirmation") => {
                    let params = require_exact_params(&request.params, &["action", "confirmationId"]).map_err(invalid_params)?;
                    self.projects.cancel_application_confirmation(param_string(params, "confirmationId").map_err(invalid_params)?)
                }
                Some("openApplicationConfirmation") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "chatId", "fingerprint"]).map_err(invalid_params)?;
                    self.projects.open_application_confirmation(&mut self.storage, param_string(params, "projectId").map_err(invalid_params)?, param_string(params, "chatId").map_err(invalid_params)?, param_string(params, "fingerprint").map_err(invalid_params)?, &request.request_id)
                }
                Some("inspectChatDiscard" | "discardChatWorkspace") => {
                    if !self.agent_runtime.owned_run_ids().is_empty()
                        || !self.pending_agent_starts.is_empty()
                        || !self.pending_agent_cancels.is_empty()
                    {
                        return Ok(HandledRequest::response(error_response(&request, "requestBlocked", "requestBlocked", "workspace discard requires idle native execution")));
                    }
                    let committing = request.params["action"] == "discardChatWorkspace";
                    let fields = ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"];
                    let params = if committing {
                        let outer = require_exact_params(&request.params, &["action", "inspection", "confirmed"]).map_err(invalid_params)?;
                        if outer["confirmed"] != true { return Err(invalid_params("workspace discard requires explicit confirmation".into())); }
                        require_exact_params(&outer["inspection"], &["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "chatRevision"]).map_err(invalid_params)?
                    } else {
                        require_exact_params(&request.params, &["action", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"]).map_err(invalid_params)?
                    };
                    for field in fields { param_string(params, field).map_err(invalid_params)?; }
                    let address = project::WorkspaceAddress {
                        chat_id: param_string(params, "chatId").map_err(invalid_params)?.to_owned(),
                        project_id: param_string(params, "projectId").map_err(invalid_params)?.to_owned(),
                        worktree_id: param_string(params, "worktreeId").map_err(invalid_params)?.to_owned(),
                        workspace_identity: param_string(params, "workspaceIdentity").map_err(invalid_params)?.to_owned(),
                    };
                    let task_id = param_string(params, "taskId").map_err(invalid_params)?;
                    if committing {
                        let revision = params["chatRevision"].as_u64().filter(|value| *value > 0 && *value <= 9_007_199_254_740_991).ok_or_else(|| invalid_params("invalid chat revision".into()))?;
                        self.g5.discard_workspace(&mut self.storage, &mut self.projects, &address, task_id, revision, &request.request_id)
                            .map(|chat| project::ProjectAction { result: serde_json::json!({"schemaVersion":1,"action":"discardChatWorkspace","chat":chat}), ..Default::default() })
                    } else {
                        self.g5.preview_workspace_discard(&self.storage, &self.projects, &address, task_id)
                            .map(|inspection| project::ProjectAction { result: serde_json::json!({"schemaVersion":1,"action":"inspectChatDiscard","inspection":inspection}), ..Default::default() })
                    }
                }
                Some("applicationCapabilities") => {
                    require_exact_params(&request.params, &["action"]).map_err(invalid_params)?;
                    Ok(self.projects.application_capabilities())
                }
                Some(action @ ("inspectGitTrust" | "confirmGitTrust" | "revokeGitTrust")) => {
                    if action == "confirmGitTrust" {
                        let params = require_exact_params(&request.params, &["action", "identity", "confirmed"]).map_err(invalid_params)?;
                        require_exact_params(&params["identity"], &["installationPath", "volumePath", "directoryId", "commonName", "organization"]).map_err(invalid_params)?;
                        if params["confirmed"] != true { return Err(invalid_params("Git trust requires explicit confirmation".into())); }
                    } else {
                        require_exact_params(&request.params, &["action"]).map_err(invalid_params)?;
                    }
                    #[cfg(windows)]
                    {
                        let result = match action {
                            "inspectGitTrust" => self.projects.preview_git_trust().map(|inspection| serde_json::json!({"schemaVersion":1,"action":action,"inspection":inspection})),
                            "confirmGitTrust" => {
                                let identity = serde_json::from_value(request.params["identity"].clone()).map_err(|_| invalid_params("invalid Git trust identity".into()))?;
                                self.projects.confirm_git_trust(identity, true).map(|inspection| serde_json::json!({"schemaVersion":1,"action":action,"inspection":inspection}))
                            },
                            _ => self.projects.revoke_git_trust().map(|()| serde_json::json!({"schemaVersion":1,"action":action,"revoked":true})),
                        };
                        result.map(|result| project::ProjectAction { result, ..Default::default() })
                    }
                    #[cfg(not(windows))]
                    { return Ok(HandledRequest::response(error_response(&request, "methodUnavailable", "unsupportedCapability", "Windows Git trust is not available on this platform"))); }
                }
                Some("getAppRoleDefaults") => {
                    require_exact_params(&request.params, &["action"]).map_err(invalid_params)?;
                    self.projects.app_role_defaults().map(|settings| project::ProjectAction { result: serde_json::json!({"schemaVersion":1,"action":"getAppRoleDefaults","settings":settings}), ..Default::default() })
                }
                Some("updateAppRoleDefaults") => {
                    let params = require_exact_params(&request.params, &["action", "expectedRevision", "roles"]).map_err(invalid_params)?;
                    let revision = params.get("expectedRevision").and_then(Value::as_u64).ok_or_else(|| invalid_params("invalid settings revision".into()))?;
                    self.projects.update_app_role_defaults(revision, params["roles"].clone()).map(|settings| project::ProjectAction { result: serde_json::json!({"schemaVersion":1,"action":"updateAppRoleDefaults","settings":settings}), ..Default::default() })
                }
                Some("backupStorageUsage") => {
                    require_exact_params(&request.params, &["action"]).map_err(invalid_params)?;
                    self.projects.backup_storage_usage()
                }
                Some("setBackupStorageLimit") => {
                    let params = require_exact_params(&request.params, &["action", "expectedLimitBytes", "limitBytes"]).map_err(invalid_params)?;
                    let expected = params.get("expectedLimitBytes").and_then(Value::as_u64).ok_or_else(|| invalid_params("invalid expected budget".to_string()))?;
                    let limit = params.get("limitBytes").and_then(Value::as_u64).ok_or_else(|| invalid_params("invalid budget".to_string()))?;
                    self.projects.set_backup_storage_limit(expected, limit)
                }
                Some("backupPreparations") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"]).map_err(invalid_params)?;
                    self.projects.backup_preparations(&self.storage, param_string(params, "projectId").map_err(invalid_params)?)
                }
                Some("backupRetention") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"]).map_err(invalid_params)?;
                    self.projects.backup_retention(&self.storage, param_string(params, "projectId").map_err(invalid_params)?)
                }
                Some("inspectBackupCleanup") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "reviewId"]).map_err(invalid_params)?;
                    self.projects.inspect_backup_cleanup(&self.storage, param_string(params,"projectId").map_err(invalid_params)?, param_string(params,"reviewId").map_err(invalid_params)?)
                }
                Some("inspectApplicationRetirement") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "applicationId"]).map_err(invalid_params)?;
                    self.projects.inspect_application_retirement(&self.storage, param_string(params,"projectId").map_err(invalid_params)?, param_string(params,"applicationId").map_err(invalid_params)?)
                }
                Some("retireApplication") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "applicationId", "fingerprint"]).map_err(invalid_params)?;
                    self.projects.retire_application(&mut self.storage, param_string(params,"projectId").map_err(invalid_params)?, param_string(params,"applicationId").map_err(invalid_params)?, param_string(params,"fingerprint").map_err(invalid_params)?, &request.request_id)
                }
                Some("cleanupBackup") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "reviewId", "fingerprint"]).map_err(invalid_params)?;
                    self.projects.cleanup_backup(&mut self.storage, param_string(params,"projectId").map_err(invalid_params)?, param_string(params,"reviewId").map_err(invalid_params)?, param_string(params,"fingerprint").map_err(invalid_params)?, &request.request_id)
                }
                Some("abandonBackupPreparation") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "reviewId", "fingerprint"]).map_err(invalid_params)?;
                    self.projects.abandon_backup_preparation(&mut self.storage, param_string(params, "projectId").map_err(invalid_params)?, param_string(params, "reviewId").map_err(invalid_params)?, param_string(params, "fingerprint").map_err(invalid_params)?, &request.request_id)
                }
                Some("startApplication") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "chatId", "confirmationId"]).map_err(invalid_params)?;
                    self.projects.start_application_request(&mut self.storage, param_string(params, "projectId").map_err(invalid_params)?, param_string(params, "chatId").map_err(invalid_params)?, param_string(params, "confirmationId").map_err(invalid_params)?, &request.request_id)
                }
                Some("restoreApplication") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "applicationId", "fingerprint"]).map_err(invalid_params)?;
                    self.projects.restore_application_request(&self.storage, param_string(params, "projectId").map_err(invalid_params)?, param_string(params, "applicationId").map_err(invalid_params)?, param_string(params, "fingerprint").map_err(invalid_params)?, &request.request_id)
                }
                Some("applicationPreviewPage") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "chatId", "offset", "fingerprint"]).map_err(invalid_params)?;
                    let chat_id = if params["chatId"].is_null() { None } else { Some(param_string(params, "chatId").map_err(invalid_params)?) };
                    let fingerprint = if params["fingerprint"].is_null() { None } else { Some(param_string(params, "fingerprint").map_err(invalid_params)?) };
                    let offset = params["offset"].as_u64().filter(|v| *v <= 100_000).ok_or_else(|| invalid_params("invalid review offset".into()))? as usize;
                    self.projects.application_preview_page(&self.storage, param_string(params, "projectId").map_err(invalid_params)?, chat_id, offset, fingerprint)
                }
                Some("restorationPreview") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "applicationId", "offset", "fingerprint"]).map_err(invalid_params)?;
                    let fingerprint = if params["fingerprint"].is_null() { None } else { Some(param_string(params, "fingerprint").map_err(invalid_params)?) };
                    let offset = params["offset"].as_u64().filter(|v| *v <= 100_000).ok_or_else(|| invalid_params("invalid restoration offset".into()))? as usize;
                    self.projects.restoration_preview(&self.storage, param_string(params, "projectId").map_err(invalid_params)?, param_string(params, "applicationId").map_err(invalid_params)?, offset, fingerprint)
                }
                Some("chatApplicationPreview") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "chatId"]).map_err(invalid_params)?;
                    self.projects.chat_application_preview(&self.storage, param_string(params, "projectId").map_err(invalid_params)?, param_string(params, "chatId").map_err(invalid_params)?)
                }
                Some("prepareApplicationWorkspace") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"]).map_err(invalid_params)?;
                    self.projects.prepare_application_workspace(&mut self.storage, param_string(params, "projectId").map_err(invalid_params)?, &request.request_id)
                }
                Some("applicationPreview") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"]).map_err(invalid_params)?;
                    self.projects.application_preview(&self.storage, param_string(params, "projectId").map_err(invalid_params)?)
                }
                Some("applicationHistory") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"]).map_err(invalid_params)?;
                    self.projects.application_history(&self.storage, param_string(params, "projectId").map_err(invalid_params)?)
                }
                Some("applicationStatus") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"]).map_err(invalid_params)?;
                    self.projects.application_status(&self.storage,param_string(params,"projectId").map_err(invalid_params)?)
                }
                Some("chatApplicationStatus") => {
                    let params = require_exact_params(&request.params, &["action", "projectId", "chatId"]).map_err(invalid_params)?;
                    self.projects.chat_application_status(&self.storage, param_string(params,"projectId").map_err(invalid_params)?, param_string(params,"chatId").map_err(invalid_params)?)
                }
                Some("inspectPreparation") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"]).map_err(invalid_params)?;
                    self.projects.inspect_unstarted_snapshot(&self.storage, param_string(params, "projectId").map_err(invalid_params)?)
                        .map(|inspection| project::ProjectAction { result: serde_json::json!({"schemaVersion":1,"action":"inspectPreparation","inspection":inspection}), ..Default::default() })
                }
                Some("abandonPreparation") => {
                    let params = require_exact_params(&request.params, &["action", "inspection"]).map_err(invalid_params)?;
                    let inspection = params.get("inspection").ok_or_else(|| invalid_params("inspection missing".into()))?;
                    self.projects.abandon_unstarted_snapshot(&mut self.storage, inspection, &request.request_id)
                        .map(|()| project::ProjectAction { result: serde_json::json!({"schemaVersion":1,"action":"abandonPreparation","inspection":inspection}), ..Default::default() })
                }
                Some("releaseView") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"])
                        .map_err(invalid_params)?;
                    self.projects.release_project_view(
                        &self.storage,
                        param_string(params, "projectId").map_err(invalid_params)?,
                    )
                }
                Some("list") => {
                    let params = require_exact_params(&request.params, &["action", "cursor"])
                        .map_err(invalid_params)?;
                    self.projects.list_saved_projects(
                        &self.storage,
                        param_nullable_string(params, "cursor").map_err(invalid_params)?,
                    )
                }
                Some("reopen") => {
                    let params = require_exact_params(&request.params, &["action", "projectId"])
                        .map_err(invalid_params)?;
                    self.projects.reopen_project(
                            &mut self.storage,
                            param_string(params, "projectId").map_err(invalid_params)?,
                            &request.request_id,
                        ).map(|mut action| {
                            action.result = serde_json::json!({ "schemaVersion": 1, "action": "reopen", "project": action.result });
                            action
                        })
                }
                _ => return Err(invalid_params("invalid project catalog action".to_owned())),
            },
            "project.open" => {
                let params = require_exact_params(&request.params, &["selectedPath"])
                    .map_err(invalid_params)?;
                self.projects.open_project(
                    &mut self.storage,
                    param_string(params, "selectedPath").map_err(invalid_params)?,
                    &request.request_id,
                )
            }
            "project.getTree" => {
                let params =
                    require_exact_params(&request.params, &["projectId", "parentPath", "cursor"])
                        .map_err(invalid_params)?;
                self.projects.get_tree(
                    &self.storage,
                    param_string(params, "projectId").map_err(invalid_params)?,
                    param_string_allow_empty(params, "parentPath").map_err(invalid_params)?,
                    param_nullable_string(params, "cursor").map_err(invalid_params)?,
                )
            }
            "project.createWorktree" => {
                let params = require_exact_params(&request.params, &["projectId"])
                    .map_err(invalid_params)?;
                self.projects.create_worktree(
                    &mut self.storage,
                    param_string(params, "projectId").map_err(invalid_params)?,
                    &request.request_id,
                )
            }
            "project.close" => {
                let params = require_exact_params(&request.params, &["projectId"])
                    .map_err(invalid_params)?;
                self.projects.close_project(
                    &mut self.storage,
                    param_string(params, "projectId").map_err(invalid_params)?,
                    &request.request_id,
                )
            }
            "file.read" => {
                let params = require_exact_params(&request.params, &["projectId", "path"])
                    .map_err(invalid_params)?;
                self.projects.read_file(
                    &self.storage,
                    param_string(params, "projectId").map_err(invalid_params)?,
                    param_string(params, "path").map_err(invalid_params)?,
                )
            }
            "file.save" => self.handle_file_save(&request)?,
            "file.watch" => {
                let params = require_exact_params(&request.params, &["projectId"])
                    .map_err(invalid_params)?;
                self.projects.watch_project(
                    &self.storage,
                    param_string(params, "projectId").map_err(invalid_params)?,
                )
            }
            "diff.getSummary" => {
                let params = require_exact_params(&request.params, &["projectId"])
                    .map_err(invalid_params)?;
                self.projects.diff_summary(
                    &self.storage,
                    param_string(params, "projectId").map_err(invalid_params)?,
                )
            }
            "diff.getFile" => {
                let params = require_exact_params(&request.params, &["projectId", "path"])
                    .map_err(invalid_params)?;
                self.projects.diff_file(
                    &self.storage,
                    param_string(params, "projectId").map_err(invalid_params)?,
                    param_string(params, "path").map_err(invalid_params)?,
                )
            }
            "workspace.getTree" => {
                let params = require_exact_params(
                    &request.params,
                    &[
                        "chatId",
                        "projectId",
                        "worktreeId",
                        "workspaceIdentity",
                        "parentPath",
                        "cursor",
                    ],
                )
                .map_err(invalid_params)?;
                let address = workspace_address(params).map_err(invalid_params)?;
                self.projects
                    .workspace_get_tree(
                        &self.storage,
                        &address,
                        param_string_allow_empty(params, "parentPath").map_err(invalid_params)?,
                        param_nullable_string(params, "cursor").map_err(invalid_params)?,
                    )
                    .map(|action| addressed_action(action, &address))
            }
            "workspace.close" => {
                let params = require_exact_params(
                    &request.params,
                    &["chatId", "projectId", "worktreeId", "workspaceIdentity"],
                )
                .map_err(invalid_params)?;
                let address = workspace_address(params).map_err(invalid_params)?;
                self.projects
                    .workspace_close(&self.storage, &address)
                    .map(|action| addressed_action(action, &address))
            }
            "workspace.file.read" => {
                let params = require_exact_params(
                    &request.params,
                    &[
                        "chatId",
                        "projectId",
                        "worktreeId",
                        "workspaceIdentity",
                        "path",
                    ],
                )
                .map_err(invalid_params)?;
                let address = workspace_address(params).map_err(invalid_params)?;
                self.projects
                    .workspace_read_file(
                        &self.storage,
                        &address,
                        param_string(params, "path").map_err(invalid_params)?,
                    )
                    .map(|action| addressed_action(action, &address))
            }
            "workspace.resolveSaveRecovery" => {
                let params = require_exact_params(&request.params,
                    &["chatId", "projectId", "worktreeId", "workspaceIdentity", "saveId", "expectedLockRevision", "decision", "confirmed"])
                    .map_err(invalid_params)?;
                let address = workspace_address(params).map_err(invalid_params)?;
                let save_id = param_string(params, "saveId").map_err(invalid_params)?;
                let revision = param_u64(params, "expectedLockRevision").map_err(invalid_params)?;
                let decision = param_string(params, "decision").map_err(invalid_params)?;
                if params.get("confirmed") != Some(&Value::Bool(true)) || revision == 0
                    || revision > hello_agent_protocol::MAX_SEQUENCE || !matches!(decision, "complete" | "abort") {
                    return Err(invalid_params("explicit save recovery confirmation is invalid".into()));
                }
                if !cfg!(windows) || !project::WINDOWS_SAVE_RECOVERY_MUTATIONS_ENABLED {
                    return Ok(HandledRequest::response(error_response(&request, "methodUnavailable", "unsupportedCapability",
                        "Windows save recovery mutations have not been activated")));
                }
                #[cfg(windows)]
                {
                    let result = if decision == "abort" {
                        self.projects.abort_windows_save(&mut self.storage, &address, save_id, revision, &request.request_id)
                    } else {
                        self.projects.resume_windows_save_cleanup(&mut self.storage, &address, save_id, revision, &request.request_id)
                    };
                    result.map(|action| addressed_action(action, &address))
                }
                #[cfg(not(windows))]
                { let _ = (address, save_id); unreachable!("non-Windows recovery rejected above") }
            }
            "workspace.saveRecovery" => {
                let params = require_exact_params(&request.params,
                    &["chatId", "projectId", "worktreeId", "workspaceIdentity"])
                    .map_err(invalid_params)?;
                let address = workspace_address(params).map_err(invalid_params)?;
                #[cfg(windows)]
                { self.projects.describe_windows_save_recovery(&self.storage, &address)
                    .map(|action| addressed_action(action, &address)) }
                #[cfg(not(windows))]
                { let _ = address; return Ok(HandledRequest::response(error_response(
                    &request, "methodUnavailable", "unsupportedCapability",
                    "Windows save recovery is not available on this platform"))); }
            }
            "workspace.file.save" => self.handle_workspace_file_save(&request)?,
            "workspace.file.watch" => {
                let params = require_exact_params(
                    &request.params,
                    &["chatId", "projectId", "worktreeId", "workspaceIdentity"],
                )
                .map_err(invalid_params)?;
                let address = workspace_address(params).map_err(invalid_params)?;
                self.projects
                    .workspace_watch(&self.storage, &address)
                    .map(|action| addressed_action(action, &address))
            }
            "workspace.diff.getSummary" => {
                let params = require_exact_params(
                    &request.params,
                    &["chatId", "projectId", "worktreeId", "workspaceIdentity"],
                )
                .map_err(invalid_params)?;
                let address = workspace_address(params).map_err(invalid_params)?;
                self.projects
                    .workspace_diff_summary(&self.storage, &address)
                    .map(|action| addressed_action(action, &address))
            }
            "workspace.diff.getFile" => {
                let params = require_exact_params(
                    &request.params,
                    &[
                        "chatId",
                        "projectId",
                        "worktreeId",
                        "workspaceIdentity",
                        "path",
                    ],
                )
                .map_err(invalid_params)?;
                let address = workspace_address(params).map_err(invalid_params)?;
                self.projects
                    .workspace_diff_file(
                        &self.storage,
                        &address,
                        param_string(params, "path").map_err(invalid_params)?,
                    )
                    .map(|action| addressed_action(action, &address))
            }
            _ => {
                return Ok(HandledRequest::response(error_response(
                    &request,
                    "methodUnavailable",
                    "unsupportedCapability",
                    "method is not exposed by the G4 sidecar",
                )))
            }
        };

        let action = match action {
            Ok(action) => action,
            Err(failure) => {
                return Ok(HandledRequest::response(project_error_response(
                    &request, &failure,
                )))
            }
        };
        // The stdio loop advances watchers after responses (and on its 100ms
        // idle tick). Do not repeat a scan/identity check before every project
        // response, which delays user requests while a large scan is pending.
        let events = self.action_events(&action);
        Ok(HandledRequest {
            response: Some(ResponseV1 {
                protocol_version: PROTOCOL_VERSION,
                kind: "response".to_string(),
                sidecar_instance_id: request.sidecar_instance_id,
                request_id: request.request_id,
                ok: true,
                result: Some(action.result),
                error: None,
            }),
            events,
        })
    }

    fn handle_file_save(
        &mut self,
        request: &RequestV1,
    ) -> Result<Result<project::ProjectAction, ProjectFailure>, SidecarError> {
        let object = request
            .params
            .as_object()
            .ok_or_else(|| invalid_params("file.save params must be an object".to_string()))?;
        let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
        if keys
            == BTreeSet::from([
                "projectId",
                "path",
                "expectedVersion",
                "totalBytes",
                "encoding",
            ])
        {
            return Ok(self.projects.begin_save(
                &self.storage,
                param_string(object, "projectId").map_err(invalid_params)?,
                param_string(object, "path").map_err(invalid_params)?,
                param_string(object, "expectedVersion").map_err(invalid_params)?,
                param_u64(object, "totalBytes").map_err(invalid_params)?,
                param_string(object, "encoding").map_err(invalid_params)?,
            ));
        }
        if keys == BTreeSet::from(["saveId", "seq", "text"]) {
            return Ok(self.projects.append_save_chunk(
                param_string(object, "saveId").map_err(invalid_params)?,
                param_u64(object, "seq").map_err(invalid_params)?,
                param_string(object, "text").map_err(invalid_params)?,
            ));
        }
        if keys == BTreeSet::from(["saveId", "expectedVersion", "lastSeq", "totalBytes"]) {
            return Ok(self.projects.commit_save(
                &mut self.storage,
                param_string(object, "saveId").map_err(invalid_params)?,
                param_string(object, "expectedVersion").map_err(invalid_params)?,
                param_u64(object, "lastSeq").map_err(invalid_params)?,
                param_u64(object, "totalBytes").map_err(invalid_params)?,
                &request.request_id,
            ));
        }
        Err(invalid_params(
            "file.save params do not match begin, chunk, or commit".to_string(),
        ))
    }

    fn handle_workspace_file_save(
        &mut self,
        request: &RequestV1,
    ) -> Result<Result<project::ProjectAction, ProjectFailure>, SidecarError> {
        let object = request.params.as_object().ok_or_else(|| {
            invalid_params("workspace.file.save params must be an object".to_string())
        })?;
        let address = workspace_address(object).map_err(invalid_params)?;
        let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
        let reference = ["chatId", "projectId", "worktreeId", "workspaceIdentity"];
        if keys
            == reference
                .into_iter()
                .chain(["path", "expectedVersion", "totalBytes", "encoding"])
                .collect()
        {
            return Ok(self
                .projects
                .workspace_begin_save(
                    &mut self.storage,
                    &address,
                    param_string(object, "path").map_err(invalid_params)?,
                    param_string(object, "expectedVersion").map_err(invalid_params)?,
                    param_u64(object, "totalBytes").map_err(invalid_params)?,
                    param_string(object, "encoding").map_err(invalid_params)?,
                )
                .map(|action| addressed_action(action, &address)));
        }
        if keys
            == reference
                .into_iter()
                .chain(["saveId", "seq", "text"])
                .collect()
        {
            return Ok(self
                .projects
                .workspace_append_save_chunk(
                    &self.storage,
                    &address,
                    param_string(object, "saveId").map_err(invalid_params)?,
                    param_u64(object, "seq").map_err(invalid_params)?,
                    param_string(object, "text").map_err(invalid_params)?,
                )
                .map(|action| addressed_action(action, &address)));
        }
        if keys
            == reference
                .into_iter()
                .chain(["saveId", "expectedVersion", "lastSeq", "totalBytes"])
                .collect()
        {
            return Ok(self
                .projects
                .workspace_commit_save(
                    &mut self.storage,
                    &address,
                    param_string(object, "saveId").map_err(invalid_params)?,
                    param_string(object, "expectedVersion").map_err(invalid_params)?,
                    param_u64(object, "lastSeq").map_err(invalid_params)?,
                    param_u64(object, "totalBytes").map_err(invalid_params)?,
                    &request.request_id,
                )
                .map(|action| addressed_action(action, &address)));
        }
        Err(invalid_params(
            "workspace.file.save params do not match begin, chunk, or commit".to_string(),
        ))
    }

    fn handle_g5_request(&mut self, request: RequestV1) -> Result<HandledRequest, SidecarError> {
        let result = (|| -> Result<chat::G5Action, ProjectFailure> {
            let action = match request.method.as_str() {
                "project.getRoleSettings" => {
                    let params = require_exact_params(&request.params, &["projectId"])
                        .map_err(ProjectFailure::manifest)?;
                    chat::G5Action {
                        result: self.g5.default_role_settings(
                            &mut self.storage,
                            &self.projects,
                            param_string(params, "projectId").map_err(ProjectFailure::manifest)?,
                            &request.request_id,
                        )?,
                        ..chat::G5Action::default()
                    }
                }
                "project.updateRoleSettings" => {
                    let params = require_exact_params(
                        &request.params,
                        &["projectId", "expectedRevision", "roles"],
                    )
                    .map_err(ProjectFailure::manifest)?;
                    chat::G5Action {
                        result: self.g5.update_role_settings(
                            &mut self.storage,
                            param_string(params, "projectId").map_err(ProjectFailure::manifest)?,
                            param_u64(params, "expectedRevision")
                                .map_err(ProjectFailure::manifest)?,
                            params.get("roles").cloned().unwrap_or(Value::Null),
                            &request.request_id,
                        )?,
                        ..chat::G5Action::default()
                    }
                }
                "workspace.prepareChat" => {
                    let keys: &[&str] = if request.params.get("applicationBaseline").is_some() { &["projectId", "applicationBaseline"] } else { &["projectId"] };
                    let params = require_exact_params(&request.params, keys)
                        .map_err(ProjectFailure::manifest)?;
                    let baseline = match params.get("applicationBaseline") {
                        Some(value) => value.as_bool().ok_or_else(|| ProjectFailure::manifest("applicationBaseline must be boolean"))?,
                        None => false,
                    };
                    self.g5.prepare_chat_workspace_with_baseline(
                        &mut self.storage,
                        &mut self.projects,
                        param_string(params, "projectId").map_err(ProjectFailure::manifest)?,
                        &request.request_id,
                        baseline,
                    )?
                }
                "workspace.abandonPrepared" => {
                    let params = require_exact_params(
                        &request.params,
                        &[
                            "preparationId",
                            "projectId",
                            "worktreeId",
                            "workspaceIdentity",
                        ],
                    )
                    .map_err(ProjectFailure::manifest)?;
                    self.g5.abandon_prepared_workspace(
                        &mut self.storage,
                        &mut self.projects,
                        param_string(params, "preparationId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "projectId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "worktreeId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "workspaceIdentity")
                            .map_err(ProjectFailure::manifest)?,
                        &request.request_id,
                    )?
                }
                "chat.create" => {
                    let params = require_exact_params(
                        &request.params,
                        &[
                            "preparationId",
                            "projectId",
                            "worktreeId",
                            "workspaceIdentity",
                            "goal",
                            "expectedSettingsRevision",
                        ],
                    )
                    .map_err(ProjectFailure::manifest)?;
                    self.g5.create_chat(
                        &mut self.storage,
                        &self.projects,
                        param_string(params, "preparationId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "projectId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "worktreeId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "workspaceIdentity")
                            .map_err(ProjectFailure::manifest)?,
                        param_string(params, "goal").map_err(ProjectFailure::manifest)?,
                        param_u64(params, "expectedSettingsRevision")
                            .map_err(ProjectFailure::manifest)?,
                        &request.request_id,
                    )?
                }
                "chat.appendMessage" => self.g5.append_user_message(
                    &mut self.storage,
                    &self.projects,
                    &request.params,
                )?,
                "chat.list" => {
                    let params =
                        require_exact_params(&request.params, &["projectId", "cursor", "limit"])
                            .map_err(ProjectFailure::manifest)?;
                    chat::G5Action {
                        result: self.g5.list_chats(
                            &self.storage,
                            param_string(params, "projectId").map_err(ProjectFailure::manifest)?,
                            param_nullable_string(params, "cursor")
                                .map_err(ProjectFailure::manifest)?,
                            param_u64(params, "limit").map_err(ProjectFailure::manifest)?,
                        )?,
                        ..chat::G5Action::default()
                    }
                }
                "chat.get" => {
                    let params = require_exact_params(
                        &request.params,
                        &["chatId", "afterEventSequence", "cursor", "limit"],
                    )
                    .map_err(ProjectFailure::manifest)?;
                    chat::G5Action {
                        result: self.g5.get_chat(
                            &self.storage,
                            param_string(params, "chatId").map_err(ProjectFailure::manifest)?,
                            param_u64(params, "afterEventSequence")
                                .map_err(ProjectFailure::manifest)?,
                            param_nullable_string(params, "cursor")
                                .map_err(ProjectFailure::manifest)?,
                            param_u64(params, "limit").map_err(ProjectFailure::manifest)?,
                            self.agent_runtime.advertises_g5a(),
                        )?,
                        ..chat::G5Action::default()
                    }
                }
                "run.startFixture" => {
                    let params =
                        require_exact_params(&request.params, &["chatId", "roleId", "scenario"])
                            .map_err(ProjectFailure::manifest)?;
                    self.g5.start_fixture(
                        &mut self.storage,
                        &mut self.projects,
                        param_string(params, "chatId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "roleId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "scenario").map_err(ProjectFailure::manifest)?,
                        &request.request_id,
                    )?
                }
                "run.cancel" => {
                    let params = require_exact_params(&request.params, &["chatId", "runId"])
                        .map_err(ProjectFailure::manifest)?;
                    self.g5.cancel_run(
                        &mut self.storage,
                        &mut self.projects,
                        param_string(params, "chatId").map_err(ProjectFailure::manifest)?,
                        param_string(params, "runId").map_err(ProjectFailure::manifest)?,
                    )?
                }
                "recovery.list" => {
                    let params = require_exact_params(&request.params, &["cursor", "limit"])
                        .map_err(ProjectFailure::manifest)?;
                    chat::G5Action {
                        result: self.g5.list_recoveries(
                            &self.storage,
                            param_nullable_string(params, "cursor")
                                .map_err(ProjectFailure::manifest)?,
                            param_u64(params, "limit").map_err(ProjectFailure::manifest)?,
                        )?,
                        ..chat::G5Action::default()
                    }
                }
                "recovery.inspect" => {
                    let params = require_exact_params(&request.params, &["recoveryId"])
                        .map_err(ProjectFailure::manifest)?;
                    chat::G5Action {
                        result: self.g5.inspect_recovery(
                            &self.storage,
                            param_string(params, "recoveryId").map_err(ProjectFailure::manifest)?,
                        )?,
                        ..chat::G5Action::default()
                    }
                }
                "recovery.markResolved" => {
                    let params = require_exact_params(
                        &request.params,
                        &["recoveryId", "expectedRevision", "resolution"],
                    )
                    .map_err(ProjectFailure::manifest)?;
                    chat::G5Action {
                        result: self.g5.mark_recovery_resolved(
                            &mut self.storage,
                            &mut self.projects,
                            param_string(params, "recoveryId").map_err(ProjectFailure::manifest)?,
                            param_u64(params, "expectedRevision")
                                .map_err(ProjectFailure::manifest)?,
                            param_string(params, "resolution").map_err(ProjectFailure::manifest)?,
                            &request.request_id,
                        )?,
                        ..chat::G5Action::default()
                    }
                }
                _ => unreachable!("G5 dispatcher called for non-G5 method"),
            };
            Ok(action)
        })();
        let action = match result {
            Ok(action) => action,
            Err(failure) => {
                return Ok(HandledRequest::response(project_error_response(
                    &request, &failure,
                )))
            }
        };
        Ok(HandledRequest {
            response: Some(ResponseV1 {
                protocol_version: PROTOCOL_VERSION,
                kind: "response".to_string(),
                sidecar_instance_id: request.sidecar_instance_id,
                request_id: request.request_id,
                ok: true,
                result: Some(action.result),
                error: None,
            }),
            events: self.g5_events(action.events),
        })
    }

    #[cfg(target_os = "macos")]
    fn installation_conflicts(&mut self, request: &RequestV1) -> bool {
        if request.method == "provider.install"
            && matches!(request.params["action"].as_str(), Some("prepare" | "start" | "recover"))
        {
            return self.login.blocks_installation(&self.storage)
                || self.claude_login.blocks_installation(&self.storage)
                || !self.agent_runtime.owned_run_ids().is_empty()
                || !self.pending_agent_starts.is_empty()
                || !self.pending_agent_capabilities.is_empty()
                || !self.pending_agent_cancels.is_empty();
        }
        let launching = matches!(request.method.as_str(),
            "agent.getCapabilities" | "agent.getModelChoices" | "run.startAgent")
            || (request.method == "provider.login"
                && matches!(request.params["action"].as_str(), Some("diagnose" | "start" | "check" | "recover")));
        launching && self.install.blocks_provider_launch(&self.storage)
    }

    fn handle_agent_request(&mut self, request: RequestV1) -> Result<HandledRequest, SidecarError> {
        use hello_agent_protocol::agent_adapter_v1::AgentRunStartRequestV1;
        if request.method == "agent.getModelChoices" {
            if !self.agent_runtime.uses_native_providers() || request.params["runtime"] != "codex" {
                return Err(SidecarError::new("agentDiscoveryRejected", "model choices require a native Codex context"));
            }
            #[cfg(target_os = "macos")]
            return self.defer_agent_capabilities(request);
        }
        #[cfg(target_os = "macos")]
        if self.agent_runtime.uses_native_providers() {
            if request.method == "run.startAgent" {
                #[cfg(target_os = "macos")]
                return self.defer_agent_start(request);
            }
            if request.method == "agent.getCapabilities" {
                use hello_agent_protocol::agent_adapter_v1::{
                    validate_agent_adapter_payload_v1, AgentCapabilitiesRequestV1,
                };
                validate_agent_adapter_payload_v1("capabilitiesRequest", &request.params)
                    .map_err(|_| SidecarError::new("invalidParams", "invalid discovery request"))?;
                let input: AgentCapabilitiesRequestV1 = serde_json::from_value(request.params.clone())
                    .map_err(|_| SidecarError::new("invalidParams", "invalid discovery request"))?;
                if matches!(input.runtime.as_str(), "codex" | "claude-code") && input.chat_id.is_some() {
                    #[cfg(target_os = "macos")]
                    return self.defer_agent_capabilities(request);
                }
                // Unsupported/unscoped queries keep a blocked capability,
                // never a fixture-derived start permission.
            }
        }
        if request.method == "run.cancelAgent" {
            let queued = (|| -> Result<agent_runtime::RunIdentity, ProjectFailure> {
                let params = require_exact_params(
                    &request.params,
                    &[
                        "chatId",
                        "taskId",
                        "projectId",
                        "worktreeId",
                        "workspaceIdentity",
                        "runId",
                    ],
                )
                .map_err(ProjectFailure::manifest)?;
                let field = |key| {
                    param_string(params, key)
                        .map(str::to_owned)
                        .map_err(ProjectFailure::manifest)
                };
                let identity = agent_runtime::RunIdentity {
                    chat_id: field("chatId")?,
                    task_id: field("taskId")?,
                    project_id: field("projectId")?,
                    worktree_id: field("worktreeId")?,
                    workspace_identity: field("workspaceIdentity")?,
                    run_id: field("runId")?,
                };
                if self.pending_agent_cancels.contains_key(&identity.run_id) {
                    return Err(ProjectFailure::manifest(
                        "agent cancellation is already pending",
                    ));
                }
                self.agent_runtime.request_cancel(&identity)?;
                Ok(identity)
            })();
            return match queued {
                Ok(identity) => {
                    self.pending_agent_cancels
                        .insert(identity.run_id.clone(), (request, identity));
                    Ok(HandledRequest {
                        response: None,
                        events: Vec::new(),
                    })
                }
                Err(failure) => Ok(HandledRequest::response(project_error_response(
                    &request, &failure,
                ))),
            };
        }
        let result =
            (|| -> Result<chat::G5Action, ProjectFailure> {
                match request.method.as_str() {
                    "agent.getCapabilities" => {
                        hello_agent_protocol::agent_adapter_v1::validate_agent_adapter_payload_v1(
                            "capabilitiesRequest", &request.params,
                        ).map_err(|error| ProjectFailure::manifest(error.to_string()))?;
                        let input: hello_agent_protocol::agent_adapter_v1::AgentCapabilitiesRequestV1 =
                            serde_json::from_value(request.params.clone())
                                .map_err(|error| ProjectFailure::manifest(error.to_string()))?;
                        Ok(chat::G5Action {
                            result: self.agent_runtime.get_context_capabilities(
                                &self.storage, &self.projects, &input,
                            )?,
                            ..chat::G5Action::default()
                        })
                    }
                    "agent.outputHead" => Ok(chat::G5Action {
                        result: agent_runtime::read_output_head(&self.storage, &request.params)?,
                        ..chat::G5Action::default()
                    }),
                    "agent.outputPage" => Ok(chat::G5Action {
                        result: agent_runtime::read_output_page(&mut self.storage, &request.params)?,
                        ..chat::G5Action::default()
                    }),
                    "run.startAgent" => {
                        let parsed: AgentRunStartRequestV1 =
                            serde_json::from_value(request.params.clone()).map_err(|error| {
                                ProjectFailure::manifest(format!(
                                    "invalid agent start request: {error}"
                                ))
                            })?;
                        self.agent_runtime.start_agent(
                            &mut self.storage,
                            &mut self.projects,
                            parsed,
                            &request.request_id,
                        )
                    }
                    "recovery.inspectAgent" => Ok(chat::G5Action {
                        result: self
                            .agent_runtime
                            .inspect_recovery(&self.storage, &request.params)?,
                        ..chat::G5Action::default()
                    }),
                    "recovery.resolveAgent" => Ok(chat::G5Action {
                        result: self.agent_runtime.resolve_recovery(
                            &mut self.storage,
                            &mut self.projects,
                            &request.params,
                            &request.request_id,
                        )?,
                        ..chat::G5Action::default()
                    }),
                    _ => unreachable!("agent dispatcher called for non-agent method"),
                }
            })();
        let action = match result {
            Ok(action) => action,
            Err(failure) => {
                return Ok(HandledRequest::response(project_error_response(
                    &request, &failure,
                )))
            }
        };
        Ok(HandledRequest {
            response: Some(ResponseV1 {
                protocol_version: PROTOCOL_VERSION,
                kind: "response".into(),
                sidecar_instance_id: request.sidecar_instance_id,
                request_id: request.request_id,
                ok: true,
                result: Some(action.result),
                error: None,
            }),
            events: self.g5_events(action.events),
        })
    }

    fn action_events(&self, action: &project::ProjectAction) -> Vec<EventV1> {
        let mut events = Vec::new();
        let workspace = action.result.as_object().and_then(|result| {
            Some((
                result.get("chatId")?.as_str()?.to_string(),
                result.get("projectId")?.as_str()?.to_string(),
                result.get("worktreeId")?.as_str()?.to_string(),
                result.get("workspaceIdentity")?.as_str()?.to_string(),
            ))
        });
        for stream in &action.streams {
            for (index, text) in stream.chunks.iter().enumerate() {
                events.push(EventV1 {
                    protocol_version: PROTOCOL_VERSION,
                    kind: "event".to_string(),
                    sidecar_instance_id: self.ready.sidecar_instance_id.clone(),
                    stream_id: stream.stream_id.clone(),
                    seq: (index + 1) as u64,
                    event: if workspace.is_some() {
                        "workspace.blob.chunk".to_string()
                    } else {
                        "blob.chunk".to_string()
                    },
                    chat_id: None,
                    task_id: None,
                    run_id: None,
                    payload: match &workspace {
                        Some((chat_id, project_id, worktree_id, workspace_identity)) => {
                            serde_json::json!({
                                "chatId":chat_id,"projectId":project_id,"worktreeId":worktree_id,
                                "workspaceIdentity":workspace_identity,"text":text,
                            })
                        }
                        None => serde_json::json!({"text":text}),
                    },
                });
            }
        }
        for emission in &action.watch_events {
            events.push(EventV1 {
                protocol_version: PROTOCOL_VERSION,
                kind: "event".to_string(),
                sidecar_instance_id: self.ready.sidecar_instance_id.clone(),
                stream_id: emission.stream_id.clone(),
                seq: emission.seq,
                event: if emission.payload.get("chatId").is_some() {
                    "workspace.changed".to_string()
                } else {
                    emission.event.to_string()
                },
                chat_id: None,
                task_id: None,
                run_id: None,
                payload: emission.payload.clone(),
            });
        }
        events
    }

    fn poll_watch_events(&mut self) -> Vec<EventV1> {
        let action = project::ProjectAction {
            result: Value::Null,
            streams: Vec::new(),
            watch_events: self.projects.poll_watchers(&self.storage),
        };
        self.action_events(&action)
    }

    /// Native mode dispatches scoped requests here. Keep the request
    /// until its owned worker has completed, including failed-query cleanup.
    #[cfg(target_os = "macos")]
    fn defer_agent_capabilities(
        &mut self,
        request: RequestV1,
    ) -> Result<HandledRequest, SidecarError> {
        if !matches!(request.method.as_str(), "agent.getCapabilities" | "agent.getModelChoices")
            || self.pending_agent_starts.contains_key(&request.request_id)
            || self
                .pending_agent_capabilities
                .contains_key(&request.request_id)
        {
            return Err(SidecarError::new(
                "invalidParams",
                "duplicate or invalid discovery request",
            ));
        }
        let input: hello_agent_protocol::agent_adapter_v1::AgentCapabilitiesRequestV1 = serde_json::from_value(request.params.clone())
            .map_err(|_| SidecarError::new("invalidParams", "invalid scoped discovery request"))?;
        let discovery = if input.runtime == "claude-code" {
            self.agent_runtime.begin_claude_discovery(&self.storage, &self.projects, &request.request_id, &input)
        } else { self.agent_runtime.begin_scoped_discovery(
            &self.storage,
            &self.projects,
            &input,
            &request.request_id,
        ) };
        // Admission rejection belongs to this query, not the stdio service.
        // Keep cleanup ownership inside the discovery manager and do not queue
        // a request whose discovery did not start.
        if let Err(error) = discovery {
            return Ok(HandledRequest::response(error_response(
                &request, error.kind(), "requestBlocked", error.detail(),
            )));
        }
        self.pending_agent_capabilities
            .insert(request.request_id.clone(), request);
        Ok(HandledRequest {
            response: None,
            events: Vec::new(),
        })
    }

    #[cfg(target_os = "macos")]
    fn defer_agent_start(&mut self, request: RequestV1) -> Result<HandledRequest, SidecarError> {
        if request.method != "run.startAgent"
            || self.pending_agent_starts.contains_key(&request.request_id)
            || self.pending_agent_capabilities.contains_key(&request.request_id)
        {
            return Err(SidecarError::new("invalidParams", "duplicate or invalid start request"));
        }
        let input: hello_agent_protocol::agent_adapter_v1::AgentRunStartRequestV1 = serde_json::from_value(request.params.clone())
            .map_err(|_| SidecarError::new("invalidParams", "invalid start request"))?;
        let runtime = self.agent_runtime.begin_scoped_start(&self.storage, &self.projects, &input, &request.request_id)?;
        self.pending_agent_starts.insert(request.request_id.clone(), PendingAgentStart {
            runtime, request, input, reservation: None, process: None, result: None,
        });
        Ok(HandledRequest { response: None, events: Vec::new() })
    }

    fn poll_runtime_events(&mut self) -> Result<(Vec<ResponseV1>, Vec<EventV1>), SidecarError> {
        if let Some(error) = self.deferred_runtime_error.take() {
            return Err(error);
        }
        let mut events = self.g5.poll_runs(&mut self.storage, &mut self.projects);
        self.projects.poll_application(&mut self.storage);
        // A cleanup error in another owner must not starve completed requests.
        let runtime_error = match self.agent_runtime.poll(&mut self.storage, &mut self.projects) {
            Ok(next) => { events.extend(next); None },
            Err(error) => Some(error),
        };
        let mut responses = Vec::new();
        #[cfg(target_os = "macos")]
        {
        let ids: Vec<_> = self.pending_agent_starts.keys().cloned().collect();
        for id in ids {
            let mut pending = self.pending_agent_starts.remove(&id).unwrap();
            if pending.result.is_none() {
                if pending.runtime == "claude-code" {
                    let Some(observed) = self.agent_runtime.take_claude_discovery(&self.storage, &self.projects, &id) else {
                        self.pending_agent_starts.insert(id, pending); continue;
                    };
                    pending.result = Some(observed.and_then(|observed| {
                        self.agent_runtime.reserve_scoped_claude_start(&mut self.storage, &self.projects,
                            &pending.input, &id, &observed, &mut pending.reservation, &mut pending.process)?;
                        let invalid = || SidecarError::new("agentInputChanged", "Claude reservation unavailable");
                        let admission = Arc::new(pending.reservation.as_ref().ok_or_else(invalid)?.claude_admission(
                            &self.storage, &self.projects, pending.process.as_ref().ok_or_else(invalid)?)?);
                        let prompt_admission = admission.clone();
                        let tool_admission = admission.clone();
                        self.agent_runtime.start_reserved_claude(&mut self.storage, &self.projects,
                            &mut pending.reservation, &mut pending.process, &observed.evidence.executable,
                            &AtomicBool::new(self.prepared_to_quit),
                            &mut |workspace, home, model, effort, access| admission.launch(workspace, home, model, effort, access),
                            Box::new(move |process| prompt_admission.process(process)),
                            Box::new(move |process, request| tool_admission.tool(process, request)))
                    }));
                } else {
                let Some(observed) = self.agent_runtime.take_scoped_discovery(&self.storage, &self.projects, &id) else {
                    self.pending_agent_starts.insert(id, pending);
                    continue;
                };
                pending.result = Some(observed.and_then(|observed| {
                    self.agent_runtime.reserve_scoped_start(
                        &mut self.storage, &self.projects, &pending.input, &id,
                        &observed, &mut pending.reservation, &mut pending.process,
                    )?;
                    self.agent_runtime.start_reserved_codex(
                        &mut self.storage, &self.projects, &mut pending.reservation,
                        &mut pending.process, &observed.evidence.observation,
                        &observed.evidence.executable, &observed.evidence.helper,
                        &AtomicBool::new(self.prepared_to_quit),
                    )
                }));
                }
            }
            if self.agent_runtime.retain_rejected_start(&mut pending.reservation, &mut pending.process).is_err() {
                // Keep the original result and both owners; never rerun launch.
                self.pending_agent_starts.insert(id, pending);
                continue;
            }
            responses.push(match pending.result.take().unwrap() {
                Ok(action) => { events.extend(action.events); success_response(&pending.request, action.result) },
                Err(error) => error_response(&pending.request, error.kind(), "requestBlocked", error.detail()),
            });
        }
        let ids: Vec<_> = self.pending_agent_capabilities.keys().cloned().collect();
        for id in ids {
            if self.pending_agent_capabilities[&id].method == "agent.getModelChoices" {
                let Some(result) = self.agent_runtime.take_scoped_model_choices(&self.storage, &self.projects, &id) else { continue; };
                let request = self.pending_agent_capabilities.remove(&id).unwrap();
                responses.push(match result {
                    Ok((scope, choices)) => success_response(&request, serde_json::json!({
                        "schemaVersion":1, "runtime":"codex", "scope":scope,
                        "models":choices.into_iter().map(|choice| serde_json::json!({"modelSelector":choice.model,"reasoningEfforts":choice.reasoning_efforts})).collect::<Vec<_>>()
                    })),
                    Err(error) => error_response(&request, error.kind(), "requestBlocked", error.detail()),
                });
                continue;
            }
            let result = if self.pending_agent_capabilities[&id].params["runtime"] == "claude-code" {
                self.agent_runtime
                    .take_claude_discovery(&self.storage, &self.projects, &id)
                    .map(|result| result.and_then(|evidence| evidence.capability()))
            } else {
                self.agent_runtime
                    .take_scoped_discovery(&self.storage, &self.projects, &id)
                    .map(|result| result.and_then(|evidence| evidence.capability()))
            };
            let Some(result) = result else {
                continue;
            };
            let request = self.pending_agent_capabilities.remove(&id).unwrap();
            let result = result
                .and_then(|snapshot| {
                    serde_json::to_value(snapshot).map_err(|_| {
                        SidecarError::new("agentDiscoveryRejected", "invalid discovery response")
                    })
                });
            responses.push(match result {
                Ok(value) => success_response(&request, value),
                Err(error) => {
                    error_response(&request, error.kind(), "requestBlocked", error.detail())
                }
            });
        }
        }
        let ids: Vec<_> = self.pending_agent_cancels.keys().cloned().collect();
        for run_id in ids {
            let (request, identity) = &self.pending_agent_cancels[&run_id];
            let response = match self.agent_runtime.cancel_result(&self.storage, identity) {
                Ok(None) => continue,
                Ok(Some(result)) => success_response(request, result),
                Err(failure) => project_error_response(request, &failure),
            };
            self.pending_agent_cancels.remove(&run_id);
            responses.push(response);
        }
        self.finish_runtime_poll(responses, events, runtime_error)
    }

    fn finish_runtime_poll(&mut self, responses: Vec<ResponseV1>, events: Vec<chat::G5Event>, runtime_error: Option<SidecarError>) -> Result<(Vec<ResponseV1>, Vec<EventV1>), SidecarError> {
        if responses.is_empty() && events.is_empty() {
            if let Some(error) = runtime_error { return Err(error); }
        } else {
            self.deferred_runtime_error = runtime_error;
        }
        Ok((responses, self.g5_events(events)))
    }

    fn maintenance_tick(
        &mut self,
    ) -> Result<storage_lifetime::scheduler::MaintenanceTickResult, SidecarError> {
        if self.prepared_to_quit {
            return Ok(storage_lifetime::scheduler::MaintenanceTickResult::NoWork);
        }
        if self.maintenance_poisoned {
            return Err(maintenance_poisoned_error());
        }
        match self.maintenance_scheduler.tick(&mut self.storage) {
            Ok(result) => Ok(result),
            Err(error) => {
                self.maintenance_poisoned = true;
                Err(error)
            }
        }
    }

    fn g5_events(&self, events: Vec<chat::G5Event>) -> Vec<EventV1> {
        events
            .into_iter()
            .map(|event| EventV1 {
                protocol_version: PROTOCOL_VERSION,
                kind: "event".to_string(),
                sidecar_instance_id: self.ready.sidecar_instance_id.clone(),
                stream_id: event.stream_id,
                seq: event.seq,
                event: event.event.to_string(),
                chat_id: event.chat_id,
                task_id: event.task_id,
                run_id: event.run_id,
                payload: event.payload,
            })
            .collect()
    }
}

fn maintenance_poisoned_error() -> SidecarError {
    SidecarError::new(
        "storageMaintenancePoisoned",
        "storage maintenance is poisoned and durable mutation or agent execution is blocked",
    )
}

fn admission_not_cancelled(cancellation: &AtomicBool) -> Result<(), SidecarError> {
    if cancellation.load(Ordering::Acquire) {
        return Err(SidecarError::new(
            "storageAdmissionCancelled",
            "storage admission was cancelled before startup registration",
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn g4_platform_gate() -> Result<(), SidecarError> {
    Err(SidecarError::new(
        "unsupportedPlatform",
        "G4 project safety is gated to macOS/Unix until Windows handle-bound Git creation, directory durability, replace, and watcher behavior are verified",
    ))
}

#[cfg(target_os = "macos")]
fn g4_platform_gate() -> Result<(), SidecarError> {
    Ok(())
}

struct HandledRequest {
    response: Option<ResponseV1>,
    events: Vec<EventV1>,
}

impl HandledRequest {
    fn response(response: ResponseV1) -> Self {
        Self {
            response: Some(response),
            events: Vec::new(),
        }
    }
}

fn require_exact_params<'a>(
    value: &'a Value,
    expected: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "params must be an object".to_string())?;
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err("params contain missing or unknown fields".to_string());
    }
    Ok(object)
}

fn workspace_address(object: &Map<String, Value>) -> Result<WorkspaceAddress, String> {
    Ok(WorkspaceAddress {
        chat_id: param_string(object, "chatId")?.to_string(),
        project_id: param_string(object, "projectId")?.to_string(),
        worktree_id: param_string(object, "worktreeId")?.to_string(),
        workspace_identity: param_string(object, "workspaceIdentity")?.to_string(),
    })
}

fn addressed_action(
    mut action: project::ProjectAction,
    address: &WorkspaceAddress,
) -> project::ProjectAction {
    if let Some(result) = action.result.as_object_mut() {
        result.insert("chatId".to_string(), Value::String(address.chat_id.clone()));
        result.insert(
            "projectId".to_string(),
            Value::String(address.project_id.clone()),
        );
        result.insert(
            "worktreeId".to_string(),
            Value::String(address.worktree_id.clone()),
        );
        result.insert(
            "workspaceIdentity".to_string(),
            Value::String(address.workspace_identity.clone()),
        );
    }
    action
}

fn param_string<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} must be a non-empty string"))
}

fn param_string_allow_empty<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} must be a string"))
}

fn param_nullable_string<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, String> {
    match object.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value)),
        _ => Err(format!("{name} must be a non-empty string or null")),
    }
}

fn param_u64(object: &Map<String, Value>, name: &str) -> Result<u64, String> {
    object
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{name} must be a non-negative integer"))
}

fn invalid_params(detail: String) -> SidecarError {
    SidecarError::new("invalidParams", detail)
}

fn project_error_response(request: &RequestV1, failure: &ProjectFailure) -> ResponseV1 {
    ResponseV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "response".to_string(),
        sidecar_instance_id: request.sidecar_instance_id.clone(),
        request_id: request.request_id.clone(),
        ok: false,
        result: None,
        error: Some(ErrorV1 {
            code: failure.code.to_string(),
            kind: failure.kind.to_string(),
            detail: failure.detail.clone(),
            retryable: false,
            outcome_unknown: Some(failure.outcome_unknown),
        }),
    }
}

fn success_response(request: &RequestV1, result: Value) -> ResponseV1 {
    ResponseV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "response".to_string(),
        sidecar_instance_id: request.sidecar_instance_id.clone(),
        request_id: request.request_id.clone(),
        ok: true,
        result: Some(result),
        error: None,
    }
}

fn admission_blocked_response(request: &RequestV1, state: StorageAdmissionStateV1) -> ResponseV1 {
    let (code, detail, retryable) = match state {
        StorageAdmissionStateV1::Verifying => (
            "storageVerifying",
            STORAGE_VERIFYING_BLOCKED_DETAIL_V1,
            true,
        ),
        StorageAdmissionStateV1::Failed => (
            "storageAdmissionFailed",
            STORAGE_ADMISSION_FAILED_DETAIL_V1,
            false,
        ),
        StorageAdmissionStateV1::Ready => unreachable!("ready requests are not blocked"),
    };
    ResponseV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "response".to_string(),
        sidecar_instance_id: request.sidecar_instance_id.clone(),
        request_id: request.request_id.clone(),
        ok: false,
        result: None,
        error: Some(ErrorV1 {
            code: code.to_string(),
            kind: "requestBlocked".to_string(),
            detail: detail.to_string(),
            retryable,
            outcome_unknown: Some(false),
        }),
    }
}

fn error_response(request: &RequestV1, code: &str, kind: &str, detail: &str) -> ResponseV1 {
    ResponseV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "response".to_string(),
        sidecar_instance_id: request.sidecar_instance_id.clone(),
        request_id: request.request_id.clone(),
        ok: false,
        result: None,
        error: Some(ErrorV1 {
            code: code.to_string(),
            kind: kind.to_string(),
            detail: detail.to_string(),
            retryable: false,
            outcome_unknown: None,
        }),
    }
}

fn new_sidecar_instance_id() -> String {
    format!("sidecar_{}", Uuid::new_v4().simple())
}

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn protocol_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn protocol_error(error: impl Display) -> SidecarError {
    SidecarError::new("protocolViolation", error.to_string())
}

fn io_error(error: io::Error) -> SidecarError {
    SidecarError::new("stdioFailure", error.to_string())
}

fn json_error(error: serde_json::Error) -> SidecarError {
    SidecarError::new("jsonFailure", error.to_string())
}

#[cfg(test)]
mod save_recovery_dispatch_tests;

#[cfg(all(test, target_os = "macos"))]
mod tests;

#[cfg(all(test, not(target_os = "macos")))]
mod unsupported_platform_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn unsupported_platform_fails_before_database_or_ready_advertisement() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join(SQLITE_FILE_V1);
        let mut output = Vec::new();
        let error = serve_stdio_with_identity(
            &database,
            Cursor::new(Vec::<u8>::new()),
            &mut output,
            "sidecar_windows_gate".to_string(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), "unsupportedPlatform");
        assert!(output.is_empty());
        assert!(!database.exists());
    }
}
