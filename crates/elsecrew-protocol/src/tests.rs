use super::*;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureManifest {
    schema_version: u32,
    contract: ContractFixture,
    valid: Vec<ValidCase>,
    invalid: Vec<InvalidCase>,
    generated: Vec<GeneratedCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractFixture {
    protocol_version: u32,
    sidecar_schema_version: u32,
    app_schema_version: u32,
    max_control_line_bytes: usize,
    max_text_chunk_bytes: usize,
    max_sequence: u64,
    protocol_stream_ids_per_session: usize,
    methods: Vec<String>,
    events: Vec<String>,
    bridge_methods: Vec<String>,
    workspace_event_addressing: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidCase {
    file: String,
    observe_session: Option<bool>,
    ready_envelope: Option<usize>,
    prepare_quit_envelope: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvalidCase {
    file: String,
    validator: String,
    envelope: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedCase {
    name: String,
    size_kind: String,
    target_bytes: usize,
    repeat_path: Vec<String>,
    expected_valid: bool,
    template: Value,
}

#[test]
fn rust_decodes_and_byte_matches_canonical_protocol_v1_transcripts() {
    let (root, manifest) = manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.contract.protocol_version, PROTOCOL_VERSION);
    assert_eq!(
        manifest.contract.sidecar_schema_version,
        SIDECAR_SCHEMA_VERSION
    );
    assert_eq!(manifest.contract.app_schema_version, APP_SCHEMA_VERSION);
    assert_eq!(
        manifest.contract.max_control_line_bytes,
        MAX_CONTROL_LINE_BYTES
    );
    assert_eq!(manifest.contract.max_text_chunk_bytes, MAX_TEXT_CHUNK_BYTES);
    assert_eq!(manifest.contract.max_sequence, MAX_SEQUENCE);
    assert_eq!(
        manifest.contract.protocol_stream_ids_per_session,
        PROTOCOL_STREAM_IDS_PER_SESSION_MAX_V1
    );
    assert!(METHODS_V1
        .iter()
        .copied()
        .eq(manifest.contract.methods.iter().map(String::as_str)));
    assert!(EVENTS_V1
        .iter()
        .copied()
        .eq(manifest.contract.events.iter().map(String::as_str)));
    assert_eq!(
        manifest.contract.bridge_methods,
        ["getSystemStatus", "prepareQuit", "onSidecarEvent"]
    );
    assert_eq!(
        manifest.contract.workspace_event_addressing,
        "payload.chatId+projectId+worktreeId+workspaceIdentity"
    );

    for fixture in manifest.valid {
        let source = fs::read(root.join(&fixture.file)).expect("read valid fixture");
        let lines = jsonl_lines(&source);
        let mut session = ProtocolSessionV1::default();
        let mut envelopes = Vec::new();
        for line in lines {
            let envelope = decode_jsonl(line).expect(&fixture.file);
            if fixture.observe_session != Some(false) {
                session.observe(&envelope).expect(&fixture.file);
            }
            assert_eq!(encode_jsonl(&envelope).expect("encode"), as_utf8(line));
            envelopes.push(envelope);
        }

        if let Some(index) = fixture.ready_envelope {
            let EnvelopeV1::Event(event) = &envelopes[index] else {
                panic!("ready fixture is not an event")
            };
            ready_payload(event).expect("ready payload");
        }
        if let Some(index) = fixture.prepare_quit_envelope {
            let EnvelopeV1::Response(response) = &envelopes[index] else {
                panic!("prepareQuit fixture is not a response")
            };
            assert!(
                prepare_quit_result(response)
                    .expect("prepareQuit result")
                    .safe_to_quit
            );
        }
    }
}

#[test]
fn rust_rejects_every_invalid_golden_fixture_fail_closed() {
    let (root, manifest) = manifest();
    for fixture in manifest.invalid {
        let result = validate_invalid_fixture(&root, &fixture);
        assert!(
            result.is_err(),
            "fixture unexpectedly passed: {}",
            fixture.file
        );
    }
}

#[test]
fn rust_enforces_generated_control_and_text_boundaries() {
    let (_, manifest) = manifest();
    for fixture in manifest.generated {
        let line = materialize_boundary(&fixture);
        let result = decode_jsonl(line.as_bytes());
        assert_eq!(
            result.is_ok(),
            fixture.expected_valid,
            "boundary fixture: {}",
            fixture.name
        );
    }
}

#[test]
fn rust_requires_one_exclusive_stream_and_unique_event_ids_per_actual_run() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/m5/protocol-v1/valid/agent-run-state-changed.jsonl");
    let source = fs::read(root).expect("read state event fixture");
    let envelopes: Vec<_> = jsonl_lines(&source)
        .into_iter()
        .map(|line| decode_jsonl(line).expect("decode fixture"))
        .collect();
    let mut valid = ProtocolSessionV1::default();
    for envelope in &envelopes {
        valid.observe(envelope).expect("valid shared stream");
    }

    let mut mismatched = ProtocolSessionV1::default();
    mismatched.observe(&envelopes[0]).expect("ready");
    mismatched.observe(&envelopes[1]).expect("public event");
    let EnvelopeV1::Event(state) = &envelopes[2] else {
        panic!("state fixture must be an event")
    };
    let mut wrong_stream = state.clone();
    wrong_stream.stream_id = "stream_agent_run_other".to_string();
    wrong_stream.seq = 1;
    assert!(mismatched
        .observe(&EnvelopeV1::Event(wrong_stream))
        .is_err());

    let mut replay = state.clone();
    replay.seq = 3;
    assert!(valid.observe(&EnvelopeV1::Event(replay)).is_err());
}

fn validate_invalid_fixture(root: &Path, fixture: &InvalidCase) -> Result<(), ProtocolError> {
    let source = fs::read(root.join(&fixture.file))
        .map_err(|error| ProtocolError::new(format!("could not read fixture: {error}")))?;
    let mut session = ProtocolSessionV1::default();
    let mut envelopes = Vec::new();
    for line in jsonl_lines(&source) {
        let envelope = decode_jsonl(line)?;
        session.observe(&envelope)?;
        envelopes.push(envelope);
    }
    if fixture.validator == "prepareQuit" {
        let index = fixture
            .envelope
            .ok_or_else(|| ProtocolError::new("fixture envelope index is required"))?;
        let response = match envelopes.get(index) {
            Some(EnvelopeV1::Response(response)) => response,
            _ => return Err(ProtocolError::new("fixture is not a response")),
        };
        prepare_quit_result(response)?;
    }
    Ok(())
}

fn manifest() -> (PathBuf, FixtureManifest) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/m5/protocol-v1");
    let source = fs::read(root.join("manifest.json")).expect("read fixture manifest");
    let manifest = serde_json::from_slice(&source).expect("decode fixture manifest");
    (root, manifest)
}

fn jsonl_lines(source: &[u8]) -> Vec<&[u8]> {
    assert_eq!(source.last(), Some(&b'\n'), "fixture must end in newline");
    source
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect()
}

fn materialize_boundary(fixture: &GeneratedCase) -> String {
    let mut value = fixture.template.clone();
    if fixture.size_kind == "textChunk" {
        set_path(
            &mut value,
            &fixture.repeat_path,
            "a".repeat(fixture.target_bytes),
        );
    } else {
        set_path(&mut value, &fixture.repeat_path, String::new());
        let empty = serde_json::to_string(&value).expect("encode boundary template");
        let padding_bytes = fixture
            .target_bytes
            .checked_sub(empty.len())
            .expect("control fixture target is smaller than its template");
        set_path(&mut value, &fixture.repeat_path, "a".repeat(padding_bytes));
    }
    let line = format!(
        "{}\n",
        serde_json::to_string(&value).expect("encode boundary fixture")
    );
    if fixture.size_kind == "controlLine" {
        assert_eq!(line.len() - 1, fixture.target_bytes, "{}", fixture.name);
    }
    line
}

fn set_path(root: &mut Value, path: &[String], value: String) {
    let mut target = root;
    for segment in &path[..path.len() - 1] {
        target = target
            .get_mut(segment)
            .unwrap_or_else(|| panic!("invalid repeatPath at {segment}"));
    }
    let last = path.last().expect("repeatPath cannot be empty");
    target[last] = Value::String(value);
}

fn as_utf8(line: &[u8]) -> String {
    String::from_utf8(line.to_vec()).expect("fixture is UTF-8")
}
