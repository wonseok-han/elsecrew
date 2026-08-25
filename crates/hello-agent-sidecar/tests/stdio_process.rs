#![cfg(not(windows))]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

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

#[test]
fn binary_project_worktree_read_stream_and_quit_follow_g4_order() {
    let root = tempfile::tempdir().expect("temporary directory");
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("src/file.txt"), "가나다 hello").unwrap();
    git(&source, &["init", "-q"]);
    git(&source, &["add", "."]);
    git(
        &source,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .arg("--stdio")
        .current_dir(&data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sidecar");
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let ready = receive_envelope(&envelopes);
    let EnvelopeV1::Event(ready_event) = ready else {
        panic!("ready event")
    };
    let ready = ready_payload(&ready_event).unwrap();
    assert_eq!(ready.capabilities.len(), 11);

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_open",
        "project.open",
        serde_json::json!({"selectedPath":source}),
    );
    let EnvelopeV1::Response(open) = receive_envelope(&envelopes) else {
        panic!("open response")
    };
    assert!(open.ok);
    let project_id = open.result.unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_string();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_create",
        "project.createWorktree",
        serde_json::json!({"projectId":project_id}),
    );
    let EnvelopeV1::Response(create) = receive_envelope(&envelopes) else {
        panic!("create response")
    };
    assert!(create.ok);
    let worktree_path = create.result.unwrap()["worktreePath"]
        .as_str()
        .unwrap()
        .to_string();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_watch",
        "file.watch",
        serde_json::json!({"projectId":project_id}),
    );
    let EnvelopeV1::Response(watch) = receive_envelope(&envelopes) else {
        panic!("watch response")
    };
    assert!(watch.ok);
    fs::write(
        std::path::Path::new(&worktree_path).join("src/file.txt"),
        "비동기 변경",
    )
    .unwrap();
    let EnvelopeV1::Event(changed) = receive_envelope(&envelopes) else {
        panic!("spontaneous watch event")
    };
    assert_eq!(changed.event, "file.changed");
    assert!(changed.payload["paths"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path == "src/file.txt"));

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_read",
        "file.read",
        serde_json::json!({"projectId":project_id,"path":"src/file.txt"}),
    );
    let EnvelopeV1::Response(read) = receive_envelope(&envelopes) else {
        panic!("read descriptor")
    };
    let descriptor = read.result.unwrap();
    let stream_id = descriptor["stream"]["streamId"]
        .as_str()
        .unwrap()
        .to_string();
    let EnvelopeV1::Event(chunk) = receive_envelope(&envelopes) else {
        panic!("blob chunk")
    };
    assert_eq!(chunk.stream_id, stream_id);
    assert_eq!(chunk.seq, 1);
    assert_eq!(chunk.event, "blob.chunk");
    assert_eq!(chunk.payload["text"], "비동기 변경");

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_quit",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    let EnvelopeV1::Response(quit) = receive_envelope(&envelopes) else {
        panic!("quit response")
    };
    assert!(quit.ok);
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

fn write_request(
    stdin: &mut impl Write,
    instance: &str,
    request_id: &str,
    method: &str,
    params: serde_json::Value,
) {
    let request = EnvelopeV1::Request(RequestV1 {
        protocol_version: 1,
        kind: "request".to_string(),
        sidecar_instance_id: instance.to_string(),
        request_id: request_id.to_string(),
        method: method.to_string(),
        params,
    });
    stdin
        .write_all(encode_jsonl(&request).unwrap().as_bytes())
        .unwrap();
    stdin.flush().unwrap();
}

fn spawn_envelope_reader(stdout: impl Read + Send + 'static) -> Receiver<EnvelopeV1> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            if sender.send(decode_jsonl(line.as_bytes()).unwrap()).is_err() {
                break;
            }
        }
    });
    receiver
}

fn receive_envelope(receiver: &Receiver<EnvelopeV1>) -> EnvelopeV1 {
    receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("bounded sidecar envelope timeout")
}

fn git(directory: &std::path::Path, args: &[&str]) {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
