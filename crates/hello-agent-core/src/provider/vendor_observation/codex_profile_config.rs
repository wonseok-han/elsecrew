//! Bounded app-profile configuration parsing, not launch authorization.
//! Trust records are recognized separately so callers cannot mistake parsing
//! success for permission to load project-local configuration.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Component, Path};

pub const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_PROJECTS: usize = 128;

/// Candidate validation for reported disabled project layers. Not admission:
/// callers still validate all other layers, effective settings and RPC order.
/// Do not use a disabled reason alone as evidence of startup isolation.
pub fn validate_disabled_project_layers(
    result: &serde_json::Value,
    home: &Path,
    cwd: &Path,
) -> Result<(), &'static str> {
    let layers = result["layers"].as_array().ok_or("layers shape")?;
    if layers.len() > 259 {
        return Err("layers budget");
    }
    let mut seen = std::collections::BTreeSet::new();
    let flags: Vec<_> = layers
        .iter()
        .filter(|layer| layer["name"]["type"] == "sessionFlags")
        .collect();
    if flags.len() != 1 {
        return Err("session flags identity");
    }
    let flags = &flags[0]["config"];
    if flags.get("projects").is_none() {
        return Err("isolation flags absent");
    }
    strip_isolation_flag(&mut flags.clone(), &result["config"], &[home, cwd])?;
    for feature in ["hooks", "apps", "plugins"] {
        if flags["features"][feature] != false || result["config"]["features"][feature] != false {
            return Err("extensions enabled");
        }
    }
    for layer in layers
        .iter()
        .filter(|layer| layer["name"]["type"] == "project")
    {
        let folder = layer["name"]["dotCodexFolder"]
            .as_str()
            .ok_or("project source path")?;
        let expected = serde_json::json!({"type":"project","dotCodexFolder":folder});
        if layer["name"] != expected || !layer["config"].is_object() {
            return Err("project source shape");
        }
        // Exact textual paths avoid aliases such as ../, repeated separators,
        // or a sibling project that happens to have a stored trust override.
        if ![home, cwd].iter().any(|base| {
            base.ancestors()
                .any(|ancestor| ancestor.join(".codex").to_str() == Some(folder))
        }) {
            return Err("project source outside launch ancestry");
        }
        if !seen.insert(folder) {
            return Err("duplicate project source");
        }
        let reason = layer["disabledReason"]
            .as_str()
            .ok_or("project layer enabled")?;
        if reason.trim().is_empty() || reason.len() > 2048 || reason.chars().any(char::is_control) {
            return Err("project disabled reason shape");
        }
    }
    Ok(())
}

/// Disposable diagnostic probes may count bounded warnings while collecting
/// the following settings and marker evidence. Not product warning admission:
/// callers must not infer safety from a warning or its English wording.
pub fn is_probe_configuration_warning(input: &super::super::BoundedObservation) -> bool {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Note {
        method: String,
        params: Params,
        #[serde(default, rename = "emittedAtMs")]
        emitted_at_ms: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Params {
        summary: String,
        details: Option<String>,
        path: Option<String>,
        range: Option<serde_json::Value>,
    }
    let Ok(value) = input.json() else {
        return false;
    };
    let Ok(note) = serde_json::from_value::<Note>(value) else {
        return false;
    };
    note.method == "configWarning"
        && note.emitted_at_ms <= 9_007_199_254_740_991
        && !note.params.summary.is_empty()
        && note.params.summary.len() <= 16 * 1024
        && note.params.path.is_none()
        && note.params.range.is_none()
        && note.params.details.as_ref().is_none_or(|details| {
            details.len() <= 16 * 1024
                && !details
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
        })
}

/// Fixed outcome for the model-free native hook probe. No raw transcript,
/// session handle, or authority to submit a turn leaves this function.
pub fn hook_probe_notification(input: &super::super::BoundedObservation) -> &'static str {
    let Ok(value) = input.json() else {
        return "invalid";
    };
    if value.get("id").is_some() {
        return "response";
    }
    match value["method"].as_str() {
        Some("hook/started") => "hook/started",
        Some("hook/completed") => "hook/completed",
        Some("thread/started") => "thread/started",
        Some("warning") => "warning",
        Some("error") => "error",
        Some("configWarning") => "configWarning",
        Some(_) => "other-notification",
        None => "invalid",
    }
}

