//! Consistent in-place content redaction of one chat (docs/proposals/m5-chat-erasure.md).
//!
//! Every SQLite copy of the chat's natural-language content is rewritten in one
//! IMMEDIATE transaction as if it had always been `ERASED_TEXT`, so the existing
//! G5D reopen/replay/materialization/compaction verifiers need no exception for
//! redacted rows. Callers own eligibility and the durable request state machine.
use super::*;
use elsecrew_protocol::agent_adapter_v1::{
    agent_public_ledger_cumulative_snapshot_bytes_v1, validate_agent_adapter_payload_v1,
    AgentPublicEventV1,
};
use elsecrew_protocol::run_recovery_v1::validate_g5_payload_v1;

/// Deterministic replacement for every erased content string. Identical originals
/// stay identical, preserving cross-copy equality checks (final == completed item).
pub(crate) const ERASED_TEXT: &str = "[erased]";

/// Storage schema versions whose exact identity this module re-verifies before
/// commit. INTEGRATOR NOTE: when storage V13 (custom roles) lands, add 13 here and
/// its identity verifier to `verify_schema_identity_for_erasure`. No other version
/// list is involved; erasure does not bump the schema.
pub(crate) const ERASURE_SCHEMA_VERSIONS: &[u32] = &[
    STORAGE_LIFETIME_SCHEMA_VERSION_V10,
    STORAGE_LIFETIME_SCHEMA_VERSION_V11,
    STORAGE_LIFETIME_SCHEMA_VERSION_V12,
    STORAGE_LIFETIME_SCHEMA_VERSION_V13,
];

/// The only immutability triggers that block the narrowly scoped UPDATEs below.
/// They are recreated from their captured SQL inside the same transaction.
const GUARDED_UPDATE_TRIGGERS: [(&str, &str); 3] = [
    ("events_are_append_only_update", "events"),
    ("operation_receipts_compaction_immutable_update", "operation_receipts"),
    ("ledger_baselines_compaction_immutable_update", "ledger_baselines"),
];

/// Original strings shorter than this are not used for the leftover scan: short
/// fragments (e.g. "ok") would match unrelated identifiers and fail closed spuriously.
const LEFTOVER_SCAN_MIN_BYTES: usize = 12;
const LEFTOVER_SCAN_MAX_NEEDLES: usize = 4096;

const CHAT_RETAINED_KEYS: &[&str] = &[
    "chat_id", "project_id", "worktree_id", "workspace_identity", "state",
    "durable_event_sequence", "save_sessions", "discard_receipt", "application_review",
];
const CHAT_CONTENT_KEYS: &[&str] = &["record", "last_message", "orchestration_v1"];
const WORKFLOW_KEYS: &[&str] = &[
    "id", "status", "step", "role", "request_id", "run_id", "input_ids", "trigger_id",
    "instruction", "reports", "changed",
];
const RUN_RETAINED_KEYS: &[&str] = &[
    "run_id", "chat_id", "state", "worktree_path", "source_kind", "record",
    "workspace_manifest", "barrier", "process_identity_internal", "process_identity",
    "termination", "recovery", "output_format_version", "provider_discovery",
    "runtime_attestation", "native_thread_receipt", "native_transport",
    "native_provider_manifest", "claude_preflight", "claude_session_observation",
    "approval_intents_internal", "provider_run_fence", "recovery_id_reservation",
    // Recovery abandon intent: identities, revision, tracked processes and the
    // expected workspace manifest/digest only (agent_runtime.rs RECOVERY_INTENT_FIELD).
    "recovery_resolution_intent",
    "public_event_count",
];
const RUN_CONTENT_KEYS: &[&str] = &[
    "public_events", "public_text_bytes", "public_ledger_cumulative_snapshot_bytes",
];

pub(crate) struct ChatErasureCommit {
    pub chat_id: String,
    pub project_id: String,
    pub expected_project_sequence: u64,
    pub project_operation_id: String,
    pub request_id: String,
    /// Full project payload carrying the `redacted` state record.
    pub project_payload: Map<String, Value>,
}

