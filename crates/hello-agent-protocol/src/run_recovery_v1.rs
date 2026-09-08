use serde_json::{Map, Value};
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display};

pub const G5_SCHEMA_VERSION_V1: u64 = 1;
pub const G5_ROLE_IDS_V1: &[&str] = &["planning", "coding", "review"];
pub const G5_RUNTIMES_V1: &[&str] = &["codex", "claude-code"];
pub const G5_PERMISSIONS_V1: &[&str] = &["readOnly", "workspaceWrite"];
pub const G5_CHAT_STATES_V1: &[&str] = &[
    "draft",
    "preparing",
    "running",
    "waitingForUser",
    "reviewReady",
    "applying",
    "stopping",
    "completed",
    "needsAttention",
    "recoveryRequired",
    "canceled",
    "discarded",
];
pub const G5_RUN_START_CHAT_STATES_V1: &[&str] =
    &["draft", "running", "waitingForUser", "needsAttention"];
pub const G5_RUN_STATES_V1: &[&str] = &[
    "preparing",
    "running",
    "cancelling",
    "succeeded",
    "failed",
    "canceled",
    "recoveryRequired",
];
pub const G5_RUN_STAGES_V1: &[&str] = &["planning", "implementing", "verifying"];
pub const G5_FIXTURE_SCENARIOS_V1: &[&str] = &[
    "planning-success",
    "coding-success",
    "review-success",
    "cancel-clean",
    "cancel-uncertain",
];
pub const G5_PAGE_MAX_ITEMS_V1: usize = 100;
pub const G5_EVENT_PAGE_MAX_ITEMS_V1: usize = 50;
pub const G5_MODEL_SELECTOR_MAX_BYTES_V1: usize = 256;
pub const G5_REASONING_EFFORT_MAX_BYTES_V1: usize = 64;
pub const G5_GOAL_MAX_BYTES_V1: usize = 16 * 1024;
pub const G5_PUBLIC_MESSAGE_MAX_BYTES_V1: usize = 64 * 1024;
pub const G5_ID_MAX_BYTES_V1: usize = 512;
pub const G5_EXECUTION_PLATFORMS_V1: &[&str] = &["darwin", "win32"];

pub const G5_CAPABILITIES_V1: &[&str] = &[
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
];

pub const G5D_PRODUCT_CAPABILITIES_V1: &[&str] = &[
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
    "storage.status",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct G5ContractError {
    detail: String,
}

impl G5ContractError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for G5ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for G5ContractError {}

pub fn validate_g5_payload_v1(validator: &str, value: &Value) -> Result<(), G5ContractError> {
    match validator {
        "roleProfile" => validate_role_profile(value).map(|_| ()),
        "projectRoleSettings" => validate_project_role_settings(value).map(|_| ()),
        "projectRoleSettingsUpdate" => validate_project_role_settings_update(value),
        "prepareChatWorkspaceRequest" => validate_prepare_chat_workspace_request(value).map(|_| ()),
        "preparedChatWorkspace" => validate_prepared_chat_workspace(value).map(|_| ()),
        "prepareChatWorkspace" => validate_prepare_chat_workspace(value),
        "abandonPreparedWorkspaceRequest" => validate_abandon_prepared_request(value).map(|_| ()),
        "abandonPreparedWorkspace" => validate_abandon_prepared_workspace(value),
        "chatCreateRequest" => validate_chat_create_request(value).map(|_| ()),
        "chatRecord" => validate_chat_record(value).map(|_| ()),
        "chatTransition" => validate_chat_transition(value),
        "teamMessage" => validate_team_message(value).map(|_| ()),
        "runRecord" => validate_run_record(value).map(|_| ()),
        "runTransition" => validate_run_transition(value),
        "chatCreation" => validate_chat_creation(value),
        "chatDetailPage" => validate_chat_detail_page(value),
        "chatListPage" => validate_chat_list_page(value),
        "chatListRequest" => validate_chat_list_request(value),
        "chatGetRequest" => validate_chat_get_request(value),
        "pageRequest" => validate_page_request(value, G5_PAGE_MAX_ITEMS_V1),
        "fixtureRunStart" => validate_fixture_run_start(value).map(|_| ()),
        "runCancelRequest" => validate_run_cancel_request(value),
        "writerStart" => validate_writer_start(value),
        "runStart" => validate_run_start(value),
        "cancelTerminationRecovery" => validate_cancel_termination_recovery(value),
        "recoveryRecord" => validate_recovery_record(value).map(|_| ()),
        "recoveryPage" => validate_recovery_page(value),
        "recoveryMarkResolved" => validate_recovery_mark_resolved(value).map(|_| ()),
        "recoveryResolution" => validate_recovery_resolution(value),
        "g5Ready" => validate_g5_ready(value),
        other => fail(format!("unknown G5 payload validator: {other}")),
    }
}

pub fn assert_g5_capabilities_v1() -> Result<(), G5ContractError> {
    if G5_CAPABILITIES_V1
        .iter()
        .any(|capability| !super::METHODS_V1.contains(capability))
    {
        return fail("G5 capability is not reserved by protocol v1");
    }
    if G5_CAPABILITIES_V1.contains(&"run.startAgentSmoke") {
        return fail("agent smoke must remain unadvertised");
    }
    let unique: HashSet<_> = G5_CAPABILITIES_V1.iter().collect();
    if unique.len() != G5_CAPABILITIES_V1.len() {
        return fail("G5 capabilities must be unique");
    }
    Ok(())
}

pub fn assert_g5d_product_capabilities_v1() -> Result<(), G5ContractError> {
    if G5D_PRODUCT_CAPABILITIES_V1[..G5_CAPABILITIES_V1.len()] != *G5_CAPABILITIES_V1 {
        return fail("G5D product capabilities must preserve the exact G5 prefix");
    }
    if G5D_PRODUCT_CAPABILITIES_V1.last() != Some(&"storage.status") {
        return fail("G5D product capabilities must add storage.status at the tail");
    }
    if G5D_PRODUCT_CAPABILITIES_V1
        .iter()
        .any(|capability| !super::METHODS_V1.contains(capability))
    {
        return fail("G5D product capability is not reserved by protocol v1");
    }
    if G5D_PRODUCT_CAPABILITIES_V1.contains(&"run.startAgentSmoke") {
        return fail("agent smoke must remain unadvertised");
    }
    let unique: HashSet<_> = G5D_PRODUCT_CAPABILITIES_V1.iter().collect();
    if unique.len() != G5D_PRODUCT_CAPABILITIES_V1.len() {
        return fail("G5D product capabilities must be unique");
    }
    Ok(())
}

pub fn validate_g5_ready_payload_v1(value: &Value) -> Result<(), G5ContractError> {
    let ready: super::ReadyPayloadV1 = serde_json::from_value(value.clone())
        .map_err(|error| G5ContractError::new(format!("invalid G5 ready payload: {error}")))?;
    let event = super::EventV1 {
        protocol_version: super::PROTOCOL_VERSION,
        kind: "event".to_string(),
        sidecar_instance_id: ready.sidecar_instance_id.clone(),
        stream_id: "system".to_string(),
        seq: 1,
        event: "system.ready".to_string(),
        chat_id: None,
        task_id: None,
        run_id: None,
        payload: value.clone(),
    };
    super::ready_payload(&event)
        .map_err(|error| G5ContractError::new(format!("invalid G5 ready identity: {error}")))?;
    if ready.protocol_version != super::PROTOCOL_VERSION
        || ready.sidecar_schema_version != super::SIDECAR_SCHEMA_VERSION
        || ready.app_schema_version != super::APP_SCHEMA_VERSION
    {
        return fail("G5 ready identity versions do not match v1");
    }
    if !ready
        .capabilities
        .iter()
        .map(String::as_str)
        .eq(G5_CAPABILITIES_V1.iter().copied())
    {
        return fail("G5 ready capabilities do not match the exact supported method set");
    }
    Ok(())
}

pub fn validate_g5d_product_ready_payload_v1(value: &Value) -> Result<(), G5ContractError> {
    assert_g5d_product_capabilities_v1()?;
    let ready: super::ReadyPayloadV1 = serde_json::from_value(value.clone()).map_err(|error| {
        G5ContractError::new(format!("invalid G5D product ready payload: {error}"))
    })?;
    let event = super::EventV1 {
        protocol_version: super::PROTOCOL_VERSION,
        kind: "event".to_string(),
        sidecar_instance_id: ready.sidecar_instance_id.clone(),
        stream_id: "system".to_string(),
        seq: 1,
        event: "system.ready".to_string(),
        chat_id: None,
        task_id: None,
        run_id: None,
        payload: value.clone(),
    };
    super::ready_payload(&event).map_err(|error| {
        G5ContractError::new(format!("invalid G5D product ready identity: {error}"))
    })?;
    if ready.protocol_version != super::PROTOCOL_VERSION
        || ready.sidecar_schema_version != super::SIDECAR_SCHEMA_VERSION
        || ready.app_schema_version != super::APP_SCHEMA_VERSION
    {
        return fail("G5D product ready identity versions do not match v1");
    }
    if !ready
        .capabilities
        .iter()
        .map(String::as_str)
        .eq(G5D_PRODUCT_CAPABILITIES_V1.iter().copied())
    {
        return fail("G5D product ready capabilities do not match the exact supported method set");
    }
    Ok(())
}

fn validate_g5_ready(value: &Value) -> Result<(), G5ContractError> {
    validate_g5_ready_payload_v1(value)
}

fn validate_role_profile(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "role profile")?;
    exact_keys(
        object,
        &[
            "roleId",
            "runtime",
            "modelSelector",
            "reasoningEffort",
            "permission",
        ],
    )?;
    let role = one_of(object.get("roleId"), G5_ROLE_IDS_V1, "roleId")?;
    one_of(object.get("runtime"), G5_RUNTIMES_V1, "runtime")?;
    bounded_text(
        object.get("modelSelector"),
        "modelSelector",
        G5_MODEL_SELECTOR_MAX_BYTES_V1,
    )?;
    if !object.get("reasoningEffort").is_some_and(Value::is_null) {
        bounded_text(
            object.get("reasoningEffort"),
            "reasoningEffort",
            G5_REASONING_EFFORT_MAX_BYTES_V1,
        )?;
    }
    let permission = one_of(object.get("permission"), G5_PERMISSIONS_V1, "permission")?;
    let expected = if role == "coding" {
        "workspaceWrite"
    } else {
        "readOnly"
    };
    if permission != expected {
        return fail(format!("{role} permission must be {expected}"));
    }
    Ok(object)
}

