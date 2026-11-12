//! App-use consent is independent of provider authentication. Missing state is disconnected.
use super::*;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
const FILE: &str = "provider-connections.json";
impl ProjectManager {
    pub(crate) fn provider_connections(&self) -> Result<Value, ProjectFailure> {
        self.verify_managed_paths()?;
        let dir =
            create_managed_directory(&self.data_root, &self.data_dir, "application-settings-v1")?.1;
        let mut options = CapOpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = match dir.open_with(FILE, &options) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(json!({"codex":false,"claude-code":false}))
            }
            Err(e) => return Err(ProjectFailure::manifest(e.to_string())),
        };
        let meta = file
            .metadata()
            .map_err(|e| ProjectFailure::manifest(e.to_string()))?;
        if !meta.is_file() || meta.len() > 256 {
            return Err(ProjectFailure::manifest("invalid connections file"));
        }
        #[cfg(unix)]
        if meta.nlink() != 1 {
            return Err(ProjectFailure::manifest("linked connections file"));
        }
        let mut bytes = Vec::new();
        file.take(257)
            .read_to_end(&mut bytes)
            .map_err(|e| ProjectFailure::manifest(e.to_string()))?;
        if bytes.len() > 256 {
            return Err(ProjectFailure::manifest("connections file too large"));
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|e| ProjectFailure::manifest(e.to_string()))?;
        if value.as_object().map(|o| o.len()) != Some(2)
            || !value["codex"].is_boolean()
            || !value["claude-code"].is_boolean()
        {
            return Err(ProjectFailure::manifest("invalid connections"));
        }
        Ok(value)
    }
    pub(crate) fn set_provider_connection(
        &self,
        runtime: &str,
        connected: bool,
    ) -> Result<Value, ProjectFailure> {
        if !matches!(runtime, "codex" | "claude-code") {
            return Err(ProjectFailure::manifest("invalid provider"));
        }
        let mut value = self.provider_connections()?;
        value[runtime] = json!(connected);
        let dir =
            create_managed_directory(&self.data_root, &self.data_dir, "application-settings-v1")?.1;
        write_json_atomic(&dir, FILE, &value)?;
        Ok(value)
    }
}
