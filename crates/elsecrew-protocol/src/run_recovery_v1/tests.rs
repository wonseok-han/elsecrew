use super::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[test]
fn chat_preparation_baseline_is_optional_and_strictly_boolean() {
    for value in [serde_json::json!({"projectId":"project_a"}), serde_json::json!({"projectId":"project_a","applicationBaseline":true}), serde_json::json!({"projectId":"project_a","applicationBaseline":false})] {
        assert!(validate_g5_payload_v1("prepareChatWorkspaceRequest", &value).is_ok());
    }
    for value in [serde_json::json!({"projectId":"project_a","applicationBaseline":null}), serde_json::json!({"projectId":"project_a","applicationBaseline":"true"}), serde_json::json!({"projectId":"project_a","applicationBaseline":true,"path":"/tmp"})] {
        assert!(validate_g5_payload_v1("prepareChatWorkspaceRequest", &value).is_err());
    }
}

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

#[test]
fn v3_role_ids_stage_and_legacy_permissions_are_separate() {
    for valid in ["qa", "coordinator", "ui-design", "a1", &format!("a{}", "b".repeat(31))] {
        assert!(is_g5_role_id_v3(valid), "{valid}");
    }
    for invalid in ["", "a", "1a", "-a", "Qa", "q a", "q_a", "한글", &format!("a{}", "b".repeat(32))] {
        assert!(!is_g5_role_id_v3(invalid), "{invalid}");
    }
    assert_eq!(g5_stage_for_profile_v3("planning", "workspaceWrite"), "implementing");
    assert_eq!(g5_stage_for_profile_v3("coding", "readOnly"), "planning");
    assert_eq!(g5_stage_for_profile_v3("review", "readOnly"), "verifying");
    assert_eq!(g5_stage_for_profile_v3("qa", "readOnly"), "planning");
    let profile = |role: &str, permission: &str| serde_json::json!({"roleId":role,"runtime":"codex","modelSelector":"m","reasoningEffort":null,"permission":permission});
    // Standalone profiles no longer derive permission from the name.
    assert!(validate_g5_payload_v1("roleProfile", &profile("planning", "workspaceWrite")).is_ok());
    assert!(validate_g5_payload_v1("roleProfile", &profile("coordinator", "workspaceWrite")).is_err());
    // Stored V2 sets still require the exact fixed mapping.
    let ids = ["coordinator", "planning", "design", "coding", "review"];
    let mut roles: Vec<_> = ids.iter().map(|id| profile(id, if *id == "coding" { "workspaceWrite" } else { "readOnly" })).collect();
    let settings = |roles: &Vec<Value>| serde_json::json!({"schemaVersion":2,"projectId":"p","revision":1,"roles":roles,"updatedAt":"2026-11-12T00:00:00Z"});
    assert!(validate_g5_payload_v1("projectRoleSettings", &settings(&roles)).is_ok());
    roles[3]["permission"] = serde_json::json!("readOnly");
    assert!(validate_g5_payload_v1("projectRoleSettings", &settings(&roles)).is_err());
    // V3 default seed is internally valid.
    let definitions: Vec<_> = G5_DEFAULT_ROLE_DEFINITIONS_V3.iter().map(|(id, name, text)| serde_json::json!({"roleId":id,"name":name,"responsibility":text})).collect();
    roles[3]["permission"] = serde_json::json!("workspaceWrite");
    let v3 = serde_json::json!({"schemaVersion":3,"projectId":"p","revision":1,"roles":roles,"roleDefinitions":definitions,"updatedAt":"2026-11-12T00:00:00Z"});
    assert!(validate_g5_payload_v1("projectRoleSettings", &v3).is_ok());
    let mut missing = v3.clone();
    missing.as_object_mut().unwrap().remove("roleDefinitions");
    assert!(validate_g5_payload_v1("projectRoleSettings", &missing).is_err());
}