fn validate_project_role_settings(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "project role settings")?;
    exact_keys(
        object,
        &[
            "schemaVersion",
            "projectId",
            "revision",
            "roles",
            "updatedAt",
        ],
    )?;
    schema(object.get("schemaVersion"))?;
    id(object.get("projectId"), "projectId")?;
    positive(object.get("revision"), "settings revision")?;
    validate_role_set(object.get("roles"), "roles")?;
    timestamp(object.get("updatedAt"), "updatedAt")?;
    Ok(object)
}

fn validate_project_role_settings_update(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "project role settings update")?;
    exact_keys(object, &["projectId", "expectedRevision", "roles"])?;
    id(object.get("projectId"), "projectId")?;
    positive(object.get("expectedRevision"), "expectedRevision")?;
    validate_role_set(object.get("roles"), "roles")
}

fn validate_prepare_chat_workspace_request(
    value: &Value,
) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "prepare chat workspace request")?;
    exact_keys(object, &["projectId"])?;
    id(object.get("projectId"), "projectId")?;
    Ok(object)
}

fn validate_prepared_chat_workspace(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "prepared chat workspace")?;
    exact_keys(
        object,
        &[
            "schemaVersion",
            "preparationId",
            "manifest",
            "bindingState",
            "boundChatId",
            "createdAt",
            "updatedAt",
        ],
    )?;
    schema(object.get("schemaVersion"))?;
    id(object.get("preparationId"), "preparationId")?;
    let manifest_value = required(object, "manifest")?;
    super::project_v1::validate_project_payload_v1("worktreeManifest", manifest_value)
        .map_err(|error| G5ContractError::new(format!("invalid prepared manifest: {error}")))?;
    let manifest = self::object(manifest_value, "prepared manifest")?;
    let state = one_of(
        object.get("bindingState"),
        &["unbound", "bound", "abandoned"],
        "prepared binding state",
    )?;
    if state == "bound" {
        id(object.get("boundChatId"), "boundChatId")?;
    } else if !object.get("boundChatId").is_some_and(Value::is_null) {
        return fail(format!("{state} preparation cannot have boundChatId"));
    }
    let lifecycle = string(manifest.get("lifecycle"), "prepared manifest lifecycle")?;
    validate_prepared_workspace_state_fields_v1(
        state,
        lifecycle,
        string(object.get("createdAt"), "createdAt")?,
        string(object.get("updatedAt"), "updatedAt")?,
    )?;
    Ok(object)
}

/// State/time relation only; manifest, exact keys, schema and identity are
/// deliberately separate checks for both whole-value and fragment consumers.
pub fn validate_prepared_workspace_state_fields_v1(
    state: &str,
    lifecycle: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<(), G5ContractError> {
    if (state == "abandoned" && lifecycle != "abandoned")
        || (state != "abandoned" && lifecycle != "active")
    {
        return fail("prepared binding state does not match manifest lifecycle");
    }
    timestamp_text(created_at, "createdAt")?;
    timestamp_text(updated_at, "updatedAt")?;
    if updated_at < created_at {
        return fail("prepared workspace updatedAt must be monotonic");
    }
    Ok(())
}

fn validate_prepare_chat_workspace(value: &Value) -> Result<(), G5ContractError> {
    let transcript = object(value, "prepare chat workspace transcript")?;
    exact_keys(
        transcript,
        &[
            "request",
            "existingPreparationPages",
            "candidate",
            "persisted",
            "transactionCommitted",
            "outcome",
        ],
    )?;
    let request = validate_prepare_chat_workspace_request(required(transcript, "request")?)?;
    let existing =
        validate_prepared_workspace_pages(required(transcript, "existingPreparationPages")?)?;
    let candidate = validate_prepared_chat_workspace(required(transcript, "candidate")?)?;
    let candidate_manifest = prepared_manifest(candidate)?;
    if candidate.get("bindingState").and_then(Value::as_str) != Some("unbound")
        || candidate_manifest.get("projectId") != request.get("projectId")
    {
        return fail(
            "prepare candidate must be a new unbound active snapshot for the requested project",
        );
    }
    let collision = existing.iter().any(|prepared| {
        let manifest = prepared_manifest(prepared).expect("validated prepared manifest");
        prepared.get("preparationId") == candidate.get("preparationId")
            || manifest.get("worktreeId") == candidate_manifest.get("worktreeId")
            || manifest.get("workspaceIdentity") == candidate_manifest.get("workspaceIdentity")
    });
    let expected = if collision {
        "identityCollision"
    } else {
        "prepared"
    };
    let outcome = one_of(
        transcript.get("outcome"),
        &["prepared", "identityCollision"],
        "prepare outcome",
    )?;
    if outcome != expected {
        return fail(format!("prepare outcome must be {expected}"));
    }
    if outcome == "prepared" {
        validate_prepared_chat_workspace(required(transcript, "persisted")?)?;
        if transcript.get("persisted") != transcript.get("candidate")
            || transcript.get("transactionCommitted") != Some(&Value::Bool(true))
        {
            return fail("successful preparation must atomically persist the exact new snapshot");
        }
    } else if !transcript.get("persisted").is_some_and(Value::is_null)
        || transcript.get("transactionCommitted") != Some(&Value::Bool(false))
    {
        return fail("identity collision cannot persist or publish a preparation");
    }
    Ok(())
}

fn validate_abandon_prepared_request(
    value: &Value,
) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "abandon prepared workspace request")?;
    exact_keys(
        object,
        &[
            "preparationId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
        ],
    )?;
    for field in [
        "preparationId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        id(object.get(field), field)?;
    }
    Ok(object)
}

fn validate_abandon_prepared_result(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "abandon prepared workspace result")?;
    exact_keys(
        object,
        &[
            "preparationId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "bindingState",
            "physicallyDeleted",
        ],
    )?;
    for field in [
        "preparationId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        id(object.get(field), field)?;
    }
    if object.get("bindingState").and_then(Value::as_str) != Some("abandoned")
        || object.get("physicallyDeleted") != Some(&Value::Bool(false))
    {
        return fail("prepared workspace abandon must preserve evidence without physical deletion");
    }
    Ok(object)
}

fn validate_abandon_prepared_workspace(value: &Value) -> Result<(), G5ContractError> {
    let transcript = object(value, "abandon prepared workspace transcript")?;
    exact_keys(
        transcript,
        &[
            "request",
            "persistedBefore",
            "unrelatedBefore",
            "persistedAfter",
            "unrelatedAfter",
            "result",
            "transactionCommitted",
            "outcome",
        ],
    )?;
    let request = validate_abandon_prepared_request(required(transcript, "request")?)?;
    let before = if transcript
        .get("persistedBefore")
        .is_some_and(Value::is_null)
    {
        None
    } else {
        Some(validate_prepared_chat_workspace(required(
            transcript,
            "persistedBefore",
        )?)?)
    };
    let unrelated_before = validate_prepared_workspace_array(
        required(transcript, "unrelatedBefore")?,
        "unrelated preparations before",
    )?;
    let unrelated_after = validate_prepared_workspace_array(
        required(transcript, "unrelatedAfter")?,
        "unrelated preparations after",
    )?;
    if transcript.get("unrelatedBefore") != transcript.get("unrelatedAfter") {
        return fail("abandon prepared cannot mutate another workspace");
    }
    if unrelated_before.iter().any(|prepared| {
        let manifest = prepared_manifest(prepared).expect("validated prepared manifest");
        prepared.get("preparationId") == request.get("preparationId")
            || manifest.get("worktreeId") == request.get("worktreeId")
            || manifest.get("workspaceIdentity") == request.get("workspaceIdentity")
    }) || unrelated_after.len() != unrelated_before.len()
    {
        return fail("target preparation cannot appear in unrelated evidence");
    }
    let mismatch = before.is_some_and(|prepared| {
        let manifest = prepared_manifest(prepared).expect("validated prepared manifest");
        prepared.get("preparationId") != request.get("preparationId")
            || manifest.get("projectId") != request.get("projectId")
            || manifest.get("worktreeId") != request.get("worktreeId")
            || manifest.get("workspaceIdentity") != request.get("workspaceIdentity")
    });
    let expected = match before {
        None => "unknownPreparation",
        Some(_) if mismatch => "identityMismatch",
        Some(prepared) => match prepared.get("bindingState").and_then(Value::as_str) {
            Some("bound") => "alreadyBound",
            Some("abandoned") => "alreadyAbandoned",
            Some("unbound") => "abandoned",
            _ => return fail("unknown prepared binding state"),
        },
    };
    let outcome = one_of(
        transcript.get("outcome"),
        &[
            "abandoned",
            "unknownPreparation",
            "identityMismatch",
            "alreadyBound",
            "alreadyAbandoned",
        ],
        "abandon prepared outcome",
    )?;
    if outcome != expected {
        return fail(format!("abandon prepared outcome must be {expected}"));
    }
    if outcome == "abandoned" {
        let before = before.expect("abandon outcome requires preparation");
        let after = validate_prepared_chat_workspace(required(transcript, "persistedAfter")?)?;
        let result = validate_abandon_prepared_result(required(transcript, "result")?)?;
        same_prepared_identity(before, after, "abandon transition")?;
        same_prepared_identity(before, result, "abandon result")?;
        if after.get("bindingState").and_then(Value::as_str) != Some("abandoned")
            || !same_manifest_except_lifecycle(
                prepared_manifest(before)?,
                prepared_manifest(after)?,
            )
            || after.get("createdAt") != before.get("createdAt")
            || string(after.get("updatedAt"), "after updatedAt")?
                < string(before.get("updatedAt"), "before updatedAt")?
            || transcript.get("transactionCommitted") != Some(&Value::Bool(true))
        {
            return fail("abandon must atomically terminally mark only the unbound preparation");
        }
    } else if transcript.get("persistedAfter") != transcript.get("persistedBefore")
        || !transcript.get("result").is_some_and(Value::is_null)
        || transcript.get("transactionCommitted") != Some(&Value::Bool(false))
    {
        return fail("failed or duplicate abandon must be a fail-closed no-op");
    }
    Ok(())
}

