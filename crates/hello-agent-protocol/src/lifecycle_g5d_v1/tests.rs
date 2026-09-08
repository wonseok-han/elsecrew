use super::*;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    protocol_version: u32,
    ready_payloads: ReadyPayloads,
    policy: Policy,
    system_status_cases: Vec<SystemStatusCase>,
    malformed_cases: Vec<SystemStatusCase>,
    blocked_request_cases: Vec<BlockedRequestCase>,
    startup_cases: Vec<StartupCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadyPayloads {
    general_product: Value,
    agent_fixture: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Policy {
    shell_ready_event: String,
    allowed_while_not_ready: Vec<String>,
    ready_only: Vec<String>,
    gate_order: String,
    blocked_request_side_effects: String,
    internal_only_step: String,
    public_states: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemStatusCase {
    name: String,
    expected_valid: bool,
    value: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockedRequestCase {
    name: String,
    state: StorageAdmissionStateV1,
    expected_valid: bool,
    side_effects: String,
    error: ErrorV1,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartupCase {
    name: String,
    expected_valid: bool,
    value: Value,
}

#[test]
fn rust_matches_g5d_admission_policy_and_both_d1_capability_modes() {
    let manifest = manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.protocol_version, crate::PROTOCOL_VERSION);
    assert_eq!(manifest.policy.shell_ready_event, "system.ready");
    assert!(G5D_ALLOWED_WHILE_STORAGE_NOT_READY_V1
        .iter()
        .copied()
        .eq(manifest
            .policy
            .allowed_while_not_ready
            .iter()
            .map(String::as_str)));
    assert!(G5D_STORAGE_READY_ONLY_METHODS_V1
        .iter()
        .copied()
        .eq(manifest.policy.ready_only.iter().map(String::as_str)));
    assert_eq!(
        manifest.policy.gate_order,
        "beforeParameterValidationAndSideEffects"
    );
    assert_eq!(manifest.policy.blocked_request_side_effects, "knownNoOp");
    assert_eq!(manifest.policy.internal_only_step, "verifiedForStartup");
    assert_eq!(
        manifest.policy.public_states,
        ["verifying", "ready", "failed"]
    );

    crate::run_recovery_v1::validate_g5d_product_ready_payload_v1(
        &manifest.ready_payloads.general_product,
    )
    .expect("product ready payload");
    crate::agent_adapter_v1::validate_g5d_agent_fixture_ready_payload_v1(
        &manifest.ready_payloads.agent_fixture,
    )
    .expect("agent fixture ready payload");
}

#[test]
fn rust_accepts_three_states_and_rejects_malformed_cross_state_combinations() {
    let manifest = manifest();
    assert!(manifest
        .malformed_cases
        .iter()
        .all(|case| !case.expected_valid));
    for case in manifest
        .system_status_cases
        .into_iter()
        .chain(manifest.malformed_cases)
    {
        for (mode, sidecar) in [
            ("product", &manifest.ready_payloads.general_product),
            ("agentFixture", &manifest.ready_payloads.agent_fixture),
        ] {
            let mut value = case.value.clone();
            value.insert("sidecar".to_string(), sidecar.clone());
            let value = Value::Object(value);
            let result = if mode == "product" {
                parse_g5d_product_system_status_v1(&value)
            } else {
                parse_g5d_agent_fixture_system_status_v1(&value)
            };
            assert_eq!(
                result.is_ok(),
                case.expected_valid,
                "{} ({mode})",
                case.name
            );
        }
    }
}

#[test]
fn rust_matches_blocked_errors_known_no_op_policy_and_startup_order() {
    let manifest = manifest();
    for case in manifest.blocked_request_cases {
        assert_eq!(case.side_effects, "knownNoOp", "{}", case.name);
        assert_eq!(
            validate_g5d_blocked_request_error_v1(case.state, &case.error).is_ok(),
            case.expected_valid,
            "{}",
            case.name
        );
    }
    for case in manifest.startup_cases {
        let result = serde_json::from_value::<StartupTraceG5DV1>(case.value)
            .map_err(|error| G5DAdmissionContractError::new(error.to_string()))
            .and_then(|trace| validate_g5d_startup_trace_v1(&trace));
        assert_eq!(result.is_ok(), case.expected_valid, "{}", case.name);
    }
}

fn manifest() -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/g5d-admission-v1/manifest.json");
    let source = fs::read(path).expect("read G5D admission fixture");
    serde_json::from_slice(&source).expect("decode G5D admission fixture")
}
