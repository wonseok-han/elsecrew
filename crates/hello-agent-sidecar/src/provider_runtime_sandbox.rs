//! Offline, read-only macOS runtime candidate. No production provider admission.
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

pub(crate) fn command(
    executable: &Path,
    helper: &Path,
    args: &[OsString],
    home: &Path,
    workspace: &Path,
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
    {
        return Err(invalid());
    }
    // Parameter values are individual argv values, never Scheme/shell source.
    let mut sandbox_args = vec![OsString::from("-p"), OsString::from(POLICY)];
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
        assert!(super::command(&cli, &helper, &[], &home, &home).is_err());
        assert!(super::command(&cli, &helper, &[], &root, &workspace).is_err());
        assert!(super::command(&cli, &helper, &[], Path::new("/"), &workspace).is_err());
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&home, &alias).unwrap();
        assert!(super::command(&cli, &helper, &[], &alias, &workspace).is_err());
        assert!(super::command(&cli, &helper, &[], Path::new("relative"), &workspace).is_err());
    }
}
