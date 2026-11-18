//! Work history rows for the team chat: one compact line per tool use, so a
//! long run visibly moves. Shows what kind of work (read, edit, command…) and
//! where inside the workspace, never the tool input itself (file contents,
//! command text). A row that cannot be stored never stops the run.
use super::*;
use elsecrew_protocol::agent_adapter_v1::output_pages::{
    OutputItemChannel, OutputItemOperation, OutputItemUpdate,
};
use std::path::{Component, Path};

const TARGET_MAX_CHARS: usize = 160;

/// A workspace-relative path, or None when it leaves the workspace.
fn relative(path: &str, cwd: &Path) -> Option<String> {
    let path = Path::new(path);
    let inside = if path.is_absolute() { path.strip_prefix(cwd).ok()? } else { path };
    if inside.components().any(|part| !matches!(part, Component::Normal(_) | Component::CurDir)) {
        return None;
    }
    let text = inside.to_str()?.trim_start_matches("./");
    Some(if text.is_empty() { ".".into() } else { bounded(text)? })
}

/// Invisible format marks that can reorder or hide what the row shows.
fn hidden_format(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}')
}

fn bounded(text: &str) -> Option<String> {
    let line = text.split(['\n', '\r']).next()?.trim();
    if line.is_empty() || line.chars().any(|c| c.is_control() || hidden_format(c)) { return None; }
    Some(if line.chars().count() > TARGET_MAX_CHARS {
        format!("{}…", line.chars().take(TARGET_MAX_CHARS).collect::<String>())
    } else {
        line.into()
    })
}

/// "pattern · folder" for a search: the model's own pattern (never file
/// contents), and the folder only when it is not the whole workspace.
fn search_target(input: &str, path: &str, cwd: &Path) -> Option<String> {
    let pattern = serde_json::from_str::<Value>(input).ok()
        .and_then(|input| input.get("pattern").and_then(Value::as_str).and_then(bounded));
    let place = relative(path, cwd).filter(|path| path != ".");
    match (pattern, place) {
        (Some(pattern), Some(place)) => bounded(&format!("{pattern} · {place}")),
        (pattern, place) => pattern.or(place),
    }
}

fn row(action: &str, target: Option<String>, tool: Option<&str>, outcome: Option<&str>) -> Value {
    let mut value = json!({"action": action});
    if let Some(target) = target { value["target"] = json!(target); }
    if let Some(tool) = tool { value["tool"] = json!(tool); }
    if let Some(outcome) = outcome { value["outcome"] = json!(outcome); }
    value
}

/// Claude asks before every tool use; record the app's answer.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn claude(
    request: &elsecrew_core::provider::vendor_observation::claude_approval::Request,
    cwd: &Path,
    refused: bool,
) -> Value {
    use elsecrew_core::provider::vendor_observation::claude_approval::ToolTarget;
    let outcome = refused.then_some("refused");
    match request.target() {
        ToolTarget::File { path, .. } => {
            let action = match request.tool_name.as_str() { "Read" => "read", "Write" => "write", _ => "edit" };
            row(action, relative(path, cwd), None, outcome)
        }
        // What was looked for (the model's pattern, never file contents) and
        // where; the whole workspace needs no place. Glob finds file names,
        // Grep searches inside files.
        ToolTarget::Search { path } => {
            let tool = matches!(request.tool_name.as_str(), "Glob" | "Grep").then_some(request.tool_name.as_str());
            row("search", search_target(request.inspect_input(), path, cwd), tool, outcome)
        }
        // The model's own one-line summary, never the command text.
        ToolTarget::Shell => {
            let summary = serde_json::from_str::<Value>(request.inspect_input()).ok()
                .and_then(|input| input.get("description").and_then(Value::as_str).and_then(bounded));
            row("command", summary, None, outcome)
        }
        ToolTarget::Unsupported => row("other", None, Some(&request.tool_name), outcome),
    }
}

/// Codex reports the kind and phase of each command or file change.
pub(super) fn codex(tool: &elsecrew_core::provider::vendor_observation::codex_events::stream::ToolEvent) -> Option<Value> {
    use elsecrew_core::provider::vendor_observation::codex_events::stream::{ToolKind, ToolPhase};
    let action = match tool.kind { ToolKind::Command => "command", ToolKind::FileChange => "edit" };
    let outcome = match tool.phase {
        ToolPhase::Started => None,
        ToolPhase::Failed => Some("failed"),
        ToolPhase::Declined => Some("refused"),
        ToolPhase::Output | ToolPhase::Completed => return None,
    };
    // Which command ran, so a list of commands is readable (user request
    // 2026-11-18): the first line, without the shell wrapper Codex adds.
    let command = tool.command.as_deref().map(unwrap_shell).and_then(bounded);
    Some(row(action, command, None, outcome))
}

