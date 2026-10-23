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

struct RunScan {
    expected: OutputHead,
    expected_bytes: u64,
    after: u64,
    text_bytes: u64,
}

/// Full output-table closure only. Run/fence and schema verification must still
/// be composed into the same admission fence before this can authorize writes.
pub(in crate::storage) struct AllOutputVerification<'a> {
    connection: &'a mut Connection,
    version: i64,
    after_head: i64,
    current: Option<RunScan>,
    reverse_after: Option<i64>,
    runs_after: Option<i64>,
    usable: bool,
}

impl<'a> AllOutputVerification<'a> {
    pub(in crate::storage) fn start(connection: &'a mut Connection) -> Result<Self, SidecarError> {
        if !connection.is_autocommit() {
            return Err(invalid("output verification requires idle connection"));
        }
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let marker = version(&tx)?;
        for table in ["run_output_heads", "run_output_records"] {
            let hidden: bool = tx
                .query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE rowid<=0 LIMIT 1)"),
                    [],
                    |row| row.get(0),
                )
                .map_err(sqlite_error)?;
            if hidden {
                return Err(invalid("output table has invalid row identity"));
            }
        }
        tx.commit().map_err(sqlite_error)?;
        Ok(Self {
            connection,
            version: marker,
            after_head: 0,
            current: None,
            reverse_after: None,
            runs_after: None,
            usable: true,
        })
    }

    pub(in crate::storage) fn step(
        &mut self,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<bool, SidecarError> {
        if !std::mem::replace(&mut self.usable, false) || cancelled() {
            return Err(invalid("output closure invalidated"));
        }
        if let Some(progress) = self.current.take() {
            let mut run = OutputVerification {
                connection: self.connection,
                version: self.version,
                expected: progress.expected,
                expected_bytes: progress.expected_bytes,
                after: progress.after,
                text_bytes: progress.text_bytes,
                usable: true,
                finished: false,
            };
            let done = run.step(cancelled)?;
            if !done {
                self.current = Some(RunScan {
                    expected: run.expected.clone(),
                    expected_bytes: run.expected_bytes,
                    after: run.after,
                    text_bytes: run.text_bytes,
                });
            }
            self.usable = true;
            return Ok(false);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        if version(&tx)? != self.version {
            return Err(invalid("output closure source changed"));
        }
        if let Some(after) = self.runs_after {
            let row: Option<(i64, Option<String>, Option<String>)> = tx.query_row(
                "SELECT rowid, CASE WHEN length(CAST(run_id AS BLOB))<=8192 THEN run_id ELSE NULL END, CASE WHEN length(CAST(projection_json AS BLOB))<=1048576 THEN projection_json ELSE NULL END FROM runs WHERE rowid>?1 ORDER BY rowid LIMIT 1",
                [after], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional().map_err(sqlite_error)?;
            let next = if let Some((rowid, id, json)) = row {
                let id = id.ok_or_else(|| invalid("oversized run identity"))?;
                let value: Value =
                    serde_json::from_str(&json.ok_or_else(|| invalid("oversized run projection"))?)
                        .map_err(|_| invalid("invalid run projection"))?;
                if let Some(format) = value.get("output_format_version") {
                    let exists: bool = tx
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM run_output_heads WHERE run_id=?1)",
                            [&id],
                            |row| row.get(0),
                        )
                        .map_err(sqlite_error)?;
                    if format != OUTPUT_FORMAT_VERSION || !exists {
                        return Err(invalid(
                            "durable output run has missing head or unsupported format",
                        ));
                    }
                }
                Some(rowid)
            } else {
                None
            };
            if cancelled() || version(&tx)? != self.version {
                return Err(invalid("output closure interrupted"));
            }
            tx.commit().map_err(sqlite_error)?;
            let done = next.is_none();
            self.runs_after = next;
            self.usable = !done;
            return Ok(done);
        }
        if let Some(after) = self.reverse_after {
            // Each indexed existence check has one dependency. No body is read.
            let mut statement = tx.prepare("SELECT r.rowid, EXISTS(SELECT 1 FROM run_output_heads h WHERE h.run_id=r.run_id AND r.output_sequence>0 AND r.output_sequence<=h.committed_sequence) FROM run_output_records r WHERE r.rowid>?1 ORDER BY r.rowid LIMIT 100").map_err(sqlite_error)?;
            let mut rows = statement.query([after]).map_err(sqlite_error)?;
            let mut next = after;
            let mut count = 0;
            while let Some(row) = rows.next().map_err(sqlite_error)? {
                if cancelled() || !row.get::<_, bool>(1).map_err(sqlite_error)? {
                    return Err(invalid("orphan output record or cancelled closure"));
                }
                next = row.get(0).map_err(sqlite_error)?;
                count += 1;
            }
            drop(rows);
            drop(statement);
            if cancelled() || version(&tx)? != self.version {
                return Err(invalid("output closure interrupted"));
            }
            tx.commit().map_err(sqlite_error)?;
            self.reverse_after = Some(next);
            if count < 100 {
                self.runs_after = Some(0);
            }
            self.usable = true;
            return Ok(false);
        }
        let row: Option<(i64, Option<String>, Option<String>, u64)> = tx.query_row(
            "SELECT rowid, CASE WHEN length(CAST(run_id AS BLOB))<=8192 THEN run_id ELSE NULL END, CASE WHEN length(CAST(binding_json AS BLOB))<=8192 THEN binding_json ELSE NULL END, text_bytes FROM run_output_heads WHERE rowid>?1 ORDER BY rowid LIMIT 1",
            [self.after_head], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row_u64(row, 3)?)),
        ).optional().map_err(sqlite_error)?;
        let next = if let Some((rowid, run_id, json, expected_bytes)) = row {
            let binding: AgentRunBindingV1 =
                serde_json::from_str(&json.ok_or_else(|| invalid("oversized output binding"))?)
                    .map_err(|_| invalid("invalid output binding"))?;
            if Some(&binding.run_id) != run_id.as_ref() {
                return Err(invalid("output head key mismatch"));
            }
            let expected = head(&tx, &binding)?;
            verify_run_binding(&tx, &expected)?;
            Some((
                rowid,
                RunScan {
                    expected,
                    expected_bytes,
                    after: 0,
                    text_bytes: 0,
                },
            ))
        } else {
            None
        };
        if cancelled() || version(&tx)? != self.version {
            return Err(invalid("output closure interrupted"));
        }
        tx.commit().map_err(sqlite_error)?;
        match next {
            Some((rowid, progress)) => {
                self.after_head = rowid;
                self.current = Some(progress);
            }
            None => {
                self.reverse_after = Some(0);
            }
        }
        self.usable = true;
        Ok(false)
    }
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
                |row| row_u64(row, 0),
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
            let extra: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM run_output_records WHERE run_id=?1 AND (output_sequence>?2 OR output_sequence<=0))", params![self.expected.binding.run_id, sql_integer(self.expected.committed_sequence)?], |row| row.get(0)).map_err(sqlite_error)?;
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
        db.execute_batch("CREATE TABLE runs(run_id TEXT PRIMARY KEY, state TEXT NOT NULL, projection_json TEXT NOT NULL)").unwrap();
        let fixture: OutputPage = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/m5/agent-output-page.json"
        ))
        .unwrap();
        let binding = fixture.request.binding;
        let tx = db.transaction().unwrap();
        install_run(&tx, &binding);
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

    fn install_run(tx: &Transaction<'_>, binding: &AgentRunBindingV1) {
        let envelope: Value = serde_json::from_str(
            include_str!(
                "../../../../../tests/fixtures/m5/protocol-v1/valid/agent-run-state-changed.jsonl"
            )
            .lines()
            .last()
            .unwrap(),
        )
        .unwrap();
        let mut record = envelope["payload"]["run"].clone();
        record["runId"] = Value::String(binding.run_id.clone());
        let projection = serde_json::json!({"run_id":binding.run_id,"state":"running","source_kind":"actualAgent","output_format_version":1,"record":record});
        tx.execute(
            "INSERT INTO runs VALUES (?1,'running',?2)",
            params![binding.run_id, projection.to_string()],
        )
        .unwrap();
    }

    #[test]
    fn full_closure_visits_empty_runs_and_rejects_orphans() {
        let (mut db, binding) = database();
        let mut empty = binding.clone();
        empty.run_id = "agent_run_empty".to_string();
        let tx = db.transaction().unwrap();
        install_run(&tx, &empty);
        begin(&tx, &empty).unwrap();
        tx.commit().unwrap();
        {
            let mut all = AllOutputVerification::start(&mut db).unwrap();
            let mut calls = 0;
            while !all.step(&mut || false).unwrap() {
                calls += 1;
                assert!(calls < 20);
            }
            assert!(calls >= 5);
            assert!(all.step(&mut || false).is_err());
        }
        // Simulate a damaged database even when normal foreign keys prevent it.
        db.execute_batch("PRAGMA foreign_keys=OFF; DELETE FROM run_output_heads")
            .unwrap();
        let mut all = AllOutputVerification::start(&mut db).unwrap();
        assert!(!all.step(&mut || false).unwrap());
        assert!(all.step(&mut || false).is_err());
    }

    #[test]
    fn closure_rejects_missing_legacy_and_mismatched_runs() {
        for sql in [
            "DELETE FROM runs",
            "UPDATE runs SET projection_json=json_remove(projection_json,'$.output_format_version')",
            "UPDATE runs SET projection_json=json_set(projection_json,'$.record.taskId','task_other')",
            "UPDATE runs SET state='succeeded'",
        ] {
            let (mut db, _) = database();
            db.execute_batch(sql).unwrap();
            let mut all = AllOutputVerification::start(&mut db).unwrap();
            assert!(all.step(&mut || false).is_err());
        }
    }

    #[test]
    fn closure_rejects_new_run_without_output_head() {
        let (mut db, _) = database();
        db.execute_batch("DELETE FROM run_output_records; DELETE FROM run_output_heads")
            .unwrap();
        let mut all = AllOutputVerification::start(&mut db).unwrap();
        assert!(!all.step(&mut || false).unwrap());
        assert!(!all.step(&mut || false).unwrap());
        assert!(all.step(&mut || false).is_err());
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
