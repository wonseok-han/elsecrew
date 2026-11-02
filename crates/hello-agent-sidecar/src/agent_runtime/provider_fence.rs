//! Read-only completion evidence for explicit provider-fence recovery.
use super::*;
use crate::provider_run_binding::RunFenceBinding;

pub(crate) fn verify_resolved_provider_fence(
    storage: &Storage,
    projects: &ProjectManager,
    binding: &RunFenceBinding,
    recorded: &BTreeSet<ProcIdentity>,
) -> Result<(), SidecarError> {
    verify(storage, projects, binding, recorded).map_err(|_| {
        SidecarError::new(
            "providerLoginRecoveryRequired",
            "bound run completion could not be verified",
        )
    })
}

fn verify(
    storage: &Storage,
    projects: &ProjectManager,
    binding: &RunFenceBinding,
    recorded: &BTreeSet<ProcIdentity>,
) -> Result<(), ProjectFailure> {
    let failure = || ProjectFailure::recovery("provider fence completion differs");
    let projection = actual_run_projection(storage, binding.run_id())?;
    if projection.get("provider_run_fence") != Some(&binding.to_value())
        || projection.contains_key(RECOVERY_INTENT_FIELD)
    {
        return Err(failure());
    }
    binding
        .validate(
            binding.run_id(),
            projection["record"]["selectedProfile"]["runtime"]
                .as_str()
                .ok_or_else(failure)?,
        )
        .map_err(|_| failure())?;
    verify_completion(storage, projects, binding.run_id(), &projection, recorded)
}

// Operational repair only: not compiled into the app or exposed through IPC.
#[cfg(test)]
pub(crate) fn verify_approved_legacy_fence(
    storage: &Storage,
    projects: &ProjectManager,
    run_id: &str,
    recorded: &BTreeSet<ProcIdentity>,
) -> Result<(), SidecarError> {
    let check = || -> Result<(), ProjectFailure> {
        let failure = || ProjectFailure::recovery("legacy repair evidence differs");
        let projection = actual_run_projection(storage, run_id)?;
        if projection.contains_key("provider_run_fence")
            || projection.contains_key(RECOVERY_INTENT_FIELD)
            || projection["record"]["selectedProfile"]["runtime"] != "codex"
        {
            return Err(failure());
        }
        for other in storage.projection_records("run")? {
            if other["record"]["runId"] == run_id {
                continue;
            }
            if other["source_kind"] != "actualAgent" {
                continue;
            }
            if other["record"]["selectedProfile"]["runtime"] == "codex"
                && !matches!(
                    other["record"]["state"].as_str(),
                    Some("succeeded" | "failed" | "canceled")
                )
            {
                return Err(failure());
            }
            if let Some(identities) = other
                .get("process_identity_internal")
                .and_then(Value::as_array)
            {
                for identity in identities {
                    if recorded
                        .iter()
                        .any(|p| identity["pid"] == p.pid && identity["start_us"] == p.start_us)
                    {
                        return Err(failure());
                    }
                }
            }
        }
        verify_completion(storage, projects, run_id, &projection, recorded)
    };
    check().map_err(|_| {
        SidecarError::new(
            "providerLoginRecoveryRequired",
            "legacy repair completion could not be verified",
        )
    })
}

fn verify_completion(
    storage: &Storage,
    projects: &ProjectManager,
    run_id: &str,
    projection: &Map<String, Value>,
    recorded: &BTreeSet<ProcIdentity>,
) -> Result<(), ProjectFailure> {
    let failure = || ProjectFailure::recovery("provider fence completion differs");
    let run = &projection["record"];
    let recovery = &projection["recovery"];
    let termination = &projection["termination"];
    for (kind, value) in [
        ("runRecord", run),
        ("recoveryRecord", recovery),
        ("terminationTranscript", termination),
    ] {
        validate_agent_adapter_payload_v1(kind, value).map_err(|_| failure())?;
    }
    let writer_state = match run["selectedProfile"]["permission"].as_str() {
        Some("workspaceWrite") => "released",
        Some("readOnly") => "none",
        _ => return Err(failure()),
    };
    if run["runId"] != run_id
        || run["state"] != "canceled"
        || run["terminationOutcome"] != "cleanExit"
        || recovery["runId"] != run["runId"]
        || recovery["state"] != "resolvedAbandoned"
        || recovery["poisoned"] != false
        || termination["runId"] != run["runId"]
        || termination["outcome"] != "cleanExit"
        || termination["poisoned"] != false
        || termination["processesProvenGone"] != true
        || termination["writerLockState"] != writer_state
        || termination["chatState"] != "canceled"
        || termination["trackedProcesses"] != recovery["trackedProcesses"]
    {
        return Err(failure());
    }
    let internal = projection["process_identity_internal"]
        .as_array()
        .ok_or_else(failure)?;
    let public = recovery["trackedProcesses"]
        .as_array()
        .ok_or_else(failure)?;
    if internal.is_empty() || internal.len() != recorded.len() || public.len() != internal.len() {
        return Err(failure());
    }
    let mut expected = BTreeSet::new();
    for (private, public) in internal.iter().zip(public) {
        crate::require_exact_params(private, &["pid", "start_us"]).map_err(|_| failure())?;
        let identity = ProcIdentity {
            pid: u32::try_from(private["pid"].as_u64().ok_or_else(failure)?)
                .map_err(|_| failure())?,
            start_us: private["start_us"].as_i64().ok_or_else(failure)?,
        };
        if !expected.insert(identity)
            || public["pid"] != identity.pid
            || public["startIdentityDigest"] != digest_text(&identity.start_us.to_string())
            || public["observedGone"] != true
        {
            return Err(failure());
        }
    }
    if &expected != recorded {
        return Err(failure());
    }
    let address = address_from_chat(run)?;
    let chat = actual_chat_projection(storage, &address.chat_id)?;
    for field in [
        "chatId",
        "taskId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        if run[field].is_null()
            || run[field] != recovery[field]
            || run[field] != chat["record"][field]
        {
            return Err(failure());
        }
    }
    if chat["record"]["state"] != "discarded" {
        return Err(failure());
    }
    let lock = storage
        .projection_record("lock", &format!("lock_{}", address.worktree_id))?
        .ok_or_else(failure)?;
    if lock.get("poisoned") != Some(&Value::Bool(false))
        || lock.get("lock_id").and_then(Value::as_str)
            != Some(format!("lock_{}", address.worktree_id).as_str())
        || lock.get("project_id").and_then(Value::as_str) != Some(address.project_id.as_str())
        || lock.get("recovery_required") != Some(&Value::Bool(false))
        || lock.get("writer_run_id").is_none_or(|id| !id.is_null())
    {
        return Err(failure());
    }
    let mut manifest = projection["workspace_manifest"].clone();
    manifest["lifecycle"] = json!("recoveryRequired");
    projects.workspace_verify_recovery_abandoned(
        &address,
        &manifest,
        &recovery_manifest_digest(&manifest)?,
    )
}
