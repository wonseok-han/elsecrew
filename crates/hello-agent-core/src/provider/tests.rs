use super::*;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

#[test]
fn official_prerelease_versions_are_preserved_without_relaxing_version_syntax() {
    for version in [
        "0.162.0-alpha.2",
        "1.2.3-rc.1+build.001",
        "1.2.3+001",
        "1.2.3",
    ] {
        let observation =
            BoundedObservation::new(format!("codex-cli {version}\n").into_bytes()).unwrap();
        assert_eq!(
            parse_cli_version(ProviderId::Codex, &observation).unwrap(),
            version
        );
    }
    for version in [
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "1.2",
        "1.2.3-",
        "1.2.3+",
        "1.2.3-alpha..2",
        "1.2.3-01",
        "1.2.3+one+two",
        "1.2.3-alpha_2",
        "1.2.3-α",
        "1.2.3\nextra",
    ] {
        let observation =
            BoundedObservation::new(format!("codex-cli {version}").into_bytes()).unwrap();
        assert!(
            parse_cli_version(ProviderId::Codex, &observation).is_err(),
            "{version}"
        );
    }
    assert!(validate_semver(&format!("1.2.3-{}", "a".repeat(128))).is_err());
}

const D1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const D2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const D3: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const D4: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const D5: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const D6: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";
const D7: &str = "sha256:7777777777777777777777777777777777777777777777777777777777777777";
const D8: &str = "sha256:8888888888888888888888888888888888888888888888888888888888888888";

fn identity(name: &str, path: &str, file: &str) -> ProviderExecutableFileIdentityV1 {
    ProviderExecutableFileIdentityV1 {
        logical_name: name.into(),
        path_digest: path.into(),
        file_identity_digest: file.into(),
    }
}

fn executable(provider: ProviderId) -> ObservedExecutableClosureV1 {
    match provider {
        ProviderId::ClaudeCode => ObservedExecutableClosureV1 {
            binary: identity("claude", D1, D2),
            required_helpers: vec![identity("node", D3, D4)],
        },
        ProviderId::Codex => ObservedExecutableClosureV1 {
            binary: identity("codex", D5, D6),
            required_helpers: vec![identity("rg", D7, D8)],
        },
    }
}

fn observation(text: impl AsRef<[u8]>) -> BoundedObservation {
    BoundedObservation::new(text.as_ref().to_vec()).expect("bounded observation")
}

fn discovery_input(provider: ProviderId) -> DiscoveryInputV1 {
    let definition = ProviderDefinitionV1::new(provider, executable(provider)).unwrap();
    DiscoveryInputV1 {
        executable: Some(executable(provider)),
        version: Some(observation(match provider {
            ProviderId::ClaudeCode => "Claude Code 2.1.241",
            ProviderId::Codex => "codex-cli 0.150.0",
        })),
        auth: Some(observation(match provider {
            ProviderId::ClaudeCode => {
                r#"{"loggedIn":true,"email":"person@example.com","organization":"secret-org","token":"secret-token"}"#
            }
            ProviderId::Codex => "Logged in using ChatGPT",
        })),
        model_catalog_evidence: match provider {
            ProviderId::ClaudeCode => ModelCatalogEvidenceV1::RuntimeVerified,
            ProviderId::Codex => ModelCatalogEvidenceV1::ArgvOnly,
        },
        exact_profiles: definition.manifest().exact_profiles.clone(),
        observed_runtime_verification: definition
            .manifest()
            .launch_policy
            .required_runtime_verification
            .clone(),
        observed_at: "2026-08-27T08:00:00Z".into(),
    }
}

