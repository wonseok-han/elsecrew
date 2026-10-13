//! Unix Git pipes share one wall-clock budget, including stderr and exit wait.
use super::GIT_STDERR_TAIL_BYTES;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const BUDGET: Duration = Duration::from_secs(120);
const TICK: Duration = Duration::from_millis(5);

pub(super) type GitStdin = BudgetPipe<ChildStdin>;
pub(super) type GitStdout = BudgetPipe<ChildStdout>;

pub(super) struct BudgetPipe<T> {
    pipe: T,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

impl<T: AsRawFd> BudgetPipe<T> {
    fn wait_ready(&self, events: libc::c_short) -> io::Result<()> {
        self.check()?;
        let mut descriptor = libc::pollfd {
            fd: self.pipe.as_raw_fd(),
            events,
            revents: 0,
        };
        // Wake as soon as the pipe is ready, while retaining the cancellation
        // check interval. Sleeping on every blob round trip compounds latency.
        let result = unsafe { libc::poll(&mut descriptor, 1, TICK.as_millis() as i32) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        // EOF and pipe errors are consumed by the next read/write, not hidden.
        self.check()
    }

    fn new(pipe: T, deadline: Instant, cancelled: Arc<AtomicBool>) -> io::Result<Self> {
        let fd = pipe.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            pipe,
            deadline,
            cancelled,
        })
    }
}

impl<T> BudgetPipe<T> {
    fn check(&self) -> io::Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
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
}

impl<T: Read + AsRawFd> Read for BudgetPipe<T> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            self.check()?;
            match self.pipe.read(buffer) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => self.wait_ready(libc::POLLIN)?,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }
}

impl<T: Write + AsRawFd> Write for BudgetPipe<T> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        loop {
            self.check()?;
            match self.pipe.write(buffer) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.wait_ready(libc::POLLOUT)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        // ChildStdin is an unbuffered OS pipe.
        self.check()
    }
}

pub(super) struct StreamingGitChild {
    child: Child,
    pub(super) stdin: Option<GitStdin>,
    pub(super) stdout: Option<GitStdout>,
    stderr: Option<JoinHandle<io::Result<Vec<u8>>>>,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    reaped: bool,
}

impl StreamingGitChild {
    pub(super) fn spawn(command: &mut Command) -> io::Result<Self> {
        Self::spawn_with_budget(command, BUDGET)
    }

    fn spawn_with_budget(command: &mut Command, budget: Duration) -> io::Result<Self> {
        command.process_group(0).stderr(Stdio::piped());
        let child = command.spawn()?;
        let mut owner = Self {
            child,
            stdin: None,
            stdout: None,
            stderr: None,
            deadline: Instant::now() + budget,
            cancelled: Arc::new(AtomicBool::new(false)),
            reaped: false,
        };
        if let Some(pipe) = owner.child.stdin.take() {
            owner.stdin = Some(BudgetPipe::new(
                pipe,
                owner.deadline,
                owner.cancelled.clone(),
            )?);
        }
        if let Some(pipe) = owner.child.stdout.take() {
            owner.stdout = Some(BudgetPipe::new(
                pipe,
                owner.deadline,
                owner.cancelled.clone(),
            )?);
        }
        let pipe = owner
            .child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Git stderr missing"))?;
        let mut pipe = BudgetPipe::new(pipe, owner.deadline, owner.cancelled.clone())?;
        owner.stderr = Some(thread::spawn(move || {
            let mut tail = Vec::with_capacity(GIT_STDERR_TAIL_BYTES);
            let mut buffer = [0; 4096];
            loop {
                let count = pipe.read(&mut buffer)?;
                if count == 0 {
                    return Ok(tail);
                }
                let excess = (tail.len() + count).saturating_sub(GIT_STDERR_TAIL_BYTES);
                tail.drain(..excess);
                tail.extend_from_slice(&buffer[..count]);
            }
        }));
        Ok(owner)
    }

