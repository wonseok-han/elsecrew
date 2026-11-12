//! Native Claude evidence, separate from the all-or-nothing G5C fixture contract.
//! Requested policy is not effective-config attestation or account entitlement.
use super::*;

pub const SESSION_OBSERVATIONS: &[&str] =
    &["model", "cwd", "permission", "tools", "mcp", "plugins"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Preflight {
    pub schema_version: u64,
    pub scope: AgentCapabilityScopeV1,
    pub profile: AgentResolvedProfileV1,
    pub executable: AgentExecutableIdentityV1,
    pub requested_launch_policy_digest: String,
    pub resolved_model: String,
    pub credentials_present: bool,
    pub observed_at: String,
}

impl Preflight {
    pub fn validate(&self) -> Result<(), AgentAdapterContractError> {
        schema(self.schema_version)?;
        validate_profile(&self.profile)?;
        validate_executable(&self.executable)?;
        for (name, value) in [
            ("chatId", &self.scope.chat_id),
            ("worktreeId", &self.scope.worktree_id),
            ("workspaceIdentity", &self.scope.workspace_identity),
        ] {
            id(value, name)?;
        }
        if self.profile.runtime != "claude-code" || self.profile.role_id != self.scope.role_id {
            return fail("Claude preflight must match its stored role");
        }
        fingerprint_digest(
            &self.requested_launch_policy_digest,
            "requestedLaunchPolicyDigest",
        )?;
        bounded(
            &self.resolved_model,
            "resolvedModel",
            G5_MODEL_SELECTOR_MAX_BYTES_V1,
        )?;
        timestamp(&self.observed_at, "observedAt")
    }

    /// Integrity binding of this record, never a registered executable hash.
    pub fn digest(&self) -> Result<String, AgentAdapterContractError> {
        self.validate()?;
        let value =
            canonicalize_fingerprint_value(serde_json::to_value(self).map_err(contract_json)?);
        let json = serde_json::to_string(&value).map_err(contract_json)?;
        Ok(crate::project_v1::sha256_file_version_v1(json.as_bytes()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SessionObservation {
    pub schema_version: u64,
    pub run_id: String,
    pub connection_id: String,
    pub preflight_digest: String,
    pub workspace_identity: String,
    pub resolved_model: String,
    pub observed_session_verification: Vec<String>,
    /// Old records were admitted only with an empty plugin list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_builtin_plugins: Vec<String>,
    pub observed_at: String,
}

impl SessionObservation {
    pub fn validate_against(&self, preflight: &Preflight) -> Result<(), AgentAdapterContractError> {
        schema(self.schema_version)?;
        agent_run_id(&self.run_id, "runId")?;
        id(&self.connection_id, "connectionId")?;
        timestamp(&self.observed_at, "observedAt")?;
        if self.observed_builtin_plugins.len() > 2
            || self.observed_builtin_plugins.iter().any(|name| !matches!(name.as_str(), "cc-plugin-privacy@builtin" | "cc-plugin-sec-default@builtin"))
            || self.observed_builtin_plugins.windows(2).any(|pair| pair[0] >= pair[1]) {
            return fail("Claude builtin observations differ");
        }
        if !preflight.credentials_present
            || self.preflight_digest != preflight.digest()?
            || self.workspace_identity != preflight.scope.workspace_identity
            || self.resolved_model != preflight.resolved_model
            || self
                .observed_session_verification
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                != SESSION_OBSERVATIONS
        {
            return fail("Claude session observations do not match preflight");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preflight() -> Preflight {
        serde_json::from_value(serde_json::json!({
            "schemaVersion":1,
            "scope":{"chatId":"chat_test","worktreeId":"worktree_test","workspaceIdentity":"workspace_test","roleId":"coding"},
            "profile":{"roleId":"coding","runtime":"claude-code","modelSelector":"opus","reasoningEffort":"medium","permission":"workspaceWrite"},
            "executable":{"pathDigest":format!("sha256:{}", "a".repeat(64)),"fileIdentityDigest":format!("sha256:{}", "b".repeat(64)),"cliVersion":"2.1.287"},
            "requestedLaunchPolicyDigest":format!("sha256:{}", "c".repeat(64)),
            "resolvedModel":"claude-test","credentialsPresent":true,"observedAt":"2026-10-07T00:00:00Z"
        })).unwrap()
    }
    #[test]
    fn observed_config_cannot_be_fabricated_from_a_requested_policy() {
        let preflight = preflight();
        let mut session = SessionObservation {
            observed_builtin_plugins: Vec::new(),
            schema_version: 1,
            run_id: "agent_run_test".into(),
            connection_id: "connection_test".into(),
            preflight_digest: preflight.digest().unwrap(),
            workspace_identity: preflight.scope.workspace_identity.clone(),
            resolved_model: preflight.resolved_model.clone(),
            observed_session_verification: SESSION_OBSERVATIONS
                .iter()
                .map(|v| (*v).into())
                .collect(),
            observed_at: "2026-10-07T00:00:01Z".into(),
        };
        session.validate_against(&preflight).unwrap();
        let legacy = serde_json::to_value(&session).unwrap();
        assert!(legacy.get("observedBuiltinPlugins").is_none());
        let restored: SessionObservation = serde_json::from_value(legacy).unwrap();
        assert!(restored.observed_builtin_plugins.is_empty());
        session.observed_builtin_plugins = vec!["cc-plugin-sec-default@builtin".into()];
        session.validate_against(&preflight).unwrap();
        let restored: SessionObservation = serde_json::from_value(serde_json::to_value(&session).unwrap()).unwrap();
        assert_eq!(restored.observed_builtin_plugins, session.observed_builtin_plugins);
        for plugins in [vec!["cc-plugin-privacy@builtin".into()], vec!["cc-plugin-privacy@builtin".into(), "cc-plugin-sec-default@builtin".into()]] {
            session.observed_builtin_plugins = plugins;
            assert!(session.validate_against(&preflight).is_ok());
        }
        for plugins in [vec!["unknown".into()], vec!["cc-plugin-privacy@builtin".into(); 2], vec!["cc-plugin-sec-default@builtin".into(), "cc-plugin-privacy@builtin".into()], vec!["cc-plugin-sec-default@builtin".into(); 2]] {
            session.observed_builtin_plugins = plugins;
            assert!(session.validate_against(&preflight).is_err());
        }
        session.observed_builtin_plugins.clear();
        session.observed_session_verification.push("config".into());
        assert!(session.validate_against(&preflight).is_err());
        session.observed_session_verification.pop();
        let mut changed = preflight.clone();
        changed.profile.permission = "readOnly".into();
        assert!(session.validate_against(&changed).is_err());
        changed = preflight.clone();
        changed.credentials_present = false;
        assert!(session.validate_against(&changed).is_err());
        changed = preflight.clone();
        changed.scope.workspace_identity = "workspace_other".into();
        assert!(session.validate_against(&changed).is_err());
        assert!(serde_json::from_value::<SessionObservation>(
            serde_json::to_value(&preflight).unwrap()
        )
        .is_err());
    }
}