#[test]
fn exact_manifests_match_the_cross_language_golden() {
    let claude =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .expect("Claude manifest");
    assert_eq!(
        claude.manifest().executable_closure.closure_digest,
        "sha256:3d2f1e123e5bcae5be55245df865f6ce6d21388b365fa078a7de85faa64efba0"
    );
    assert_eq!(
        claude.manifest().manifest_digest,
        "sha256:8df06be9cf9e1774021a714595e6ae964f0a26d1fbf04cdbb775572b726515e2"
    );
    let codex = ProviderDefinitionV1::new(ProviderId::Codex, executable(ProviderId::Codex))
        .expect("Codex manifest");
    assert_eq!(
        codex.manifest().executable_closure.closure_digest,
        "sha256:cf7e57d48dc720b24ab2d7f786c2612e26a45e2833df4a5bdafae666aa214af6"
    );
    assert_eq!(
        codex.manifest().manifest_digest,
        "sha256:c8043701e36e7b815f401e527afe3ba348565a644f75362c2fa10ae8d84e9c8c"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn manifest_rejects_missing_helper_and_identity_drift() {
    let mut missing = executable(ProviderId::ClaudeCode);
    missing.required_helpers.clear();
    assert!(ProviderDefinitionV1::new(ProviderId::ClaudeCode, missing).is_err());

    let mut drift = executable(ProviderId::ClaudeCode);
    drift.required_helpers[0].logical_name = "rg".into();
    assert!(ProviderDefinitionV1::new(ProviderId::ClaudeCode, drift).is_err());
}

#[test]
fn discovery_is_conservative_for_missing_malformed_and_unknown_versions() {
    let definition =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();

    let mut missing_helper = discovery_input(ProviderId::ClaudeCode);
    missing_helper
        .executable
        .as_mut()
        .unwrap()
        .required_helpers
        .clear();
    let result = definition.discover(missing_helper).unwrap();
    assert_eq!(result.installation_state, "incomplete");
    assert_eq!(result.auth_state, "unknown");
    assert_eq!(result.blocked_reason.as_deref(), Some("closureMismatch"));
    assert!(result.executable_closure.is_none());

    let mut malformed = discovery_input(ProviderId::ClaudeCode);
    malformed.version = Some(observation("Claude Code latest"));
    assert!(definition.discover(malformed).is_err());

    let mut unknown = discovery_input(ProviderId::ClaudeCode);
    unknown.version = Some(observation("Claude Code 2.1.242"));
    let result = definition.discover(unknown).unwrap();
    assert_eq!(result.installation_state, "installed");
    assert_eq!(result.blocked_reason.as_deref(), Some("closureMismatch"));
    assert!(!result.can_start);
}

#[test]
fn observations_enforce_line_and_total_limits_without_debug_leaks() {
    assert!(BoundedObservation::new(vec![b'x'; PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1]).is_err());
    let mut total = Vec::new();
    while total.len() <= PROVIDER_OBSERVATION_TOTAL_MAX_BYTES {
        total.extend(std::iter::repeat_n(
            b'x',
            PROVIDER_OBSERVATION_LINE_MAX_BYTES,
        ));
        total.push(b'\n');
    }
    assert!(BoundedObservation::new(total).is_err());
    let secret = observation("token prompt person@example.com secret-org");
    let debug = format!("{secret:?}");
    assert!(!debug.contains("token"));
    assert!(!debug.contains("example.com"));
}

#[test]
fn auth_variants_are_sanitized_and_never_retain_account_fields() {
    let definition =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();
    let authenticated = definition
        .discover(discovery_input(ProviderId::ClaudeCode))
        .unwrap();
    assert_eq!(authenticated.auth_state, "authenticated");
    let serialized = serde_json::to_string(&authenticated).unwrap();
    for secret in [
        "person@example.com",
        "secret-org",
        "secret-token",
        "email",
        "organization",
        "token",
    ] {
        assert!(!serialized.contains(secret));
    }

    let mut expired = discovery_input(ProviderId::ClaudeCode);
    expired.auth = Some(observation(
        r#"{"loggedIn":true,"expired":true,"email":"private@example.com"}"#,
    ));
    let expired = definition.discover(expired).unwrap();
    assert_eq!(expired.auth_state, "expired");
    assert_eq!(expired.blocked_reason.as_deref(), Some("authExpired"));

    let codex =
        ProviderDefinitionV1::new(ProviderId::Codex, executable(ProviderId::Codex)).unwrap();
    for (raw, expected) in [
        ("Logged in using an API key", "authenticated"),
        ("Not logged in", "notAuthenticated"),
        ("Authentication expired", "expired"),
        ("Account: private@example.com", "unknown"),
    ] {
        let mut input = discovery_input(ProviderId::Codex);
        input.auth = Some(observation(raw));
        assert_eq!(codex.discover(input).unwrap().auth_state, expected);
    }
}

#[test]
fn exact_tuple_mismatch_and_codex_catalog_evidence_never_start() {
    let claude =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();
    let mut mismatch = discovery_input(ProviderId::ClaudeCode);
    mismatch.exact_profiles[0].model_selector = "claude-unknown".into();
    let result = claude.discover(mismatch).unwrap();
    assert_eq!(result.blocked_reason.as_deref(), Some("unsupportedProfile"));
    assert!(result.exact_profiles.is_empty());
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("claude-unknown"));

    let codex =
        ProviderDefinitionV1::new(ProviderId::Codex, executable(ProviderId::Codex)).unwrap();
    let argv_only = codex.discover(discovery_input(ProviderId::Codex)).unwrap();
    assert_eq!(
        argv_only.blocked_reason.as_deref(),
        Some("modelCatalogUnverified")
    );
    let mut unknown = discovery_input(ProviderId::Codex);
    unknown.model_catalog_evidence = ModelCatalogEvidenceV1::Unknown;
    unknown.exact_profiles.clear();
    unknown
        .observed_runtime_verification
        .retain(|entry| entry != "model");
    let unknown = codex.discover(unknown).unwrap();
    assert_eq!(
        unknown.blocked_reason.as_deref(),
        Some("modelCatalogUnverified")
    );
    assert!(!unknown.can_start);

    let mut effective_unknown = discovery_input(ProviderId::Codex);
    effective_unknown.model_catalog_evidence = ModelCatalogEvidenceV1::ExactVersionManifest;
    effective_unknown.exact_profiles.clear();
    let effective_unknown = codex.discover(effective_unknown).unwrap();
    assert_eq!(
        effective_unknown.blocked_reason.as_deref(),
        Some("unsupportedProfile")
    );
}

fn managed_cwd() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "hello-agent-g5c-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&root).expect("managed cwd");
    root
}

