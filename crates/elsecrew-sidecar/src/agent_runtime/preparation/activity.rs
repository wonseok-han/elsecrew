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
        // The whole workspace needs no label.
        ToolTarget::Search { path } => row("search", relative(path, cwd).filter(|path| path != "."), None, outcome),
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
    Some(row(action, None, None, outcome))
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
    fn codex_rows_mark_start_failure_and_refusal_only() {
        use elsecrew_core::provider::vendor_observation::codex_events::stream::{ToolEvent, ToolKind, ToolPhase};
        let event = |phase| ToolEvent { item_id: "i".into(), kind: ToolKind::Command, phase };
        assert_eq!(codex(&event(ToolPhase::Started)), Some(json!({"action":"command"})));
        assert_eq!(codex(&event(ToolPhase::Failed)), Some(json!({"action":"command","outcome":"failed"})));
        assert_eq!(codex(&event(ToolPhase::Declined)), Some(json!({"action":"command","outcome":"refused"})));
        assert_eq!(codex(&event(ToolPhase::Completed)), None);
        assert_eq!(codex(&event(ToolPhase::Output)), None);
    }
}
