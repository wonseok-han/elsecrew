//! Offline matching of Codex 0.149.1 config/read, model/list and thread/start results.
//! A match is NOT config isolation, runtime attestation, or permission to send a prompt.
//! The caller must correlate responses and verify the executable/version separately.

use super::super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use std::collections::BTreeSet;

/// An explicit requested tuple; never filled from vendor defaults or upgrade suggestions.
pub struct ExpectedProfile<'a> {
    pub model: &'a str,
    pub reasoning_effort: &'a str,
}

fn invalid() -> ProviderError {
    ProviderError("Codex preflight observation is incomplete or mismatched".into())
}

fn validate_profile(profile: &ExpectedProfile<'_>) -> Result<(), ProviderError> {
    for value in [profile.model, profile.reasoning_effort] {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Match reported settings, not filesystem identity or enforcement. Only the owning runtime
/// can verify that the expected absolute cwd still identifies its managed snapshot.
pub fn match_read_only_thread(
    input: &BoundedObservation,
    profile: &ExpectedProfile<'_>,
    expected_cwd: &std::path::Path,
) -> Result<(), ProviderError> {
    #[derive(Deserialize)]
    struct Thread {
        id: String,
    }
    #[derive(Deserialize)]
    #[serde(tag = "type", deny_unknown_fields)]
    enum Sandbox {
        #[serde(rename = "readOnly")]
        ReadOnly {
            #[serde(rename = "networkAccess")]
            network_access: bool,
        },
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Response {
        thread: Thread,
        model: String,
        model_provider: String,
        cwd: String,
        instruction_sources: Vec<String>,
        approval_policy: String,
        approvals_reviewer: String,
        sandbox: Sandbox,
        reasoning_effort: String,
    }
    validate_profile(profile)?;
    let expected = expected_cwd.to_str().ok_or_else(invalid)?;
    if !expected_cwd.is_absolute()
        || expected.contains('\0')
        || expected_cwd
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(invalid());
    }
    let response: Response = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
    let Sandbox::ReadOnly { network_access } = response.sandbox;
    if response.thread.id.is_empty()
        || response.thread.id.len() > 128
        || response.thread.id.chars().any(char::is_control)
        || response.model != profile.model
        || response.reasoning_effort != profile.reasoning_effort
        || response.model_provider != "openai"
        || response.cwd != expected
        || response.approval_policy != "on-request"
        || response.approvals_reviewer != "user"
        || !response.instruction_sources.is_empty()
        || network_access
    {
        return Err(invalid());
    }
    Ok(())
}

/// Strict initial candidate: read-only and explicit on-request approval, not a writer policy.
/// Other config keys may exist; this function does not certify that hooks/MCP are disabled.
pub fn match_read_only_config(
    input: &BoundedObservation,
    profile: &ExpectedProfile<'_>,
) -> Result<(), ProviderError> {
    #[derive(Deserialize)]
    struct Config {
        model: String,
        model_provider: String,
        model_reasoning_effort: String,
        approval_policy: String,
        sandbox_mode: String,
    }
    #[derive(Deserialize)]
    struct Response {
        config: Config,
    }
    validate_profile(profile)?;
    let response: Response = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
    let config = response.config;
    if config.model != profile.model
        || config.model_reasoning_effort != profile.reasoning_effort
        || config.model_provider != "openai"
        || config.approval_policy != "on-request"
        || config.sandbox_mode != "read-only"
    {
        return Err(invalid());
    }
    Ok(())
}

/// Only a complete, bounded catalog can match. No aliases, defaults, hidden models or upgrades.
/// Pagination is deliberately rejected until an owning bounded collector exists.
pub fn match_model_catalog(
    input: &BoundedObservation,
    profile: &ExpectedProfile<'_>,
) -> Result<(), ProviderError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Effort {
        reasoning_effort: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Model {
        id: String,
        model: String,
        hidden: bool,
        supported_reasoning_efforts: Vec<Effort>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Response {
        data: Vec<Model>,
        next_cursor: Option<String>,
    }
    validate_profile(profile)?;
    // Missing cursor is not evidence of a complete catalog. Typed parsing below rejects duplicates.
    if input.json()?.get("nextCursor").is_none() {
        return Err(invalid());
    }
    let response: Response = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
    if response.next_cursor.is_some() || response.data.is_empty() || response.data.len() > 256 {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut found = false;
    for model in response.data {
        if model.id.is_empty()
            || model.id.len() > 128
            || model.model.is_empty()
            || model.model.len() > 128
            || !ids.insert(model.id)
            || !names.insert(model.model.clone())
            || model.supported_reasoning_efforts.len() > 32
        {
            return Err(invalid());
        }
        let mut efforts = BTreeSet::new();
        for effort in model.supported_reasoning_efforts {
            if effort.reasoning_effort.is_empty()
                || effort.reasoning_effort.len() > 128
                || !efforts.insert(effort.reasoning_effort)
            {
                return Err(invalid());
            }
        }
        if model.model == profile.model
            && !model.hidden
            && efforts.contains(profile.reasoning_effort)
        {
            found = true;
        }
    }
    if !found {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn thread() -> Value {
        json!({"thread":{"id":"thread-test"},"model":"test-model","modelProvider":"openai","cwd":"/managed/snapshot","instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"})
    }

    #[test]
    fn thread_settings_are_exact_and_never_filesystem_authority() {
        let cwd = std::path::Path::new("/managed/snapshot");
        assert!(match_read_only_thread(&input(&thread()), &profile(), cwd).is_ok());
        for key in [
            "model",
            "modelProvider",
            "cwd",
            "approvalPolicy",
            "approvalsReviewer",
            "reasoningEffort",
        ] {
            for replacement in [json!("OTHER"), Value::Null] {
                let mut value = thread();
                value[key] = replacement;
                assert!(match_read_only_thread(&input(&value), &profile(), cwd).is_err());
            }
        }
        for sandbox in [
            json!({"type":"dangerFullAccess"}),
            json!({"type":"workspaceWrite"}),
            json!({"type":"readOnly","networkAccess":true}),
            json!({"type":"readOnly","networkAccess":false,"extra":true}),
        ] {
            let mut value = thread();
            value["sandbox"] = sandbox;
            assert!(match_read_only_thread(&input(&value), &profile(), cwd).is_err());
        }
        let mut value = thread();
        value["instructionSources"] = json!(["/user/AGENTS.md"]);
        assert!(match_read_only_thread(&input(&value), &profile(), cwd).is_err());
        assert!(match_read_only_thread(
            &input(&thread()),
            &profile(),
            std::path::Path::new("relative")
        )
        .is_err());
    }

    #[test]
    fn duplicate_config_and_model_fields_are_rejected() {
        let config_wire = serde_json::to_string(&config()).unwrap().replace(
            "\"model\":\"test-model\"",
            "\"model\":\"other\",\"model\":\"test-model\"",
        );
        assert!(match_read_only_config(
            &BoundedObservation::new(config_wire.into_bytes()).unwrap(),
            &profile()
        )
        .is_err());
        let model_wire = serde_json::to_string(&catalog())
            .unwrap()
            .replace("\"hidden\":false", "\"hidden\":true,\"hidden\":false");
        assert!(match_model_catalog(
            &BoundedObservation::new(model_wire.into_bytes()).unwrap(),
            &profile()
        )
        .is_err());
    }

    fn input(value: &Value) -> BoundedObservation {
        BoundedObservation::new(serde_json::to_vec(value).unwrap()).unwrap()
    }
    fn profile() -> ExpectedProfile<'static> {
        ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
        }
    }
    fn config() -> Value {
        json!({"config":{"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only","token":"SECRET","hooks":{"command":"PRIVATE"}}})
    }
    fn catalog() -> Value {
        json!({"data":[{"id":"display-id","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"PRIVATE"}]}],"nextCursor":null})
    }

    #[test]
    fn explicit_observations_match_without_returning_raw_fields() {
        assert_eq!(
            match_read_only_config(&input(&config()), &profile()).unwrap(),
            ()
        );
        assert_eq!(
            match_model_catalog(&input(&catalog()), &profile()).unwrap(),
            ()
        );
    }

    #[test]
    fn every_config_dimension_must_match_without_defaults() {
        for key in [
            "model",
            "model_provider",
            "model_reasoning_effort",
            "approval_policy",
            "sandbox_mode",
        ] {
            for replacement in [json!("SECRET"), Value::Null, json!(true)] {
                let mut value = config();
                value["config"][key] = replacement;
                let error = match_read_only_config(&input(&value), &profile()).unwrap_err();
                assert!(!format!("{error:?}").contains("SECRET"));
            }
            let mut value = config();
            value["config"].as_object_mut().unwrap().remove(key);
            assert!(match_read_only_config(&input(&value), &profile()).is_err());
        }
    }

    #[test]
    fn catalog_rejects_alias_hidden_effort_and_partial_results() {
        for (field, replacement) in [
            ("model", json!("other")),
            ("hidden", json!(true)),
            ("supportedReasoningEfforts", json!([])),
        ] {
            let mut value = catalog();
            value["data"][0][field] = replacement;
            assert!(match_model_catalog(&input(&value), &profile()).is_err());
        }
        let mut value = catalog();
        value["nextCursor"] = json!("next");
        assert!(match_model_catalog(&input(&value), &profile()).is_err());
        value.as_object_mut().unwrap().remove("nextCursor");
        assert!(match_model_catalog(&input(&value), &profile()).is_err());
    }

    #[test]
    fn duplicate_models_and_efforts_cannot_make_ambiguous_matches() {
        let mut value = catalog();
        let model = value["data"][0].clone();
        value["data"].as_array_mut().unwrap().push(model);
        assert!(match_model_catalog(&input(&value), &profile()).is_err());
        let mut value = catalog();
        value["data"][0]["supportedReasoningEfforts"] =
            json!([{"reasoningEffort":"medium"},{"reasoningEffort":"medium"}]);
        assert!(match_model_catalog(&input(&value), &profile()).is_err());
        let duplicate = BoundedObservation::new(
            br#"{"data":[],"nextCursor":"next","nextCursor":null}"#.to_vec(),
        )
        .unwrap();
        assert!(match_model_catalog(&duplicate, &profile()).is_err());
    }

    #[test]
    fn malformed_cross_vendor_and_invalid_requested_tuple_fail() {
        for value in [json!({"loggedIn":true}), json!([]), json!(null)] {
            assert!(match_model_catalog(&input(&value), &profile()).is_err());
            assert!(match_read_only_config(&input(&value), &profile()).is_err());
        }
        let bad = ExpectedProfile {
            model: "",
            reasoning_effort: "medium",
        };
        assert!(match_read_only_config(&input(&config()), &bad).is_err());
        assert!(match_model_catalog(&input(&catalog()), &bad).is_err());
    }
}
