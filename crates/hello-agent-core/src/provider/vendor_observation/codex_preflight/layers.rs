//! Reported config provenance only. Not proof that startup hooks never ran.
use super::{invalid, match_config, ExpectedPermission, ExpectedProfile, WorkspaceWriteConfig};
use crate::provider::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::value::RawValue;

pub(super) fn match_layers(
    input: &BoundedObservation,
    profile: &ExpectedProfile<'_>,
    home: &str,
    permission: ExpectedPermission,
) -> Result<(), ProviderError> {
    #[derive(Deserialize)]
    struct Response {
        layers: Vec<Layer>,
        config: Effective,
    }
    #[derive(Deserialize)]
    struct Effective {
        features: Features,
    }
    #[derive(Deserialize)]
    struct Features {
        hooks: bool,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FlagFeatures {
        hooks: bool,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Layer {
        name: Source,
        config: Box<RawValue>,
        #[serde(default)]
        disabled_reason: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(tag = "type", deny_unknown_fields)]
    enum Source {
        #[serde(rename = "sessionFlags")]
        Flags,
        #[serde(rename = "user")]
        User {
            file: String,
            profile: Option<String>,
        },
        #[serde(rename = "system")]
        System { file: String },
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct UserConfig {
        cli_auth_credentials_store: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Flags {
        model: String,
        model_provider: String,
        model_reasoning_effort: String,
        approval_policy: String,
        sandbox_mode: String,
        features: FlagFeatures,
        sandbox_workspace_write: Option<WorkspaceWriteConfig>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    let response: Response = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
    if response.layers.len() != 3 || response.config.features.hooks {
        return Err(invalid());
    }
    let mut seen = [false; 3];
    for layer in response.layers {
        if layer.disabled_reason.is_some() {
            return Err(invalid());
        }
        let index = match layer.name {
            Source::Flags => {
                // Preserve the old deny-unknown contract, including explicit null.
                if permission == ExpectedPermission::ReadOnly
                    && serde_json::from_str::<serde_json::Value>(layer.config.get())
                        .map_err(|_| invalid())?
                        .get("sandbox_workspace_write")
                        .is_some()
                {
                    return Err(invalid());
                }
                let config: Flags =
                    serde_json::from_str(layer.config.get()).map_err(|_| invalid())?;
                if config.model != profile.model
                    || config.model_provider != "openai"
                    || config.model_reasoning_effort != profile.reasoning_effort
                    || config.approval_policy != "on-request"
                    || config.sandbox_mode != permission.sandbox_mode()
                    || config.features.hooks
                    || match permission {
                        ExpectedPermission::ReadOnly => config.sandbox_workspace_write.is_some(),
                        ExpectedPermission::WorkspaceWrite => !config
                            .sandbox_workspace_write
                            .as_ref()
                            .is_some_and(WorkspaceWriteConfig::matches),
                    }
                {
                    return Err(invalid());
                }
                0
            }
            Source::User { file, profile } => {
                if file != format!("{home}/config.toml") || profile.is_some() {
                    return Err(invalid());
                }
                let config: UserConfig =
                    serde_json::from_str(layer.config.get()).map_err(|_| invalid())?;
                if config.cli_auth_credentials_store != "file" {
                    return Err(invalid());
                }
                1
            }
            Source::System { file } => {
                if file != "/etc/codex/config.toml" {
                    return Err(invalid());
                }
                let _: Empty = serde_json::from_str(layer.config.get()).map_err(|_| invalid())?;
                2
            }
        };
        if std::mem::replace(&mut seen[index], true) {
            return Err(invalid());
        }
    }
    match_config(input, profile, permission)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn only_exact_reported_profile_layers_match() {
        let config = json!({"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only","features":{"hooks":false}});
        let base = json!({"config":config,"layers":[{"name":{"type":"sessionFlags"},"config":config},{"name":{"type":"user","file":"/app/profile/config.toml"},"config":{"cli_auth_credentials_store":"file"}},{"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}]});
        for case in [
            "valid",
            "missing",
            "project",
            "system-hook",
            "foreign-user",
            "disabled",
            "duplicate",
            "extra-flag",
            "wrong-model",
            "effective-hooks",
            "flag-hooks",
            "missing-hooks",
            "null-write",
        ] {
            let mut value = base.clone();
            match case {
                "missing" => {
                    value.as_object_mut().unwrap().remove("layers");
                }
                "project" => {
                    value["layers"][2]["name"] = json!({"type":"project","dotCodexFolder":"/other"})
                }
                "system-hook" => value["layers"][2]["config"] = json!({"hooks":{}}),
                "foreign-user" => value["layers"][1]["name"]["file"] = json!("/other/config.toml"),
                "disabled" => value["layers"][0]["disabledReason"] = json!("ignored"),
                "duplicate" => value["layers"][2] = value["layers"][1].clone(),
                "extra-flag" => value["layers"][0]["config"]["mcp_servers"] = json!({}),
                "wrong-model" => value["layers"][0]["config"]["model"] = json!("other"),
                "effective-hooks" => value["config"]["features"]["hooks"] = json!(true),
                "flag-hooks" => value["layers"][0]["config"]["features"]["hooks"] = json!(true),
                "missing-hooks" => value["config"]["features"] = json!({}),
                "null-write" => {
                    value["layers"][0]["config"]["sandbox_workspace_write"] =
                        serde_json::Value::Null
                }
                _ => {}
            }
            let input = BoundedObservation::new(value.to_string().into_bytes()).unwrap();
            assert_eq!(
                match_layers(
                    &input,
                    &ExpectedProfile {
                        model: "test-model",
                        reasoning_effort: "medium"
                    },
                    "/app/profile",
                    ExpectedPermission::ReadOnly,
                )
                .is_ok(),
                case == "valid",
                "{case}"
            );
        }
    }
}
