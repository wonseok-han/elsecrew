//! Optional request timing for latency diagnosis (`ELSECREW_SIDECAR_TIMING_LOG`
//! names a file). One JSON line per request: the method (only a known protocol
//! method name, else "other"), how long it waited behind the loop and how long
//! handling took; and each loop phase that took 5 ms or more. Nothing the
//! request carried (params, request ID, action words, paths) is written. Off
//! unless the variable is set; the product never sets it.
//!
//! The engine loop only hands a small record to a channel; a writer thread does
//! the encoding and file I/O, so a slow or stuck log never holds the loop
//! (PR #15 review I2). A full channel drops the record and counts it.
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const ENV: &str = "ELSECREW_SIDECAR_TIMING_LOG";
const PHASE_FLOOR: Duration = Duration::from_millis(5);
const QUEUE: usize = 4096;

enum Record {
    Request { at: u64, method: &'static str, queue: Duration, handle: Duration },
    Phase { at: u64, phase: &'static str, elapsed: Duration },
}

#[derive(Default)]
pub(crate) struct LatencyLog {
    sender: Option<SyncSender<Record>>,
    dropped: Arc<AtomicU64>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

fn ms(duration: Duration) -> f64 {
    (duration.as_micros() as f64) / 1000.0
}

/// The protocol's own name for a known method; anything else is "other".
pub(crate) fn known_method(method: &str) -> &'static str {
    elsecrew_protocol::METHODS_V1
        .iter()
        .copied()
        .chain(elsecrew_protocol::preview_v1::PREVIEW_METHODS_V1.iter().copied())
        .find(|known| *known == method)
        .unwrap_or("other")
}

fn line(record: &Record, dropped: u64) -> serde_json::Value {
    match record {
        Record::Request { at, method, queue, handle } => serde_json::json!({
            "kind": "request", "at": at, "method": method,
            "queueMs": ms(*queue), "handleMs": ms(*handle), "dropped": dropped,
        }),
        Record::Phase { at, phase, elapsed } => serde_json::json!({
            "kind": "phase", "at": at, "phase": phase, "ms": ms(*elapsed), "dropped": dropped,
        }),
    }
}

impl LatencyLog {
    pub(crate) fn from_env() -> Self {
        std::env::var_os(ENV)
            .and_then(|path| std::fs::OpenOptions::new().create(true).append(true).open(path).ok())
            .map(Self::to_file)
            .unwrap_or_default()
    }

    fn to_file(mut file: std::fs::File) -> Self {
        let (sender, receiver) = sync_channel::<Record>(QUEUE);
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&dropped);
        let started = std::thread::Builder::new().name("latency-log".into()).spawn(move || {
            for record in receiver {
                // Diagnostics never affect the engine: a failed write ends them.
                if writeln!(file, "{}", line(&record, counted.load(Ordering::Relaxed))).is_err() {
                    return;
                }
            }
        });
        Self { sender: started.ok().map(|_| sender), dropped }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.sender.is_some()
    }

    fn send(&mut self, record: Record) {
        let Some(sender) = &self.sender else { return };
        match sender.try_send(record) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => self.sender = None,
        }
    }

    /// `method` comes from `known_method`, taken before the request is handled.
    pub(crate) fn request(&mut self, method: &'static str, received: Instant, started: Instant, finished: Instant) {
        self.send(Record::Request {
            at: now_ms(),
            method,
            queue: started.saturating_duration_since(received),
            handle: finished.saturating_duration_since(started),
        });
    }

    pub(crate) fn phase(&mut self, phase: &'static str, elapsed: Duration) {
        if self.enabled() && elapsed >= PHASE_FLOOR {
            self.send(Record::Phase { at: now_ms(), phase, elapsed });
        }
    }

    /// Runs `action`, recording it as `phase` when it was slow.
    pub(crate) fn measure<T>(&mut self, phase: &'static str, action: impl FnOnce() -> T) -> T {
        if !self.enabled() {
            return action();
        }
        let started = Instant::now();
        let result = action();
        self.phase(phase, started.elapsed());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(path: &std::path::Path) -> Vec<serde_json::Value> {
        for _ in 0..200 {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if text.lines().count() >= 3 {
                return text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the writer thread wrote nothing");
    }

    /// PR #15 review I2: only fixed names and numbers reach the file, and the
    /// caller never waits for the write.
    #[test]
    fn timing_log_writes_only_known_names_and_numbers_off_the_loop() {
        assert_eq!(known_method("chat.get"), "chat.get");
        assert_eq!(known_method("run.approval"), "run.approval");
        assert_eq!(known_method("secret-token-1234"), "other");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("timing.jsonl");
        let mut log = LatencyLog::to_file(std::fs::File::create(&path).unwrap());
        assert!(log.enabled());
        let now = Instant::now();
        log.request(known_method("secret-token-1234"), now, now, now + Duration::from_millis(3));
        log.request(known_method("chat.get"), now, now + Duration::from_millis(2), now + Duration::from_millis(5));
        log.phase("watch", Duration::from_millis(1)); // below the floor: not written
        log.phase("runtime", Duration::from_millis(7));
        let written = lines(&path);
        assert_eq!(written.len(), 3);
        assert_eq!(written[0]["method"], "other");
        assert_eq!(written[1]["method"], "chat.get");
        assert_eq!(written[1]["queueMs"], 2.0);
        assert_eq!(written[2]["phase"], "runtime");
        for value in &written {
            let keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
            assert!(keys.iter().all(|key| ["kind", "at", "method", "queueMs", "handleMs", "dropped", "phase", "ms"].contains(key)), "{keys:?}");
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("secret"));
        // Off: nothing is recorded and nothing is spawned.
        let mut off = LatencyLog::default();
        assert!(!off.enabled());
        off.request("chat.get", now, now, now);
    }

    /// A writer that cannot keep up never blocks the caller: records drop.
    #[test]
    fn a_full_timing_queue_drops_records_instead_of_waiting() {
        let (sender, receiver) = sync_channel::<Record>(1);
        let mut log = LatencyLog { sender: Some(sender), dropped: Arc::new(AtomicU64::new(0)) };
        let now = Instant::now();
        let started = Instant::now();
        for _ in 0..100 {
            log.request("chat.get", now, now, now);
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(log.dropped.load(Ordering::Relaxed), 99);
        drop(receiver);
        log.request("chat.get", now, now, now);
        assert!(!log.enabled(), "a gone writer turns the log off");
    }
}
