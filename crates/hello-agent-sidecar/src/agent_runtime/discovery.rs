//! Scoped discovery ownership. Product activation remains a separate gate.
use super::*;
use crate::provider_preflight_session::discovery_worker::{Completion, DiscoveryWorker, Evidence};
use hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedPermission;
use hello_agent_protocol::agent_adapter_v1::{AgentCapabilitiesRequestV1, AgentCapabilityScopeV1};

pub(super) struct PendingDiscovery {
    pub(super) worker: DiscoveryWorker,
    scope: AgentCapabilityScopeV1,
    profile: AgentResolvedProfileV1,
    completion: Option<Completion>,
}

pub(crate) struct ScopedEvidence {
    pub(crate) scope: AgentCapabilityScopeV1,
    pub(crate) profile: AgentResolvedProfileV1,
    pub(crate) evidence: Evidence,
}

pub(super) struct PendingClaudeDiscovery {
    pub(super) worker: crate::provider_claude_session::discovery::DiscoveryWorker,
    completion: Option<crate::provider_claude_session::discovery::Completion>,
    scope: AgentCapabilityScopeV1,
    profile: AgentResolvedProfileV1,
}

#[allow(dead_code)] // Consumed by forthcoming Claude capability/reservation branch.
pub(crate) struct ScopedClaudeEvidence {
    pub(crate) scope: AgentCapabilityScopeV1,
    pub(crate) profile: AgentResolvedProfileV1,
    pub(crate) evidence: crate::provider_claude_session::discovery::DiscoveryEvidence,
}

impl ScopedClaudeEvidence {
    /// Eligibility for the selected stored role, not permission to send a prompt.
    /// Launch still requires fresh discovery and same-connection initialization.
    pub(crate) fn capability(&self) -> Result<AgentCapabilitySnapshotV1, SidecarError> {
        let failure = || SidecarError::new("agentInputChanged", "Claude discovery projection differs");
        let model = self.evidence.observation.selected_model()?;
        if !self.evidence.is_fresh()
            || self.profile.runtime != "claude-code"
            || self.profile.role_id != self.scope.role_id
            || model.model_selector != self.profile.model_selector
            || model.reasoning_efforts.first() != self.profile.reasoning_effort.as_ref()
            || model.permissions != vec![self.profile.permission.clone()]
        {
            return Err(failure());
        }
        let authenticated = self.evidence.credentials_present;
        let mut snapshot = AgentCapabilitySnapshotV1 {
            schema_version: 1,
            scope: Some(self.scope.clone()),
            runtime: "claude-code".into(),
            platform: "darwin".into(),
            installation_state: "installed".into(),
            auth_state: if authenticated { "authenticated" } else { "notAuthenticated" }.into(),
            executable: Some(self.evidence.executable.public_identity(&self.evidence.version)?),
            models: vec![model],
            discovery: AgentDiscoveryEvidenceV1 {
                source: "machineReadableCatalog".into(),
                verification_level: "runtimeVerified".into(),
                exact_cli_version: Some(self.evidence.version.clone()),
                effective_model_evidence: "runtimeVerified".into(),
            },
            can_start: authenticated,
            blocked_reason: (!authenticated).then(|| "notAuthenticated".into()),
            fingerprint: String::new(),
            observed_at: now(),
        };
        snapshot.fingerprint = compute_agent_capability_fingerprint_v1(&snapshot).map_err(|_| failure())?;
        validate_agent_adapter_payload_v1("capabilitySnapshot", &serde_json::to_value(&snapshot).map_err(|_| failure())?)
            .map_err(|_| failure())?;
        Ok(snapshot)
    }
}

pub(super) struct NativeProviderContext {
    manifest: Value,
    discovery: Value,
}

