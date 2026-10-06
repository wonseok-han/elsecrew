//! Reported named settings; not process or filesystem enforcement authority.
use super::{ExpectedPermission, ExpectedProfile};
use serde_json::{json, Value};
use std::path::Path;

pub fn expected_write_policy(home: &Path, workspace: &Path) -> Value {
    let (Some(home), Some(workspace)) = (home.to_str(), workspace.to_str()) else {
        return Value::Null;
    };
    json!({"filesystem":{
        ":root":"deny", ":minimal":"read", home:"deny",
        workspace:{".":"write",".git":"deny",".codex":"deny",".claude":"deny",".agents":"deny"}
    },"network":{"enabled":false}})
}

pub fn matches_write_policy(value: &Value, home: &Path, workspace: &Path) -> bool {
    matches_policy(value, &expected_write_policy(home, workspace))
}

fn matches_policy(value: &Value, expected: &Value) -> bool {
    let mut normalized = value.clone();
    // Observed 0.159.2 serializer defaults, not an arbitrary null-field bypass.
    for (object, keys) in [
        ("", &["description", "extends", "workspace_roots"][..]),
        ("filesystem", &["glob_scan_max_depth"][..]),
        (
            "network",
            &[
                "allow_local_binding",
                "allow_upstream_proxy",
                "dangerously_allow_all_unix_sockets",
                "dangerously_allow_non_loopback_proxy",
                "domains",
                "enable_socks5",
                "enable_socks5_udp",
                "mitm",
                "mode",
                "proxy_url",
                "socks_url",
                "unix_sockets",
            ][..],
        ),
    ] {
        let target = if object.is_empty() {
            &mut normalized
        } else {
            &mut normalized[object]
        };
        let Some(map) = target.as_object_mut() else {
            return false;
        };
        for key in keys {
            if map.get(*key).is_some_and(Value::is_null) {
                map.remove(*key);
            }
        }
    }
    normalized == *expected
}

/// Reported layer provenance only. This does not prove that startup extensions
/// never ran, nor authorize a process/turn. The RPC owner must validate framing.
pub fn matches_configuration_layers(
    result: &serde_json::Value,
    home: &Path,
    expected: &ExpectedProfile<'_>,
    expected_policy: &serde_json::Value,
    approval: &str,
) -> bool {
    use serde_json::json;
    if !matches!(approval, "never" | "on-request") {
        return false;
    }
    let flags = json!({
        "model":expected.model,"model_reasoning_effort":expected.reasoning_effort,
        "model_provider":"openai","approval_policy":approval,
        "default_permissions":"hello-agent-snapshot",
        "permissions":{"hello-agent-snapshot":expected_policy},
        "features":{"hooks":false,"apps":false,"plugins":false}
    });
    let Some(layers) = result["layers"].as_array() else {
        return false;
    };
    if layers.len() != 3 {
        return false;
    }
    let Some(home) = home.to_str() else {
        return false;
    };
    let mut seen = [false; 3];
    for layer in layers {
        if !layer["disabledReason"].is_null() {
            return false;
        }
        let (index, expected_name, expected_config) = match layer["name"]["type"].as_str() {
            Some("sessionFlags") => (0, json!({"type":"sessionFlags"}), flags.clone()),
            Some("user") => (
                1,
                json!({"type":"user","file":format!("{home}/config.toml")}),
                json!({"cli_auth_credentials_store":"file"}),
            ),
            Some("system") => (
                2,
                json!({"type":"system","file":"/etc/codex/config.toml"}),
                json!({}),
            ),
            _ => return false,
        };
        let mut name = layer["name"].clone();
        // Codex serializes the absent user config profile as explicit null.
        // A selected profile or any other extra source field still differs.
        if index == 1 && name.get("profile").is_some_and(Value::is_null) {
            if let Some(object) = name.as_object_mut() {
                object.remove("profile");
            }
        }
        if name != expected_name
            || layer["config"] != expected_config
            || std::mem::replace(&mut seen[index], true)
        {
            return false;
        }
    }
    let config = &result["config"];
    for key in [
        "model",
        "model_provider",
        "model_reasoning_effort",
        "approval_policy",
        "default_permissions",
    ] {
        if config[key] != flags[key] {
            return false;
        }
    }
    for feature in ["hooks", "apps", "plugins"] {
        if config["features"][feature] != false {
            return false;
        }
    }
    config["sandbox_mode"].is_null() && config["sandbox_workspace_write"].is_null()
}

pub(super) fn match_configuration(
    input: &super::BoundedObservation,
    home: &Path,
    workspace: &Path,
    expected: &ExpectedProfile<'_>,
    permission: ExpectedPermission,
) -> Result<(), super::ProviderError> {
    let mut policy = expected_write_policy(home, workspace);
    if permission == ExpectedPermission::ReadOnly {
        policy["filesystem"][workspace.to_str().ok_or_else(super::invalid)?]["."] = json!("read");
    }
    let result = input.json()?;
    if !matches_configuration_layers(&result, home, expected, &policy, "on-request")
        || !matches_policy(
            &result["config"]["permissions"]["hello-agent-snapshot"],
            &policy,
        )
    {
        return Err(super::invalid());
    }
    Ok(())
}
