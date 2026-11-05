//! Windows adapter for the shared Git readers. Product admission stays closed.
use super::{
    windows_directory_lease::DirectoryLease, windows_executable::ExecutableLease,
    windows_pipe::BudgetPipe, windows_transport::Transport,
};
use std::{
    collections::BTreeMap,
    io,
    os::windows::process::ExitStatusExt,
    process::{Command, ExitStatus},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

pub(super) type GitStdin = BudgetPipe;
pub(super) type GitStdout = BudgetPipe;

pub(super) struct StreamingGitChild {
    owner: Transport,
    pub(super) stdin: Option<GitStdin>,
    pub(super) stdout: Option<GitStdout>,
}

impl StreamingGitChild {
    // A Command cannot carry the selected image/CWD capabilities. Never fall
    // back to Command::spawn even when the caller supplied an absolute path.
    pub(super) fn spawn(_command: &mut Command) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows Git requires admitted image and CWD capabilities",
        ))
    }

    pub(super) fn spawn_cancellable(
        command: &mut Command,
        cancel: Arc<AtomicBool>,
    ) -> io::Result<Self> {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Git owner stopped",
            ));
        }
        Self::spawn(command)
    }

    #[allow(dead_code)] // Installation trust and product admission are separate.
    pub(super) fn spawn_bound(
        image: ExecutableLease,
        args: &[String],
        environment: &BTreeMap<String, String>,
        directory: DirectoryLease,
        interactive_input: bool,
        cancel: Option<Arc<AtomicBool>>,
    ) -> io::Result<Self> {
        let mut owner = Transport::spawn_bound(
            image,
            args,
            environment,
            directory,
            Duration::from_secs(120),
            cancel,
        )?;
        let stdin = if interactive_input {
            owner.stdin.take()
        } else {
            owner.stdin.take();
            None
        };
        let stdout = owner.stdout.take();
        Ok(Self {
            owner,
            stdin,
            stdout,
        })
    }

    pub(super) fn finish(&mut self) -> io::Result<(ExitStatus, Vec<u8>)> {
        // EOF must precede the owner's stderr/family drain, including for callers
        // that did not move stdin into a batch reader.
        self.stdin.take();
        self.owner
            .finish()
            .map(|(code, stderr)| (ExitStatus::from_raw(code), stderr))
    }
}

impl Drop for StreamingGitChild {
    fn drop(&mut self) {
        self.stdin.take();
        self.stdout.take();
        // Transport owns cancellation and the complete descendant barrier.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::{ambient_authority, fs::Dir};
    use std::ffi::OsStr;

    #[test]
    fn raw_command_cannot_bypass_bound_transport() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        assert!(matches!(StreamingGitChild::spawn(&mut command),
            Err(error) if error.kind() == io::ErrorKind::Unsupported));
    }

    #[test]
    fn bound_transport_feeds_shared_git_control_reader() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("guard"), b"pin").unwrap();
        std::fs::write(root.path().join("cwd-marker"), b"owned-directory").unwrap();
        let dir = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
        let cwd = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        let exe = std::env::current_exe().unwrap();
        let parent = Dir::open_ambient_dir(exe.parent().unwrap(), ambient_authority()).unwrap();
        let image = ExecutableLease::acquire(&parent, exe.file_name().unwrap()).unwrap();
        let child = StreamingGitChild::spawn_bound(
            image,
            &[
                "--exact".into(),
                "project::windows_job::native::tests::family_fixture".into(),
                "--nocapture".into(),
            ],
            &BTreeMap::from([
                ("SystemRoot".into(), std::env::var("SystemRoot").unwrap()),
                ("HELLO_AGENT_JOB_FIXTURE".into(), "control-eof".into()),
            ]),
            cwd,
            false,
            None,
        )
        .unwrap();
        let output = crate::project::ProjectManager::finish_git_control(child, &[0]).unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("HELLO_CWD_OK"));
        std::fs::remove_file(root.path().join("guard")).unwrap();
    }
}
