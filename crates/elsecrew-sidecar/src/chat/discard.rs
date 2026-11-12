//! Normal review discard. Transport admission must also exclude deferred/native work.
use super::*;

impl G5Manager {
    pub(crate) fn preview_workspace_discard(
        &self,
        storage: &Storage,
        projects: &ProjectManager,
        address: &WorkspaceAddress,
        task_id: &str,
    ) -> Result<Value, ProjectFailure> {
        let committed = chat_projection_record(storage, &address.chat_id)?;
        if committed["state"] == "discarded" && committed["record"]["state"] == "discarded" {
            let revision = committed
                .get("discard_receipt")
                .and_then(|receipt| receipt["chatRevision"].as_u64())
                .filter(|value| *value > 0 && *value <= 9_007_199_254_740_991)
                .ok_or_else(|| {
                    ProjectFailure::recovery("discarded chat has no normal discard receipt")
                })?;
            let receipt = json!({"chatId":address.chat_id,"taskId":task_id,"projectId":address.project_id,
                "worktreeId":address.worktree_id,"workspaceIdentity":address.workspace_identity,"chatRevision":revision});
            validate_g5_payload_v1("chatRecord", &committed["record"])
                .map_err(|error| ProjectFailure::manifest(error.to_string()))?;
            if committed.get("discard_receipt") != Some(&receipt) {
                return Err(ProjectFailure::manifest(
                    "discard inspection receipt identity differs",
                ));
            }
            for key in [
                "chatId",
                "taskId",
                "projectId",
                "worktreeId",
                "workspaceIdentity",
            ] {
                if committed["record"][key] != receipt[key] {
                    return Err(ProjectFailure::manifest(
                        "discard inspection chat identity differs",
                    ));
                }
            }
            return Ok(receipt);
        }
        let (stored, _) = self.validate_workspace_discard(storage, projects, address, task_id)?;
        Ok(
            json!({"chatId":address.chat_id,"taskId":task_id,"projectId":address.project_id,
            "worktreeId":address.worktree_id,"workspaceIdentity":address.workspace_identity,
            "chatRevision":stored["last_event_sequence"]}),
        )
    }