fn validate_chat_create_request(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "chat create request")?;
    exact_keys(
        object,
        &[
            "preparationId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "goal",
            "expectedSettingsRevision",
        ],
    )?;
    for field in [
        "preparationId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        id(object.get(field), field)?;
    }
    bounded_text(object.get("goal"), "goal", G5_GOAL_MAX_BYTES_V1)?;
    positive(
        object.get("expectedSettingsRevision"),
        "expectedSettingsRevision",
    )?;
    Ok(object)
}

fn validate_chat_record(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "chat record")?;
    exact_keys(
        object,
        &[
            "schemaVersion",
            "chatId",
            "taskId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "goal",
            "settingsRevision",
            "roleSnapshots",
            "state",
            "createdAt",
            "updatedAt",
        ],
    )?;
    schema(object.get("schemaVersion"))?;
    for field in [
        "chatId",
        "taskId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        id(object.get(field), field)?;
    }
    bounded_text(object.get("goal"), "goal", G5_GOAL_MAX_BYTES_V1)?;
    positive(object.get("settingsRevision"), "settingsRevision")?;
    validate_role_set(object.get("roleSnapshots"), "roleSnapshots")?;
    one_of(object.get("state"), G5_CHAT_STATES_V1, "chat state")?;
    timestamp(object.get("createdAt"), "createdAt")?;
    timestamp(object.get("updatedAt"), "updatedAt")?;
    Ok(object)
}

fn validate_chat_transition(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "chat transition")?;
    exact_keys(object, &["before", "after"])?;
    let before = validate_chat_record(required(object, "before")?)?;
    let after = validate_chat_record(required(object, "after")?)?;
    for field in [
        "schemaVersion",
        "chatId",
        "taskId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
        "goal",
        "settingsRevision",
        "createdAt",
    ] {
        if before.get(field) != after.get(field) {
            return fail(format!("chat transition changed {field}"));
        }
    }
    if before.get("roleSnapshots") != after.get("roleSnapshots") {
        return fail("chat transition changed roleSnapshots");
    }
    let before_state = string(before.get("state"), "before chat state")?;
    let after_state = string(after.get("state"), "after chat state")?;
    let allowed: &[&str] = match before_state {
        "draft" => &["preparing"],
        "preparing" => &["running", "stopping", "needsAttention", "recoveryRequired"],
        "running" => &[
            "waitingForUser",
            "reviewReady",
            "stopping",
            "needsAttention",
            "recoveryRequired",
        ],
        "waitingForUser" => &["running", "stopping", "needsAttention", "recoveryRequired"],
        "reviewReady" => &["applying", "discarded", "needsAttention"],
        "applying" => &["completed", "needsAttention", "recoveryRequired"],
        "stopping" => &["canceled", "recoveryRequired"],
        "needsAttention" => &["preparing", "discarded", "recoveryRequired"],
        "completed" | "canceled" | "discarded" | "recoveryRequired" => &[],
        _ => return fail("unknown before chat state"),
    };
    if !allowed.contains(&after_state) {
        return fail(format!(
            "invalid chat transition {before_state} -> {after_state}"
        ));
    }
    let before_updated = string(before.get("updatedAt"), "before updatedAt")?;
    let after_updated = string(after.get("updatedAt"), "after updatedAt")?;
    if after_updated < before_updated {
        return fail("chat transition updatedAt must be monotonic");
    }
    Ok(())
}

fn validate_team_message(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "team message")?;
    exact_keys(
        object,
        &[
            "schemaVersion",
            "messageId",
            "chatId",
            "taskId",
            "worktreeId",
            "eventSequence",
            "kind",
            "roleId",
            "runId",
            "text",
            "createdAt",
        ],
    )?;
    schema(object.get("schemaVersion"))?;
    for field in ["messageId", "chatId", "taskId", "worktreeId"] {
        id(object.get(field), field)?;
    }
    positive(object.get("eventSequence"), "eventSequence")?;
    let kind = one_of(
        object.get("kind"),
        &["user", "role", "system", "status"],
        "message kind",
    )?;
    bounded_text(
        object.get("text"),
        "public message text",
        G5_PUBLIC_MESSAGE_MAX_BYTES_V1,
    )?;
    timestamp(object.get("createdAt"), "createdAt")?;
    if kind == "role" {
        one_of(object.get("roleId"), G5_ROLE_IDS_V1, "roleId")?;
        id(object.get("runId"), "runId")?;
    } else {
        if !object.get("roleId").is_some_and(Value::is_null) {
            return fail("non-role message roleId must be null");
        }
        if kind == "user" && !object.get("runId").is_some_and(Value::is_null) {
            return fail("user message runId must be null");
        }
        if !object.get("runId").is_some_and(Value::is_null) {
            id(object.get("runId"), "runId")?;
        }
    }
    Ok(object)
}

fn validate_run_record(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "run record")?;
    exact_keys(
        object,
        &[
            "schemaVersion",
            "runId",
            "chatId",
            "taskId",
            "worktreeId",
            "workspaceIdentity",
            "selectedProfile",
            "fixtureScenario",
            "state",
            "stage",
            "terminationEvidence",
            "createdAt",
            "updatedAt",
        ],
    )?;
    schema(object.get("schemaVersion"))?;
    for field in [
        "runId",
        "chatId",
        "taskId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        id(object.get(field), field)?;
    }
    let profile = validate_role_profile(required(object, "selectedProfile")?)?;
    one_of(
        object.get("fixtureScenario"),
        G5_FIXTURE_SCENARIOS_V1,
        "fixture scenario",
    )?;
    let state = one_of(object.get("state"), G5_RUN_STATES_V1, "run state")?;
    let stage = one_of(object.get("stage"), G5_RUN_STAGES_V1, "run stage")?;
    let expected_stage = match profile.get("roleId").and_then(Value::as_str) {
        Some("planning") => "planning",
        Some("coding") => "implementing",
        Some("review") => "verifying",
        _ => return fail("unknown selected role snapshot"),
    };
    if stage != expected_stage {
        return fail("run stage does not match selected role snapshot");
    }
    if ["preparing", "running", "cancelling"].contains(&state) {
        if !object
            .get("terminationEvidence")
            .is_some_and(Value::is_null)
        {
            return fail("active run cannot have termination evidence");
        }
    } else {
        validate_termination_evidence(required(object, "terminationEvidence")?, state)?;
    }
    timestamp(object.get("createdAt"), "createdAt")?;
    timestamp(object.get("updatedAt"), "updatedAt")?;
    Ok(object)
}

fn validate_run_transition(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "run transition")?;
    exact_keys(object, &["before", "after"])?;
    let before = validate_run_record(required(object, "before")?)?;
    let after = validate_run_record(required(object, "after")?)?;
    for field in [
        "runId",
        "chatId",
        "taskId",
        "worktreeId",
        "workspaceIdentity",
        "fixtureScenario",
    ] {
        if before.get(field) != after.get(field) {
            return fail(format!("run transition changed {field}"));
        }
    }
    if before.get("selectedProfile") != after.get("selectedProfile") {
        return fail("run transition changed selectedProfile");
    }
    let from = string(before.get("state"), "before state")?;
    let to = string(after.get("state"), "after state")?;
    let allowed: &[&str] = match from {
        "preparing" => &["running", "cancelling", "failed", "recoveryRequired"],
        "running" => &["cancelling", "succeeded", "failed", "recoveryRequired"],
        "cancelling" => &["canceled", "recoveryRequired"],
        _ => &[],
    };
    if !allowed.contains(&to) {
        return fail(format!("invalid run transition {from} -> {to}"));
    }
    Ok(())
}

