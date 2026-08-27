use super::*;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u64,
    protocol_version: u32,
    contract: Contract,
    cases_file: String,
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

fn manifest() -> (PathBuf, Manifest) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/agent-adapter-v1");
    let manifest = read_json(root.join("manifest.json"));
    (root, manifest)
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