impl NativeProviderContext {
    pub(super) fn attest(&self, run: &Value, cwd: &Path) -> Result<(Value, Value, Value), SidecarError> {
        use hello_agent_protocol::agent_adapter_v1 as contract;
        let failure = || SidecarError::new("agentInputChanged", "native provider evidence differs");
        let profile = &run["selectedProfile"];
        let attestation = json!({
            "schemaVersion":1,"runId":run["runId"],"providerId":self.manifest["providerId"],"runtime":self.manifest["runtime"],
            "capabilityFingerprint":run["capabilityFingerprint"],
            "manifestDigest":self.manifest["manifestDigest"],
            "executableClosureDigest":self.manifest["executableClosure"]["closureDigest"],
            "effectiveProfile":{"modelSelector":profile["modelSelector"],"reasoningEffort":profile["reasoningEffort"],"permission":profile["permission"]},
            "workspaceIdentity":run["workspaceIdentity"],
            "managedCwdIdentity":format!("sha256:{:x}", Sha256::digest(cwd.as_os_str().as_encoded_bytes())),
            "eventParserId":self.manifest["eventParserId"],
            "verifiedRuntimeEvidence":self.manifest["launchPolicy"]["requiredRuntimeVerification"],
            "verifiedAt":now(),
        });
        let accepted = json!({"schemaVersion":1,"runId":run["runId"],"chatId":run["chatId"],
            "taskId":run["taskId"],"projectId":run["projectId"],"worktreeId":run["worktreeId"],
            "workspaceIdentity":run["workspaceIdentity"],"roleId":profile["roleId"],
            "capabilityFingerprint":run["capabilityFingerprint"],"state":"preparing"});
        contract::validate_agent_runtime_attestation_against_accepted_v1(
            &attestation, &self.manifest, &self.discovery, &accepted, profile,
        ).map_err(|_| failure())?;
        Ok((self.manifest.clone(), self.discovery.clone(), attestation))
    }
}

impl ScopedEvidence {
    fn provider_context(&self, capability: &AgentCapabilitySnapshotV1) -> Result<NativeProviderContext, SidecarError> {
        use hello_agent_protocol::agent_adapter_v1 as contract;
        let failure = || SidecarError::new("agentInputChanged", "native provider manifest differs");
        let binary = capability.executable.as_ref().ok_or_else(failure)?;
        let helper = self.evidence.helper.public_identity(&self.evidence.version)?;
        let mut closure = contract::ProviderExecutableClosureV1 {
            binary: contract::ProviderExecutableBinaryIdentityV1 {
                logical_name:"codex".into(), path_digest:binary.path_digest.clone(),
                file_identity_digest:binary.file_identity_digest.clone(), cli_version:self.evidence.version.clone(),
            },
            required_helpers:vec![contract::ProviderExecutableFileIdentityV1 {
                logical_name:"codex-code-mode-host".into(), path_digest:helper.path_digest,
                file_identity_digest:helper.file_identity_digest,
            }], closure_digest:String::new(),
        };
        closure.closure_digest = contract::compute_provider_executable_closure_digest_v1(&closure).map_err(|_| failure())?;
        let profiles = vec![contract::ProviderProfileTupleV1 {
            model_selector:self.profile.model_selector.clone(), reasoning_effort:self.profile.reasoning_effort.clone(),
            permission:self.profile.permission.clone(),
        }];
        let verification: Vec<String> = contract::G5C_CODEX_RUNTIME_VERIFICATION_V1.iter().map(|value| (*value).into()).collect();
        let mut manifest = contract::ProviderManifestV1 {
            schema_version:1,provider_id:"codex".into(),runtime:"codex".into(),manifest_version:"codex-native-named-v1".into(),
            executable_closure:closure.clone(),exact_profiles:profiles.clone(),
            auth_parser_id:"codex.auth-status.v1".into(),event_parser_id:"codex.jsonl.v1".into(),
            launch_policy:contract::ProviderLaunchPolicyV1 {
                invocation:"oneShot".into(),prompt_transport:"serverResolvedMessages".into(),cwd_policy:"managedWorkspaceExact".into(),
                environment_policy:"allowlistedNoSecrets".into(),fallback_policy:"forbidden".into(),required_runtime_verification:verification.clone(),
            },manifest_digest:String::new(),
        };
        manifest.manifest_digest = contract::compute_provider_manifest_digest_v1(&manifest).map_err(|_| failure())?;
        let observation = contract::SanitizedDiscoveryObservationV1 {
            schema_version:1,provider_id:"codex".into(),runtime:"codex".into(),manifest_digest:manifest.manifest_digest.clone(),
            installation_state:"installed".into(),auth_state:"authenticated".into(),executable_closure:Some(closure),
            exact_cli_version:Some(self.evidence.version.clone()),model_catalog_evidence:"runtimeVerified".into(),
            exact_profiles:profiles,auth_parser_id:manifest.auth_parser_id.clone(),event_parser_id:manifest.event_parser_id.clone(),
            observed_runtime_verification:verification,can_start:true,blocked_reason:None,observed_at:now(),
        };
        let manifest = serde_json::to_value(manifest).map_err(|_| failure())?;
        let discovery = serde_json::to_value(observation).map_err(|_| failure())?;
        contract::validate_sanitized_discovery_observation_v1(&discovery, &manifest).map_err(|_| failure())?;
        Ok(NativeProviderContext { manifest, discovery })
    }
    /// Discovery eligibility only. The start path must freshly inspect and
    /// perform its own same-connection preflight before sending user input.
    #[allow(dead_code)] // Deferred capability response consumes this projection.
    pub(crate) fn capability(&self) -> Result<AgentCapabilitySnapshotV1, SidecarError> {
        let failure = || SidecarError::new("agentInputChanged", "discovery projection differs");
        let model = self.evidence.observation.selected_model()?;
        if self.profile.runtime != "codex"
            || self.profile.role_id != self.scope.role_id
            || model.model_selector != self.profile.model_selector
            || model.reasoning_efforts.first() != self.profile.reasoning_effort.as_ref()
            || model.permissions != vec![self.profile.permission.clone()]
        {
            return Err(failure());
        }
        self.evidence.helper.revalidate()?;
        let executable = self
            .evidence
            .executable
            .public_identity(&self.evidence.version)?;
        let mut snapshot = AgentCapabilitySnapshotV1 {
            scope: Some(self.scope.clone()),
            schema_version: 1,
            runtime: "codex".into(),
            platform: "darwin".into(),
            installation_state: "installed".into(),
            auth_state: "authenticated".into(),
            executable: Some(executable),
            models: vec![model],
            discovery: AgentDiscoveryEvidenceV1 {
                source: "machineReadableCatalog".into(),
                verification_level: "runtimeVerified".into(),
                exact_cli_version: Some(self.evidence.version.clone()),
                effective_model_evidence: "runtimeVerified".into(),
            },
            can_start: true,
            blocked_reason: None,
            fingerprint: String::new(),
            observed_at: now(),
        };
        snapshot.fingerprint =
            compute_agent_capability_fingerprint_v1(&snapshot).map_err(|_| failure())?;
        validate_agent_adapter_payload_v1(
            "capabilitySnapshot",
            &serde_json::to_value(&snapshot).map_err(|_| failure())?,
        )
        .map_err(|_| failure())?;
        Ok(snapshot)
    }
}

