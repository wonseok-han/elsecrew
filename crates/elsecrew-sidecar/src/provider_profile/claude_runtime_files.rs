//! The signed Claude parents' mux sockets only (this run's and, since runs of
//! one agent go on side by side, the profile's other live runs'), never
//! workspace socket admission.
use super::*;
use elsecrew_core::proctree::{identity_of, ProcIdentity};

/// `executable`: the signed Claude program this run launched; another run's
/// socket is admitted only for a process running that very program.
pub(crate) fn verify_claude_runtime_files(
    home: &Path,
    owner: ProcIdentity,
    executable: Option<&Path>,
) -> Result<(), SidecarError> {
    verify_with(home, Some(owner), executable)
}

/// The profile before (or without) a Claude parent of its own: only the mux
/// sockets of this profile's other live runs are admitted.
pub(crate) fn verify_claude_home_files(home: &Path, executable: Option<&Path>) -> Result<(), SidecarError> {
    verify_with(home, None, executable)
}

fn verify_with(home: &Path, owner: Option<ProcIdentity>, executable: Option<&Path>) -> Result<(), SidecarError> {
    let live = |identity: &ProcIdentity| identity_of(identity.pid).is_ok_and(|current| current == *identity);
    // Runs of one agent go on side by side in one profile (ADE multi-project
    // P5), so its `tmp` also holds the mux sockets of the other live runs'
    // Claude parents: those recorded in this profile's run holds whose engine
    // holds them now, and (when this run knows its program) running the same
    // signed Claude program (PR #20 review I1).
    let program = executable.and_then(|path| std::fs::canonicalize(path).ok());
    if executable.is_some() && program.is_none() {
        return Err(failure());
    }
    let same_program = |identity: &ProcIdentity| program.as_ref()
        .is_none_or(|program| process_program(identity.pid).is_some_and(|path| &path == program));
    let admitted: Vec<ProcIdentity> = owner.into_iter()
        .chain(sibling_runs(home).into_iter().filter(same_program))
        .filter(live).collect();
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

/// The program a process runs, resolved (macOS `proc_pidpath`).
fn process_program(pid: u32) -> Option<std::path::PathBuf> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let length = unsafe { libc::proc_pidpath(pid as libc::c_int, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 { return None; }
    buffer.truncate(length as usize);
    let path = std::path::PathBuf::from(<std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(buffer));
    std::fs::canonicalize(path).ok()
}

/// Process identities recorded in the Claude run holds next to the profile
/// (`claude-run-<id>.pending`, one per live run, model list or check) whose
/// owner holds them now: a hold whose lock is free belongs to no live engine
/// and admits nothing (PR #20 review I1). A hold that cannot be read admits
/// nothing.
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
        // Its engine holds the hold's lock while it lives. A lock we can take
        // has no owner (closing the file below lets it go again).
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::EWOULDBLOCK)
        {
            continue;
        }
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
        verify_claude_runtime_files(&home, owner, None).unwrap();
        assert!(verify_runtime_files(&home).is_err());
        assert!(verify_claude_runtime_files(
            &home,
            ProcIdentity {
                start_us: owner.start_us + 1,
                ..owner
            },
            None,
        )
        .is_err());
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o722)).unwrap();
        assert!(verify_claude_runtime_files(&home, owner, None).is_err());
    }

    /// Another live run's mux socket (P5) is admitted only while that run's
    /// engine holds its hold (lock taken) and records the very process, and,
    /// when this run knows its program, only for a process running it (PR #20
    /// review I1). A hold written without its lock, a stale identity, another
    /// agent's hold or another program admits nothing.
    #[test]
    fn another_live_runs_mux_is_admitted_only_while_its_engine_holds_its_hold() {
        use std::os::fd::AsRawFd;
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
        assert!(verify_claude_runtime_files(&home, owner, None).is_err());
        assert!(verify_claude_home_files(&home, None).is_err());
        let hold = root.join(format!("claude-run-{}.pending", "a".repeat(32)));
        let record = |start: i64| std::fs::write(&hold, format!("elsecrew provider login pending v1\n{} {}\n", other.pid, start)).unwrap();
        // Written, but no engine holds it: nothing admitted.
        record(other.start_us);
        assert!(verify_claude_home_files(&home, None).is_err(), "a hold without its owner's lock");
        // Its engine holds it now.
        let held = std::fs::File::open(&hold).unwrap();
        assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        verify_claude_runtime_files(&home, owner, None).unwrap();
        verify_claude_home_files(&home, None).unwrap();
        // The same program as this run's: admitted; another program: not.
        let this_program = std::env::current_exe().unwrap();
        verify_claude_home_files(&home, Some(&this_program)).unwrap();
        verify_claude_runtime_files(&home, owner, Some(&this_program)).unwrap();
        assert!(verify_claude_home_files(&home, Some(std::path::Path::new("/bin/sleep"))).is_err(), "another program");
        // A recorded identity that is not the live process admits nothing.
        record(other.start_us + 1);
        assert!(verify_claude_home_files(&home, None).is_err());
        // The engine let go: nothing admitted again.
        record(other.start_us);
        drop(held);
        assert!(verify_claude_home_files(&home, None).is_err(), "the owner let go");
        // Nor does another agent's hold.
        std::fs::remove_file(&hold).unwrap();
        let codex = root.join(format!("codex-run-{}.pending", "a".repeat(32)));
        std::fs::write(&codex, format!("elsecrew provider login pending v1\n{} {}\n", other.pid, other.start_us)).unwrap();
        let held = std::fs::File::open(&codex).unwrap();
        assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        assert!(verify_claude_home_files(&home, None).is_err());
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
            assert!(verify_claude_runtime_files(&home, owner, None).is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        std::fs::create_dir(home.join("tmp")).unwrap();
        std::fs::write(home.join("original"), b"unchanged").unwrap();
        let path = home.join(format!("tmp/srt-mux-{}-0.sock", owner.pid));
        std::os::unix::fs::symlink(home.join("original"), &path).unwrap();
        assert!(verify_claude_runtime_files(&home, owner, None).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(home.join("original"), &path).unwrap();
        assert!(verify_claude_runtime_files(&home, owner, None).is_err());
        assert_eq!(std::fs::read(home.join("original")).unwrap(), b"unchanged");
    }
}
