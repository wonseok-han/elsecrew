//! Shared durable preparation, without spawning or granting provider authority.
use super::*;
use hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedPermission;
mod terminal;

fn codex_permission(profile: &AgentResolvedProfileV1) -> Result<ExpectedPermission, SidecarError> {
    match (profile.runtime.as_str(), profile.permission.as_str()) {
        ("codex", "readOnly") => Ok(ExpectedPermission::ReadOnly),
        ("codex", "workspaceWrite") => Ok(ExpectedPermission::WorkspaceWrite),
        _ => Err(SidecarError::new(
            "agentInputChanged",
            "reserved role permission is unavailable",
        )),
    }
}

pub(crate) struct RunPreparation {
    request: AgentRunStartRequestV1,
    pub(super) chat: Value,
    pub(super) profile: AgentResolvedProfileV1,
    pub(super) manifest: Value,
    pub(super) prompt: Vec<u8>,
}

pub(crate) struct RunReservation {
    pub(super) run_id: String,
    pub(super) created: String,
    pub(super) run: Value,
    request: AgentRunStartRequestV1,
    prompt: Vec<u8>,
}

impl RunReservation {
    /// Offline OS-isolated launch candidate only, with no model networking.
    /// Pins do not supply provider admission. Caller retains failure cleanup.
    /// Model/effort come from the reserved role, never independent launch arguments.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn launch_codex_candidate(
        &self,
        storage: &mut Storage,
        projects: &ProjectManager,
        process: &mut crate::provider_login_process::LoginProcess<'_>,
        executable: &crate::provider_executable::PinnedExecutable,
        helper: &crate::provider_executable::PinnedExecutable,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<(), SidecarError> {
        use hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
        let failure = || {
            SidecarError::new(
                "agentLaunchRejected",
                "reserved provider launch is unavailable",
            )
        };
        let profile: AgentResolvedProfileV1 =
            serde_json::from_value(self.run["selectedProfile"].clone()).map_err(|_| failure())?;
        let permission = codex_permission(&profile)?;
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(failure());
        }
        projects
            .verify_runtime_file_isolation(
                storage,
                &address_from_chat(&self.run).map_err(|_| failure())?,
            )
            .map_err(|_| failure())?;
        self.validate_text_input(storage, projects)
            .map_err(|_| failure())?;
        let home = process.codex_preflight_home()?;
        crate::provider_profile::verify_runtime_files(&home)?;
        let expected = ExpectedProfile {
            model: &profile.model_selector,
            reasoning_effort: profile.reasoning_effort.as_deref().ok_or_else(failure)?,
        };
        executable.revalidate()?;
        helper.revalidate()?;
        // Construct the exact invocation before creating any child. The command
        // starts the existing inert gate; only the identity-recorded owner releases it.
        let manifest = projects
            .verify_workspace(
                storage,
                &address_from_chat(&self.run).map_err(|_| failure())?,
            )
            .map_err(|_| failure())?;
        let workspace =
            std::path::Path::new(manifest["worktreePath"].as_str().ok_or_else(failure)?);
        let mut command = crate::provider_preflight_session::offline_discovery_command(
            executable.revalidate()?,
            helper.revalidate()?,
            &home,
            workspace,
            &expected,
            permission,
        )
        .map_err(|_| failure())?;
        self.launch_gated(
            storage,
            projects,
            process,
            || command.spawn(),
            |child| {
                #[cfg(test)]
                crate::provider_preflight_session::BEFORE_RELEASE.with(|hook| {
                    if let Some(hook) = hook.borrow_mut().take() {
                        hook();
                    }
                });
                executable.revalidate().map_err(std::io::Error::other)?;
                helper.revalidate().map_err(std::io::Error::other)?;
                crate::provider_profile::verify_runtime_files(&home)
                    .map_err(std::io::Error::other)?;
                if cancel.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(std::io::Error::other("provider launch canceled"));
                }
                crate::provider_login_gate::release(child)
            },
        )
    }

    /// Internal read-only candidate: inputs come only from this reservation.
    /// Deltas remain provisional. Completed items use the existing public ledger;
    /// only a completed turn with confirmed cleanup may publish its final answer.
    /// The process owner's explicit closure evidence controls terminal persistence.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn run_read_only_text(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        process: &mut crate::provider_login_process::LoginProcess<'_>,
        cancel: &std::sync::atomic::AtomicBool,
        on_public: &mut impl FnMut(
            &mut Storage,
            hello_agent_core::provider::vendor_observation::codex_events::Projection,
        ) -> Result<(), SidecarError>,
    ) -> Result<
        hello_agent_core::provider::vendor_observation::codex_events::stream::TurnOutcome,
        SidecarError,
    > {
        if self.run["selectedProfile"]["permission"] != "readOnly" {
            return Err(SidecarError::new(
                "agentInputChanged",
                "reserved text input is unavailable",
            ));
        }
        self.run_text_candidate(storage, projects, process, cancel, on_public)
    }

    /// Internal role-bound transport; tools still require a separate handler.
    /// This does not admit a provider or grant OS permissions to a process.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn run_text_candidate(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        process: &mut crate::provider_login_process::LoginProcess<'_>,
        cancel: &std::sync::atomic::AtomicBool,
        on_public: &mut impl FnMut(
            &mut Storage,
            hello_agent_core::provider::vendor_observation::codex_events::Projection,
        ) -> Result<(), SidecarError>,
    ) -> Result<
        hello_agent_core::provider::vendor_observation::codex_events::stream::TurnOutcome,
        SidecarError,
    > {
        use hello_agent_core::provider::vendor_observation::codex_events::{
            stream::TurnOutcome, Projection, PublicKind,
        };
        use hello_agent_core::provider::vendor_observation::codex_preflight::{
            exchange::PreflightExchange, ExpectedProfile,
        };
        let failure =
            || SidecarError::new("agentInputChanged", "reserved text input is unavailable");
        let profile: AgentResolvedProfileV1 =
            serde_json::from_value(self.run["selectedProfile"].clone()).map_err(|_| failure())?;
        let permission = codex_permission(&profile)?;
        self.validate_text_input(storage, projects)
            .map_err(|_| failure())?;
        // Reject a different owner before the transport acquires cleanup authority.
        self.observe_process(storage, process)?;
        let home = process.codex_preflight_home()?;
        let flow = PreflightExchange::with_permission(
            &ExpectedProfile {
                model: &profile.model_selector,
                reasoning_effort: profile.reasoning_effort.as_deref().ok_or_else(failure)?,
            },
            home.to_str().ok_or_else(failure)?,
            permission,
        )
        .map_err(|_| failure())?;
        let text = std::str::from_utf8(&self.prompt).map_err(|_| failure())?;
        let address = address_from_chat(&self.run).map_err(|_| failure())?;
        let mut workspace =
            crate::provider_preflight_session::TextWorkspace::new(projects, storage, &address)?;
        let mut pending_answer = None;
        process.retain_fence_until_terminal()?;
        let outcome = crate::provider_preflight_session::run_text_and_close(
            process,
            flow,
            cancel,
            &mut workspace,
            text,
            &mut |storage, event| {
                // Completed text replaces deltas; never append the same answer twice.
                if let Projection::Completed { kind, text, .. } = &event {
                    match kind {
                        PublicKind::AgentMessage => {
                            if let Some(previous) = pending_answer.take() {
                                self.persist_text(storage, "status", previous)?;
                            }
                            pending_answer = Some(text.clone());
                        }
                        PublicKind::Plan => self.persist_text(storage, "status", text.clone())?,
                    }
                }
                on_public(storage, event)
            },
            &mut |storage, process| {
                self.observe_process(storage, process)?;
                self.validate_text_input(storage, projects)
                    .map_err(|_| failure())
            },
        );
        // The transport has returned only after observed cleanup. An interrupted
        // or failed turn may preserve text, but must not label it a final answer.
        let result = outcome.and_then(|outcome| {
            self.validate_text_input(storage, projects)
                .map_err(|_| failure())?;
            if let Some(answer) = pending_answer {
                self.persist_text(
                    storage,
                    if outcome == TurnOutcome::Completed {
                        "final"
                    } else {
                        "status"
                    },
                    answer,
                )?;
            }
            Ok(outcome)
        });
        self.persist_text_terminal(
            storage,
            projects,
            process,
            &result,
            cancel.load(std::sync::atomic::Ordering::Acquire),
        )
        .map_err(|_| {
            SidecarError::new(
                "agentTerminalPersistenceFailed",
                "execution requires recovery; terminal state was not stored",
            )
        })?;
        if process
            .closure_evidence()
            .is_some_and(|evidence| evidence.confirmed())
        {
            process.release_after_run_terminal()?;
        }
        result
    }

    fn persist_text(
        &self,
        storage: &mut Storage,
        kind: &str,
        text: String,
    ) -> Result<(), SidecarError> {
        let event = json!({
            "schemaVersion":1, "eventId":format!("agent_event_{}", Uuid::new_v4().simple()),
            "chatId":self.run["chatId"], "taskId":self.run["taskId"],
            "projectId":self.run["projectId"], "worktreeId":self.run["worktreeId"],
            "workspaceIdentity":self.run["workspaceIdentity"], "runId":self.run_id,
            "roleId":self.run["selectedProfile"]["roleId"],
            "capabilityFingerprint":self.run["capabilityFingerprint"],
            "kind":kind, "text":text, "createdAt":now()
        });
        persist_public_event_record(storage, &self.run, &event).map_err(|_| {
            SidecarError::new(
                "agentOutputPersistenceFailed",
                "public output could not be stored within the run budget",
            )
        })
    }

    fn validate_text_input(
        &self,
        storage: &Storage,
        projects: &ProjectManager,
    ) -> Result<(), ProjectFailure> {
        let projection = actual_run_projection(storage, &self.run_id)?;
        let chat = actual_chat_projection(storage, &self.request.chat_id)?;
        let role = chat["record"]["roleSnapshots"]
            .as_array()
            .and_then(|roles| {
                roles
                    .iter()
                    .find(|role| role["roleId"] == self.request.role_id)
            });
        if projection["record"] != self.run
            || projection["state"] != "preparing"
            || !projection["recovery"].is_null()
            || role != Some(&self.run["selectedProfile"])
            || address_from_chat(&chat["record"])? != address_from_chat(&self.run)?
            || resolve_prompt(
                storage,
                &self.request.chat_id,
                &self.request.input_message_ids,
            )? != self.prompt
            || projects.verify_workspace(storage, &address_from_chat(&self.run)?)?
                != projection["workspace_manifest"]
        {
            return Err(ProjectFailure::recovery("reserved text input changed"));
        }
        Ok(())
    }

    /// Keep the run's cumulative process evidence in step with its sole owner.
    /// A failed observation is never upgraded by a successful evidence write.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn observe_process(
        &self,
        storage: &mut Storage,
        process: &mut crate::provider_login_process::LoginProcess<'_>,
    ) -> Result<(), SidecarError> {
        let (identities, observed) = process.observe_for_run();
        let saved = (|| -> Result<(), ProjectFailure> {
            let projection = actual_run_projection(storage, &self.run_id)?;
            if projection["record"] != self.run
                || projection["state"] != "preparing"
                || projection["barrier"]["phase"] != "providerLaunchGate"
                || projection["barrier"]["releaseGranted"] != true
                || !projection["recovery"].is_null()
            {
                return Err(ProjectFailure::recovery(
                    "run no longer accepts process observations",
                ));
            }
            let prior = projection["process_identity_internal"]
                .as_array()
                .ok_or_else(|| ProjectFailure::recovery("run process binding is missing"))?;
            if prior.is_empty()
                || prior.iter().any(|item| {
                    !identities.iter().any(|identity| {
                        item["pid"].as_u64() == Some(identity.pid as u64)
                            && item["start_us"].as_i64() == Some(identity.start_us)
                    })
                })
            {
                return Err(ProjectFailure::recovery(
                    "process owner differs from reserved run",
                ));
            }
            persist_tracker_snapshot(storage, &self.run_id, &identities)
        })();
        if saved.is_err() || observed.is_err() {
            process.invalidate_transport();
            return Err(SidecarError::new(
                "agentProcessObservationFailed",
                "run process evidence requires cleanup",
            ));
        }
        Ok(())
    }

    /// Keep process ownership with the caller on every failure, including a
    /// durable-write failure AFTER a successful release. No retry/relaunch here.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn launch_gated(
        &self,
        storage: &mut Storage,
        projects: &ProjectManager,
        process: &mut crate::provider_login_process::LoginProcess<'_>,
        spawn: impl FnOnce() -> std::io::Result<Child>,
        release: impl FnOnce(&mut Child) -> std::io::Result<()>,
    ) -> Result<(), SidecarError> {
        process.launch_with_identity_barrier(spawn, |child, identities| {
            self.record_pre_release(storage, projects, identities)
                .map_err(|_| std::io::Error::other("run identity persistence failed"))?;
            release(child)
        })?;
        self.record_released(storage).map_err(|_| {
            process.invalidate_transport();
            SidecarError::new(
                "agentReleasePersistenceFailed",
                "released process requires cleanup; run evidence could not be updated",
            )
        })
    }

    fn record_released(&self, storage: &mut Storage) -> Result<(), ProjectFailure> {
        let mut projection = actual_run_projection(storage, &self.run_id)?;
        if projection["record"] != self.run
            || projection["state"] != "preparing"
            || !projection["recovery"].is_null()
            || projection["barrier"]["phase"] != "providerLaunchGate"
            || projection["barrier"]["releaseGranted"] != false
            || !projection["process_identity_internal"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        {
            return Err(ProjectFailure::recovery("run cannot record gate release"));
        }
        projection.get_mut("barrier").unwrap()["releaseGranted"] = json!(true);
        projection.get_mut("barrier").unwrap()["capturedAt"] = json!(now());
        storage.upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: self.run_id.clone(),
            operation_id: format!("op_agent_released_{}", self.run_id),
            request_id: None,
            payload: projection,
        }])?;
        Ok(())
    }

    /// Record the gated process tree, not a vendor handshake/attestation. The
    /// caller must use the lifecycle owner's observations, before gate release.
    #[cfg_attr(not(test), allow(dead_code))] // Native run owner is not enabled yet.
    pub(crate) fn record_pre_release(
        &self,
        storage: &mut Storage,
        projects: &ProjectManager,
        identities: &[ProcIdentity],
    ) -> Result<(), ProjectFailure> {
        let mut projection = actual_run_projection(storage, &self.run_id)?;
        if projection["record"] != self.run
            || projection["state"] != "preparing"
            || !projection["barrier"].is_null()
            || !projection["process_identity_internal"].is_null()
            || !projection["recovery"].is_null()
        {
            return Err(ProjectFailure::recovery(
                "run no longer accepts pre-release identity",
            ));
        }
        let address = address_from_chat(&self.run)?;
        projects.verify_runtime_file_isolation(storage, &address)?;
        let manifest = projects.verify_workspace(storage, &address)?;
        if projection["workspace_manifest"] != manifest {
            return Err(ProjectFailure::recovery(
                "reserved workspace changed before release",
            ));
        }
        let lock = storage
            .projection_record("lock", &format!("lock_{}", address.worktree_id))?
            .ok_or_else(|| ProjectFailure::recovery("reserved workspace lock is missing"))?;
        let expected_writer = if self.run["selectedProfile"]["permission"] == "workspaceWrite" {
            json!(self.run_id)
        } else {
            Value::Null
        };
        if lock.get("writer_run_id") != Some(&expected_writer) {
            return Err(ProjectFailure::recovery(
                "reserved writer changed before release",
            ));
        }
        let mut unique = BTreeSet::new();
        if identities.is_empty()
            || identities.iter().any(|identity| {
                identity.pid == 0
                    || identity.start_us == 0
                    || !unique.insert((identity.pid, identity.start_us))
            })
        {
            return Err(ProjectFailure::recovery(
                "invalid observed gated process identities",
            ));
        }
        projection.insert(
            "process_identity_internal".into(),
            json!(identities
                .iter()
                .map(|identity| json!({"pid":identity.pid,"start_us":identity.start_us}))
                .collect::<Vec<_>>()),
        );
        projection.insert("barrier".into(), json!({
            "phase":"providerLaunchGate", "releaseGranted":false,
            "trackedProcesses":identities.iter().map(|identity| process_value(*identity, Path::new("/redacted/agent"), false)).collect::<Vec<_>>(),
            "capturedAt":now()
        }));
        storage.upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: self.run_id.clone(),
            operation_id: format!("op_agent_identity_{}", self.run_id),
            request_id: None,
            payload: projection,
        }])?;
        Ok(())
    }
}