#[cfg(test)]
thread_local! { static INJECT_REFUSAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
/// Test-only: refuse inside the rewrite transaction after the guard triggers dropped.
#[cfg(test)]
pub(crate) fn inject_refusal_in_transaction() {
    INJECT_REFUSAL.with(|value| value.set(true));
}

fn denied(detail: &str) -> SidecarError {
    SidecarError::new("chatErasureDenied", detail)
}
fn invalid(detail: &str) -> SidecarError {
    SidecarError::new("chatErasureInvalid", detail)
}

fn redact_text(value: &mut Value, needles: &mut BTreeSet<String>) -> Result<(), SidecarError> {
    match value {
        Value::String(text) if text.is_empty() || text == ERASED_TEXT => Ok(()),
        Value::String(text) => {
            needles.insert(std::mem::replace(text, ERASED_TEXT.to_string()));
            Ok(())
        }
        _ => Err(invalid("content field is not text")),
    }
}

fn redact_message(message: &mut Value, needles: &mut BTreeSet<String>) -> Result<(), SidecarError> {
    let object = message.as_object_mut().ok_or_else(|| invalid("message is not an object"))?;
    redact_text(object.get_mut("text").ok_or_else(|| invalid("message text missing"))?, needles)?;
    validate_g5_payload_v1("teamMessage", message).map_err(|_| invalid("redacted message invalid"))
}

fn redact_chat_payload(payload: &mut Map<String, Value>, needles: &mut BTreeSet<String>) -> Result<(), SidecarError> {
    if let Some(key) = payload.keys().find(|key| {
        !CHAT_RETAINED_KEYS.contains(&key.as_str()) && !CHAT_CONTENT_KEYS.contains(&key.as_str())
    }) {
        // Unclassified fields could hold content; never report a partial erasure.
        return Err(denied(&format!("chat field {key} is not classified for erasure")));
    }
    let record = payload.get_mut("record").ok_or_else(|| invalid("chat record missing"))?;
    redact_text(record.get_mut("goal").ok_or_else(|| invalid("chat goal missing"))?, needles)?;
    validate_g5_payload_v1("chatRecord", record).map_err(|_| invalid("redacted chat record invalid"))?;
    match payload.get_mut("last_message") {
        None | Some(Value::Null) => {}
        Some(message) => redact_message(message, needles)?,
    }
    if let Some(workflow) = payload.get_mut("orchestration_v1").filter(|value| !value.is_null()) {
        let object = workflow.as_object_mut().ok_or_else(|| invalid("team task is not an object"))?;
        if object.keys().any(|key| !WORKFLOW_KEYS.contains(&key.as_str())) {
            return Err(denied("team task field is not classified for erasure"));
        }
        redact_text(object.get_mut("instruction").ok_or_else(|| invalid("team instruction missing"))?, needles)?;
        for report in object.get_mut("reports").and_then(Value::as_array_mut)
            .ok_or_else(|| invalid("team reports missing"))? {
            let report = report.as_object_mut().ok_or_else(|| invalid("team report invalid"))?;
            if report.keys().any(|key| !matches!(key.as_str(), "role" | "instruction" | "response")) {
                return Err(denied("team report field is not classified for erasure"));
            }
            for field in ["instruction", "response"] {
                redact_text(report.get_mut(field).ok_or_else(|| invalid("team report text missing"))?, needles)?;
            }
        }
    }
    Ok(())
}

fn redact_run_payload(payload: &mut Map<String, Value>, needles: &mut BTreeSet<String>) -> Result<(), SidecarError> {
    if let Some(key) = payload.keys().find(|key| {
        !RUN_RETAINED_KEYS.contains(&key.as_str()) && !RUN_CONTENT_KEYS.contains(&key.as_str())
    }) {
        return Err(denied(&format!("run field {key} is not classified for erasure")));
    }
    let Some(events) = payload.get_mut("public_events").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let events = events.as_array_mut().ok_or_else(|| invalid("public events are not an array"))?;
    for event in events.iter_mut() {
        let object = event.as_object_mut().ok_or_else(|| invalid("public event invalid"))?;
        redact_text(object.get_mut("text").ok_or_else(|| invalid("public event text missing"))?, needles)?;
        validate_agent_adapter_payload_v1("publicEvent", event)
            .map_err(|_| invalid("redacted public event invalid"))?;
    }
    let text_bytes = events.iter().map(|event| event["text"].as_str().map_or(0, str::len) as u64).sum::<u64>();
    let cumulative = agent_public_ledger_cumulative_snapshot_bytes_v1(events)
        .map_err(|_| invalid("redacted public ledger invalid"))? as u64;
    // Counters are recomputed only when the projection already carried them.
    if payload.contains_key("public_text_bytes") {
        payload.insert("public_text_bytes".into(), Value::from(text_bytes));
    }
    if payload.contains_key("public_ledger_cumulative_snapshot_bytes") {
        payload.insert("public_ledger_cumulative_snapshot_bytes".into(), Value::from(cumulative));
    }
    Ok(())
}

fn redact_event_payload(
    aggregate_type: &str,
    raw: &str,
    needles: &mut BTreeSet<String>,
) -> Result<String, SidecarError> {
    let mut payload: Map<String, Value> =
        serde_json::from_str(raw).map_err(|_| invalid("ledger payload is not an object"))?;
    match aggregate_type {
        "chat" => redact_chat_payload(&mut payload, needles)?,
        "run" => redact_run_payload(&mut payload, needles)?,
        _ => return Err(invalid("unexpected erasure aggregate")),
    }
    canonical_json_v1(Value::Object(payload)).map_err(storage_contract_error)
}

/// Integrator: extend with the V13 verifier when that schema is merged.
fn verify_schema_identity_for_erasure(connection: &Connection) -> Result<(), SidecarError> {
    match storage_schema_version(connection)? {
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V10) => verify_schema_v10(connection).map(|_| ()),
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V11 | STORAGE_LIFETIME_SCHEMA_VERSION_V12 | STORAGE_LIFETIME_SCHEMA_VERSION_V13) => {
            agent_output::schema::verify(connection).map(|_| ())
        }
        _ => Err(denied("storage schema version does not support chat erasure")),
    }
}

