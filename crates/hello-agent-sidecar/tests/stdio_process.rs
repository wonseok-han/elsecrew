use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use hello_agent_protocol::{decode_jsonl, encode_jsonl, ready_payload, EnvelopeV1, RequestV1};
use rusqlite::Connection;

#[test]
fn binary_startup_failure_uses_stderr_nonzero_and_no_stdout_ready() {
    let root = tempfile::tempdir().expect("temporary directory");
    fs::create_dir(root.path().join("hello-agent.sqlite3")).expect("invalid database directory");
    let output = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .arg("--stdio")
        .current_dir(root.path())
        .stdin(Stdio::null())
        .output()
        .expect("run sidecar");

    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty(), "pre-ready stdout must stay empty");
    assert!(!output.stderr.is_empty(), "startup detail must use stderr");
}

#[test]
fn binary_ready_status_quit_flow_marks_clean_and_exits_zero() {
    let root = tempfile::tempdir().expect("temporary directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .arg("--stdio")
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sidecar");
    let mut stdin = child.stdin.take().expect("stdin pipe");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout pipe"));

    let mut ready_line = String::new();
    stdout.read_line(&mut ready_line).expect("read ready");
    let EnvelopeV1::Event(ready_event) = decode_jsonl(ready_line.as_bytes()).expect("decode ready")
    else {
        panic!("first envelope must be ready")
    };
    let ready = ready_payload(&ready_event).expect("ready payload");

    for (request_id, method) in [
        ("req_process_status", "system.status"),
        ("req_process_quit", "app.prepareQuit"),
    ] {
        let request = EnvelopeV1::Request(RequestV1 {
            protocol_version: 1,
            kind: "request".to_string(),
            sidecar_instance_id: ready.sidecar_instance_id.clone(),
            request_id: request_id.to_string(),
            method: method.to_string(),
            params: serde_json::json!({}),
        });
        stdin
            .write_all(encode_jsonl(&request).unwrap().as_bytes())
            .expect("write request");
        stdin.flush().expect("flush request");
        let mut response_line = String::new();
        stdout.read_line(&mut response_line).expect("read response");
        let EnvelopeV1::Response(response) =
            decode_jsonl(response_line.as_bytes()).expect("decode response")
        else {
            panic!("request must receive response")
        };
        assert!(response.ok, "{method}");
    }
    drop(stdin);
    let mut trailing_stdout = Vec::new();
    stdout
        .read_to_end(&mut trailing_stdout)
        .expect("read remaining stdout");
    assert!(trailing_stdout.is_empty());
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .expect("stderr pipe")
        .read_to_end(&mut stderr)
        .expect("read stderr");
    let status = child.wait().expect("wait sidecar");
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());

    let database = Connection::open(root.path().join("hello-agent.sqlite3"))
        .expect("inspect sidecar database");
    let clean: Option<String> = database
        .query_row(
            "SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id=?1;",
            [ready.sidecar_instance_id],
            |row| row.get(0),
        )
        .expect("clean marker");
    assert!(clean.is_some());
}