fn validate_chat_creation(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "chat creation transcript")?;
    exact_keys(
        object,
        &[
            "request",
            "currentSettings",
            "preparedBefore",
            "existingGlobalChatBindingPages",
            "preparedAfter",
            "createdChat",
            "createdUserMessage",
            "transactionCommitted",
            "outcome",
        ],
    )?;
    let request = validate_chat_create_request(required(object, "request")?)?;
    let settings = validate_project_role_settings(required(object, "currentSettings")?)?;
    let prepared = if object.get("preparedBefore").is_some_and(Value::is_null) {
        None
    } else {
        Some(validate_prepared_chat_workspace(required(
            object,
            "preparedBefore",
        )?)?)
    };
    let bindings =
        validate_chat_binding_pages(required(object, "existingGlobalChatBindingPages")?)?;
    let created_chat = if object.get("createdChat").is_some_and(Value::is_null) {
        None
    } else {
        Some(validate_chat_record(required(object, "createdChat")?)?)
    };
    if let Some(chat) = created_chat {
        if chat.get("state").and_then(Value::as_str) != Some("draft")
            || chat.get("projectId") != request.get("projectId")
            || chat.get("worktreeId") != request.get("worktreeId")
            || chat.get("workspaceIdentity") != request.get("workspaceIdentity")
        {
            return fail("created chat identity mismatch");
        }
        if chat.get("goal") != request.get("goal") {
            return fail("created chat goal mismatch");
        }
        if chat.get("settingsRevision") != settings.get("revision") {
            return fail("created chat settings revision drift");
        }
        if chat.get("roleSnapshots") != settings.get("roles") {
            return fail("created chat did not snapshot exact roles");
        }
    }
    let chat_id_conflict = created_chat.is_some_and(|chat| {
        bindings
            .iter()
            .any(|binding| binding.get("chatId") == chat.get("chatId"))
    });
    let binding_conflict = bindings.iter().any(|binding| {
        binding.get("worktreeId") == request.get("worktreeId")
            || binding.get("workspaceIdentity") == request.get("workspaceIdentity")
            || created_chat.is_some_and(|chat| binding.get("chatId") == chat.get("chatId"))
    });
    let preparation_mismatch = prepared.is_some_and(|prepared| {
        let manifest = prepared_manifest(prepared).expect("validated prepared manifest");
        prepared.get("preparationId") != request.get("preparationId")
            || manifest.get("projectId") != request.get("projectId")
            || manifest.get("worktreeId") != request.get("worktreeId")
            || manifest.get("workspaceIdentity") != request.get("workspaceIdentity")
    });
    let expected_outcome = if prepared.is_none() {
        "preparationUnknown"
    } else if request.get("expectedSettingsRevision") != settings.get("revision") {
        "settingsDrift"
    } else if prepared
        .and_then(|value| value.get("bindingState"))
        .and_then(Value::as_str)
        != Some("unbound")
    {
        "preparationNotUnbound"
    } else if preparation_mismatch {
        "preparationMismatch"
    } else if request.get("projectId") != settings.get("projectId") {
        "identityMismatch"
    } else if binding_conflict {
        "bindingConflict"
    } else {
        "created"
    };
    let outcome = one_of(
        object.get("outcome"),
        &[
            "created",
            "preparationUnknown",
            "settingsDrift",
            "preparationNotUnbound",
            "preparationMismatch",
            "identityMismatch",
            "bindingConflict",
        ],
        "chat create outcome",
    )?;
    if outcome != expected_outcome {
        return fail(format!("chat create outcome must be {expected_outcome}"));
    }
    if outcome == "created" {
        let chat = created_chat
            .ok_or_else(|| G5ContractError::new("created outcome requires a chat candidate"))?;
        let message = validate_team_message(required(object, "createdUserMessage")?)?;
        let prepared_after = validate_prepared_chat_workspace(required(object, "preparedAfter")?)?;
        if request.get("expectedSettingsRevision") != settings.get("revision")
            || chat.get("settingsRevision") != settings.get("revision")
        {
            return fail("created chat settings revision drift");
        }
        if message.get("chatId") != chat.get("chatId")
            || message.get("taskId") != chat.get("taskId")
            || message.get("worktreeId") != chat.get("worktreeId")
            || message.get("kind").and_then(Value::as_str) != Some("user")
            || message.get("eventSequence").and_then(Value::as_u64) != Some(1)
            || message.get("text") != request.get("goal")
            || !message.get("roleId").is_some_and(Value::is_null)
            || !message.get("runId").is_some_and(Value::is_null)
        {
            return fail("chat creation must atomically snapshot the initial user message");
        }
        let prepared = prepared.expect("created chat requires preparation");
        same_prepared_identity(prepared, prepared_after, "chat preparation bind")?;
        if prepared.get("manifest") != prepared_after.get("manifest")
            || prepared_after.get("bindingState").and_then(Value::as_str) != Some("bound")
            || prepared_after.get("boundChatId") != chat.get("chatId")
            || prepared_after.get("createdAt") != prepared.get("createdAt")
            || string(prepared_after.get("updatedAt"), "prepared after updatedAt")?
                < string(prepared.get("updatedAt"), "prepared before updatedAt")?
            || object.get("transactionCommitted") != Some(&Value::Bool(true))
        {
            return fail("chat creation must atomically bind the exact prepared snapshot");
        }
    } else {
        let rejected_chat_id_candidate = outcome == "bindingConflict" && chat_id_conflict;
        if (!rejected_chat_id_candidate && !object.get("createdChat").is_some_and(Value::is_null))
            || !object.get("createdUserMessage").is_some_and(Value::is_null)
            || object.get("transactionCommitted") != Some(&Value::Bool(false))
        {
            return fail("failed chat creation cannot persist a chat or user message");
        }
        if object.get("preparedAfter") != object.get("preparedBefore") {
            return fail("failed chat creation must leave the exact preparation cleanup-eligible");
        }
    }
    Ok(())
}

fn validate_chat_binding_pages(value: &Value) -> Result<Vec<&Map<String, Value>>, G5ContractError> {
    let pages = array(
        Some(value),
        "global chat binding pages",
        G5_PAGE_MAX_ITEMS_V1,
    )?;
    if pages.is_empty() {
        return fail("global chat binding scan requires at least one complete page");
    }
    let mut bindings = Vec::new();
    let mut expected_cursor: Option<&str> = None;
    for (index, page) in pages.iter().enumerate() {
        let page = object(page, "chat binding page")?;
        exact_keys(page, &["cursor", "items", "nextCursor"])?;
        let cursor = nullable_id(page.get("cursor"), "cursor")?;
        if cursor != expected_cursor {
            return fail("chat binding pages must form one cursor chain");
        }
        for binding in array(
            page.get("items"),
            "chat binding items",
            G5_PAGE_MAX_ITEMS_V1,
        )? {
            let binding = object(binding, "chat binding")?;
            exact_keys(
                binding,
                &["chatId", "projectId", "worktreeId", "workspaceIdentity"],
            )?;
            for field in ["chatId", "projectId", "worktreeId", "workspaceIdentity"] {
                id(binding.get(field), field)?;
            }
            bindings.push(binding);
        }
        expected_cursor = nullable_id(page.get("nextCursor"), "nextCursor")?;
        if index < pages.len() - 1 && expected_cursor.is_none() {
            return fail("chat binding scan ended before its last page");
        }
    }
    if expected_cursor.is_some() {
        return fail("global chat binding scan must end with nextCursor null");
    }
    let mut chats = HashSet::new();
    let mut worktrees = HashSet::new();
    let mut workspaces = HashSet::new();
    for binding in &bindings {
        let chat = string(binding.get("chatId"), "chatId")?;
        let worktree = string(binding.get("worktreeId"), "worktreeId")?;
        let workspace = string(binding.get("workspaceIdentity"), "workspaceIdentity")?;
        if !chats.insert(chat) || !worktrees.insert(worktree) || !workspaces.insert(workspace) {
            return fail("global chat/worktree/workspace binding must be 1:1 across pages");
        }
    }
    Ok(bindings)
}

fn validate_prepared_workspace_pages(
    value: &Value,
) -> Result<Vec<&Map<String, Value>>, G5ContractError> {
    let pages = array(
        Some(value),
        "prepared workspace pages",
        G5_PAGE_MAX_ITEMS_V1,
    )?;
    if pages.is_empty() {
        return fail("prepared workspace scan requires at least one complete page");
    }
    let mut prepared = Vec::new();
    let mut expected_cursor: Option<&str> = None;
    for (index, page) in pages.iter().enumerate() {
        let page = object(page, "prepared workspace page")?;
        exact_keys(page, &["cursor", "items", "nextCursor"])?;
        let cursor = nullable_id(page.get("cursor"), "prepared cursor")?;
        if cursor != expected_cursor {
            return fail("prepared workspace pages must form one cursor chain");
        }
        for item in array(
            page.get("items"),
            "prepared workspace items",
            G5_PAGE_MAX_ITEMS_V1,
        )? {
            prepared.push(validate_prepared_chat_workspace(item)?);
        }
        expected_cursor = nullable_id(page.get("nextCursor"), "prepared nextCursor")?;
        if index < pages.len() - 1 && expected_cursor.is_none() {
            return fail("prepared workspace scan ended before its last page");
        }
    }
    if expected_cursor.is_some() {
        return fail("prepared workspace scan must end with nextCursor null");
    }
    let mut preparation_ids = HashSet::new();
    let mut worktree_ids = HashSet::new();
    let mut workspace_identities = HashSet::new();
    for item in &prepared {
        let manifest = prepared_manifest(item)?;
        if !preparation_ids.insert(string(item.get("preparationId"), "preparationId")?)
            || !worktree_ids.insert(string(manifest.get("worktreeId"), "worktreeId")?)
            || !workspace_identities.insert(string(
                manifest.get("workspaceIdentity"),
                "workspaceIdentity",
            )?)
        {
            return fail("prepared workspace identities must remain globally unique");
        }
    }
    Ok(prepared)
}

fn validate_prepared_workspace_array<'a>(
    value: &'a Value,
    name: &str,
) -> Result<Vec<&'a Map<String, Value>>, G5ContractError> {
    array(Some(value), name, G5_PAGE_MAX_ITEMS_V1)?
        .iter()
        .map(validate_prepared_chat_workspace)
        .collect()
}

fn prepared_manifest<'a>(
    prepared: &'a Map<String, Value>,
) -> Result<&'a Map<String, Value>, G5ContractError> {
    object(required(prepared, "manifest")?, "prepared manifest")
}

fn same_prepared_identity(
    expected: &Map<String, Value>,
    actual: &Map<String, Value>,
    name: &str,
) -> Result<(), G5ContractError> {
    let expected_manifest = prepared_manifest(expected)?;
    let actual_manifest = actual.get("manifest").map_or(Ok(actual), |manifest| {
        object(manifest, "actual prepared manifest")
    })?;
    for field in ["preparationId"] {
        if expected.get(field) != actual.get(field) {
            return fail(format!("{name} changed preparation identity"));
        }
    }
    for field in ["projectId", "worktreeId", "workspaceIdentity"] {
        if expected_manifest.get(field) != actual_manifest.get(field) {
            return fail(format!(
                "{name} changed project/worktree/workspace identity"
            ));
        }
    }
    Ok(())
}

