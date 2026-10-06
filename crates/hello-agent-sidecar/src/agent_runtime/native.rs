//! Dispatcher ownership for prepared native sessions. Registration is not provider
//! admission: product construction/start remains blocked until launch gates pass.
use super::*;
use preparation::OwnedTextRun;

pub(super) struct FailedStart {
    reservation: preparation::RunReservation,
    process: crate::provider_login_process::LoginProcess<'static>,
    stream_id: String,
    persisted: bool,
}

pub(super) struct NativeRun {
    owner: OwnedTextRun,
    pub(super) identity: RunIdentity,
    stream_id: String,
    sequence: u64,
    public_count: usize,
    last_state: Option<Value>,
    stopped: bool,
    approvals: super::approval::ApprovalQueue,
}

impl NativeRun {
    fn live_approvals(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        identity: &RunIdentity,
    ) -> Result<&mut super::approval::ApprovalQueue, SidecarError> {
        if self.stopped
            || identity_value(&self.identity) != identity_value(identity)
            || identity_value(&self.owner.registration_identity(storage, projects)?)
                != identity_value(identity)
        {
            return Err(SidecarError::new(
                "agentApprovalChanged",
                "approval run is no longer current",
            ));
        }
        let connection = format!("{}:{}", self.stream_id, self.owner.connection_id());
        self.approvals
            .retain_live(identity, &connection, &self.owner.pending_approvals())?;
        Ok(&mut self.approvals)
    }

    pub(super) fn request_cancel(&self, identity: &RunIdentity) -> Result<(), ProjectFailure> {
        self.owner
            .request_cancel(identity)
            .map_err(|_| ProjectFailure::manifest("native cancellation identity or state differs"))
    }

    fn events(&mut self, storage: &Storage) -> Result<(Vec<G5Event>, bool), SidecarError> {
        let failure = || SidecarError::new("agentRecoveryRequired", "native event ledger differs");
        let projection =
            actual_run_projection(storage, &self.identity.run_id).map_err(|_| failure())?;
        let run = &projection["record"];
        if identity_value(&self.identity)
            .as_object()
            .unwrap()
            .iter()
            .any(|(key, value)| run.get(key) != Some(value))
        {
            return Err(failure());
        }
        let public = projection["public_events"].as_array().ok_or_else(failure)?;
        if public.len() < self.public_count {
            return Err(failure());
        }
        let mut sequence = self.sequence;
        let mut events = Vec::new();
        let mut emit = |event, payload| -> Result<(), SidecarError> {
            sequence = sequence.checked_add(1).ok_or_else(failure)?;
            events.push(G5Event {
                stream_id: self.stream_id.clone(),
                seq: sequence,
                event,
                chat_id: Some(self.identity.chat_id.clone()),
                task_id: Some(self.identity.task_id.clone()),
                run_id: Some(self.identity.run_id.clone()),
                payload,
            });
            Ok(())
        };
        for event in &public[self.public_count..] {
            validate_agent_adapter_payload_v1("publicEvent", event).map_err(|_| failure())?;
            if identity_value(&self.identity)
                .as_object()
                .unwrap()
                .iter()
                .any(|(key, value)| event.get(key) != Some(value))
            {
                return Err(failure());
            }
            emit("agent.publicEvent", event.clone())?;
        }
        // `preparing` is an internal reservation, not a wire run-state event.
        // Do not fabricate `running` before native admission/attestation exists.
        if run["state"] != "preparing" && self.last_state.as_ref() != Some(run) {
            let payload = json!({"schemaVersion":1,
                "eventId":format!("agent_state_event_{}",Uuid::new_v4().simple()),"run":run});
            validate_agent_adapter_payload_v1("runStateChanged", &payload)
                .map_err(|_| failure())?;
            emit("agent.runStateChanged", payload)?;
        }
        self.sequence = sequence;
        self.public_count = public.len();
        self.last_state = Some(run.clone());
        let terminal = matches!(
            run["state"].as_str(),
            Some("succeeded" | "failed" | "canceled" | "recoveryRequired")
        );
        Ok((events, terminal))
    }
}

impl AgentRuntime {
    /// Cleanup ownership must still be accepted during shutdown or at capacity:
    /// this process already exists. Rejection never consumes the caller's owner.
    #[allow(dead_code)] // Product start dispatcher is not activated yet.
    pub(crate) fn register_failed_start(
        &mut self,
        reservation: &mut Option<preparation::RunReservation>,
        process: &mut Option<crate::provider_login_process::LoginProcess<'static>>,
    ) -> Result<(), SidecarError> {
        let failure = || SidecarError::new("agentRecoveryRequired", "startup owner differs");
        let pending = reservation.as_ref().ok_or_else(failure)?;
        let owner = process.as_ref().ok_or_else(failure)?;
        if self.has_active_run(&pending.run_id) || !owner.matches_startup_run(&pending.run_id) {
            return Err(failure());
        }
        self.failed_starts.insert(
            pending.run_id.clone(),
            FailedStart {
                reservation: reservation.take().ok_or_else(failure)?,
                process: process.take().ok_or_else(failure)?,
                stream_id: format!("agent_stream_{}", Uuid::new_v4().simple()),
                persisted: false,
            },
        );
        Ok(())
    }

