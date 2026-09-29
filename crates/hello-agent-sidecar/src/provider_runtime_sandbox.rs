//! Offline macOS runtime candidate. No production provider admission.
//! Apply after the inert gate has closed inherited descriptors and been recorded.
use crate::provider_model_proxy::broker::Broker;
use std::ffi::{OsStr, OsString};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::process::Command;

const POLICY: &str = r#"(version 1)
(deny default)
; CoreFoundation needs OS user identity to synchronize preferences.
; This is not proof that managed preferences are visible or honored.
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo"))
(allow file-read-metadata)
(allow file-read*
    (literal "/")
    (subpath "/System") (subpath "/usr") (subpath "/bin") (subpath "/sbin")
    (literal "/dev/null") (literal "/dev/urandom")
    (literal (param "CLI")) (literal (param "HELPER"))
    (subpath (param "WORKSPACE")) (subpath (param "PROFILE")))
(allow process-exec)
(allow process-fork)
(allow signal (target self))
(allow sysctl-read)
(allow file-write-data (literal "/dev/null"))
(allow file-write* (subpath (param "PROFILE")))
(deny file-write* (literal (string-append (param "PROFILE") "/config.toml")))
; An invocation flag is not an OS file boundary. Do not load or modify cached
; plugins, including through children, even if a future vendor ignores the flag.
(deny file-read* file-write* (subpath (string-append (param "PROFILE") "/plugins")))
"#;

#[derive(Clone, Copy)]
pub(crate) enum WorkspaceAccess {
    ReadOnly,
    // Native offline candidate only; tools/approvals and vendor admission remain.
    Write,
}

const WORKSPACE_WRITE: &str = r#"
(allow file-write* (subpath (param "WORKSPACE")))
(deny file-write*
    (subpath (string-append (param "WORKSPACE") "/.git"))
    (subpath (string-append (param "WORKSPACE") "/.agents"))
    (subpath (string-append (param "WORKSPACE") "/.codex")))
"#;

const MODEL_PROXY: &str = r#"
(allow network-outbound (remote tcp4 (param "MODEL_PROXY")))
(allow file-read* (literal "/private/etc/ssl/cert.pem"))
"#;

const SYSTEM_CA: &str = "/private/etc/ssl/cert.pem";

pub(crate) fn command(
    executable: &Path,
    helper: &Path,
    args: &[OsString],
    home: &Path,
    workspace: &Path,
    access: WorkspaceAccess,
) -> io::Result<Command> {
    command_network(executable, helper, args, home, workspace, access, None)
}

/// A command and its owned endpoint move together into the lifecycle owner.
/// No raw Command/Child escape hatch: spawning always transfers the listener.
#[allow(dead_code)] // Pending native compatibility and product admission.
pub(crate) struct ModelProxyCommand {
    command: Command,
    broker: Broker,
}

#[allow(dead_code)]
impl ModelProxyCommand {
    pub(crate) fn launch(
        self,
        owner: &mut crate::provider_login_process::LoginProcess<'_>,
        release: impl FnOnce(&mut std::process::Child) -> io::Result<()>,
    ) -> Result<(), crate::SidecarError> {
        owner.launch_with_model_proxy(self.command, self.broker, release)
    }
}

/// Trusted backend candidate only. Upstream filtering/authentication is separate;
/// descendants share access to this endpoint. Not product activation authority.
#[allow(dead_code)]
pub(crate) fn command_with_model_proxy(
    executable: &Path,
    helper: &Path,
    args: &[OsString],
    home: &Path,
    workspace: &Path,
    access: WorkspaceAccess,
    listener: TcpListener,
) -> io::Result<ModelProxyCommand> {
    let ca = std::fs::symlink_metadata(SYSTEM_CA)?;
    if !ca.is_file()
        || ca.uid() != 0
        || ca.mode() & 0o022 != 0
        || ca.len() == 0
        || ca.len() > 1024 * 1024
    {
        return Err(io::Error::other("untrusted system CA bundle"));
    }
    let broker = Broker::new(listener, std::time::Instant::now())?;
    let mut command = command_network(
        executable,
        helper,
        args,
        home,
        workspace,
        access,
        Some(broker.listener()),
    )?;
    // Backend-only environment handoff. The secret is never a command argument
    // or IPC field. Tool descendants may inherit it; this is not role isolation.
    let url = broker.proxy_url()?;
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env(key, &url);
    }
    command.env("NO_PROXY", "").env("no_proxy", "");
    // Use the public system PEM bundle without granting keychain/Mach access.
    // Never inherit a caller-supplied CA path or disable TLS verification.
    command.env("CODEX_CA_CERTIFICATE", SYSTEM_CA);
    command
        .env("HOME", home)
        .env("CODEX_HOME", home)
        .env("TMPDIR", home.join("tmp"))
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    Ok(ModelProxyCommand { command, broker })
}