fn launch(cwd: PathBuf) -> LaunchSpec {
    LaunchSpec::new_internal(
        PathBuf::from(test_path!("/bin/echo")),
        vec![OsString::from("--fixture")],
        cwd,
        BTreeMap::new(),
        b"secret prompt".to_vec(),
        CLAUDE_HANDSHAKE,
        CLAUDE_EVENT_PARSER,
    )
    .expect("launch")
}

fn handshake_json(cwd: &str) -> Value {
    serde_json::json!({
        "type": "system",
        "subtype": "init",
        "claude_code_version": "2.1.241",
        "model": "claude-opus-5",
        "cwd": cwd,
        "permissionMode": "workspaceWrite",
        "tools": ["Edit", "Read"],
        "mcp_servers": [],
        "plugins": [],
        "configIsolation": "managedOnly"
    })
}

fn handshake_context<'a>(launch_spec: &'a LaunchSpec) -> ClaudeHandshakeContextV1<'a> {
    ClaudeHandshakeContextV1 {
        launch_spec,
        run_id: "agent_run_g5c_claude_01".into(),
        capability_fingerprint: format!("sha256:{}", "a".repeat(64)),
        effective_profile: ProviderProfileTupleV1 {
            model_selector: "claude-opus-5".into(),
            reasoning_effort: None,
            permission: "workspaceWrite".into(),
        },
        workspace_identity: "workspace_01".into(),
        managed_cwd_identity: format!("sha256:{}", "b".repeat(64)),
        expected_tools: vec!["Read".into(), "Edit".into()],
        expected_mcp_servers: Vec::new(),
        expected_plugins: Vec::new(),
        verified_at: "2026-08-27T08:00:01Z".into(),
    }
}

#[test]
fn claude_handshake_binds_every_runtime_field_before_attestation() {
    let definition =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();
    let discovery = definition
        .discover(discovery_input(ProviderId::ClaudeCode))
        .unwrap();
    assert!(discovery.can_start);
    let cwd = managed_cwd();
    let spec = launch(cwd.clone());
    let wire = handshake_json(cwd.to_str().unwrap());
    let mut classifier = definition.event_classifier();
    let (attestation, event) = classifier
        .accept_claude_handshake(
            &discovery,
            observation(serde_json::to_vec(&wire).unwrap()),
            handshake_context(&spec),
        )
        .unwrap();
    assert_eq!(
        attestation.manifest_digest,
        definition.manifest().manifest_digest
    );
    assert_eq!(
        attestation.verified_runtime_evidence,
        strings(CLAUDE_VERIFICATION)
    );
    assert!(matches!(
        event,
        InternalProviderEventV1::Handshake { sequence: 1, .. }
    ));
    let _ = fs::remove_dir(cwd);
}