/// `/bin/zsh -lc "sed -n 1,80p app.js"` → `sed -n 1,80p app.js`.
fn unwrap_shell(command: &str) -> &str {
    for prefix in ["/bin/zsh -lc ", "/bin/bash -lc ", "/bin/sh -c ", "zsh -lc ", "bash -lc ", "sh -c "] {
        if let Some(rest) = command.strip_prefix(prefix) {
            let rest = rest.trim();
            for quote in ['"', '\''] {
                if rest.len() >= 2 && rest.starts_with(quote) && rest.ends_with(quote) {
                    return &rest[1..rest.len() - 1];
                }
            }
            return rest;
        }
    }
    command
}

impl RunReservation {
    /// Best effort: the history is a view of the run, not part of its result.
    pub(super) fn record_activity(&self, storage: &mut Storage, activity: Value) {
        let update = OutputItemUpdate {
            item_id: format!("activity_{}", Uuid::new_v4().simple()),
            operation: OutputItemOperation::Complete,
            channel: OutputItemChannel::Activity,
        };
        if let Err(error) = self.persist_item_progress(storage, &update, activity.to_string()) {
            eprintln!("work history row not stored: {}", error.kind());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_workspace() {
        let cwd = Path::new("/work/copy");
        assert_eq!(relative("/work/copy/src/app.ts", cwd).as_deref(), Some("src/app.ts"));
        assert_eq!(relative("src/app.ts", cwd).as_deref(), Some("src/app.ts"));
        assert_eq!(relative("/work/copy", cwd).as_deref(), Some("."));
        assert_eq!(relative("/elsewhere/secret.txt", cwd), None);
        assert_eq!(relative("/work/copy/../secret.txt", cwd), None);
        assert_eq!(relative("../secret.txt", cwd), None);
    }

    #[test]
    fn summaries_are_one_bounded_line() {
        assert_eq!(bounded("Run tests\nrm -rf /").as_deref(), Some("Run tests"));
        assert_eq!(bounded("  ").as_deref(), None);
        assert_eq!(bounded("file\u{202e}txt.exe").as_deref(), None);
        assert_eq!(relative("/work/copy/a\u{202e}b", Path::new("/work/copy")), None);
        assert_eq!(bounded(&"a".repeat(400)).map(|text| text.chars().count()), Some(TARGET_MAX_CHARS + 1));
    }

    #[test]
    fn searches_show_what_was_looked_for_and_where() {
        let cwd = Path::new("/work/copy");
        assert_eq!(search_target(r#"{"pattern":"**/*.md"}"#, "/work/copy", cwd).as_deref(), Some("**/*.md"));
        assert_eq!(search_target(r#"{"pattern":"TODO","path":"src"}"#, "/work/copy/src", cwd).as_deref(), Some("TODO · src"));
        assert_eq!(search_target(r#"{"pattern":"a\nrm -rf /"}"#, "/work/copy", cwd).as_deref(), Some("a"));
        assert_eq!(search_target(r#"{"pattern":"x"}"#, "/elsewhere", cwd).as_deref(), Some("x"));
        assert_eq!(search_target("not json", "/work/copy/docs", cwd).as_deref(), Some("docs"));
    }

    #[test]
    fn codex_rows_mark_start_failure_and_refusal_only() {
        use elsecrew_core::provider::vendor_observation::codex_events::stream::{ToolEvent, ToolKind, ToolPhase};
        let event = |phase| ToolEvent { item_id: "i".into(), kind: ToolKind::Command, phase, command: None };
        assert_eq!(codex(&event(ToolPhase::Started)), Some(json!({"action":"command"})));
        assert_eq!(codex(&event(ToolPhase::Failed)), Some(json!({"action":"command","outcome":"failed"})));
        assert_eq!(codex(&event(ToolPhase::Declined)), Some(json!({"action":"command","outcome":"refused"})));
        assert_eq!(codex(&event(ToolPhase::Completed)), None);
        assert_eq!(codex(&event(ToolPhase::Output)), None);
        let started = |command: &str| codex(&ToolEvent { item_id: "i".into(), kind: ToolKind::Command, phase: ToolPhase::Started, command: Some(command.into()) });
        assert_eq!(started(r#"/bin/zsh -lc "sed -n 1,80p app.js""#), Some(json!({"action":"command","target":"sed -n 1,80p app.js"})));
        assert_eq!(started("/bin/bash -lc 'node tests/check.js'"), Some(json!({"action":"command","target":"node tests/check.js"})));
        assert_eq!(started("ls -la"), Some(json!({"action":"command","target":"ls -la"})));
        assert_eq!(started("echo one\nrm -rf /"), Some(json!({"action":"command","target":"echo one"})));
    }
}