impl AgentRuntime {
    /// A start always obtains fresh, stored-role evidence; it cannot reuse a UI
    /// capability response. The caller owns the actual policy admission.
    #[allow(dead_code)]
    pub(crate) fn begin_scoped_claude_start(
        &mut self, storage: &Storage, projects: &ProjectManager,
        request: &AgentRunStartRequestV1, request_id: &str,
        admit: impl FnMut(&Path, &Path, &str, &str, crate::provider_claude_session::ToolAccess)
            -> Result<(), SidecarError> + Send + 'static,
    ) -> Result<(), SidecarError> {
        let preparation = preparation::RunPreparation::resolve(storage, projects, request)
            .map_err(|_| SidecarError::new("agentInputChanged", "Claude start input differs"))?;
        self.begin_claude_discovery(storage, projects, request_id, &AgentCapabilitiesRequestV1 {
            runtime: preparation.profile.runtime,
            refresh: true,
            chat_id: Some(request.chat_id.clone()),
            role_id: Some(request.role_id.clone()),
        }, admit)
    }

    /// Preserve the fresh process in caller storage on every later error. No
    /// launch or fabricated runtime attestation occurs during reservation.
    #[allow(dead_code)]
    pub(crate) fn reserve_scoped_claude_start(
        &mut self, storage: &mut Storage, projects: &ProjectManager,
        request: &AgentRunStartRequestV1, request_id: &str,
        observed: &ScopedClaudeEvidence,
        reservation: &mut Option<preparation::RunReservation>,
        process: &mut Option<crate::provider_login_process::LoginProcess<'static>>,
    ) -> Result<(), SidecarError> {
        use crate::provider_claude_session::ToolAccess;
        let failure = || SidecarError::new("agentInputChanged", "Claude start discovery differs");
        if self.preparing_to_quit || reservation.is_some() || process.is_some()
            || active_run_limit_reached(self.owned_run_ids().len()) { return Err(failure()); }
        let capability = observed.capability()?;
        let preparation = preparation::RunPreparation::resolve(storage, projects, request)
            .map_err(|_| failure())?;
        if observed.scope != capability_scope(&preparation.chat, &request.role_id).map_err(|_| failure())?
            || observed.profile != preparation.profile
            || preflight_outcome(request, &preparation.profile, &capability) != "started"
        { return Err(failure()); }
        let access = match preparation.profile.permission.as_str() {
            "readOnly" => ToolAccess::ReadOnly,
            "workspaceWrite" => ToolAccess::Edit,
            _ => return Err(failure()),
        };
        let binding = crate::provider_claude_profile::ClaudeProfileBinding::from_storage(storage)?;
        *process = Some(crate::provider_login_process::LoginProcess::reserve_claude_owned(binding)?);
        let home = process.as_ref().ok_or_else(failure)?.claude_preflight_home()?;
        if !observed.evidence.is_fresh() || !observed.evidence.observation.matches(
            &preparation.profile.model_selector,
            preparation.profile.reasoning_effort.as_deref().ok_or_else(failure)?, access,
            Path::new(preparation.manifest["worktreePath"].as_str().ok_or_else(failure)?),
            Path::new(preparation.manifest["canonicalGitRoot"].as_str().ok_or_else(failure)?), &home,
        ) { return Err(failure()); }
        observed.evidence.executable.revalidate()?;
        *reservation = Some(preparation.reserve(storage, projects, &capability, request_id)
            .map_err(|_| failure())?);
        Ok(())
    }

