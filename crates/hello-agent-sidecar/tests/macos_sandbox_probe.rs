//! Opt-in OS mechanism probe, NOT a production sandbox or an admission gate.
//! Only temporary fixtures and system shells are used; no vendor/account access.
#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::symlink;
use std::process::{Command, Stdio};

// Deliberately a filesystem-only experiment. Broad read/process/mach permissions
// are NOT an acceptable production policy; this proves only the assertions below.
const POLICY: &str = r#"(version 1)
(allow default)
(deny network*)
(deny file-write*)
(allow file-write* (subpath (param "WORKSPACE")))
"#;

// Second experiment: no blanket permission for Mach IPC, sockets or networking.
// Read roots are narrowed for this toy policy; vendor compatibility is NOT proven.
const DEFAULT_DENY_POLICY: &str = r#"(version 1)
(deny default)
(allow file-read-metadata)
(allow file-read*
    (literal "/")
    (subpath "/System") (subpath "/usr") (subpath "/bin") (subpath "/sbin")
    (literal "/dev/null") (literal "/dev/urandom")
    (subpath (param "WORKSPACE"))
    (subpath (param "PROFILE"))
    (subpath (param "SCRATCH")))
(allow process-exec)
(allow process-fork)
(allow signal (target self))
(allow sysctl-read)
(allow file-write*
    (subpath (param "WORKSPACE"))
    (subpath (param "PROFILE"))
    (subpath (param "SCRATCH")))
(deny file-write*
    (subpath (string-append (param "WORKSPACE") "/.git"))
    (subpath (string-append (param "WORKSPACE") "/.agents"))
    (subpath (string-append (param "WORKSPACE") "/.codex"))
    (literal (string-append (param "PROFILE") "/config.toml")))
"#;