fn same_manifest_except_lifecycle(before: &Map<String, Value>, after: &Map<String, Value>) -> bool {
    let mut before = before.clone();
    let mut after = after.clone();
    before.remove("lifecycle");
    after.remove("lifecycle");
    before == after
}

fn validate_chat_detail_page(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "chat detail page")?;
    exact_keys(
        object,
        &[
            "chat",
            "afterEventSequence",
            "messages",
            "runs",
            "nextEventSequence",
        ],
    )?;
    let chat = validate_chat_record(required(object, "chat")?)?;
    let after = unsigned(object.get("afterEventSequence"), "afterEventSequence")?;
    let messages = array(
        object.get("messages"),
        "messages",
        G5_EVENT_PAGE_MAX_ITEMS_V1,
    )?;
    let runs = array(object.get("runs"), "runs", G5_EVENT_PAGE_MAX_ITEMS_V1)?;
    let mut previous = after;
    for message in messages {
        let message = validate_team_message(message)?;
        same_chat_identity(chat, message)?;
        let sequence = positive(message.get("eventSequence"), "eventSequence")?;
        if sequence != previous + 1 {
            return fail("durable eventSequence must be contiguous and ordered");
        }
        previous = sequence;
    }
    for run in runs {
        same_chat_identity(chat, validate_run_record(run)?)?;
    }
    if !object.get("nextEventSequence").is_some_and(Value::is_null) {
        let next = positive(object.get("nextEventSequence"), "nextEventSequence")?;
        if next != previous {
            return fail("nextEventSequence must equal the last returned durable event sequence");
        }
    }
    Ok(())
}

fn validate_chat_list_page(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "chat list page")?;
    exact_keys(object, &["projectId", "items", "nextCursor"])?;
    let project_id = id(object.get("projectId"), "projectId")?;
    let chats = array(object.get("items"), "chat items", G5_PAGE_MAX_ITEMS_V1)?;
    let mut worktrees = HashSet::new();
    let mut workspaces = HashSet::new();
    for chat in chats {
        let chat = validate_chat_record(chat)?;
        if chat.get("projectId").and_then(Value::as_str) != Some(project_id) {
            return fail("chat list project mismatch");
        }
        let worktree = string(chat.get("worktreeId"), "worktreeId")?;
        let workspace = string(chat.get("workspaceIdentity"), "workspaceIdentity")?;
        if !worktrees.insert(worktree) || !workspaces.insert(workspace) {
            return fail("chat/worktree/workspace binding must be 1:1");
        }
    }
    if !object.get("nextCursor").is_some_and(Value::is_null) {
        bounded_text(object.get("nextCursor"), "nextCursor", 512)?;
    }
    Ok(())
}

fn validate_chat_list_request(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "chat list request")?;
    exact_keys(object, &["projectId", "cursor", "limit"])?;
    id(object.get("projectId"), "projectId")?;
    validate_page_fields(object, G5_PAGE_MAX_ITEMS_V1)
}

fn validate_chat_get_request(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "chat get request")?;
    exact_keys(object, &["chatId", "afterEventSequence", "cursor", "limit"])?;
    id(object.get("chatId"), "chatId")?;
    unsigned(object.get("afterEventSequence"), "afterEventSequence")?;
    validate_page_fields(object, G5_EVENT_PAGE_MAX_ITEMS_V1)
}

fn validate_page_request(value: &Value, maximum: usize) -> Result<(), G5ContractError> {
    let object = object(value, "page request")?;
    exact_keys(object, &["cursor", "limit"])?;
    if !object.get("cursor").is_some_and(Value::is_null) {
        bounded_text(object.get("cursor"), "cursor", 512)?;
    }
    let limit = positive(object.get("limit"), "page limit")?;
    if limit > maximum as u64 {
        return fail(format!("page limit exceeds {maximum}"));
    }
    Ok(())
}

fn validate_fixture_run_start(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "fixture run request")?;
    exact_keys(object, &["chatId", "roleId", "scenario"])?;
    id(object.get("chatId"), "chatId")?;
    one_of(object.get("roleId"), G5_ROLE_IDS_V1, "roleId")?;
    one_of(
        object.get("scenario"),
        G5_FIXTURE_SCENARIOS_V1,
        "fixture scenario",
    )?;
    Ok(object)
}

fn validate_run_cancel_request(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "run cancel request")?;
    exact_keys(object, &["chatId", "runId"])?;
    id(object.get("chatId"), "chatId")?;
    id(object.get("runId"), "runId")?;
    Ok(())
}

fn validate_writer_start(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "writer start transcript")?;
    exact_keys(
        object,
        &[
            "chat",
            "request",
            "workspace",
            "existingRuns",
            "existingWriterLocks",
            "unresolvedRecoveries",
            "persistedRun",
            "persistedLock",
            "transactionCommitted",
            "spawnAttempted",
            "outcome",
        ],
    )?;
    let chat = validate_chat_record(required(object, "chat")?)?;
    let request = validate_fixture_run_start(required(object, "request")?)?;
    if request.get("chatId") != chat.get("chatId") {
        return fail("run request chatId mismatch");
    }
    let requested_role = request.get("roleId");
    let profile = chat
        .get("roleSnapshots")
        .and_then(Value::as_array)
        .and_then(|roles| {
            roles
                .iter()
                .find(|role| role.get("roleId") == requested_role)
        })
        .ok_or_else(|| G5ContractError::new("role snapshot is missing"))?;
    let profile = validate_role_profile(profile)?;
    if profile.get("permission").and_then(Value::as_str) != Some("workspaceWrite")
        || profile.get("roleId").and_then(Value::as_str) != Some("coding")
    {
        return fail("writer start contract is coding-only");
    }
    let workspace = validate_workspace_binding(required(object, "workspace")?)?;
    if workspace.get("projectId") != chat.get("projectId")
        || workspace.get("worktreeId") != chat.get("worktreeId")
        || workspace.get("workspaceIdentity") != chat.get("workspaceIdentity")
    {
        return fail("writer workspace identity mismatch");
    }
    let runs = array(
        object.get("existingRuns"),
        "existing runs",
        G5_PAGE_MAX_ITEMS_V1,
    )?
    .iter()
    .map(|run| validate_run_record(run))
    .collect::<Result<Vec<_>, _>>()?;
    let locks = array(
        object.get("existingWriterLocks"),
        "existingWriterLocks",
        G5_PAGE_MAX_ITEMS_V1,
    )?;
    let locks = locks
        .iter()
        .map(|lock| validate_writer_lock(lock))
        .collect::<Result<Vec<_>, _>>()?;
    let recoveries = array(
        object.get("unresolvedRecoveries"),
        "unresolved recoveries",
        G5_PAGE_MAX_ITEMS_V1,
    )?
    .iter()
    .map(|recovery| validate_recovery_record(recovery))
    .collect::<Result<Vec<_>, _>>()?;
    if recoveries
        .iter()
        .any(|recovery| recovery.get("state").and_then(Value::as_str) != Some("unresolved"))
    {
        return fail("writer start recovery evidence must be unresolved");
    }
    let recovery_blocked = chat.get("state").and_then(Value::as_str) == Some("recoveryRequired")
        || workspace.get("lifecycle").and_then(Value::as_str) == Some("recoveryRequired")
        || runs.iter().any(|run| {
            run.get("state").and_then(Value::as_str) == Some("recoveryRequired")
                && (run.get("worktreeId") == chat.get("worktreeId")
                    || run.get("workspaceIdentity") == chat.get("workspaceIdentity"))
        })
        || locks.iter().any(|lock| {
            lock.get("state").and_then(Value::as_str) == Some("recoveryRequired")
                && (lock.get("worktreeId") == chat.get("worktreeId")
                    || lock.get("workspaceIdentity") == chat.get("workspaceIdentity"))
        })
        || recoveries.iter().any(|recovery| {
            recovery.get("chatId") == chat.get("chatId")
                || recovery.get("worktreeId") == chat.get("worktreeId")
                || recovery.get("workspaceIdentity") == chat.get("workspaceIdentity")
        });
    let chat_state_blocked = !chat
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|state| G5_RUN_START_CHAT_STATES_V1.contains(&state));
    let run_active = runs.iter().any(|run| {
        run.get("state")
            .and_then(Value::as_str)
            .is_some_and(|state| ["preparing", "running", "cancelling"].contains(&state))
            && run.get("chatId") == chat.get("chatId")
            && run.get("worktreeId") == chat.get("worktreeId")
            && run.get("workspaceIdentity") == chat.get("workspaceIdentity")
    });
    let writer_locked = locks.iter().any(|lock| {
        lock.get("state").and_then(Value::as_str) == Some("held")
            && (lock.get("worktreeId") == chat.get("worktreeId")
                || lock.get("workspaceIdentity") == chat.get("workspaceIdentity"))
    });
    let outcome = one_of(
        object.get("outcome"),
        &[
            "started",
            "writerLocked",
            "recoveryBlocked",
            "workspaceAbandoned",
            "chatStateBlocked",
            "runActive",
        ],
        "writer outcome",
    )?;
    let expected_outcome =
        if workspace.get("lifecycle").and_then(Value::as_str) == Some("abandoned") {
            "workspaceAbandoned"
        } else if recovery_blocked {
            "recoveryBlocked"
        } else if chat_state_blocked {
            "chatStateBlocked"
        } else if run_active {
            "runActive"
        } else if writer_locked {
            "writerLocked"
        } else {
            "started"
        };
    if outcome != expected_outcome {
        return fail(format!("writer outcome must be {expected_outcome}"));
    }
    if outcome == "started" {
        let run = validate_run_record(required(object, "persistedRun")?)?;
        let lock = validate_writer_lock(required(object, "persistedLock")?)?;
        if lock.get("worktreeId") != chat.get("worktreeId")
            || lock.get("workspaceIdentity") != chat.get("workspaceIdentity")
            || lock.get("writerRunId") != run.get("runId")
            || lock.get("state").and_then(Value::as_str) != Some("held")
        {
            return fail("persisted run/lock identity mismatch");
        }
        same_chat_identity(chat, run)?;
        if run.get("selectedProfile") != Some(&Value::Object(profile.clone())) {
            return fail("run profile must resolve from chat snapshot");
        }
        if run.get("fixtureScenario") != request.get("scenario") {
            return fail("persisted writer run changed fixture scenario");
        }
        if object.get("transactionCommitted").and_then(Value::as_bool) != Some(true)
            || object.get("spawnAttempted").and_then(Value::as_bool) != Some(true)
        {
            return fail("run and lock must commit atomically before spawn");
        }
    } else if !object.get("persistedRun").is_some_and(Value::is_null)
        || !object.get("persistedLock").is_some_and(Value::is_null)
        || object.get("transactionCommitted").and_then(Value::as_bool) != Some(false)
        || object.get("spawnAttempted").and_then(Value::as_bool) != Some(false)
    {
        return fail("rejected writer start cannot persist or spawn");
    }
    Ok(())
}