    /// Resolve all launch inputs from the durable chat; renderer supplies IDs
    /// only. Policy admission remains an internal caller responsibility.
    #[allow(dead_code)]
    pub(crate) fn begin_claude_discovery(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        request_id: &str,
        request: &AgentCapabilitiesRequestV1,
        admit: impl FnMut(&Path, &Path, &str, &str, crate::provider_claude_session::ToolAccess)
            -> Result<(), SidecarError> + Send + 'static,
    ) -> Result<(), SidecarError> {
        use crate::provider_claude_session::{discovery::{DiscoveryWorker, DiscoveryRequest}, ToolAccess};
        let failure = || SidecarError::new("agentDiscoveryRejected", "Claude discovery unavailable");
        validate_agent_adapter_payload_v1("capabilitiesRequest", &serde_json::to_value(request).map_err(|_| failure())?)
            .map_err(|_| failure())?;
        if self.preparing_to_quit || request.runtime != "claude-code"
            || request_id.is_empty() || request_id.len() > 256
            || self.pending_claude_discoveries.contains_key(request_id)
            || self.pending_discoveries.contains_key(request_id)
            || active_run_limit_reached(self.owned_run_ids().len())
        {
            return Err(failure());
        }
        let chat_id = request.chat_id.as_deref().ok_or_else(failure)?;
        let role_id = request.role_id.as_deref().ok_or_else(failure)?;
        let chat = actual_chat_projection(storage, chat_id).map_err(|_| failure())?["record"].clone();
        let profile: AgentResolvedProfileV1 = serde_json::from_value(
            chat["roleSnapshots"].as_array().and_then(|roles| roles.iter().find(|role| role["roleId"] == role_id))
                .cloned().ok_or_else(failure)?,
        ).map_err(|_| failure())?;
        if profile.runtime != "claude-code" { return Err(failure()); }
        let access = match profile.permission.as_str() {
            "readOnly" => ToolAccess::ReadOnly, "workspaceWrite" => ToolAccess::Edit,
            _ => return Err(failure()),
        };
        let address = address_from_chat(&chat).map_err(|_| failure())?;
        projects.verify_runtime_file_isolation(storage, &address).map_err(|_| failure())?;
        let manifest = projects.verify_workspace_for_run(storage, &address).map_err(|_| failure())?;
        let scope = capability_scope(&chat, role_id).map_err(|_| failure())?;
        let worker = DiscoveryWorker::spawn(DiscoveryRequest {
            profile: crate::provider_claude_profile::ClaudeProfileBinding::from_storage(storage)?,
            selector: profile.model_selector.clone(), effort: profile.reasoning_effort.clone().ok_or_else(failure)?, access,
            workspace: PathBuf::from(manifest["worktreePath"].as_str().ok_or_else(failure)?),
            source: PathBuf::from(manifest["canonicalGitRoot"].as_str().ok_or_else(failure)?),
        }, admit)?;
        self.pending_claude_discoveries.insert(request_id.into(), PendingClaudeDiscovery { worker, completion: None, scope, profile });
        Ok(())
    }

