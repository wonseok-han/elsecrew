//! Named tool policy candidate based on the local vendor-sandbox probe.
//! Matching reported configuration is not proof of OS enforcement or launch admission.
use serde_json::{json, Value};

pub(crate) const PROFILE: &str = "hello-agent-probe";

#[derive(Clone, Copy)]
pub(crate) enum WorkspaceMode {
    ReadOnly,
    Write,
}

pub(crate) fn profile(mode: WorkspaceMode) -> Value {
    json!({"filesystem":{":root":"deny",":minimal":"read",":workspace_roots":{".":match mode { WorkspaceMode::ReadOnly => "read", WorkspaceMode::Write => "write" }}},"network":{"enabled":false}})
}

pub(crate) fn configuration_arguments(mode: WorkspaceMode) -> Vec<std::ffi::OsString> {
    let permissions = format!("permissions={{{PROFILE}={}}}", match mode {
        WorkspaceMode::ReadOnly => "{filesystem={\":root\"=\"deny\",\":minimal\"=\"read\",\":workspace_roots\"={\".\"=\"read\"}},network={enabled=false}}",
        WorkspaceMode::Write => "{filesystem={\":root\"=\"deny\",\":minimal\"=\"read\",\":workspace_roots\"={\".\"=\"write\"}},network={enabled=false}}",
    });
    let mut args: Vec<std::ffi::OsString> =
        ["app-server", "--listen", "stdio://", "--strict-config"]
            .into_iter()
            .map(Into::into)
            .collect();
    for setting in [
        format!("default_permissions=\"{PROFILE}\""),
        "approval_policy=\"never\"".into(),
        permissions,
        "features.hooks=false".into(),
        "features.apps=false".into(),
        "features.plugins=false".into(),
    ] {
        args.extend(["-c".into(), setting.into()]);
    }
    args
}

pub(crate) fn matches(config: &Value, mode: WorkspaceMode) -> bool {
    config["default_permissions"] == PROFILE
        && config["approval_policy"] == "never"
        && config["sandbox_mode"].is_null()
        && config["sandbox_workspace_write"].is_null()
        && config["permissions"][PROFILE] == profile(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn denies_extra_paths_network_and_legacy_policy_mix() {
        let valid = json!({"default_permissions":PROFILE,"approval_policy":"never","permissions":{PROFILE:profile(WorkspaceMode::Write)}});
        assert!(matches(&valid, WorkspaceMode::Write));
        assert!(!matches(&valid, WorkspaceMode::ReadOnly));
        for (field, value) in [
            ("sandbox_mode", json!("workspace-write")),
            ("sandbox_workspace_write", json!({})),
            ("approval_policy", json!("on-request")),
        ] {
            let mut changed = valid.clone();
            changed[field] = value;
            assert!(!matches(&changed, WorkspaceMode::Write));
        }
        let mut changed = valid.clone();
        changed["permissions"][PROFILE]["network"]["enabled"] = json!(true);
        assert!(!matches(&changed, WorkspaceMode::Write));
        let mut changed = valid;
        changed["permissions"][PROFILE]["filesystem"]["/outside"] = json!("read");
        assert!(!matches(&changed, WorkspaceMode::Write));
    }
}
