//! Read-only completion evidence for explicit provider-fence recovery.
use super::*;
use crate::provider_login_process::RunFenceBinding;

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
    let runtime = run["selectedProfile"]["runtime"]
        .as_str()
        .ok_or_else(failure)?;
    let writer_state = match run["selectedProfile"]["permission"].as_str() {
        Some("workspaceWrite") => "released",
        Some("readOnly") => "none",
        _ => return Err(failure()),
    };
    binding
        .validate(binding.run_id(), runtime)
        .map_err(|_| failure())?;
    if run["runId"] != binding.run_id()
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