    pub(super) fn poll_claude_discoveries(&mut self) -> Result<Vec<G5Event>, SidecarError> {
        let ids: Vec<_> = self.pending_claude_discoveries.keys().cloned().collect();
        let mut failure = None;
        for id in ids {
            let pending = self.pending_claude_discoveries.get_mut(&id).unwrap();
            if pending.completion.is_none() { pending.completion = pending.worker.poll(); }
            let mut cleanup = pending.completion.as_mut().and_then(|done| done.cleanup.take());
            if cleanup.is_some() {
                if let Err(error) = self.retain_discovery_cleanup(&mut cleanup) {
                    self.pending_claude_discoveries.get_mut(&id).unwrap()
                        .completion.as_mut().unwrap().cleanup = cleanup;
                    failure = Some(error);
                }
            }
        }
        match failure { Some(error) => Err(error), None => Ok(Vec::new()) }
    }

    #[allow(dead_code)]
    pub(crate) fn take_claude_discovery(
        &mut self, storage: &Storage, projects: &ProjectManager, request_id: &str,
    ) -> Option<Result<ScopedClaudeEvidence, SidecarError>> {
        let pending = self.pending_claude_discoveries.get(request_id)?;
        if pending.completion.as_ref()?.cleanup.is_some() { return None; }
        let mut pending = self.pending_claude_discoveries.remove(request_id)?;
        let result = pending.completion.take()?.result;
        Some(result.and_then(|evidence| {
            use crate::provider_profile::LoginProfile;
            use crate::provider_claude_session::ToolAccess;
            let failure = || SidecarError::new("agentInputChanged", "Claude discovery context changed");
            if self.preparing_to_quit { return Err(failure()); }
            let chat = actual_chat_projection(storage, &pending.scope.chat_id).map_err(|_| failure())?["record"].clone();
            if capability_scope(&chat, &pending.scope.role_id).map_err(|_| failure())? != pending.scope
                || !chat["roleSnapshots"].as_array().is_some_and(|roles| roles.iter().any(|role| {
                    serde_json::from_value::<AgentResolvedProfileV1>(role.clone()).ok().as_ref() == Some(&pending.profile)
                })) { return Err(failure()); }
            let address = address_from_chat(&chat).map_err(|_| failure())?;
            projects.verify_runtime_file_isolation(storage, &address).map_err(|_| failure())?;
            let manifest = projects.verify_workspace_for_run(storage, &address).map_err(|_| failure())?;
            let home = crate::provider_claude_profile::ClaudeProfileBinding::from_storage(storage)?.revalidate()?;
            let access = match pending.profile.permission.as_str() {
                "readOnly" => ToolAccess::ReadOnly, "workspaceWrite" => ToolAccess::Edit,
                _ => return Err(failure()),
            };
            if !evidence.is_fresh() || !evidence.observation.matches(&pending.profile.model_selector,
                pending.profile.reasoning_effort.as_deref().ok_or_else(failure)?, access,
                Path::new(manifest["worktreePath"].as_str().ok_or_else(failure)?),
                Path::new(manifest["canonicalGitRoot"].as_str().ok_or_else(failure)?), &home) {
                return Err(failure());
            }
            evidence.executable.revalidate()?;
            let scoped = ScopedClaudeEvidence { scope: pending.scope, profile: pending.profile, evidence };
            scoped.capability()?;
            Ok(scoped)
        }))
    }

