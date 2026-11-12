//! Fixed model-free commands. Raw account output never leaves this module.
use super::*;
use crate::provider_login_io::OutputChunk;
use elsecrew_core::provider::{parse_cli_version, BoundedObservation, ProviderId};

pub(super) enum Query {
    Version,
    Auth,
}

pub(super) enum Observation {
    Version(String),
    CredentialsPresent(bool),
}

pub(super) fn inspect(
    profile: ClaudeProfileBinding,
    executable: &PinnedExecutable,
    cancel: &AtomicBool,
    query: Query,
) -> Result<ProviderInspection<'static, Observation>, SidecarError> {
    let home = profile.revalidate()?;
    let user_home = PathBuf::from(std::env::var_os("HOME").ok_or_else(failure)?);
    if cancel.load(Ordering::Acquire) || !user_home.is_absolute() || user_home == home {
        return Err(failure());
    }
    let (args, limit, seconds): (&[&str], usize, u64) = match query {
        Query::Version => (&["--version"], 1024, 5),
        Query::Auth => (&["auth", "status", "--json"], 16384, 30),
    };
    let args: Vec<_> = args.iter().map(std::ffi::OsStr::new).collect();
    let mut command = crate::provider_login_gate::command(executable.revalidate()?, &args, &home)
        .map_err(|_| failure())?;
    // Same Keychain identity and isolated config as the login command. The gate
    // clears inherited API keys, proxies and vendor settings before these envs.
    command
        .env("HOME", &user_home)
        .env("CLAUDE_CONFIG_DIR", &home)
        .env("TMPDIR", home.join("tmp"))
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("BROWSER", "/usr/bin/false")
        .env("DISABLE_AUTOUPDATER", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
    let mut process = LoginProcess::reserve_claude_owned(profile)?;
    let result = (|| {
        process.launch_with_barrier(
            || command.spawn(),
            |child| {
                executable.revalidate().map_err(std::io::Error::other)?;
                if cancel.load(Ordering::Acquire) {
                    return Err(std::io::Error::other("Claude inspection canceled"));
                }
                crate::provider_login_gate::release(child)
            },
        )?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut output = Vec::new();
        let mut eof = false;
        loop {
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(failure());
            }
            if eof {
                process.tick()?;
            } else {
                match process.poll_output(cancel)? {
                    OutputChunk::Bytes(bytes) => {
                        if output.len() + bytes.len() > limit {
                            return Err(failure());
                        }
                        output.extend(bytes);
                    }
                    OutputChunk::Eof => eof = true,
                    OutputChunk::Pending => {}
                }
            }
            if eof {
                if let Some(status) = process.exit_status()? {
                    return match query {
                        Query::Version if status.success() => parse_cli_version(
                            ProviderId::ClaudeCode,
                            &BoundedObservation::new(output).map_err(|_| failure())?,
                        )
                        .map(Observation::Version)
                        .map_err(|_| failure()),
                        Query::Version => Err(failure()),
                        Query::Auth => crate::provider_claude_login::account(
                            &output,
                            status.code().ok_or_else(failure)?,
                            &home,
                        )
                        .map(Observation::CredentialsPresent),
                    };
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    let cleanup = process.finish();
    let result = result.and_then(|observation| {
        cleanup?;
        executable.revalidate()?;
        if process.closed_claude_home()? != home || cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        Ok(observation)
    });
    Ok(ProviderInspection { result, process, display_models: None, display_usage: None })
}
