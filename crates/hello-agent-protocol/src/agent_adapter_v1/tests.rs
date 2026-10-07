use super::*;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[test]
fn native_model_choices_are_advertised_without_expanding_fixture_contracts() {
    assert!(G5D_NATIVE_CAPABILITIES_V1.contains(&"agent.getModelChoices"));
    assert!(!G5A_CAPABILITIES_V1.contains(&"agent.getModelChoices"));
    assert!(!G5D_AGENT_FIXTURE_CAPABILITIES_V1.contains(&"agent.getModelChoices"));
    validate_g5d_native_ready_payload_v1(&ready(G5D_NATIVE_CAPABILITIES_V1)).unwrap();
    let old: Vec<_> = G5D_NATIVE_CAPABILITIES_V1.iter().copied()
        .filter(|method| *method != "agent.getModelChoices").collect();
    assert!(validate_g5d_native_ready_payload_v1(&ready(&old)).is_err());
}

#[test]
fn unobserved_installation_is_not_missing_or_startable() {
    let (root, _) = manifest();
    let value: Value = read_json(root.join("unobserved-capability.json"));
    validate_capability_snapshot(&value).unwrap();
    let mut with_executable = value.clone();
    with_executable["executable"] = serde_json::json!({"pathDigest":"sha256:path","fileIdentityDigest":"sha256:file","cliVersion":"0.158.0"});
    let snapshot: AgentCapabilitySnapshotV1 =
        serde_json::from_value(with_executable.clone()).unwrap();
    with_executable["fingerprint"] = compute_agent_capability_fingerprint_v1(&snapshot)
        .unwrap()
        .into();
    assert!(validate_capability_snapshot(&with_executable)
        .unwrap_err()
        .to_string()
        .contains("installed state and executable identity must agree"));
    for (key, replacement) in [
        ("canStart", serde_json::json!(true)),
        ("blockedReason", serde_json::json!("notInstalled")),
        ("authState", serde_json::json!("authenticated")),
        (
            "models",
            serde_json::json!([{"modelSelector":"some-model","reasoningEfforts":[],"permissions":["readOnly"]}]),
        ),
    ] {
        let mut changed = value.clone();
        changed[key] = replacement;
        let snapshot: AgentCapabilitySnapshotV1 = serde_json::from_value(changed.clone()).unwrap();
        changed["fingerprint"] = compute_agent_capability_fingerprint_v1(&snapshot)
            .unwrap()
            .into();
        assert!(validate_capability_snapshot(&changed).is_err());
    }
    let mut windows = value;
    windows["platform"] = "win32".into();
    windows["blockedReason"] = "platformUnsupported".into();
    let snapshot: AgentCapabilitySnapshotV1 = serde_json::from_value(windows.clone()).unwrap();
    windows["fingerprint"] = compute_agent_capability_fingerprint_v1(&snapshot)
        .unwrap()
        .into();
    assert!(!validate_capability_snapshot(&windows).unwrap().can_start);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u64,
    protocol_version: u32,
    contract: Contract,
    cases_file: String,
    g5b_cases_file: String,
    g5c_cases_file: String,
    g5c_recovery_discovery_cases_file: String,
    g5c_identityless_recovery_cases_file: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Contract {
    capabilities: Vec<String>,
    bridge_methods_added: Vec<String>,
    actual_agent_advertised_by_current_g5: bool,
    agent_smoke_advertised: bool,
    fixture_run_semantics: String,
    actual_run_persistence: String,
    execution_mode: String,
    follow_up_mode: String,
    renderer_launch_fields: String,
    fingerprint: FingerprintContract,
    actual_run_id_namespace: String,
    actual_recovery_id_namespace: String,
    actual_event_id_namespace: String,
    model_start_evidence: Vec<String>,
    start_outcomes: Vec<String>,
    capability_block_reasons: Vec<String>,
    argv_only_can_start: bool,
    public_events: Vec<String>,
    public_event_protocol_event: String,
    public_event_envelope_binding_fields: Vec<String>,
    accepted_binding_fields: Vec<String>,
    public_event_binding_fields: Vec<String>,
    chat_detail_capability_discriminator: String,
    preflight_failure: PreflightFailure,
    launcher_barrier: Vec<String>,
    uncertain_termination: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreflightFailure {
    spawn_count: u64,
    lock_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FingerprintContract {
    algorithm: String,
    encoding: String,
    object_keys: String,
    array_order: String,
    explicit_null: String,
    excluded_fields: Vec<String>,
    canonical_fixture: String,
    canonical_digest: String,
}

#[derive(Deserialize)]
struct Cases {
    valid: Vec<FixtureCase>,
    invalid: Vec<FixtureCase>,
    bindings: Vec<BindingCase>,
}

#[derive(Deserialize)]
struct FixtureCase {
    name: String,
    validator: String,
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindingCase {
    name: String,
    kind: String,
    valid: bool,
    expected: Value,
    value: Value,
}

#[derive(Deserialize)]
struct G5BCases {
    accepted: Value,
    valid: Vec<StateEventCase>,
    invalid: Vec<MutationCase>,
    bindings: Vec<MutationCase>,
}

#[derive(Deserialize)]
struct StateEventCase {
    name: String,
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MutationCase {
    name: String,
    base_valid_name: String,
    pointer: Option<String>,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    changes: Vec<MutationChange>,
    valid: Option<bool>,
}

#[derive(Deserialize)]
struct MutationChange {
    pointer: String,
    value: Value,
}

#[derive(Deserialize)]
struct LedgerAmplificationGolden {
    events: Vec<Value>,
    snapshots: Vec<LedgerSnapshot>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LedgerSnapshot {
    event_count: usize,
    snapshot_bytes: usize,
    cumulative_snapshot_bytes: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct G5CCases {
    claude_manifest: Value,
    claude_observation: Value,
    accepted_run: Value,
    resolved_profile: Value,
    claude_attestation: Value,
    claude_events: Value,
    codex_manifest: Value,
    codex_argv_only_observation: Value,
    codex_unknown_catalog_observation: Value,
    invalid_mutations: Vec<G5CMutationCase>,
    generated_event_cases: Vec<G5CGeneratedEventCase>,
}

#[derive(Deserialize)]
struct RecoveryDiscoveryCases {
    valid: Vec<StateEventCase>,
    invalid: Vec<RecoveryDiscoveryMutationCase>,
}

#[derive(Deserialize)]
struct IdentitylessRecoveryCases {
    valid: Vec<FixtureCase>,
    invalid: Vec<RecoveryDiscoveryMutationCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryDiscoveryMutationCase {
    name: String,
    base_valid_name: String,
    removals: Vec<String>,
    changes: Vec<MutationChange>,
}

#[derive(Deserialize)]
struct G5CMutationCase {
    name: String,
    target: String,
    changes: Vec<MutationChange>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct G5CGeneratedEventCase {
    name: String,
    valid: bool,
    generator: String,
    public_event_count: Option<usize>,
    public_text_bytes_each: Option<usize>,
    public_text_byte_lengths: Option<Vec<usize>>,
    expected_public_event_count: usize,
    expected_public_text_bytes: usize,
}

#[test]
fn rust_matches_the_planned_g5a_manifest_without_changing_g5_ready() {
    let (root, manifest) = manifest();
    assert_eq!(manifest.schema_version, G5A_SCHEMA_VERSION_V1);
    assert_eq!(manifest.protocol_version, crate::PROTOCOL_VERSION);
    assert!(G5A_CAPABILITIES_V1.iter().copied().eq(manifest
        .contract
        .capabilities
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.contract.bridge_methods_added,
        [
            "getAgentCapabilities",
            "startAgentRun",
            "cancelAgentRun",
            "inspectAgentRecovery",
            "resolveAgentRecovery",
            "onAgentPublicEvent"
        ]
    );
    assert!(!manifest.contract.actual_agent_advertised_by_current_g5);
    assert!(!manifest.contract.agent_smoke_advertised);
    assert_eq!(manifest.contract.fixture_run_semantics, "fakeOnly");
    assert_eq!(
        manifest.contract.actual_run_persistence,
        "separateAgentRunsLedger"
    );
    assert_g5a_contract_v1().expect("G5A contract");

    let current_ready = ready(G5_CAPABILITIES_V1);
    crate::run_recovery_v1::validate_g5_ready_payload_v1(&current_ready)
        .expect("current G5 remains valid");
    let planned_ready = ready(G5A_CAPABILITIES_V1);
    assert!(crate::run_recovery_v1::validate_g5_ready_payload_v1(&planned_ready).is_err());
    validate_planned_g5a_ready_payload_v1(&planned_ready).expect("planned G5A ready");
    assert!(root.join(manifest.cases_file).is_file());
}

#[test]
fn rust_accepts_and_rejects_the_same_agent_adapter_golden_cases() {
    let (root, manifest) = manifest();
    let cases: Cases = read_json(root.join(manifest.cases_file));
    for fixture in cases.valid {
        validate_agent_adapter_payload_v1(&fixture.validator, &fixture.value)
            .unwrap_or_else(|error| panic!("valid fixture {} failed: {error}", fixture.name));
    }
    for fixture in cases.invalid {
        assert!(
            validate_agent_adapter_payload_v1(&fixture.validator, &fixture.value).is_err(),
            "invalid fixture {} passed",
            fixture.name
        );
    }
    for fixture in cases.bindings {
        let result = match fixture.kind.as_str() {
            "accepted" => {
                let expected: AgentRunContextV1 =
                    serde_json::from_value(fixture.expected).expect("accepted expected context");
                validate_agent_run_accepted_against_context_v1(&fixture.value, &expected)
                    .map(|_| ())
            }
            "publicEvent" => {
                let expected: AgentRunBindingV1 = serde_json::from_value(fixture.expected)
                    .expect("public event expected binding");
                validate_agent_public_event_against_run_v1(&fixture.value, &expected).map(|_| ())
            }
            other => panic!("unknown binding kind {other}"),
        };
        assert_eq!(
            result.is_ok(),
            fixture.valid,
            "binding fixture {}",
            fixture.name
        );
    }
}

#[test]
fn rust_discovers_one_unresolved_actual_recovery_without_legacy_leakage() {
    let (root, manifest) = manifest();
    let cases: RecoveryDiscoveryCases =
        read_json(root.join(&manifest.g5c_recovery_discovery_cases_file));
    for fixture in &cases.valid {
        validate_agent_adapter_payload_v1("agentChatDetailPage", &fixture.value)
            .unwrap_or_else(|error| panic!("valid fixture {} failed: {error}", fixture.name));
    }
    for fixture in &cases.invalid {
        assert!(
            validate_agent_adapter_payload_v1(
                "agentChatDetailPage",
                &materialize_recovery_discovery(&cases, fixture),
            )
            .is_err(),
            "invalid fixture {} passed",
            fixture.name
        );
    }
    let resolved_case = cases
        .invalid
        .iter()
        .find(|entry| entry.name == "fully-valid-resolved-recovery-cannot-remain-on-agent-chat")
        .expect("resolved recovery detail fixture");
    let resolved_detail = materialize_recovery_discovery(&cases, resolved_case);
    validate_agent_recovery_record(&resolved_detail["agentRecovery"])
        .expect("resolved recovery is internally valid outside chat detail");
    assert!(validate_agent_chat_detail_page(&resolved_detail).is_err());

    let recovery = &cases
        .valid
        .iter()
        .find(|entry| entry.name == "agent-chat-recovery-required-with-discoverable-recovery")
        .expect("recovery detail fixture")
        .value["agentRecovery"];
    assert!(crate::run_recovery_v1::validate_g5_payload_v1(
        "recoveryPage",
        &serde_json::json!({"items": [recovery], "nextCursor": null}),
    )
    .is_err());

    let base_cases: Cases = read_json(root.join(&manifest.cases_file));
    let mut durable = base_cases
        .valid
        .iter()
        .find(|entry| entry.name == "actual-agent-recovery-resolves-only-after-process-proof")
        .expect("durable recovery result fixture")
        .value
        .clone();
    durable["request"]["recoveryId"] = Value::String("agent_recovery_random_substitute".into());
    assert!(validate_agent_adapter_payload_v1("recoveryResolveResult", &durable).is_err());
}

#[test]
fn rust_matches_identityless_recovery_and_resolution_priority_goldens() {
    let (root, manifest) = manifest();
    let cases: IdentitylessRecoveryCases =
        read_json(root.join(&manifest.g5c_identityless_recovery_cases_file));
    for fixture in &cases.valid {
        validate_agent_adapter_payload_v1(&fixture.validator, &fixture.value)
            .unwrap_or_else(|error| panic!("valid fixture {} failed: {error}", fixture.name));
    }
    for fixture in &cases.invalid {
        let base = cases
            .valid
            .iter()
            .find(|entry| entry.name == fixture.base_valid_name)
            .unwrap_or_else(|| panic!("missing base fixture {}", fixture.base_valid_name));
        assert!(
            validate_agent_adapter_payload_v1(
                &base.validator,
                &materialize_identityless_recovery(&cases, fixture),
            )
            .is_err(),
            "invalid fixture {} passed",
            fixture.name
        );
    }

    let raw: Value = read_json(root.join("manifest.json"));
    assert_eq!(
        raw["contract"]["identitylessRecovery"],
        serde_json::json!({
            "reason": "processIdentityUnavailable",
            "trackedProcesses": "mustBeEmpty",
            "state": "unresolved",
            "poisoned": true,
            "resolvedAt": null,
            "matchingRevisionOutcome": "identityUnavailable",
            "staleRevisionOutcome": "revisionConflict",
            "after": null,
            "resolutionPriority": [
                "revisionConflict",
                "identityUnavailable",
                "processesRemain",
                "resolvedAbandoned"
            ]
        })
    );

    let stale = cases
        .valid
        .iter()
        .find(|entry| entry.name == "identityless-stale-revision-keeps-conflict-priority")
        .expect("stale identityless fixture");
    assert_eq!(stale.value["outcome"], "revisionConflict");
    assert!(stale.value["after"].is_null());
    let matching = cases
        .valid
        .iter()
        .find(|entry| entry.name == "identityless-matching-revision-is-non-mutating")
        .expect("matching identityless fixture");
    assert_eq!(matching.value["outcome"], "identityUnavailable");
    assert!(matching.value["after"].is_null());
}

#[test]
fn rust_rejects_oversize_public_event_text() {
    let (root, manifest) = manifest();
    let cases: Cases = read_json(root.join(manifest.cases_file));
    let mut event = cases
        .valid
        .into_iter()
        .find(|entry| entry.name == "bounded-public-status-event")
        .expect("bounded public event")
        .value;
    event["text"] = Value::String("a".repeat(G5_PUBLIC_MESSAGE_MAX_BYTES_V1 + 1));
    assert!(validate_agent_public_event_v1(&event).is_err());
}

#[test]
fn rust_manifest_freezes_one_shot_redacted_fail_closed_semantics() {
    let (root, manifest) = manifest();
    assert_eq!(manifest.contract.execution_mode, "oneShot");
    assert_eq!(manifest.contract.follow_up_mode, "nextRun");
    assert_eq!(manifest.contract.renderer_launch_fields, "forbidden");
    let fingerprint = &manifest.contract.fingerprint;
    assert_eq!(fingerprint.algorithm, "sha256");
    assert_eq!(fingerprint.encoding, "utf8CompactJson");
    assert_eq!(fingerprint.object_keys, "recursiveAsciiAscending");
    assert_eq!(fingerprint.array_order, "preserved");
    assert_eq!(fingerprint.explicit_null, "included");
    assert_eq!(fingerprint.excluded_fields, ["fingerprint", "observedAt"]);
    assert_eq!(manifest.contract.actual_run_id_namespace, "agent_run_");
    assert_eq!(
        manifest.contract.actual_recovery_id_namespace,
        "agent_recovery_"
    );
    assert_eq!(manifest.contract.actual_event_id_namespace, "agent_event_");
    assert_eq!(
        manifest.contract.model_start_evidence,
        ["runtimeVerified", "measuredExactVersion"]
    );
    assert!(AGENT_START_OUTCOMES_V1.iter().copied().eq(manifest
        .contract
        .start_outcomes
        .iter()
        .map(String::as_str)));
    assert!(AGENT_CAPABILITY_BLOCK_REASONS_V1
        .iter()
        .copied()
        .eq(manifest
            .contract
            .capability_block_reasons
            .iter()
            .map(String::as_str)));
    assert!(!manifest.contract.argv_only_can_start);
    assert_eq!(
        manifest.contract.public_events,
        ["status", "question", "final"]
    );
    assert_eq!(
        manifest.contract.public_event_protocol_event,
        "agent.publicEvent"
    );
    assert!(crate::EVENTS_V1.contains(&"agent.publicEvent"));
    assert_eq!(
        manifest.contract.public_event_envelope_binding_fields,
        ["chatId", "taskId", "runId"]
    );
    assert_eq!(
        manifest.contract.accepted_binding_fields,
        [
            "chatId",
            "taskId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "roleId",
            "capabilityFingerprint"
        ]
    );
    assert_eq!(
        manifest.contract.public_event_binding_fields,
        [
            "chatId",
            "taskId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "runId",
            "roleId",
            "capabilityFingerprint"
        ]
    );
    assert_eq!(
        manifest.contract.chat_detail_capability_discriminator,
        "agentRunsPresence"
    );
    assert_eq!(manifest.contract.preflight_failure.spawn_count, 0);
    assert_eq!(manifest.contract.preflight_failure.lock_count, 0);
    assert_eq!(
        manifest.contract.launcher_barrier,
        [
            "persistRunAndLock",
            "captureLauncherAndAgentIdentity",
            "attachTreeTracker",
            "persistIdentity",
            "release"
        ]
    );
    assert_eq!(
        manifest.contract.uncertain_termination,
        "recoveryRequiredAndPoisoned"
    );

    let cases: Cases = read_json(root.join("cases.json"));
    let fixture = cases
        .valid
        .iter()
        .find(|entry| entry.name == fingerprint.canonical_fixture)
        .expect("canonical capability fixture");
    let snapshot: AgentCapabilitySnapshotV1 =
        serde_json::from_value(fixture.value.clone()).expect("decode capability snapshot");
    assert_eq!(
        compute_agent_capability_fingerprint_v1(&snapshot).expect("fingerprint"),
        fingerprint.canonical_digest
    );
}

#[test]
fn rust_accepts_every_wire_state_and_rejects_g5b_fail_closed_cases() {
    let (root, manifest) = manifest();
    let cases: G5BCases = read_json(root.join(&manifest.g5b_cases_file));
    let accepted: AgentRunAcceptedV1 =
        serde_json::from_value(cases.accepted.clone()).expect("decode accepted run");
    let states: Vec<_> = cases
        .valid
        .iter()
        .map(|entry| {
            validate_agent_run_state_changed_against_accepted_v1(&entry.value, &accepted)
                .unwrap_or_else(|error| panic!("valid fixture {} failed: {error}", entry.name))
                .run
                .state
        })
        .collect();
    assert_eq!(states, AGENT_RUN_STATE_EVENT_STATES_V1);
    for entry in &cases.invalid {
        assert!(
            validate_agent_run_state_changed_v1(&materialize_g5b(&cases, entry)).is_err(),
            "invalid fixture passed: {}",
            entry.name
        );
    }
    for entry in &cases.bindings {
        let result = validate_agent_run_state_changed_against_accepted_v1(
            &materialize_g5b(&cases, entry),
            &accepted,
        );
        assert_eq!(result.is_ok(), entry.valid == Some(true), "{}", entry.name);
    }
}

#[test]
fn rust_matches_g5b_resource_limits_and_cumulative_public_ledger_snapshots() {
    let (root, _) = manifest();
    let raw: Value = read_json(root.join("manifest.json"));
    let contract = &raw["contract"];
    assert_eq!(contract["agentStateEventIdNamespace"], "agent_state_event_");
    assert_eq!(
        contract["runStatePayloadFields"],
        serde_json::json!(["schemaVersion", "eventId", "run"])
    );
    assert_eq!(
        contract["runStateWireStates"],
        serde_json::json!(AGENT_RUN_STATE_EVENT_STATES_V1)
    );
    assert_eq!(
        contract["runStateEnvelopeBindingFields"],
        serde_json::json!(["chatId", "taskId", "runId"])
    );
    assert_eq!(
        contract["runStateAcceptedBindingFields"],
        serde_json::json!([
            "chatId",
            "taskId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "runId",
            "roleId",
            "capabilityFingerprint"
        ])
    );
    assert_eq!(
        contract["rendererRunStateSemantics"],
        "hydrationTriggerOnly"
    );
    let limits = &raw["contract"]["resourceLimits"];
    assert_eq!(
        limits["publicEventsPerRun"],
        crate::AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1
    );
    assert_eq!(
        limits["publicTextBytesPerRun"],
        crate::AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1
    );
    assert_eq!(
        limits["immutablePublicLedgerCumulativeSnapshotBytesPerRun"],
        crate::AGENT_PUBLIC_LEDGER_CUMULATIVE_SNAPSHOT_BYTES_PER_RUN_MAX_V1
    );
    assert_eq!(
        limits["activeActualRunsPerSession"],
        crate::AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1
    );
    assert_eq!(
        limits["seenActualRunIdsPerSession"],
        crate::AGENT_SEEN_RUN_IDS_PER_SESSION_MAX_V1
    );
    assert_eq!(
        limits["seenActualEventIdsPerSession"],
        crate::AGENT_SEEN_EVENT_IDS_PER_SESSION_MAX_V1
    );
    assert_eq!(
        limits["actualStreamIdsPerSession"],
        crate::AGENT_STREAM_IDS_PER_SESSION_MAX_V1
    );
    assert_eq!(
        limits["protocolStreamIdsPerSession"],
        crate::PROTOCOL_STREAM_IDS_PER_SESSION_MAX_V1
    );
    assert_eq!(
        limits["immutablePublicLedgerCumulativeSnapshotBytesDefinition"],
        "sumAfterEachAppendUtf8BytesOfRecursiveAsciiKeySortedCompactJsonFullPublicEventsArraySnapshotsPerRun;arrayOrderPreserved;explicitNullIncluded"
    );
    let golden_file = limits["publicLedgerAmplificationGoldenFile"]
        .as_str()
        .expect("ledger golden file");
    let golden: LedgerAmplificationGolden = read_json(root.join(golden_file));
    for snapshot in golden.snapshots {
        let events = &golden.events[..snapshot.event_count];
        assert_eq!(
            agent_public_events_snapshot_bytes_v1(events).expect("snapshot bytes"),
            snapshot.snapshot_bytes
        );
        assert_eq!(
            agent_public_ledger_cumulative_snapshot_bytes_v1(events)
                .expect("cumulative snapshot bytes"),
            snapshot.cumulative_snapshot_bytes
        );
    }
}

#[test]
fn rust_matches_g5c_provider_discovery_attestation_and_event_goldens() {
    let (root, adapter_manifest) = manifest();
    let cases: G5CCases = read_json(root.join(&adapter_manifest.g5c_cases_file));

    let claude = validate_provider_manifest_v1(&cases.claude_manifest)
        .expect("valid Claude provider manifest");
    let codex = validate_provider_manifest_v1(&cases.codex_manifest)
        .expect("valid Codex provider manifest");
    assert_eq!(
        compute_provider_executable_closure_digest_v1(&claude.executable_closure)
            .expect("Claude closure digest"),
        claude.executable_closure.closure_digest
    );
    assert_eq!(
        compute_provider_manifest_digest_v1(&claude).expect("Claude manifest digest"),
        claude.manifest_digest
    );
    assert_eq!(
        compute_provider_executable_closure_digest_v1(&codex.executable_closure)
            .expect("Codex closure digest"),
        codex.executable_closure.closure_digest
    );
    assert_eq!(
        compute_provider_manifest_digest_v1(&codex).expect("Codex manifest digest"),
        codex.manifest_digest
    );
    assert_eq!(
        claude.launch_policy.required_runtime_verification,
        G5C_CLAUDE_RUNTIME_VERIFICATION_V1
    );
    assert_eq!(
        codex.launch_policy.required_runtime_verification,
        G5C_CODEX_RUNTIME_VERIFICATION_V1
    );

    let claude_observation = validate_sanitized_discovery_observation_v1(
        &cases.claude_observation,
        &cases.claude_manifest,
    )
    .expect("startable sanitized Claude observation");
    assert!(claude_observation.can_start);
    for codex_observation in [
        &cases.codex_argv_only_observation,
        &cases.codex_unknown_catalog_observation,
    ] {
        let observation =
            validate_sanitized_discovery_observation_v1(codex_observation, &cases.codex_manifest)
                .expect("blocked sanitized Codex observation");
        assert!(!observation.can_start);
        assert_eq!(
            observation.blocked_reason.as_deref(),
            Some("modelCatalogUnverified")
        );
    }
    let attestation = validate_agent_runtime_attestation_against_accepted_v1(
        &cases.claude_attestation,
        &cases.claude_manifest,
        &cases.claude_observation,
        &cases.accepted_run,
        &cases.resolved_profile,
    )
    .expect("valid runtime attestation");
    assert_eq!(attestation.workspace_identity, "workspace_01");
    validate_internal_provider_event_sequence_v1(&cases.claude_events, &cases.claude_manifest)
        .expect("valid internal provider events");

    assert!(G5A_CAPABILITIES_V1[..G5_CAPABILITIES_V1.len()] == *G5_CAPABILITIES_V1);
    assert_eq!(
        &crate::EVENTS_V1[crate::EVENTS_V1.len() - 2..],
        ["agent.publicEvent", "agent.runStateChanged"]
    );
    assert!(!crate::EVENTS_V1
        .iter()
        .any(|event| event.contains("provider") || event.contains("attestation")));

    for mutation in &cases.invalid_mutations {
        let mut value = match mutation.target.as_str() {
            "claudeManifest" => cases.claude_manifest.clone(),
            "claudeObservation" => cases.claude_observation.clone(),
            "claudeAttestation" => cases.claude_attestation.clone(),
            "acceptedRun" => cases.accepted_run.clone(),
            "resolvedProfile" => cases.resolved_profile.clone(),
            "claudeEvents" => cases.claude_events.clone(),
            other => panic!("unknown G5C mutation target {other}"),
        };
        for change in &mutation.changes {
            let materialized = if change.value == "__G5C_OVERSIZE_PUBLIC_TEXT__" {
                Value::String("a".repeat(G5_PUBLIC_MESSAGE_MAX_BYTES_V1 + 1))
            } else {
                change.value.clone()
            };
            set_g5c_json_pointer(&mut value, &change.pointer, materialized);
        }
        let result = match mutation.target.as_str() {
            "claudeManifest" => validate_provider_manifest_v1(&value).map(|_| ()),
            "claudeObservation" => {
                validate_sanitized_discovery_observation_v1(&value, &cases.claude_manifest)
                    .map(|_| ())
            }
            "claudeAttestation" => validate_agent_runtime_attestation_against_accepted_v1(
                &value,
                &cases.claude_manifest,
                &cases.claude_observation,
                &cases.accepted_run,
                &cases.resolved_profile,
            )
            .map(|_| ()),
            "acceptedRun" => validate_agent_runtime_attestation_against_accepted_v1(
                &cases.claude_attestation,
                &cases.claude_manifest,
                &cases.claude_observation,
                &value,
                &cases.resolved_profile,
            )
            .map(|_| ()),
            "resolvedProfile" => validate_agent_runtime_attestation_against_accepted_v1(
                &cases.claude_attestation,
                &cases.claude_manifest,
                &cases.claude_observation,
                &cases.accepted_run,
                &value,
            )
            .map(|_| ()),
            "claudeEvents" => {
                validate_internal_provider_event_sequence_v1(&value, &cases.claude_manifest)
                    .map(|_| ())
            }
            _ => unreachable!(),
        };
        assert!(
            result.is_err(),
            "invalid G5C mutation passed: {}",
            mutation.name
        );
    }

    for generated in &cases.generated_event_cases {
        let events = generate_g5c_provider_events(generated);
        assert_eq!(
            validate_internal_provider_event_sequence_v1(&events, &cases.claude_manifest).is_ok(),
            generated.valid,
            "generated G5C event validity mismatch: {}",
            generated.name,
        );
        let public = events
            .as_array()
            .expect("generated event array")
            .iter()
            .filter(|event| event["classification"] == "Public")
            .collect::<Vec<_>>();
        assert_eq!(
            public.len(),
            generated.expected_public_event_count,
            "{}",
            generated.name
        );
        assert_eq!(
            public
                .iter()
                .map(|event| event["text"].as_str().expect("public text").len())
                .sum::<usize>(),
            generated.expected_public_text_bytes,
            "{}",
            generated.name
        );
        assert!(
            public
                .iter()
                .all(|event| event["text"].as_str().expect("public text").len()
                    <= G5_PUBLIC_MESSAGE_MAX_BYTES_V1),
            "{}",
            generated.name
        );
        if generated.expected_public_text_bytes >= crate::AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1 {
            assert!(
                public.len() < crate::AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1,
                "{}",
                generated.name
            );
        }
    }
}

fn manifest() -> (PathBuf, Manifest) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/agent-adapter-v1");
    let manifest = read_json(root.join("manifest.json"));
    (root, manifest)
}

fn set_g5c_json_pointer(root: &mut Value, pointer: &str, value: Value) {
    let (parent, key) = pointer
        .rsplit_once('/')
        .unwrap_or_else(|| panic!("invalid JSON pointer {pointer}"));
    let target = if parent.is_empty() {
        root
    } else {
        root.pointer_mut(parent)
            .unwrap_or_else(|| panic!("invalid JSON pointer parent {parent}"))
    };
    if let Some(object) = target.as_object_mut() {
        object.insert(key.to_string(), value);
    } else if let Some(array) = target.as_array_mut() {
        let index: usize = key
            .parse()
            .unwrap_or_else(|_| panic!("JSON pointer array index must be numeric: {key}"));
        array[index] = value;
    } else {
        panic!("JSON pointer parent must be an object or array: {parent}");
    }
}

fn generate_g5c_provider_events(case: &G5CGeneratedEventCase) -> Value {
    let lengths = match case.generator.as_str() {
        "uniformPublicEvents" => vec![
            case.public_text_bytes_each
                .expect("uniform public text bytes");
            case.public_event_count.expect("uniform public event count")
        ],
        "publicTextByteLengths" => case
            .public_text_byte_lengths
            .clone()
            .expect("public text byte lengths"),
        other => panic!("unknown G5C event generator {other}"),
    };
    let mut events = vec![serde_json::json!({
        "schemaVersion": 1,
        "sequence": 1,
        "classification": "Handshake",
        "verifiedRuntimeEvidence": G5C_CLAUDE_RUNTIME_VERIFICATION_V1,
    })];
    for (index, length) in lengths.iter().enumerate() {
        events.push(serde_json::json!({
            "schemaVersion": 1,
            "sequence": index + 2,
            "classification": "Public",
            "kind": "status",
            "text": "a".repeat(*length),
        }));
    }
    events.push(serde_json::json!({
        "schemaVersion": 1,
        "sequence": lengths.len() + 2,
        "classification": "Terminal",
        "outcome": "succeeded",
    }));
    Value::Array(events)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> T {
    let source = fs::read(path).expect("read fixture");
    serde_json::from_slice(&source).expect("decode fixture")
}

fn ready(capabilities: &[&str]) -> Value {
    serde_json::json!({
        "protocolVersion": 1,
        "sidecarSchemaVersion": 1,
        "appSchemaVersion": 1,
        "pid": 4321,
        "platform": "darwin",
        "capabilities": capabilities,
        "sidecarInstanceId": "sidecar_g5a_fixture"
    })
}

fn materialize_g5b(cases: &G5BCases, mutation: &MutationCase) -> Value {
    let mut value = cases
        .valid
        .iter()
        .find(|entry| entry.name == mutation.base_valid_name)
        .unwrap_or_else(|| panic!("missing base fixture {}", mutation.base_valid_name))
        .value
        .clone();
    if let Some(pointer) = &mutation.pointer {
        set_json_pointer(&mut value, pointer, mutation.value.clone());
    }
    for change in &mutation.changes {
        set_json_pointer(&mut value, &change.pointer, change.value.clone());
    }
    value
}

fn materialize_recovery_discovery(
    cases: &RecoveryDiscoveryCases,
    mutation: &RecoveryDiscoveryMutationCase,
) -> Value {
    let mut value = cases
        .valid
        .iter()
        .find(|entry| entry.name == mutation.base_valid_name)
        .unwrap_or_else(|| panic!("missing base fixture {}", mutation.base_valid_name))
        .value
        .clone();
    for pointer in &mutation.removals {
        remove_json_pointer(&mut value, pointer);
    }
    for change in &mutation.changes {
        set_recovery_json_pointer(&mut value, &change.pointer, change.value.clone());
    }
    value
}

fn materialize_identityless_recovery(
    cases: &IdentitylessRecoveryCases,
    mutation: &RecoveryDiscoveryMutationCase,
) -> Value {
    let mut value = cases
        .valid
        .iter()
        .find(|entry| entry.name == mutation.base_valid_name)
        .unwrap_or_else(|| panic!("missing base fixture {}", mutation.base_valid_name))
        .value
        .clone();
    for pointer in &mutation.removals {
        remove_json_pointer(&mut value, pointer);
    }
    for change in &mutation.changes {
        set_recovery_json_pointer(&mut value, &change.pointer, change.value.clone());
    }
    value
}

fn set_recovery_json_pointer(root: &mut Value, pointer: &str, value: Value) {
    let (parent, key) = pointer
        .rsplit_once('/')
        .unwrap_or_else(|| panic!("invalid JSON pointer {pointer}"));
    let target = if parent.is_empty() {
        root
    } else {
        root.pointer_mut(parent)
            .unwrap_or_else(|| panic!("invalid JSON pointer parent {parent}"))
    };
    if let Some(object) = target.as_object_mut() {
        object.insert(key.to_string(), value);
    } else if let Some(array) = target.as_array_mut() {
        let index: usize = key
            .parse()
            .unwrap_or_else(|_| panic!("JSON pointer array index must be numeric: {key}"));
        if index == array.len() {
            array.push(value);
        } else {
            array[index] = value;
        }
    } else {
        panic!("JSON pointer parent must be an object or array: {parent}");
    }
}

fn remove_json_pointer(root: &mut Value, pointer: &str) {
    let (parent, key) = pointer
        .rsplit_once('/')
        .unwrap_or_else(|| panic!("invalid JSON pointer {pointer}"));
    let target = if parent.is_empty() {
        root
    } else {
        root.pointer_mut(parent)
            .unwrap_or_else(|| panic!("invalid JSON pointer parent {parent}"))
    };
    target
        .as_object_mut()
        .unwrap_or_else(|| panic!("JSON pointer parent must be an object: {parent}"))
        .remove(key)
        .unwrap_or_else(|| panic!("missing JSON pointer field {pointer}"));
}

fn set_json_pointer(root: &mut Value, pointer: &str, value: Value) {
    let (parent, key) = pointer
        .rsplit_once('/')
        .unwrap_or_else(|| panic!("invalid JSON pointer {pointer}"));
    let target = if parent.is_empty() {
        root
    } else {
        root.pointer_mut(parent)
            .unwrap_or_else(|| panic!("invalid JSON pointer parent {parent}"))
    };
    target
        .as_object_mut()
        .unwrap_or_else(|| panic!("JSON pointer parent must be an object: {parent}"))
        .insert(key.to_string(), value);
}
