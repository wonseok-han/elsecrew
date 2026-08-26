use super::*;
use hello_agent_protocol::lifecycle_v1::SystemStatusV1;
use hello_agent_protocol::run_recovery_v1::validate_g5_ready_payload_v1;
use hello_agent_protocol::{decode_jsonl, encode_jsonl, EnvelopeV1, RequestV1};
use rusqlite::Connection;
use std::ffi::OsString;
use std::io::Cursor;

fn database() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("temporary directory");
    let path = root.path().join("hello-agent.sqlite3");
    (root, path)
}

fn request(instance: &str, id: &str, method: &str) -> String {
    request_with_params(instance, id, method, serde_json::json!({}))
}

fn request_with_params(instance: &str, id: &str, method: &str, params: Value) -> String {
    encode_jsonl(&EnvelopeV1::Request(RequestV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "request".to_string(),
        sidecar_instance_id: instance.to_string(),
        request_id: id.to_string(),
        method: method.to_string(),
        params,
    }))
    .expect("encode request")
}

#[test]
fn project_error_response_preserves_ambiguous_save_outcome() {
    let request = RequestV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "request".to_string(),
        sidecar_instance_id: "sidecar_unknown_save".to_string(),
        request_id: "req_unknown_save".to_string(),
        method: "file.save".to_string(),
        params: serde_json::json!({}),
    };
    let response = project_error_response(
        &request,
        &ProjectFailure::ambiguous("rename completed before durable confirmation"),
    );
    assert!(!response.ok);
    assert_eq!(
        response.error.unwrap().outcome_unknown,
        Some(true),
        "library response must not downgrade an ambiguous replacement"
    );
}

#[test]
fn invalid_g4_params_return_structured_error_without_killing_the_session() {
    let (_root, path) = database();
    let instance = "sidecar_invalid_g4_params";
    let input = format!(
        "{}{}{}",
        request(instance, "req_invalid", "project.open"),
        request_with_params(
            instance,
            "req_invalid_save",
            "file.save",
            serde_json::json!({"saveId":"save_01","seq":"one","text":"hello"}),
        ),
        request(instance, "req_quit_after_invalid", "app.prepareQuit")
    );
    let mut output = Vec::new();
    serve_stdio_with_identity(&path, Cursor::new(input), &mut output, instance.to_string())
        .expect("invalid request is isolated to one response");
    let envelopes = output_envelopes(&output);
    for envelope in &envelopes[1..=2] {
        let EnvelopeV1::Response(invalid) = envelope else {
            panic!("invalid response")
        };
        assert!(!invalid.ok);
        assert_eq!(invalid.error.as_ref().unwrap().code, "invalidParams");
    }
    let EnvelopeV1::Response(quit) = &envelopes[3] else {
        panic!("quit response")
    };
    assert!(quit.ok);
}

fn output_envelopes(output: &[u8]) -> Vec<EnvelopeV1> {
    output
        .split_inclusive(|byte| *byte == b'\n')
        .map(|line| decode_jsonl(line).expect("stdout must be protocol-only JSONL"))
        .collect()
}

#[test]
fn ready_status_and_idempotent_prepare_quit_mark_clean_before_safe_response() {
    let (_root, path) = database();
    let instance = "sidecar_transport";
    let input = format!(
        "{}{}{}",
        request(instance, "req_status", "system.status"),
        request(instance, "req_quit_1", "app.prepareQuit"),
        request(instance, "req_quit_2", "app.prepareQuit")
    );
    let mut output = Vec::new();
    serve_stdio_with_identity(&path, Cursor::new(input), &mut output, instance.to_string())
        .expect("stdio session");
    let envelopes = output_envelopes(&output);
    assert_eq!(envelopes.len(), 4);

    let EnvelopeV1::Response(status_response) = &envelopes[1] else {
        panic!("status response")
    };
    let status: SystemStatusV1 =
        serde_json::from_value(status_response.result.clone().expect("status result"))
            .expect("decode status");
    let SystemStatusV1::Ready { sidecar, .. } = &status else {
        panic!("ready status")
    };
    validate_g5_ready_payload_v1(&serde_json::to_value(sidecar).unwrap()).expect("valid status");
    for envelope in &envelopes[2..] {
        let EnvelopeV1::Response(response) = envelope else {
            panic!("quit response")
        };
        assert!(response.ok);
        assert_eq!(response.result.as_ref().unwrap()["safeToQuit"], true);
    }

    let connection = Connection::open(&path).expect("inspect database");
    let clean: Option<String> = connection
        .query_row(
            "SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id=?1;",
            [instance],
            |row| row.get(0),
        )
        .expect("clean marker");
    let clean_operations: u32 = connection
        .query_row(
            "SELECT count(*) FROM events WHERE operation_id=?1;",
            [format!("op_clean_{instance}")],
            |row| row.get(0),
        )
        .expect("clean event count");
    assert!(clean.is_some());
    assert_eq!(clean_operations, 1);
}

