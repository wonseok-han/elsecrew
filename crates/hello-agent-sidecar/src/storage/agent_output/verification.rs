//! Bounded single-run output consistency check, not full storage admission.
//! Borrowing the connection excludes same-connection writes between steps.
use super::*;
use rusqlite::{Connection, TransactionBehavior};

pub(in crate::storage) struct OutputVerification<'a> {
    connection: &'a mut Connection,
    version: i64,
    expected: OutputHead,
    expected_bytes: u64,
    after: u64,
    text_bytes: u64,
    usable: bool,
    finished: bool,
}

fn version(connection: &Connection) -> Result<i64, SidecarError> {
    connection
        .query_row("PRAGMA data_version", [], |row| row.get(0))
        .map_err(sqlite_error)
}

impl<'a> OutputVerification<'a> {
    pub(in crate::storage) fn start(
        connection: &'a mut Connection,
        binding: &AgentRunBindingV1,
    ) -> Result<Self, SidecarError> {
        if !connection.is_autocommit() {
            return Err(invalid("output verification requires idle connection"));
        }
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let marker = version(&tx)?;
        let expected = head(&tx, binding)?;
        let expected_bytes = tx
            .query_row(
                "SELECT text_bytes FROM run_output_heads WHERE run_id=?1",
                [&binding.run_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        tx.commit().map_err(sqlite_error)?;
        Ok(Self {
            connection,
            version: marker,
            expected,
            expected_bytes,
            after: 0,
            text_bytes: 0,
            usable: true,
            finished: false,
        })
    }

    /// At most one 100-row/512-KiB page per call. True means this run's output
    /// rows/head agree, not that run lifecycle/fence/schema admission succeeded.
    pub(in crate::storage) fn step(
        &mut self,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<bool, SidecarError> {
        if !std::mem::replace(&mut self.usable, false) || self.finished || cancelled() {
            return Err(invalid("output verification invalidated"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        if version(&tx)? != self.version || head(&tx, &self.expected.binding)? != self.expected {
            return Err(invalid("output verification source changed"));
        }
        let request = OutputPageRequest {
            format_version: OUTPUT_FORMAT_VERSION,
            binding: self.expected.binding.clone(),
            after_sequence: self.after,
            through_sequence: self.expected.committed_sequence,
            limit: 100,
        };
        let batch = page(&tx, &request)?;
        let mut total = self.text_bytes;
        for row in &batch.rows {
            if cancelled() {
                return Err(invalid("output verification cancelled"));
            }
            total = total
                .checked_add(row.event.text.len() as u64)
                .ok_or_else(|| invalid("output byte counter overflow"))?;
        }
        let finished = batch.next_after_sequence == self.expected.committed_sequence;
        if finished {
            let extra: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM run_output_records WHERE run_id=?1 AND (output_sequence>?2 OR output_sequence<=0))", params![self.expected.binding.run_id, self.expected.committed_sequence], |row| row.get(0)).map_err(sqlite_error)?;
            if extra || total != self.expected_bytes {
                return Err(invalid("output rows/head counters disagree"));
            }
        }
        if cancelled() || version(&tx)? != self.version {
            return Err(invalid("output verification interrupted"));
        }
        tx.commit().map_err(sqlite_error)?;
        self.after = batch.next_after_sequence;
        self.text_bytes = total;
        self.finished = finished;
        self.usable = !finished;
        Ok(finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> (Connection, AgentRunBindingV1) {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        let fixture: OutputPage = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/m5/agent-output-page.json"
        ))
        .unwrap();
        let binding = fixture.request.binding;
        let tx = db.transaction().unwrap();
        begin(&tx, &binding).unwrap();
        for sequence in 1..=101 {
            let mut event = fixture.rows[0].event.clone();
            event.event_id = format!("agent_event_{sequence}");
            append(
                &tx,
                &binding,
                sequence,
                &serde_json::to_value(event).unwrap(),
                1024 * 1024,
            )
            .unwrap();
        }
        tx.commit().unwrap();
        (db, binding)
    }

    #[test]
    fn yields_between_pages_and_finishes_once() {
        let (mut db, binding) = database();
        let mut session = OutputVerification::start(&mut db, &binding).unwrap();
        assert!(!session.step(&mut || false).unwrap());
        assert!(session.step(&mut || false).unwrap());
        assert!(session.step(&mut || false).is_err());
    }

    #[test]
    fn rejects_missing_event_counter_mismatch_and_id_corruption() {
        for sql in [
            "DELETE FROM run_output_records WHERE output_sequence=101",
            "UPDATE run_output_heads SET text_bytes=text_bytes+1",
            "UPDATE run_output_records SET event_id='agent_event_corrupt' WHERE output_sequence=101",
            "UPDATE run_output_heads SET committed_sequence=100",
        ] {
            let (mut db, binding) = database();
            db.execute_batch(sql).unwrap();
            let mut session = OutputVerification::start(&mut db, &binding).unwrap();
            let first = session.step(&mut || false);
            if first.is_ok() { assert!(session.step(&mut || false).is_err()); }
            assert!(session.step(&mut || false).is_err());
        }
    }

    #[test]
    fn cancellation_discards_cursor() {
        let (mut db, binding) = database();
        let mut session = OutputVerification::start(&mut db, &binding).unwrap();
        assert!(!session.step(&mut || false).unwrap());
        assert!(session.step(&mut || true).is_err());
        assert!(session.step(&mut || false).is_err());
    }
}