    /// Start discovery is never served from a capability cache. Resolve the
    /// stored role and input before scheduling the same bounded query worker.
    pub(crate) fn begin_scoped_start(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        request: &AgentRunStartRequestV1,
        request_id: &str,
    ) -> Result<(), SidecarError> {
        let preparation = preparation::RunPreparation::resolve(storage, projects, request)
            .map_err(|_| SidecarError::new("agentInputChanged", "start input is unavailable"))?;
        self.begin_scoped_discovery(
            storage,
            projects,
            &AgentCapabilitiesRequestV1 {
                runtime: preparation.profile.runtime,
                refresh: true,
                chat_id: Some(request.chat_id.clone()),
                role_id: Some(request.role_id.clone()),
            },
            request_id,
        )
    }

    /// Prepare a fresh process, then reserve the durable run. The caller retains
    /// every nonempty slot on error and must explicitly drain/record it; this
    /// function never drops a process after a later preparation failure.
    pub(crate) fn reserve_scoped_start(
        &mut self,
        storage: &mut Storage,
        projects: &ProjectManager,
        request: &AgentRunStartRequestV1,
        request_id: &str,
        observed: &ScopedEvidence,
        reservation: &mut Option<preparation::RunReservation>,
        process: &mut Option<crate::provider_login_process::LoginProcess<'static>>,
    ) -> Result<(), SidecarError> {
        let failure =
            || SidecarError::new("agentInputChanged", "start discovery no longer matches");
        if self.preparing_to_quit
            || reservation.is_some()
            || process.is_some()
            || active_run_limit_reached(self.owned_run_ids().len())
        {
            return Err(failure());
        }
        let capability = observed.capability()?;
        let provider_context = observed.provider_context(&capability)?;
        let preparation = preparation::RunPreparation::resolve(storage, projects, request)
            .map_err(|_| failure())?;
        if observed.scope
            != capability_scope(&preparation.chat, &request.role_id).map_err(|_| failure())?
            || observed.profile != preparation.profile
            || preflight_outcome(request, &preparation.profile, &capability) != "started"
        {
            return Err(failure());
        }
        let binding = crate::provider_profile::CodexProfileBinding::from_storage(storage)?;
        *process = Some(crate::provider_login_process::LoginProcess::reserve_codex_owned(binding)?);
        let permission = match preparation.profile.permission.as_str() {
            "readOnly" => ExpectedPermission::ReadOnly,
            "workspaceWrite" => ExpectedPermission::WorkspaceWrite,
            _ => return Err(failure()),
        };
        let expected =
            hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile {
                model: &preparation.profile.model_selector,
                reasoning_effort: preparation
                    .profile
                    .reasoning_effort
                    .as_deref()
                    .ok_or_else(failure)?,
            };
        let workspace = Path::new(
            preparation.manifest["worktreePath"]
                .as_str()
                .ok_or_else(failure)?,
        );
        let home = process
            .as_ref()
            .ok_or_else(failure)?
            .codex_preflight_home()?;
        if !observed
            .evidence
            .observation
            .matches(&expected, permission, workspace, &home)
        {
            return Err(failure());
        }
        *reservation = Some(
            preparation
                .reserve(storage, projects, &capability, request_id)
                .map_err(|_| failure())?,
        );
        reservation.as_mut().ok_or_else(failure)?.native_provider = Some(provider_context);
        Ok(())
    }

