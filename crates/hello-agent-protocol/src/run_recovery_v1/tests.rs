use super::*;
use serde::Deserialize;
use std::collections::HashMap;
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
    roles: Vec<String>,
    chat_states: Vec<String>,
    run_start_chat_states: Vec<String>,
    run_start_outcome_priority: Vec<String>,
    runtimes: Vec<String>,
    permissions: Vec<String>,
    fixture_scenarios: Vec<String>,
    page_max_items: usize,
    event_page_max_items: usize,
    id_max_bytes: usize,
    model_selector_max_bytes: usize,
    reasoning_effort_max_bytes: usize,
    goal_max_bytes: usize,
    public_message_max_bytes: usize,
    chat_workspace_preparation: String,
    platform_execution: PlatformExecution,
    agent_smoke_advertised: bool,
}

#[derive(Deserialize)]
struct PlatformExecution {
    macos: String,
    windows: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreparationManifest {
    schema_version: u64,
    protocol_version: u32,
    contract: PreparationContract,
    cases_file: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreparationContract {
    methods: Vec<String>,
    prepare_semantics: String,
    chat_bind_semantics: String,
    cleanup_semantics: String,
    physical_deletion: bool,
    idempotency: String,
}

#[derive(Deserialize)]
struct Cases {
    valid: Vec<FixtureCase>,
    invalid: Vec<FixtureCase>,
    derived: Vec<DerivedCase>,
}

#[derive(Deserialize)]
struct FixtureCase {
    name: String,
    validator: String,
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DerivedCase {
    name: String,
    base_valid_name: String,
    validator: String,
    changes: Vec<FixtureChange>,
    expected_valid: bool,
}

#[derive(Deserialize)]
struct FixtureChange {
    pointer: String,
    value: Value,
}

#[test]
fn rust_matches_the_canonical_g5_manifest() {
    let (root, manifest) = manifest();
    assert_eq!(manifest.schema_version, G5_SCHEMA_VERSION_V1);
    assert_eq!(manifest.protocol_version, super::super::PROTOCOL_VERSION);
    assert!(G5_CAPABILITIES_V1.iter().copied().eq(manifest
        .contract
        .capabilities
        .iter()
        .map(String::as_str)));
    assert!(G5_ROLE_IDS_V1
        .iter()
        .copied()
        .eq(manifest.contract.roles.iter().map(String::as_str)));
    assert!(G5_CHAT_STATES_V1.iter().copied().eq(manifest
        .contract
        .chat_states
        .iter()
        .map(String::as_str)));
    assert!(G5_RUN_START_CHAT_STATES_V1.iter().copied().eq(manifest
        .contract
        .run_start_chat_states
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.contract.run_start_outcome_priority,
        [
            "platformUnsupported",
            "workspaceAbandoned",
            "recoveryBlocked",
            "chatStateBlocked",
            "runActive",
            "profileUnavailable",
            "writerLocked",
            "started",
        ]
    );
    assert_eq!(
        manifest.contract.chat_workspace_preparation,
        "newUnboundSnapshotThenAtomicBind"
    );
    assert!(G5_RUNTIMES_V1.iter().copied().eq(manifest
        .contract
        .runtimes
        .iter()
        .map(String::as_str)));
    assert!(G5_PERMISSIONS_V1.iter().copied().eq(manifest
        .contract
        .permissions
        .iter()
        .map(String::as_str)));
    assert!(G5_FIXTURE_SCENARIOS_V1.iter().copied().eq(manifest
        .contract
        .fixture_scenarios
        .iter()
        .map(String::as_str)));
    assert_eq!(manifest.contract.page_max_items, G5_PAGE_MAX_ITEMS_V1);
    assert_eq!(
        manifest.contract.event_page_max_items,
        G5_EVENT_PAGE_MAX_ITEMS_V1
    );
    assert_eq!(manifest.contract.id_max_bytes, G5_ID_MAX_BYTES_V1);
    assert_eq!(
        manifest.contract.model_selector_max_bytes,
        G5_MODEL_SELECTOR_MAX_BYTES_V1
    );
    assert_eq!(
        manifest.contract.reasoning_effort_max_bytes,
        G5_REASONING_EFFORT_MAX_BYTES_V1
    );
    assert_eq!(manifest.contract.goal_max_bytes, G5_GOAL_MAX_BYTES_V1);
    assert_eq!(
        manifest.contract.public_message_max_bytes,
        G5_PUBLIC_MESSAGE_MAX_BYTES_V1
    );
    assert_eq!(
        manifest.contract.platform_execution.macos,
        "fakeFixtureOnly"
    );
    assert_eq!(manifest.contract.platform_execution.windows, "disabled");
    assert!(!manifest.contract.agent_smoke_advertised);
    assert_g5_capabilities_v1().expect("G5 capabilities");
    let ready = serde_json::json!({
        "protocolVersion": 1,
        "sidecarSchemaVersion": 1,
        "appSchemaVersion": 1,
        "pid": 4321,
        "platform": "darwin",
        "capabilities": manifest.contract.capabilities.clone(),
        "sidecarInstanceId": "sidecar_g5_fixture"
    });
    validate_g5_ready_payload_v1(&ready).expect("exact G5 ready");
    let mut with_agent_smoke = ready;
    with_agent_smoke["capabilities"]
        .as_array_mut()
        .expect("capabilities")
        .push(Value::String("run.startAgentSmoke".to_string()));
    assert!(validate_g5_ready_payload_v1(&with_agent_smoke).is_err());
    assert!(root.join(manifest.cases_file).is_file());
}

#[test]
fn rust_accepts_and_rejects_the_same_canonical_g5_cases() {
    let (root, manifest) = manifest();
    let cases: Cases = read_json(root.join(manifest.cases_file));
    for fixture in cases.valid {
        validate_g5_payload_v1(&fixture.validator, &fixture.value)
            .unwrap_or_else(|error| panic!("valid fixture {} failed: {error}", fixture.name));
    }
    for fixture in cases.invalid {
        assert!(
            validate_g5_payload_v1(&fixture.validator, &fixture.value).is_err(),
            "invalid fixture unexpectedly passed: {}",
            fixture.name
        );
    }
}

#[test]
fn rust_matches_canonical_chat_workspace_preparation_cases() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/g5-preparation-v1");
    let manifest: PreparationManifest = read_json(root.join("manifest.json"));
    assert_eq!(manifest.schema_version, G5_SCHEMA_VERSION_V1);
    assert_eq!(manifest.protocol_version, super::super::PROTOCOL_VERSION);
    assert_eq!(
        manifest.contract.methods,
        [
            "workspace.prepareChat",
            "workspace.abandonPrepared",
            "chat.create"
        ]
    );
    assert_eq!(
        manifest.contract.prepare_semantics,
        "alwaysNewStandaloneSnapshot"
    );
    assert_eq!(
        manifest.contract.chat_bind_semantics,
        "unboundToBoundWithChatAndInitialMessageAtomically"
    );
    assert_eq!(
        manifest.contract.cleanup_semantics,
        "exactUnboundPreparationOnlyEvidencePreserving"
    );
    assert!(!manifest.contract.physical_deletion);
    assert_eq!(
        manifest.contract.idempotency,
        "duplicateOrLateMutationIsFailClosedNoOp"
    );
    let cases: Cases = read_json(root.join(manifest.cases_file));
    let valid: HashMap<_, _> = cases
        .valid
        .iter()
        .map(|fixture| (fixture.name.clone(), fixture.value.clone()))
        .collect();
    for fixture in &cases.valid {
        validate_g5_payload_v1(&fixture.validator, &fixture.value).unwrap_or_else(|error| {
            panic!("valid preparation fixture {} failed: {error}", fixture.name)
        });
    }
    for fixture in &cases.invalid {
        assert!(
            validate_g5_payload_v1(&fixture.validator, &fixture.value).is_err(),
            "invalid preparation fixture unexpectedly passed: {}",
            fixture.name
        );
    }
    for fixture in cases.derived {
        let mut value = valid
            .get(&fixture.base_valid_name)
            .unwrap_or_else(|| panic!("missing preparation base for {}", fixture.name))
            .clone();
        for change in fixture.changes {
            apply_json_pointer(&mut value, &change.pointer, change.value);
        }
        assert_eq!(
            validate_g5_payload_v1(&fixture.validator, &value).is_ok(),
            fixture.expected_valid,
            "{}",
            fixture.name
        );
    }
}

#[test]
fn rust_matches_canonical_settings_drift_cross_chat_and_recovery_outcomes() {
    let (root, manifest) = manifest();
    let cases: Cases = read_json(root.join(manifest.cases_file));
    let valid: HashMap<_, _> = cases
        .valid
        .into_iter()
        .map(|fixture| (fixture.name, fixture.value))
        .collect();
    for fixture in cases.derived {
        let mut value = valid
            .get(&fixture.base_valid_name)
            .unwrap_or_else(|| panic!("missing base for {}", fixture.name))
            .clone();
        for change in fixture.changes {
            apply_json_pointer(&mut value, &change.pointer, change.value);
        }
        assert_eq!(
            validate_g5_payload_v1(&fixture.validator, &value).is_ok(),
            fixture.expected_valid,
            "{}",
            fixture.name
        );
    }
}

#[test]
fn rust_enforces_exact_role_text_and_page_bounds() {
    let profile = |model: String, effort: Option<String>| {
        serde_json::json!({
            "roleId": "coding",
            "runtime": "codex",
            "modelSelector": model,
            "reasoningEffort": effort,
            "permission": "workspaceWrite"
        })
    };
    assert!(validate_g5_payload_v1(
        "roleProfile",
        &profile("m".repeat(G5_MODEL_SELECTOR_MAX_BYTES_V1), None)
    )
    .is_ok());
    assert!(validate_g5_payload_v1(
        "roleProfile",
        &profile("m".repeat(G5_MODEL_SELECTOR_MAX_BYTES_V1 + 1), None)
    )
    .is_err());
    assert!(validate_g5_payload_v1(
        "roleProfile",
        &profile(
            "model".to_string(),
            Some("e".repeat(G5_REASONING_EFFORT_MAX_BYTES_V1))
        )
    )
    .is_ok());
    assert!(validate_g5_payload_v1(
        "roleProfile",
        &profile(
            "model".to_string(),
            Some("e".repeat(G5_REASONING_EFFORT_MAX_BYTES_V1 + 1))
        )
    )
    .is_err());
    let boundary_id = format!("{}aa", "가".repeat(170));
    assert_eq!(boundary_id.len(), G5_ID_MAX_BYTES_V1);
    assert!(validate_g5_payload_v1(
        "fixtureRunStart",
        &serde_json::json!({"chatId": boundary_id, "roleId": "planning", "scenario": "planning-success"})
    )
    .is_ok());
    assert!(validate_g5_payload_v1(
        "fixtureRunStart",
        &serde_json::json!({"chatId": format!("{boundary_id}a"), "roleId": "planning", "scenario": "planning-success"})
    )
    .is_err());
    let bindings: Vec<_> = (0..G5_PAGE_MAX_ITEMS_V1)
        .map(|index| {
            serde_json::json!({
                "chatId": format!("chat_{index}"),
                "projectId": "project_boundary",
                "worktreeId": format!("worktree_{index}"),
                "workspaceIdentity": format!("workspace_{index}")
            })
        })
        .collect();
    let boundary_page =
        serde_json::json!([{"cursor": null, "items": bindings.clone(), "nextCursor": null}]);
    assert!(validate_chat_binding_pages(&boundary_page).is_ok());
    let mut over_bound = bindings;
    over_bound.push(serde_json::json!({
        "chatId": "chat_over", "projectId": "project_boundary",
        "worktreeId": "worktree_over", "workspaceIdentity": "workspace_over"
    }));
    let over_bound_page =
        serde_json::json!([{"cursor": null, "items": over_bound, "nextCursor": null}]);
    assert!(validate_chat_binding_pages(&over_bound_page).is_err());
}

fn manifest() -> (PathBuf, Manifest) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/g5-v1");
    let manifest = read_json(root.join("manifest.json"));
    (root, manifest)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> T {
    let source = fs::read(path).expect("read fixture");
    serde_json::from_slice(&source).expect("decode fixture")
}

fn apply_json_pointer(root: &mut Value, pointer: &str, value: Value) {
    let segments: Vec<_> = pointer
        .split('/')
        .skip(1)
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        .collect();
    let mut current = root;
    for segment in &segments[..segments.len() - 1] {
        current = match current {
            Value::Object(object) => object
                .get_mut(segment)
                .unwrap_or_else(|| panic!("invalid fixture pointer {pointer}")),
            Value::Array(array) => {
                &mut array[segment
                    .parse::<usize>()
                    .unwrap_or_else(|_| panic!("invalid fixture pointer {pointer}"))]
            }
            _ => panic!("invalid fixture pointer {pointer}"),
        };
    }
    let last = segments.last().expect("nonempty fixture pointer");
    match current {
        Value::Object(object) => {
            object.insert(last.clone(), value);
        }
        Value::Array(array) => {
            array[last
                .parse::<usize>()
                .unwrap_or_else(|_| panic!("invalid fixture pointer {pointer}"))] = value;
        }
        _ => panic!("invalid fixture pointer {pointer}"),
    }
}
