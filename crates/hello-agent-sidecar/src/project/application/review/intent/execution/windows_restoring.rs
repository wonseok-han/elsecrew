//! Explicit Windows restore owner. Durable records are inspected, not replayed
//! as an apply session. Unresolved/unsupported stages keep their recovery fence.
use super::*;

pub(in crate::project) struct RestoringBatch {
    current: Value,
    project_id: String,
    request: String,
    halted: bool,
    steps: usize,
}
impl RestoringBatch {
    pub(in crate::project) fn start(
        storage: &Storage,
        project_id: &str,
        expected: &Value,
        request: &str,
    ) -> Result<Self, ProjectFailure> {
        if inspect(storage, project_id)? != *expected
            || !matches!(
                expected["state"].as_str(),
                Some("prepared" | "applying" | "applied")
            )
        {
            return Err(invalid());
        }
        Ok(Self {
            current: expected.clone(),
            project_id: project_id.into(),
            request: request.into(),
            halted: false,
            steps: 0,
        })
    }
    pub(in crate::project) fn record(&self) -> &Value {
        &self.current
    }
    pub(in crate::project) fn advance(
        &mut self,
        manager: &ProjectManager,
        storage: &mut Storage,
        canceled: &AtomicBool,
    ) -> Result<bool, ProjectFailure> {
        // At most a bounded number of transitions for 100,000 archive entries.
        if self.halted || self.steps >= 800_020 {
            return Err(invalid());
        }
        self.halted = true;
        if inspect(storage, &self.project_id)? != self.current {
            return Err(invalid());
        }
        if canceled.load(Ordering::Acquire) {
            return Ok(true);
        }
        self.steps += 1;
        let check = || {
            if canceled.load(Ordering::Acquire) {
                Err(invalid())
            } else {
                Ok(())
            }
        };
        let current = &self.current;
        let id = &self.project_id;
        let request = &self.request;
        let next = match current["state"].as_str() {
            Some("prepared") => abandon_unwritten(storage, id, current, request)?,
            Some("applied") => completion::begin_restore(manager, storage, id, current, request)?,
            Some("applying") => {
                let pending = &current["pendingItem"];
                if current.get("pendingItem").is_none()
                    || (pending["index"] == 0 && pending.get("writeJournal").is_none())
                {
                    completion::cancel_before_first_write(manager, storage, id, current, request)?
                } else if pending.get("writeJournal").is_none() {
                    cancel_unstarted_tail(manager, storage, id, current, request)?
                } else {
                    let phase = pending["writeJournal"]["phase"]
                        .as_str()
                        .ok_or_else(invalid)?;
                    let tail = current.get("restoredItems").is_none()
                        && current.get("unstartedFrom").is_none()
                        && pending["index"].as_u64().ok_or_else(invalid)? + 1
                            < current["backup"]["header"]["count"]
                                .as_u64()
                                .ok_or_else(invalid)?;
                    match phase {
                        "applied" | "restored" if tail => {
                            cancel_unstarted_tail(manager, storage, id, current, request)?
                        }
                        "reserved" => writer::recovery::cancel_reserved_staging(
                            manager, storage, id, current, request, &check,
                        )?,
                        "staged" => writer::recovery::cancel_unwritten_item(
                            manager, storage, id, current, request, &check,
                        )?,
                        "beforeMoved" | "cancelReady" => writer::move_file(
                            manager,
                            storage,
                            id,
                            current,
                            writer::Move::CancelStaged,
                            request,
                            &check,
                        )?,
                        "applied" => writer::move_file(
                            manager,
                            storage,
                            id,
                            current,
                            writer::Move::DetachAfter,
                            request,
                            &check,
                        )?,
                        "afterDetached" => writer::move_file(
                            manager,
                            storage,
                            id,
                            current,
                            writer::Move::RestoreBefore,
                            request,
                            &check,
                        )?,
                        "moveBeforeIntent" | "installIntent" => {
                            let observed = writer::recovery::inspect_interrupted_move(
                                manager, storage, id, current, &check,
                            )?;
                            match observed["comparison"].as_str() {
                                Some("matchesMoved") => {
                                    writer::recovery::acknowledge_interrupted_move(
                                        manager, storage, id, current, request, &check,
                                    )?
                                }
                                Some("matchesUnmoved") if phase == "moveBeforeIntent" => {
                                    writer::recovery::cancel_unwritten_item(
                                        manager, storage, id, current, request, &check,
                                    )?
                                }
                                Some("matchesUnmoved") => writer::recovery::cancel_unmoved_install(
                                    manager, storage, id, current, request, &check,
                                )?,
                                _ => return Err(invalid()),
                            }
                        }
                        "detachAfterIntent" | "cancelStagedIntent" | "restoreBeforeIntent" => {
                            let observed = writer::recovery::inspect_interrupted_move(
                                manager, storage, id, current, &check,
                            )?;
                            match observed["comparison"].as_str() {
                                Some("matchesMoved") => {
                                    writer::recovery::acknowledge_interrupted_move(
                                        manager, storage, id, current, request, &check,
                                    )?
                                }
                                Some("matchesUnmoved") => {
                                    writer::recovery::resume_interrupted_restore(
                                        manager, storage, id, current, request, &check,
                                    )?
                                }
                                _ => return Err(invalid()),
                            }
                        }
                        "restored" if pending["index"] == 0 => {
                            completion::finish(manager, storage, id, current, true, request)?
                        }
                        "restored" => {
                            previous_regular_item(manager, storage, id, current, request)?
                        }
                        // Unknown phases are not authority for recovery writes.
                        _ => return Err(invalid()),
                    }
                }
            }
            _ => return Err(invalid()),
        };
        self.current = next;
        if matches!(
            self.current["state"].as_str(),
            Some("restored" | "abandoned")
        ) {
            return Ok(true);
        }
        self.halted = false;
        Ok(false)
    }
}
