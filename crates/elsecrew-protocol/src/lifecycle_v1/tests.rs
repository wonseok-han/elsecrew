use super::*;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    channels: Channels,
    capabilities: Vec<String>,
    data_directory: DataDirectory,
    production_sidecar: ProductionSidecar,
    system_status_cases: Vec<StatusCase>,
    startup_cases: Vec<StartupCase>,
    prepare_quit_cases: Vec<PrepareQuitCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Channels {
    system_get_status: String,
    app_prepare_quit: String,
    sidecar_event: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataDirectory {
    base: String,
    segments: Vec<String>,
    sqlite_file: String,
    separate_from_legacy_tauri_store: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProductionSidecar {
    base: String,
    relative_segments: Vec<String>,
    windows_suffix: String,
    argv: Vec<String>,
    shell: bool,
    allow_environment_override: bool,
    stdout: String,
    stderr: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusCase {
    name: String,
    expected_valid: bool,
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartupCase {
    name: String,
    expected_valid: bool,
    value: StartupTraceV1,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareQuitCase {
    name: String,
    expected_valid: bool,
    value: PrepareQuitTraceV1,
}

#[test]
fn rust_matches_fixed_channels_paths_and_g3_capabilities() {
    let manifest = manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.channels.system_get_status,
        CHANNEL_SYSTEM_GET_STATUS_V1
    );
    assert_eq!(
        manifest.channels.app_prepare_quit,
        CHANNEL_APP_PREPARE_QUIT_V1
    );
    assert_eq!(manifest.channels.sidecar_event, CHANNEL_SIDECAR_EVENT_V1);
    assert!(G3_CAPABILITIES_V1
        .iter()
        .copied()
        .eq(manifest.capabilities.iter().map(String::as_str)));

    assert_eq!(manifest.data_directory.base, "userData");
    assert!(DATA_DIRECTORY_SEGMENTS_V1.iter().copied().eq(manifest
        .data_directory
        .segments
        .iter()
        .map(String::as_str)));
    assert_eq!(manifest.data_directory.sqlite_file, SQLITE_FILE_V1);
    assert!(manifest.data_directory.separate_from_legacy_tauri_store);

    assert_eq!(manifest.production_sidecar.base, "process.resourcesPath");
    assert!(PRODUCTION_SIDECAR_SEGMENTS_V1.iter().copied().eq(manifest
        .production_sidecar
        .relative_segments
        .iter()
        .map(String::as_str)));
    assert_eq!(
        manifest.production_sidecar.windows_suffix,
        PRODUCTION_SIDECAR_WINDOWS_SUFFIX_V1
    );
    assert!(PRODUCTION_SIDECAR_ARGV_V1.iter().copied().eq(manifest
        .production_sidecar
        .argv
        .iter()
        .map(String::as_str)));
    assert!(!manifest.production_sidecar.shell);
    assert!(!manifest.production_sidecar.allow_environment_override);
    assert_eq!(manifest.production_sidecar.stdout, "pipe");
    assert_eq!(manifest.production_sidecar.stderr, "pipe");
}

#[test]
fn rust_accepts_and_rejects_canonical_system_status_cases() {
    for case in manifest().system_status_cases {
        let result = serde_json::from_value::<SystemStatusV1>(case.value)
            .map_err(|error| LifecycleContractError::new(error.to_string()))
            .and_then(|status| validate_system_status_v1(&status));
        assert_eq!(result.is_ok(), case.expected_valid, "{}", case.name);
    }
}

#[test]
fn rust_enforces_startup_and_main_owned_prepare_quit_traces() {
    let manifest = manifest();
    for case in manifest.startup_cases {
        assert_eq!(
            validate_startup_trace_v1(&case.value).is_ok(),
            case.expected_valid,
            "{}",
            case.name
        );
    }
    for case in manifest.prepare_quit_cases {
        assert_eq!(
            validate_prepare_quit_trace_v1(&case.value).is_ok(),
            case.expected_valid,
            "{}",
            case.name
        );
    }
}

fn manifest() -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/m5/lifecycle-v1/manifest.json");
    let source = fs::read(path).expect("read lifecycle fixture");
    serde_json::from_slice(&source).expect("decode lifecycle fixture")
}
