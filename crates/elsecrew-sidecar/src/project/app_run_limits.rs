//! How many runs of each provider may work at once (ADE multi-project P3′).
//! More tasks than this wait their turn instead of stopping.
//!
//! The default, 3 each, comes from the memory a provider's program takes on
//! a working Mac (measured 2026-11-26): a Claude Code session 180–360 MB, a
//! Codex session about 220 MB plus a 35 MB helper. Three of each add about
//! 1.8 GB at most next to the app's own ~440 MB, which a 16 GB Mac carries;
//! the subscription's usage limits are the other reason not to go higher by
//! default. The person may pick 1 to 6.
use super::*;
const FILE: &str = "run-limits.json";
pub(crate) const RUN_LIMIT_DEFAULT: u64 = 3;
pub(crate) const RUN_LIMIT_MAX: u64 = 6;

impl ProjectManager {
    pub(crate) fn run_limits(&self) -> Result<Value, ProjectFailure> {
        self.verify_managed_paths()?;
        let defaults = json!({"codex":RUN_LIMIT_DEFAULT,"claude-code":RUN_LIMIT_DEFAULT});
        let dir = create_managed_directory(&self.data_root, &self.data_dir, "application-settings-v1")?.1;
        let mut options = CapOpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = match dir.open_with(FILE, &options) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(defaults),
            Err(e) => return Err(ProjectFailure::manifest(e.to_string())),
        };
        let meta = file.metadata().map_err(|e| ProjectFailure::manifest(e.to_string()))?;
        if !meta.is_file() || meta.len() > 128 {
            return Err(ProjectFailure::manifest("invalid run limits file"));
        }
        #[cfg(unix)]
        if meta.nlink() != 1 {
            return Err(ProjectFailure::manifest("linked run limits file"));
        }
        let mut bytes = Vec::new();
        file.take(129).read_to_end(&mut bytes).map_err(|e| ProjectFailure::manifest(e.to_string()))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| ProjectFailure::manifest(e.to_string()))?;
        let valid = |key: &str| value[key].as_u64().is_some_and(|n| (1..=RUN_LIMIT_MAX).contains(&n));
        if value.as_object().map(|o| o.len()) != Some(2) || !valid("codex") || !valid("claude-code") {
            return Err(ProjectFailure::manifest("invalid run limits"));
        }
        Ok(value)
    }

    /// The limit for one provider; an unreadable file falls back to the default.
    pub(crate) fn run_limit(&self, runtime: &str) -> usize {
        self.run_limits().ok().and_then(|limits| limits[runtime].as_u64()).unwrap_or(RUN_LIMIT_DEFAULT) as usize
    }

    pub(crate) fn set_run_limit(&self, runtime: &str, limit: u64) -> Result<Value, ProjectFailure> {
        if !matches!(runtime, "codex" | "claude-code") || !(1..=RUN_LIMIT_MAX).contains(&limit) {
            return Err(ProjectFailure::manifest("invalid run limit"));
        }
        let mut value = self.run_limits().unwrap_or_else(|_| json!({"codex":RUN_LIMIT_DEFAULT,"claude-code":RUN_LIMIT_DEFAULT}));
        value[runtime] = json!(limit);
        let dir = create_managed_directory(&self.data_root, &self.data_dir, "application-settings-v1")?.1;
        write_json_atomic(&dir, FILE, &value)?;
        Ok(value)
    }
}
