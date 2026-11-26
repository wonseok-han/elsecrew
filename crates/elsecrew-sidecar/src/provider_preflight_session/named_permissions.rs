//! Invocation-only named policy candidate. Selection is not launch admission.
//! The runtime must match effective config/layers and tool enforcement before use.
use super::{discovery_arguments, ExpectedPermission, PreflightExchange};
#[cfg(test)]
pub(crate) use elsecrew_core::provider::vendor_observation::codex_preflight::named_config::{
    expected_write_policy, matches_configuration_layers, matches_write_policy,
};
use elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
use std::{ffi::OsString, io, path::Path};

/// Do not inherit :workspace: its default temp writes are outside our snapshot.
/// Fixed policy name is scoped to this process's flags, never user config files.
pub(crate) fn named_discovery_arguments(
    home: &Path,
    cwd: &Path,
    expected: &ExpectedProfile<'_>,
    permission: ExpectedPermission,
) -> io::Result<Vec<OsString>> {
    let invalid = || io::Error::other("invalid named permission boundary");
    let path = PreflightExchange::validate_thread_cwd(cwd).map_err(|_| invalid())?;
    let home_text = PreflightExchange::validate_thread_cwd(home).map_err(|_| invalid())?;
    // Literal filesystem keys must not become glob rules. Profiles and snapshots
    // cannot contain each other, regardless of rule specificity.
    if cwd.starts_with(home)
        || home.starts_with(cwd)
        || [path, home_text]
            .iter()
            .any(|value| value.contains(['*', '?', '[', ']', '{', '}']))
    {
        return Err(invalid());
    }
    let legacy = discovery_arguments(home, expected, permission)?;
    let mut args = legacy[..3].to_vec();
    for pair in legacy[3..].chunks_exact(2) {
        let value = pair[1].to_str().ok_or_else(invalid)?;
        if !value.starts_with("sandbox_mode=") && !value.starts_with("sandbox_workspace_write.") {
            args.extend_from_slice(pair);
        }
    }
    let access = match permission {
        ExpectedPermission::ReadOnly => "read",
        ExpectedPermission::WorkspaceWrite => "write",
    };
    // JSON string quoting is also TOML basic-string quoting for these validated
    // non-control path strings. Never interpolate raw paths into command syntax.
    let quote = |value: &str| serde_json::to_string(value).expect("string serialization");
    // Same answer as the expected policy (`linked_source_git`): a task
    // worktree may read its `.git` link and the repository's Git data.
    let source_git = elsecrew_core::provider::vendor_observation::codex_preflight::named_config::linked_source_git(cwd);
    let (git_link, source_rule) = match source_git.as_deref().and_then(Path::to_str) {
        Some(git) => {
            // Same developer folder as the expected policy (`developer_dir`).
            let developer = elsecrew_core::provider::vendor_observation::codex_preflight::named_config::developer_dir()
                .and_then(|path| path.to_str().map(|text| format!(",{}=\"read\"", quote(text))))
                .unwrap_or_default();
            ("read", format!(",{}=\"read\"{developer}", quote(git)))
        }
        None => ("deny", String::new()),
    };
    let filesystem = format!(
        "{{\":root\"=\"deny\",\":minimal\"=\"read\",{}=\"deny\",{}={{\".\"=\"{access}\",\".git\"=\"{git_link}\",\".codex\"=\"deny\",\".claude\"=\"deny\",\".agents\"=\"deny\"}}{source_rule}}}",
        quote(home_text), quote(path),
    );
    for setting in [
        "default_permissions=\"elsecrew-snapshot\"".to_owned(),
        format!("permissions={{elsecrew-snapshot={{filesystem={filesystem},network={{enabled=false}}}}}}"),
    ] {
        args.extend([OsString::from("-c"), OsString::from(setting)]);
    }
    Ok(args)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn named_layers_reject_extra_sources_flags_and_policy() {
        use serde_json::json;
        let expected = ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
            approval: elsecrew_core::provider::vendor_observation::codex_preflight::CodexApprovalPolicy::OnRequest,
        };
        let policy = json!({"filesystem":{":root":"deny"},"network":{"enabled":false}});
        let flags = json!({"model":"test-model","model_reasoning_effort":"medium","model_provider":"openai","approval_policy":"on-request","default_permissions":"elsecrew-snapshot","permissions":{"elsecrew-snapshot":policy},"features":{"hooks":false,"apps":false,"plugins":false}});
        let base = json!({"config":flags,"layers":[{"name":{"type":"sessionFlags"},"config":flags},{"name":{"type":"user","file":"/profile/config.toml"},"config":{"cli_auth_credentials_store":"file"}},{"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}]});
        assert!(matches_configuration_layers(
            &base,
            Path::new("/profile"),
            &expected,
            &policy,
            "on-request"
        ));
        // Switched-off built-in tools may be reported, but only as off.
        let tools = elsecrew_core::provider::vendor_observation::codex_preflight::UNSUPERVISED_TOOL_FEATURES;
        let mut tools_off = base.clone();
        for name in tools {
            tools_off["layers"][0]["config"]["features"][*name] = json!(false);
            tools_off["config"]["features"][*name] = json!(false);
        }
        tools_off["layers"][0]["config"]["web_search"] = json!("live");
        tools_off["config"]["web_search"] = json!("live");
        assert!(matches_configuration_layers(&tools_off, Path::new("/profile"), &expected, &policy, "on-request"));
        for pointer in ["/layers/0/config/features", "/config/features"] {
            let mut tool_on = tools_off.clone();
            tool_on.pointer_mut(pointer).unwrap()["computer_use"] = json!(true);
            assert!(!matches_configuration_layers(&tool_on, Path::new("/profile"), &expected, &policy, "on-request"), "{pointer}");
        }
        for pointer in ["/layers/0/config", "/config"] {
            let mut search_other = tools_off.clone();
            search_other.pointer_mut(pointer).unwrap()["web_search"] = json!("cached");
            assert!(!matches_configuration_layers(&search_other, Path::new("/profile"), &expected, &policy, "on-request"), "{pointer}");
        }
        let mut explicit_null = base.clone();
        explicit_null["layers"][1]["name"]["profile"] = serde_json::Value::Null;
        assert!(matches_configuration_layers(
            &explicit_null,
            Path::new("/profile"),
            &expected,
            &policy,
            "on-request"
        ));
        explicit_null["layers"][1]["name"]["profile"] = json!("other-profile");
        assert!(!matches_configuration_layers(
            &explicit_null,
            Path::new("/profile"),
            &expected,
            &policy,
            "on-request"
        ));
        for pointer in [
            "/layers/0/config/sandbox_mode",
            "/layers/1/config/hooks",
            "/layers/2/config/permissions",
            "/config/features/hooks",
            "/layers/0/disabledReason",
        ] {
            let mut changed = base.clone();
            let (parent, field) = pointer.rsplit_once('/').unwrap();
            changed.pointer_mut(parent).unwrap()[field] = json!(true);
            assert!(!matches_configuration_layers(
                &changed,
                Path::new("/profile"),
                &expected,
                &policy,
                "on-request"
            ));
        }
    }

    /// A real task worktree and its repository, canonical paths.
    pub(crate) fn real_worktree() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("source");
        std::fs::create_dir(&source).unwrap();
        let git = |dir: &Path, args: &[&str]| assert!(std::process::Command::new("/usr/bin/git")
            .arg("-C").arg(dir).args(args).output().unwrap().status.success());
        git(&source, &["init", "-q"]);
        git(&source, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "base"]);
        let workspace = root.join("work");
        git(&source, &["worktree", "add", "-q", "-b", "elsecrew/task-test", workspace.to_str().unwrap()]);
        (temp, source, workspace)
    }

    #[test]
    fn a_task_worktree_may_read_the_repository_git_data_and_nothing_else_opens() {
        let (_temp, source, workspace) = real_worktree();
        let expected = ExpectedProfile { model: "test-model", reasoning_effort: "medium", approval: elsecrew_core::provider::vendor_observation::codex_preflight::CodexApprovalPolicy::OnRequest };
        let profile = workspace.parent().unwrap().join("profile");
        let args = named_discovery_arguments(&profile, &workspace, &expected, ExpectedPermission::WorkspaceWrite).unwrap();
        let policy = args.last().unwrap().to_str().unwrap().to_owned();
        let source_git = format!("{:?}=\"read\"", source.join(".git").to_str().unwrap());
        assert!(policy.contains(&source_git), "{policy}");
        assert!(policy.contains("\".git\"=\"read\""), "{policy}");
        assert!(!policy.contains("\".git\"=\"write\""));
        let reported = expected_write_policy(&profile, &workspace);
        assert_eq!(reported["filesystem"][source.join(".git").to_str().unwrap()], "read");
        assert_eq!(reported["filesystem"][workspace.to_str().unwrap()][".git"], "read");
        // The developer folder's git is readable on both sides, never writable.
        if let Some(developer) = elsecrew_core::provider::vendor_observation::codex_preflight::named_config::developer_dir() {
            let key = developer.to_str().unwrap();
            assert!(policy.contains(&format!("{key:?}=\"read\"")), "{policy}");
            assert_eq!(reported["filesystem"][key], "read");
        }
        // A plain folder (v1 copy) keeps `.git` denied and adds nothing.
        let plain = workspace.parent().unwrap().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let args = named_discovery_arguments(&profile, &plain, &expected, ExpectedPermission::WorkspaceWrite).unwrap();
        let policy = args.last().unwrap().to_str().unwrap();
        assert!(policy.contains("\".git\"=\"deny\"") && !policy.contains("source"), "{policy}");
    }

    #[test]
    fn named_candidate_has_no_legacy_sandbox_or_profile_file_writes() {
        let expected = ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
            approval: elsecrew_core::provider::vendor_observation::codex_preflight::CodexApprovalPolicy::OnRequest,
        };
        for permission in [
            ExpectedPermission::ReadOnly,
            ExpectedPermission::WorkspaceWrite,
        ] {
            let args = named_discovery_arguments(
                Path::new("/private/profile"),
                Path::new("/private/snapshot"),
                &expected,
                permission,
            )
            .unwrap();
            let settings: Vec<_> = args[3..]
                .chunks_exact(2)
                .map(|pair| {
                    assert_eq!(pair[0], "-c");
                    pair[1].to_str().unwrap()
                })
                .collect();
            assert!(!settings.iter().any(|value| value.starts_with("sandbox_")));
            assert!(settings.contains(&"default_permissions=\"elsecrew-snapshot\""));
            let policy = settings.last().unwrap();
            assert!(policy.contains("network={enabled=false}"));
            assert!(policy.contains("\"/private/profile\"=\"deny\""));
            assert!(!policy.contains(":workspace_roots"));
            assert!(!policy.contains("extends"));
            assert!(
                policy.contains(if permission == ExpectedPermission::ReadOnly {
                    "\".\"=\"read\""
                } else {
                    "\".\"=\"write\""
                })
            );
        }
        for cwd in [
            "/",
            "/private/profile/work",
            "/private",
            "/private/snap*",
            "/private/profile",
            "relative",
        ] {
            assert!(named_discovery_arguments(
                Path::new("/private/profile"),
                Path::new(cwd),
                &expected,
                ExpectedPermission::ReadOnly
            )
            .is_err());
        }
    }
}
