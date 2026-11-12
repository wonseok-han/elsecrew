//! Login-only configuration evidence: no model selection or execution authority.
use super::super::codex_profile_config::{self, ProfileConfig};
use crate::provider::{BoundedObservation, ProviderError};
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn validate(input: &BoundedObservation, home: &Path) -> Result<Value, ProviderError> {
    let invalid = || ProviderError("invalid login configuration evidence".into());
    let envelope = input.json()?;
    let object = envelope.as_object().ok_or_else(invalid)?;
    if object.len() != 2 || envelope["id"] != 60 || !object.contains_key("result") {
        return Err(invalid());
    }
    let result = &envelope["result"];
    codex_profile_config::validate_disabled_project_layers(result, home, home)
        .map_err(|_| invalid())?;
    let layers = result["layers"].as_array().ok_or_else(invalid)?;
    let flags = layers
        .iter()
        .find(|layer| layer["name"]["type"] == "sessionFlags")
        .ok_or_else(invalid)?;
    let mut normalized_flags = flags["config"].clone();
    codex_profile_config::strip_isolation_flag(&mut normalized_flags, &result["config"], &[home])
        .map_err(|_| invalid())?;
    if normalized_flags != json!({"features":{"hooks":false,"apps":false,"plugins":false}}) {
        return Err(invalid());
    }
    let mut seen = [false; 3];
    for layer in layers {
        if layer["name"]["type"] == "project" {
            continue;
        }
        if !layer["disabledReason"].is_null() {
            return Err(invalid());
        }
        let mut name = layer["name"].clone();
        if name["type"] == "user" && name.get("profile") == Some(&Value::Null) {
            name.as_object_mut().ok_or_else(invalid)?.remove("profile");
        }
        let index = if name == json!({"type":"sessionFlags"}) {
            0
        } else if name == json!({"type":"user","file":home.join("config.toml")}) {
            if !ProfileConfig::matches_observed_layer(
                &layer["config"],
                &flags["config"],
                &result["config"],
                home,
            ) {
                return Err(invalid());
            }
            1
        } else if name == json!({"type":"system","file":"/etc/codex/config.toml"}) {
            if layer["config"] != json!({}) {
                return Err(invalid());
            }
            2
        } else {
            return Err(invalid());
        };
        if std::mem::replace(&mut seen[index], true) {
            return Err(invalid());
        }
    }
    if seen != [true; 3] {
        return Err(invalid());
    }
    Ok(result.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::vendor_observation::codex_login::connection::{
        LoginAction, LoginConnection,
    };

    #[test]
    fn configuration_precedes_account_or_oauth_and_rejects_mutations() {
        // Pure protocol data: this is Unix-rooted and Windows-UNC absolute.
        // No filesystem/network access occurs. Preserve the macOS login
        // connection's leading-slash contract as well as host path validation.
        let home = Path::new("//fixture/profile/app");
        assert!(home.is_absolute());
        let projects = ProfileConfig::parse(b"cli_auth_credentials_store='file'\n")
            .unwrap()
            .isolated_projects(&[home])
            .unwrap();
        let flags =
            json!({"projects":projects,"features":{"hooks":false,"apps":false,"plugins":false}});
        let base = json!({"id":60,"result":{"config":flags,"layers":[
            {"name":{"type":"sessionFlags"},"config":flags},
            {"name":{"type":"user","file":home.join("config.toml")},"config":{"cli_auth_credentials_store":"file"}},
            {"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}
        ]}});
        let obs = |value: Value| BoundedObservation::new(value.to_string().into_bytes()).unwrap();
        for check_only in [true, false] {
            for case in [
                "valid",
                "null-profile",
                "selected-profile",
                "extra-source",
                "id",
                "error",
                "hooks",
                "system",
                "source",
                "flags",
                "trust",
            ] {
                let (mut flow, _) = LoginConnection::begin(home.to_str().unwrap()).unwrap();
                flow.require_startup_isolation().unwrap();
                flow.accept(&obs(json!({"id":1,"result":{"userAgent":"test","codexHome":home,"platformFamily":"unix","platformOs":"macos"}}))).unwrap();
                let request = if check_only {
                    flow.check_account()
                } else {
                    flow.start_login()
                }
                .unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(request.as_str()).unwrap()["method"],
                    "config/read"
                );
                assert!(flow.check_account().is_err());
                assert!(flow.start_login().is_err());
                let mut value = base.clone();
                match case {
                    "null-profile" => value["result"]["layers"][1]["name"]["profile"] = Value::Null,
                    "selected-profile" => {
                        value["result"]["layers"][1]["name"]["profile"] = "other".into()
                    }
                    "extra-source" => value["result"]["layers"][1]["name"]["extra"] = true.into(),
                    "id" => value["id"] = 3.into(),
                    "error" => value["error"] = json!({}),
                    "hooks" => value["result"]["config"]["features"]["hooks"] = true.into(),
                    "system" => value["result"]["layers"][2]["config"] = json!({"hooks":{}}),
                    "source" => value["result"]["layers"][2]["name"]["type"] = "mdm".into(),
                    "flags" => value["result"]["layers"][0]["config"]["extra"] = true.into(),
                    "trust" => {
                        value["result"]["config"]["projects"][home.to_str().unwrap()]
                            ["trust_level"] = "trusted".into()
                    }
                    _ => {}
                }
                let outcome = flow.accept(&obs(value));
                if matches!(case, "valid" | "null-profile") {
                    let LoginAction::Send(request) = outcome.unwrap() else {
                        panic!()
                    };
                    assert_eq!(
                        serde_json::from_str::<Value>(request.as_str()).unwrap()["method"],
                        if check_only {
                            "account/read"
                        } else {
                            "account/login/start"
                        }
                    );
                } else {
                    assert!(outcome.is_err(), "{case}");
                    assert!(flow.accept(&obs(base.clone())).is_err());
                }
            }
        }
    }
}
