//! Pure row/source comparisons. No hidden database reads or all-members walk.
use super::*;
use elsecrew_protocol::run_recovery_v1::validate_g5_payload_v1;
use elsecrew_protocol::storage_lifetime_v1::{
    validate_frozen_message_source_v1, validate_storage_lifetime_workspace_project_source_v1,
};

pub(super) fn text<'a>(row: &'a Value, key: &str) -> Result<&'a str, SidecarError> {
    row.get(key)
        .and_then(Value::as_str)
        .ok_or_else(session_error)
}
pub(super) fn number(row: &Value, key: &str) -> Result<u64, SidecarError> {
    row.get(key)
        .and_then(Value::as_u64)
        .filter(|n| *n <= elsecrew_protocol::MAX_SEQUENCE)
        .ok_or_else(session_error)
}
fn canonical(raw: &str) -> Result<Value, SidecarError> {
    let value: Value = serde_json::from_str(raw).map_err(|_| session_error())?;
    if canonical_json_v1(value.clone()).map_err(|_| session_error())? != raw {
        return Err(session_error());
    }
    Ok(value)
}
pub(super) fn frozen(row: Value, cutoff: u64) -> Result<LedgerEventV1, SidecarError> {
    let source = validate_frozen_operation_source_v1(&projection::protocol_value(&row))
        .map_err(|_| session_error())?;
    if source.source_event_rowid > cutoff {
        return Err(session_error());
    }
    Ok(ledger_event_from_frozen_operation(&source))
}
pub(super) fn receipt(row: &Value, source: &LedgerEventV1) -> Result<(), SidecarError> {
    let mut row = row.clone();
    row.as_object_mut()
        .ok_or_else(session_error)?
        .remove("materialized_at");
    let receipt: OperationReceiptV2 =
        serde_json::from_value(projection::protocol_value(&row)).map_err(|_| session_error())?;
    if classify_operation_against_receipt_v2(&receipt, source).map_err(|_| session_error())?
        != OperationDecisionV1::DuplicateNoOp
    {
        return Err(session_error());
    }
    Ok(())
}
pub(super) fn baseline(row: &Value, source: &LedgerEventV1) -> Result<(), SidecarError> {
    let identity = format!(
        "{}\0{}\0{}",
        source.aggregate_type, source.aggregate_id, source.aggregate_sequence
    );
    let id = format!(
        "baseline_{}",
        sha256_file_version_v1(identity.as_bytes()).trim_start_matches("sha256:")
    );
    let raw = text(row, "canonical_projection_json")?;
    if source.aggregate_type == "app_instance"
        || text(row, "baseline_id")? != id
        || text(row, "aggregate_type")? != source.aggregate_type
        || text(row, "aggregate_id")? != source.aggregate_id
        || number(row, "cutoff_aggregate_sequence")? != source.aggregate_sequence
        || number(row, "payload_schema_version")? != 1
        || raw != source.canonical_json_payload
        || text(row, "projection_digest")? != sha256_file_version_v1(raw.as_bytes())
    {
        return Err(session_error());
    }
    Ok(())
}
pub(super) fn message_row(row: &Value) -> Result<Value, SidecarError> {
    let raw = text(row, "canonical_message_json")?;
    let value = canonical(raw)?;
    validate_g5_payload_v1("teamMessage", &value).map_err(|_| session_error())?;
    if text(row, "message_id")? != text(&value, "messageId")?
        || text(row, "chat_id")? != text(&value, "chatId")?
        || number(row, "event_sequence")? != number(&value, "eventSequence")?
        || text(row, "created_at")? != text(&value, "createdAt")?
        || text(row, "message_digest")? != sha256_file_version_v1(raw.as_bytes())
    {
        return Err(session_error());
    }
    Ok(value)
}
pub(super) fn source_message(source: &projection::SourceFacts) -> Result<Value, SidecarError> {
    let payload = projection::validate_event(&source.event)?;
    let message = payload.get("last_message").ok_or_else(session_error)?;
    validate_g5_payload_v1("teamMessage", message).map_err(|_| session_error())?;
    let record = payload.get("record").ok_or_else(session_error)?;
    if source.introduced.as_deref() != Some(text(message, "messageId")?)
        || text(message, "chatId")? != source.event.aggregate_id
        || text(record, "chatId")? != source.event.aggregate_id
        || text(message, "taskId")? != text(record, "taskId")?
        || text(message, "worktreeId")? != text(record, "worktreeId")?
    {
        return Err(session_error());
    }
    Ok(message.clone())
}
pub(super) fn message(row: &Value, source: &projection::SourceFacts) -> Result<(), SidecarError> {
    if message_row(row)? != source_message(source)? {
        return Err(session_error());
    }
    Ok(())
}
pub(super) fn frozen_message(row: &Value, frozen: &Value) -> Result<(), SidecarError> {
    let frozen = validate_frozen_message_source_v1(&projection::protocol_value(frozen))
        .map_err(|_| session_error())?;
    if text(row, "canonical_message_json")? != frozen.canonical_message_json
        || text(row, "message_id")? != frozen.message_id
        || text(row, "chat_id")? != frozen.chat_id
        || number(row, "event_sequence")? != frozen.durable_sequence
    {
        return Err(session_error());
    }
    message_row(row)?;
    Ok(())
}
pub(super) fn workspace_member(
    key: &str,
    project: &str,
    value: &Value,
) -> Result<(), SidecarError> {
    validate_g5_payload_v1("preparedChatWorkspace", value).map_err(|_| session_error())?;
    if text(value, "preparationId")? != key
        || text(
            value.get("manifest").ok_or_else(session_error)?,
            "projectId",
        )? != project
    {
        return Err(session_error());
    }
    Ok(())
}
pub(super) fn workspace_row(row: &Value) -> Result<Value, SidecarError> {
    let raw = text(row, "canonical_record_json")?;
    let value = canonical(raw)?;
    workspace_member(
        text(row, "preparation_id")?,
        text(row, "project_id")?,
        &value,
    )?;
    let manifest = value.get("manifest").ok_or_else(session_error)?;
    for (column, key) in [
        ("worktree_id", "worktreeId"),
        ("workspace_identity", "workspaceIdentity"),
    ] {
        if text(row, column)? != text(manifest, key)? {
            return Err(session_error());
        }
    }
    for (column, key) in [
        ("binding_state", "bindingState"),
        ("created_at", "createdAt"),
        ("updated_at", "updatedAt"),
    ] {
        if text(row, column)? != text(&value, key)? {
            return Err(session_error());
        }
    }
    if row.get("bound_chat_id") != value.get("boundChatId")
        || text(row, "record_digest")? != sha256_file_version_v1(raw.as_bytes())
    {
        return Err(session_error());
    }
    Ok(value)
}
pub(super) fn workspace_matches(row: &Value, member: &Value) -> Result<(), SidecarError> {
    if workspace_row(row)? != *member {
        return Err(session_error());
    }
    Ok(())
}
pub(super) fn bound_chat(member: &Value, chat: Option<&LedgerEventV1>) -> Result<(), SidecarError> {
    let Some(id) = member.get("boundChatId").and_then(Value::as_str) else {
        return Ok(());
    };
    let chat = chat.ok_or_else(session_error)?;
    let payload = projection::validate_event(chat)?;
    let manifest = member.get("manifest").ok_or_else(session_error)?;
    if chat.aggregate_type != "chat" || chat.aggregate_id != id {
        return Err(session_error());
    }
    for (column, key) in [
        ("project_id", "projectId"),
        ("worktree_id", "worktreeId"),
        ("workspace_identity", "workspaceIdentity"),
    ] {
        if payload.get(column).and_then(Value::as_str) != Some(text(manifest, key)?) {
            return Err(session_error());
        }
    }
    Ok(())
}
pub(super) fn project_payload(
    source: &LedgerEventV1,
    project: &str,
) -> Result<Value, SidecarError> {
    validate_storage_lifetime_workspace_project_source_v1(
        project,
        &source.aggregate_type,
        &source.aggregate_id,
        &source.canonical_json_payload,
    )
    .map_err(|_| session_error())
}
pub(super) fn project_members(
    source: &LedgerEventV1,
) -> Result<BTreeMap<String, Value>, SidecarError> {
    let mut payload = project_payload(source, &source.aggregate_id)?;
    match payload
        .as_object_mut()
        .ok_or_else(session_error)?
        .remove("prepared_workspaces")
    {
        None => Ok(BTreeMap::new()),
        Some(Value::Object(members)) => Ok(members.into_iter().collect()),
        _ => Err(session_error()),
    }
}
