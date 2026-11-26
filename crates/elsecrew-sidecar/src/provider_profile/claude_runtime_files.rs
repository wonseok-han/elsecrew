//! The signed Claude parents' mux sockets only (this run's and, since runs of
//! one agent go on side by side, the profile's other live runs'), never
//! workspace socket admission.
use super::*;
use elsecrew_core::proctree::{identity_of, ProcIdentity};

pub(crate) fn verify_claude_runtime_files(
    home: &Path,
    owner: ProcIdentity,
) -> Result<(), SidecarError> {
    verify_with(home, Some(owner))
}

/// The profile before (or without) a Claude parent of its own: only the mux
/// sockets of this profile's other live runs are admitted.
pub(crate) fn verify_claude_home_files(home: &Path) -> Result<(), SidecarError> {
    verify_with(home, None)
}

fn verify_with(home: &Path, owner: Option<ProcIdentity>) -> Result<(), SidecarError> {
    let live = |identity: &ProcIdentity| identity_of(identity.pid).is_ok_and(|current| current == *identity);
    // Runs of one agent go on side by side in one profile (ADE multi-project
    // P5), so its `tmp` also holds the mux sockets of the other live runs'
    // Claude parents: those recorded in this profile's run holds.
    let admitted: Vec<ProcIdentity> = owner.into_iter().chain(sibling_runs(home)).filter(live).collect();
    let root = open_absolute_directory(home)?;
    let mut remaining = elsecrew_protocol::project_v1::SNAPSHOT_ENTRIES_MAX_V1;
    let mut seen = Vec::new();
    scan(&root, Path::new(""), &admitted, &mut seen, &mut remaining, 0)?;
    // Each socket's process must still be the one admitted after the scan.
    if seen.iter().any(|identity| !live(identity)) {
        return Err(failure());
    }
    Ok(())
}

/// Process identities recorded in the Claude run holds next to the profile
/// (`claude-run-<id>.pending`, one per live run, model list or check). A hold
/// that cannot be read admits nothing.
fn sibling_runs(home: &Path) -> Vec<ProcIdentity> {
    let Some(parent) = home.parent() else { return Vec::new() };
    let Ok(directory) = open_absolute_directory(parent) else { return Vec::new() };
    let Ok(names) = crate::provider_login_process::run_hold_names(&directory, "claude") else { return Vec::new() };
    let mut identities = Vec::new();
    for name in names {
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).custom_flags(libc::O_NONBLOCK);
        let Ok(file) = directory.open_with(&name, &options) else { continue };
        let Ok(meta) = file.metadata() else { continue };
        if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() }
            || meta.len() > crate::provider_login_process::HOLD_MAX_BYTES { continue; }
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut file.into_std(), &mut text).is_err() { continue; }
        let mut lines = text.lines();
        if lines.next() != Some("elsecrew provider login pending v1") { continue; }
        identities.extend(lines.filter_map(|line| {
            let (pid, start) = line.split_once(' ')?;
            Some(ProcIdentity { pid: pid.parse().ok()?, start_us: start.parse().ok()? })
        }).filter(|identity| identity.pid > 1 && identity.start_us > 0));
    }
    identities
}