fn exists(tx: &Transaction<'_>, sql: &str, chat_id: &str) -> Result<bool, SidecarError> {
    tx.query_row(sql, [chat_id], |row| row.get(0)).map_err(sqlite_error)
}

/// Aggregates owned by the chat: the chat itself and runs bound to it by both the
/// projection column and every source payload.
fn target_runs(tx: &Transaction<'_>, chat_id: &str) -> Result<Vec<String>, SidecarError> {
    let collect = |sql: &str| -> Result<BTreeSet<String>, SidecarError> {
        let mut statement = tx.prepare(sql).map_err(sqlite_error)?;
        let rows = statement.query_map([chat_id], |row| row.get::<_, String>(0)).map_err(sqlite_error)?;
        rows.collect::<Result<_, _>>().map_err(sqlite_error)
    };
    let projected = collect("SELECT run_id FROM runs WHERE chat_id=?1")?;
    let sourced = collect(
        "SELECT DISTINCT aggregate_id FROM events WHERE aggregate_type='run' \
         AND json_extract(canonical_json_payload,'$.chat_id')=?1",
    )?;
    if projected != sourced {
        return Err(denied("run ownership differs between projection and ledger"));
    }
    // A run event that names a different chat would be skipped silently.
    let foreign: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM events e JOIN runs r ON r.run_id=e.aggregate_id \
         WHERE e.aggregate_type='run' AND r.chat_id=?1 \
         AND json_extract(e.canonical_json_payload,'$.chat_id') IS NOT ?1)",
        [chat_id], |row| row.get(0),
    ).map_err(sqlite_error)?;
    if foreign {
        return Err(denied("run ledger names another chat"));
    }
    Ok(projected.into_iter().collect())
}

/// Compacted or plan-referenced history carries frozen copies whose rewrite is out of v1 scope.
fn reject_compaction_artifacts(tx: &Transaction<'_>, chat_id: &str) -> Result<(), SidecarError> {
    const OWNED: &str = "((aggregate_type='chat' AND aggregate_id=?1) OR (aggregate_type='run' \
        AND aggregate_id IN (SELECT run_id FROM runs WHERE chat_id=?1)))";
    for table in [
        "frozen_operation_sources", "ledger_compaction_prefixes", "storage_compaction_plan_rows",
        "storage_compaction_plans", "ledger_compaction_delete_authority",
    ] {
        if exists(tx, &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {OWNED})"), chat_id)? {
            return Err(denied("compacted chat history is not eligible for erasure yet"));
        }
    }
    let active: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM ledger_compaction_delete_authority) \
             OR EXISTS(SELECT 1 FROM maintenance_jobs WHERE state='running')",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if active || exists(tx, "SELECT EXISTS(SELECT 1 FROM frozen_message_sources WHERE chat_id=?1)", chat_id)? {
        return Err(denied("compaction state blocks chat erasure"));
    }
    Ok(())
}

