//! Internal synchronous orchestration candidate. Product IPC activation and
//! worker-thread/cancellation transport are separate integration steps.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// New execution requires a move-only confirmation handoff. There is deliberately
/// no method that resumes source writes from a persisted applying record.
#[allow(dead_code)]
pub(in crate::project) fn apply_confirmed(
    manager: &ProjectManager,
    storage: &mut Storage,
    confirmations: &mut Confirmations,
    prepared: PreparedApplication,
    canceled: &AtomicBool,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if canceled.load(Ordering::Acquire) {
        return Err(invalid());
    }
    let mut current = begin(manager, storage, confirmations, prepared, request)?;
    let project_id = current["binding"]["projectId"]
        .as_str()
        .ok_or_else(invalid)?
        .to_owned();
    let archive = backup::Backup::from_value(&current["backup"]["backup"])?;
    let (header, items) = backup::verify_items(manager, &archive)?;
    if header != current["backup"]["header"] || items.is_empty() {
        return Err(invalid());
    }
    // Reject currently unsupported directory batches before the first source
    // mutation, rather than applying a prefix and discovering this halfway in.
    if items.iter().any(|item| {
        ["before", "after"].iter().any(|side| {
            !item[*side].is_null() && !matches!(item[*side]["kind"].as_u64(), Some(1 | 3))
        })
    }) {
        return Err(ProjectFailure::recovery(
            "directory application is not connected yet; source unchanged",
        ));
    }
    if canceled.load(Ordering::Acquire) {
        return Ok(current);
    }
    current = mark_write_started(manager, storage, &project_id, &current, request)?;
    current = record_first_item(manager, storage, &project_id, &current, request)?;
    for index in 0..items.len() {
        if canceled.load(Ordering::Acquire) {
            return Ok(current);
        }
        current =
            writer::write_first_regular_item(manager, storage, &project_id, &current, request)?;
        if index + 1 < items.len() {
            if canceled.load(Ordering::Acquire) {
                return Ok(current);
            }
            current = advance_regular_item(manager, storage, &project_id, &current, request)?;
        }
    }
    completion::finish(manager, storage, &project_id, &current, false, request)
}

/// Explicit restoration request, tied to the exact displayed durable record.
/// On cancellation/conflict leave the current journal intact, never hide a
/// partially restored batch behind a successful terminal state.
#[allow(dead_code)]
pub(in crate::project) fn restore_selected(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    canceled: &AtomicBool,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect(storage, project_id)? != *expected {
        return Err(invalid());
    }
    if canceled.load(Ordering::Acquire) {
        return Ok(expected.clone());
    }
    let mut current = match expected["state"].as_str() {
        Some("prepared") => return abandon_unwritten(storage, project_id, expected, request),
        Some("applied") => {
            completion::begin_restore(manager, storage, project_id, expected, request)?
        }
        Some("applying") => {
            if expected.get("pendingItem").is_none()
                || (expected["pendingItem"]["index"] == 0
                    && expected["pendingItem"].get("writeJournal").is_none())
            {
                return completion::cancel_before_first_write(
                    manager, storage, project_id, expected, request,
                );
            }
            inspect_pending_item(manager, storage, project_id)?
        }
        _ => return Err(invalid()),
    };
    if current["pendingItem"].get("writeJournal").is_none() {
        current = cancel_unstarted_tail(manager, storage, project_id, &current, request)?;
    }
    if current["pendingItem"]["writeJournal"]["phase"] == "staged" {
        current =
            writer::acknowledge_regular_outcome(manager, storage, project_id, &current, request)?;
    }
    if matches!(
        current["pendingItem"]["writeJournal"]["phase"].as_str(),
        Some("applied" | "restored" | "restoring")
    ) && current.get("restoredItems").is_none()
        && current.get("unstartedFrom").is_none()
        && current["pendingItem"]["index"]
            .as_u64()
            .ok_or_else(invalid)?
            + 1
            < current["backup"]["header"]["count"]
                .as_u64()
                .ok_or_else(invalid)?
    {
        if current["pendingItem"]["writeJournal"]["phase"] == "restoring" {
            current = recover_restoring(manager, storage, project_id, &current, request)?;
        }
        current = cancel_unstarted_tail(manager, storage, project_id, &current, request)?;
    }
    // Bound execution by the archive contract, even if a malformed cursor ever
    // reaches this layer. Every successful pass decreases the pending index.
    for _ in 0..100_000 {
        if canceled.load(Ordering::Acquire) {
            return Ok(current);
        }
        match current["pendingItem"]["writeJournal"]["phase"].as_str() {
            Some("applied") => {
                current = writer::restore_first_regular_item(
                    manager, storage, project_id, &current, request,
                )?;
            }
            Some("restoring") => {
                current = recover_restoring(manager, storage, project_id, &current, request)?;
            }
            Some("restored") => {}
            _ => return Err(invalid()),
        }
        if current["pendingItem"]["index"] == 0 {
            return completion::finish(manager, storage, project_id, &current, true, request);
        }
        current = previous_regular_item(manager, storage, project_id, &current, request)?;
    }
    Err(invalid())
}

fn recover_restoring(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    let outcome = writer::inspect_regular_outcome(manager, storage, project_id)?;
    match outcome["comparison"].as_str() {
        Some("matchesAppliedPair") => {
            writer::restore_first_regular_item(manager, storage, project_id, expected, request)
        }
        Some("matchesUnappliedPair") => {
            writer::acknowledge_regular_outcome(manager, storage, project_id, expected, request)
        }
        _ => Err(invalid()),
    }
}