fn validate_run_start(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "run start transcript")?;
    exact_keys(
        object,
        &[
            "chat",
            "request",
            "platform",
            "workspace",
            "existingRuns",
            "existingWriterLocks",
            "unresolvedRecoveries",
            "profileResolution",
            "persistedRun",
            "persistedLock",
            "transactionCommitted",
            "spawnAttempted",
            "outcome",
        ],
    )?;
    let chat = validate_chat_record(required(object, "chat")?)?;
    let request = validate_fixture_run_start(required(object, "request")?)?;
    if request.get("chatId") != chat.get("chatId") {
        return fail("run request chatId mismatch");
    }
    let platform = one_of(
        object.get("platform"),
        G5_EXECUTION_PLATFORMS_V1,
        "execution platform",
    )?;
    let workspace = validate_workspace_binding(required(object, "workspace")?)?;
    if workspace.get("projectId") != chat.get("projectId")
        || workspace.get("worktreeId") != chat.get("worktreeId")
        || workspace.get("workspaceIdentity") != chat.get("workspaceIdentity")
    {
        return fail("run workspace identity mismatch");
    }
    let runs = array(
        object.get("existingRuns"),
        "existing runs",
        G5_PAGE_MAX_ITEMS_V1,
    )?;
    let locks = array(
        object.get("existingWriterLocks"),
        "existing writer locks",
        G5_PAGE_MAX_ITEMS_V1,
    )?;
    let recoveries = array(
        object.get("unresolvedRecoveries"),
        "unresolved recoveries",
        G5_PAGE_MAX_ITEMS_V1,
    )?;
    let runs: Vec<_> = runs
        .iter()
        .map(validate_run_record)
        .collect::<Result<_, _>>()?;
    let locks: Vec<_> = locks
        .iter()
        .map(validate_writer_lock)
        .collect::<Result<_, _>>()?;
    let recoveries: Vec<_> = recoveries
        .iter()
        .map(validate_recovery_record)
        .collect::<Result<_, _>>()?;
    if recoveries
        .iter()
        .any(|recovery| recovery.get("state").and_then(Value::as_str) != Some("unresolved"))
    {
        return fail("run start recovery evidence must be unresolved");
    }
    let resolution = validate_profile_resolution(required(object, "profileResolution")?)?;
    let requested_role = request.get("roleId");
    let snapshot_profile_value = chat
        .get("roleSnapshots")
        .and_then(Value::as_array)
        .and_then(|roles| {
            roles
                .iter()
                .find(|profile| profile.get("roleId") == requested_role)
        })
        .ok_or_else(|| G5ContractError::new("role snapshot is missing"))?;
    let snapshot_profile = validate_role_profile(snapshot_profile_value)?;
    if resolution.get("snapshotProfile") != Some(snapshot_profile_value) {
        return fail("profile resolution drifted from chat snapshot");
    }
    let recovery_blocked = chat.get("state").and_then(Value::as_str) == Some("recoveryRequired")
        || workspace.get("lifecycle").and_then(Value::as_str) == Some("recoveryRequired")
        || runs.iter().any(|run| {
            run.get("state").and_then(Value::as_str) == Some("recoveryRequired")
                && (run.get("worktreeId") == chat.get("worktreeId")
                    || run.get("workspaceIdentity") == chat.get("workspaceIdentity"))
        })
        || locks.iter().any(|lock| {
            lock.get("state").and_then(Value::as_str) == Some("recoveryRequired")
                && (lock.get("worktreeId") == chat.get("worktreeId")
                    || lock.get("workspaceIdentity") == chat.get("workspaceIdentity"))
        })
        || recoveries.iter().any(|recovery| {
            recovery.get("chatId") == chat.get("chatId")
                || recovery.get("worktreeId") == chat.get("worktreeId")
                || recovery.get("workspaceIdentity") == chat.get("workspaceIdentity")
        });
    let chat_state_blocked = !chat
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|state| G5_RUN_START_CHAT_STATES_V1.contains(&state));
    let run_active = runs.iter().any(|run| {
        run.get("state")
            .and_then(Value::as_str)
            .is_some_and(|state| ["preparing", "running", "cancelling"].contains(&state))
            && run.get("chatId") == chat.get("chatId")
            && run.get("worktreeId") == chat.get("worktreeId")
            && run.get("workspaceIdentity") == chat.get("workspaceIdentity")
    });
    let writer_locked = snapshot_profile.get("permission").and_then(Value::as_str)
        == Some("workspaceWrite")
        && locks.iter().any(|lock| {
            lock.get("worktreeId") == chat.get("worktreeId")
                || lock.get("workspaceIdentity") == chat.get("workspaceIdentity")
        });
    let expected_outcome = if platform == "win32" {
        "platformUnsupported"
    } else if workspace.get("lifecycle").and_then(Value::as_str) == Some("abandoned") {
        "workspaceAbandoned"
    } else if recovery_blocked {
        "recoveryBlocked"
    } else if chat_state_blocked {
        "chatStateBlocked"
    } else if run_active {
        "runActive"
    } else if resolution.get("outcome").and_then(Value::as_str) == Some("unavailable") {
        "profileUnavailable"
    } else if writer_locked {
        "writerLocked"
    } else {
        "started"
    };
    let outcome = one_of(
        object.get("outcome"),
        &[
            "started",
            "platformUnsupported",
            "recoveryBlocked",
            "workspaceAbandoned",
            "chatStateBlocked",
            "runActive",
            "profileUnavailable",
            "writerLocked",
        ],
        "run start outcome",
    )?;
    if outcome != expected_outcome {
        return fail(format!("run start outcome must be {expected_outcome}"));
    }
    if outcome == "started" {
        let run = validate_run_record(required(object, "persistedRun")?)?;
        same_chat_identity(chat, run)?;
        if resolution.get("resolvedProfile").is_none_or(Value::is_null)
            || run.get("selectedProfile") != resolution.get("resolvedProfile")
            || run.get("selectedProfile") != Some(snapshot_profile_value)
        {
            return fail("persisted run profile must exactly match the chat snapshot");
        }
        if run.get("fixtureScenario") != request.get("scenario") {
            return fail("persisted run changed fixture scenario");
        }
        if run.get("state").and_then(Value::as_str) != Some("preparing") {
            return fail("new run must persist in preparing state");
        }
        if snapshot_profile.get("permission").and_then(Value::as_str) == Some("workspaceWrite") {
            let lock = validate_writer_lock(required(object, "persistedLock")?)?;
            if lock.get("worktreeId") != chat.get("worktreeId")
                || lock.get("workspaceIdentity") != chat.get("workspaceIdentity")
                || lock.get("writerRunId") != run.get("runId")
                || lock.get("state").and_then(Value::as_str) != Some("held")
            {
                return fail("writer run and lock identity mismatch");
            }
        } else if !object.get("persistedLock").is_some_and(Value::is_null) {
            return fail("read-only run must not acquire a writer lock");
        }
        if object.get("transactionCommitted").and_then(Value::as_bool) != Some(true)
            || object.get("spawnAttempted").and_then(Value::as_bool) != Some(true)
        {
            return fail("run persistence must commit before spawn");
        }
    } else if !object.get("persistedRun").is_some_and(Value::is_null)
        || !object.get("persistedLock").is_some_and(Value::is_null)
        || object.get("transactionCommitted").and_then(Value::as_bool) != Some(false)
        || object.get("spawnAttempted").and_then(Value::as_bool) != Some(false)
    {
        return fail("rejected run start cannot persist, lock, or spawn");
    }
    Ok(())
}

