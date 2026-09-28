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