    pub(crate) fn begin_scoped_discovery(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        request: &AgentCapabilitiesRequestV1,
        request_id: &str,
    ) -> Result<(), SidecarError> {
        let failure =
            || SidecarError::new("agentDiscoveryRejected", "scoped discovery unavailable");
        validate_agent_adapter_payload_v1(
            "capabilitiesRequest",
            &serde_json::to_value(request).map_err(|_| failure())?,
        )
        .map_err(|_| failure())?;
        if self.preparing_to_quit
            || request.runtime != "codex"
            || request_id.is_empty()
            || request_id.len() > 256
            || self.pending_discoveries.contains_key(request_id)
            || self.pending_claude_discoveries.contains_key(request_id)
            || active_run_limit_reached(self.owned_run_ids().len())
        {
            return Err(failure());
        }
        let chat_id = request.chat_id.as_deref().ok_or_else(failure)?;
        let role_id = request.role_id.as_deref().ok_or_else(failure)?;
        let chat =
            actual_chat_projection(storage, chat_id).map_err(|_| failure())?["record"].clone();
        let profile: AgentResolvedProfileV1 = serde_json::from_value(
            chat["roleSnapshots"]
                .as_array()
                .and_then(|roles| roles.iter().find(|role| role["roleId"] == role_id))
                .cloned()
                .ok_or_else(failure)?,
        )
        .map_err(|_| failure())?;
        if profile.runtime != request.runtime {
            return Err(failure());
        }
        let permission = match profile.permission.as_str() {
            "readOnly" => ExpectedPermission::ReadOnly,
            "workspaceWrite" => ExpectedPermission::WorkspaceWrite,
            _ => return Err(failure()),
        };
        let address = address_from_chat(&chat).map_err(|_| failure())?;
        projects
            .verify_runtime_file_isolation(storage, &address)
            .map_err(|_| failure())?;
        let manifest = projects
            .verify_workspace_for_run(storage, &address)
            .map_err(|_| failure())?;
        let workspace = PathBuf::from(manifest["worktreePath"].as_str().ok_or_else(failure)?);
        let scope = capability_scope(&chat, role_id).map_err(|_| failure())?;
        let binding = crate::provider_profile::CodexProfileBinding::from_storage(storage)?;
        let worker = DiscoveryWorker::spawn(
            binding,
            profile.model_selector.clone(),
            profile.reasoning_effort.clone().ok_or_else(failure)?,
            permission,
            workspace,
        )?;
        self.pending_discoveries.insert(
            request_id.into(),
            PendingDiscovery {
                worker,
                scope,
                profile,
                completion: None,
            },
        );
        Ok(())
    }

    pub(super) fn poll_discoveries(&mut self) -> Result<Vec<G5Event>, SidecarError> {
        let ids: Vec<_> = self.pending_discoveries.keys().cloned().collect();
        for id in ids {
            let pending = self.pending_discoveries.get_mut(&id).unwrap();
            if pending.completion.is_none() {
                pending.completion = pending.worker.poll();
            }
            let mut cleanup = pending
                .completion
                .as_mut()
                .and_then(|done| done.cleanup.take());
            if cleanup.is_some() {
                if let Err(error) = self.retain_discovery_cleanup(&mut cleanup) {
                    self.pending_discoveries
                        .get_mut(&id)
                        .unwrap()
                        .completion
                        .as_mut()
                        .unwrap()
                        .cleanup = cleanup;
                    return Err(error);
                }
            }
        }
        Ok(Vec::new())
    }

    /// Single-consumer result; recheck durable scope after background work.
    pub(crate) fn take_scoped_discovery(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        request_id: &str,
    ) -> Option<Result<ScopedEvidence, SidecarError>> {
        let pending = self.pending_discoveries.get(request_id)?;
        if !pending
            .completion
            .as_ref()
            .is_some_and(|done| done.cleanup.is_none())
        {
            return None;
        }
        let mut pending = self.pending_discoveries.remove(request_id).unwrap();
        let result = pending.completion.take().unwrap().result;
        Some(result.and_then(|evidence| {
            let failure = || SidecarError::new("agentInputChanged", "discovery context changed");
            if self.preparing_to_quit {
                return Err(failure());
            }
            let chat = actual_chat_projection(storage, &pending.scope.chat_id)
                .map_err(|_| failure())?["record"]
                .clone();
            if capability_scope(&chat, &pending.scope.role_id).map_err(|_| failure())?
                != pending.scope
                || !chat["roleSnapshots"].as_array().is_some_and(|roles| {
                    roles.iter().any(|role| {
                        serde_json::from_value::<AgentResolvedProfileV1>(role.clone())
                            .ok()
                            .as_ref()
                            == Some(&pending.profile)
                    })
                })
            {
                return Err(failure());
            }
            projects
                .verify_workspace_for_run(
                    storage,
                    &address_from_chat(&chat).map_err(|_| failure())?,
                )
                .map_err(|_| failure())?;
            Ok(ScopedEvidence {
                scope: pending.scope,
                profile: pending.profile,
                evidence,
            })
        }))
    }
}
