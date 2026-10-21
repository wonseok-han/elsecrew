//! An observed disabled-project warning is pending evidence, not authority.
//! Accept only in an isolated config/read phase and reconcile against layers.
use crate::provider::BoundedObservation;
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

const HEADER: &str = "Project-local config, hooks, and exec policies are disabled in the following folders until the project is trusted, but skills still load.\n";
const REASON: &str = " is marked as untrusted in the effective configuration. To load project-local config, hooks, and exec policies, update its trust setting. If that setting is managed by your organization, contact your administrator.";

// No Debug/public paths: private vendor text never becomes user diagnostics.
pub struct IsolationWarning(BTreeMap<String, String>);

impl IsolationWarning {
    pub fn parse(
        input: &BoundedObservation,
        home: &Path,
        cwd: &Path,
    ) -> Result<Self, &'static str> {
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
        let note: Note = serde_json::from_value(input.json().map_err(|_| "warning JSON")?)
            .map_err(|_| "warning envelope")?;
        if note.method != "configWarning"
            || note.emitted_at_ms > 9_007_199_254_740_991
            || note.params.details.is_some()
            || note.params.path.is_some()
            || note.params.range.is_some()
            || note.params.summary.len() > 16 * 1024
            || !note.params.summary.ends_with('\n')
        {
            return Err("warning shape");
        }
        let body = note
            .params
            .summary
            .strip_prefix(HEADER)
            .ok_or("unknown configuration warning")?;
        let lines: Vec<_> = body.lines().collect();
        if lines.is_empty() || lines.len() % 2 != 0 || lines.len() > 256 {
            return Err("warning entries");
        }
        let mut entries = BTreeMap::new();
        for (index, pair) in lines.chunks_exact(2).enumerate() {
            let prefix = format!("    {}. ", index + 1);
            let folder = pair[0].strip_prefix(&prefix).ok_or("warning numbering")?;
            let parent = [home, cwd]
                .iter()
                .flat_map(|path| path.ancestors())
                .find(|parent| parent.join(".codex").to_str() == Some(folder))
                .ok_or("warning path outside launch ancestry")?;
            let reason = format!("{}{REASON}", parent.to_str().ok_or("warning path")?);
            if pair[1] != format!("       {reason}")
                || entries.insert(folder.into(), reason).is_some()
            {
                return Err("warning reason or duplicate");
            }
        }
        Ok(Self(entries))
    }

    pub fn matches_configuration(&self, result: &serde_json::Value) -> bool {
        let Some(layers) = result["layers"].as_array() else {
            return false;
        };
        let mut seen = std::collections::BTreeSet::new();
        for layer in layers
            .iter()
            .filter(|layer| layer["name"]["type"] == "project")
        {
            let Some(folder) = layer["name"]["dotCodexFolder"].as_str() else {
                return false;
            };
            let Some(reason) = self.0.get(folder) else {
                return false;
            };
            if !seen.insert(folder) || Some(reason.as_str()) != layer["disabledReason"].as_str() {
                return false;
            }
        }
        seen.len() == self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_exact_isolation_warning_can_be_reconciled() {
        let home = Path::new("/app/profile");
        let cwd = Path::new("/app/workspace");
        let summary =
            format!("{HEADER}    1. /app/workspace/.codex\n       /app/workspace{REASON}\n");
        let base = json!({"method":"configWarning","params":{"summary":summary}});
        let observation = |value: &serde_json::Value| {
            BoundedObservation::new(value.to_string().into_bytes()).unwrap()
        };
        let warning = IsolationWarning::parse(&observation(&base), home, cwd).unwrap();
        let layers = json!({"layers":[{"name":{"type":"project","dotCodexFolder":"/app/workspace/.codex"},"disabledReason":format!("/app/workspace{REASON}")}]});
        assert!(warning.matches_configuration(&layers));
        assert!(!warning.matches_configuration(&json!({"layers":[]})));
        let mut enabled = layers.clone();
        enabled["layers"][0]["disabledReason"] = serde_json::Value::Null;
        assert!(!warning.matches_configuration(&enabled));
        for case in [
            "summary",
            "id",
            "path",
            "details",
            "outside",
            "reason",
            "duplicate",
            "time",
        ] {
            let mut value = base.clone();
            match case {
                "summary" => {
                    value["params"]["summary"] = "Invalid configuration; using defaults".into()
                }
                "id" => value["id"] = 2.into(),
                "path" => value["params"]["path"] = "/private".into(),
                "details" => value["params"]["details"] = "unexpected".into(),
                "outside" => {
                    value["params"]["summary"] = summary.replace("/app/workspace", "/other").into()
                }
                "reason" => {
                    value["params"]["summary"] = summary.replace("untrusted", "trusted").into()
                }
                "duplicate" => {
                    value["params"]["summary"] = format!(
                        "{summary}    2. /app/workspace/.codex\n       /app/workspace{REASON}\n"
                    )
                    .into()
                }
                "time" => value["emittedAtMs"] = 9_007_199_254_740_992_u64.into(),
                _ => unreachable!(),
            }
            assert!(
                IsolationWarning::parse(&observation(&value), home, cwd).is_err(),
                "{case}"
            );
        }
    }
}