#[test]
#[ignore = "macOS external sandbox only; temporary files and loopback listener, no vendor"]
fn default_deny_outer_policy_enforces_writes_and_network_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let workspace = root.join("workspace");
    let profile = root.join("profile");
    let scratch = root.join("scratch");
    let source = root.join("source");
    for directory in [&workspace, &profile, &scratch, &source] {
        fs::create_dir(directory).unwrap();
    }
    for directory in [".git", ".agents", ".codex"] {
        fs::create_dir(workspace.join(directory)).unwrap();
        fs::write(workspace.join(directory).join("protected"), b"original").unwrap();
    }
    fs::write(profile.join("config.toml"), b"original").unwrap();
    fs::write(source.join("original"), b"original").unwrap();
    symlink(&source, workspace.join("escape")).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let control = Command::new("/usr/bin/nc")
        .args(["-n", "-z", "-G", "1", "-w", "1", "127.0.0.1", &port])
        .output()
        .unwrap();
    assert!(control.status.success(), "loopback positive control failed");
    drop(
        listener
            .accept()
            .expect("positive-control connection missing"),
    );
    let run = |script: &str| {
        Command::new("/usr/bin/sandbox-exec")
            .args(["-p", DEFAULT_DENY_POLICY, "-D"])
            .arg(format!("WORKSPACE={}", workspace.display()))
            .arg("-D")
            .arg(format!("PROFILE={}", profile.display()))
            .arg("-D")
            .arg(format!("SCRATCH={}", scratch.display()))
            .args(["/bin/sh", "-c", script, "deny-default-probe"])
            .arg(&workspace)
            .arg(&profile)
            .arg(&scratch)
            .arg(&source)
            .arg(&port)
            .env_clear()
            .current_dir(&workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    };
    for script in [
        "printf allowed > \"$1/allowed\"",
        "printf allowed > \"$2/runtime\"",
        "printf allowed > \"$3/temporary\"",
        "/bin/sh -c 'printf child > \"$1/child\"' child \"$1\"",
    ] {
        let output = run(script);
        assert!(
            output.status.success(),
            "allowed operation failed ({script}, status={}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let read_allowed = run("/bin/cat \"$1/allowed\"");
    assert!(read_allowed.status.success());
    assert_eq!(read_allowed.stdout, b"allowed");
    for script in [
        "/bin/cat \"$4/original\"",
        "printf changed > \"$4/original\"",
        "printf changed > \"$1/escape/original\"",
        "printf changed > \"$1/.git/protected\"",
        "printf changed > \"$1/.agents/protected\"",
        "printf changed > \"$1/.codex/protected\"",
        "printf changed > \"$2/config.toml\"",
        "/bin/mv \"$1/.git\" \"$1/renamed-git\"",
        "/bin/rm \"$2/config.toml\"",
        "/bin/ln \"$4/original\" \"$1/new-link\"",
        "/usr/bin/nc -n -z -G 1 -w 1 127.0.0.1 \"$5\"",
    ] {
        let output = run(script);
        assert!(
            !output.status.success(),
            "forbidden operation succeeded: {script}"
        );
    }
    assert_eq!(fs::read(source.join("original")).unwrap(), b"original");
    assert_eq!(fs::read(profile.join("config.toml")).unwrap(), b"original");
    for directory in [".git", ".agents", ".codex"] {
        assert_eq!(
            fs::read(workspace.join(directory).join("protected")).unwrap(),
            b"original"
        );
    }
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}

#[test]
#[ignore = "requires macOS sandbox-exec outside an existing nested sandbox; temp fixtures only"]
fn outer_sandbox_write_boundary_probe() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let workspace = root.join("workspace");
    let source = root.join("source");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&source).unwrap();
    let original = source.join("original");
    fs::write(&original, b"original").unwrap();
    symlink(&source, workspace.join("source-link")).unwrap();
    fs::hard_link(&original, workspace.join("hard-link")).unwrap();
    let run = |script: &str, output: Stdio| {
        Command::new("/usr/bin/sandbox-exec")
            .args(["-p", POLICY, "-D"])
            .arg(format!("WORKSPACE={}", workspace.display()))
            .args(["/bin/sh", "-c", script, "sandbox-probe"])
            .arg(&workspace)
            .arg(&source)
            .env_clear()
            .current_dir(&workspace)
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    };
    let allowed = run("printf allowed > \"$1/allowed\"", Stdio::null());
    assert!(
        allowed.status.success(),
        "sandbox unavailable or allowed write failed: {}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    assert_eq!(fs::read(workspace.join("allowed")).unwrap(), b"allowed");
    for (name, script) in [
        ("direct", "printf changed > \"$2/original\""),
        ("symlink", "printf changed > \"$1/source-link/original\""),
        ("parent", "printf changed > \"$1/../source/original\""),
        (
            "child",
            "/bin/sh -c 'printf changed > \"$1/original\"' child \"$2\"",
        ),
        ("delete", "/bin/rm \"$2/original\""),
        ("rename", "/bin/mv \"$2/original\" \"$1/stolen\""),
    ] {
        let result = run(script, Stdio::null());
        assert!(!result.status.success(), "outside write succeeded: {name}");
        assert_eq!(fs::read(&original).unwrap(), b"original", "{name}");
    }
    // An inherited writable descriptor must not be presumed protected by paths.
    let held = fs::OpenOptions::new().write(true).open(&original).unwrap();
    let inherited = run("printf changed", Stdio::from(held));
    let inherited_unchanged = fs::read(&original).unwrap() == b"original";
    eprintln!(
        "inherited descriptor: exit_success={} source_unchanged={inherited_unchanged}",
        inherited.status.success()
    );
    fs::write(&original, b"original").unwrap();
    let linked = run("printf changed > \"$1/hard-link\"", Stdio::null());
    let hardlink_unchanged = fs::read(&original).unwrap() == b"original";
    eprintln!(
        "preexisting hard link: exit_success={} source_unchanged={hardlink_unchanged}",
        linked.status.success()
    );
    fs::write(&original, b"original").unwrap();
    let new_link = run(
        "/bin/ln \"$2/original\" \"$1/new-link\" && printf changed > \"$1/new-link\"",
        Stdio::null(),
    );
    let new_link_unchanged = fs::read(&original).unwrap() == b"original";
    eprintln!(
        "new hard link: exit_success={} source_unchanged={new_link_unchanged}",
        new_link.status.success()
    );
    // Observations, not relaxed product assertions. A changed source rejects this
    // policy as a stand-alone source-protection solution, even when this probe passes.
    eprintln!(
        "standalone_source_protection_candidate={}",
        inherited_unchanged && hardlink_unchanged && new_link_unchanged
    );
}