    pub(crate) fn discard_workspace(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        address: &WorkspaceAddress,
        task_id: &str,
        expected_revision: u64,
        request_id: &str,
    ) -> Result<Value, ProjectFailure> {
        let receipt = json!({"chatId":address.chat_id,"taskId":task_id,
            "projectId":address.project_id,"worktreeId":address.worktree_id,
            "workspaceIdentity":address.workspace_identity,"chatRevision":expected_revision});
        let committed = chat_projection_record(storage, &address.chat_id)?;
        if committed["state"] == "discarded"
            && committed["record"]["state"] == "discarded"
            && committed.get("discard_receipt") == Some(&receipt)
        {
            // A lost response must not require another state transition or write.
            validate_g5_payload_v1("chatRecord", &committed["record"])
                .map_err(|error| ProjectFailure::manifest(error.to_string()))?;
            for key in [
                "chatId",
                "taskId",
                "projectId",
                "worktreeId",
                "workspaceIdentity",
            ] {
                if committed["record"][key] != receipt[key] {
                    return Err(ProjectFailure::manifest("discard receipt identity differs"));
                }
            }
            return Ok(committed["record"].clone());
        }
        let (mut stored, manifest) =
            self.validate_workspace_discard(storage, projects, address, task_id)?;
        if stored["last_event_sequence"].as_u64() != Some(expected_revision) {
            return Err(ProjectFailure::dirty(
                "chat changed before workspace discard",
            ));
        }
        let chat = update_chat_state(&stored["record"], "discarded")?;
        let sequence = stored["durable_event_sequence"]
            .as_u64()
            .and_then(|value| value.checked_add(1))
            .filter(|value| *value <= 9_007_199_254_740_991)
            .ok_or_else(|| ProjectFailure::manifest("chat message sequence is invalid"))?;
        let message = team_message(
            &chat,
            sequence,
            "system",
            None,
            None,
            "Workspace discarded. Original files and conversation history were preserved.",
        )?;
        stored.insert("discard_receipt".into(), receipt);
        stored.insert("record".into(), chat.clone());
        stored.insert("state".into(), json!("discarded"));
        stored.insert("last_message".into(), message);
        stored.insert("durable_event_sequence".into(), json!(sequence));
        // Validate everything before the first mutation. A DB failure after this
        // point leaves an unavailable manifest; the same revision can retry.
        projects.terminalize_discard_workspace(address, &manifest)?;
        storage.upsert_projection_records_atomic_checked(
            vec![ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: address.chat_id.clone(),
                operation_id: format!("op_chat_discard_{}_{expected_revision}", address.chat_id),
                request_id: Some(request_id.to_owned()),
                payload: stored,
            }],
            &[("chat", &address.chat_id, expected_revision)],
        )?;
        projects.finish_workspace_discard(address);
        Ok(chat)
    }

    fn validate_workspace_discard(
        &self,
        storage: &Storage,
        projects: &ProjectManager,
        address: &WorkspaceAddress,
        task_id: &str,
    ) -> Result<(Map<String, Value>, Value), ProjectFailure> {
        if self.preparing_to_quit
            || self
                .active
                .values()
                .any(|run| run.chat_id == address.chat_id)
        {
            return Err(ProjectFailure::recovery(
                "workspace discard is blocked by an active fixture or shutdown",
            ));
        }
        let stored = chat_projection_record(storage, &address.chat_id)?;
        let record = &stored["record"];
        validate_g5_payload_v1("chatRecord", record)
            .map_err(|error| ProjectFailure::manifest(error.to_string()))?;
        for (field, expected) in [
            ("chatId", address.chat_id.as_str()),
            ("taskId", task_id),
            ("projectId", address.project_id.as_str()),
            ("worktreeId", address.worktree_id.as_str()),
            ("workspaceIdentity", address.workspace_identity.as_str()),
        ] {
            if record[field].as_str() != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "workspace discard identity differs from the chat",
                ));
            }
        }
        if stored["state"] != "reviewReady"
            || record["state"] != "reviewReady"
            || stored["last_event_sequence"].as_u64().is_none()
        {
            return Err(ProjectFailure::recovery(
                "only a review-ready chat can discard its workspace",
            ));
        }
        for run in storage.projection_records("run")? {
            if run.get("chat_id").and_then(Value::as_str) != Some(address.chat_id.as_str())
                && run["record"]["chatId"].as_str() != Some(address.chat_id.as_str())
            {
                continue;
            }
            let record = &run["record"];
            let evidence = if run.get("source_kind").and_then(Value::as_str) == Some("actualAgent")
            {
                run.get("termination").unwrap_or(&Value::Null)
            } else {
                &record["terminationEvidence"]
            };
            if run["chat_id"].as_str() != Some(address.chat_id.as_str())
                || record["chatId"].as_str() != Some(address.chat_id.as_str())
                || record["worktreeId"].as_str() != Some(address.worktree_id.as_str())
                || run["workspace_manifest"]["projectId"].as_str()
                    != Some(address.project_id.as_str())
                || record["taskId"].as_str() != Some(task_id)
                || record["workspaceIdentity"].as_str() != Some(address.workspace_identity.as_str())
                || run["state"] != record["state"]
                || !matches!(
                    record["state"].as_str(),
                    Some("succeeded" | "failed" | "canceled")
                )
                || evidence["processesProvenGone"] != true
                || run
                    .get("recovery")
                    .is_some_and(|recovery| !recovery.is_null())
            {
                return Err(ProjectFailure::recovery(
                    "workspace discard requires terminal run evidence",
                ));
            }
        }
        let manifest = projects.verify_discard_workspace(storage, address)?;
        Ok((stored, manifest))
    }
}