#[test]
fn every_claude_handshake_dimension_fails_closed_on_mismatch() {
    let definition =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();
    let discovery = definition
        .discover(discovery_input(ProviderId::ClaudeCode))
        .unwrap();
    let cwd = managed_cwd();
    let spec = launch(cwd.clone());
    let base = handshake_json(cwd.to_str().unwrap());
    for (field, value) in [
        ("claude_code_version", serde_json::json!("2.1.242")),
        ("model", serde_json::json!("claude-unknown")),
        ("cwd", serde_json::json!(test_path!("/tmp/not-managed"))),
        ("permissionMode", serde_json::json!("readOnly")),
        ("tools", serde_json::json!(["Read"])),
        ("mcp_servers", serde_json::json!(["unsafe"])),
        ("plugins", serde_json::json!(["unsafe"])),
        ("configIsolation", serde_json::json!("userAndProject")),
    ] {
        let mut mutated = base.clone();
        mutated.as_object_mut().unwrap().insert(field.into(), value);
        let mut classifier = definition.event_classifier();
        assert!(
            classifier
                .accept_claude_handshake(
                    &discovery,
                    observation(serde_json::to_vec(&mutated).unwrap()),
                    handshake_context(&spec),
                )
                .is_err(),
            "{field} mismatch must fail"
        );
    }
    let mut duplicate_policy = handshake_context(&spec);
    duplicate_policy.expected_tools = vec!["Read".into(), "Read".into()];
    let mut classifier = definition.event_classifier();
    assert!(classifier
        .accept_claude_handshake(
            &discovery,
            observation(serde_json::to_vec(&base).unwrap()),
            duplicate_policy,
        )
        .is_err());
    let _ = fs::remove_dir(cwd);
}

#[test]
fn forged_startable_discovery_cannot_reach_attestation() {
    let definition =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();
    let discovery = definition
        .discover(discovery_input(ProviderId::ClaudeCode))
        .unwrap();
    let cwd = managed_cwd();
    let spec = launch(cwd.clone());
    let wire = handshake_json(cwd.to_str().unwrap());

    let mutations: [fn(&mut SanitizedDiscoveryObservationV1); 3] = [
        |value: &mut SanitizedDiscoveryObservationV1| value.auth_state = "unknown".into(),
        |value: &mut SanitizedDiscoveryObservationV1| {
            value.model_catalog_evidence = "argvOnly".into()
        },
        |value: &mut SanitizedDiscoveryObservationV1| {
            value
                .executable_closure
                .as_mut()
                .unwrap()
                .binary
                .path_digest = D8.into()
        },
    ];
    for mutate in mutations {
        let mut forged = discovery.clone();
        mutate(&mut forged);
        let mut classifier = definition.event_classifier();
        assert!(classifier
            .accept_claude_handshake(
                &forged,
                observation(serde_json::to_vec(&wire).unwrap()),
                handshake_context(&spec),
            )
            .is_err());
    }
    let _ = fs::remove_dir(cwd);
}

fn ready_classifier() -> (ProviderEventClassifierV1, PathBuf) {
    let definition =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();
    let discovery = definition
        .discover(discovery_input(ProviderId::ClaudeCode))
        .unwrap();
    let cwd = managed_cwd();
    let spec = launch(cwd.clone());
    let wire = handshake_json(cwd.to_str().unwrap());
    let mut classifier = definition.event_classifier();
    classifier
        .accept_claude_handshake(
            &discovery,
            observation(serde_json::to_vec(&wire).unwrap()),
            handshake_context(&spec),
        )
        .unwrap();
    (classifier, cwd)
}