fn capture_triggers(tx: &Transaction<'_>) -> Result<Vec<String>, SidecarError> {
    GUARDED_UPDATE_TRIGGERS
        .iter()
        .map(|(name, table)| {
            let sql: String = tx
                .query_row(
                    "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1 AND tbl_name=?2",
                    [name, table],
                    |row| row.get(0),
                )
                .map_err(|_| invalid("guarded trigger missing"))?;
            if !sql.contains(&format!("BEFORE UPDATE ON {table}")) || !sql.contains("RAISE(ABORT") {
                return Err(invalid("guarded trigger differs from the expected guard"));
            }
            Ok(sql)
        })
        .collect()
}

struct Rewrite {
    // (aggregate_type, aggregate_id, aggregate_sequence) -> redacted canonical payload
    payloads: BTreeMap<(String, String, u64), String>,
    // rewritten row texts for the leftover scan
    written: Vec<String>,
    needles: BTreeSet<String>,
}

fn rewrite_events(tx: &Transaction<'_>, chat_id: &str, runs: &[String], apply: bool) -> Result<Rewrite, SidecarError> {
    let mut rewrite = Rewrite { payloads: BTreeMap::new(), written: Vec::new(), needles: BTreeSet::new() };
    let mut targets = vec![("chat".to_string(), chat_id.to_string())];
    targets.extend(runs.iter().map(|run| ("run".to_string(), run.clone())));
    for (aggregate_type, aggregate_id) in &targets {
        let rows: Vec<(i64, String, String, u64, String)> = {
            let mut statement = tx.prepare(
                "SELECT rowid,event_id,operation_id,aggregate_sequence,canonical_json_payload FROM events \
                 WHERE aggregate_type=?1 AND aggregate_id=?2 ORDER BY rowid",
            ).map_err(sqlite_error)?;
            let rows = statement.query_map(params![aggregate_type, aggregate_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row_u64(row, 3)?, row.get(4)?))
            }).map_err(sqlite_error)?;
            rows.collect::<Result<_, _>>().map_err(sqlite_error)?
        };
        for (rowid, event_id, operation_id, sequence, raw) in rows {
            let redacted = redact_event_payload(aggregate_type, &raw, &mut rewrite.needles)?;
            if apply && redacted != raw {
                let changed = tx.execute(
                    "UPDATE events SET canonical_json_payload=?1 WHERE rowid=?2 AND event_id=?3",
                    params![redacted, rowid, event_id],
                ).map_err(sqlite_error)?;
                if changed != 1 { return Err(invalid("event rewrite lost its row")); }
            }
            // Receipts may still be pending materialization; when present they must match.
            let digest = sha256_file_version_v1(redacted.as_bytes())
                .strip_prefix("sha256:").map(str::to_owned)
                .ok_or_else(|| invalid("digest encoding invalid"))?;
            if apply { tx.execute(
                "UPDATE operation_receipts SET canonical_payload_json=?1, canonical_payload_digest=?2 \
                 WHERE operation_id=?3 AND event_id=?4 AND aggregate_type=?5 AND aggregate_id=?6 \
                 AND aggregate_sequence=?7",
                params![redacted, digest, operation_id, event_id, aggregate_type, aggregate_id, sql_i64(sequence)?],
            ).map_err(sqlite_error)?; }
            rewrite.written.push(redacted.clone());
            rewrite.payloads.insert((aggregate_type.clone(), aggregate_id.clone(), sequence), redacted);
        }
        // Every receipt of the aggregate must correspond to a live event (before or
        // after the rewrite: receipts are exact copies, so the query holds in both).
        let stale: i64 = tx.query_row(
            "SELECT count(*) FROM operation_receipts r WHERE r.aggregate_type=?1 AND r.aggregate_id=?2 \
             AND NOT EXISTS(SELECT 1 FROM events e WHERE e.operation_id=r.operation_id \
             AND e.canonical_json_payload=r.canonical_payload_json)",
            params![aggregate_type, aggregate_id], |row| row.get(0),
        ).map_err(sqlite_error)?;
        if stale != 0 {
            return Err(denied("receipt without a live source blocks erasure"));
        }
        rewrite_baselines(tx, aggregate_type, aggregate_id, &mut rewrite, apply)?;
    }
    Ok(rewrite)
}

