//! Reported initialization, not authentication, OS confinement, or launch authority.
//! https://code.claude.com/docs/en/agent-sdk/typescript#sdksystemmessage
//! Effort is normally absent in CLI init; do not infer reasoning configuration.
use super::{claude_events::SessionProjection, BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::collections::BTreeSet;

pub struct ExpectedInit<'a> {
    pub cwd: &'a str,
    pub model: &'a str,
    pub permission_mode: &'a str,
    pub tools: &'a [&'a str],
}

pub struct InitObservation {
    session_id: String,
    pub organization_guard: bool,
    pub interrupt_receipt: bool,
    pub interrupt_cancel_queued: bool,
}

impl InitObservation {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn into_projection(self) -> Result<SessionProjection, ProviderError> {
        SessionProjection::new(&self.session_id)
    }
}

#[derive(Deserialize)]
struct Init {
    #[serde(rename = "type")]
    kind: String,
    subtype: String,
    uuid: String,
    session_id: String,
    cwd: String,
    model: String,
    #[serde(rename = "permissionMode")]
    permission_mode: String,
    tools: Vec<String>,
    mcp_servers: Vec<Box<RawValue>>,
    skills: Vec<String>,
    plugins: Vec<Box<RawValue>>,
    #[serde(default)]
    agents: Vec<String>,
    #[serde(default)]
    plugin_errors: Vec<Box<RawValue>>,
    #[serde(default)]
    capabilities: Vec<String>,
}

fn invalid() -> ProviderError {
    ProviderError("Claude initialization did not match the requested session".into())
}
fn name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

pub fn observe(
    input: &BoundedObservation,
    expected: &ExpectedInit<'_>,
) -> Result<InitObservation, ProviderError> {
    observe_with_stage(input, expected, &mut "init expected")
}

pub(super) fn observe_with_stage(
    input: &BoundedObservation,
    expected: &ExpectedInit<'_>,
    stage: &mut &'static str,
) -> Result<InitObservation, ProviderError> {
    observe_with_guard_policy(input, expected, stage, false)
}