#[test]
fn event_classifier_requires_handshake_and_only_releases_public_kinds() {
    let definition =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();
    let mut early = definition.event_classifier();
    assert!(early
        .classify(observation(r#"{"type":"status","text":"too early"}"#))
        .is_err());

    let (mut classifier, cwd) = ready_classifier();
    let public = classifier
        .classify(observation(r#"{"type":"status","text":"working"}"#))
        .unwrap();
    assert!(matches!(
        public,
        InternalProviderEventV1::Public { sequence: 2, .. }
    ));
    let private = classifier
        .classify(observation(
            r#"{"type":"reasoning","token":"secret","prompt":"private"}"#,
        ))
        .unwrap();
    assert_eq!(
        private,
        InternalProviderEventV1::Private {
            schema_version: 1,
            sequence: 3,
            private_kind: "reasoning".into()
        }
    );
    let serialized = serde_json::to_string(&private).unwrap();
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("prompt"));
    assert!(classifier
        .classify(observation(
            r#"{"type":"status","text":"ok","rawStdout":"private"}"#
        ))
        .is_err());
    assert!(classifier
        .classify(observation(r#"{"type":"thinking","text":"private"}"#))
        .is_err());
    let _ = fs::remove_dir(cwd);
}

#[test]
fn event_classifier_rejects_malformed_oversize_and_post_terminal_input() {
    let (mut classifier, cwd) = ready_classifier();
    assert!(classifier.classify(observation("not-json")).is_err());
    let oversized = "x".repeat(PROVIDER_PUBLIC_TEXT_MAX_BYTES + 1);
    let line =
        serde_json::to_vec(&serde_json::json!({"type":"status", "text": oversized})).unwrap();
    assert!(classifier.classify(observation(line)).is_err());
    let terminal = classifier
        .classify(observation(r#"{"type":"terminal","outcome":"succeeded"}"#))
        .unwrap();
    assert!(matches!(
        terminal,
        InternalProviderEventV1::Terminal { sequence: 2, .. }
    ));
    assert!(classifier
        .classify(observation(r#"{"type":"final","text":"late"}"#))
        .is_err());
    let _ = fs::remove_dir(cwd);
}

#[test]
fn auth_failure_is_sanitized_and_must_be_the_only_pre_handshake_event() {
    let definition =
        ProviderDefinitionV1::new(ProviderId::ClaudeCode, executable(ProviderId::ClaudeCode))
            .unwrap();
    let mut classifier = definition.event_classifier();
    let event = classifier
        .classify(observation(
            r#"{"type":"authFailure","authState":"expired"}"#,
        ))
        .unwrap();
    assert_eq!(
        event,
        InternalProviderEventV1::AuthFailure {
            schema_version: 1,
            sequence: 1,
            auth_state: "expired".into()
        }
    );
    assert!(classifier
        .classify(observation(r#"{"type":"status","text":"late"}"#))
        .is_err());

    let mut leaking = definition.event_classifier();
    assert!(leaking
        .classify(observation(
            r#"{"type":"authFailure","authState":"expired","token":"secret"}"#,
        ))
        .is_err());
}

#[test]
fn public_event_count_and_utf8_byte_budgets_fail_before_release() {
    let (mut count_classifier, count_cwd) = ready_classifier();
    for _ in 0..PROVIDER_PUBLIC_EVENTS_MAX {
        count_classifier
            .classify(observation(r#"{"type":"status","text":"x"}"#))
            .unwrap();
    }
    assert!(count_classifier
        .classify(observation(r#"{"type":"status","text":"x"}"#))
        .is_err());

    let (mut text_classifier, text_cwd) = ready_classifier();
    let exact_chunk = "가".repeat(PROVIDER_PUBLIC_TEXT_MAX_BYTES / "가".len());
    let exact_chunk_bytes = exact_chunk.len();
    let full_chunks = PROVIDER_PUBLIC_TEXT_TOTAL_MAX_BYTES / exact_chunk_bytes;
    let remainder = PROVIDER_PUBLIC_TEXT_TOTAL_MAX_BYTES % exact_chunk_bytes;
    for _ in 0..full_chunks {
        let line = serde_json::to_vec(&serde_json::json!({"type":"status", "text": &exact_chunk}))
            .unwrap();
        text_classifier.classify(observation(line)).unwrap();
    }
    if remainder > 0 {
        let line = serde_json::to_vec(&serde_json::json!({
            "type": "status",
            "text": "x".repeat(remainder)
        }))
        .unwrap();
        text_classifier.classify(observation(line)).unwrap();
    }
    assert!(text_classifier
        .classify(observation(r#"{"type":"status","text":"x"}"#))
        .is_err());
    let _ = fs::remove_dir(count_cwd);
    let _ = fs::remove_dir(text_cwd);
}

#[test]
fn launch_spec_keeps_prompt_out_of_debug_and_digest_material() {
    let cwd = managed_cwd();
    let spec = launch(cwd.clone());
    let debug = format!("{spec:?}");
    let digest_material = spec.redacted_digest_material();
    let digest = String::from_utf8_lossy(&digest_material);
    assert!(!debug.contains("secret prompt"));
    assert!(!digest.contains("secret prompt"));
    assert_eq!(spec.prompt(), b"secret prompt");
    let _ = fs::remove_dir(cwd);
}
