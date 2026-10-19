//! Persisted native transport evidence. This is not provider admission or proof
//! that a restarted process is still owned; restart always uses recovery.
use super::*;

pub(super) fn validate(projection: &Map<String, Value>) -> Result<(), ProjectFailure> {
    let failure = || ProjectFailure::recovery("native execution evidence differs");
    let Some(marker) = projection.get("native_transport") else {
        return if ["native_thread_receipt", "native_provider_manifest", "claude_preflight", "claude_session_observation"]
            .iter().any(|key| projection.contains_key(*key)) { Err(failure()) } else { Ok(()) };
    };
    let marker = crate::require_exact_params(marker, &["schemaVersion", "adapter", "connectionId", "requiresProviderAttestation"]).map_err(|_| failure())?;
    let requires_provider = marker["requiresProviderAttestation"].as_bool().ok_or_else(failure)?;
    let connection_id = marker["connectionId"].as_str().ok_or_else(failure)?;
    let connection = Uuid::parse_str(connection_id).map_err(|_| failure())?;
    let record = projection.get("record").ok_or_else(failure)?;
    validate_agent_adapter_payload_v1("runRecord", record).map_err(|_| failure())?;
    let runtime = match marker["adapter"].as_str() {
        Some("codex.preflight.v1") => "codex",
        Some("claude.stream.v1" | "claude.stream.v2") => "claude-code",
        _ => return Err(failure()),
    };
    if marker["schemaVersion"] != 1
        || connection.is_nil() || connection.to_string() != connection_id
        || record["selectedProfile"]["runtime"] != runtime
        || projection.get("state") != record.get("state")
    { return Err(failure()); }
    let phased = marker["adapter"] == "claude.stream.v2";
    let preflight = if phased {
        use hello_agent_protocol::agent_adapter_v1::phased_claude::Preflight;
        if requires_provider || projection.contains_key("native_provider_manifest") { return Err(failure()); }
        let value: Preflight = serde_json::from_value(projection.get("claude_preflight").ok_or_else(failure)?.clone())
            .map_err(|_| failure())?;
        value.validate().map_err(|_| failure())?;
        if !value.credentials_present
            || serde_json::to_value(&value.profile).map_err(|_| failure())? != record["selectedProfile"]
            || value.scope.chat_id != record["chatId"].as_str().ok_or_else(failure)?
            || value.scope.worktree_id != record["worktreeId"].as_str().ok_or_else(failure)?
            || value.scope.workspace_identity != record["workspaceIdentity"].as_str().ok_or_else(failure)?
        { return Err(failure()); }
        Some(value)
    } else {
        if projection.contains_key("claude_preflight") || projection.contains_key("claude_session_observation") { return Err(failure()); }
        None
    };
    let Some(receipt) = projection.get("native_thread_receipt") else {
        // A launch can fail before a thread exists. It cannot produce output or
        // be marked running/successful without the atomic thread receipt.
        if projection.contains_key("claude_session_observation") || matches!(record["state"].as_str(), Some("running" | "succeeded"))
            || !projection.get("public_events").and_then(Value::as_array).is_some_and(Vec::is_empty)
        { return Err(failure()); }
        return Ok(());
    };
    if let Some(preflight) = preflight {
        use hello_agent_protocol::agent_adapter_v1::phased_claude::SessionObservation;
        let session: SessionObservation = serde_json::from_value(projection.get("claude_session_observation").ok_or_else(failure)?.clone())
            .map_err(|_| failure())?;
        session.validate_against(&preflight).map_err(|_| failure())?;
        if session.run_id != record["runId"].as_str().ok_or_else(failure)? || session.connection_id != connection_id {
            return Err(failure());
        }
    }
    if requires_provider && ["native_provider_manifest", "provider_discovery", "runtime_attestation"]
        .iter().any(|field| !projection.contains_key(*field))
    { return Err(failure()); }
    let receipt = crate::require_exact_params(receipt, &[
        "schemaVersion", "connectionId", "threadDigest", "runId", "workspaceIdentity", "capabilityFingerprint", "selectedProfile",
    ]).map_err(|_| failure())?;
    let digest = receipt["threadDigest"].as_str().and_then(|value| value.strip_prefix("sha256:")).ok_or_else(failure)?;
    if receipt["schemaVersion"] != 1 || receipt["connectionId"] != connection_id
        || digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || record["state"] == "preparing"
        || record["runId"] != receipt["runId"]
        || record["workspaceIdentity"] != receipt["workspaceIdentity"]
        || record["capabilityFingerprint"] != receipt["capabilityFingerprint"]
        || record["selectedProfile"] != receipt["selectedProfile"]
    { return Err(failure()); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection() -> Map<String, Value> {
        let profile = AgentResolvedProfileV1 {
            role_id: "coding".into(), runtime: "codex".into(),
            model_selector: "test-model".into(), reasoning_effort: Some("medium".into()),
            permission: "workspaceWrite".into(),
        };
        let chat = json!({"chatId":"chat_test", "taskId":"task_test", "projectId":"project_test",
            "worktreeId":"worktree_test", "workspaceIdentity":"workspace_test"});
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let record = agent_run_record("agent_run_test", &chat, &profile, &fingerprint, "running", None, &now()).unwrap();
        let connection = Uuid::new_v4().to_string();
        json!({"record":record, "state":"running", "public_events":[],
            "native_transport":{"schemaVersion":1,"adapter":"codex.preflight.v1","connectionId":connection,"requiresProviderAttestation":false},
            "native_thread_receipt":{"schemaVersion":1,"connectionId":connection,
                "threadDigest":format!("sha256:{}", "b".repeat(64)), "runId":"agent_run_test",
                "workspaceIdentity":"workspace_test", "capabilityFingerprint":fingerprint,
                "selectedProfile":profile}
        }).as_object().unwrap().clone()
    }

    #[test]
    fn phased_claude_requires_bound_preflight_and_session_records() {
        use hello_agent_protocol::agent_adapter_v1::phased_claude::{Preflight, SessionObservation, SESSION_OBSERVATIONS};
        let mut value = projection();
        value.get_mut("record").unwrap()["selectedProfile"]["runtime"] = json!("claude-code");
        value.get_mut("native_thread_receipt").unwrap()["selectedProfile"]["runtime"] = json!("claude-code");
        value.get_mut("native_transport").unwrap()["adapter"] = json!("claude.stream.v2");
        let preflight: Preflight = serde_json::from_value(json!({
            "schemaVersion":1,"scope":{"chatId":"chat_test","worktreeId":"worktree_test","workspaceIdentity":"workspace_test","roleId":"coding"},
            "profile":value["record"]["selectedProfile"],
            "executable":{"pathDigest":format!("sha256:{}","a".repeat(64)),"fileIdentityDigest":format!("sha256:{}","b".repeat(64)),"cliVersion":"2.1.287"},
            "requestedLaunchPolicyDigest":format!("sha256:{}","c".repeat(64)),
            "resolvedModel":"test-model","credentialsPresent":true,"observedAt":now(),
        })).unwrap();
        let session = SessionObservation {
            observed_builtin_plugins: Vec::new(),
            schema_version:1,run_id:"agent_run_test".into(),
            connection_id:value["native_transport"]["connectionId"].as_str().unwrap().into(),
            preflight_digest:preflight.digest().unwrap(),workspace_identity:"workspace_test".into(),
            resolved_model:"test-model".into(),observed_session_verification:SESSION_OBSERVATIONS.iter().map(|v| (*v).into()).collect(),
            observed_at:now(),
        };
        value.insert("claude_preflight".into(),serde_json::to_value(preflight).unwrap());
        value.insert("claude_session_observation".into(),serde_json::to_value(session).unwrap());
        validate(&value).unwrap();
        for field in ["claude_preflight","claude_session_observation"] {
            let mut changed = value.clone(); changed.remove(field);
            assert!(validate(&changed).is_err());
        }
        let mut changed = value.clone();
        changed.get_mut("native_transport").unwrap()["adapter"] = json!("claude.stream.v1");
        assert!(validate(&changed).is_err());
        let mut changed = value.clone();
        changed.get_mut("claude_session_observation").unwrap()["connectionId"] = json!(Uuid::new_v4().to_string());
        assert!(validate(&changed).is_err());
        let mut changed = value;
        changed.get_mut("claude_preflight").unwrap()["requestedLaunchPolicyDigest"] = json!(format!("sha256:{}","d".repeat(64)));
        assert!(validate(&changed).is_err());
    }

    #[test]
    fn claude_receipt_cannot_be_relabelled_as_codex_or_omit_initialization() {
        let mut value = projection();
        value.get_mut("record").unwrap()["selectedProfile"]["runtime"] = json!("claude-code");
        value.get_mut("native_thread_receipt").unwrap()["selectedProfile"]["runtime"] = json!("claude-code");
        value.get_mut("native_transport").unwrap()["adapter"] = json!("claude.stream.v1");
        validate(&value).unwrap();
        let mut required = value.clone();
        required.get_mut("native_transport").unwrap()["requiresProviderAttestation"] = json!(true);
        assert!(validate(&required).is_err());
        let mut wrong = value.clone();
        wrong.get_mut("native_transport").unwrap()["adapter"] = json!("codex.preflight.v1");
        assert!(validate(&wrong).is_err());
        value.remove("native_thread_receipt");
        assert!(validate(&value).is_err());
    }

    #[test]
    fn receipt_is_exactly_bound_and_required_after_thread_start() {
        let original = projection();
        validate(&original).unwrap();
        for field in ["connectionId", "runId", "workspaceIdentity", "capabilityFingerprint", "threadDigest"] {
            let mut changed = original.clone();
            changed.get_mut("native_thread_receipt").unwrap()[field] = json!("different");
            assert!(validate(&changed).is_err(), "{field}");
        }
        for field in ["native_transport", "native_thread_receipt"] {
            let mut changed = original.clone();
            changed.remove(field);
            assert!(validate(&changed).is_err(), "{field}");
        }
        let mut preparing = original;
        preparing.remove("native_thread_receipt");
        preparing.insert("state".into(), json!("preparing"));
        preparing.get_mut("record").unwrap()["state"] = json!("preparing");
        validate(&preparing).unwrap();
        preparing.insert("public_events".into(), json!([{}]));
        assert!(validate(&preparing).is_err());
    }
}