fn rewrite_baselines(tx: &Transaction<'_>, aggregate_type: &str, aggregate_id: &str, rewrite: &mut Rewrite, apply: bool) -> Result<(), SidecarError> {
    let rows: Vec<(String, u64)> = {
        let mut statement = tx.prepare(
            "SELECT baseline_id,cutoff_aggregate_sequence FROM ledger_baselines \
             WHERE aggregate_type=?1 AND aggregate_id=?2",
        ).map_err(sqlite_error)?;
        let rows = statement.query_map(params![aggregate_type, aggregate_id], |row| Ok((row.get(0)?, row_u64(row, 1)?)))
            .map_err(sqlite_error)?;
        rows.collect::<Result<_, _>>().map_err(sqlite_error)?
    };
    for (baseline_id, cutoff) in rows {
        // Baselines are exact copies of the source payload at their cutoff.
        let payload = rewrite.payloads
            .get(&(aggregate_type.to_string(), aggregate_id.to_string(), cutoff))
            .cloned()
            .ok_or_else(|| denied("baseline without a live source blocks erasure"))?;
        if apply { tx.execute(
            "UPDATE ledger_baselines SET canonical_projection_json=?1, projection_digest=?2 WHERE baseline_id=?3",
            params![payload, sha256_file_version_v1(payload.as_bytes()), baseline_id],
        ).map_err(sqlite_error)?; }
        rewrite.written.push(payload);
    }
    Ok(())
}

fn rewrite_messages(tx: &Transaction<'_>, chat_id: &str, rewrite: &mut Rewrite, apply: bool) -> Result<(), SidecarError> {
    let rows: Vec<(String, String)> = {
        let mut statement = tx.prepare("SELECT message_id,canonical_message_json FROM chat_messages WHERE chat_id=?1")
            .map_err(sqlite_error)?;
        let rows = statement.query_map([chat_id], |row| Ok((row.get(0)?, row.get(1)?))).map_err(sqlite_error)?;
        rows.collect::<Result<_, _>>().map_err(sqlite_error)?
    };
    for (message_id, raw) in rows {
        let mut message: Value = serde_json::from_str(&raw).map_err(|_| invalid("retained message invalid"))?;
        redact_message(&mut message, &mut rewrite.needles)?;
        let canonical = canonical_json_v1(message).map_err(storage_contract_error)?;
        if apply { tx.execute(
            "UPDATE chat_messages SET canonical_message_json=?1, message_digest=?2 WHERE message_id=?3",
            params![canonical, sha256_file_version_v1(canonical.as_bytes()), message_id],
        ).map_err(sqlite_error)?; }
        rewrite.written.push(canonical);
    }
    Ok(())
}

fn rewrite_outputs(tx: &Transaction<'_>, runs: &[String], apply: bool) -> Result<(), SidecarError> {
    // Output tables exist from V11 on; every newer schema must be listed here
    // or erasure would silently leave output text behind.
    if !matches!(storage_schema_version(tx)?, Some(STORAGE_LIFETIME_SCHEMA_VERSION_V11 | STORAGE_LIFETIME_SCHEMA_VERSION_V12 | STORAGE_LIFETIME_SCHEMA_VERSION_V13)) {
        return Ok(());
    }
    for run_id in runs {
        let rows: Vec<(i64, String)> = {
            let mut statement = tx.prepare("SELECT rowid,event_json FROM run_output_records WHERE run_id=?1")
                .map_err(sqlite_error)?;
            let rows = statement.query_map([run_id], |row| Ok((row.get(0)?, row.get(1)?))).map_err(sqlite_error)?;
            rows.collect::<Result<_, _>>().map_err(sqlite_error)?
        };
        let mut total = 0_u64;
        for (rowid, raw) in rows {
            let mut event: AgentPublicEventV1 =
                serde_json::from_str(&raw).map_err(|_| invalid("output record invalid"))?;
            if !event.text.is_empty() {
                event.text = ERASED_TEXT.to_string();
            }
            total += event.text.len() as u64;
            let json = serde_json::to_string(&event).map_err(|_| invalid("output encoding failed"))?;
            if apply {
                tx.execute("UPDATE run_output_records SET event_json=?1 WHERE rowid=?2", params![json, rowid])
                    .map_err(sqlite_error)?;
            }
        }
        if apply {
            tx.execute(
                "UPDATE run_output_heads SET text_bytes=?1 WHERE run_id=?2",
                params![sql_i64(total)?, run_id],
            ).map_err(sqlite_error)?;
        }
    }
    Ok(())
}

