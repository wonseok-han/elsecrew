//! Persist only closure evidence issued by the owner of this reserved process.
use super::*;
use hello_agent_core::provider::vendor_observation::codex_events::stream::TurnOutcome;

impl RunReservation {
    pub(super) fn persist_text_terminal(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        process: &crate::provider_login_process::LoginProcess<'_>,
        result: &Result<TurnOutcome, SidecarError>,
        cancel_requested: bool,
    ) -> Result<(), ProjectFailure> {
        let evidence = process
            .closure_evidence()
            .ok_or_else(|| ProjectFailure::recovery("explicit process closure is missing"))?;
        let projection = actual_run_projection(storage, &self.run_id)?;
        let identities = evidence.identities();
        let prior = projection["process_identity_internal"]
            .as_array()
            .ok_or_else(|| ProjectFailure::recovery("reserved process identities are missing"))?;
        if projection["record"] != self.run
            || projection["state"] != "preparing"
            || !projection["recovery"].is_null()
            || projection["barrier"]["phase"] != "providerLaunchGate"
            || projection["barrier"]["releaseGranted"] != true
            || self.run["selectedProfile"]["permission"] != "readOnly"
            || prior.is_empty()
            || identities.is_empty()
            || prior.iter().any(|item| {
                !identities.iter().any(|identity| {
                    item["pid"].as_u64() == Some(identity.pid as u64)
                        && item["start_us"].as_i64() == Some(identity.start_us)
                })
            })
        {
            return Err(ProjectFailure::recovery(
                "closure does not belong to the reserved run",
            ));
        }
        let chat_projection = actual_chat_projection(storage, &self.request.chat_id)?;
        let chat = &chat_projection["record"];
        let address = address_from_chat(&self.run)?;
        if address_from_chat(chat)? != address {
            return Err(ProjectFailure::recovery("terminal chat binding changed"));
        }
        let confirmed = evidence.confirmed();
        let state = if !confirmed {
            "recoveryRequired"
        } else if cancel_requested {
            "canceled"
        } else {
            match result {
                Ok(TurnOutcome::Completed) => "succeeded",
                Ok(TurnOutcome::Interrupted) => "canceled",
                _ => "failed",
            }
        };
        let outcome = if confirmed {
            "cleanExit"
        } else {
            "recoveryRequired"
        };
        let tracked: Vec<_> = identities
            .iter()
            .map(|identity| process_value(*identity, Path::new("/redacted/agent"), confirmed))
            .collect();
        let termination = json!({
            "schemaVersion":1,"runId":self.run_id,"outcome":outcome,
            "processesProvenGone":confirmed,"poisoned":!confirmed,
            "writerLockState":if confirmed {"none"} else {"recoveryRequired"},
            "chatState":state,"trackedProcesses":tracked,
            "evidenceDigest":digest_text(&serde_json::to_string(&json!({"runId":self.run_id,"trackedProcesses":tracked,"confirmed":confirmed})).map_err(json_failure)?),
            "observedAt":now()
        });
        validate_agent_adapter_payload_v1("terminationTranscript", &termination)
            .map_err(contract_failure)?;
        let recovery = if confirmed {
            Value::Null
        } else {
            let recovery = json!({
                "schemaVersion":1,"recoveryId":reserved_recovery_id(&projection)?,
                "chatId":self.run["chatId"],"taskId":self.run["taskId"],
                "projectId":self.run["projectId"],"worktreeId":self.run["worktreeId"],
                "workspaceIdentity":self.run["workspaceIdentity"],"runId":self.run_id,
                "source":"actualAgent","reason":"processIdentityUncertain","state":"unresolved",
                "trackedProcesses":tracked,"poisoned":true,"revision":1,"createdAt":now(),"resolvedAt":null
            });
            validate_agent_adapter_payload_v1("recoveryRecord", &recovery)
                .map_err(contract_failure)?;
            recovery
        };
        if projects.verify_workspace(storage, &address)? != projection["workspace_manifest"] {
            return Err(ProjectFailure::recovery(
                "terminal workspace binding changed",
            ));
        }
        let profile =
            serde_json::from_value(self.run["selectedProfile"].clone()).map_err(json_failure)?;
        let run = agent_run_record(
            &self.run_id,
            chat,
            &profile,
            self.run["capabilityFingerprint"]
                .as_str()
                .unwrap_or_default(),
            state,
            Some(outcome),
            &self.created,
        )?;
        let updated_chat = updated_chat(
            chat,
            match state {
                "succeeded" if self.request.role_id == "planning" => "waitingForUser",
                "succeeded" => "reviewReady",
                "canceled" => "canceled",
                "recoveryRequired" => "recoveryRequired",
                _ => "needsAttention",
            },
        )?;
        let mut updated_run = projection.clone();
        updated_run.remove("last_event_sequence");
        updated_run.insert("record".into(), run);
        updated_run.insert("state".into(), json!(state));
        updated_run.insert("termination".into(), termination);
        updated_run.insert("recovery".into(), recovery);
        updated_run.insert(
            "process_identity_internal".into(),
            json!(identities
                .iter()
                .map(|identity| json!({"pid":identity.pid,"start_us":identity.start_us}))
                .collect::<Vec<_>>()),
        );
        updated_run.get_mut("barrier").unwrap()["trackedProcesses"] = json!(tracked);
        updated_run.get_mut("barrier").unwrap()["capturedAt"] = json!(now());
        if !confirmed {
            // Same existing manifest-first recovery order. A DB failure keeps the
            // workspace blocked; startup recovery reconciles the preparing record.
            projects.workspace_mark_recovery_required(storage, &address)?;
        }
        storage.upsert_projection_records_atomic(vec![
            ProjectionUpsert {
                aggregate_type: "run",
                aggregate_id: self.run_id.clone(),
                operation_id: format!("op_agent_native_terminal_{}", self.run_id),
                request_id: None,
                payload: updated_run,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: self.request.chat_id.clone(),
                operation_id: format!("op_agent_native_chat_terminal_{}", self.run_id),
                request_id: None,
                payload: chat_projection_with_record(&chat_projection, &updated_chat, None)?,
            },
        ])?;
        Ok(())
    }
}
