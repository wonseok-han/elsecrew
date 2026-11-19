//! Reported named settings; not process or filesystem enforcement authority.
use super::{ExpectedPermission, ExpectedProfile};
use serde_json::{json, Value};
use std::path::Path;

/// `<repo>/.git` when `workspace` is a linked worktree whose `.git` file and
/// the repository's back link name each other; otherwise none. One answer
/// for the OS sandbox, the Codex policy and the Claude settings: agents may
/// read the repository's Git data (user decision 2026-11-18), never write it.
pub fn linked_source_git(workspace: &Path) -> Option<std::path::PathBuf> {
    let link = workspace.join(".git");
    let metadata = std::fs::symlink_metadata(&link).ok()?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return None;
    }
    let text = std::fs::read_to_string(&link).ok()?;
    let record = Path::new(text.strip_prefix("gitdir: ")?.strip_suffix('\n')?);
    let worktrees = record.parent()?;
    let git_dir = worktrees.parent()?;
    if worktrees.file_name()? != "worktrees"
        // `<repo>/.git`, or a plain folder's shadow history `<name>.git`.
        || !git_dir.file_name()?.to_str()?.ends_with(".git")
        || git_dir.canonicalize().ok()? != git_dir
        || workspace.starts_with(git_dir.parent()?)
        || git_dir.to_str()?.contains(['*', '?', '[', ']', '{', '}'])
    {
        return None;
    }
    let back = std::fs::read_to_string(record.join("gitdir")).ok()?;
    (Path::new(back.strip_suffix('\n')?) == link).then(|| git_dir.to_owned())
}

/// The active Apple developer folder (`xcode-select`'s link) when it is an
/// installed folder holding `usr/bin/git`. In a task worktree, Codex runs
/// that git directly: `/usr/bin/git` is an `xcrun` shim that needs the link,
/// the developer folder and a per-user cache, none of which the sandbox opens.
pub fn developer_dir() -> Option<std::path::PathBuf> {
    let dir = std::fs::canonicalize("/private/var/db/xcode_select_link").ok()?;
    let text = dir.to_str()?;
    // Command Line Tools, or an Xcode app's own developer folder only.
    let xcode = dir.parent().filter(|parent| parent.file_name() == Some("Contents".as_ref()))
        .and_then(Path::parent)
        .filter(|app| app.extension() == Some("app".as_ref()) && app.parent() == Some(Path::new("/Applications")))
        .is_some();
    let installed = dir == Path::new("/Library/Developer/CommandLineTools") || (xcode && dir.file_name() == Some("Developer".as_ref()));
    if !installed || text.contains(['*', '?', '[', ']', '{', '}', '"', '\\', '(', ')']) || !dir.join("usr/bin/git").is_file() {
        return None;
    }
    Some(dir)
}

/// The Git directory a project folder uses: its own `.git` folder, or the
/// directory its small `.git` link file names (a shadow history). Compared
/// with `linked_source_git` to tie a worktree to its own project.
pub fn source_git_dir(source: &Path) -> Option<std::path::PathBuf> {
    let link = source.join(".git");
    let metadata = std::fs::symlink_metadata(&link).ok()?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        return Some(link);
    }
    if !metadata.is_file() || metadata.len() > 4096 {
        return None;
    }
    let text = std::fs::read_to_string(&link).ok()?;
    let target = std::path::PathBuf::from(text.strip_prefix("gitdir: ")?.strip_suffix('\n')?);
    (target.canonicalize().ok()? == target).then_some(target)
}

pub fn expected_write_policy(home: &Path, workspace: &Path) -> Value {
    let (Some(home), Some(workspace_text)) = (home.to_str(), workspace.to_str()) else {
        return Value::Null;
    };
    let mut policy = json!({"filesystem":{
        ":root":"deny", ":minimal":"read", home:"deny",
        workspace_text:{".":"write",".git":"deny",".codex":"deny",".claude":"deny",".agents":"deny"}
    },"network":{"enabled":false}});
    // A task worktree: its `.git` link and the repository's Git data are
    // readable so `git status/diff/log` work; writes stay denied.
    if let Some(source_git) = linked_source_git(workspace).and_then(|path| path.to_str().map(str::to_owned)) {
        policy["filesystem"][workspace_text][".git"] = json!("read");
        policy["filesystem"][source_git] = json!("read");
        if let Some(developer) = developer_dir().and_then(|path| path.to_str().map(str::to_owned)) {
            policy["filesystem"][developer] = json!("read");
        }
    }
    policy
}

pub fn matches_write_policy(value: &Value, home: &Path, workspace: &Path) -> bool {
    matches_policy(value, &expected_write_policy(home, workspace))
}

fn matches_policy(value: &Value, expected: &Value) -> bool {
    let mut normalized = value.clone();
    // Observed 0.159.2 serializer defaults, not an arbitrary null-field bypass.
    for (object, keys) in [
        ("", &["description", "extends", "workspace_roots"][..]),
        ("filesystem", &["glob_scan_max_depth"][..]),
        (
            "network",
            &[
                "allow_local_binding",
                "allow_upstream_proxy",
                "dangerously_allow_all_unix_sockets",
                "dangerously_allow_non_loopback_proxy",
                "domains",
                "enable_socks5",
                "enable_socks5_udp",
                "mitm",
                "mode",
                "proxy_url",
                "socks_url",
                "unix_sockets",
            ][..],
        ),
    ] {
        let target = if object.is_empty() {
            &mut normalized
        } else {
            &mut normalized[object]
        };
        let Some(map) = target.as_object_mut() else {
            return false;
        };
        for key in keys {
            if map.get(*key).is_some_and(Value::is_null) {
                map.remove(*key);
            }
        }
    }
    normalized == *expected
}

