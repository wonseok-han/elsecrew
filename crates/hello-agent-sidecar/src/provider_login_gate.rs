//! Internal pre-vendor gate. Not executable admission or a complete vendor environment.
//! Only trusted backend code may provide paths/arguments; never renderer input.
use std::ffi::OsStr;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};

const RELEASE: &[u8] = b"hello-agent-login-release-v1\n";
// No interpolated script, PATH lookup, startup files, or subprocess before release.
// exec preserves the tracked root PID. stdin then becomes the vendor protocol pipe.
const SCRIPT: &str =
    "IFS= read -r gate && [ \"$gate\" = hello-agent-login-release-v1 ] && exec \"$@\"";

pub(super) fn spawn(program: &Path, args: &[&OsStr], cwd: &Path) -> io::Result<Child> {
    command(program, args, cwd)?.spawn()
}

pub(super) fn command(program: &Path, args: &[&OsStr], cwd: &Path) -> io::Result<Command> {
    if !program.is_absolute() || !cwd.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute launch paths required",
        ));
    }
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", SCRIPT, "hello-agent-login-gate"])
        .arg(program)
        .args(args)
        .env_clear()
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    Ok(command)
}

/// Only the lifecycle owner's one-shot release callback calls this. Nonblocking,
/// one small pipe write: no indefinite wait and no retries after an uncertain write.
pub(super) fn release(child: &mut Child) -> io::Result<()> {
    let input = child
        .stdin
        .as_mut()
        .ok_or_else(|| io::Error::other("missing gate pipe"))?;
    let fd = input.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    if input.write(RELEASE)? != RELEASE.len() {
        return Err(io::Error::other("incomplete gate release"));
    }
    Ok(())
}
