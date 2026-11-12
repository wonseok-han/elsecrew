//! Bounded QA-only dispatcher timing. Never includes request payloads or paths.
#[derive(Default)]
pub(crate) struct DispatchTiming {
    #[cfg(feature = "qa-output-fixtures")]
    phases: std::collections::BTreeMap<&'static str, (u64, u128, u128)>,
}

impl DispatchTiming {
    #[inline]
    pub(crate) fn measure<T>(&mut self, phase: &'static str, action: impl FnOnce() -> T) -> T {
        #[cfg(feature = "qa-output-fixtures")]
        let started = std::time::Instant::now();
        #[cfg(not(feature = "qa-output-fixtures"))]
        let _ = phase;
        let result = action();
        #[cfg(feature = "qa-output-fixtures")]
        {
            let micros = started.elapsed().as_micros();
            let entry = self.phases.entry(phase).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(micros);
            entry.2 = entry.2.max(micros);
        }
        result
    }

    pub(crate) fn emit(&self) {
        #[cfg(feature = "qa-output-fixtures")]
        eprintln!(
            "QA_DISPATCH_TIMING {}",
            serde_json::json!({
                "diagnostic": true, "gateEligible": false,
                "units": "microseconds", "columns": ["count", "total", "maximum"],
                "phases": self.phases,
            })
        );
    }
}
