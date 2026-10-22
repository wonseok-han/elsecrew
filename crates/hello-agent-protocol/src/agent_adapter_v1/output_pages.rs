//! Draft long-run page contract. Not advertised as a runtime capability yet.
//! Durable output sequences are independent of transport envelope sequences.
use super::{
    validate_agent_public_event_against_run_v1, validate_run_binding, AgentAdapterContractError,
    AgentPublicEventV1, AgentRunBindingV1,
};
use crate::MAX_SEQUENCE;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const OUTPUT_PAGE_MAX_ITEMS: usize = 100;
// A 64 KiB public text can expand sixfold when JSON escaped. Leave room for
// identifiers and page metadata, while staying below the 1 MiB control frame.
pub const OUTPUT_PAGE_MAX_BYTES: usize = 512 * 1024;
pub const OUTPUT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OutputPageRequest {
    pub format_version: u32,
    pub binding: AgentRunBindingV1,
    pub after_sequence: u64,
    pub through_sequence: u64,
    pub limit: usize,
}

impl OutputPageRequest {
    pub fn validate(&self) -> Result<(), AgentAdapterContractError> {
        validate_run_binding(&self.binding)?;
        if self.format_version != OUTPUT_FORMAT_VERSION
            || self.after_sequence > self.through_sequence
            || self.through_sequence > MAX_SEQUENCE
            || self.limit == 0
            || self.limit > OUTPUT_PAGE_MAX_ITEMS
        {
            return Err(AgentAdapterContractError::new(
                "invalid output page request",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OutputRow {
    pub output_sequence: u64,
    pub event: AgentPublicEventV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OutputPage {
    pub request: OutputPageRequest,
    pub rows: Vec<OutputRow>,
    pub next_after_sequence: u64,
}

/// Decode only a bounded payload, then verify exact request binding and a
/// contiguous durable prefix. Cross-page event-ID uniqueness belongs to the
/// durable ledger; this validator never accumulates all previous output.
/// The storage reader must independently verify through_sequence is committed.
pub fn decode_output_page(
    bytes: &[u8],
    expected: &OutputPageRequest,
) -> Result<OutputPage, AgentAdapterContractError> {
    expected.validate()?;
    if bytes.len() > OUTPUT_PAGE_MAX_BYTES {
        return Err(AgentAdapterContractError::new(
            "output page byte limit exceeded",
        ));
    }
    let page: OutputPage = serde_json::from_slice(bytes)
        .map_err(|_| AgentAdapterContractError::new("invalid output page encoding"))?;
    if &page.request != expected || page.rows.len() > expected.limit {
        return Err(AgentAdapterContractError::new(
            "output page request mismatch",
        ));
    }
    let mut sequence = expected.after_sequence;
    let mut ids = HashSet::new();
    for row in &page.rows {
        if sequence == expected.through_sequence || row.output_sequence != sequence + 1 {
            return Err(AgentAdapterContractError::new("noncontiguous output page"));
        }
        let value = serde_json::to_value(&row.event)
            .map_err(|_| AgentAdapterContractError::new("invalid output event encoding"))?;
        validate_agent_public_event_against_run_v1(&value, &expected.binding)?;
        if !ids.insert(&row.event.event_id) {
            return Err(AgentAdapterContractError::new("duplicate output event id"));
        }
        sequence = row.output_sequence;
    }
    if page.next_after_sequence != sequence
        || (page.rows.is_empty() && sequence != expected.through_sequence)
    {
        return Err(AgentAdapterContractError::new(
            "output page made invalid progress",
        ));
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> AgentPublicEventV1 {
        let envelope: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/m5/protocol-v1/valid/agent-public-event.jsonl"
        ))
        .unwrap();
        serde_json::from_value(envelope["payload"].clone()).unwrap()
    }

    fn request(after: u64, through: u64) -> OutputPageRequest {
        OutputPageRequest {
            format_version: OUTPUT_FORMAT_VERSION,
            binding: super::super::binding_from_event(&event()),
            after_sequence: after,
            through_sequence: through,
            limit: OUTPUT_PAGE_MAX_ITEMS,
        }
    }

    fn page(request: OutputPageRequest, count: u64) -> OutputPage {
        let rows = (1..=count)
            .map(|offset| {
                let sequence = request.after_sequence + offset;
                let mut event = event();
                event.event_id = format!("agent_event_{sequence}");
                OutputRow {
                    output_sequence: sequence,
                    event,
                }
            })
            .collect();
        let next_after_sequence = request.after_sequence + count;
        OutputPage {
            request,
            rows,
            next_after_sequence,
        }
    }

    fn decode(
        page: &OutputPage,
        request: &OutputPageRequest,
    ) -> Result<OutputPage, AgentAdapterContractError> {
        decode_output_page(&serde_json::to_vec(page).unwrap(), request)
    }

    #[test]
    fn three_thousand_events_in_bounded_pages() {
        for after in (0..3000).step_by(100) {
            let request = request(after, 3000);
            assert_eq!(
                decode(&page(request.clone(), 100), &request)
                    .unwrap()
                    .next_after_sequence,
                after + 100
            );
        }
        let request = request(3000, 3000);
        assert!(decode(&page(request.clone(), 0), &request).is_ok());
    }

    #[test]
    fn rejects_gaps_duplicates_cross_run_and_false_progress() {
        let request = request(0, 3);
        let valid = page(request.clone(), 2);
        let mut bad = valid.clone();
        bad.rows[1].output_sequence = 3;
        assert!(decode(&bad, &request).is_err());
        let mut bad = valid.clone();
        bad.rows[1].event.event_id = bad.rows[0].event.event_id.clone();
        assert!(decode(&bad, &request).is_err());
        let mut bad = valid.clone();
        bad.rows[0].event.run_id.push_str("_other");
        assert!(decode(&bad, &request).is_err());
        let mut bad = valid;
        bad.next_after_sequence = 3;
        assert!(decode(&bad, &request).is_err());
        assert!(decode(&page(request.clone(), 0), &request).is_err());
    }

    #[test]
    fn accepts_one_maximum_escaped_text_without_splitting_event() {
        let request = request(0, 1);
        let mut page = page(request.clone(), 1);
        page.rows[0].event.text = "\u{0001}".repeat(64 * 1024);
        let encoded = serde_json::to_vec(&page).unwrap();
        assert!(encoded.len() > 256 * 1024);
        assert!(decode_output_page(&encoded, &request).is_ok());
    }

    #[test]
    fn rejects_invalid_cursor_and_oversize_before_decode() {
        assert!(request(4, 3).validate().is_err());
        assert!(request(0, MAX_SEQUENCE + 1).validate().is_err());
        let request = request(0, 3);
        assert!(decode_output_page(&vec![b' '; OUTPUT_PAGE_MAX_BYTES + 1], &request).is_err());
        let mut bad = page(request.clone(), 1);
        bad.request.through_sequence = 2;
        assert!(decode(&bad, &request).is_err());
        assert!(decode(&page(request.clone(), 4), &request).is_err());
    }
}