fn command_network(
    executable: &Path,
    helper: &Path,
    args: &[OsString],
    home: &Path,
    workspace: &Path,
    access: WorkspaceAccess,
    listener: Option<&TcpListener>,
) -> io::Result<Command> {
    let invalid = || io::Error::other("invalid offline runtime roots");
    for path in [executable, helper, home, workspace] {
        if !path.is_absolute()
            || path == Path::new("/")
            || path
                .components()
                .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
            || path.canonicalize()? != path
        {
            return Err(invalid());
        }
    }
    if home.starts_with(workspace)
        || workspace.starts_with(home)
        || executable.starts_with(home)
        || helper.starts_with(home)
        || executable.starts_with(workspace)
        || helper.starts_with(workspace)
    {
        return Err(invalid());
    }
    // Parameter values are individual argv values, never Scheme/shell source.
    let proxy = listener
        .map(|listener| match listener.local_addr()? {
            SocketAddr::V4(address)
                if address.ip() == &Ipv4Addr::LOCALHOST && address.port() != 0 =>
            {
                Ok(format!("localhost:{}", address.port()))
            }
            _ => Err(invalid()),
        })
        .transpose()?;
    let mut policy = match access {
        WorkspaceAccess::ReadOnly => POLICY.to_owned(),
        WorkspaceAccess::Write => format!("{POLICY}{WORKSPACE_WRITE}"),
    };
    if proxy.is_some() {
        policy.push_str(MODEL_PROXY);
    }
    let mut sandbox_args = vec![OsString::from("-p"), OsString::from(policy)];
    for (name, path) in [
        ("CLI", executable),
        ("HELPER", helper),
        ("PROFILE", home),
        ("WORKSPACE", workspace),
    ] {
        sandbox_args.push(OsString::from("-D"));
        let mut value = OsString::from(format!("{name}="));
        value.push(path);
        sandbox_args.push(value);
    }
    if let Some(proxy) = proxy {
        sandbox_args.push(OsString::from("-D"));
        sandbox_args.push(OsString::from(format!("MODEL_PROXY={proxy}")));
    }
    sandbox_args.push(executable.as_os_str().to_owned());
    sandbox_args.extend_from_slice(args);
    let references: Vec<&OsStr> = sandbox_args.iter().map(OsString::as_os_str).collect();
    crate::provider_login_gate::command(Path::new("/usr/bin/sandbox-exec"), &references, home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires outer macOS sandbox and local TCP; toy process only"]
    fn proxy_command_transfers_listener_to_process_owner() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let profile = crate::provider_profile::CodexProfileBinding::from_storage(&storage).unwrap();
        let home = profile.revalidate().unwrap();
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let command = command_with_model_proxy(
            Path::new("/bin/sleep"),
            Path::new("/bin/sleep"),
            &[OsString::from("30")],
            &home,
            &workspace,
            WorkspaceAccess::ReadOnly,
            listener,
        )
        .unwrap();
        assert!(TcpListener::bind(address).is_err());
        let proxy = command.broker.proxy_url().unwrap();
        let env: std::collections::BTreeMap<_, _> = command.command.get_envs().collect();
        assert!(env.get(OsStr::new("CODEX_CA_CERTIFICATE")) == Some(&Some(OsStr::new(SYSTEM_CA))));
        for key in ["HOME", "CODEX_HOME"] {
            assert!(env.get(OsStr::new(key)) == Some(&Some(home.as_os_str())));
        }
        assert!(env.get(OsStr::new("TMPDIR")) == Some(&Some(home.join("tmp").as_os_str())));
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            assert!(env.get(OsStr::new(key)) == Some(&Some(OsStr::new(&proxy))));
        }
        for key in ["NO_PROXY", "no_proxy"] {
            assert!(env.get(OsStr::new(key)) == Some(&Some(OsStr::new(""))));
        }
        assert!(!command
            .command
            .get_args()
            .any(|arg| arg == OsStr::new(&proxy)));
        let mut owner = crate::provider_login_process::LoginProcess::reserve(&profile).unwrap();
        command
            .launch(&mut owner, crate::provider_login_gate::release)
            .unwrap();
        assert!(TcpListener::bind(address).is_err());
        owner.finish().unwrap();
        assert!(TcpListener::bind(address).is_ok());
    }

    #[test]
    #[ignore = "requires outer macOS sandbox; local listeners only, no provider/account"]
    fn model_proxy_policy_allows_only_the_owned_listener() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("profile");
        let workspace = root.join("workspace");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        let allowed = TcpListener::bind("127.0.0.1:0").unwrap();
        let denied = TcpListener::bind("127.0.0.1:0").unwrap();
        let ipv6 = TcpListener::bind((
            std::net::Ipv6Addr::LOCALHOST,
            allowed.local_addr().unwrap().port(),
        ))
        .unwrap();
        let broad = TcpListener::bind("0.0.0.0:0").unwrap();
        let accept = |listener: &TcpListener| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(5))
                    }
                    Err(error) => panic!("local accept failed: {error}"),
                }
            }
        };
        for listener in [&allowed, &denied, &ipv6] {
            listener.set_nonblocking(true).unwrap();
            // Prove both endpoints work outside the sandbox first.
            let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            drop(accept(listener));
            drop(client);
        }
        let run = |host: &str, port: u16, proxy: Option<&TcpListener>| {
            let args: Vec<OsString> = [
                "-n",
                "-v",
                "-z",
                "-G",
                "1",
                "-w",
                "1",
                host,
                &port.to_string(),
            ]
            .into_iter()
            .map(OsString::from)
            .collect();
            let mut command = command_network(
                Path::new("/usr/bin/nc"),
                Path::new("/usr/bin/nc"),
                &args,
                &home,
                &workspace,
                WorkspaceAccess::ReadOnly,
                proxy,
            )
            .unwrap();
            command.stderr(std::process::Stdio::piped());
            let mut child = command.spawn().unwrap();
            crate::provider_login_gate::release(&mut child).unwrap();
            child.wait_with_output().unwrap()
        };
        assert!(
            !run("127.0.0.1", allowed.local_addr().unwrap().port(), None)
                .status
                .success()
        );
        let permitted = run(
            "127.0.0.1",
            allowed.local_addr().unwrap().port(),
            Some(&allowed),
        );
        assert!(
            permitted.status.success(),
            "local probe failed: {:?} {}",
            permitted.status,
            String::from_utf8_lossy(&permitted.stderr)
        );
        drop(accept(&allowed));
        assert!(!run(
            "127.0.0.1",
            denied.local_addr().unwrap().port(),
            Some(&allowed)
        )
        .status
        .success());
        assert_eq!(
            denied.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(
            !run("::1", ipv6.local_addr().unwrap().port(), Some(&allowed))
                .status
                .success()
        );
        assert_eq!(ipv6.accept().unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert!(command_with_model_proxy(
            Path::new("/usr/bin/nc"),
            Path::new("/usr/bin/nc"),
            &[],
            &home,
            &workspace,
            WorkspaceAccess::ReadOnly,
            broad
        )
        .is_err());
    }

    #[test]
    fn offline_roots_are_parameters_and_cannot_overlap() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("profile \" (allow default)");
        let workspace = root.join("workspace");
        let cli = root.join("cli");
        let helper = root.join("helper");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(&cli, b"cli").unwrap();
        std::fs::write(&helper, b"helper").unwrap();
        let command = command(
            &cli,
            &helper,
            &[OsString::from("one argument")],
            &home,
            &workspace,
            WorkspaceAccess::ReadOnly,
        )
        .unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(command.get_program(), "/bin/sh");
        assert_eq!(args[3], "/usr/bin/sandbox-exec");
        assert_eq!(args[5], POLICY);
        assert_eq!(
            args[11],
            OsString::from(format!("PROFILE={}", home.display()))
        );
        assert_eq!(args.last().copied(), Some(OsStr::new("one argument")));
        let invalid = |home: &Path, workspace: &Path| {
            super::command(
                &cli,
                &helper,
                &[],
                home,
                workspace,
                WorkspaceAccess::ReadOnly,
            )
            .is_err()
        };
        assert!(invalid(&home, &home));
        assert!(invalid(&root, &workspace));
        assert!(invalid(Path::new("/"), &workspace));
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&home, &alias).unwrap();
        assert!(invalid(&alias, &workspace));
        assert!(invalid(Path::new("relative"), &workspace));
        let nested_cli = workspace.join("cli");
        std::fs::write(&nested_cli, b"cli").unwrap();
        assert!(super::command(
            &nested_cli,
            &helper,
            &[],
            &home,
            &workspace,
            WorkspaceAccess::Write
        )
        .is_err());
    }

    #[test]
    #[ignore = "requires outer macOS sandbox; temporary fixture only, no provider/account"]
    fn workspace_write_policy_keeps_original_and_control_files_protected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        let home = root.join("profile");
        let original = root.join("original");
        for path in [&workspace, &home] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(&original, b"original").unwrap();
        std::fs::write(home.join("config.toml"), b"original").unwrap();
        let plugin = home.join("plugins/cache/payload");
        std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
        std::fs::write(&plugin, b"#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&plugin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&plugin, home.join("plugin-alias")).unwrap();
        for name in [".git", ".agents", ".codex"] {
            std::fs::create_dir(workspace.join(name)).unwrap();
            std::fs::write(workspace.join(name).join("protected"), b"original").unwrap();
        }
        let run = |script: &str| {
            let args: Vec<OsString> = [
                OsStr::new("-c"),
                OsStr::new(script),
                OsStr::new("write-probe"),
                workspace.as_os_str(),
                home.as_os_str(),
                original.as_os_str(),
            ]
            .into_iter()
            .map(OsString::from)
            .collect();
            let mut cmd = super::command(
                Path::new("/bin/sh"),
                Path::new("/bin/sh"),
                &args,
                &home,
                &workspace,
                WorkspaceAccess::Write,
            )
            .unwrap();
            let mut child = cmd.spawn().unwrap();
            crate::provider_login_gate::release(&mut child).unwrap();
            child.wait_with_output().unwrap()
        };
        for script in [
            "printf changed > \"$1/file\"",
            "/bin/sh -c 'printf child > \"$1/child\"' child \"$1\"",
            "printf scratch > \"$2/scratch\"",
        ] {
            assert!(
                run(script).status.success(),
                "allowed write rejected: {script}"
            );
        }
        assert_eq!(std::fs::read(workspace.join("file")).unwrap(), b"changed");
        for script in [
            "printf changed > \"$3\"",
            "/bin/cat \"$2/plugins/cache/payload\"",
            "/bin/cat \"$2/plugin-alias\"",
            "\"$2/plugins/cache/payload\"",
            "/bin/sh -c '/bin/cat \"$1/plugins/cache/payload\"' child \"$2\"",
            "printf changed > \"$2/plugins/cache/payload\"",
            "printf new > \"$2/plugins/new\"",
            "/bin/mv \"$2/plugins\" \"$2/moved-plugins\"",
            "printf changed > \"$2/config.toml\"",
            "printf changed > \"$1/.git/protected\"",
            "printf changed > \"$1/.agents/protected\"",
            "printf changed > \"$1/.codex/protected\"",
            "/bin/mv \"$1/.git\" \"$1/moved\"",
            "/bin/ln \"$3\" \"$1/shared\"",
            "/bin/ln \"$2/config.toml\" \"$1/config-alias\" && printf changed > \"$1/config-alias\"",
            "/bin/ln -s \"$3\" \"$1/source-link\" && printf changed > \"$1/source-link\"",
        ] {
            assert!(!run(script).status.success(), "forbidden write succeeded: {script}");
        }
        assert_eq!(std::fs::read(original).unwrap(), b"original");
        assert_eq!(std::fs::read(plugin).unwrap(), b"#!/bin/sh\nexit 0\n");
        assert_eq!(
            std::fs::read(home.join("config.toml")).unwrap(),
            b"original"
        );
        for name in [".git", ".agents", ".codex"] {
            assert_eq!(
                std::fs::read(workspace.join(name).join("protected")).unwrap(),
                b"original"
            );
        }
    }
}