    pub(super) fn finish(&mut self) -> io::Result<(ExitStatus, Vec<u8>)> {
        // Keep the unreaped leader as a PID anchor until all pipes finish. A
        // descendant retaining stderr must not bypass the command deadline.
        let stderr = self
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Git stderr already consumed"))?
            .join()
            .map_err(|_| io::Error::other("Git stderr reader panicked"))??;
        loop {
            if Instant::now() >= self.deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Git exit deadline exceeded",
                ));
            }
            if let Some(status) = self.child.try_wait()? {
                self.reaped = true;
                return Ok((status, stderr));
            }
            thread::sleep(TICK);
        }
    }
}

impl Drop for StreamingGitChild {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.stdin.take();
        self.stdout.take();
        if !self.reaped {
            // Only signal while the direct child has not been reaped: never
            // signal a recycled numeric process group after successful finish.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.kill();
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                match self.child.try_wait() {
                    Ok(Some(_)) => {
                        self.reaped = true;
                        break;
                    }
                    Ok(None) => thread::sleep(TICK),
                    Err(_) => break,
                }
            }
        }
        if let Some(reader) = self.stderr.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    fn assert_reaped(pid: u32) {
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn git_budget_handles_many_interactive_round_trips() {
        let mut command = shell("while IFS= read -r line; do printf 'x'; done");
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child =
            StreamingGitChild::spawn_with_budget(&mut command, Duration::from_secs(3)).unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        for _ in 0..1000 {
            input.write_all(b"request\n").unwrap();
            let mut byte = [0];
            output.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [b'x']);
        }
        drop(input);
        assert_eq!(output.read(&mut [0]).unwrap(), 0);
        assert!(child.finish().unwrap().0.success());
    }

    #[test]
    fn git_budget_preserves_output_status_and_bounded_stderr() {
        let mut command = shell(
            "printf hello; i=0; while [ $i -lt 10000 ]; do printf x >&2; i=$((i+1)); done; exit 7",
        );
        command.stdout(Stdio::piped());
        let mut child =
            StreamingGitChild::spawn_with_budget(&mut command, Duration::from_secs(5)).unwrap();
        let mut text = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let (status, stderr) = child.finish().unwrap();
        assert_eq!(text, "hello");
        assert_eq!(status.code(), Some(7));
        assert_eq!(stderr, vec![b'x'; GIT_STDERR_TAIL_BYTES]);
    }

    #[test]
    fn git_budget_bounds_stdout_stdin_and_inherited_stderr() {
        for scenario in ["stdout", "stdin", "stderr", "exit"] {
            let mut command = shell(match scenario {
                "stderr" => "/bin/sleep 5 & exit 0",
                "exit" => "exec >/dev/null 2>&1; exec /bin/sleep 5",
                _ => "exec /bin/sleep 5",
            });
            command.stdin(Stdio::piped()).stdout(Stdio::piped());
            let started = Instant::now();
            let mut child =
                StreamingGitChild::spawn_with_budget(&mut command, Duration::from_millis(80))
                    .unwrap();
            let pid = child.child.id();
            let error = match scenario {
                "stdout" => child.stdout.take().unwrap().read(&mut [0; 1]).unwrap_err(),
                "stdin" => child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(&vec![0; 1024 * 1024])
                    .unwrap_err(),
                _ => child.finish().unwrap_err(),
            };
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
            drop(child);
            assert_reaped(pid);
            assert!(started.elapsed() < Duration::from_secs(3));
        }
    }

    #[test]
    fn git_budget_drop_cancels_stderr_without_waiting_full_budget() {
        let mut command = shell("exec /bin/sleep 5");
        command.stdout(Stdio::piped());
        let child = StreamingGitChild::spawn(&mut command).unwrap();
        let pid = child.child.id();
        let started = Instant::now();
        drop(child);
        assert_reaped(pid);
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
