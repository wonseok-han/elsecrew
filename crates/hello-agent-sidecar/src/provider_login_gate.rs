//! Internal pre-vendor gate. Not executable admission or a complete vendor environment.
//! Only trusted backend code may provide paths/arguments; never renderer input.
use std::ffi::OsStr;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};

const RELEASE: &[u8] = b"hello-agent-login-release-v1\n";
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
IFS= read -r gate && [ "$gate" = hello-agent-login-release-v1 ] && exec "$@"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::{FromRawFd, OwnedFd};

    #[test]
    fn vendor_gate_does_not_inherit_unrelated_writable_descriptors() {
        for minimum in [3, 200, 4096] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("original");
            std::fs::write(&path, b"original").unwrap();
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            // Deliberately simulate a non-CLOEXEC descriptor owned by another library.
            let raw = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, minimum) };
            assert!(raw >= minimum);
            let inherited = unsafe { OwnedFd::from_raw_fd(raw) };
            let number = raw.to_string();
            let mut child = spawn(
                Path::new("/bin/sh"),
                &[
                    OsStr::new("-c"),
                    OsStr::new(
                        "if (printf changed >&\"$1\") 2>/dev/null; then exit 17; fi; printf closed",
                    ),
                    OsStr::new("fd-probe"),
                    OsStr::new(&number),
                ],
                temp.path(),
            )
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
                "unrelated descriptor reached payload"
            );
            assert_eq!(output.stdout, b"closed");
            assert_eq!(std::fs::read(path).unwrap(), b"original");
        }
    }
}
