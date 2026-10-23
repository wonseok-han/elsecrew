//! Reused by append and reopen verification. One indexed predecessor per item;
//! no unbounded map of all previously seen item IDs or text bodies.
use super::*;
use hello_agent_protocol::agent_adapter_v1::{AgentPublicEventV1, output_pages::OutputItemOperation};

pub(super) fn verify(
    tx: &Transaction<'_>, binding: &AgentRunBindingV1, sequence: u64,
    event: &AgentPublicEventV1, update: Option<&OutputItemUpdate>,
) -> Result<(), SidecarError> {
    let Some(update) = update else { return Ok(()); };
    update.validate(event).map_err(|_| invalid("invalid output item update"))?;
    let prior: Option<(String, String)> = tx.query_row(
        "SELECT CASE WHEN length(CAST(event_json AS BLOB))<=524288 THEN event_json ELSE NULL END, CASE WHEN length(CAST(item_update_json AS BLOB))<=2048 THEN item_update_json ELSE NULL END FROM run_output_records WHERE run_id=?1 AND json_extract(item_update_json,'$.itemId')=?2 AND output_sequence<?3 ORDER BY output_sequence DESC LIMIT 1",
        params![binding.run_id, update.item_id, sql_integer(sequence)?],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(sqlite_error)?;
    let Some((body, metadata)) = prior else {
        return if event.kind == "final" { Err(invalid("final output item has no completed predecessor")) } else { Ok(()) };
    };
    let value: Value = serde_json::from_str(&body).map_err(|_| invalid("invalid predecessor event"))?;
    let previous = validate_agent_public_event_against_run_v1(&value, binding)
        .map_err(|_| invalid("predecessor event binding differs"))?;
    let prior: OutputItemUpdate = serde_json::from_str(&metadata).map_err(|_| invalid("invalid predecessor item"))?;
    prior.validate(&previous).map_err(|_| invalid("invalid predecessor item contract"))?;
    if prior.item_id != update.item_id || prior.channel != update.channel {
        return Err(invalid("output item identity or channel changed"));
    }
    if prior.operation == OutputItemOperation::Complete {
        // Exactly one final promotion of the already-completed public body.
        if update.operation != OutputItemOperation::Complete || previous.kind == "final"
            || event.kind != "final" || previous.text != event.text {
            return Err(invalid("output changed after item completion"));
        }
    } else if event.kind == "final" {
        return Err(invalid("final output item is not completed"));
    }
    Ok(())
}
