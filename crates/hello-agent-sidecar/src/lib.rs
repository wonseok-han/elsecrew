//! M5 Rust sidecar의 G3 저장소와 bounded JSONL 전송 구현.
//!
//! 이 단계는 `system.status`와 `app.prepareQuit`만 제공한다. 프로젝트,
//! worktree, 에이전트 실행은 후속 게이트에서 추가한다.

mod storage;

use hello_agent_core as _;
use hello_agent_protocol::lifecycle_v1::{
    DatabaseStatusV1, SystemStatusV1, G3_CAPABILITIES_V1, SQLITE_FILE_V1,
};
use hello_agent_protocol::{
    decode_jsonl, encode_jsonl, EnvelopeV1, ErrorV1, EventV1, PrepareQuitResultV1,
    ProtocolSessionV1, ReadyPayloadV1, RequestV1, ResponseV1, APP_SCHEMA_VERSION,
    MAX_CONTROL_LINE_BYTES, PROTOCOL_VERSION, SIDECAR_SCHEMA_VERSION,
};
use std::ffi::OsString;
use std::fmt::{self, Display};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use uuid::Uuid;

pub use storage::{AppendFailurePoint, AppendOutcome, Storage};

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
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--stdio")) || args.next().is_some() {
        return Err(SidecarError::new(
            "invalidArguments",
            "sidecar requires exactly one argument: --stdio",
        ));
    }

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
    run_stdio(&database_path, stdin.lock(), stdout.lock())
}

pub fn run_stdio<R: Read, W: Write>(
    database_path: &Path,
    input: R,
    output: W,
) -> Result<(), SidecarError> {
    serve_stdio_with_identity(database_path, input, output, new_sidecar_instance_id())
}

fn serve_stdio_with_identity<R: Read, W: Write>(
    database_path: &Path,
    input: R,
    mut output: W,
    sidecar_instance_id: String,
) -> Result<(), SidecarError> {
    let mut sidecar = Sidecar::start(database_path, sidecar_instance_id)?;
    let ready = EnvelopeV1::Event(sidecar.ready_event()?);
    let mut session = ProtocolSessionV1::default();
    session.observe(&ready).map_err(protocol_error)?;
    write_envelope(&mut output, &ready)?;

    let mut input = BufReader::with_capacity(8 * 1024, input);
    loop {
        let Some(line) = read_bounded_jsonl(&mut input)? else {
            if sidecar.prepared_to_quit {
                return Ok(());
            }
            return Err(SidecarError::new(
                "unexpectedEof",
                "stdin closed before app.prepareQuit; clean shutdown was not marked",
            ));
        };
        let envelope = decode_jsonl(&line).map_err(protocol_error)?;
        session.observe(&envelope).map_err(protocol_error)?;
        let EnvelopeV1::Request(request) = envelope else {
            return Err(SidecarError::new(
                "protocolViolation",
                "sidecar stdin accepts request envelopes only",
            ));
        };
        let response = EnvelopeV1::Response(sidecar.handle_request(request)?);
        session.observe(&response).map_err(protocol_error)?;
        write_envelope(&mut output, &response)?;
    }
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

struct Sidecar {
    storage: Storage,
    ready: ReadyPayloadV1,
    prepared_to_quit: bool,
}

impl Sidecar {
    fn start(database_path: &Path, sidecar_instance_id: String) -> Result<Self, SidecarError> {
        let mut storage = Storage::open(database_path)?;
        storage.recover_unclean_shutdown()?;
        let ready = ReadyPayloadV1 {
            protocol_version: PROTOCOL_VERSION,
            sidecar_schema_version: SIDECAR_SCHEMA_VERSION,
            app_schema_version: APP_SCHEMA_VERSION,
            pid: std::process::id(),
            platform: protocol_platform().to_string(),
            capabilities: G3_CAPABILITIES_V1
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            sidecar_instance_id,
        };
        storage.register_app_instance(&ready)?;
        Ok(Self {
            storage,
            ready,
            prepared_to_quit: false,
        })
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

    fn handle_request(&mut self, request: RequestV1) -> Result<ResponseV1, SidecarError> {
        if request
            .params
            .as_object()
            .is_none_or(|params| !params.is_empty())
        {
            return Ok(error_response(
                &request,
                "invalidParams",
                "invalidRequest",
                "G3 methods accept an empty params object",
            ));
        }
        if self.prepared_to_quit && request.method != "app.prepareQuit" {
            return Ok(error_response(
                &request,
                "quitting",
                "requestBlocked",
                "sidecar is prepared to quit and blocks new requests",
            ));
        }

        let result = match request.method.as_str() {
            "system.status" => serde_json::to_value(self.system_status()).map_err(json_error)?,
            "app.prepareQuit" => {
                if !self.prepared_to_quit {
                    self.storage.checkpoint()?;
                    self.storage
                        .mark_app_instance_clean(&self.ready.sidecar_instance_id)?;
                    self.prepared_to_quit = true;
                }
                serde_json::to_value(PrepareQuitResultV1 {
                    safe_to_quit: true,
                    blockers: Vec::new(),
                })
                .map_err(json_error)?
            }
            _ => {
                return Ok(error_response(
                    &request,
                    "methodUnavailable",
                    "unsupportedCapability",
                    "G3 sidecar exposes only system.status and app.prepareQuit",
                ))
            }
        };
        Ok(ResponseV1 {
            protocol_version: PROTOCOL_VERSION,
            kind: "response".to_string(),
            sidecar_instance_id: request.sidecar_instance_id,
            request_id: request.request_id,
            ok: true,
            result: Some(result),
            error: None,
        })
    }

    fn system_status(&self) -> SystemStatusV1 {
        match self.storage.blocker_ids() {
            Ok(blocker_ids) => SystemStatusV1::Ready {
                sidecar: self.ready.clone(),
                database: DatabaseStatusV1::Healthy {
                    schema_version: APP_SCHEMA_VERSION,
                },
                recovery_required: !blocker_ids.is_empty(),
                blocker_ids,
            },
            Err(error) => SystemStatusV1::Unavailable {
                last_ready_sidecar: Some(self.ready.clone()),
                database: DatabaseStatusV1::Unavailable {
                    schema_version: Some(APP_SCHEMA_VERSION),
                },
                recovery_required: true,
                blocker_ids: vec!["storage:unknown".to_string()],
                error: ErrorV1 {
                    code: "storageUnavailable".to_string(),
                    kind: error.kind().to_string(),
                    detail: error.detail().to_string(),
                    retryable: false,
                    outcome_unknown: Some(true),
                },
            },
        }
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
mod tests;
