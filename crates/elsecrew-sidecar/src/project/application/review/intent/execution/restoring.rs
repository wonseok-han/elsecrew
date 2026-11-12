use super::*;

/// Explicit restoration of one exact durable record, never write resumption.
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
        if self.halted || self.steps >= 400_010 {
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
        let current = &self.current;
        let project_id = &self.project_id;
        let request = &self.request;
        let next = match current["state"].as_str() {
            Some("prepared") => abandon_unwritten(storage, project_id, current, request)?,
            Some("applied") => {
                completion::begin_restore(manager, storage, project_id, current, request)?
            }
            Some("applying") => {
                let pending = &current["pendingItem"];
                if current.get("pendingItem").is_none()
                    || (pending["index"] == 0 && pending.get("writeJournal").is_none())
                {
                    completion::cancel_before_first_write(
                        manager, storage, project_id, current, request,
                    )?
                } else if pending.get("writeJournal").is_none() {
                    cancel_unstarted_tail(manager, storage, project_id, current, request)?
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
                        "reserved" => writer::cancel_reserved_staging(
                            manager, storage, project_id, current, request,
                        )?,
                        "staged" => writer::acknowledge_regular_outcome(
                            manager, storage, project_id, current, request,
                        )?,
                        "restoring" => {
                            recover_restoring(manager, storage, project_id, current, request)?
                        }
                        "applied" | "restored" if tail => {
                            cancel_unstarted_tail(manager, storage, project_id, current, request)?
                        }
                        "applied" => writer::restore_first_regular_item(
                            manager, storage, project_id, current, request,
                        )?,
                        "restored" if pending["index"] == 0 => completion::finish(
                            manager, storage, project_id, current, true, request,
                        )?,
                        "restored" => {
                            previous_regular_item(manager, storage, project_id, current, request)?
                        }
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
