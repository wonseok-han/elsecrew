//! App-owned defaults seed a project once; existing projects keep their settings.
use super::*;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;

const FILE: &str = "role-defaults.json";
const MAX_BYTES: u64 = 16 * 1024;

fn validate(value: &Value) -> Result<(), ProjectFailure> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| ProjectFailure::manifest("invalid app role defaults"))?;
    if object.len() != 4
        || !["schemaVersion", "revision", "roles", "updatedAt"]
            .iter()
            .all(|key| object.contains_key(*key))
    {
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

    pub(crate) fn app_role_defaults(&self) -> Result<Value, ProjectFailure> {
        let dir = self.role_defaults_directory()?;
        let mut options = CapOpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = match dir.open_with(FILE, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut value = crate::chat::settings_value("app_defaults", 1);
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
    ) -> Result<Value, ProjectFailure> {
        if roles.as_array().is_some_and(|roles| roles.len() == 5)
            && !storage.coordinator_roles_enabled()?
        {
            return Err(ProjectFailure::manifest("coordinator roles require storage V12 admission"));
        }
        let current = self.app_role_defaults()?;
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
        value["schemaVersion"] = json!(if roles.as_array().is_some_and(|roles| roles.len() == 5) {
            2
        } else {
            1
        });
        value["roles"] = roles;
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