/// A successful thread response does not prove that a lifecycle hook ran.
pub fn hook_probe_started(input: &super::super::BoundedObservation) -> Result<bool, &'static str> {
    let value = input.json().map_err(|_| "invalid hook probe JSON")?;
    if value.get("id").is_none() && value["method"].is_string() {
        return Ok(false);
    }
    if value["id"] != 99 || !value["error"].is_null() {
        return Err("hook probe thread response rejected");
    }
    if value["result"]["thread"]["id"]
        .as_str()
        .is_none_or(str::is_empty)
    {
        return Err("hook probe thread missing");
    }
    Ok(true)
}

/// Model-free native probe only; exposes a fixed outcome, never raw settings.
/// The caller owns process isolation/termination. This is not launch admission.
pub fn matches_project_config_probe(
    input: &super::super::BoundedObservation,
    home: &Path,
    workspace: &Path,
    isolated: bool,
    hooks_enabled: bool,
) -> Result<(), &'static str> {
    let value = input.json().map_err(|_| "invalid config probe JSON")?;
    if is_probe_configuration_warning(input) {
        return Err("config probe configuration warning");
    }
    if value["method"].is_string() {
        return Err("config probe unexpected notification");
    }
    if !value["error"].is_null() {
        return Err("config probe RPC error");
    }
    if value["id"] != 2 || !value["error"].is_null() {
        return Err("config probe response mismatch");
    }
    let result = &value["result"];
    if result["config"]["features"]["hooks"] != hooks_enabled {
        return Err("hook probe effective feature mismatch");
    }
    let layers = result["layers"]
        .as_array()
        .ok_or("config probe layers missing")?;
    let marker = result["config"]["developer_instructions"]
        == "HELLO_AGENT_PROJECT_CONFIG_SENTINEL"
        || layers.iter().any(|layer| {
            layer["disabledReason"].is_null()
                && layer["config"]["developer_instructions"]
                    == "HELLO_AGENT_PROJECT_CONFIG_SENTINEL"
        });
    if marker == isolated {
        return Err(if isolated {
            "project marker loaded despite isolation"
        } else if !layers
            .iter()
            .any(|layer| layer["name"]["type"] == "project")
        {
            "positive control project layer absent"
        } else if layers
            .iter()
            .any(|layer| layer["name"]["type"] == "project" && !layer["disabledReason"].is_null())
        {
            "positive control project layer disabled"
        } else {
            "positive control project marker absent"
        });
    }
    if !isolated {
        return Ok(());
    }
    let Some(flags) = result["layers"].as_array().and_then(|layers| {
        layers
            .iter()
            .find(|layer| layer["name"]["type"] == "sessionFlags")
    }) else {
        return Err("config probe flags missing");
    };
    if flags["config"].get("projects").is_none() {
        return Err("config probe isolation flag missing");
    }
    strip_isolation_flag(
        &mut flags["config"].clone(),
        &result["config"],
        &[home, workspace],
    )
}

