//! Internal synchronous orchestration candidate. Product IPC activation and
//! worker-thread/cancellation transport are separate integration steps.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
mod applying;
mod restoring;
pub(in crate::project) use applying::ApplyingBatch;
pub(in crate::project) use restoring::RestoringBatch;

pub(in crate::project) enum ApplicationJob {
    Apply(ApplyingBatch),
    Restore(RestoringBatch),
}

impl ApplicationJob {
    pub(in crate::project) fn record(&self) -> &Value {
        match self {
            Self::Apply(owner) => owner.record(),
            Self::Restore(owner) => owner.record(),
        }
    }
    pub(in crate::project) fn advance(
        &mut self,
        manager: &ProjectManager,
        storage: &mut Storage,
        canceled: &AtomicBool,
    ) -> Result<bool, ProjectFailure> {
        match self {
            Self::Apply(owner) => owner.advance(manager, storage, canceled),
            Self::Restore(owner) => owner.advance(manager, storage, canceled),
        }
    }
}

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
    let mut owner = ApplyingBatch::start(manager, storage, confirmations, prepared, request)?;
    while !owner.advance(manager, storage, canceled)? {}
    Ok(owner.record().clone())
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
    let mut owner = RestoringBatch::start(storage, project_id, expected, request)?;
    while !owner.advance(manager, storage, canceled)? {}
    Ok(owner.record().clone())
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
