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
    events: Vec<String>,
    bridge_v2_methods: Vec<String>,
    bridge_v3_workspace_methods: Vec<String>,
    id_max_bytes: usize,
    addressing: String,
    operation_resolution: String,
    save_stream_binding: String,
    v2_project_tree_request_line: String,
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
fn rust_matches_the_canonical_workspace_manifest() {
    let (root, manifest) = manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.protocol_version, super::super::PROTOCOL_VERSION);
    assert!(WORKSPACE_CAPABILITIES_V1.iter().copied().eq(manifest
        .contract
        .capabilities
        .iter()
        .map(String::as_str)));
    assert!(WORKSPACE_EVENTS_V1.iter().copied().eq(manifest
        .contract
        .events
        .iter()
        .map(String::as_str)));
    assert!(WORKSPACE_CAPABILITIES_V1
        .iter()
        .all(|method| super::super::METHODS_V1.contains(method)));
    assert!(WORKSPACE_EVENTS_V1
        .iter()
        .all(|event| super::super::EVENTS_V1.contains(event)));
    assert_eq!(
        manifest.contract.addressing,
        "explicitChatProjectWorktreeWorkspaceIdentity"
    );
    assert_eq!(
        manifest.contract.operation_resolution,
        "persistedChatRecordAndActiveSnapshotBeforeOperation"
    );
    assert_eq!(
        manifest.contract.save_stream_binding,
        "everyPhaseEchoesChatWorkspaceAndMatchesPersistedSaveSession"
    );
    let v2_request =
        super::super::decode_jsonl(manifest.contract.v2_project_tree_request_line.as_bytes())
            .expect("decode unchanged V2 project request");
    assert_eq!(
        super::super::encode_jsonl(&v2_request).expect("encode unchanged V2 project request"),
        manifest.contract.v2_project_tree_request_line
    );
    assert_eq!(manifest.contract.bridge_v2_methods.len(), 15);
    assert_eq!(manifest.contract.bridge_v3_workspace_methods.len(), 12);
    assert_eq!(
        manifest.contract.id_max_bytes,
        super::super::run_recovery_v1::G5_ID_MAX_BYTES_V1
    );
    let boundary_id = format!("{}aa", "가".repeat(170));
    assert_eq!(boundary_id.len(), manifest.contract.id_max_bytes);
    assert!(validate_workspace_ref(&serde_json::json!({
        "projectId": boundary_id.clone(), "worktreeId": "worktree", "workspaceIdentity": "workspace"
    }))
    .is_ok());
    assert!(validate_workspace_ref(&serde_json::json!({
        "projectId": format!("{boundary_id}a"), "worktreeId": "worktree", "workspaceIdentity": "workspace"
    }))
    .is_err());
    assert!(root.join(manifest.cases_file).is_file());
}

#[test]
fn rust_accepts_and_rejects_the_same_canonical_workspace_cases() {
    let (root, manifest) = manifest();
    let cases: Cases = read_json(root.join(manifest.cases_file));
    let valid_by_name: HashMap<_, _> = cases
        .valid
        .iter()
        .map(|fixture| (fixture.name.as_str(), &fixture.value))
        .collect();

    for fixture in &cases.valid {
        validate_workspace_payload_v1(&fixture.validator, &fixture.value)
            .unwrap_or_else(|error| panic!("valid fixture {} failed: {error}", fixture.name));
    }
    for fixture in &cases.invalid {
        assert!(
            validate_workspace_payload_v1(&fixture.validator, &fixture.value).is_err(),
            "{}",
            fixture.name
        );
    }
    for fixture in cases.derived {
        let mut value = (*valid_by_name
            .get(fixture.base_valid_name.as_str())
            .unwrap_or_else(|| panic!("missing base fixture {}", fixture.base_valid_name)))
        .clone();
        for change in fixture.changes {
            apply_json_pointer(&mut value, &change.pointer, change.value);
        }
        assert_eq!(
            validate_workspace_payload_v1(&fixture.validator, &value).is_ok(),
            fixture.expected_valid,
            "{}",
            fixture.name
        );
    }
}

fn manifest() -> (PathBuf, Manifest) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/g5-workspace-v1");
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
            Value::Object(object) => object.get_mut(segment).expect("fixture object pointer"),
            Value::Array(array) => {
                &mut array[segment.parse::<usize>().expect("fixture array pointer")]
            }
            _ => panic!("invalid fixture pointer {pointer}"),
        };
    }
    let last = segments.last().expect("nonempty fixture pointer");
    match current {
        Value::Object(object) => {
            object.insert(last.clone(), value);
        }
        Value::Array(array) => array[last.parse::<usize>().expect("fixture array pointer")] = value,
        _ => panic!("invalid fixture pointer {pointer}"),
    }
}