/// Validate optional new invocation flags while retaining older auth-only
/// observations. This compatibility is NOT admission for trust-bearing profiles.
pub fn strip_isolation_flag(
    flags: &mut serde_json::Value,
    effective: &serde_json::Value,
    paths: &[&Path],
) -> Result<(), &'static str> {
    let Some(projects) = flags.get("projects") else {
        return Ok(());
    };
    let map = projects.as_object().ok_or("invalid isolation flags")?;
    if map.len() > MAX_PROJECTS + 256
        || map
            .values()
            .any(|value| *value != serde_json::json!({"trust_level":"untrusted"}))
        || effective.get("projects") != Some(projects)
    {
        return Err("project isolation differs");
    }
    for path in paths {
        if !path.is_absolute() {
            return Err("invalid isolation path");
        }
        for ancestor in path.ancestors() {
            if !map.contains_key(ancestor.to_str().ok_or("invalid isolation path")?) {
                return Err("project isolation ancestor missing");
            }
        }
    }
    flags
        .as_object_mut()
        .ok_or("invalid isolation flags")?
        .remove("projects");
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    cli_auth_credentials_store: String,
    #[serde(default)]
    projects: BTreeMap<String, ProjectTrust>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectTrust {
    trust_level: TrustLevel,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum TrustLevel {
    Trusted,
    Untrusted,
}

impl ProfileConfig {
    /// Reported user-layer compatibility. A trust-bearing layer is accepted only
    /// with invocation overrides for every stored key and matching effective
    /// values. This is still post-start evidence, not startup authority.
    pub fn matches_observed_layer(
        value: &serde_json::Value,
        flags: &serde_json::Value,
        effective: &serde_json::Value,
        home: &Path,
    ) -> bool {
        let Ok(bytes) = serde_json::to_vec(value) else {
            return false;
        };
        Self::matches_observed_bytes(&bytes, flags, effective, home)
    }

    pub fn matches_observed_bytes(
        bytes: &[u8],
        flags: &serde_json::Value,
        effective: &serde_json::Value,
        home: &Path,
    ) -> bool {
        if bytes.len() > MAX_CONFIG_BYTES {
            return false;
        }
        let Ok(value) = super::super::unique_json::parse(bytes) else {
            return false;
        };
        let Ok(config) = serde_json::from_value::<Self>(value) else {
            return false;
        };
        let Ok(config) = config.validate() else {
            return false;
        };
        if config.is_auth_only() {
            return true;
        }
        let Some(projects) = flags.get("projects").and_then(serde_json::Value::as_object) else {
            return false;
        };
        if config
            .projects
            .keys()
            .any(|key| !projects.contains_key(key))
        {
            return false;
        }
        strip_isolation_flag(&mut flags.clone(), effective, &[home]).is_ok()
    }

    /// Invocation-only replacement values. Include every stored trust key and
    /// every launch-path ancestor so no known trusted selector is inherited.
    /// Callers must supply canonical launch paths; this does not access files.
    pub fn isolated_projects(&self, paths: &[&Path]) -> Result<serde_json::Value, &'static str> {
        let mut projects = serde_json::Map::new();
        for path in self.projects.keys() {
            projects.insert(path.clone(), serde_json::json!({"trust_level":"untrusted"}));
        }
        for path in paths {
            let text = path.to_str().ok_or("invalid isolation path")?;
            if !path.is_absolute()
                || text.len() > 4096
                || text.chars().any(char::is_control)
                || text.split('/').any(|part| part == "." || part == "..")
            {
                return Err("invalid isolation path");
            }
            for ancestor in path.ancestors() {
                projects.insert(
                    ancestor.to_str().ok_or("invalid isolation path")?.into(),
                    serde_json::json!({"trust_level":"untrusted"}),
                );
                if projects.len() > MAX_PROJECTS + 256 {
                    return Err("isolation paths exceed limit");
                }
            }
        }
        Ok(serde_json::Value::Object(projects))
    }

    pub fn isolation_argument(&self, paths: &[&Path]) -> Result<String, &'static str> {
        let projects = self.isolated_projects(paths)?;
        // JSON basic-string encoding is TOML-compatible for these non-control
        // keys. Values are fixed, never rendered from user-controlled TOML.
        let entries = projects
            .as_object()
            .ok_or("invalid isolation map")?
            .keys()
            .map(|path| {
                format!(
                    "{}={{trust_level=\"untrusted\"}}",
                    serde_json::to_string(path).unwrap()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!("projects={{{entries}}}"))
    }

    /// No credential contents, raw parser errors, or project paths escape on error.
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err("profile configuration exceeds size limit");
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "profile configuration is not UTF-8")?;
        let config: Self = toml::from_str(text).map_err(|_| "unsupported profile configuration")?;
        config.validate()
    }

    fn validate(self) -> Result<Self, &'static str> {
        let config = self;
        if config.cli_auth_credentials_store != "file" || config.projects.len() > MAX_PROJECTS {
            return Err("unsupported profile configuration");
        }
        for (path, project) in &config.projects {
            if path.len() > 4096
                || path.chars().any(char::is_control)
                || !Path::new(path).is_absolute()
                || path.split('/').any(|part| part == ".")
                || Path::new(path)
                    .components()
                    .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
            {
                return Err("unsupported profile project identity");
            }
            // Exhaustive typed validation; no arbitrary TOML subtree survives.
            match project.trust_level {
                TrustLevel::Trusted | TrustLevel::Untrusted => {}
            }
        }
        Ok(config)
    }

    /// Until a launcher proves project-layer isolation, only auth-only profiles
    /// may be admitted. Whitespace/comments are not an authorization boundary.
    pub fn is_auth_only(&self) -> bool {
        self.projects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TOML project keys are host paths too. Keep relative/parent/current-dir
    // negative fixtures intact while adding a drive to rooted Windows inputs.
    fn host_profile(input: &str) -> String {
        if cfg!(windows) {
            input.replace("[projects.'/", "[projects.'C:/")
        } else {
            input.to_owned()
        }
    }

    #[test]
    fn disabled_project_provenance_requires_isolation_and_exact_launch_ancestry() {
        let home = Path::new(test_path!("/app/profile"));
        let cwd = Path::new(test_path!("/app/workspace"));
        let profile = ProfileConfig::parse(b"cli_auth_credentials_store='file'\n").unwrap();
        let projects = profile.isolated_projects(&[home, cwd]).unwrap();
        let flags = serde_json::json!({"projects":projects,"features":{"hooks":false,"apps":false,"plugins":false}});
        let base = serde_json::json!({"config":flags,"layers":[
            {"name":{"type":"sessionFlags"},"config":flags},
            {"name":{"type":"project","dotCodexFolder":test_path!("/app/workspace", ".codex")},"config":{"private_marker":"disabled content"},"disabledReason":"Not trusted"}
        ]});
        assert!(validate_disabled_project_layers(&base, home, cwd).is_ok());
        for case in [
            "active",
            "empty",
            "outside",
            "alias",
            "duplicate",
            "flags",
            "effective",
            "hooks",
            "source",
            "content",
        ] {
            let mut value = base.clone();
            match case {
                "active" => value["layers"][1]["disabledReason"] = serde_json::Value::Null,
                "empty" => value["layers"][1]["disabledReason"] = " ".into(),
                "outside" => {
                    value["layers"][1]["name"]["dotCodexFolder"] =
                        test_path!("/other/.codex").into()
                }
                "alias" => {
                    value["layers"][1]["name"]["dotCodexFolder"] =
                        test_path!("/app/workspace/../workspace/.codex").into()
                }
                "duplicate" => {
                    let duplicate = value["layers"][1].clone();
                    value["layers"].as_array_mut().unwrap().push(duplicate);
                }
                "flags" => {
                    value["layers"][0]["config"]
                        .as_object_mut()
                        .unwrap()
                        .remove("projects");
                }
                "effective" => {
                    value["config"]["projects"][test_path!("/app/workspace")]["trust_level"] =
                        "trusted".into()
                }
                "hooks" => value["config"]["features"]["hooks"] = true.into(),
                "source" => value["layers"][1]["name"]["extra"] = true.into(),
                "content" => value["layers"][1]["config"] = serde_json::Value::Null,
                _ => unreachable!(),
            }
            assert!(
                validate_disabled_project_layers(&value, home, cwd).is_err(),
                "{case}"
            );
        }
    }

    #[test]
    fn hook_diagnostic_emits_only_fixed_labels_not_vendor_content() {
        for (input, expected) in [
            (
                r#"{"method":"hook/started","params":{"secret":"private"}}"#,
                "hook/started",
            ),
            (
                r#"{"method":"hook/completed","params":{}}"#,
                "hook/completed",
            ),
            (
                r#"{"method":"thread/started","params":{}}"#,
                "thread/started",
            ),
            (
                r#"{"method":"private-unrecognized-method"}"#,
                "other-notification",
            ),
            (
                r#"{"id":99,"result":{"thread":{"id":"private"}}}"#,
                "response",
            ),
            (
                r#"{"method":"hook/started","method":"thread/started"}"#,
                "invalid",
            ),
            ("not json", "invalid"),
        ] {
            let observation =
                super::super::super::BoundedObservation::new(input.as_bytes().to_vec()).unwrap();
            assert_eq!(hook_probe_notification(&observation), expected);
        }
    }

    #[test]
    fn diagnostic_warning_rejects_requests_duplicates_and_malformed_envelopes() {
        let base = serde_json::json!({"method":"configWarning","params":{
            "summary":"Project-local config, hooks, and exec policies are disabled in the following folders until the project is trusted, but skills still load.",
            "details":"1. /temporary/project/.codex\n", "path":null,"range":null
        }});
        for case in [
            "valid", "summary", "method", "id", "extra", "path", "range", "budget", "control",
        ] {
            let mut note = base.clone();
            match case {
                "summary" => note["params"]["summary"] = "".into(),
                "method" => note["method"] = "warning".into(),
                "id" => note["id"] = 2.into(),
                "extra" => note["params"]["extra"] = true.into(),
                "path" => note["params"]["path"] = test_path!("/other").into(),
                "range" => note["params"]["range"] = serde_json::json!({}),
                "budget" => note["params"]["details"] = "x".repeat(16385).into(),
                "control" => note["params"]["details"] = "\u{1b}secret".into(),
                _ => {}
            }
            let input = super::super::super::BoundedObservation::new(note.to_string().into_bytes())
                .unwrap();
            assert_eq!(
                is_probe_configuration_warning(&input),
                case == "valid",
                "{case}"
            );
        }
        let duplicate = base
            .to_string()
            .replace("\"method\":", "\"method\":\"other\",\"method\":");
        let input = super::super::super::BoundedObservation::new(duplicate.into_bytes()).unwrap();
        assert!(!is_probe_configuration_warning(&input));
    }

    #[test]
    fn observed_trust_requires_matching_override_and_rejects_unknown_user_settings() {
        let user = serde_json::json!({"cli_auth_credentials_store":"file","projects":{
            (test_path!("/old")):{"trust_level":"trusted"}
        }});
        let parsed = ProfileConfig::parse(
            host_profile(
                "cli_auth_credentials_store='file'\n[projects.'/old']\ntrust_level='trusted'",
            )
            .as_bytes(),
        )
        .unwrap();
        let projects = parsed
            .isolated_projects(&[Path::new(test_path!("/profile"))])
            .unwrap();
        let flags = serde_json::json!({"projects":projects});
        let effective = flags.clone();
        assert!(ProfileConfig::matches_observed_layer(
            &user,
            &flags,
            &effective,
            Path::new(test_path!("/profile"))
        ));
        assert!(!ProfileConfig::matches_observed_layer(
            &user,
            &serde_json::json!({}),
            &effective,
            Path::new(test_path!("/profile"))
        ));
        let mut missing = flags.clone();
        missing["projects"]
            .as_object_mut()
            .unwrap()
            .remove(test_path!("/old"));
        assert!(!ProfileConfig::matches_observed_layer(
            &user,
            &missing,
            &missing,
            Path::new(test_path!("/profile"))
        ));
        let mut extra = user;
        extra["hooks"] = serde_json::json!({});
        assert!(!ProfileConfig::matches_observed_layer(
            &extra,
            &flags,
            &effective,
            Path::new(test_path!("/profile"))
        ));
        assert!(!ProfileConfig::matches_observed_bytes(
            br#"{"cli_auth_credentials_store":"file","cli_auth_credentials_store":"file"}"#,
            &flags,
            &effective,
            Path::new(test_path!("/profile")),
        ));
    }

    #[test]
    fn observed_isolation_requires_every_ancestor_and_exact_effective_values() {
        let config = ProfileConfig::parse(b"cli_auth_credentials_store='file'").unwrap();
        let paths = [
            Path::new(test_path!("/profile")),
            Path::new(test_path!("/workspace/child")),
        ];
        let projects = config.isolated_projects(&paths).unwrap();
        let effective = serde_json::json!({"projects":projects});
        let mut flags = serde_json::json!({"projects":projects,"model":"test"});
        strip_isolation_flag(&mut flags, &effective, &paths).unwrap();
        assert_eq!(flags, serde_json::json!({"model":"test"}));
        for case in ["trusted", "missing", "extraField", "effectiveDrift", "null"] {
            let mut changed = projects.clone();
            let mut observed = effective.clone();
            match case {
                "trusted" => {
                    changed[test_path!("/workspace")]["trust_level"] = serde_json::json!("trusted")
                }
                "missing" => {
                    changed
                        .as_object_mut()
                        .unwrap()
                        .remove(test_path!("/workspace"));
                }
                "extraField" => {
                    changed[test_path!("/workspace")]["other"] = serde_json::json!(true)
                }
                "effectiveDrift" => {
                    observed["projects"][test_path!("/workspace")]["trust_level"] =
                        serde_json::json!("trusted")
                }
                "null" => changed = serde_json::Value::Null,
                _ => unreachable!(),
            }
            let mut flags = serde_json::json!({"projects":changed});
            assert!(
                strip_isolation_flag(&mut flags, &observed, &paths).is_err(),
                "{case}"
            );
        }
    }

    #[test]
    fn isolation_overrides_all_stored_keys_and_ancestors_without_changing_profile() {
        let input = "cli_auth_credentials_store='file'\n[projects.'/old']\ntrust_level='trusted'";
        let config = ProfileConfig::parse(host_profile(&input).as_bytes()).unwrap();
        let paths = [
            Path::new(test_path!("/app/profile")),
            Path::new(test_path!("/work/한글.\"quoted\"\\name")),
        ];
        let argument = config.isolation_argument(&paths).unwrap();
        let parsed: toml::Value = toml::from_str(&argument).unwrap();
        let projects = parsed["projects"].as_table().unwrap();
        for key in [
            test_path!("/"),
            test_path!("/app"),
            test_path!("/app/profile"),
            test_path!("/old"),
            test_path!("/work"),
            test_path!("/work/한글.\"quoted\"\\name"),
        ] {
            assert_eq!(projects[key]["trust_level"].as_str(), Some("untrusted"));
        }
        assert!(!config.is_auth_only());
        assert!(config.isolation_argument(&[Path::new("relative")]).is_err());
        assert!(config
            .isolation_argument(&[Path::new(test_path!("/a/../b"))])
            .is_err());
        assert!(config
            .isolation_argument(&[Path::new(test_path!("/a/./b"))])
            .is_err());
    }

    #[test]
    fn accepts_equivalent_auth_configuration_without_rewriting() {
        for input in [
            "cli_auth_credentials_store=\"file\"",
            "# vendor comment\r\ncli_auth_credentials_store = 'file'\r\n",
            "cli_auth_credentials_store = \"file\"\n[projects]\n",
        ] {
            assert!(ProfileConfig::parse(host_profile(&input).as_bytes())
                .unwrap()
                .is_auth_only());
        }
    }

    #[test]
    fn recognizes_trust_but_does_not_admit_it_as_auth_only() {
        for level in ["trusted", "untrusted"] {
            let input = format!(
                "cli_auth_credentials_store='file'\n[projects.'/snapshot']\ntrust_level='{level}'"
            );
            assert!(!ProfileConfig::parse(host_profile(&input).as_bytes())
                .unwrap()
                .is_auth_only());
        }
    }

    #[test]
    fn rejects_unknown_settings_malformed_trust_and_budget_excess() {
        for suffix in [
            "\nnotify=['command']",
            "\n[hooks]",
            "\ncli_auth_credentials_store='file'",
            "\n[projects.'/snapshot']\ntrust_level='trusted'\nextra=true",
            "\n[projects.'/snapshot']\ntrust_level='other'",
            "\n[projects.'/snapshot']",
            "\n[projects.'relative']\ntrust_level='trusted'",
            "\n[projects.'/a/../b']\ntrust_level='trusted'",
            "\n[projects.'/a/./b']\ntrust_level='trusted'",
        ] {
            let text = format!("cli_auth_credentials_store='file'{suffix}");
            assert!(
                ProfileConfig::parse(host_profile(&text).as_bytes()).is_err(),
                "{suffix}"
            );
        }
        assert!(ProfileConfig::parse(b"cli_auth_credentials_store='keyring'").is_err());
        assert!(ProfileConfig::parse(&vec![b' '; MAX_CONFIG_BYTES + 1]).is_err());
        assert!(ProfileConfig::parse(&[0xff]).is_err());
        let mut text = "cli_auth_credentials_store='file'\n".to_owned();
        for index in 0..=MAX_PROJECTS {
            text.push_str(&format!("[projects.'/p{index}']\ntrust_level='trusted'\n"));
        }
        assert!(ProfileConfig::parse(host_profile(&text).as_bytes()).is_err());
    }
}
