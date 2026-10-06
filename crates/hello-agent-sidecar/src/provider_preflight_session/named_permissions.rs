//! Invocation-only named policy candidate. Selection is not launch admission.
//! The runtime must match effective config/layers and tool enforcement before use.
use super::{discovery_arguments, ExpectedPermission, PreflightExchange};
#[cfg(test)]
pub(crate) use hello_agent_core::provider::vendor_observation::codex_preflight::named_config::{
    expected_write_policy, matches_configuration_layers, matches_write_policy,
};
use hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
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
    let filesystem = format!(
        "{{\":root\"=\"deny\",\":minimal\"=\"read\",{}=\"deny\",{}={{\".\"=\"{access}\",\".git\"=\"deny\",\".codex\"=\"deny\",\".claude\"=\"deny\",\".agents\"=\"deny\"}}}}",
        quote(home_text), quote(path),
    );
    for setting in [
        "default_permissions=\"hello-agent-snapshot\"".to_owned(),
        format!("permissions={{hello-agent-snapshot={{filesystem={filesystem},network={{enabled=false}}}}}}"),
    ] {
        args.extend([OsString::from("-c"), OsString::from(setting)]);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_layers_reject_extra_sources_flags_and_policy() {
        use serde_json::json;
        let expected = ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
        };
        let policy = json!({"filesystem":{":root":"deny"},"network":{"enabled":false}});
        let flags = json!({"model":"test-model","model_reasoning_effort":"medium","model_provider":"openai","approval_policy":"on-request","default_permissions":"hello-agent-snapshot","permissions":{"hello-agent-snapshot":policy},"features":{"hooks":false,"apps":false,"plugins":false}});
        let base = json!({"config":flags,"layers":[{"name":{"type":"sessionFlags"},"config":flags},{"name":{"type":"user","file":"/profile/config.toml"},"config":{"cli_auth_credentials_store":"file"}},{"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}]});
        assert!(matches_configuration_layers(
            &base,
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

    #[test]
    fn named_candidate_has_no_legacy_sandbox_or_profile_file_writes() {
        let expected = ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
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
            assert!(settings.contains(&"default_permissions=\"hello-agent-snapshot\""));
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
