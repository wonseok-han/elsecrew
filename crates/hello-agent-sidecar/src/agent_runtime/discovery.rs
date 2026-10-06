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

impl ScopedEvidence {
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