/// Fail closed if any original content string (>= LEFTOVER_SCAN_MIN_BYTES, raw
/// or JSON-escaped) survived in a rewritten row, i.e. sits in a retained field.
fn scan_leftovers(rewrite: &Rewrite) -> Result<(), SidecarError> {
    let needles: Vec<(String, String)> = rewrite
        .needles
        .iter()
        .filter(|needle| needle.len() >= LEFTOVER_SCAN_MIN_BYTES)
        .take(LEFTOVER_SCAN_MAX_NEEDLES)
        .map(|needle| {
            let escaped = serde_json::to_string(needle).unwrap_or_default();
            (needle.clone(), escaped.trim_matches('"').to_string())
        })
        .collect();
    for row in &rewrite.written {
        if needles.iter().any(|(raw, escaped)| row.contains(raw.as_str()) || row.contains(escaped.as_str())) {
            return Err(denied("content was copied into an unclassified field"));
        }
    }
    Ok(())
}

fn projection_object(tx: &Transaction<'_>, sql: &str, id: &str) -> Result<Option<Map<String, Value>>, SidecarError> {
    let raw: Option<String> = tx.query_row(sql, [id], |row| row.get(0)).optional().map_err(sqlite_error)?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(|_| invalid("projection is not an object"))).transpose()
}

/// Durable part of the eligibility contract (design §2 items 1-5), re-checked in
/// the same transaction as the rewrite. Sidecar idleness (item 6) is enforced by
/// the caller on the single event loop before this call.
fn verify_eligibility(tx: &Transaction<'_>, chat_id: &str, project_id: &str, runs: &[String]) -> Result<(), SidecarError> {
    let chat = projection_object(tx, "SELECT projection_json FROM chats WHERE chat_id=?1", chat_id)?
        .ok_or_else(|| denied("chat is missing"))?;
    let workflow_running = chat.get("orchestration_v1").and_then(|value| value.get("status"))
        .is_some_and(|status| status == "running");
    if chat.get("state") != Some(&json!("discarded"))
        || chat.get("record").and_then(|record| record.get("state")) != Some(&json!("discarded"))
        || chat.get("project_id") != Some(&json!(project_id))
        || workflow_running
        || chat.get("save_sessions").is_some_and(|sessions| sessions.as_object().is_none_or(|items| !items.is_empty()))
    {
        return Err(denied("chat is not an idle discarded chat"));
    }
    let worktree = chat.get("worktree_id").and_then(Value::as_str).ok_or_else(|| denied("chat worktree is missing"))?;
    if let Some(lock) = projection_object(tx, "SELECT projection_json FROM locks WHERE lock_id=?1", &format!("lock_{worktree}"))? {
        if lock.get("writer_run_id").is_some_and(|value| !value.is_null())
            || lock.get("poisoned") != Some(&json!(false))
            || lock.get("recovery_required") != Some(&json!(false))
        {
            return Err(denied("chat workspace lock needs attention"));
        }
    }
    for run_id in runs {
        let run = projection_object(tx, "SELECT projection_json FROM runs WHERE run_id=?1", run_id)?
            .ok_or_else(|| denied("run is missing"))?;
        let evidence = if run.get("source_kind") == Some(&json!("actualAgent")) {
            run.get("termination")
        } else {
            run.get("record").and_then(|record| record.get("terminationEvidence"))
        };
        let proven_gone = evidence.and_then(|value| value.get("processesProvenGone")) == Some(&json!(true));
        let unresolved = run.get("recovery")
            .is_some_and(|value| !value.is_null() && value.get("state") != Some(&json!("resolvedAbandoned")));
        if !matches!(run.get("state").and_then(Value::as_str), Some("succeeded" | "failed" | "canceled"))
            || !proven_gone
            || unresolved
        {
            return Err(denied("chat run or recovery needs attention"));
        }
    }
    let project = projection_object(tx, "SELECT projection_json FROM projects WHERE project_id=?1", project_id)?
        .ok_or_else(|| denied("project is missing"))?;
    if project.get("application_intent").is_some_and(|value| !value.is_null() && value["state"] != "abandoned") {
        return Err(denied("project application is in progress"));
    }
    Ok(())
}

