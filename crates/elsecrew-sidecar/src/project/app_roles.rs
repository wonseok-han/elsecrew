//! App-owned defaults seed a project once; existing projects keep their settings.
use super::*;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;

const FILE: &str = "role-defaults.json";
// V3 allows 8 roles with 2000-byte responsibilities; keep the file bounded above that.
const MAX_BYTES: u64 = 32 * 1024;

fn validate(value: &Value) -> Result<(), ProjectFailure> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| ProjectFailure::manifest("invalid app role defaults"))?;
    let expected: &[&str] = if object.get("schemaVersion") == Some(&json!(3)) {
        &["schemaVersion", "revision", "roles", "roleDefinitions", "updatedAt"]
    } else {
        &["schemaVersion", "revision", "roles", "updatedAt"]
    };
    if object.len() != expected.len() || !expected.iter().all(|key| object.contains_key(*key)) {
        return Err(ProjectFailure::manifest("invalid app role defaults fields"));
    }
    object.insert("projectId".into(), json!("app_defaults"));
    elsecrew_protocol::run_recovery_v1::validate_g5_payload_v1(
        "projectRoleSettings",
        &Value::Object(object),
    )
    .map_err(|error| ProjectFailure::manifest(error.to_string()))
}

impl ProjectManager {
    fn role_defaults_directory(&self) -> Result<Dir, ProjectFailure> {
        self.verify_managed_paths()?;
        Ok(create_managed_directory(&self.data_root, &self.data_dir, "application-settings-v1")?.1)
    }

    /// Missing defaults fall back to the built-in team: V3 once storage admits
    /// user-defined authors (V13), otherwise the legacy V1 set.
    pub(crate) fn app_role_defaults(&self, custom_roles: bool) -> Result<Value, ProjectFailure> {
        let dir = self.role_defaults_directory()?;
        let mut options = CapOpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = match dir.open_with(FILE, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut value = if custom_roles {
                    crate::chat::default_settings_v3("app_defaults", 1)
                } else {
                    crate::chat::settings_value("app_defaults", 1)
                };
                value.as_object_mut().unwrap().remove("projectId");
                return Ok(value);
            }
            Err(error) => return Err(ProjectFailure::manifest(error.to_string())),
        };
        let metadata = file
            .metadata()
            .map_err(|error| ProjectFailure::manifest(error.to_string()))?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Err(ProjectFailure::manifest(
                "app role defaults is not a bounded file",
            ));
        }
        #[cfg(unix)]
        if metadata.nlink() != 1 {
            return Err(ProjectFailure::manifest("app role defaults is linked"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ProjectFailure::manifest(error.to_string()))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(ProjectFailure::manifest(
                "app role defaults grew past limit",
            ));
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| ProjectFailure::manifest(error.to_string()))?;
        validate(&value)?;
        Ok(value)
    }

    // Product calls hold Storage's exclusive ownership of the one fixed database
    // in this data directory and execute on its serial dispatcher. If additional
    // databases or writers ever share this directory, add a cross-process lock
    // spanning this read/compare/replace; atomic rename alone is not file CAS.
    pub(crate) fn update_app_role_defaults(
        &self,
        storage: &crate::storage::Storage,
        expected_revision: u64,
        roles: Value,
        role_definitions: Option<Value>,
    ) -> Result<Value, ProjectFailure> {
        let version = crate::chat::role_schema_version(&roles, role_definitions.is_some());
        crate::chat::require_role_schema_storage(storage, version)?;
        let current = self.app_role_defaults(storage.custom_roles_enabled()?)?;
        if current["revision"].as_u64() != Some(expected_revision) {
            return Err(ProjectFailure::dirty(
                "app defaults changed; reload before saving",
            ));
        }
        let revision = expected_revision
            .checked_add(1)
            .filter(|revision| *revision <= 9_007_199_254_740_991)
            .ok_or_else(|| ProjectFailure::manifest("app defaults revision exhausted"))?;
        let mut value = crate::chat::settings_value("app_defaults", revision);
        value.as_object_mut().unwrap().remove("projectId");
        value["schemaVersion"] = json!(version);
        value["roles"] = roles;
        if let Some(definitions) = role_definitions {
            value["roleDefinitions"] = definitions;
        }
        validate(&value)?;
        if serde_json::to_vec(&value)
            .map_err(|error| ProjectFailure::manifest(error.to_string()))?
            .len() as u64
            > MAX_BYTES
        {
            return Err(ProjectFailure::manifest("app role defaults exceeds limit"));
        }
        write_json_atomic(&self.role_defaults_directory()?, FILE, &value)?;
        Ok(value)
    }
}
