use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display};

pub mod lifecycle_v1;
pub mod project_v1;
pub mod storage_v1;

pub const PROTOCOL_VERSION: u32 = 1;
pub const SIDECAR_SCHEMA_VERSION: u32 = 1;
pub const APP_SCHEMA_VERSION: u32 = 1;
pub const MAX_CONTROL_LINE_BYTES: usize = 1024 * 1024;
pub const MAX_TEXT_CHUNK_BYTES: usize = 256 * 1024;
pub const MAX_SEQUENCE: u64 = (1_u64 << 53) - 1;

pub const METHODS_V1: &[&str] = &[
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
    "chat.create",
    "chat.list",
    "chat.get",
    "run.startFixture",
    "run.startAgentSmoke",
    "run.cancel",
    "recovery.list",
    "recovery.inspect",
    "recovery.markResolved",
];

pub const EVENTS_V1: &[&str] = &[
    "system.ready",
    "system.fatal",
    "file.changed",
    "file.conflict",
    "diff.changed",
    "chat.eventAppended",
    "run.event",
    "run.finished",
    "blob.chunk",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    detail: String,
}

impl ProtocolError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for ProtocolError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RequestV1 {
    pub protocol_version: u32,
    pub kind: String,
    pub sidecar_instance_id: String,
    pub request_id: String,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResponseV1 {
    pub protocol_version: u32,
    pub kind: String,
    pub sidecar_instance_id: String,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ErrorV1 {
    pub code: String,
    pub kind: String,
    pub detail: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_unknown: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EventV1 {
    pub protocol_version: u32,
    pub kind: String,
    pub sidecar_instance_id: String,
    pub stream_id: String,
    pub seq: u64,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EnvelopeV1 {
    Request(RequestV1),
    Response(ResponseV1),
    Event(EventV1),
}

impl EnvelopeV1 {
    pub fn sidecar_instance_id(&self) -> &str {
        match self {
            Self::Request(value) => &value.sidecar_instance_id,
            Self::Response(value) => &value.sidecar_instance_id,
            Self::Event(value) => &value.sidecar_instance_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReadyPayloadV1 {
    pub protocol_version: u32,
    pub sidecar_schema_version: u32,
    pub app_schema_version: u32,
    pub pid: u32,
    pub platform: String,
    pub capabilities: Vec<String>,
    pub sidecar_instance_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PrepareQuitResultV1 {
    pub safe_to_quit: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ProtocolSessionV1 {
    ready_received: bool,
    sidecar_instance_id: Option<String>,
    request_ids: HashSet<String>,
    response_ids: HashSet<String>,
    stream_sequences: HashMap<String, u64>,
}

impl ProtocolSessionV1 {
    pub fn observe(&mut self, envelope: &EnvelopeV1) -> Result<(), ProtocolError> {
        if !self.ready_received {
            let EnvelopeV1::Event(event) = envelope else {
                return Err(ProtocolError::new(
                    "the first protocol envelope must be system.ready",
                ));
            };
            if event.event != "system.ready" {
                return Err(ProtocolError::new(
                    "the first protocol envelope must be system.ready",
                ));
            }
            self.ready_received = true;
            self.sidecar_instance_id = Some(event.sidecar_instance_id.clone());
        } else if matches!(envelope, EnvelopeV1::Event(event) if event.event == "system.ready") {
            return Err(ProtocolError::new(
                "system.ready can appear only once per session",
            ));
        }

        let instance_id = envelope.sidecar_instance_id();
        if let Some(expected) = &self.sidecar_instance_id {
            if expected != instance_id {
                return Err(ProtocolError::new(
                    "sidecarInstanceId changed within a session",
                ));
            }
        }

        match envelope {
            EnvelopeV1::Request(request) => {
                if !self.request_ids.insert(request.request_id.clone()) {
                    return Err(ProtocolError::new(
                        "requestId must be unique within a session",
                    ));
                }
            }
            EnvelopeV1::Response(response) => {
                if !self.request_ids.contains(&response.request_id) {
                    return Err(ProtocolError::new(
                        "response references an unknown requestId",
                    ));
                }
                if !self.response_ids.insert(response.request_id.clone()) {
                    return Err(ProtocolError::new(
                        "requestId received more than one response",
                    ));
                }
            }
            EnvelopeV1::Event(event) => {
                let expected = match self.stream_sequences.get(&event.stream_id) {
                    Some(last) => last
                        .checked_add(1)
                        .ok_or_else(|| ProtocolError::new("stream seq overflow"))?,
                    None => 1,
                };
                if event.seq != expected {
                    return Err(ProtocolError::new(format!(
                        "stream {} expected seq {expected}, got {}",
                        event.stream_id, event.seq
                    )));
                }
                self.stream_sequences
                    .insert(event.stream_id.clone(), event.seq);
            }
        }
        Ok(())
    }
}

pub fn decode_jsonl(line: &[u8]) -> Result<EnvelopeV1, ProtocolError> {
    let line = strip_delimiter(line)?;
    if line.is_empty() {
        return Err(ProtocolError::new("protocol line is empty"));
    }
    if line.len() > MAX_CONTROL_LINE_BYTES {
        return Err(ProtocolError::new("control line exceeds 1 MiB"));
    }
    if line.contains(&b'\n') || line.contains(&b'\r') {
        return Err(ProtocolError::new(
            "protocol input must contain exactly one JSON line",
        ));
    }

    let value: Value = serde_json::from_slice(line)
        .map_err(|error| ProtocolError::new(format!("invalid JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| ProtocolError::new("protocol line must be a JSON object"))?;
    let version = object
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtocolError::new("protocolVersion is required"))?;
    if version != u64::from(PROTOCOL_VERSION) {
        return Err(ProtocolError::new(format!(
            "unsupported protocolVersion: {version}"
        )));
    }
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::new("kind is required"))?;

    let envelope = match kind {
        "request" => EnvelopeV1::Request(deserialize(value)?),
        "response" => EnvelopeV1::Response(deserialize(value)?),
        "event" => EnvelopeV1::Event(deserialize(value)?),
        other => return Err(ProtocolError::new(format!("unknown kind: {other}"))),
    };
    validate_envelope(&envelope)?;
    Ok(envelope)
}

pub fn encode_jsonl(envelope: &EnvelopeV1) -> Result<String, ProtocolError> {
    validate_envelope(envelope)?;
    let value = match envelope {
        EnvelopeV1::Request(value) => serde_json::to_value(value),
        EnvelopeV1::Response(value) => serde_json::to_value(value),
        EnvelopeV1::Event(value) => serde_json::to_value(value),
    }
    .map_err(|error| ProtocolError::new(format!("could not encode envelope: {error}")))?;
    let canonical = canonicalize(value);
    let encoded = serde_json::to_string(&canonical)
        .map_err(|error| ProtocolError::new(format!("could not encode envelope: {error}")))?;
    if encoded.len() > MAX_CONTROL_LINE_BYTES {
        return Err(ProtocolError::new("control line exceeds 1 MiB"));
    }
    Ok(format!("{encoded}\n"))
}

pub fn ready_payload(event: &EventV1) -> Result<ReadyPayloadV1, ProtocolError> {
    if event.event != "system.ready" {
        return Err(ProtocolError::new("event is not system.ready"));
    }
    let payload: ReadyPayloadV1 = deserialize(event.payload.clone())?;
    if payload.protocol_version != PROTOCOL_VERSION {
        return Err(ProtocolError::new(
            "ready protocolVersion does not match v1",
        ));
    }
    if payload.sidecar_schema_version != SIDECAR_SCHEMA_VERSION
        || payload.app_schema_version != APP_SCHEMA_VERSION
    {
        return Err(ProtocolError::new("ready schema version does not match v1"));
    }
    if payload.pid == 0 || payload.platform.is_empty() || payload.capabilities.is_empty() {
        return Err(ProtocolError::new(
            "ready identity and capabilities are required",
        ));
    }
    if payload.capabilities.iter().any(|value| value.is_empty()) {
        return Err(ProtocolError::new(
            "ready capabilities cannot contain empty values",
        ));
    }
    if payload
        .capabilities
        .iter()
        .any(|value| !METHODS_V1.contains(&value.as_str()))
    {
        return Err(ProtocolError::new("ready contains an unknown capability"));
    }
    let unique: HashSet<_> = payload.capabilities.iter().collect();
    if unique.len() != payload.capabilities.len() {
        return Err(ProtocolError::new("ready capabilities must be unique"));
    }
    if payload.sidecar_instance_id != event.sidecar_instance_id {
        return Err(ProtocolError::new(
            "ready sidecarInstanceId does not match its envelope",
        ));
    }
    Ok(payload)
}

pub fn prepare_quit_result(response: &ResponseV1) -> Result<PrepareQuitResultV1, ProtocolError> {
    if !response.ok {
        return Err(ProtocolError::new("prepareQuit response is an error"));
    }
    let result = response
        .result
        .clone()
        .ok_or_else(|| ProtocolError::new("prepareQuit result is missing"))?;
    let result: PrepareQuitResultV1 = deserialize(result)?;
    if result.safe_to_quit && !result.blockers.is_empty() {
        return Err(ProtocolError::new("safeToQuit cannot include blockers"));
    }
    if !result.safe_to_quit && result.blockers.is_empty() {
        return Err(ProtocolError::new(
            "unsafe prepareQuit must include blockers",
        ));
    }
    if result.blockers.iter().any(|value| value.is_empty()) {
        return Err(ProtocolError::new("prepareQuit blockers cannot be empty"));
    }
    Ok(result)
}

fn validate_envelope(envelope: &EnvelopeV1) -> Result<(), ProtocolError> {
    require_id(envelope.sidecar_instance_id(), "sidecarInstanceId")?;
    match envelope {
        EnvelopeV1::Request(request) => {
            require_kind_and_version(&request.kind, request.protocol_version, "request")?;
            require_id(&request.request_id, "requestId")?;
            if !METHODS_V1.contains(&request.method.as_str()) {
                return Err(ProtocolError::new(format!(
                    "unknown protocol method: {}",
                    request.method
                )));
            }
            if !request.params.is_object() {
                return Err(ProtocolError::new("request params must be an object"));
            }
        }
        EnvelopeV1::Response(response) => {
            require_kind_and_version(&response.kind, response.protocol_version, "response")?;
            require_id(&response.request_id, "requestId")?;
            match (response.ok, &response.result, &response.error) {
                (true, Some(_), None) => {}
                (false, None, Some(error)) => validate_error(error)?,
                _ => {
                    return Err(ProtocolError::new(
                        "response must contain exactly one matching result or error",
                    ))
                }
            }
        }
        EnvelopeV1::Event(event) => {
            require_kind_and_version(&event.kind, event.protocol_version, "event")?;
            require_id(&event.stream_id, "streamId")?;
            if event.seq == 0 || event.seq > MAX_SEQUENCE {
                return Err(ProtocolError::new("event seq must be between 1 and 2^53-1"));
            }
            if !EVENTS_V1.contains(&event.event.as_str()) {
                return Err(ProtocolError::new(format!(
                    "unknown protocol event: {}",
                    event.event
                )));
            }
            for (name, value) in [
                ("chatId", &event.chat_id),
                ("taskId", &event.task_id),
                ("runId", &event.run_id),
            ] {
                if let Some(value) = value {
                    require_id(value, name)?;
                }
            }
            if matches!(event.event.as_str(), "run.event" | "run.finished")
                && (event.chat_id.is_none() || event.task_id.is_none() || event.run_id.is_none())
            {
                return Err(ProtocolError::new(
                    "run events require explicit chatId, taskId, and runId",
                ));
            }
            if event.event == "chat.eventAppended" && event.chat_id.is_none() {
                return Err(ProtocolError::new(
                    "chat.eventAppended requires explicit chatId",
                ));
            }
            if !event.payload.is_object() {
                return Err(ProtocolError::new("event payload must be an object"));
            }
            if event.event == "blob.chunk" {
                let payload = event
                    .payload
                    .as_object()
                    .expect("payload was checked above");
                if payload.len() != 1 || !matches!(payload.get("text"), Some(Value::String(_))) {
                    return Err(ProtocolError::new(
                        "blob.chunk payload must contain exactly one text field",
                    ));
                }
            }
            if let Some(text) = event.payload.get("text").and_then(Value::as_str) {
                if text.len() > MAX_TEXT_CHUNK_BYTES {
                    return Err(ProtocolError::new("text chunk exceeds 256 KiB"));
                }
            }
            if event.event == "system.ready" {
                ready_payload(event)?;
            }
        }
    }
    Ok(())
}

fn validate_error(error: &ErrorV1) -> Result<(), ProtocolError> {
    for (name, value) in [
        ("error.code", &error.code),
        ("error.kind", &error.kind),
        ("error.detail", &error.detail),
    ] {
        if value.is_empty() {
            return Err(ProtocolError::new(format!("{name} is required")));
        }
    }
    if error.outcome_unknown == Some(true) && error.retryable {
        return Err(ProtocolError::new(
            "an error with an unknown outcome cannot be auto-retryable",
        ));
    }
    Ok(())
}

fn require_kind_and_version(
    kind: &str,
    protocol_version: u32,
    expected_kind: &str,
) -> Result<(), ProtocolError> {
    if protocol_version != PROTOCOL_VERSION || kind != expected_kind {
        return Err(ProtocolError::new(
            "envelope kind or protocolVersion mismatch",
        ));
    }
    Ok(())
}

fn require_id(value: &str, name: &str) -> Result<(), ProtocolError> {
    if value.is_empty() {
        return Err(ProtocolError::new(format!("{name} is required")));
    }
    Ok(())
}

fn strip_delimiter(line: &[u8]) -> Result<&[u8], ProtocolError> {
    if let Some(without_lf) = line.strip_suffix(b"\n") {
        return Ok(without_lf.strip_suffix(b"\r").unwrap_or(without_lf));
    }
    if line.ends_with(b"\r") {
        return Err(ProtocolError::new(
            "bare carriage return is not a JSONL delimiter",
        ));
    }
    Ok(line)
}

fn deserialize<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ProtocolError> {
    serde_json::from_value(value)
        .map_err(|error| ProtocolError::new(format!("invalid protocol shape: {error}")))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut canonical = Map::new();
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, value) in entries {
                canonical.insert(key, canonicalize(value));
            }
            Value::Object(canonical)
        }
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests;