#[test]
fn saved_v3_settings_may_leave_a_model_unselected_but_chats_may_not() {
    let unselected = |role: &str, permission: &str| serde_json::json!({"roleId":role,"runtime":"codex","modelSelector":"","reasoningEffort":null,"permission":permission});
    let roles = serde_json::json!([
        unselected("coordinator", "readOnly"), unselected("planning", "readOnly"), unselected("design", "readOnly"),
        unselected("coding", "workspaceWrite"), unselected("review", "readOnly"),
    ]);
    let definitions: Vec<Value> = G5_DEFAULT_ROLE_DEFINITIONS_V3.iter()
        .map(|(id, name, responsibility)| serde_json::json!({"roleId":id,"name":name,"responsibility":responsibility}))
        .collect();
    let at = "2026-11-13T00:00:00Z";
    let settings = serde_json::json!({"schemaVersion":3,"projectId":"p","revision":1,"roles":roles,"roleDefinitions":definitions,"updatedAt":at});
    assert!(validate_g5_payload_v1("projectRoleSettings", &settings).is_ok());
    let mut effort = settings.clone();
    effort["roles"][1]["reasoningEffort"] = serde_json::json!("medium");
    assert!(validate_g5_payload_v1("projectRoleSettings", &effort).is_err());
    let mut blank = settings.clone();
    blank["roles"][1]["modelSelector"] = serde_json::json!("  ");
    assert!(validate_g5_payload_v1("projectRoleSettings", &blank).is_err());
    assert!(validate_g5_payload_v1("roleProfile", &roles[1]).is_err());
    let chat = serde_json::json!({"schemaVersion":3,"chatId":"c","taskId":"t","projectId":"p","worktreeId":"w","workspaceIdentity":"i","goal":"g","settingsRevision":1,"roleSnapshots":roles,"roleDefinitions":definitions,"state":"draft","createdAt":at,"updatedAt":at});
    assert!(validate_g5_payload_v1("chatRecord", &chat).is_err());
    let legacy = serde_json::json!({"schemaVersion":1,"projectId":"p","revision":1,"roles":[unselected("planning","readOnly"),unselected("coding","workspaceWrite"),unselected("review","readOnly")],"updatedAt":at});
    assert!(validate_g5_payload_v1("projectRoleSettings", &legacy).is_err());
}

#[test]
fn saved_role_names_follow_the_shared_table_and_stored_ones_still_open() {
    let table: Value = serde_json::from_str(include_str!("../../../../tests/fixtures/m5/role-names-v3.json")).unwrap();
    let ranges: Vec<(u32, u32)> = table["formatRanges"].as_array().unwrap().iter()
        .map(|pair| (pair[0].as_u64().unwrap() as u32, pair[1].as_u64().unwrap() as u32)).collect();
    assert_eq!(ranges, G5_FORMAT_CHAR_RANGES_V3, "Rust and TS share one format-character table");
    let labels: Vec<(String, Vec<String>)> = serde_json::from_value(table["legacyLabels"].clone()).unwrap();
    assert_eq!(labels.len(), G5_LEGACY_ROLE_LABELS_V3.len());
    for ((id, shown), (rust_id, rust_shown)) in labels.iter().zip(G5_LEGACY_ROLE_LABELS_V3) {
        assert_eq!((id.as_str(), shown.iter().map(String::as_str).collect::<Vec<_>>()), (*rust_id, rust_shown.to_vec()));
    }
    for case in table["cases"].as_array().unwrap() {
        let names: Vec<&str> = case["names"].as_array().unwrap().iter().map(|name| name.as_str().unwrap()).collect();
        let expected: Vec<Option<&str>> = case["problems"].as_array().unwrap().iter().map(Value::as_str).collect();
        let ids: Vec<String> = match case["roleIds"].as_array() {
            Some(ids) => ids.iter().map(|id| id.as_str().unwrap().to_owned()).collect(),
            None => (0..names.len()).map(|index| if index == 0 { "coordinator".into() } else { format!("member-{index}") }).collect(),
        };
        let named: Vec<(&str, &str)> = ids.iter().map(String::as_str).zip(names.iter().copied()).collect();
        assert_eq!(role_name_problems_v3(&named), expected, "{}", case["note"]);
        let profile = |id: &str| serde_json::json!({"roleId":id,"runtime":"claude-code","modelSelector":"opus","reasoningEffort":null,"permission":"readOnly"});
        let roles: Vec<Value> = ids.iter().map(|id| profile(id)).collect();
        let definitions: Vec<Value> = ids.iter().zip(&names).map(|(id, name)| serde_json::json!({"roleId":id,"name":name,"responsibility":""})).collect();
        let update = serde_json::json!({"projectId":"p","expectedRevision":1,"roles":roles,"roleDefinitions":definitions});
        assert_eq!(validate_g5_payload_v1("projectRoleSettingsUpdate", &update).is_ok(), expected.iter().all(Option::is_none), "{}", case["note"]);
        // A stored record with the same names still reads (only Cc is refused there).
        let stored = serde_json::json!({"schemaVersion":3,"projectId":"p","revision":1,"roles":roles,"roleDefinitions":definitions,"updatedAt":"2026-11-26T00:00:00Z"});
        assert!(validate_g5_payload_v1("projectRoleSettings", &stored).is_ok(), "{}", case["note"]);
    }
}