    pub(super) fn poll_failed_starts(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Result<Vec<G5Event>, SidecarError> {
        let mut events = Vec::new();
        let mut cleanup_error = None;
        let ids: Vec<_> = self.failed_starts.keys().cloned().collect();
        for id in ids {
            let pending = self.failed_starts.get_mut(&id).unwrap();
            if !pending.persisted {
                // Keep ownership on cleanup or persistence errors. Other owners
                // must still get their cleanup poll even if this one is blocked.
                match pending
                    .reservation
                    .poll_failed_start(storage, projects, &mut pending.process)
                {
                    Ok(true) => pending.persisted = true,
                    Ok(false) => continue,
                    Err(error) => {
                        cleanup_error = Some(error);
                        continue;
                    }
                }
            }
            let failure = || SidecarError::new("agentRecoveryRequired", "startup terminal differs");
            // Terminal persistence is remembered independently from release.
            // A retry must not validate the already-terminal run as preparing
            // or repeat its terminal transaction.
            if pending
                .process
                .closure_evidence()
                .is_some_and(|evidence| evidence.confirmed())
                && pending.process.retains_run_fence()
            {
                if let Err(error) = pending.process.release_after_run_terminal() {
                    cleanup_error = Some(error);
                    continue;
                }
            }
            let projection = actual_run_projection(storage, &id).map_err(|_| failure())?;
            let run = &projection["record"];
            if !matches!(run["state"].as_str(), Some("failed" | "recoveryRequired")) {
                return Err(failure());
            }
            let payload = json!({"schemaVersion":1,
                "eventId":format!("agent_state_event_{}",Uuid::new_v4().simple()),"run":run});
            validate_agent_adapter_payload_v1("runStateChanged", &payload)
                .map_err(|_| failure())?;
            events.push(G5Event {
                stream_id: pending.stream_id.clone(),
                seq: 1,
                event: "agent.runStateChanged",
                chat_id: Some(run["chatId"].as_str().ok_or_else(failure)?.into()),
                task_id: Some(run["taskId"].as_str().ok_or_else(failure)?.into()),
                run_id: Some(id.clone()),
                payload,
            });
            self.failed_starts.remove(&id);
        }
        if events.is_empty() {
            if let Some(error) = cleanup_error {
                return Err(error);
            }
        }
        Ok(events)
    }

    pub(crate) fn approval_request(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        params: &Value,
    ) -> Result<Value, SidecarError> {
        let invalid = || SidecarError::new("invalidParams", "invalid approval request");
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let keys: &[&str] = match action {
            "list" => &["action", "identity"],
            "decline" | "inspect" => &["action", "identity", "approvalId"],
            _ => return Err(invalid()),
        };
        let object = params.as_object().ok_or_else(invalid)?;
        if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
            return Err(invalid());
        }
        let raw = &params["identity"];
        validate_agent_adapter_payload_v1("runCancelRequest", raw).map_err(|_| invalid())?;
        let identity = RunIdentity {
            chat_id: raw["chatId"].as_str().unwrap().into(),
            task_id: raw["taskId"].as_str().unwrap().into(),
            project_id: raw["projectId"].as_str().unwrap().into(),
            worktree_id: raw["worktreeId"].as_str().unwrap().into(),
            workspace_identity: raw["workspaceIdentity"].as_str().unwrap().into(),
            run_id: raw["runId"].as_str().unwrap().into(),
        };
        let queued = if action == "decline" {
            let id = params["approvalId"].as_str().ok_or_else(invalid)?;
            if !id.strip_prefix("approval_").is_some_and(|suffix| {
                suffix.len() == 32
                    && suffix
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            }) {
                return Err(invalid());
            }
            self.decline_native_approval(storage, projects, &identity, id)?;
            Some(id)
        } else {
            None
        };
        let active = self.native.get_mut(&identity.run_id).ok_or_else(|| {
            SidecarError::new("agentApprovalChanged", "approval run is no longer current")
        })?;
        active.live_approvals(storage, projects, &identity)?;
        let connection = format!("{}:{}", active.stream_id, active.owner.connection_id());
        let pending = active.approvals.summaries(
            &identity,
            &connection,
            &active.owner.pending_approvals(),
        )?;
        let mut result = json!({"identity":raw,"pending":pending,"queued":queued});
        if action == "inspect" {
            let id = params["approvalId"].as_str().ok_or_else(invalid)?;
            result["inspection"] = active.approvals.inspect(
                &identity,
                &connection,
                &active.owner.pending_approvals(),
                id,
            )?;
        }
        Ok(result)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn pending_native_approvals(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        identity: &RunIdentity,
    ) -> Result<Vec<String>, SidecarError> {
        let active = self.native.get_mut(&identity.run_id).ok_or_else(|| {
            SidecarError::new("agentApprovalChanged", "approval run is no longer current")
        })?;
        Ok(active.live_approvals(storage, projects, identity)?.ids())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn decline_native_approval(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        identity: &RunIdentity,
        approval_id: &str,
    ) -> Result<(), SidecarError> {
        let active = self.native.get_mut(&identity.run_id).ok_or_else(|| {
            SidecarError::new("agentApprovalChanged", "approval run is no longer current")
        })?;
        active
            .live_approvals(storage, projects, identity)?
            .decline(approval_id)
    }

    /// Internal candidate only. Public IPC acceptance is a separate UI boundary.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn accept_native_approval_once(
        &mut self,
        storage: &mut Storage,
        projects: &ProjectManager,
        identity: &RunIdentity,
        approval_id: &str,
    ) -> Result<(), SidecarError> {
        let active = self.native.get_mut(&identity.run_id).ok_or_else(|| {
            SidecarError::new("agentApprovalChanged", "approval run is no longer current")
        })?;
        active
            .live_approvals(storage, projects, identity)?
            .validate_acceptance(approval_id)?;
        active.owner.verify_approval_boundary(storage, projects)?;
        active.approvals.accept_once(approval_id)
    }

    /// Take the owner only after validation; rejection leaves cleanup responsibility
    /// with the caller. No public IPC/production start path invokes this yet.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn register_native_candidate(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        owner: &mut Option<OwnedTextRun>,
    ) -> Result<(), SidecarError> {
        let failure = || {
            SidecarError::new(
                "agentRegistrationRejected",
                "native registration unavailable",
            )
        };
        if self.preparing_to_quit
            || active_run_limit_reached(
                self.active.len() + self.native.len() + self.failed_starts.len(),
            )
        {
            return Err(failure());
        }
        let identity = owner
            .as_ref()
            .ok_or_else(failure)?
            .registration_identity(storage, projects)?;
        if self.has_active_run(&identity.run_id) {
            return Err(failure());
        }
        self.native.insert(
            identity.run_id.clone(),
            NativeRun {
                owner: owner.take().ok_or_else(failure)?,
                identity,
                stream_id: format!("agent_stream_{}", Uuid::new_v4().simple()),
                sequence: 0,
                public_count: 0,
                last_state: None,
                stopped: false,
                approvals: Default::default(),
            },
        );
        Ok(())
    }

