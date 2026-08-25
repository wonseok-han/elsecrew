use super::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureManifest {
    schema_version: u64,
    protocol_version: u64,
    contract: ContractFixture,
    cases_file: String,
    activation_evidence_file: String,
    generated_cases: Vec<GeneratedCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractFixture {
    capabilities: Vec<String>,
    bridge_methods: Vec<String>,
    tree_page_max_entries: usize,
    text_chunk_max_bytes: usize,
    file_max_bytes: u64,
    file_version_algorithm: String,
    file_version_prefix: String,
    file_version_digest_input: String,
    canonical_path_format: String,
    worktree_head_mode: String,
    workspace_kind: String,
    snapshot_transport: String,
    source_mutation_policy: String,
    worktree_lifecycles: Vec<String>,
    snapshot_limits: SnapshotLimitsFixture,
    snapshot_creation_phases: Vec<String>,
    snapshot_failure_kinds: Vec<String>,
    materialization: Value,
    threat_model: Value,
    platform_support: Value,
    project_close_lifecycle: String,
    project_close_physically_deletes: bool,
    tree_cursor_semantics: String,
    save_concurrency: String,
    watch_overflow: String,
    safety_error_kinds: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotLimitsFixture {
    bundle_max_bytes: u64,
    materialized_total_max_bytes: u64,
    entries_max: u64,
    single_blob_max_bytes: u64,
    disk_reserve_bytes: u64,
    streaming_required: bool,
    full_allocation_forbidden: bool,
}

#[derive(Deserialize)]
struct FixtureCases {
    valid: Vec<FixtureCase>,
    invalid: Vec<FixtureCase>,
}

#[derive(Deserialize)]
struct FixtureCase {
    name: String,
    validator: String,
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivationEvidenceFixture {
    schema_version: u64,
    validator: String,
    valid: Value,
    invalid_cases: Vec<ActivationEvidenceInvalidCase>,
}

#[derive(Deserialize)]
struct ActivationEvidenceInvalidCase {
    name: String,
    changes: Vec<ActivationEvidenceChange>,
}

#[derive(Deserialize)]
struct ActivationEvidenceChange {
    pointer: String,
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedCase {
    name: String,
    validator: String,
    entry_count: Option<usize>,
    text_bytes: Option<usize>,
    byte_length: Option<u64>,
    utf8_valid: Option<bool>,
    classification: Option<String>,
    expected_valid: bool,
}

#[test]
fn manifest_is_the_canonical_g4_limits_and_capabilities_contract() {
    let (_, manifest) = fixture_manifest();
    assert_eq!(manifest.schema_version, PROJECT_SCHEMA_VERSION_V1);
    assert_eq!(
        manifest.protocol_version,
        super::super::PROTOCOL_VERSION as u64
    );
    assert!(G4_CAPABILITIES_V1.iter().copied().eq(manifest
        .contract
        .capabilities
        .iter()
        .map(String::as_str)));
    assert_g4_capabilities_v1().expect("G4 capabilities remain reserved in protocol v1");
    assert_eq!(
        manifest.contract.tree_page_max_entries,
        TREE_PAGE_MAX_ENTRIES_V1
    );
    assert_eq!(
        manifest.contract.text_chunk_max_bytes,
        super::super::MAX_TEXT_CHUNK_BYTES
    );
    assert_eq!(manifest.contract.file_max_bytes, FILE_MAX_BYTES_V1);
    assert_eq!(
        manifest.contract.file_version_algorithm,
        FILE_VERSION_ALGORITHM_V1
    );
    assert_eq!(
        manifest.contract.file_version_prefix,
        FILE_VERSION_PREFIX_V1
    );
    assert_eq!(
        manifest.contract.file_version_digest_input,
        FILE_VERSION_DIGEST_INPUT_V1
    );
    assert_eq!(
        manifest.contract.canonical_path_format,
        "absoluteRealPathForwardSlashesUppercaseWindowsDrive"
    );
    assert_eq!(manifest.contract.worktree_head_mode, WORKTREE_HEAD_MODE_V1);
    assert_eq!(manifest.contract.workspace_kind, WORKSPACE_KIND_V1);
    assert_eq!(manifest.contract.snapshot_transport, SNAPSHOT_TRANSPORT_V1);
    assert_eq!(
        manifest.contract.source_mutation_policy,
        SOURCE_MUTATION_POLICY_V1
    );
    assert!(WORKTREE_LIFECYCLES_V1.iter().copied().eq(manifest
        .contract
        .worktree_lifecycles
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.contract.snapshot_limits.bundle_max_bytes,
        SNAPSHOT_BUNDLE_MAX_BYTES_V1
    );
    assert_eq!(
        manifest
            .contract
            .snapshot_limits
            .materialized_total_max_bytes,
        SNAPSHOT_MATERIALIZED_TOTAL_MAX_BYTES_V1
    );
    assert_eq!(
        manifest.contract.snapshot_limits.entries_max,
        SNAPSHOT_ENTRIES_MAX_V1
    );
    assert_eq!(
        manifest.contract.snapshot_limits.single_blob_max_bytes,
        SNAPSHOT_SINGLE_BLOB_MAX_BYTES_V1
    );
    assert_eq!(
        manifest.contract.snapshot_limits.disk_reserve_bytes,
        SNAPSHOT_DISK_RESERVE_BYTES_V1
    );
    assert!(manifest.contract.snapshot_limits.streaming_required);
    assert!(manifest.contract.snapshot_limits.full_allocation_forbidden);
    assert!(SNAPSHOT_CREATION_PHASES_V1.iter().copied().eq(manifest
        .contract
        .snapshot_creation_phases
        .iter()
        .map(String::as_str)));
    assert!(SNAPSHOT_FAILURE_KINDS_V1.iter().copied().eq(manifest
        .contract
        .snapshot_failure_kinds
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.contract.materialization,
        json!({
            "blobSource":"rawCommittedBytes",
            "modeSource":"committedTreeMode",
            "symlinkPolicy":"noFollow",
            "submodulePolicy":"noInit",
            "forbiddenTransforms":["checkout","filter","eol","encoding","ident","exportAttributes","lfs"],
            "failClosedEntries":["caseCollision","nonUtf8Path","dotGitEntry","abnormalMode"]
        })
    );
    assert_eq!(
        manifest.contract.threat_model,
        json!({
            "protected":["selectedRootPathSwap","managedTargetPathSwap","ordinaryConcurrentSourceChange"],
            "requiredChecks":["exportedBaseCommit","exportedBaseTree","objectFormat","sourceDigestBeforeAfter"],
            "outOfBoundary":["activelyMaliciousSameUserEntryMutationInsideHeldSourceDirectoryDuringExport"]
        })
    );
    assert_eq!(
        manifest.contract.platform_support,
        json!({"macos":"contracted","windows":"unsupportedUntilSeparateGate"})
    );
    assert_eq!(manifest.contract.project_close_lifecycle, "abandoned");
    assert!(!manifest.contract.project_close_physically_deletes);
    assert_eq!(manifest.contract.tree_cursor_semantics, "opaque");
    assert_eq!(
        manifest.contract.save_concurrency,
        "expectedVersionMustEqualCurrentVersion"
    );
    assert_eq!(manifest.contract.watch_overflow, "rescanRequired");
    assert!(G4_ERROR_KINDS_V1.iter().copied().eq(manifest
        .contract
        .safety_error_kinds
        .iter()
        .map(String::as_str)));
    assert_eq!(manifest.contract.bridge_methods.len(), 15);
}

#[test]
fn rust_accepts_every_canonical_valid_g4_payload() {
    let (root, manifest) = fixture_manifest();
    let cases = fixture_cases(&root, &manifest.cases_file);
    for fixture in cases.valid {
        parse_project_payload_v1(&fixture.validator, &fixture.value)
            .unwrap_or_else(|error| panic!("{}: {error}", fixture.name));
    }
}

#[test]
fn rust_rejects_every_canonical_invalid_g4_payload_fail_closed() {
    let (root, manifest) = fixture_manifest();
    let cases = fixture_cases(&root, &manifest.cases_file);
    for fixture in cases.invalid {
        assert!(
            parse_project_payload_v1(&fixture.validator, &fixture.value).is_err(),
            "fixture unexpectedly passed: {}",
            fixture.name
        );
    }
}

#[test]
fn rust_requires_one_composite_pre_activation_identity_evidence() {
    let (root, manifest) = fixture_manifest();
    let source = fs::read(root.join(manifest.activation_evidence_file))
        .expect("read activation evidence fixture");
    let fixture: ActivationEvidenceFixture =
        serde_json::from_slice(&source).expect("decode activation evidence fixture");
    assert_eq!(fixture.schema_version, PROJECT_SCHEMA_VERSION_V1);
    assert_eq!(fixture.validator, "snapshotActivationEvidence");
    validate_project_payload_v1(&fixture.validator, &fixture.valid)
        .expect("canonical composite activation evidence must pass");

    for invalid in fixture.invalid_cases {
        let mut value = fixture.valid.clone();
        for change in invalid.changes {
            let target = value.pointer_mut(&change.pointer).unwrap_or_else(|| {
                panic!("{} has an unknown pointer {}", invalid.name, change.pointer)
            });
            *target = change.value;
        }
        assert!(
            validate_project_payload_v1(&fixture.validator, &value).is_err(),
            "composite activation mismatch unexpectedly passed: {}",
            invalid.name
        );
    }
}

#[test]
fn rust_enforces_manifest_generated_page_chunk_and_size_boundaries() {
    let (_, manifest) = fixture_manifest();
    for fixture in manifest.generated_cases {
        let value = materialize(&fixture);
        assert_eq!(
            validate_project_payload_v1(&fixture.validator, &value).is_ok(),
            fixture.expected_valid,
            "{}",
            fixture.name
        );
    }
}

fn materialize(fixture: &GeneratedCase) -> Value {
    match fixture.validator.as_str() {
        "treePage" => {
            let entries: Vec<_> = (0..fixture.entry_count.unwrap_or_default())
                .map(|index| {
                    let name = format!("file-{index:04}.ts");
                    json!({"path": format!("src/{name}"), "name": name, "kind": "file"})
                })
                .collect();
            json!({
                "projectId": "project_generated",
                "parentPath": "src",
                "entries": entries,
                "nextCursor": null
            })
        }
        "saveTranscript" => {
            let text_bytes = fixture.text_bytes.unwrap_or_default();
            let text = "a".repeat(text_bytes);
            let version = sha256_file_version_v1(text.as_bytes());
            json!({
                "begin": {
                    "saveId": "save_generated",
                    "projectId": "project_generated",
                    "path": "src/generated.ts",
                    "expectedVersion": version,
                    "totalBytes": text_bytes,
                    "encoding": "utf8",
                    "chunkBytes": super::super::MAX_TEXT_CHUNK_BYTES
                },
                "chunks": [{"saveId":"save_generated", "seq":1, "text":text}],
                "commit": {
                    "saveId": "save_generated",
                    "expectedVersion": version,
                    "lastSeq": 1,
                    "totalBytes": text_bytes
                },
                "currentVersion": version,
                "outcome": "committed"
            })
        }
        "fileClassification" => json!({
            "byteLength": fixture.byte_length,
            "utf8Valid": fixture.utf8_valid,
            "classification": fixture.classification
        }),
        other => panic!("unknown generated validator: {other}"),
    }
}

fn fixture_manifest() -> (PathBuf, FixtureManifest) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/m5/project-v1");
    let source = fs::read(root.join("manifest.json")).expect("read project fixture manifest");
    let manifest = serde_json::from_slice(&source).expect("decode project fixture manifest");
    (root, manifest)
}

fn fixture_cases(root: &Path, file: &str) -> FixtureCases {
    let source = fs::read(root.join(file)).expect("read project fixture cases");
    serde_json::from_slice(&source).expect("decode project fixture cases")
}