impl RunPreparation {
    pub(crate) fn resolve(
        storage: &Storage,
        projects: &ProjectManager,
        request: &AgentRunStartRequestV1,
    ) -> Result<Self, ProjectFailure> {
        validate_agent_adapter_payload_v1(
            "runStartRequest",
            &serde_json::to_value(request).map_err(json_failure)?,
        )
        .map_err(contract_failure)?;
        let chat = actual_chat_projection(storage, &request.chat_id)?["record"].clone();
        if !G5_RUN_START_CHAT_STATES_V1.contains(&chat["state"].as_str().unwrap_or_default()) {
            return Err(ProjectFailure::recovery(
                "chat state does not allow a new agent run",
            ));
        }
        if storage.projection_records("run")?.iter().any(|run| {
            run.get("chat_id").and_then(Value::as_str) == Some(request.chat_id.as_str())
                && run
                    .get("state")
                    .and_then(Value::as_str)
                    .is_some_and(|state| {
                        ["preparing", "running", "cancelling", "recoveryRequired"].contains(&state)
                    })
        }) {
            return Err(ProjectFailure::recovery(
                "chat already has an active or poisoned run",
            ));
        }
        let profile = serde_json::from_value(
            chat["roleSnapshots"]
                .as_array()
                .and_then(|roles| roles.iter().find(|role| role["roleId"] == request.role_id))
                .cloned()
                .ok_or_else(|| ProjectFailure::manifest("chat role snapshot is missing"))?,
        )
        .map_err(json_failure)?;
        let manifest = projects.verify_workspace(storage, &address_from_chat(&chat)?)?;
        let prompt = resolve_prompt(storage, &request.chat_id, &request.input_message_ids)?;
        Ok(Self {
            request: request.clone(),
            chat,
            profile,
            manifest,
            prompt,
        })
    }