/// Classify, redact and leftover-scan every copy. With `apply=false` nothing is
/// written: the same refusals surface before any durable request state exists.
fn plan_erasure(tx: &Transaction<'_>, chat_id: &str, project_id: &str, apply: bool) -> Result<(Vec<String>, Rewrite), SidecarError> {
    storage_schema_version(tx)?
        .filter(|version| ERASURE_SCHEMA_VERSIONS.contains(version))
        .ok_or_else(|| denied("storage schema version does not support chat erasure"))?;
    let owner: Option<String> = tx
        .query_row("SELECT project_id FROM chats WHERE chat_id=?1", [chat_id], |row| row.get(0))
        .optional()
        .map_err(sqlite_error)?;
    if owner.as_deref() != Some(project_id) {
        return Err(denied("chat does not belong to the project"));
    }
    let runs = target_runs(tx, chat_id)?;
    reject_compaction_artifacts(tx, chat_id)?;
    verify_eligibility(tx, chat_id, project_id, &runs)?;
    let mut rewrite = rewrite_events(tx, chat_id, &runs, apply)?;
    rewrite_messages(tx, chat_id, &mut rewrite, apply)?;
    rewrite_outputs(tx, &runs, apply)?;
    scan_leftovers(&rewrite)?;
    Ok((runs, rewrite))
}

/// Refusals that are properties of the stored chat (not transient I/O or races).
pub(crate) fn is_erasure_refusal(error: &SidecarError) -> bool {
    error.kind == "chatErasureDenied"
}

