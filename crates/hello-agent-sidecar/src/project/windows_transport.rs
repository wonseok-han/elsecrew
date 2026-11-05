//! Owned Windows stdio transport. The Git/CWD admission gate is still closed.
#![allow(dead_code)]
use super::{
    windows_directory_lease::DirectoryLease,
    windows_job::native::Family,
    windows_pipe::{BudgetPipe, Direction},
    GIT_STDERR_TAIL_BYTES,
};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    os::windows::io::AsHandle,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) struct Transport {
    pub(super) stdin: Option<BudgetPipe>,
    pub(super) stdout: Option<BudgetPipe>,
    family: Family,
    cancelled: Arc<AtomicBool>,
    external_cancel: Option<Arc<AtomicBool>>,
    stderr: Option<JoinHandle<io::Result<Vec<u8>>>>,
    deadline: Instant,
    finished: bool,
}
impl Transport {
    /// Caller must persist its launch fence before calling. Family holds the
    /// shared Windows spawn lock while temporary inheritable handles exist. This API does not establish CWD/executable authority.
    pub(super) fn spawn(
        executable: &str,
        args: &[String],
        environment: &BTreeMap<String, String>,
        directory: &str,
        budget: Duration,
        external_cancel: Option<Arc<AtomicBool>>,
    ) -> io::Result<Self> {
        Self::spawn_inner(
            executable,
            args,
            environment,
            directory,
            budget,
            external_cancel,
            None,
        )
    }
    /// Moves CWD protection into process-family ownership. This does not grant
    /// executable authority or enable the product's Windows Git admission.
    pub(super) fn spawn_in_lease(
        executable: &str,
        args: &[String],
        environment: &BTreeMap<String, String>,
        directory: DirectoryLease,
        budget: Duration,
        external_cancel: Option<Arc<AtomicBool>>,
    ) -> io::Result<Self> {
        let path = directory.path().to_owned();
        Self::spawn_inner(
            executable,
            args,
            environment,
            &path,
            budget,
            external_cancel,
            Some(directory),
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn spawn_inner(
        executable: &str,
        args: &[String],
        environment: &BTreeMap<String, String>,
        directory: &str,
        budget: Duration,
        external_cancel: Option<Arc<AtomicBool>>,
        lease: Option<DirectoryLease>,
    ) -> io::Result<Self> {
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or_else(|| io::Error::other("invalid Git budget"))?;
        let cancelled = Arc::new(AtomicBool::new(false));
        if budget.is_zero()
            || external_cancel
                .as_ref()
                .is_some_and(|v| v.load(Ordering::Acquire))
        {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Git start canceled",
            ));
        }
        let (stdin, child_in) = BudgetPipe::pair(
            Direction::Write,
            deadline,
            cancelled.clone(),
            external_cancel.clone(),
        )?;
        let (stdout, child_out) = BudgetPipe::pair(
            Direction::Read,
            deadline,
            cancelled.clone(),
            external_cancel.clone(),
        )?;
        let (mut stderr, child_err) = BudgetPipe::pair(
            Direction::Read,
            deadline,
            cancelled.clone(),
            external_cancel.clone(),
        )?;
        let stdio = Some([
            child_in.as_handle(),
            child_out.as_handle(),
            child_err.as_handle(),
        ]);
        let family = match lease {
            Some(lease) => Family::suspended_in_lease(executable, args, environment, lease, stdio),
            None => Family::suspended_with_stdio(executable, args, environment, directory, stdio),
        }?;
        // Do not keep parent copies of child endpoints: they would suppress EOF.
        drop((child_in, child_out, child_err));
        let mut owner = Self {
            stdin: Some(stdin),
            stdout: Some(stdout),
            family,
            cancelled,
            external_cancel,
            stderr: None,
            deadline,
            finished: false,
        };
        owner.stderr = Some(
            thread::Builder::new()
                .name("git-stderr".into())
                .spawn(move || {
                    let mut tail = Vec::with_capacity(GIT_STDERR_TAIL_BYTES);
                    let mut buffer = [0; 4096];
                    loop {
                        let count = stderr.read(&mut buffer)?;
                        if count == 0 {
                            return Ok(tail);
                        }
                        let excess = (tail.len() + count).saturating_sub(GIT_STDERR_TAIL_BYTES);
                        tail.drain(..excess);
                        tail.extend_from_slice(&buffer[..count]);
                    }
                })?,
        );
        owner.check()?;
        owner.family.start()?;
        Ok(owner)
    }
    fn check(&self) -> io::Result<()> {
        if self
            .external_cancel
            .as_ref()
            .is_some_and(|v| v.load(Ordering::Acquire))
        {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Git owner stopped",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Git command exceeded its time budget",
            ));
        }
        Ok(())
    }
    pub(super) fn finish(&mut self) -> io::Result<(u32, Vec<u8>)> {
        if self.finished {
            return Err(io::Error::other("Git completion already consumed"));
        }
        // Closing stdin is an explicit EOF; a child must not wait for its owner.
        self.stdin.take();
        let reader = self
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Git stderr already consumed"))?;
        // The reader shares the deadline/cancel token and has bounded cancel drain.
        let stderr = reader
            .join()
            .map_err(|_| io::Error::other("Git stderr reader panicked"))??;
        loop {
            self.check()?;
            if let Some(status) = self.family.completed()? {
                self.finished = true;
                return Ok((status, stderr));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.stdin.take();
        self.stdout.take();
        if !self.finished {
            // Cleanup request only: a failed caller must retain its durable fence.
            // Drop cannot upgrade a timeout/error into a successful termination.
            let _ = self.family.terminate(Duration::from_secs(2));
        }
        if let Some(reader) = self.stderr.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unconfirmed_unwind_retains_pins_until_owner_process_exits() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("guard"), b"pin").unwrap();
        let mut owner = Transport::spawn(
            std::env::current_exe().unwrap().to_str().unwrap(),
            &[
                "--exact".into(),
                "project::windows_job::native::tests::family_fixture".into(),
                "--nocapture".into(),
            ],
            &BTreeMap::from([
                ("SystemRoot".into(), std::env::var("SystemRoot").unwrap()),
                ("HELLO_AGENT_JOB_FIXTURE".into(), "retention-unwind".into()),
            ]),
            root.path().canonicalize().unwrap().to_str().unwrap(),
            Duration::from_secs(10),
            None,
        )
        .unwrap();
        let mut output = String::new();
        owner
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut output)
            .unwrap();
        assert_eq!(owner.finish().unwrap().0, 0);
        assert!(output.contains("HELLO_RETAINED"));
        std::fs::remove_file(root.path().join("guard")).unwrap();
    }

    #[test]
    fn guid_cwd_pins_survive_until_confirmed_family_release() {
        use cap_std::{ambient_authority, fs::Dir};
        use std::ffi::OsStr;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("guard"), b"pin").unwrap();
        std::fs::write(root.path().join("cwd-marker"), b"owned-directory").unwrap();
        let dir = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
        let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        let mut owner = Transport::spawn_in_lease(
            std::env::current_exe().unwrap().to_str().unwrap(),
            &[
                "--exact".into(),
                "project::windows_job::native::tests::family_fixture".into(),
                "--nocapture".into(),
            ],
            &BTreeMap::from([
                ("SystemRoot".into(), std::env::var("SystemRoot").unwrap()),
                ("HELLO_AGENT_JOB_FIXTURE".into(), "cwd".into()),
            ]),
            lease,
            Duration::from_secs(10),
            None,
        )
        .unwrap();
        assert_eq!(
            std::fs::remove_file(root.path().join("guard"))
                .unwrap_err()
                .raw_os_error(),
            Some(32)
        );
        let mut output = String::new();
        owner
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut output)
            .unwrap();
        assert_eq!(owner.finish().unwrap().0, 0);
        assert!(output.contains("HELLO_CWD_OK"));
        drop(owner);
        std::fs::remove_file(root.path().join("guard")).unwrap();
    }

    #[test]
    fn owned_transport_captures_fixture_and_proves_completion() {
        let directory = tempfile::tempdir().unwrap();
        let env = BTreeMap::from([
            ("SystemRoot".into(), std::env::var("SystemRoot").unwrap()),
            ("HELLO_AGENT_JOB_FIXTURE".into(), "output".into()),
        ]);
        let mut owner = Transport::spawn(
            std::env::current_exe().unwrap().to_str().unwrap(),
            &[
                "--exact".into(),
                "project::windows_job::native::tests::family_fixture".into(),
                "--nocapture".into(),
            ],
            &env,
            directory.path().canonicalize().unwrap().to_str().unwrap(),
            Duration::from_secs(10),
            None,
        )
        .unwrap();
        let mut stdout = String::new();
        owner
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        let (code, stderr) = owner.finish().unwrap();
        assert_eq!(code, 0);
        assert!(stdout.contains("HELLO_JOB_STDOUT"));
        assert!(String::from_utf8(stderr)
            .unwrap()
            .contains("HELLO_JOB_STDERR"));
        assert!(owner.finish().is_err());
    }
}
