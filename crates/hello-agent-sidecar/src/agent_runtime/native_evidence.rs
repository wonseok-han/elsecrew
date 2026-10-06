//! Persisted native transport evidence. This is not provider admission or proof
//! that a restarted process is still owned; restart always uses recovery.
use super::*;

pub(super) fn validate(projection: &Map<String, Value>) -> Result<(), ProjectFailure> {
    let failure = || ProjectFailure::recovery("native execution evidence differs");
    let Some(marker) = projection.get("native_transport") else {
        return if projection.contains_key("native_thread_receipt") || projection.contains_key("native_provider_manifest") { Err(failure()) } else { Ok(()) };
    };
    let marker = crate::require_exact_params(marker, &["schemaVersion", "adapter", "connectionId", "requiresProviderAttestation"]).map_err(|_| failure())?;
    let requires_provider = marker["requiresProviderAttestation"].as_bool().ok_or_else(failure)?;
    let connection_id = marker["connectionId"].as_str().ok_or_else(failure)?;
    let connection = Uuid::parse_str(connection_id).map_err(|_| failure())?;
    let record = projection.get("record").ok_or_else(failure)?;
    validate_agent_adapter_payload_v1("runRecord", record).map_err(|_| failure())?;
    let runtime = match marker["adapter"].as_str() {
        Some("codex.preflight.v1") => "codex",
        Some("claude.stream.v1") => "claude-code",
        _ => return Err(failure()),
    };
    if marker["schemaVersion"] != 1
        || connection.is_nil() || connection.to_string() != connection_id
        || record["selectedProfile"]["runtime"] != runtime
        || projection.get("state") != record.get("state")
    { return Err(failure()); }
    let Some(receipt) = projection.get("native_thread_receipt") else {
        // A launch can fail before a thread exists. It cannot produce output or
        // be marked running/successful without the atomic thread receipt.
        if matches!(record["state"].as_str(), Some("running" | "succeeded"))
            || !projection.get("public_events").and_then(Value::as_array).is_some_and(Vec::is_empty)
        { return Err(failure()); }
        return Ok(());
    };
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
    fn claude_receipt_cannot_be_relabelled_as_codex_or_omit_initialization() {
        let mut value = projection();
        value.get_mut("record").unwrap()["selectedProfile"]["runtime"] = json!("claude-code");
        value.get_mut("native_thread_receipt").unwrap()["selectedProfile"]["runtime"] = json!("claude-code");
        value.get_mut("native_transport").unwrap()["adapter"] = json!("claude.stream.v1");
        validate(&value).unwrap();
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
