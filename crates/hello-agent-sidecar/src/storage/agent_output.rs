//! Transaction-scoped output ledger. No migration or public runtime activation.
//! Caller must hold write admission and atomically persist the run/fence state.
use super::{row_u64, sqlite_error, SidecarError};
use hello_agent_protocol::agent_adapter_v1::{
    output_pages::{
        decode_output_page, OutputHead, OutputPage, OutputPageRequest, OutputRow,
        OUTPUT_FORMAT_VERSION, OUTPUT_PAGE_MAX_BYTES,
    },
    validate_agent_public_event_against_run_v1, AgentRunBindingV1,
};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;

pub(super) mod schema;
pub(super) mod verification;

impl super::Storage {
    /// Output mutations use the same admitted connection and transaction as
    /// lifecycle storage. This is not a public IPC or provider authorization API.
    pub(crate) fn append_agent_output_record(
        &mut self,
        binding: &AgentRunBindingV1,
        sequence: u64,
        event: &Value,
        text_budget: u64,
    ) -> Result<(), SidecarError> {
        self.ensure_writes_admitted()?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        require_output_schema(&tx)?;
        super::verify_append_runtime_state(&tx)?;
        let current = head(&tx, binding)?;
        verify_run_binding(&tx, &current)?;
        append(&tx, binding, sequence, event, text_budget)?;
        tx.commit().map_err(sqlite_error)?;
        self.note_maintenance_invalidation();
        Ok(())
    }

    pub(crate) fn agent_output_head(
        &mut self,
        binding: &AgentRunBindingV1,
    ) -> Result<OutputHead, SidecarError> {
        self.ensure_writes_admitted()?;
        let tx = self.connection.transaction().map_err(sqlite_error)?;
        require_output_schema(&tx)?;
        let result = head(&tx, binding)?;
        verify_run_binding(&tx, &result)?;
        tx.commit().map_err(sqlite_error)?;
        Ok(result)
    }

    pub(crate) fn agent_output_page(
        &mut self,
        request: &OutputPageRequest,
    ) -> Result<OutputPage, SidecarError> {
        self.ensure_writes_admitted()?;
        let tx = self.connection.transaction().map_err(sqlite_error)?;
        require_output_schema(&tx)?;
        let current = head(&tx, &request.binding)?;
        verify_run_binding(&tx, &current)?;
        let result = page(&tx, request)?;
        tx.commit().map_err(sqlite_error)?;
        Ok(result)
    }
}

fn require_output_schema(connection: &rusqlite::Connection) -> Result<(), SidecarError> {
    if super::storage_schema_version(connection)? != Some(schema::VERSION) {
        return Err(invalid("durable output requires storage schema v11"));
    }
    Ok(())
}

/// Called after the run projection is written, inside that same transaction.
/// A failed output transition rolls back the run/lock updates as well.
pub(super) fn sync_run_projection(
    tx: &Transaction<'_>,
    projection: &serde_json::Map<String, Value>,
    run_id: &str,
    event_sequence: u64,
) -> Result<(), SidecarError> {
    use hello_agent_protocol::agent_adapter_v1::{
        validate_agent_adapter_payload_v1, AgentRunRecordV1,
    };
    let format = projection.get("output_format_version");
    if super::storage_schema_version(tx)? != Some(schema::VERSION) {
        if format.is_some() {
            return Err(invalid("new output run requires schema v11"));
        }
        return Ok(());
    }
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM run_output_heads WHERE run_id=?1)",
            [run_id],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    let Some(format) = format else {
        if exists {
            return Err(invalid("durable output run cannot revert to legacy format"));
        }
        return Ok(());
    };
    if format != OUTPUT_FORMAT_VERSION {
        return Err(invalid("unsupported output format"));
    }
    let value = projection
        .get("record")
        .ok_or_else(|| invalid("missing output run record"))?;
    validate_agent_adapter_payload_v1("runRecord", value)
        .map_err(|_| invalid("invalid output run record"))?;
    let record: AgentRunRecordV1 =
        serde_json::from_value(value.clone()).map_err(|_| invalid("invalid output run record"))?;
    let binding = AgentRunBindingV1 {
        chat_id: record.chat_id,
        task_id: record.task_id,
        project_id: record.project_id,
        worktree_id: record.worktree_id,
        workspace_identity: record.workspace_identity,
        run_id: record.run_id,
        role_id: record.selected_profile.role_id,
        capability_fingerprint: record.capability_fingerprint,
    };
    if binding.run_id != run_id {
        return Err(invalid("output run key mismatch"));
    }
    if !exists {
        if event_sequence != 1 {
            return Err(invalid("existing run is missing output head"));
        }
        begin(tx, &binding)?;
    }
    let current = head(tx, &binding)?;
    let active = matches!(
        record.state.as_str(),
        "preparing" | "running" | "cancelling"
    );
    if current.sealed && active {
        return Err(invalid("sealed output run cannot resume"));
    }
    if !active {
        seal(tx, &binding)?;
    }
    verify_run_binding(tx, &head(tx, &binding)?)
}

