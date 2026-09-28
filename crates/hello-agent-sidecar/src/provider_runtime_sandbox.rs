//! Offline macOS runtime candidate. No production provider admission.
//! Apply after the inert gate has closed inherited descriptors and been recorded.
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path};
use std::process::Command;

const POLICY: &str = r#"(version 1)
(deny default)
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
"#;

#[derive(Clone, Copy)]
pub(crate) enum WorkspaceAccess {
    ReadOnly,
    // Not yet selected by a native run: tools/approvals and vendor admission remain.
    #[allow(dead_code)]
    Write,
}

const WORKSPACE_WRITE: &str = r#"
(allow file-write* (subpath (param "WORKSPACE")))
(deny file-write*
    (subpath (string-append (param "WORKSPACE") "/.git"))
    (subpath (string-append (param "WORKSPACE") "/.agents"))
    (subpath (string-append (param "WORKSPACE") "/.codex")))
"#;

pub(crate) fn command(
    executable: &Path,
    helper: &Path,
    args: &[OsString],
    home: &Path,
    workspace: &Path,
    access: WorkspaceAccess,
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
    let policy = match access {
        WorkspaceAccess::ReadOnly => POLICY.to_owned(),
        WorkspaceAccess::Write => format!("{POLICY}{WORKSPACE_WRITE}"),
    };
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
    sandbox_args.push(executable.as_os_str().to_owned());
    sandbox_args.extend_from_slice(args);
    let references: Vec<&OsStr> = sandbox_args.iter().map(OsString::as_os_str).collect();
    crate::provider_login_gate::command(Path::new("/usr/bin/sandbox-exec"), &references, home)
}

#[cfg(test)]
mod tests {
    use super::*;

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
