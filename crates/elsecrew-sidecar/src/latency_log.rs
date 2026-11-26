//! Optional request timing for latency diagnosis (`ELSECREW_SIDECAR_TIMING_LOG`
//! names a file). One JSON line per request: method, a short action word, how
//! long it waited behind the loop and how long handling took; and each loop
//! phase that took 5 ms or more. Never params, paths, or results. Off unless
//! the variable is set; the product never sets it.
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const ENV: &str = "ELSECREW_SIDECAR_TIMING_LOG";
const PHASE_FLOOR: Duration = Duration::from_millis(5);

#[derive(Default)]
pub(crate) struct LatencyLog {
    file: Option<std::fs::File>,
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis())
}

fn ms(duration: Duration) -> f64 {
    (duration.as_micros() as f64) / 1000.0
}

/// A short identifier-like word (an action name), else nothing.
fn word(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.is_empty() && text.len() <= 40 && text.bytes().all(|byte| byte.is_ascii_alphanumeric()))
}

impl LatencyLog {
    pub(crate) fn from_env() -> Self {
        let file = std::env::var_os(ENV).and_then(|path| {
            std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
        });
        Self { file }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.file.is_some()
    }

    fn write(&mut self, value: serde_json::Value) {
        if let Some(file) = &mut self.file {
            // Diagnostics never affect the engine: a failed write turns them off.
            if writeln!(file, "{value}").is_err() {
                self.file = None;
            }
        }
    }

    pub(crate) fn request(
        &mut self,
        request: &elsecrew_protocol::RequestV1,
        received: Instant,
        started: Instant,
        finished: Instant,
    ) {
        if !self.enabled() {
            return;
        }
        let action = word(request.params.get("action").and_then(serde_json::Value::as_str));
        self.write(serde_json::json!({
            "kind": "request", "at": now_ms() as u64, "requestId": request.request_id,
            "method": request.method, "action": action,
            "queueMs": ms(started.saturating_duration_since(received)),
            "handleMs": ms(finished.saturating_duration_since(started)),
        }));
    }

    pub(crate) fn phase(&mut self, phase: &'static str, elapsed: Duration) {
        if self.enabled() && elapsed >= PHASE_FLOOR {
            self.write(serde_json::json!({"kind": "phase", "at": now_ms() as u64, "phase": phase, "ms": ms(elapsed)}));
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