fn validate_cancel_termination_recovery(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "cancel termination recovery transcript")?;
    exact_keys(
        object,
        &[
            "beforeChat",
            "beforeRun",
            "beforeWriterLock",
            "afterChat",
            "afterRun",
            "afterWriterLock",
            "recovery",
            "outcome",
        ],
    )?;
    let before_chat = validate_chat_record(required(object, "beforeChat")?)?;
    let before_run = validate_run_record(required(object, "beforeRun")?)?;
    let after_chat = validate_chat_record(required(object, "afterChat")?)?;
    let after_run = validate_run_record(required(object, "afterRun")?)?;
    same_chat_identity(before_chat, before_run)?;
    same_chat_identity(after_chat, after_run)?;
    let chat_transition = serde_json::json!({
        "before": Value::Object(before_chat.clone()),
        "after": Value::Object(after_chat.clone())
    });
    validate_chat_transition(&chat_transition)?;
    if before_run.get("state").and_then(Value::as_str) != Some("cancelling") {
        return fail("cancel transcript must begin with a cancelling run");
    }
    for field in [
        "chatId",
        "taskId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
        "settingsRevision",
    ] {
        if before_chat.get(field) != after_chat.get(field) {
            return fail(format!("cancel transcript changed chat {field}"));
        }
    }
    if before_chat.get("roleSnapshots") != after_chat.get("roleSnapshots")
        || before_chat.get("goal") != after_chat.get("goal")
        || before_run.get("runId") != after_run.get("runId")
        || before_run.get("fixtureScenario") != after_run.get("fixtureScenario")
        || before_run.get("selectedProfile") != after_run.get("selectedProfile")
    {
        return fail("cancel transcript identity changed");
    }
    let before_lock = if object.get("beforeWriterLock").is_some_and(Value::is_null) {
        None
    } else {
        Some(validate_writer_lock(required(object, "beforeWriterLock")?)?)
    };
    let writer_run = before_run
        .get("selectedProfile")
        .and_then(|profile| profile.get("permission"))
        .and_then(Value::as_str)
        == Some("workspaceWrite");
    if writer_run {
        let Some(lock) = before_lock else {
            return fail("writer cancel requires the matching held writer lock");
        };
        if lock.get("worktreeId") != before_run.get("worktreeId")
            || lock.get("workspaceIdentity") != before_run.get("workspaceIdentity")
            || lock.get("writerRunId") != before_run.get("runId")
            || lock.get("state").and_then(Value::as_str) != Some("held")
        {
            return fail("writer cancel requires the matching held writer lock");
        }
    } else if before_lock.is_some() {
        return fail("read-only cancel cannot hold a writer lock");
    }
    let outcome = one_of(
        object.get("outcome"),
        &["canceled", "recoveryRequired"],
        "cancel outcome",
    )?;
    if outcome == "canceled" {
        if before_run.get("fixtureScenario").and_then(Value::as_str) != Some("cancel-clean")
            || after_run.get("state").and_then(Value::as_str) != Some("canceled")
            || after_chat.get("state").and_then(Value::as_str) != Some("canceled")
            || after_run
                .get("terminationEvidence")
                .and_then(|evidence| evidence.get("processesProvenGone"))
                .and_then(Value::as_bool)
                != Some(true)
            || !object.get("afterWriterLock").is_some_and(Value::is_null)
            || !object.get("recovery").is_some_and(Value::is_null)
        {
            return fail("clean cancel must release lock after proven termination");
        }
    } else {
        let recovery = validate_recovery_record(required(object, "recovery")?)?;
        if before_run.get("fixtureScenario").and_then(Value::as_str) != Some("cancel-uncertain")
            || after_run.get("state").and_then(Value::as_str) != Some("recoveryRequired")
            || after_chat.get("state").and_then(Value::as_str) != Some("recoveryRequired")
            || after_run
                .get("terminationEvidence")
                .and_then(|evidence| evidence.get("processesProvenGone"))
                .and_then(Value::as_bool)
                != Some(false)
            || recovery.get("state").and_then(Value::as_str) != Some("unresolved")
        {
            return fail("uncertain cancel must atomically enter recoveryRequired");
        }
        if recovery.get("chatId") != after_chat.get("chatId")
            || recovery.get("taskId") != after_chat.get("taskId")
            || recovery.get("projectId") != after_chat.get("projectId")
            || recovery.get("runId") != after_run.get("runId")
            || recovery.get("worktreeId") != after_chat.get("worktreeId")
            || recovery.get("workspaceIdentity") != after_chat.get("workspaceIdentity")
        {
            return fail("cancel recovery identity mismatch");
        }
        if let Some(before_lock) = before_lock {
            let after_lock = validate_writer_lock(required(object, "afterWriterLock")?)?;
            if after_lock.get("worktreeId") != before_lock.get("worktreeId")
                || after_lock.get("workspaceIdentity") != before_lock.get("workspaceIdentity")
                || after_lock.get("writerRunId") != before_lock.get("writerRunId")
                || after_lock.get("state").and_then(Value::as_str) != Some("recoveryRequired")
            {
                return fail("uncertain cancel must retain a recoveryRequired writer lock");
            }
        } else if !object.get("afterWriterLock").is_some_and(Value::is_null) {
            return fail("read-only cancel cannot create a writer lock");
        }
    }
    Ok(())
}

fn validate_recovery_record(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "recovery record")?;
    exact_keys(
        object,
        &[
            "schemaVersion",
            "recoveryId",
            "chatId",
            "taskId",
            "runId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "reason",
            "state",
            "processIdentities",
            "revision",
            "createdAt",
            "resolvedAt",
        ],
    )?;
    schema(object.get("schemaVersion"))?;
    for field in [
        "recoveryId",
        "chatId",
        "taskId",
        "runId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        id(object.get(field), field)?;
    }
    one_of(
        object.get("reason"),
        &[
            "uncleanShutdown",
            "cancelUncertain",
            "processIdentityUncertain",
        ],
        "recovery reason",
    )?;
    let state = one_of(
        object.get("state"),
        &["unresolved", "resolvedAbandoned"],
        "recovery state",
    )?;
    let processes = array(
        object.get("processIdentities"),
        "process identities",
        G5_PAGE_MAX_ITEMS_V1,
    )?;
    if processes.is_empty() {
        return fail("recovery requires tracked process identities");
    }
    for process in processes {
        validate_process_identity(process)?;
    }
    positive(object.get("revision"), "recovery revision")?;
    timestamp(object.get("createdAt"), "createdAt")?;
    if state == "resolvedAbandoned" {
        if processes
            .iter()
            .any(|process| process.get("observedGone").and_then(Value::as_bool) != Some(true))
        {
            return fail("recovery cannot resolve while a process identity remains");
        }
        timestamp(object.get("resolvedAt"), "resolvedAt")?;
    } else if !object.get("resolvedAt").is_some_and(Value::is_null) {
        return fail("unresolved recovery resolvedAt must be null");
    }
    Ok(object)
}

fn validate_recovery_mark_resolved(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "recovery resolution request")?;
    exact_keys(object, &["recoveryId", "expectedRevision", "resolution"])?;
    id(object.get("recoveryId"), "recoveryId")?;
    positive(object.get("expectedRevision"), "expectedRevision")?;
    if object.get("resolution").and_then(Value::as_str) != Some("abandon") {
        return fail("G5 recovery resolution is abandon-only");
    }
    Ok(object)
}

fn validate_recovery_page(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "recovery page")?;
    exact_keys(object, &["items", "nextCursor"])?;
    let records = array(object.get("items"), "recovery items", G5_PAGE_MAX_ITEMS_V1)?;
    let mut identities = HashSet::new();
    for record in records {
        let record = validate_recovery_record(record)?;
        let recovery_id = string(record.get("recoveryId"), "recoveryId")?;
        if !identities.insert(recovery_id) {
            return fail("recovery page contains duplicate identities");
        }
    }
    if !object.get("nextCursor").is_some_and(Value::is_null) {
        bounded_text(object.get("nextCursor"), "nextCursor", 512)?;
    }
    Ok(())
}

fn validate_recovery_resolution(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "recovery resolution transcript")?;
    exact_keys(
        object,
        &[
            "before",
            "request",
            "after",
            "workspaceBefore",
            "workspaceAfter",
            "chatAfter",
            "outcome",
        ],
    )?;
    let before = validate_recovery_record(required(object, "before")?)?;
    let request = validate_recovery_mark_resolved(required(object, "request")?)?;
    let workspace_before = validate_workspace_binding(required(object, "workspaceBefore")?)?;
    if workspace_before.get("worktreeId") != before.get("worktreeId")
        || workspace_before.get("workspaceIdentity") != before.get("workspaceIdentity")
        || workspace_before.get("projectId") != before.get("projectId")
        || workspace_before.get("lifecycle").and_then(Value::as_str) != Some("recoveryRequired")
    {
        return fail("recovery workspace identity or lifecycle mismatch");
    }
    let outcome = one_of(
        object.get("outcome"),
        &["resolvedAbandoned", "processesRemain", "revisionConflict"],
        "recovery outcome",
    )?;
    if request.get("recoveryId") != before.get("recoveryId") {
        return fail("recovery request identity mismatch");
    }
    let all_gone = before
        .get("processIdentities")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values
                .iter()
                .all(|value| value.get("observedGone").and_then(Value::as_bool) == Some(true))
        });
    if outcome == "resolvedAbandoned" {
        let after = validate_recovery_record(required(object, "after")?)?;
        if !all_gone
            || request.get("expectedRevision") != before.get("revision")
            || after.get("state").and_then(Value::as_str) != Some("resolvedAbandoned")
            || after.get("revision").and_then(Value::as_u64)
                != before
                    .get("revision")
                    .and_then(Value::as_u64)
                    .map(|value| value + 1)
        {
            return fail("invalid abandon resolution");
        }
        for field in [
            "recoveryId",
            "chatId",
            "taskId",
            "runId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
        ] {
            if before.get(field) != after.get(field) {
                return fail(format!("recovery resolution changed {field}"));
            }
        }
        let workspace_after = validate_workspace_binding(required(object, "workspaceAfter")?)?;
        let chat_after = validate_chat_record(required(object, "chatAfter")?)?;
        if workspace_after.get("projectId") != workspace_before.get("projectId")
            || workspace_after.get("worktreeId") != workspace_before.get("worktreeId")
            || workspace_after.get("workspaceIdentity") != workspace_before.get("workspaceIdentity")
            || workspace_after.get("lifecycle").and_then(Value::as_str) != Some("abandoned")
        {
            return fail("resolved recovery must terminally abandon the workspace");
        }
        if chat_after.get("chatId") != before.get("chatId")
            || chat_after.get("taskId") != before.get("taskId")
            || chat_after.get("projectId") != before.get("projectId")
            || chat_after.get("worktreeId") != before.get("worktreeId")
            || chat_after.get("workspaceIdentity") != before.get("workspaceIdentity")
            || chat_after.get("state").and_then(Value::as_str) != Some("discarded")
        {
            return fail("resolved recovery must terminally discard the old chat");
        }
    } else {
        if !object.get("after").is_some_and(Value::is_null) {
            return fail("failed recovery resolution cannot return an updated record");
        }
        if !object.get("workspaceAfter").is_some_and(Value::is_null)
            || !object.get("chatAfter").is_some_and(Value::is_null)
        {
            return fail("failed recovery resolution cannot mutate workspace or chat");
        }
        if outcome == "processesRemain" && all_gone {
            return fail("processesRemain requires a live or uncertain identity");
        }
        if outcome == "revisionConflict"
            && request.get("expectedRevision") == before.get("revision")
        {
            return fail("revisionConflict requires revision drift");
        }
    }
    Ok(())
}

