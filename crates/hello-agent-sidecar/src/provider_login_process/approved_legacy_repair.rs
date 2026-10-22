//! One approved historical repair, excluded from production and ordinary tests.
//! This does not manufacture a binding or prove absence of unobserved descendants.
use super::*;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use std::io::{Read, Seek, SeekFrom};

const ROOT: &str =
    "/Users/wonseok-han/Library/Application Support/hello-agent/com.wonseokhan.helloagent/m5";
const RUN: &str = "agent_run_b219c802fd2547659b2b452986786bde";

#[test]
#[ignore = "explicit single-case operational approval required; dry-run unless commit selected"]
fn approved_legacy_codex_repair() {
    let mode = std::env::var("HELLO_AGENT_APPROVED_LEGACY_REPAIR").expect("explicit mode required");
    assert!(matches!(mode.as_str(), "inspect" | "commit"));
    repair(mode == "commit").expect("repair refused; preserve fence and inspect evidence");
}

fn repair(commit: bool) -> Result<(), SidecarError> {
    let root = std::path::Path::new(ROOT);
    let profile = crate::provider_profile::CodexProfileBinding::existing_for_repair(root)?;
    let home = profile.revalidate()?;
    let directory = profile.login_lease_directory()?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let mut file = directory
        .open_with(PENDING, &options)
        .map_err(|_| failure())?;
    lock_exclusive(&file)?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !meta.is_file() || meta.len() != 107 {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(108)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() != 107 || !bytes.ends_with(b"\n") || bytes.contains(&b'\r') {
        return Err(failure());
    }
    let mut lines = std::str::from_utf8(&bytes).map_err(|_| failure())?.lines();
    if lines.next() != Some("hello-agent provider login pending v1") {
        return Err(failure());
    }
    let mut recorded = BTreeSet::new();
    for line in lines {
        let (pid, start) = line.split_once(' ').ok_or_else(failure)?;
        let p = ProcIdentity {
            pid: pid.parse().map_err(|_| failure())?,
            start_us: start.parse().map_err(|_| failure())?,
        };
        if p.pid <= 1
            || p.start_us <= 0
            || line != format!("{} {}", p.pid, p.start_us)
            || !recorded.insert(p)
        {
            return Err(failure());
        }
    }
    if recorded.len() != 3 {
        return Err(failure());
    }
    let mut reservation = Reservation {
        name: PENDING,
        directory,
        file,
        recorded,
        failed: false,
    };
    reservation.verify()?;
    // Do not let ProjectManager create missing storage directories during repair.
    for name in ["worktrees-v1", "worktree-manifests-v1", "save-staging-v1"] {
        let m = reservation
            .directory
            .symlink_metadata(name)
            .map_err(|_| failure())?;
        if !m.is_dir() {
            return Err(failure());
        }
    }
    let database = root.join("hello-agent.sqlite3");
    let storage = crate::Storage::locked_repair_snapshot(&database)?;
    let projects = crate::project::ProjectManager::new(&database)?;
    crate::agent_runtime::verify_approved_legacy_fence(
        &storage,
        &projects,
        RUN,
        &reservation.recorded,
    )?;
    for p in &reservation.recorded {
        if !matches!(
            SystemProcTable.probe(*p),
            hello_agent_core::proctree::ProbeResult::Gone
        ) {
            return Err(failure());
        }
    }
    if profile.revalidate()? != home
        || !crate::provider_profile::same_directory(
            &reservation.directory,
            &profile.login_lease_directory()?,
        )?
    {
        return Err(failure());
    }
    reservation.verify()?;
    reservation
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| failure())?;
    let mut after = Vec::new();
    (&mut reservation.file)
        .take(108)
        .read_to_end(&mut after)
        .map_err(|_| failure())?;
    if after != bytes {
        return Err(failure());
    }
    println!(
        "verified legacy candidate: run={RUN}, identities=3, device={}, inode={}, commit={commit}",
        meta.dev(),
        meta.ino()
    );
    if commit {
        let expected = std::env::var("HELLO_AGENT_APPROVED_LEGACY_FILE").map_err(|_| failure())?;
        if expected != format!("{}:{}", meta.dev(), meta.ino()) {
            return Err(failure());
        }
        crate::agent_runtime::verify_approved_legacy_fence(
            &storage,
            &projects,
            RUN,
            &reservation.recorded,
        )?;
        // Retain an exclusive durable audit before the only destructive operation.
        // A retry after interruption is intentionally refused, never automatic.
        let mut audit_options = OpenOptions::new();
        audit_options.write(true).create_new(true).mode(0o600);
        let mut audit = reservation
            .directory
            .open_with("approved-legacy-codex-repair-20261022.json", &audit_options)
            .map_err(|_| failure())?;
        let value = serde_json::json!({"runId":RUN,"device":meta.dev(),"inode":meta.ino(),"state":"approvedRemovalAttempt","unobservedDescendantsNotProven":true});
        audit
            .write_all(value.to_string().as_bytes())
            .map_err(|_| failure())?;
        audit.sync_all().map_err(|_| failure())?;
        sync_directory(&reservation.directory)?;
        reservation.release()?;
        println!("approved legacy fence removed; durable audit retained");
    }
    Ok(())
}

#[test]
fn repair_snapshot_excludes_other_writers_and_forbids_own_writes() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("hello-agent.sqlite3");
    drop(crate::Storage::open(&database).unwrap());
    let held = crate::Storage::locked_repair_snapshot(&database).unwrap();
    let other = rusqlite::Connection::open(&database).unwrap();
    other.busy_timeout(Duration::ZERO).unwrap();
    let error = other
        .execute_batch("CREATE TABLE forbidden_other(x);")
        .unwrap_err();
    assert!(matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ));
    let error = held
        .connection_for_test()
        .execute_batch("CREATE TABLE forbidden_self(x);")
        .unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::ReadOnly)
    );
    drop(held);
    assert_eq!(
        other
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name LIKE 'forbidden_%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}
