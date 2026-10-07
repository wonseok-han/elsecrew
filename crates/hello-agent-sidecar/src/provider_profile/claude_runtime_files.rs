//! The signed Claude parent's mux socket only, never workspace socket admission.
use super::*;
use hello_agent_core::proctree::{identity_of, ProcIdentity};

pub(crate) fn verify_claude_runtime_files(
    home: &Path,
    owner: ProcIdentity,
) -> Result<(), SidecarError> {
    let live = || identity_of(owner.pid).is_ok_and(|current| current == owner);
    let owner_live = live();
    let root = open_absolute_directory(home)?;
    let mut remaining = hello_agent_protocol::project_v1::SNAPSHOT_ENTRIES_MAX_V1;
    let mut socket_seen = false;
    scan(
        &root,
        Path::new(""),
        owner.pid,
        owner_live,
        &mut socket_seen,
        &mut remaining,
        0,
    )?;
    if socket_seen && !live() {
        return Err(failure());
    }
    Ok(())
}

fn scan(
    root: &Dir,
    relative: &Path,
    pid: u32,
    owner_live: bool,
    socket_seen: &mut bool,
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
            let prefix = format!("srt-mux-{pid}-");
            let sequence = name
                .to_str()
                .and_then(|s| s.strip_prefix(&prefix))
                .and_then(|s| s.strip_suffix(".sock"));
            if !owner_live
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
            *socket_seen = true;
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
            pid,
            owner_live,
            socket_seen,
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