impl Storage {
    /// Read-only storage-side eligibility (ownership and compaction state). The
    /// write path repeats every check inside its own transaction.
    pub(crate) fn chat_erasure_precheck(&self, chat_id: &str, project_id: &str) -> Result<(), SidecarError> {
        self.ensure_writes_admitted()?;
        let transaction = Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)
            .map_err(sqlite_error)?;
        // Full in-memory dry run: classification, compaction, eligibility, leftovers.
        plan_erasure(&transaction, chat_id, project_id, false).map(|_| ())
        // Dropping the read transaction rolls it back.
    }

    /// Atomically redact every DB copy of one chat's content and append the
    /// caller's `redacted` project record. Any failure rolls back everything,
    /// including the temporarily dropped guard triggers.
    pub(crate) fn redact_chat_content(&mut self, commit: ChatErasureCommit) -> Result<(), SidecarError> {
        self.ensure_writes_admitted()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let version = storage_schema_version(&transaction)?
            .filter(|version| ERASURE_SCHEMA_VERSIONS.contains(version))
            .ok_or_else(|| denied("storage schema version does not support chat erasure"))?;
        verify_schema_identity_for_erasure(&transaction)?;
        verify_append_runtime_state(&transaction)?;
        // Refuse before touching the guard triggers; the identical plan then applies.
        plan_erasure(&transaction, &commit.chat_id, &commit.project_id, false)?;
        let triggers = capture_triggers(&transaction)?;
        for (name, _) in GUARDED_UPDATE_TRIGGERS {
            transaction.execute_batch(&format!("DROP TRIGGER {name};")).map_err(sqlite_error)?;
        }
        let (runs, rewrite) = plan_erasure(&transaction, &commit.chat_id, &commit.project_id, true)?;
        #[cfg(test)]
        if INJECT_REFUSAL.with(|value| value.replace(false)) {
            return Err(denied("injected in-transaction refusal"));
        }

        // Replay the rewritten ledger; projections are exactly the latest payloads.
        let mut events = load_events(&transaction)?;
        let mut projection = replay_storage_ledger(&transaction, &events)?;
        let mut targets = vec![("chat", commit.chat_id.clone())];
        targets.extend(runs.iter().map(|run| ("run", run.clone())));
        for (kind, id) in &targets {
            let record = match *kind { "chat" => projection.chats.get(id), _ => projection.runs.get(id) }
                .ok_or_else(|| invalid("replay omitted an erased aggregate"))?;
            let sequence = required_u64(record, "last_event_sequence")?;
            let mut payload = record.clone();
            payload.remove("last_event_sequence");
            let json = canonical_json_v1(Value::Object(payload)).map_err(storage_contract_error)?;
            if rewrite.payloads.get(&(kind.to_string(), id.clone(), sequence)) != Some(&json) {
                return Err(invalid("replayed projection differs from the rewritten source"));
            }
            let sql = if *kind == "chat" {
                "UPDATE chats SET projection_json=?1 WHERE chat_id=?2 AND last_event_sequence=?3"
            } else {
                "UPDATE runs SET projection_json=?1 WHERE run_id=?2 AND last_event_sequence=?3"
            };
            if transaction.execute(sql, params![json, id, sql_i64(sequence)?]).map_err(sqlite_error)? != 1 {
                return Err(invalid("projection row differs from the replayed sequence"));
            }
        }

        // Durable `redacted` record in the same transaction (ordinary append path).
        let current = projection.projects.get(&commit.project_id)
            .and_then(|record| record.get("last_event_sequence")).and_then(Value::as_u64);
        if current != Some(commit.expected_project_sequence) {
            return Err(SidecarError::new("operationConflict", "project changed before chat erasure"));
        }
        let mut payload = commit.project_payload;
        payload.remove("last_event_sequence");
        let event = LedgerEventV1 {
            event_id: format!("evt_{}", Uuid::new_v4().simple()),
            operation_id: commit.project_operation_id,
            request_id: Some(commit.request_id),
            aggregate_type: "project".to_string(),
            aggregate_id: commit.project_id.clone(),
            aggregate_sequence: commit.expected_project_sequence + 1,
            event_type: "project.upserted".to_string(),
            payload_schema_version: SQLITE_SCHEMA_VERSION,
            canonical_json_payload: canonical_json_v1(Value::Object(payload)).map_err(storage_contract_error)?,
            timestamp: now_utc(),
        };
        if classify_storage_operation(&transaction, &events, &event)? != OperationDecisionV1::Append {
            return Err(SidecarError::new("operationConflict", "erasure record conflicted with history"));
        }
        insert_event(&transaction, &event)?;
        events.push(event.clone());
        projection = replay_storage_ledger(&transaction, &events)?;
        write_projection(&transaction, &event, &projection)?;

        for sql in &triggers {
            transaction.execute_batch(sql).map_err(sqlite_error)?;
        }
        verify_schema_identity_for_erasure(&transaction)?;
        if storage_schema_version(&transaction)? != Some(version) {
            return Err(invalid("schema version changed during erasure"));
        }
        transaction.commit().map_err(sqlite_error)?;
        self.note_maintenance_invalidation();
        Ok(())
    }

    /// Physical step after a committed redaction: freed cells were zeroed by
    /// `secure_delete`; truncate the WAL so pre-erasure page images are gone.
    pub(crate) fn purge_erased_pages(&mut self) -> Result<(), SidecarError> {
        self.ensure_writes_admitted()?;
        let secure: i64 = self.connection
            .query_row("PRAGMA secure_delete;", [], |row| row.get(0))
            .map_err(sqlite_error)?;
        if secure != 1 {
            return Err(invalid("secure_delete is not enabled"));
        }
        let (busy, log, checkpointed): (i64, i64, i64) = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(sqlite_error)?;
        if busy != 0 || log != checkpointed {
            return Err(SidecarError::new("checkpointBusy", "erasure WAL checkpoint did not complete"));
        }
        let mut wal = self.path.clone().into_os_string();
        wal.push("-wal");
        match fs::metadata(&wal) {
            Ok(metadata) if metadata.len() != 0 => {
                Err(SidecarError::new("checkpointBusy", "erasure WAL was not truncated"))
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SidecarError::new("storageReadFailed", error.to_string())),
        }
    }
}