/// New output format is opt-in per run; a legacy array is never silently
/// reinterpreted as this ledger. Existing lifecycle/fence checks remain required.
fn verify_run_binding(tx: &Transaction<'_>, output: &OutputHead) -> Result<(), SidecarError> {
    use hello_agent_protocol::agent_adapter_v1::{
        validate_agent_adapter_payload_v1, AgentRunRecordV1,
    };
    let row: Option<(String, String)> = tx.query_row(
        "SELECT state, projection_json FROM runs WHERE run_id=?1 AND length(CAST(projection_json AS BLOB))<=1048576 AND length(CAST(state AS BLOB))<=128",
        [&output.binding.run_id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(sqlite_error)?;
    let (state, json) = row.ok_or_else(|| invalid("output run missing or oversized"))?;
    let projection: Value =
        serde_json::from_str(&json).map_err(|_| invalid("invalid output run projection"))?;
    if projection["source_kind"] != "actualAgent"
        || projection["output_format_version"] != OUTPUT_FORMAT_VERSION
        || projection.get("public_events").is_some()
        || projection["run_id"].as_str() != Some(&output.binding.run_id)
    {
        return Err(invalid("run is not a durable output run"));
    }
    validate_agent_adapter_payload_v1("runRecord", &projection["record"])
        .map_err(|_| invalid("invalid output run record"))?;
    let record: AgentRunRecordV1 = serde_json::from_value(projection["record"].clone())
        .map_err(|_| invalid("invalid output run record"))?;
    let binding = AgentRunBindingV1 {
        chat_id: record.chat_id,
        task_id: record.task_id,
        project_id: record.project_id,
        worktree_id: record.worktree_id,
        workspace_identity: record.workspace_identity,
        run_id: record.run_id,
        role_id: record.selected_profile.role_id,
        capability_fingerprint: record.capability_fingerprint,
    };
    if binding != output.binding || state != record.state || projection["state"] != state {
        return Err(invalid("output run identity or state mismatch"));
    }
    if !matches!(state.as_str(), "preparing" | "running" | "cancelling") && !output.sealed {
        return Err(invalid("terminal run output must be sealed"));
    }
    if let Some(value) = projection.get("provider_run_fence") {
        let fence = crate::provider_login_process::RunFenceBinding::from_value(value)?;
        fence.validate(&binding.run_id, &record.selected_profile.runtime)?;
    }
    Ok(())
}

pub(super) const SCHEMA: &str = "
CREATE TABLE run_output_heads (
 run_id TEXT PRIMARY KEY NOT NULL,
 binding_json TEXT NOT NULL,
 committed_sequence INTEGER NOT NULL CHECK(committed_sequence >= 0),
 text_bytes INTEGER NOT NULL CHECK(text_bytes >= 0),
 sealed INTEGER NOT NULL CHECK(sealed IN (0,1))
) STRICT;
CREATE TABLE run_output_records (
 run_id TEXT NOT NULL REFERENCES run_output_heads(run_id),
 output_sequence INTEGER NOT NULL CHECK(output_sequence > 0),
 event_id TEXT NOT NULL UNIQUE,
 event_json TEXT NOT NULL CHECK(length(CAST(event_json AS BLOB)) <= 524288),
 PRIMARY KEY(run_id, output_sequence)
) STRICT;";

fn invalid(detail: &str) -> SidecarError {
    SidecarError::new("storageInvalid", detail)
}
fn encode<T: serde::Serialize>(value: &T) -> Result<String, SidecarError> {
    serde_json::to_string(value).map_err(|_| invalid("output serialization failed"))
}

fn sql_integer(value: u64) -> Result<i64, SidecarError> {
    i64::try_from(value).map_err(|_| invalid("output integer exceeds SQLite range"))
}

pub(super) fn begin(tx: &Transaction<'_>, binding: &AgentRunBindingV1) -> Result<(), SidecarError> {
    let head = OutputHead {
        format_version: OUTPUT_FORMAT_VERSION,
        binding: binding.clone(),
        committed_sequence: 0,
        sealed: false,
    };
    head.validate(binding)
        .map_err(|_| invalid("invalid output binding"))?;
    tx.execute(
        "INSERT INTO run_output_heads VALUES (?1,?2,0,0,0)",
        params![binding.run_id, encode(binding)?],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

pub(super) fn head(
    tx: &Transaction<'_>,
    binding: &AgentRunBindingV1,
) -> Result<OutputHead, SidecarError> {
    let row: Option<(String, u64, bool)> = tx.query_row(
        "SELECT binding_json, committed_sequence, sealed FROM run_output_heads WHERE run_id=?1 AND length(CAST(binding_json AS BLOB)) <= 8192",
        [&binding.run_id], |row| Ok((row.get(0)?, row_u64(row, 1)?, row.get(2)?)),
    ).optional().map_err(sqlite_error)?;
    let (json, committed_sequence, sealed) =
        row.ok_or_else(|| invalid("missing or oversized output head"))?;
    let result = OutputHead {
        format_version: OUTPUT_FORMAT_VERSION,
        binding: serde_json::from_str(&json)
            .map_err(|_| invalid("invalid stored output binding"))?,
        committed_sequence,
        sealed,
    };
    result
        .validate(binding)
        .map_err(|_| invalid("output head binding mismatch"))?;
    Ok(result)
}

/// No commit here: an error must roll back the caller's entire transaction.
/// Sequence/event ID conflicts are rejected, never silently treated as success.
pub(super) fn append(
    tx: &Transaction<'_>,
    binding: &AgentRunBindingV1,
    sequence: u64,
    value: &Value,
    text_budget: u64,
) -> Result<(), SidecarError> {
    let previous = head(tx, binding)?;
    if previous.sealed
        || previous.committed_sequence.checked_add(1) != Some(sequence)
        || sequence > hello_agent_protocol::MAX_SEQUENCE
    {
        return Err(invalid("output append sequence or sealed state mismatch"));
    }
    let event = validate_agent_public_event_against_run_v1(value, binding)
        .map_err(|_| invalid("invalid public output event"))?;
    let bytes = u64::try_from(event.text.len()).map_err(|_| invalid("output length overflow"))?;
    let used: u64 = tx
        .query_row(
            "SELECT text_bytes FROM run_output_heads WHERE run_id=?1",
            [&binding.run_id],
            |row| row_u64(row, 0),
        )
        .map_err(sqlite_error)?;
    let next = used
        .checked_add(bytes)
        .filter(|total| *total <= text_budget)
        .ok_or_else(|| invalid("output text budget exceeded"))?;
    let json = encode(&event)?;
    // Ensure a valid event fits as a single page before committing it.
    let request = OutputPageRequest {
        format_version: OUTPUT_FORMAT_VERSION,
        binding: binding.clone(),
        after_sequence: sequence - 1,
        through_sequence: sequence,
        limit: 1,
    };
    let single = OutputPage {
        request: request.clone(),
        rows: vec![OutputRow {
            output_sequence: sequence,
            event,
        }],
        next_after_sequence: sequence,
    };
    decode_output_page(encode(&single)?.as_bytes(), &request)
        .map_err(|_| invalid("output event exceeds page budget"))?;
    tx.execute(
        "INSERT INTO run_output_records VALUES (?1,?2,?3,?4)",
        params![
            binding.run_id,
            sql_integer(sequence)?,
            single.rows[0].event.event_id,
            json
        ],
    )
    .map_err(sqlite_error)?;
    let updated = tx.execute("UPDATE run_output_heads SET committed_sequence=?2, text_bytes=?3 WHERE run_id=?1 AND committed_sequence=?4 AND sealed=0", params![binding.run_id, sql_integer(sequence)?, sql_integer(next)?, sql_integer(previous.committed_sequence)?]).map_err(sqlite_error)?;
    if updated != 1 {
        return Err(invalid("output head update conflict"));
    }
    Ok(())
}

pub(super) fn seal(tx: &Transaction<'_>, binding: &AgentRunBindingV1) -> Result<(), SidecarError> {
    head(tx, binding)?;
    tx.execute(
        "UPDATE run_output_heads SET sealed=1 WHERE run_id=?1",
        [&binding.run_id],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

pub(super) fn page(
    tx: &Transaction<'_>,
    request: &OutputPageRequest,
) -> Result<OutputPage, SidecarError> {
    request
        .validate()
        .map_err(|_| invalid("invalid output page request"))?;
    let current = head(tx, &request.binding)?;
    if request.through_sequence > current.committed_sequence {
        return Err(invalid("output cursor exceeds committed head"));
    }
    let mut statement = tx.prepare("SELECT output_sequence, length(CAST(event_json AS BLOB)), CASE WHEN length(CAST(event_json AS BLOB)) <= 524288 THEN event_json ELSE NULL END, CASE WHEN length(CAST(event_id AS BLOB)) <= 8192 THEN event_id ELSE NULL END FROM run_output_records WHERE run_id=?1 AND output_sequence>?2 AND output_sequence<=?3 ORDER BY output_sequence LIMIT ?4").map_err(sqlite_error)?;
    let mut rows = statement
        .query(params![
            request.binding.run_id,
            sql_integer(request.after_sequence)?,
            sql_integer(request.through_sequence)?,
            request.limit as i64
        ])
        .map_err(sqlite_error)?;
    let mut result = OutputPage {
        request: request.clone(),
        rows: Vec::new(),
        next_after_sequence: request.after_sequence,
    };
    while let Some(row) = rows.next().map_err(sqlite_error)? {
        let sequence = row_u64(row, 0).map_err(sqlite_error)?;
        let length = row_u64(row, 1).map_err(sqlite_error)?;
        if length > OUTPUT_PAGE_MAX_BYTES as u64 {
            return Err(invalid("oversized stored output event"));
        }
        let json: String = row.get(2).map_err(sqlite_error)?;
        let event: hello_agent_protocol::agent_adapter_v1::AgentPublicEventV1 =
            serde_json::from_str(&json).map_err(|_| invalid("invalid stored output event"))?;
        let event_id: String = row.get(3).map_err(sqlite_error)?;
        if event_id != event.event_id {
            return Err(invalid("stored output event id mismatch"));
        }
        let previous_sequence = result.next_after_sequence;
        result.rows.push(OutputRow {
            output_sequence: sequence,
            event,
        });
        result.next_after_sequence = sequence;
        if encode(&result)?.len() > OUTPUT_PAGE_MAX_BYTES {
            result.rows.pop();
            result.next_after_sequence = previous_sequence;
            break;
        }
    }
    decode_output_page(encode(&result)?.as_bytes(), request)
        .map_err(|_| invalid("invalid stored output page"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fixture() -> OutputPage {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/m5/agent-output-page.json"
        ))
        .unwrap()
    }

    #[test]
    fn append_pages_and_seal_in_one_transaction() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        let fixture = fixture();
        let binding = fixture.request.binding;
        let tx = db.transaction().unwrap();
        begin(&tx, &binding).unwrap();
        for sequence in 1..=3000 {
            let mut event = fixture.rows[0].event.clone();
            event.event_id = format!("agent_event_{sequence}");
            append(
                &tx,
                &binding,
                sequence,
                &serde_json::to_value(event).unwrap(),
                64 * 1024 * 1024,
            )
            .unwrap();
        }
        seal(&tx, &binding).unwrap();
        tx.commit().unwrap();
        let tx = db.transaction().unwrap();
        assert!(head(&tx, &binding).unwrap().sealed);
        for after in (0..3000).step_by(100) {
            let request = OutputPageRequest {
                format_version: 1,
                binding: binding.clone(),
                after_sequence: after,
                through_sequence: 3000,
                limit: 100,
            };
            assert_eq!(
                page(&tx, &request).unwrap().next_after_sequence,
                after + 100
            );
        }
        assert!(append(
            &tx,
            &binding,
            3001,
            &serde_json::to_value(&fixture.rows[0].event).unwrap(),
            64 * 1024 * 1024
        )
        .is_err());
    }

    #[test]
    fn rollback_removes_output_and_head_together() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        let fixture = fixture();
        let binding = fixture.request.binding;
        {
            let tx = db.transaction().unwrap();
            begin(&tx, &binding).unwrap();
            append(
                &tx,
                &binding,
                1,
                &serde_json::to_value(&fixture.rows[0].event).unwrap(),
                1024,
            )
            .unwrap();
            // Deliberate operation abort; neither canonical row nor head survives.
        }
        let records: u64 = db
            .query_row("SELECT count(*) FROM run_output_records", [], |row| {
                row_u64(row, 0)
            })
            .unwrap();
        let heads: u64 = db
            .query_row("SELECT count(*) FROM run_output_heads", [], |row| {
                row_u64(row, 0)
            })
            .unwrap();
        assert_eq!((records, heads), (0, 0));
    }
}