    pub(super) fn poll_native(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
    ) -> Result<Vec<G5Event>, SidecarError> {
        use crate::provider_preflight_session::TextAction;
        let mut events = Vec::new();
        let ids: Vec<_> = self.native.keys().cloned().collect();
        for id in ids {
            let active = self.native.get_mut(&id).unwrap();
            if !active.stopped {
                let connection = format!("{}:{}", active.stream_id, active.owner.connection_id());
                // Tool notifications grant no permission. Only an explicit
                // queued decision can produce a reply; waiting keeps polling.
                active.stopped = !matches!(
                    active
                        .owner
                        .advance(storage, projects, &mut |storage, action| {
                            match action {
                                TextAction::Public(_) | TextAction::Tool(_) => Ok(()),
                                TextAction::PollApproval { request, decision } => {
                                    *decision = active.approvals.poll(
                                        storage,
                                        &active.identity,
                                        &connection,
                                        request,
                                    )?;
                                    Ok(())
                                }
                                _ => Err(SidecarError::new(
                                    "agentRecoveryRequired",
                                    "unexpected native checkpoint",
                                )),
                            }
                        }),
                    Ok(None)
                );
                if active
                    .approvals
                    .retain_live(
                        &active.identity,
                        &connection,
                        &active.owner.pending_approvals(),
                    )
                    .is_err()
                {
                    active.owner.request_cancel(&active.identity)?;
                }
            }
            let (emitted, terminal) = active.events(storage)?;
            if active.stopped && !terminal {
                // Keep the owner and durable fence on terminal persistence failure.
                return Err(SidecarError::new(
                    "agentRecoveryRequired",
                    "native terminal requires recovery",
                ));
            }
            events.extend(emitted);
            if active.stopped && terminal {
                self.native.remove(&id);
            }
        }
        Ok(events)
    }
}