fn validate_workspace_binding(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "workspace binding")?;
    exact_keys(
        object,
        &["projectId", "worktreeId", "workspaceIdentity", "lifecycle"],
    )?;
    for field in ["projectId", "worktreeId", "workspaceIdentity"] {
        id(object.get(field), field)?;
    }
    one_of(
        object.get("lifecycle"),
        &["active", "recoveryRequired", "abandoned"],
        "workspace lifecycle",
    )?;
    Ok(object)
}

fn validate_writer_lock(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "writer lock")?;
    exact_keys(
        object,
        &["worktreeId", "workspaceIdentity", "writerRunId", "state"],
    )?;
    for field in ["worktreeId", "workspaceIdentity", "writerRunId"] {
        id(object.get(field), field)?;
    }
    one_of(
        object.get("state"),
        &["held", "recoveryRequired"],
        "writer lock state",
    )?;
    Ok(object)
}

fn validate_profile_resolution(value: &Value) -> Result<&Map<String, Value>, G5ContractError> {
    let object = object(value, "profile resolution")?;
    exact_keys(object, &["outcome", "snapshotProfile", "resolvedProfile"])?;
    let snapshot = validate_role_profile(required(object, "snapshotProfile")?)?;
    let outcome = one_of(
        object.get("outcome"),
        &["exactAvailable", "unavailable"],
        "profile resolution outcome",
    )?;
    if outcome == "exactAvailable" {
        let resolved = validate_role_profile(required(object, "resolvedProfile")?)?;
        if resolved != snapshot {
            return fail("resolved profile must exactly equal the chat snapshot");
        }
    } else if !object.get("resolvedProfile").is_some_and(Value::is_null) {
        return fail("unavailable profile cannot silently fall back");
    }
    Ok(object)
}

fn validate_role_set(value: Option<&Value>, name: &str) -> Result<(), G5ContractError> {
    let roles = array(value, name, G5_ROLE_IDS_V1.len())?;
    if roles.len() != G5_ROLE_IDS_V1.len() {
        return fail(format!("{name} must contain exactly three roles"));
    }
    for (index, role) in roles.iter().enumerate() {
        let role = validate_role_profile(role)?;
        if role.get("roleId").and_then(Value::as_str) != Some(G5_ROLE_IDS_V1[index]) {
            return fail(format!(
                "{name} must use canonical planning/coding/review order"
            ));
        }
    }
    Ok(())
}

fn validate_page_fields(
    object: &Map<String, Value>,
    maximum: usize,
) -> Result<(), G5ContractError> {
    if !object.get("cursor").is_some_and(Value::is_null) {
        bounded_text(object.get("cursor"), "cursor", 512)?;
    }
    let limit = positive(object.get("limit"), "page limit")?;
    if limit > maximum as u64 {
        return fail(format!("page limit exceeds {maximum}"));
    }
    Ok(())
}

fn validate_termination_evidence(value: &Value, state: &str) -> Result<(), G5ContractError> {
    let object = object(value, "termination evidence")?;
    exact_keys(
        object,
        &[
            "outcome",
            "processesProvenGone",
            "exitCode",
            "signal",
            "observedAt",
        ],
    )?;
    let outcome = one_of(
        object.get("outcome"),
        &["cleanExit", "failed", "canceled", "uncertain"],
        "termination outcome",
    )?;
    let gone = object
        .get("processesProvenGone")
        .and_then(Value::as_bool)
        .ok_or_else(|| G5ContractError::new("processesProvenGone must be boolean"))?;
    if !object.get("exitCode").is_some_and(Value::is_null)
        && !object.get("exitCode").is_some_and(Value::is_i64)
        && !object.get("exitCode").is_some_and(Value::is_u64)
    {
        return fail("exitCode must be an integer or null");
    }
    if !object.get("signal").is_some_and(Value::is_null) {
        bounded_text(object.get("signal"), "signal", 64)?;
    }
    timestamp(object.get("observedAt"), "observedAt")?;
    let expected = match state {
        "succeeded" => "cleanExit",
        "failed" => "failed",
        "canceled" => "canceled",
        "recoveryRequired" => "uncertain",
        _ => return fail("active run cannot have termination evidence"),
    };
    if outcome != expected {
        return fail("termination outcome does not match run state");
    }
    if state == "recoveryRequired" && gone {
        return fail("recoveryRequired must retain uncertain process evidence");
    }
    if state != "recoveryRequired" && !gone {
        return fail("terminal run requires proven process disappearance");
    }
    Ok(())
}

fn validate_process_identity(value: &Value) -> Result<(), G5ContractError> {
    let object = object(value, "process identity")?;
    exact_keys(
        object,
        &["pid", "startTime", "executablePathDigest", "observedGone"],
    )?;
    let pid = positive(object.get("pid"), "pid")?;
    if pid > u64::from(u32::MAX) {
        return fail("pid exceeds u32");
    }
    timestamp(object.get("startTime"), "process startTime")?;
    bounded_text(
        object.get("executablePathDigest"),
        "executablePathDigest",
        256,
    )?;
    if !object.get("observedGone").is_some_and(Value::is_boolean) {
        return fail("observedGone must be boolean");
    }
    Ok(())
}

fn same_chat_identity(
    chat: &Map<String, Value>,
    child: &Map<String, Value>,
) -> Result<(), G5ContractError> {
    for field in ["chatId", "taskId", "worktreeId"] {
        if child.get(field) != chat.get(field) {
            return fail("cross-chat/task/worktree record");
        }
    }
    if child.contains_key("workspaceIdentity")
        && child.get("workspaceIdentity") != chat.get("workspaceIdentity")
    {
        return fail("workspace identity mismatch");
    }
    Ok(())
}

fn object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, G5ContractError> {
    value
        .as_object()
        .ok_or_else(|| G5ContractError::new(format!("{name} must be an object")))
}

fn required<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a Value, G5ContractError> {
    object
        .get(key)
        .ok_or_else(|| G5ContractError::new(format!("{key} is required")))
}

fn exact_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), G5ContractError> {
    let actual: HashSet<_> = object.keys().map(String::as_str).collect();
    let wanted: HashSet<_> = expected.iter().copied().collect();
    if actual != wanted {
        return fail("unexpected fields");
    }
    Ok(())
}

fn array<'a>(
    value: Option<&'a Value>,
    name: &str,
    maximum: usize,
) -> Result<&'a Vec<Value>, G5ContractError> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| G5ContractError::new(format!("{name} must be a bounded array")))?;
    if values.len() > maximum {
        return fail(format!("{name} must be a bounded array"));
    }
    Ok(values)
}

fn schema(value: Option<&Value>) -> Result<(), G5ContractError> {
    if value.and_then(Value::as_u64) != Some(G5_SCHEMA_VERSION_V1) {
        return fail("schemaVersion must be 1");
    }
    Ok(())
}

fn id<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str, G5ContractError> {
    let value = string(value, name)?;
    if value.is_empty() || value.len() > G5_ID_MAX_BYTES_V1 {
        return fail(format!(
            "{name} is required and must fit the UTF-8 byte limit"
        ));
    }
    Ok(value)
}

fn nullable_id<'a>(
    value: Option<&'a Value>,
    name: &str,
) -> Result<Option<&'a str>, G5ContractError> {
    if value.is_some_and(Value::is_null) {
        Ok(None)
    } else {
        id(value, name).map(Some)
    }
}

fn string<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str, G5ContractError> {
    value
        .and_then(Value::as_str)
        .ok_or_else(|| G5ContractError::new(format!("{name} must be a string")))
}

fn bounded_text<'a>(
    value: Option<&'a Value>,
    name: &str,
    maximum: usize,
) -> Result<&'a str, G5ContractError> {
    let value = string(value, name)?;
    if value.trim().is_empty() || value.len() > maximum {
        return fail(format!("{name} must be nonempty and bounded"));
    }
    Ok(value)
}

fn unsigned(value: Option<&Value>, name: &str) -> Result<u64, G5ContractError> {
    let value = value.and_then(Value::as_u64).ok_or_else(|| {
        G5ContractError::new(format!("{name} must be a non-negative safe integer"))
    })?;
    if value > super::MAX_SEQUENCE {
        return fail(format!("{name} must be a non-negative safe integer"));
    }
    Ok(value)
}

fn positive(value: Option<&Value>, name: &str) -> Result<u64, G5ContractError> {
    let value = unsigned(value, name)?;
    if value == 0 {
        return fail(format!("{name} must be positive"));
    }
    Ok(value)
}

fn one_of<'a>(
    value: Option<&'a Value>,
    values: &[&str],
    name: &str,
) -> Result<&'a str, G5ContractError> {
    let value = string(value, name)?;
    if !values.contains(&value) {
        return fail(format!("invalid {name}"));
    }
    Ok(value)
}

fn timestamp(value: Option<&Value>, name: &str) -> Result<(), G5ContractError> {
    let value = string(value, name)?;
    timestamp_text(value, name)
}

fn timestamp_text(value: &str, name: &str) -> Result<(), G5ContractError> {
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

fn fail<T>(detail: impl Into<String>) -> Result<T, G5ContractError> {
    Err(G5ContractError::new(detail))
}

#[cfg(test)]
mod tests;
