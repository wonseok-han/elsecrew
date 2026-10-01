use super::*;

/// Session-only owner; durable records cannot reconstruct write authority.
pub(in crate::project) struct ApplyingBatch {
    current: Value,
    project_id: String,
    request: String,
    halted: bool,
}

#[allow(dead_code)]
impl ApplyingBatch {
    pub(in crate::project) fn start(
        manager: &ProjectManager,
        storage: &mut Storage,
        confirmations: &mut Confirmations,
        prepared: PreparedApplication,
        request: &str,
    ) -> Result<Self, ProjectFailure> {
        let current = begin(manager, storage, confirmations, prepared, request)?;
        let project_id = current["binding"]["projectId"]
            .as_str()
            .ok_or_else(invalid)?
            .to_owned();
        let archive = backup::Backup::from_value(&current["backup"]["backup"])?;
        let (header, items) = backup::verify_items(manager, &archive)?;
        if header != current["backup"]["header"]
            || items.is_empty()
            || items.iter().any(|item| {
                item["before"]["kind"] == 2
                    || (item["after"]["kind"] == 2 && !item["before"].is_null())
            })
        {
            return Err(invalid());
        }
        Ok(Self {
            current,
            project_id,
            request: request.to_owned(),
            halted: false,
        })
    }

    /// At most one file writer per tick. Copy chunks observe the supplied flag;
    /// archive reads remain synchronous. Failures permanently stop this owner.
    pub(in crate::project) fn advance(
        &mut self,
        manager: &ProjectManager,
        storage: &mut Storage,
        canceled: &AtomicBool,
    ) -> Result<bool, ProjectFailure> {
        if self.halted {
            return Err(invalid());
        }
        self.halted = true;
        if inspect(storage, &self.project_id)? != self.current {
            return Err(invalid());
        }
        if canceled.load(Ordering::Acquire) {
            return Ok(true);
        }
        let next = if self.current["state"] == "prepared" {
            let writing = mark_write_started(
                manager,
                storage,
                &self.project_id,
                &self.current,
                &self.request,
            )?;
            record_first_item(manager, storage, &self.project_id, &writing, &self.request)?
        } else if self.current["state"] == "applying" {
            let pending = &self.current["pendingItem"];
            if pending.get("writeJournal").is_none() {
                writer::write_item_cancellable(
                    manager,
                    storage,
                    &self.project_id,
                    &self.current,
                    &self.request,
                    canceled,
                )?
            } else if pending["writeJournal"]["phase"] == "applied" {
                if pending["index"].as_u64().ok_or_else(invalid)? + 1
                    == self.current["backup"]["header"]["count"]
                        .as_u64()
                        .ok_or_else(invalid)?
                {
                    self.current = completion::finish(
                        manager,
                        storage,
                        &self.project_id,
                        &self.current,
                        false,
                        &self.request,
                    )?;
                    return Ok(true);
                }
                advance_regular_item(
                    manager,
                    storage,
                    &self.project_id,
                    &self.current,
                    &self.request,
                )?
            } else {
                return Err(invalid());
            }
        } else {
            return Err(invalid());
        };
        self.current = next;
        self.halted = false;
        Ok(false)
    }

    /// Last successful step only. After failure inspect the durable recovery
    /// record; a failed filesystem/DB transition may have advanced beyond this.
    pub(in crate::project) fn record(&self) -> &Value {
        &self.current
    }
}
