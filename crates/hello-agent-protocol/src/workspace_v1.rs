use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::error::Error;
use std::fmt::{self, Display};

pub const WORKSPACE_CAPABILITIES_V1: &[&str] = &[
    "workspace.getTree",
    "workspace.close",
    "workspace.file.read",
    "workspace.file.save",
    "workspace.file.watch",
    "workspace.diff.getSummary",
    "workspace.diff.getFile",
];
pub const WORKSPACE_EVENTS_V1: &[&str] = &["workspace.changed", "workspace.blob.chunk"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceRefV1 {
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceContractError(String);
impl Display for WorkspaceContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl Error for WorkspaceContractError {}

pub fn validate_workspace_payload_v1(
    validator: &str,
    value: &Value,
) -> Result<(), WorkspaceContractError> {
    match validator {
        "workspaceRef" => validate_workspace_ref(value).map(|_| ()),
        "workspaceOperation" => validate_workspace_operation(value),
        "workspaceSave" => validate_workspace_save(value),
        "workspaceEvent" => validate_workspace_event(value),
        "workspaceEventRoute" => validate_workspace_event_route(value),
        "workspaceChatBinding" => validate_workspace_chat_binding(value),
        other => fail(format!("unknown workspace validator: {other}")),
    }
}

fn validate_workspace_ref(value: &Value) -> Result<WorkspaceRefV1, WorkspaceContractError> {
    let object = object(value, "workspace reference")?;
    exact_keys(object, &["projectId", "worktreeId", "workspaceIdentity"])?;
    workspace_ref_fields(object)
}

fn validate_workspace_operation(value: &Value) -> Result<(), WorkspaceContractError> {
    let transcript = object(value, "workspace operation transcript")?;
    exact_keys(transcript, &["operation", "request", "response"])?;
    let operation = string(transcript.get("operation"), "operation")?;
    let request = object(required(transcript, "request")?, "workspace request")?;
    let response = object(required(transcript, "response")?, "workspace response")?;
    let request_ref;
    let response_ref;
    match operation {
        "getTree" => {
            exact_keys(
                request,
                &[
                    "projectId",
                    "worktreeId",
                    "workspaceIdentity",
                    "parentPath",
                    "cursor",
                ],
            )?;
            request_ref = workspace_ref_fields(request)?;
            relative_path(request.get("parentPath"), "parentPath", true)?;
            nullable_id(request.get("cursor"), "cursor")?;
            exact_keys(
                response,
                &[
                    "projectId",
                    "worktreeId",
                    "workspaceIdentity",
                    "parentPath",
                    "entries",
                    "nextCursor",
                ],
            )?;
            if request.get("parentPath") != response.get("parentPath") {
                return fail("tree response parentPath must match the request");
            }
            response_ref = workspace_ref_fields(response)?;
            delegate_project(
                "treePage",
                &json!({
                    "projectId": response.get("projectId"),
                    "parentPath": response.get("parentPath"),
                    "entries": response.get("entries"),
                    "nextCursor": response.get("nextCursor")
                }),
            )?;
        }
        "close" => {
            request_ref = validate_workspace_ref(required(transcript, "request")?)?;
            exact_keys(
                response,
                &[
                    "projectId",
                    "worktreeId",
                    "workspaceIdentity",
                    "lifecycle",
                    "physicallyDeleted",
                ],
            )?;
            response_ref = workspace_ref_fields(response)?;
            delegate_project(
                "projectClose",
                &json!({
                    "projectId": response.get("projectId"),
                    "lifecycle": response.get("lifecycle"),
                    "physicallyDeleted": response.get("physicallyDeleted")
                }),
            )?;
        }
        "fileRead" => {
            exact_keys(
                request,
                &["projectId", "worktreeId", "workspaceIdentity", "path"],
            )?;
            request_ref = workspace_ref_fields(request)?;
            relative_path(request.get("path"), "file path", false)?;
            exact_keys(
                response,
                &[
                    "projectId",
                    "worktreeId",
                    "workspaceIdentity",
                    "path",
                    "classification",
                    "byteLength",
                    "version",
                    "stream",
                ],
            )?;
            if request.get("path") != response.get("path") {
                return fail("file read response path must match the request");
            }
            response_ref = workspace_ref_fields(response)?;
            delegate_project(
                "fileRead",
                &json!({
                    "projectId": response.get("projectId"), "path": response.get("path"),
                    "classification": response.get("classification"), "byteLength": response.get("byteLength"),
                    "version": response.get("version"), "stream": response.get("stream")
                }),
            )?;
        }
        "fileWatch" => {
            request_ref = validate_workspace_ref(required(transcript, "request")?)?;
            exact_keys(
                response,
                &["projectId", "worktreeId", "workspaceIdentity", "watching"],
            )?;
            response_ref = workspace_ref_fields(response)?;
            if response.get("watching") != Some(&Value::Bool(true)) {
                return fail("workspace watch response must confirm watching");
            }
        }
        "diffSummary" => {
            request_ref = validate_workspace_ref(required(transcript, "request")?)?;
            exact_keys(
                response,
                &[
                    "projectId",
                    "worktreeId",
                    "workspaceIdentity",
                    "baseCommit",
                    "entries",
                ],
            )?;
            response_ref = workspace_ref_fields(response)?;
            delegate_project(
                "diffSummary",
                &json!({
                    "projectId": response.get("projectId"),
                    "baseCommit": response.get("baseCommit"),
                    "entries": response.get("entries")
                }),
            )?;
        }
        "diffFile" => {
            exact_keys(
                request,
                &["projectId", "worktreeId", "workspaceIdentity", "path"],
            )?;
            request_ref = workspace_ref_fields(request)?;
            relative_path(request.get("path"), "diff path", false)?;
            exact_keys(
                response,
                &[
                    "projectId",
                    "worktreeId",
                    "workspaceIdentity",
                    "kind",
                    "path",
                    "oldPath",
                    "baseVersion",
                    "worktreeVersion",
                    "byteLength",
                    "stream",
                ],
            )?;
            if request.get("path") != response.get("path") {
                return fail("diff file response path must match the request");
            }
            response_ref = workspace_ref_fields(response)?;
            delegate_project(
                "diffFile",
                &json!({
                    "projectId": response.get("projectId"), "kind": response.get("kind"),
                    "path": response.get("path"), "oldPath": response.get("oldPath"),
                    "baseVersion": response.get("baseVersion"), "worktreeVersion": response.get("worktreeVersion"),
                    "byteLength": response.get("byteLength"), "stream": response.get("stream")
                }),
            )?;
        }
        _ => return fail("unknown workspace operation"),
    }
    exact_echo(&request_ref, &response_ref, "workspace operation response")
}

fn validate_workspace_save(value: &Value) -> Result<(), WorkspaceContractError> {
    let transcript = object(value, "workspace save transcript")?;
    exact_keys(
        transcript,
        &[
            "beginRequest",
            "beginResult",
            "chunks",
            "commitRequest",
            "commitResult",
            "currentVersionBefore",
        ],
    )?;
    let begin_request = object(required(transcript, "beginRequest")?, "save begin request")?;
    exact_keys(
        begin_request,
        &[
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "path",
            "expectedVersion",
            "totalBytes",
            "encoding",
        ],
    )?;
    let workspace = workspace_ref_fields(begin_request)?;
    let begin_result = object(required(transcript, "beginResult")?, "save begin result")?;
    exact_keys(
        begin_result,
        &[
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "saveId",
            "chunkBytes",
        ],
    )?;
    exact_echo(
        &workspace,
        &workspace_ref_fields(begin_result)?,
        "save begin result",
    )?;
    let save_id = string(begin_result.get("saveId"), "saveId")?;
    let exchanges = array(transcript.get("chunks"), "save chunks")?;
    let mut chunks = Vec::new();
    for (index, exchange) in exchanges.iter().enumerate() {
        let exchange = object(exchange, "save chunk exchange")?;
        exact_keys(exchange, &["request", "response"])?;
        let request = object(required(exchange, "request")?, "save chunk request")?;
        let response = object(required(exchange, "response")?, "save chunk response")?;
        exact_keys(request, &["saveId", "seq", "text"])?;
        exact_keys(response, &["saveId", "acceptedSeq"])?;
        let expected_seq = u64::try_from(index + 1).expect("bounded fixture sequence");
        if request.get("saveId").and_then(Value::as_str) != Some(save_id)
            || response.get("saveId").and_then(Value::as_str) != Some(save_id)
            || request.get("seq").and_then(Value::as_u64) != Some(expected_seq)
            || response.get("acceptedSeq").and_then(Value::as_u64) != Some(expected_seq)
        {
            return fail("saveId/sequence must stay bound after begin");
        }
        chunks.push(Value::Object(request.clone()));
    }
    let commit = required(transcript, "commitRequest")?;
    let commit_object = object(commit, "save commit request")?;
    exact_keys(
        commit_object,
        &["saveId", "expectedVersion", "lastSeq", "totalBytes"],
    )?;
    if commit_object.get("saveId").and_then(Value::as_str) != Some(save_id) {
        return fail("workspace save commit saveId mismatch");
    }
    let result = object(required(transcript, "commitResult")?, "save commit result")?;
    let outcome = string(result.get("outcome"), "save outcome")?;
    let expected_keys = if outcome == "committed" {
        &[
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "saveId",
            "outcome",
            "version",
        ][..]
    } else {
        &[
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "saveId",
            "outcome",
            "error",
        ][..]
    };
    exact_keys(result, expected_keys)?;
    exact_echo(
        &workspace,
        &workspace_ref_fields(result)?,
        "save commit result",
    )?;
    if result.get("saveId").and_then(Value::as_str) != Some(save_id) {
        return fail("workspace save result saveId mismatch");
    }
    if outcome == "committed" {
        validate_version(result.get("version"))?;
    } else if outcome == "dirtyConflict" {
        delegate_project("safetyError", required(result, "error")?)?;
    } else {
        return fail("unknown workspace save outcome");
    }
    delegate_project(
        "saveTranscript",
        &json!({
            "begin": {
                "saveId": save_id, "projectId": workspace.project_id, "path": begin_request.get("path"),
                "expectedVersion": begin_request.get("expectedVersion"), "totalBytes": begin_request.get("totalBytes"),
                "encoding": begin_request.get("encoding"), "chunkBytes": begin_result.get("chunkBytes")
            },
            "chunks": chunks,
            "commit": commit,
            "currentVersion": transcript.get("currentVersionBefore"),
            "outcome": outcome
        }),
    )
}

fn validate_workspace_event(value: &Value) -> Result<(), WorkspaceContractError> {
    let event = object(value, "workspace event")?;
    exact_keys(
        event,
        &[
            "protocolVersion",
            "kind",
            "sidecarInstanceId",
            "streamId",
            "seq",
            "event",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "payload",
        ],
    )?;
    workspace_ref_fields(event)?;
    let encoded = serde_json::to_vec(value)
        .map_err(|error| WorkspaceContractError(format!("encode workspace event: {error}")))?;
    super::decode_jsonl(&encoded)
        .map_err(|error| WorkspaceContractError(format!("invalid workspace event: {error}")))?;
    match string(event.get("event"), "workspace event name")? {
        "workspace.changed" => {
            let payload = object(required(event, "payload")?, "workspace changed payload")?;
            exact_keys(payload, &["event", "paths", "rescanRequired"])?;
            delegate_project(
                "watchEvent",
                &json!({
                    "projectId": event.get("projectId"), "event": payload.get("event"),
                    "paths": payload.get("paths"), "rescanRequired": payload.get("rescanRequired")
                }),
            )
        }
        "workspace.blob.chunk" => Ok(()),
        _ => fail("unknown workspace event"),
    }
}

fn validate_workspace_chat_binding(value: &Value) -> Result<(), WorkspaceContractError> {
    let transcript = object(value, "workspace chat binding transcript")?;
    exact_keys(transcript, &["chat", "verifiedSnapshot", "workspace"])?;
    let chat = object(required(transcript, "chat")?, "chat")?;
    super::run_recovery_v1::validate_g5_payload_v1("chatRecord", required(transcript, "chat")?)
        .map_err(|error| WorkspaceContractError(format!("invalid chat: {error}")))?;
    let snapshot = object(required(transcript, "verifiedSnapshot")?, "snapshot")?;
    delegate_project(
        "worktreeManifest",
        required(transcript, "verifiedSnapshot")?,
    )?;
    if snapshot.get("lifecycle").and_then(Value::as_str) != Some("active") {
        return fail("chat workspace addressing requires an active snapshot");
    }
    let chat_ref = workspace_ref_fields(chat)?;
    exact_echo(&chat_ref, &workspace_ref_fields(snapshot)?, "chat snapshot")?;
    exact_echo(
        &chat_ref,
        &validate_workspace_ref(required(transcript, "workspace")?)?,
        "chat workspace request",
    )
}

fn validate_workspace_event_route(value: &Value) -> Result<(), WorkspaceContractError> {
    let transcript = object(value, "workspace event route transcript")?;
    exact_keys(transcript, &["chat", "event"])?;
    let chat = object(required(transcript, "chat")?, "chat")?;
    super::run_recovery_v1::validate_g5_payload_v1("chatRecord", required(transcript, "chat")?)
        .map_err(|error| WorkspaceContractError(format!("invalid chat: {error}")))?;
    validate_workspace_event(required(transcript, "event")?)?;
    let event = object(required(transcript, "event")?, "workspace event")?;
    exact_echo(
        &workspace_ref_fields(chat)?,
        &workspace_ref_fields(event)?,
        "workspace event route",
    )
}

fn delegate_project(validator: &str, value: &Value) -> Result<(), WorkspaceContractError> {
    super::project_v1::validate_project_payload_v1(validator, value)
        .map_err(|error| WorkspaceContractError(format!("invalid {validator}: {error}")))
}
fn validate_version(value: Option<&Value>) -> Result<(), WorkspaceContractError> {
    let Some(value) = value.and_then(Value::as_str) else {
        return fail("file version is required");
    };
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return fail("invalid file version");
    }
    Ok(())
}
fn exact_echo(
    expected: &WorkspaceRefV1,
    actual: &WorkspaceRefV1,
    name: &str,
) -> Result<(), WorkspaceContractError> {
    if expected != actual {
        return fail(format!(
            "{name} must exactly echo projectId/worktreeId/workspaceIdentity"
        ));
    }
    Ok(())
}
fn workspace_ref_fields(
    object: &Map<String, Value>,
) -> Result<WorkspaceRefV1, WorkspaceContractError> {
    Ok(WorkspaceRefV1 {
        project_id: id(object.get("projectId"), "projectId")?.to_string(),
        worktree_id: id(object.get("worktreeId"), "worktreeId")?.to_string(),
        workspace_identity: id(object.get("workspaceIdentity"), "workspaceIdentity")?.to_string(),
    })
}
fn object<'a>(
    value: &'a Value,
    name: &str,
) -> Result<&'a Map<String, Value>, WorkspaceContractError> {
    value
        .as_object()
        .ok_or_else(|| WorkspaceContractError(format!("{name} must be an object")))
}
fn exact_keys(
    object: &Map<String, Value>,
    expected: &[&str],
) -> Result<(), WorkspaceContractError> {
    if object.len() != expected.len() || object.keys().any(|key| !expected.contains(&key.as_str()))
    {
        return fail("workspace payload has unknown or missing fields");
    }
    Ok(())
}
fn required<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Value, WorkspaceContractError> {
    object
        .get(field)
        .ok_or_else(|| WorkspaceContractError(format!("{field} is required")))
}
fn string<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str, WorkspaceContractError> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| WorkspaceContractError(format!("{name} is required")))
}
fn id<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str, WorkspaceContractError> {
    let value = string(value, name)?;
    if value.len() > super::run_recovery_v1::G5_ID_MAX_BYTES_V1 {
        return fail(format!("{name} exceeds UTF-8 byte limit"));
    }
    Ok(value)
}
fn nullable_id(value: Option<&Value>, name: &str) -> Result<(), WorkspaceContractError> {
    if !value.is_some_and(Value::is_null) {
        id(value, name)?;
    }
    Ok(())
}
fn relative_path<'a>(
    value: Option<&'a Value>,
    name: &str,
    allow_empty: bool,
) -> Result<&'a str, WorkspaceContractError> {
    let value = value
        .and_then(Value::as_str)
        .ok_or_else(|| WorkspaceContractError(format!("{name} must be a string")))?;
    if (!allow_empty && value.is_empty()) || value.starts_with('/') || value.contains('\\') {
        return fail(format!("{name} must be a normalized relative path"));
    }
    if !(allow_empty && value.is_empty())
        && value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return fail(format!(
            "{name} must not traverse or contain empty segments"
        ));
    }
    Ok(value)
}
fn array<'a>(
    value: Option<&'a Value>,
    name: &str,
) -> Result<&'a Vec<Value>, WorkspaceContractError> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| WorkspaceContractError(format!("{name} must be an array")))
}
fn fail<T>(message: impl Into<String>) -> Result<T, WorkspaceContractError> {
    Err(WorkspaceContractError(message.into()))
}

#[cfg(test)]
mod tests;