/// Reported layer provenance only. This does not prove that startup extensions
/// never ran, nor authorize a process/turn. The RPC owner must validate framing.
pub fn matches_configuration_layers(
    result: &serde_json::Value,
    home: &Path,
    expected: &ExpectedProfile<'_>,
    expected_policy: &serde_json::Value,
    approval: &str,
) -> bool {
    check_configuration_layers(result, home, expected, expected_policy, approval).is_ok()
}

fn check_configuration_layers(
    result: &Value,
    home: &Path,
    expected: &ExpectedProfile<'_>,
    expected_policy: &Value,
    approval: &str,
) -> Result<(), &'static str> {
    use serde_json::json;
    if !matches!(approval, "never" | "on-request") {
        return Err("approval expectation");
    }
    let flags = json!({
        "model":expected.model,"model_reasoning_effort":expected.reasoning_effort,
        "model_provider":"openai","approval_policy":approval,
        "default_permissions":"elsecrew-snapshot",
        "permissions":{"elsecrew-snapshot":expected_policy},
        "features":{"hooks":false,"apps":false,"plugins":false}
    });
    let Some(layers) = result["layers"].as_array() else {
        return Err("layers shape");
    };
    if layers.len() != 3 {
        return Err("layers count");
    }
    let Some(home) = home.to_str() else {
        return Err("profile path");
    };
    let mut seen = [false; 3];
    for layer in layers {
        if !layer["disabledReason"].is_null() {
            return Err("disabled layer");
        }
        let (index, expected_name, expected_config) = match layer["name"]["type"].as_str() {
            Some("sessionFlags") => (0, json!({"type":"sessionFlags"}), flags.clone()),
            Some("user") => (
                1,
                json!({"type":"user","file":format!("{home}/config.toml")}),
                json!({"cli_auth_credentials_store":"file"}),
            ),
            Some("system") => (
                2,
                json!({"type":"system","file":"/etc/codex/config.toml"}),
                json!({}),
            ),
            _ => return Err("layer source"),
        };
        let mut name = layer["name"].clone();
        // Codex serializes the absent user config profile as explicit null.
        // A selected profile or any other extra source field still differs.
        if index == 1 && name.get("profile").is_some_and(Value::is_null) {
            if let Some(object) = name.as_object_mut() {
                object.remove("profile");
            }
        }
        if name != expected_name {
            return Err("layer identity");
        }
        let mut layer_config = layer["config"].clone();
        if index == 0 {
            super::strip_unsupervised_tool_flags(&mut layer_config)?;
            super::super::codex_profile_config::strip_isolation_flag(
                &mut layer_config, &result["config"], &[Path::new(home)],
            )?;
        }
        let matches = if index == 1 {
            let reported_flags = layers.iter()
                .find(|candidate| candidate["name"]["type"] == "sessionFlags")
                .map(|candidate| &candidate["config"]).ok_or("session flags missing")?;
            super::super::codex_profile_config::ProfileConfig::matches_observed_layer(
                &layer_config, reported_flags, &result["config"], Path::new(home),
            )
        } else {
            layer_config == expected_config
        };
        if !matches {
            return Err(match index {
                0 => "session flags",
                1 => "user layer",
                _ => "system layer",
            });
        }
        if std::mem::replace(&mut seen[index], true) {
            return Err("duplicate layer");
        }
    }
    let config = &result["config"];
    for key in [
        "model",
        "model_provider",
        "model_reasoning_effort",
        "approval_policy",
        "default_permissions",
    ] {
        if config[key] != flags[key] {
            return Err("effective model or permission selector");
        }
    }
    for feature in ["hooks", "apps", "plugins"] {
        if config["features"][feature] != false {
            return Err("effective extensions");
        }
    }
    if !super::unsupervised_tools_off(config) {
        return Err("effective extensions");
    }
    if !config["sandbox_mode"].is_null() || !config["sandbox_workspace_write"].is_null() {
        return Err("legacy sandbox settings");
    }
    Ok(())
}

pub(super) fn match_configuration(
    input: &super::BoundedObservation,
    home: &Path,
    workspace: &Path,
    expected: &ExpectedProfile<'_>,
    permission: ExpectedPermission,
) -> Result<(), &'static str> {
    let mut policy = expected_write_policy(home, workspace);
    if permission == ExpectedPermission::ReadOnly {
        policy["filesystem"][workspace.to_str().ok_or("workspace path")?]["."] = json!("read");
    }
    let result = input.json().map_err(|_| "configuration JSON")?;
    if let Some(layers) = result["layers"].as_array() {
        for layer in layers {
            if layer["name"]["type"] == "sessionFlags" {
                super::super::codex_profile_config::strip_isolation_flag(
                    &mut layer["config"].clone(), &result["config"], &[home, workspace],
                )?;
            }
        }
    }
    check_configuration_layers(&result, home, expected, &policy, "on-request")?;
    if !matches_policy(
        &result["config"]["permissions"]["elsecrew-snapshot"],
        &policy,
    ) {
        return Err("effective filesystem or network policy");
    }
    Ok(())
}