fn scan(
    root: &Dir,
    relative: &Path,
    admitted: &[ProcIdentity],
    seen: &mut Vec<ProcIdentity>,
    remaining: &mut u64,
    depth: usize,
) -> Result<(), SidecarError> {
    use cap_std::fs::FileTypeExt;
    if depth > 128 {
        return Err(failure());
    }
    let directory = root.dir_metadata().map_err(|_| failure())?;
    if directory.uid() != unsafe { libc::geteuid() } || directory.mode() & 0o022 != 0 {
        return Err(failure());
    }
    for entry in root.entries().map_err(|_| failure())? {
        *remaining = remaining.checked_sub(1).ok_or_else(failure)?;
        let name = entry.map_err(|_| failure())?.file_name();
        let meta = root.symlink_metadata(&name).map_err(|_| failure())?;
        if meta.is_file() && meta.nlink() == 1 {
            continue;
        }
        if meta.file_type().is_socket() {
            let owner = name.to_str().and_then(|s| s.strip_prefix("srt-mux-"))
                .and_then(|s| s.split_once('-'))
                .and_then(|(pid, _)| admitted.iter().find(|identity| identity.pid.to_string() == pid));
            let sequence = owner.and_then(|owner| name
                .to_str()
                .and_then(|s| s.strip_prefix(&format!("srt-mux-{}-", owner.pid)))
                .and_then(|s| s.strip_suffix(".sock")));
            if owner.is_none()
                || relative != Path::new("tmp")
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.nlink() != 1
                || meta.mode() & 0o022 != 0
                || !sequence.is_some_and(|s| {
                    !s.is_empty()
                        && s.len() <= 16
                        && s.bytes()
                            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
                })
            {
                return Err(failure());
            }
            seen.extend(owner);
            continue;
        }
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(failure());
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .maybe_dir(true)
            .follow(FollowSymlinks::No)
            .custom_flags(libc::O_NONBLOCK);
        let file = root.open_with(&name, &options).map_err(|_| failure())?;
        let opened = file.metadata().map_err(|_| failure())?;
        if !opened.is_dir() || opened.dev() != meta.dev() || opened.ino() != meta.ino() {
            return Err(failure());
        }
        scan(
            &Dir::from_std_file(file.into_std()),
            &relative.join(name),
            admitted,
            seen,
            remaining,
            depth + 1,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    #[test]
    fn only_live_owner_mux_in_private_tmp_is_admitted() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        std::fs::create_dir(home.join("tmp")).unwrap();
        let owner = identity_of(std::process::id()).unwrap();
        let socket = home.join(format!("tmp/srt-mux-{}-0.sock", owner.pid));
        let _listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o700)).unwrap();
        verify_claude_runtime_files(&home, owner).unwrap();
        assert!(verify_runtime_files(&home).is_err());
        assert!(verify_claude_runtime_files(
            &home,
            ProcIdentity {
                start_us: owner.start_us + 1,
                ..owner
            }
        )
        .is_err());
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o722)).unwrap();
        assert!(verify_claude_runtime_files(&home, owner).is_err());
    }

    #[test]
    fn another_live_runs_mux_is_admitted_only_while_its_hold_records_it() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("p");
        std::fs::create_dir_all(home.join("tmp")).unwrap();
        // This run's Claude parent: a child of the test; the other run's: the test.
        let mut child = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let owner = identity_of(child.id()).unwrap();
        let other = identity_of(std::process::id()).unwrap();
        let socket = home.join(format!("tmp/srt-mux-{}-0.sock", other.pid));
        let _listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(verify_claude_runtime_files(&home, owner).is_err());
        assert!(verify_claude_home_files(&home).is_err());
        let hold = root.join(format!("claude-run-{}.pending", "a".repeat(32)));
        std::fs::write(&hold, format!("elsecrew provider login pending v1\n{} {}\n", other.pid, other.start_us)).unwrap();
        verify_claude_runtime_files(&home, owner).unwrap();
        verify_claude_home_files(&home).unwrap();
        // A recorded identity that is not the live process admits nothing.
        std::fs::write(&hold, format!("elsecrew provider login pending v1\n{} {}\n", other.pid, other.start_us + 1)).unwrap();
        assert!(verify_claude_home_files(&home).is_err());
        // Nor does a hold of another agent.
        std::fs::remove_file(&hold).unwrap();
        std::fs::write(root.join(format!("codex-run-{}.pending", "a".repeat(32))), format!("elsecrew provider login pending v1\n{} {}\n", other.pid, other.start_us)).unwrap();
        assert!(verify_claude_home_files(&home).is_err());
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn unrelated_sockets_and_links_remain_rejected() {
        let owner = identity_of(std::process::id()).unwrap();
        for name in [
            format!("srt-mux-{}-0.sock", owner.pid),
            "tmp/srt-mux-0-0.sock".into(),
            format!("tmp/nested/srt-mux-{}-0.sock", owner.pid),
            format!("tmp/srt-mux-{}-.sock", owner.pid),
            "tmp/other.sock".into(),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let home = temp.path().canonicalize().unwrap();
            let socket = home.join(name);
            std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
            let _listener = UnixListener::bind(&socket).unwrap();
            assert!(verify_claude_runtime_files(&home, owner).is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        std::fs::create_dir(home.join("tmp")).unwrap();
        std::fs::write(home.join("original"), b"unchanged").unwrap();
        let path = home.join(format!("tmp/srt-mux-{}-0.sock", owner.pid));
        std::os::unix::fs::symlink(home.join("original"), &path).unwrap();
        assert!(verify_claude_runtime_files(&home, owner).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(home.join("original"), &path).unwrap();
        assert!(verify_claude_runtime_files(&home, owner).is_err());
        assert_eq!(std::fs::read(home.join("original")).unwrap(), b"unchanged");
    }
}