pub(super) fn observe_with_guard_policy(
    input: &BoundedObservation,
    expected: &ExpectedInit<'_>,
    stage: &mut &'static str,
    allow_organization_guard: bool,
) -> Result<InitObservation, ProviderError> {
    *stage = "init expected";
    // Local syntax only. The owner must bind the real snapshot directory identity.
    let path = std::path::Path::new(expected.cwd);
    if !path.is_absolute()
        || expected.cwd.len() > 16 * 1024
        || expected.cwd.chars().any(char::is_control)
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        || !name(expected.model)
        || !name(expected.permission_mode)
        || expected.tools.len() > 128
        || expected.tools.iter().any(|tool| !name(tool))
    {
        return Err(invalid());
    }
    *stage = "init shape";
    let wire: Init = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
    let expected_tools: BTreeSet<_> = expected.tools.iter().copied().collect();
    let tools: BTreeSet<_> = wire.tools.iter().map(String::as_str).collect();
    *stage = "init identity";
    if wire.kind != "system"
        || wire.subtype != "init"
        || !name(&wire.uuid)
        || !name(&wire.session_id)
    { return Err(invalid()); }
    *stage = "init cwd";
    if wire.cwd != expected.cwd { return Err(invalid()); }
    *stage = "init model";
    if wire.model != expected.model { return Err(invalid()); }
    *stage = "init permission";
    if wire.permission_mode != expected.permission_mode { return Err(invalid()); }
    *stage = "init tools";
    if tools != expected_tools
        || tools.len() != wire.tools.len()
        || expected_tools.len() != expected.tools.len()
    { return Err(invalid()); }
    *stage = "init mcp";
    if !wire.mcp_servers.is_empty() { return Err(invalid()); }
    *stage = "init skills";
    if !wire.skills.is_empty() { return Err(invalid()); }
    *stage = "init plugins";
    // Only the native signed-provider owner opts in. Registration is an
    // observation, never authentication or authority to bypass tool checks.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Guard { name: String, path: String, source: String }
    let organization_guard = allow_organization_guard && wire.plugins.len() == 1
        && serde_json::from_str::<Guard>(wire.plugins[0].get()).is_ok_and(|plugin|
            plugin.name == "cc-plugin-sec-default" && plugin.path == "builtin"
                && plugin.source == "cc-plugin-sec-default@builtin");
    if !wire.plugins.is_empty() && !organization_guard {
        // Classify only local labels; never log provider names or paths.
        let plugins: Vec<serde_json::Value> = wire.plugins.iter()
            .map(|raw| serde_json::from_str(raw.get()).unwrap_or(serde_json::Value::Null))
            .collect();
        *stage = if plugins.iter().all(|plugin| {
            plugin["path"] == "builtin" && matches!(plugin["source"].as_str(),
                Some("cc-plugin-agents-md@builtin" | "cc-plugin-plugin-authoring@builtin"))
        }) {
            "init plugins disabled builtins"
        } else if plugins.iter().all(|plugin| plugin["path"] == "builtin"
            && plugin["source"] == "cc-plugin-sec-default@builtin") {
            // Official organization guard cannot be disabled by a user. This
            // is diagnosis only, not trust in the provider's registration claim.
            "init plugins organization guard"
        } else if plugins.iter().all(|plugin| plugin["path"] == "builtin"
            && plugin["source"] == "cc-plugin-telemetry@builtin") {
            "init plugins telemetry"
        } else if plugins.iter().all(|plugin| plugin["path"] == "builtin") {
            "init plugins other builtins"
        } else {
            "init plugins other registration"
        };
        return Err(invalid());
    }
    *stage = "init agents";
    if !wire.agents.is_empty() { return Err(invalid()); }
    *stage = "init plugin errors";
    if !wire.plugin_errors.is_empty() { return Err(invalid()); }
    *stage = "init capabilities";
    if wire.capabilities.len() > 128
        || wire.capabilities.iter().any(|c| !name(c))
    {
        return Err(invalid());
    }
    // Capabilities are an open set. Unknown names do not enable known behavior.
    Ok(InitObservation {
        session_id: wire.session_id,
        organization_guard,
        interrupt_receipt: wire
            .capabilities
            .iter()
            .any(|c| c == "interrupt_receipt_v1"),
        interrupt_cancel_queued: wire
            .capabilities
            .iter()
            .any(|c| c == "interrupt_cancel_queued_v1"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn expected() -> ExpectedInit<'static> {
        ExpectedInit {
            cwd: test_path!("/managed/snapshot"),
            model: "selected-model",
            permission_mode: "default",
            tools: &["Read", "Edit"],
        }
    }
    fn init() -> Value {
        json!({"type":"system","subtype":"init","uuid":"init-a","session_id":"session-a",
            "cwd":test_path!("/managed/snapshot"),"model":"selected-model","permissionMode":"default",
            "tools":["Edit","Read"],"mcp_servers":[],"skills":[],"plugins":[]})
    }
    fn input(value: &Value) -> BoundedObservation {
        BoundedObservation::new(value.to_string().into_bytes()).unwrap()
    }
    #[test]
    fn native_guard_policy_is_exact_and_opt_in() {
        let guard = json!({"name":"cc-plugin-sec-default","path":"builtin","source":"cc-plugin-sec-default@builtin"});
        let mut wire = init();
        wire["plugins"] = json!([guard.clone()]);
        assert!(observe(&input(&wire), &expected()).is_err());
        assert!(
            observe_with_guard_policy(&input(&wire), &expected(), &mut "", true)
                .unwrap()
                .organization_guard
        );
        for plugins in [
            json!([guard.clone(), guard.clone()]),
            json!([guard.clone(),{"path":"builtin"}]),
            json!([{"name":"other","path":"builtin","source":"cc-plugin-sec-default@builtin"}]),
            json!([{"name":"cc-plugin-sec-default","path":test_path!("/tmp/plugin"),"source":"cc-plugin-sec-default@builtin"}]),
            json!([{"name":"cc-plugin-sec-default","path":"builtin","source":"cc-plugin-sec-default@builtin","extra":true}]),
            json!([{"path":"builtin","source":"cc-plugin-sec-default@builtin"}]),
        ] {
            wire["plugins"] = plugins;
            assert!(observe_with_guard_policy(&input(&wire), &expected(), &mut "", true).is_err());
        }
        wire = init();
        assert!(
            !observe_with_guard_policy(&input(&wire), &expected(), &mut "", true)
                .unwrap()
                .organization_guard
        );
        wire["plugins"] = json!([guard]);
        wire["tools"] = json!(["Bash"]);
        assert!(observe_with_guard_policy(&input(&wire), &expected(), &mut "", true).is_err());
    }

    #[test]
    fn mismatch_stage_contains_only_fixed_labels() {
        for (key, value, stage) in [
            ("cwd", json!("PRIVATE"), "init cwd"),
            ("model", json!("PRIVATE"), "init model"),
            ("permissionMode", json!("PRIVATE"), "init permission"),
            ("tools", json!(["PRIVATE"]), "init tools"),
            ("skills", json!(["PRIVATE"]), "init skills"),
            ("agents", json!(["PRIVATE"]), "init agents"),
            ("mcp_servers", json!([{"name":"PRIVATE"}]), "init mcp"),
            (
                "plugins",
                json!([{"name":"PRIVATE"}]),
                "init plugins other registration",
            ),
            (
                "plugins",
                json!([{"name":"PRIVATE","path":"builtin","source":"cc-plugin-agents-md@builtin"}]),
                "init plugins disabled builtins",
            ),
            (
                "plugins",
                json!([{"name":"PRIVATE","path":"builtin","source":"PRIVATE"}]),
                "init plugins other builtins",
            ),
            (
                "plugins",
                json!([{"name":"PRIVATE","path":"builtin","source":"cc-plugin-sec-default@builtin"}]),
                "init plugins organization guard",
            ),
            (
                "plugins",
                json!([{"path":"builtin","source":"cc-plugin-telemetry@builtin"}]),
                "init plugins telemetry",
            ),
            (
                "plugins",
                json!([{"path":"PRIVATE","source":"cc-plugin-sec-default@builtin"}]),
                "init plugins other registration",
            ),
            (
                "plugins",
                json!([{"path":"builtin","source":"cc-plugin-sec-default@builtin"},{"path":"builtin","source":"PRIVATE"}]),
                "init plugins other builtins",
            ),
            ("plugins", json!([null]), "init plugins other registration"),
            (
                "plugin_errors",
                json!([{"message":"PRIVATE"}]),
                "init plugin errors",
            ),
            ("uuid", json!(""), "init identity"),
            ("capabilities", json!([""]), "init capabilities"),
            ("model", json!(null), "init shape"),
        ] {
            let mut wire = init();
            wire[key] = value;
            let mut observed = "";
            assert!(observe_with_stage(&input(&wire), &expected(), &mut observed).is_err());
            assert_eq!(observed, stage);
        }
    }

    #[test]
    fn capabilities_are_feature_detected_not_version_pinned() {
        let baseline = observe(&input(&init()), &expected()).unwrap();
        assert!(!baseline.interrupt_receipt && !baseline.interrupt_cancel_queued);
        assert_eq!(baseline.session_id(), "session-a");
        let mut wire = init();
        wire["claude_code_version"] = json!("future-version");
        wire["capabilities"] = json!([
            "interrupt_receipt_v1",
            "interrupt_cancel_queued_v1",
            "new_feature"
        ]);
        let observation = observe(&input(&wire), &expected()).unwrap();
        assert!(observation.interrupt_receipt && observation.interrupt_cancel_queued);
        assert!(observation.into_projection().is_ok());
    }
    #[test]
    fn mismatched_target_or_loaded_extensions_are_not_matched() {
        for (key, value) in [
            ("cwd", json!(test_path!("/other"))),
            ("model", json!("other")),
            ("permissionMode", json!("bypassPermissions")),
            ("tools", json!(["Read", "Edit", "Bash"])),
            ("tools", json!(["Read", "Edit", "Read"])),
            ("mcp_servers", json!([{"name":"PRIVATE"}])),
            ("plugins", json!([{"path":"PRIVATE"}])),
            ("skills", json!(["PRIVATE"])),
            ("agents", json!(["PRIVATE"])),
            ("plugin_errors", json!([{"message":"PRIVATE"}])),
            ("capabilities", json!(null)),
            ("session_id", json!("")),
        ] {
            let mut wire = init();
            wire[key] = value;
            let error = observe(&input(&wire), &expected())
                .err()
                .expect("must reject");
            assert!(!error.to_string().contains("PRIVATE"));
        }
        let mut wire = init();
        wire.as_object_mut().unwrap().remove("mcp_servers");
        assert!(observe(&input(&wire), &expected()).is_err());
    }
}
