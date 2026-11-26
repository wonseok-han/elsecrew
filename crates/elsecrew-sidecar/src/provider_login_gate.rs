//! Internal pre-vendor gate. Not executable admission or a complete vendor environment.
//! Only trusted backend code may provide paths/arguments; never renderer input.
use std::ffi::OsStr;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};

const RELEASE: &[u8] = b"elsecrew-login-release-v1\n";
// No PATH lookup, startup files, or subprocess before release. The trusted shell
// closes inherited non-stdio descriptors after its own exec (so Rust's spawn-error
// pipe is unaffected), before any vendor code. /dev/fd enumeration is fail-closed.
// eval receives ONLY kernel-provided, digit-validated fd numbers, never arguments.
// The final exec preserves the tracked root PID and its protocol stdin/stdout.
const SCRIPT: &str = r#"
[ -d /dev/fd ] || exit 125
for fd_path in /dev/fd/*; do
  fd=${fd_path##*/}
  case "$fd" in
    ''|*[!0-9]*) exit 125 ;;
    0|1|2) continue ;;
  esac
  eval "exec $fd>&-" || exit 125
done
unset fd fd_path
IFS= read -r gate && [ "$gate" = elsecrew-login-release-v1 ] && exec "$@"
"#;

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
    // The provider and everything it starts live in a session of their own
    // (the gate's PID): a child the engine never observed is still found by
    // its session, so a hold is only let go once the whole tree has ended
    // (PR #20 re-review B2). `setsid` is async-signal-safe.
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| if libc::setsid() == -1 { Err(io::Error::last_os_error()) } else { Ok(()) });
    }
    command
        .args(["-c", SCRIPT, "elsecrew-login-gate"])
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::{FromRawFd, OwnedFd};

    #[test]
    fn vendor_gate_does_not_inherit_unrelated_writable_descriptors() {
        // 10 is bash's own first save slot (SHELL_FD_BASE): see the probe below.
        for minimum in [3, 10, 200, 4096] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("original");
            std::fs::write(&path, b"original").unwrap();
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            // Deliberately simulate a non-CLOEXEC descriptor owned by another library.
            let raw = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, minimum) };
            assert!(raw >= minimum);
            let inherited = unsafe { OwnedFd::from_raw_fd(raw) };
            let number = raw.to_string();
            let mut child = command(
                Path::new("/bin/sh"),
                &[
                    OsStr::new("-c"),
                    // An external printf: for a builtin's redirection bash first
                    // saves stdout to the lowest free fd >= 10, so a closed fd 10
                    // would look writable (seen when parallel tests held 3..9).
                    OsStr::new(
                        "if (/usr/bin/printf changed >&\"$1\") 2>/dev/null; then exit 17; fi; printf closed",
                    ),
                    OsStr::new("fd-probe"),
                    OsStr::new(&number),
                ],
                temp.path(),
            )
            .unwrap()
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
            release(&mut child).unwrap();
            let output = child.wait_with_output().unwrap();
            // Closing in the gate must not mutate the parent's descriptor or flags.
            assert_eq!(
                unsafe { libc::fcntl(inherited.as_raw_fd(), libc::F_GETFD) },
                0
            );
            drop(inherited);
            assert!(
                output.status.success(),
                "descriptor probe failed: minimum={minimum}, fd={raw}, status={:?}, stderr={:?}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, b"closed");
            assert_eq!(std::fs::read(path).unwrap(), b"original");
        }
    }
}