    /// Caller must supply freshly admitted provider evidence and own the single
    /// runtime writer. This does not create an OS sandbox or release a payload.
    pub(crate) fn reserve(
        self,
        storage: &mut Storage,
        projects: &ProjectManager,
        capability: &AgentCapabilitySnapshotV1,
        request_id: &str,
    ) -> Result<RunReservation, ProjectFailure> {
        let current = Self::resolve(storage, projects, &self.request)?;
        if current.chat != self.chat
            || current.manifest != self.manifest
            || current.prompt != self.prompt
            || serde_json::to_value(&current.profile).map_err(json_failure)?
                != serde_json::to_value(&self.profile).map_err(json_failure)?
        {
            return Err(ProjectFailure::recovery(
                "run input changed before reservation",
            ));
        }
        validate_agent_adapter_payload_v1(
            "capabilitySnapshot",
            &serde_json::to_value(capability).map_err(json_failure)?,
        )
        .map_err(contract_failure)?;
        if preflight_outcome(&self.request, &self.profile, capability) != "started" {
            return Err(ProjectFailure::manifest(
                "provider evidence does not admit this run",
            ));
        }
        let run_id = format!("agent_run_{}", Uuid::new_v4().simple());
        let created = now();
        let run = agent_run_record(
            &run_id,
            &self.chat,
            &self.profile,
            &capability.fingerprint,
            "preparing",
            None,
            &created,
        )?;
        let mut projection = agent_run_projection(&run, &self.manifest, None, None, Vec::new())?;
        projection.insert(
            RECOVERY_RESERVATION_FIELD.into(),
            Value::String(new_recovery_id()),
        );
        let mut upserts = vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: format!("op_agent_run_prepare_{request_id}"),
            request_id: Some(request_id.into()),
            payload: projection,
        }];
        if self.profile.permission == "workspaceWrite" {
            upserts.push(acquire_writer_lock(
                storage, &self.chat, &run_id, request_id,
            )?);
        }
        storage.upsert_projection_records_atomic(upserts)?;
        Ok(RunReservation {
            run_id,
            created,
            run,
            request: self.request,
            prompt: self.prompt,
        })
    }
}
