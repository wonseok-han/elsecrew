//! Dispatcher ownership for prepared native sessions. Registration is not provider
//! admission: product construction/start remains blocked until launch gates pass.
use super::*;
use preparation::OwnedTextRun;

pub(super) struct NativeRun {
    owner: OwnedTextRun,
    pub(super) identity: RunIdentity,
    stream_id: String,
    sequence: u64,
    public_count: usize,
    last_state: Option<Value>,
    stopped: bool,
}

impl NativeRun {
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
        if self.preparing_to_quit || active_run_limit_reached(self.active.len() + self.native.len())
        {
            return Err(failure());
        }
        let identity = owner
            .as_ref()
            .ok_or_else(failure)?
            .registration_identity(storage, projects)?;
        if self.active.contains_key(&identity.run_id) || self.native.contains_key(&identity.run_id)
        {
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
                // Tool notifications grant no permission. Until approval UI is
                // connected, requests are explicitly declined, never accepted.
                active.stopped = !matches!(active.owner.advance(storage, projects, &mut |storage, action| {
                    match action {
                        TextAction::Public(_) | TextAction::Tool(_) => Ok(()),
                        TextAction::PollApproval { request, decision } => {
                            let mut ticket = super::approval::ApprovalTicket::new(&active.identity, &connection, request)?;
                            ticket.record_decline(storage, &active.identity, &connection, request)?;
                            *decision = Some(hello_agent_core::provider::vendor_observation::codex_approval::Decision::Decline);
                            Ok(())
                        }
                        _ => Err(SidecarError::new("agentRecoveryRequired", "unexpected native checkpoint")),
                    }
                }), Ok(None));
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