#[test]
fn stdin_eof_without_prepare_quit_never_marks_clean() {
    let (_root, path) = database();
    let instance = "sidecar_unclean_eof";
    let mut output = Vec::new();
    let error = serve_stdio_with_identity(
        &path,
        Cursor::new(Vec::<u8>::new()),
        &mut output,
        instance.to_string(),
    )
    .expect_err("unexpected EOF");
    assert_eq!(error.kind(), "unexpectedEof");
    assert_eq!(output_envelopes(&output).len(), 1);

    let connection = Connection::open(&path).expect("inspect database");
    let clean: Option<String> = connection
        .query_row(
            "SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id=?1;",
            [instance],
            |row| row.get(0),
        )
        .expect("clean marker");
    assert_eq!(clean, None);
}

#[test]
fn malformed_oversize_and_partial_jsonl_fail_closed_after_ready() {
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (b"not-json\n".to_vec(), "protocolViolation"),
        (
            {
                let mut bytes = vec![b'a'; MAX_CONTROL_LINE_BYTES + 1];
                bytes.push(b'\n');
                bytes
            },
            "controlLineTooLarge",
        ),
        (b"{".to_vec(), "partialJsonl"),
    ];
    for (index, (input, expected_kind)) in cases.into_iter().enumerate() {
        let (_root, path) = database();
        let mut output = Vec::new();
        let error = serve_stdio_with_identity(
            &path,
            Cursor::new(input),
            &mut output,
            format!("sidecar_invalid_{index}"),
        )
        .expect_err("invalid transport must fail");
        assert_eq!(error.kind(), expected_kind);
        assert_eq!(output_envelopes(&output).len(), 1);
    }
}

#[test]
fn startup_failure_emits_no_ready_or_other_stdout() {
    let root = tempfile::tempdir().unwrap();
    let database_path = root.path().join("hello-agent.sqlite3");
    std::fs::create_dir(&database_path).unwrap();
    let mut output = Vec::new();
    let error = serve_stdio_with_identity(
        &database_path,
        Cursor::new(Vec::<u8>::new()),
        &mut output,
        "sidecar_startup_failure".to_string(),
    )
    .expect_err("startup must fail");
    assert_eq!(error.kind(), "databasePathInvalid");
    assert!(output.is_empty());
}

#[test]
fn later_wave_method_is_structured_error_and_read_only_status_remains_available_after_quit() {
    let (_root, path) = database();
    let instance = "sidecar_capabilities";
    let input = format!(
        "{}{}{}",
        request(instance, "req_later", "run.startAgentSmoke"),
        request(instance, "req_quit", "app.prepareQuit"),
        request(instance, "req_status_after", "system.status")
    );
    let mut output = Vec::new();
    serve_stdio_with_identity(&path, Cursor::new(input), &mut output, instance.to_string())
        .expect("stdio session");
    let envelopes = output_envelopes(&output);
    let EnvelopeV1::Response(unavailable) = &envelopes[1] else {
        panic!("unavailable response")
    };
    assert!(!unavailable.ok);
    assert_eq!(
        unavailable.error.as_ref().unwrap().code,
        "methodUnavailable"
    );
    let EnvelopeV1::Response(status) = &envelopes[3] else {
        panic!("status response")
    };
    assert!(status.ok);
}

#[test]
fn cli_rejects_missing_or_extra_arguments_before_touching_stdio() {
    for args in [
        vec![OsString::from("sidecar")],
        vec![
            OsString::from("sidecar"),
            OsString::from("--stdio"),
            OsString::from("database.sqlite3"),
        ],
        vec![OsString::from("sidecar"), OsString::from("--other")],
    ] {
        let error = run_cli(args).expect_err("invalid argv");
        assert_eq!(error.kind(), "invalidArguments");
    }
}
