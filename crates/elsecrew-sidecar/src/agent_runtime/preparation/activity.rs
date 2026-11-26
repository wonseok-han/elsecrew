//! Work history rows for the team chat: one compact line per tool use, so a
//! long run visibly moves. A row says what kind of work (read, edit,
//! command…) and on what: the file inside the workspace, the search, or the
//! command line. When the tool use ends a second row with the same `id` says
//! how it ended (exit code when known, time, changed lines); the screen shows
//! it in place of the first.
//!
//! What is stored is display text, never authority. Every row passes
//! `sanitize` right before it is written (the only way into the store):
//! known fields only, control and direction marks neutralised, anything that
//! looks like a secret replaced by •••• (`secret_mask`), then cut to length.
//! Ids are the app's own (`c3`, `t7`), never the provider's. Nothing a tool
//! printed or read is stored: no command output, no file contents
//! (review PR #11 B3). A row that cannot be stored never stops the run.
use super::command_redact::redact_command;
use super::secret_mask::mask_secrets;
use super::*;
use elsecrew_protocol::agent_adapter_v1::output_pages::{
    OutputItemChannel, OutputItemOperation, OutputItemUpdate,
};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

const TARGET_MAX_CHARS: usize = 160;
const BLOCK_MAX_CHARS: usize = 4000;
const NOTE_MAX_CHARS: usize = 300;
/// Open tool uses remembered until their end (per run).
const OPEN_TOOLS_MAX: usize = 256;

/// Invisible format marks and line/paragraph separators that can reorder or
/// hide what the row shows (the screen's `MARKS` is the same set).
fn hidden_format(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}')
}

/// Control and direction marks become a visible replacement mark, so text
/// can neither reorder the line nor hide part of itself. Tabs and (in a
/// block) line breaks stay.
fn neutralise(text: &str, keep_lines: bool) -> String {
    text.replace("\r\n", "\n").chars().map(|c| match c {
        '\n' if keep_lines => c,
        '\t' => c,
        c if c.is_control() || hidden_format(c) => '\u{fffd}',
        c => c,
    }).collect()
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() > max { format!("{}…", text.chars().take(max).collect::<String>()) } else { text.into() }
}

/// One line as stored: neutralised, masked, then cut (masking first, so a cut
/// never leaves part of a secret masking would have caught). Same as the
/// screen's `displayLine` (shared cases).
fn display_line(text: &str) -> String {
    cut(&mask_secrets(&neutralise(text, false)), TARGET_MAX_CHARS)
}

/// The first non-empty line of `text`, as stored.
fn bounded(text: &str) -> Option<String> {
    let line = text.split(['\n', '\r']).find(|line| !line.trim().is_empty())?.trim();
    Some(display_line(line))
}

/// Several lines (a whole command), masked as a whole, then cut.
fn block(text: &str, max: usize) -> Option<String> {
    let text = text.trim_matches('\n').trim_end();
    if text.trim().is_empty() { return None; }
    Some(cut(&mask_secrets(&neutralise(text, true)), max))
}

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

/// The last `keep` parts of a path, with `…/` when more came before.
fn tail(path: &Path, keep: usize) -> String {
    let parts: Vec<String> = path.components().filter_map(|part| match part {
        Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
        _ => None,
    }).collect();
    let start = parts.len().saturating_sub(keep);
    format!("{}{}", if start > 0 { "…/" } else { "" }, parts[start..].join("/"))
}

/// Where a path is, as stored: inside the workspace relative to it. Outside
/// it only its last two parts: under the home folder `~/…/a/b`, elsewhere
/// `…/a/b` (review PR #11 B1: a path outside says no more than that).
fn display_path(path: &str, cwd: &Path) -> Option<String> {
    relative(path, cwd).or_else(|| {
        let home = std::env::var_os("HOME").map(PathBuf::from).filter(|home| home.is_absolute() && home != Path::new("/"));
        match home.and_then(|home| Path::new(path).strip_prefix(home).ok().map(Path::to_path_buf)) {
            Some(rest) if rest.as_os_str().is_empty() => Some("~".into()),
            Some(rest) => bounded(&format!("~/{}", tail(&rest, 2))),
            // Never reads like a workspace path: always `…/` first.
            None => bounded(&format!("…/{}", tail(Path::new(path), 2).trim_start_matches("…/"))),
        }
    })
}

// ---- Rows ----

fn row(action: &str, target: Option<String>, tool: Option<&str>, outcome: Option<&str>) -> Value {
    let mut value = json!({"action": action});
    if let Some(target) = target { value["target"] = json!(target); }
    if let Some(tool) = tool { value["tool"] = json!(tool); }
    if let Some(outcome) = outcome { value["outcome"] = json!(outcome); }
    value
}

fn id_shape(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-'))
}

/// The row as it may be stored: known fields only, every string through the
/// same masking and length rules, enum values and the app's id checked.
/// Every write goes through here (review PR #11 B1). None: not a row.
pub(super) fn sanitize(value: &Value) -> Option<Value> {
    const ACTIONS: &[&str] = &["read", "edit", "write", "delete", "search", "command", "other"];
    const OUTCOMES: &[&str] = &["refused", "failed", "stopped"];
    let object = value.as_object()?;
    let action = object.get("action")?.as_str().filter(|action| ACTIONS.contains(action))?;
    let mut out = serde_json::Map::new();
    out.insert("action".into(), json!(action));
    for (key, field) in object {
        let stored = match key.as_str() {
            "action" => continue,
            "outcome" => field.as_str().filter(|outcome| OUTCOMES.contains(outcome)).map(|outcome| json!(outcome)),
            "phase" => (field.as_str() == Some("done")).then(|| json!("done")),
            "id" => field.as_str().filter(|id| id_shape(id)).map(|id| json!(id)),
            "target" if action == "command" => field.as_str().and_then(stored_command).as_deref().and_then(bounded).map(|text| json!(text)),
            "target" | "tool" | "cwd" => field.as_str().and_then(bounded).map(|text| json!(text)),
            // A command line is redacted again here, whatever built the row.
            "command" => field.as_str().and_then(stored_command).map(|text| json!(text)),
            "argsHidden" => (field.as_bool() == Some(true)).then(|| json!(true)),
            "note" => field.as_str().and_then(|text| block(text, NOTE_MAX_CHARS)).map(|text| json!(text)),
            "exit" => field.as_i64().map(|code| json!(code)),
            "ms" | "added" | "removed" => field.as_u64().map(|number| json!(number)),
            _ => None,
        };
        if let Some(stored) = stored { out.insert(key.clone(), stored); }
    }
    Some(Value::Object(out))
}

/// A command as a row: the first line as the target, the whole command and
/// where it ran for the detail (masked as a whole first).
fn command_row(command: &str, cwd: Option<String>) -> Value {
    let redacted = redact_command(&neutralise(command, true));
    let mut value = row("command", bounded(&redacted.text), None, None);
    if let Some(command) = block(&redacted.text, BLOCK_MAX_CHARS) { value["command"] = json!(command); }
    if redacted.args_hidden { value["argsHidden"] = json!(true); }
    if let Some(cwd) = cwd { value["cwd"] = json!(cwd); }
    value
}

/// A stored command line: fail-closed redaction (`command_redact`), then the
/// masking and length rules of any block.
fn stored_command(text: &str) -> Option<String> {
    block(&redact_command(&neutralise(text, true)).text, BLOCK_MAX_CHARS)
}

/// "pattern · folder" for a search: the model's own pattern (never file
/// contents), and the folder only when it is not the whole workspace.
fn search_target(pattern: Option<&str>, path: Option<&str>, cwd: &Path) -> Option<String> {
    let pattern = pattern.and_then(bounded);
    let place = path.and_then(|path| display_path(path, cwd)).filter(|path| path != ".");
    match (pattern, place) {
        (Some(pattern), Some(place)) => bounded(&format!("{pattern} · {place}")),
        (pattern, place) => pattern.or(place),
    }
}

/// The host of a web address, nothing more.
fn domain(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?;
    bounded(host).filter(|host| !host.is_empty())
}

/// Claude asks before every tool use; record the app's answer. `id` is the
/// app's own key for this tool use (none when it was refused).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn claude(
    request: &elsecrew_core::provider::vendor_observation::claude_approval::Request,
    cwd: &Path,
    refused: bool,
    id: Option<&str>,
) -> Value {
    claude_row(&request.tool_name, request.target(), request.inspect_input(), id, cwd, refused)
}

fn claude_row(
    tool_name: &str,
    target: &elsecrew_core::provider::vendor_observation::claude_approval::ToolTarget,
    input: &str,
    id: Option<&str>,
    cwd: &Path,
    refused: bool,
) -> Value {
    use elsecrew_core::provider::vendor_observation::claude_approval::ToolTarget;
    let outcome = refused.then_some("refused");
    let input = serde_json::from_str::<Value>(input).unwrap_or(Value::Null);
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let mut value = match target {
        ToolTarget::File { path, .. } => {
            let action = match tool_name { "Read" => "read", "Write" => "write", _ => "edit" };
            row(action, display_path(path, cwd), None, outcome)
        }
        // What was looked for (the model's pattern, never file contents) and
        // where; the whole workspace needs no place. Glob finds file names,
        // Grep searches inside files.
        ToolTarget::Search { path } => {
            let tool = matches!(tool_name, "Glob" | "Grep").then_some(tool_name);
            row("search", search_target(text("pattern"), Some(path), cwd), tool, outcome)
        }
        // The command line itself (secrets masked), with the model's own
        // one-line summary as a note (user request 2026-11-26).
        ToolTarget::Shell => {
            let mut value = text("command").map_or_else(|| row("command", None, None, None), |command| command_row(command, None));
            if let Some(outcome) = outcome { value["outcome"] = json!(outcome); }
            if let Some(note) = text("description").and_then(|note| block(note, NOTE_MAX_CHARS)) { value["note"] = json!(note); }
            value
        }
        ToolTarget::Unsupported => {
            let target = match tool_name {
                "WebFetch" => text("url").and_then(domain),
                "WebSearch" => text("query").and_then(bounded),
                _ => None,
            };
            row("other", target, bounded(tool_name).as_deref(), outcome)
        }
    };
    if let Some(id) = id { value["id"] = json!(id); }
    value
}

/// How a Claude tool use ended, as the row that replaces its first row; None
/// when the result does not say whether it worked (review PR #11 I3). An exit
/// code is kept only when the result states one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn claude_done(
    started: &Value,
    result: &elsecrew_core::provider::vendor_observation::claude_events::ToolResult,
    elapsed: std::time::Duration,
) -> Option<Value> {
    let is_error = result.is_error?;
    let mut value = started.clone();
    value["phase"] = json!("done");
    value["ms"] = json!(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
    if value.get("outcome").is_none() {
        if result.interrupted { value["outcome"] = json!("stopped"); }
        else if is_error { value["outcome"] = json!("failed"); }
    }
    let action = started["action"].as_str().unwrap_or("other");
    if action == "command" {
        if let Some(code) = result.exit_code { value["exit"] = json!(code); }
    }
    if matches!(action, "edit" | "write") && !is_error {
        if let Some((added, removed)) = result.lines { value["added"] = json!(added); value["removed"] = json!(removed); }
    }
    Some(value)
}

/// A Codex tool use between its start and end: its rows as first stored, and
/// for a file change the files in order, so the end row keeps the start's
/// place and verb even when the end frame leaves fields out (review I4).
pub(super) struct CodexTool {
    sequence: u64,
    rows: Vec<Value>,
    paths: Vec<String>,
}

/// Open Codex tool uses of one run, by provider item id, and the counter for
/// the app's own ids.
#[derive(Default)]
pub(crate) struct CodexTools {
    open: BTreeMap<String, CodexTool>,
    next: u64,
}

impl CodexTools {
    fn new_id(&mut self) -> (u64, String) {
        self.next += 1;
        (self.next, format!("c{}", self.next))
    }
}

/// The rows a Codex tool use starts with.
fn codex_start(
    tool: &elsecrew_core::provider::vendor_observation::codex_events::stream::ToolEvent,
    cwd: &Path,
    id: &str,
) -> (Vec<Value>, Vec<String>) {
    use elsecrew_core::provider::vendor_observation::codex_events::stream::ToolKind;
    let detail = &tool.detail;
    match tool.kind {
        ToolKind::Command => {
            // Which command ran, so a list of commands is readable (user
            // request 2026-11-18): without the shell wrapper Codex adds.
            let command = tool.command.as_deref().map(unwrap_shell);
            let place = detail.cwd.as_deref().and_then(|place| display_path(place, cwd));
            let mut value = command.map_or_else(|| row("command", None, None, None), |command| command_row(command, place));
            // Codex's own reading of a simple command: reading one file,
            // listing or searching reads as that, the command stays in the detail.
            if let [action] = detail.actions.as_slice() {
                let path = action.path.as_deref();
                let (kind, tool, target) = match action.kind.as_str() {
                    "read" => ("read", None, path.and_then(|path| display_path(path, cwd))),
                    "search" => ("search", Some("Grep"), search_target(action.query.as_deref(), path, cwd)),
                    // A bare `ls` names no place: it stays a command.
                    "listFiles" if path.is_some() => ("search", Some("Glob"), path.and_then(|path| display_path(path, cwd))),
                    _ => ("command", None, None),
                };
                if kind != "command" {
                    value["action"] = json!(kind);
                    if let Some(tool) = tool { value["tool"] = json!(tool); }
                    // Without a place the command line stays the target.
                    if let Some(target) = target { value["target"] = json!(target); }
                }
            }
            value["id"] = json!(id);
            (vec![value], Vec::new())
        }
        ToolKind::FileChange => {
            if detail.changes.is_empty() {
                let mut value = row("edit", None, None, None);
                value["id"] = json!(id);
                return (vec![value], Vec::new());
            }
            detail.changes.iter().enumerate().map(|(index, change)| {
                let mut value = row(file_action(&change.kind), display_path(&change.path, cwd), None, None);
                value["id"] = json!(format!("{id}:{index}"));
                (value, change.path.clone())
            }).unzip()
        }
    }
}

fn file_action(kind: &str) -> &'static str {
    match kind { "add" => "write", "delete" => "delete", _ => "edit" }
}

/// Codex reports each command and file change when it starts and ends. The
/// app keeps the start (`tools`) and builds the end from it.
pub(super) fn codex(
    tools: &mut CodexTools,
    tool: &elsecrew_core::provider::vendor_observation::codex_events::stream::ToolEvent,
    cwd: &Path,
) -> Vec<Value> {
    use elsecrew_core::provider::vendor_observation::codex_events::stream::{ToolKind, ToolPhase};
    let outcome = match tool.phase {
        ToolPhase::Output => return Vec::new(),
        ToolPhase::Started => {
            let (sequence, id) = tools.new_id();
            let (rows, paths) = codex_start(tool, cwd, &id);
            if tools.open.len() >= OPEN_TOOLS_MAX {
                let oldest = tools.open.iter().min_by_key(|(_, open)| open.sequence).map(|(key, _)| key.clone());
                if let Some(oldest) = oldest { tools.open.remove(&oldest); }
            }
            tools.open.insert(tool.item_id.clone(), CodexTool { sequence, rows: rows.clone(), paths });
            return rows;
        }
        ToolPhase::Completed => None,
        ToolPhase::Failed => Some("failed"),
        ToolPhase::Declined => Some("refused"),
    };
    // The start as stored; an end seen without its start starts here.
    let start = tools.open.remove(&tool.item_id).unwrap_or_else(|| {
        let (sequence, id) = tools.new_id();
        let (rows, paths) = codex_start(tool, cwd, &id);
        CodexTool { sequence, rows, paths }
    });
    let detail = &tool.detail;
    let finish = |mut value: Value| {
        value["phase"] = json!("done");
        if let Some(outcome) = outcome { value["outcome"] = json!(outcome); }
        value
    };
    match tool.kind {
        ToolKind::Command => start.rows.into_iter().map(|mut value| {
            if let Some(exit) = detail.exit_code { value["exit"] = json!(exit); }
            if let Some(ms) = detail.duration_ms { value["ms"] = json!(ms); }
            finish(value)
        }).collect(),
        ToolKind::FileChange => {
            let base = start.rows.first().and_then(|row| row["id"].as_str()).map(|id| id.split(':').next().unwrap_or(id).to_string())
                .unwrap_or_else(|| tools.new_id().1);
            let mut rows: Vec<Value> = Vec::new();
            for (index, mut value) in start.rows.into_iter().enumerate() {
                // The same file by its path, not by its place in the list.
                let change = start.paths.get(index).and_then(|path| detail.changes.iter().find(|change| &change.path == path));
                if let (Some(change), None) = (change, outcome) { value["added"] = json!(change.added); value["removed"] = json!(change.removed); }
                rows.push(finish(value));
            }
            // Files the end names that the start did not.
            let mut next = start.paths.len();
            for change in detail.changes.iter().filter(|change| !start.paths.contains(&change.path)) {
                let mut value = row(file_action(&change.kind), display_path(&change.path, cwd), None, None);
                value["id"] = json!(format!("{base}:{next}"));
                next += 1;
                if outcome.is_none() { value["added"] = json!(change.added); value["removed"] = json!(change.removed); }
                rows.push(finish(value));
            }
            // A start that named no file is replaced by the files the end names.
            if !start.paths.is_empty() || rows.len() == 1 { rows } else { rows.into_iter().skip(1).collect() }
        }
    }
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
    /// The only way a work history row is stored; it is sanitized here.
    pub(super) fn record_activity(&self, storage: &mut Storage, activity: Value) {
        let Some(activity) = sanitize(&activity) else { return };
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

    /// Inputs are synthetic; fake values say so (FAKE/example).
    fn cases() -> Value {
        serde_json::from_str(include_str!("../../../../../tests/fixtures/m5/secret-mask-cases.json")).unwrap()
    }

    #[test]
    fn paths_stay_inside_the_workspace_or_say_little_outside() {
        let cwd = Path::new("/work/copy");
        assert_eq!(relative("/work/copy/src/app.ts", cwd).as_deref(), Some("src/app.ts"));
        assert_eq!(relative("src/app.ts", cwd).as_deref(), Some("src/app.ts"));
        assert_eq!(relative("/work/copy", cwd).as_deref(), Some("."));
        assert_eq!(relative("/elsewhere/secret.txt", cwd), None);
        assert_eq!(relative("/work/copy/../secret.txt", cwd), None);
        assert_eq!(relative("../secret.txt", cwd), None);
        assert_eq!(display_path("/elsewhere/deep/folder/notes.txt", cwd).as_deref(), Some("…/folder/notes.txt"));
        assert_eq!(display_path("../up/x.txt", cwd).as_deref(), Some("…/up/x.txt"));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from).filter(|home| home.is_absolute() && home != Path::new("/")) {
            assert_eq!(display_path(home.join(".ssh/config").to_str().unwrap(), cwd).as_deref(), Some("~/.ssh/config"));
            assert_eq!(display_path(home.join("a/b/c/d.txt").to_str().unwrap(), cwd).as_deref(), Some("~/…/c/d.txt"));
        }
        // A key-shaped part of a path is masked like any other text (B1).
        let key = format!("sk-{}", "FAKEexample".repeat(2));
        assert_eq!(display_path(&format!("/work/copy/keys/{key}.txt"), cwd).as_deref(), Some("keys/sk-••••.txt"));
        assert_eq!(display_path(&format!("/srv/{key}/x"), cwd).as_deref(), Some("…/sk-••••/x"));
    }

    #[test]
    fn lines_are_masked_then_cut_with_marks_neutralised() {
        assert_eq!(bounded("Run tests\nrm -rf /").as_deref(), Some("Run tests"));
        assert_eq!(bounded("  ").as_deref(), None);
        assert_eq!(bounded("file\u{202e}txt.exe").as_deref(), Some("file\u{fffd}txt.exe"));
        assert_eq!(bounded("a\u{1b}[31mb\u{2028}c").as_deref(), Some("a\u{fffd}[31mb\u{fffd}c"));
        assert_eq!(bounded(&"a".repeat(400)).map(|text| text.chars().count()), Some(TARGET_MAX_CHARS + 1));
        assert_eq!(block(&format!("{}\n", "x".repeat(9000)), BLOCK_MAX_CHARS).map(|text| text.chars().count()), Some(BLOCK_MAX_CHARS + 1));
        assert_eq!(block("one\r\ntwo\u{7}", 100).as_deref(), Some("one\ntwo\u{fffd}"));
    }

    #[test]
    fn secrets_are_masked_like_the_screen_does() {
        for case in cases()["mask"].as_array().unwrap() {
            // Shapes a secret scanner reads as real are stored with {colon}/{dot}.
            let text = &case["text"].as_str().unwrap().replace("{colon}", ":").replace("{dot}", ".");
            let masked = mask_secrets(text);
            assert_eq!(masked, case["masked"].as_str().unwrap(), "{text:?}");
            assert_eq!(mask_secrets(&masked), masked, "masking twice changes nothing: {text:?}");
        }
        for case in cases()["line"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            assert_eq!(display_line(text), case["shown"].as_str().unwrap(), "{text:?}");
        }
    }

    /// Fail-closed command lines (review PR #11 B2, round 2): the shared
    /// cases give the same result as the screen, redacting twice changes
    /// nothing, and no synthetic secret (every one says "fake") survives, in
    /// the command cases nor in any line of the masking table.
    #[test]
    fn commands_are_redacted_fail_closed_like_the_screen() {
        let expand = |text: &str| text.replace("{colon}", ":").replace("{dot}", ".");
        let cases = cases();
        for case in cases["command"].as_array().unwrap() {
            let text = expand(case["text"].as_str().unwrap());
            let redacted = redact_command(&text);
            assert_eq!(redacted.text, case["shown"].as_str().unwrap(), "{text:?}");
            assert_eq!(redacted.args_hidden, case["argsHidden"].as_bool().unwrap(), "{text:?}");
            assert_eq!(redact_command(&redacted.text).text, redacted.text, "redacting twice: {text:?}");
            assert!(!redacted.text.to_lowercase().contains("fake"), "{text:?} => {}", redacted.text);
        }
        for case in cases["mask"].as_array().unwrap() {
            let text = expand(case["text"].as_str().unwrap());
            let stored = stored_command(&text).unwrap_or_default();
            assert!(!stored.to_lowercase().contains("fake") && !stored.contains("AKIAIOSFODNN7"), "{text:?} => {stored}");
        }
    }

    #[test]
    fn every_stored_field_is_sanitized() {
        let key = format!("ghp_{}", "FAKEexample0".repeat(2));
        let row = json!({"action":"command","target":format!("echo {key}"),"command":format!("TOKEN=FAKEexample echo\n{key}"),
            "cwd":format!("x/{key}"),"tool":key.clone(),"note":format!("uses {key}"),"id":"c1","exit":0,"ms":5,
            "output":"never stored","extra":"x","phase":"done","outcome":"failed"});
        let stored = sanitize(&row).unwrap();
        let text = stored.to_string();
        assert!(!text.contains("FAKEexample0FAKE"), "{text}");
        assert!(!text.contains("never stored") && !text.contains("extra"), "{text}");
        // Several lines cannot be read safely: only the program name is kept.
        assert_eq!(stored["command"], "echo");
        assert_eq!(sanitize(&json!({"action":"command","id":"toolu_01ABC/../x"})).unwrap().get("id"), None, "provider-shaped ids are not keys");
        assert_eq!(sanitize(&json!({"action":"move"})), None);
        assert_eq!(sanitize(&json!({"action":"read","outcome":"maybe","phase":"later"})).unwrap(), json!({"action":"read"}));
    }

    #[test]
    fn searches_show_what_was_looked_for_and_where() {
        let cwd = Path::new("/work/copy");
        assert_eq!(search_target(Some("**/*.md"), Some("/work/copy"), cwd).as_deref(), Some("**/*.md"));
        assert_eq!(search_target(Some("TODO"), Some("/work/copy/src"), cwd).as_deref(), Some("TODO · src"));
        assert_eq!(search_target(Some("a\nrm -rf /"), Some("/work/copy"), cwd).as_deref(), Some("a"));
        assert_eq!(search_target(None, Some("/work/copy/docs"), cwd).as_deref(), Some("docs"));
        assert_eq!(domain("https://user:pw@docs.example.com/a?b").as_deref(), Some("docs.example.com"));
        // Masked before the cut: a key at the cut boundary leaves nothing (B2).
        let pattern = format!("{} sk-{}", "p".repeat(150), "FAKEexample".repeat(3));
        let shown = search_target(Some(&pattern), None, cwd).unwrap();
        assert!(!shown.contains("FAKE"), "{shown}");
    }

    fn event(kind: elsecrew_core::provider::vendor_observation::codex_events::stream::ToolKind,
        phase: elsecrew_core::provider::vendor_observation::codex_events::stream::ToolPhase,
        command: Option<&str>,
        detail: elsecrew_core::provider::vendor_observation::codex_events::stream::ToolDetail,
    ) -> elsecrew_core::provider::vendor_observation::codex_events::stream::ToolEvent {
        elsecrew_core::provider::vendor_observation::codex_events::stream::ToolEvent { item_id: "i".into(), kind, phase, command: command.map(String::from), detail }
    }

    #[test]
    fn codex_commands_show_the_command_and_how_it_ended() {
        use elsecrew_core::provider::vendor_observation::codex_events::stream::{CommandAction, ToolDetail, ToolKind, ToolPhase};
        let cwd = Path::new("/work/copy");
        let mut tools = CodexTools::default();
        let started = |tools: &mut CodexTools, command: &str| codex(tools, &event(ToolKind::Command, ToolPhase::Started, Some(command), ToolDetail { cwd: Some("/work/copy".into()), ..Default::default() }), cwd);
        assert_eq!(started(&mut tools, r#"/bin/zsh -lc "npm test""#), vec![json!({"action":"command","target":"npm test","command":"npm test","cwd":".","id":"c1"})]);
        let done = codex(&mut tools, &event(ToolKind::Command, ToolPhase::Completed, Some("/bin/zsh -lc \"npm test\""), ToolDetail {
            cwd: Some("/work/copy".into()), exit_code: Some(0), duration_ms: Some(1200), ..Default::default()
        }), cwd);
        assert_eq!(done, vec![json!({"action":"command","target":"npm test","command":"npm test","cwd":".","id":"c1","exit":0,"ms":1200,"phase":"done"})]);
        let lines = started(&mut tools, "echo one\nrm -rf /");
        assert_eq!((lines[0]["command"].clone(), lines[0]["argsHidden"].clone()), (json!("echo"), json!(true)));
        let token = format!("GITHUB_TOKEN=ghp_{} gh pr list", "FAKEexample0".repeat(2));
        assert_eq!(started(&mut tools, &token)[0]["target"], "GITHUB_TOKEN=•••• gh pr list");
        assert_eq!(codex(&mut tools, &event(ToolKind::Command, ToolPhase::Output, None, ToolDetail::default()), cwd), Vec::<Value>::new());
        // Read as reading, and the end keeps the start's verb and place even
        // when the end frame leaves the command's reading out (I4).
        let mut tools = CodexTools::default();
        let read = codex(&mut tools, &event(ToolKind::Command, ToolPhase::Started, Some("/bin/zsh -lc 'sed -n 1,80p src/app.js'"), ToolDetail {
            cwd: Some("/work/copy".into()), actions: vec![CommandAction { kind: "read".into(), path: Some("/work/copy/src/app.js".into()), query: None }], ..Default::default()
        }), cwd);
        assert_eq!(read, vec![json!({"action":"read","target":"src/app.js","command":"sed -n •••• src/app.js","cwd":".","id":"c1"})]);
        let end = codex(&mut tools, &event(ToolKind::Command, ToolPhase::Failed, None, ToolDetail { exit_code: Some(1), ..Default::default() }), cwd);
        assert_eq!(end, vec![json!({"action":"read","target":"src/app.js","command":"sed -n •••• src/app.js","cwd":".","id":"c1","exit":1,"outcome":"failed","phase":"done"})]);
        let declined = codex(&mut CodexTools::default(), &event(ToolKind::Command, ToolPhase::Declined, None, ToolDetail::default()), cwd);
        assert_eq!(declined, vec![json!({"action":"command","id":"c1","outcome":"refused","phase":"done"})]);
    }

    #[test]
    fn codex_file_changes_name_each_file_and_keep_their_place() {
        use elsecrew_core::provider::vendor_observation::codex_events::stream::{FileChange, ToolDetail, ToolKind, ToolPhase};
        let cwd = Path::new("/work/copy");
        let change = |path: &str, kind: &str, added, removed| FileChange { path: path.into(), kind: kind.into(), added, removed };
        let mut tools = CodexTools::default();
        let start = ToolDetail { changes: vec![change("/work/copy/src/app.ts", "update", 0, 0), change("/work/copy/new.md", "add", 0, 0), change("/work/copy/old.md", "delete", 0, 0)], ..Default::default() };
        assert_eq!(codex(&mut tools, &event(ToolKind::FileChange, ToolPhase::Started, None, start), cwd), vec![
            json!({"action":"edit","target":"src/app.ts","id":"c1:0"}),
            json!({"action":"write","target":"new.md","id":"c1:1"}),
            json!({"action":"delete","target":"old.md","id":"c1:2"}),
        ]);
        // The end lists the files in another order and adds one: matched by path.
        let end = ToolDetail { changes: vec![change("/work/copy/new.md", "add", 10, 0), change("/work/copy/src/app.ts", "update", 3, 1), change("/work/copy/extra.md", "add", 2, 0)], ..Default::default() };
        let done = codex(&mut tools, &event(ToolKind::FileChange, ToolPhase::Completed, None, end), cwd);
        assert_eq!(done, vec![
            json!({"action":"edit","target":"src/app.ts","id":"c1:0","added":3,"removed":1,"phase":"done"}),
            json!({"action":"write","target":"new.md","id":"c1:1","added":10,"removed":0,"phase":"done"}),
            json!({"action":"delete","target":"old.md","id":"c1:2","phase":"done"}),
            json!({"action":"write","target":"extra.md","id":"c1:3","added":2,"removed":0,"phase":"done"}),
        ]);
        // An end with no file list keeps the start's files (I4).
        let mut tools = CodexTools::default();
        codex(&mut tools, &event(ToolKind::FileChange, ToolPhase::Started, None, ToolDetail { changes: vec![change("/work/copy/a.ts", "update", 0, 0)], ..Default::default() }), cwd);
        assert_eq!(codex(&mut tools, &event(ToolKind::FileChange, ToolPhase::Completed, None, ToolDetail::default()), cwd),
            vec![json!({"action":"edit","target":"a.ts","id":"c1:0","phase":"done"})]);
    }

    #[test]
    fn claude_done_rows_keep_the_start_and_add_only_what_is_known() {
        use elsecrew_core::provider::vendor_observation::claude_events::ToolResult;
        let started = json!({"action":"command","target":"npm test","command":"npm test","id":"t1"});
        let failed = ToolResult { tool_use_id: "x".into(), is_error: Some(true), exit_code: Some(2), ..Default::default() };
        assert_eq!(claude_done(&started, &failed, std::time::Duration::from_millis(1500)),
            Some(json!({"action":"command","target":"npm test","command":"npm test","id":"t1","phase":"done","ms":1500,"outcome":"failed","exit":2})));
        // Success says no exit code; none is made up (I3).
        let ok = ToolResult { tool_use_id: "x".into(), is_error: Some(false), ..Default::default() };
        assert_eq!(claude_done(&started, &ok, std::time::Duration::from_millis(5)).unwrap().get("exit"), None);
        let stopped = ToolResult { tool_use_id: "x".into(), is_error: Some(false), interrupted: true, ..Default::default() };
        assert_eq!(claude_done(&started, &stopped, std::time::Duration::from_millis(5)).unwrap()["outcome"], "stopped");
        let unknown = ToolResult { tool_use_id: "x".into(), is_error: None, ..Default::default() };
        assert_eq!(claude_done(&started, &unknown, std::time::Duration::from_millis(5)), None);
        let edit = json!({"action":"edit","target":"src/a.ts","id":"t2"});
        let lines = ToolResult { tool_use_id: "x".into(), is_error: Some(false), lines: Some((4, 2)), ..Default::default() };
        let done = claude_done(&edit, &lines, std::time::Duration::from_millis(5)).unwrap();
        assert_eq!((done["added"].clone(), done["removed"].clone()), (json!(4), json!(2)));
    }

    /// Development replay, not a gate: recorded provider frames (Claude
    /// stream-json, Codex app-server JSON-RPC) through the real parsers and
    /// row builders and `sanitize`, written as JSON lines for a screen capture.
    /// `ELSECREW_ACTIVITY_REPLAY=<claude.jsonl>|<codex.jsonl>|<cwd claude>|<cwd codex>|<out.json>`
    #[test]
    #[ignore]
    fn replay_recorded_frames() {
        use elsecrew_core::provider::BoundedObservation;
        use elsecrew_core::provider::vendor_observation::{claude_approval::ToolTarget, claude_events::{Projection, SessionProjection}, codex_events::stream::{Observed, TurnStream}};
        let Ok(spec) = std::env::var("ELSECREW_ACTIVITY_REPLAY") else { return };
        let parts: Vec<&str> = spec.split('|').collect();
        let observe = |value: &Value| BoundedObservation::new(value.to_string().into_bytes()).unwrap();
        // Claude: each tool_use becomes the permission request the app would see.
        let mut claude_rows = Vec::new();
        let mut started = BTreeMap::new();
        let mut projection: Option<SessionProjection> = None;
        let mut next = 0;
        for line in std::fs::read_to_string(parts[0]).unwrap().lines() {
            let frame: Value = serde_json::from_str(line).unwrap();
            let session = frame["session_id"].as_str().unwrap_or("s").to_string();
            let projection = projection.get_or_insert_with(|| SessionProjection::new(&session).unwrap());
            if frame["type"] == "assistant" {
                for block in frame["message"]["content"].as_array().unwrap() {
                    if block["type"] != "tool_use" { continue; }
                    // The target the permission request carries (claude_approval::tool_target).
                    let name = block["name"].as_str().unwrap();
                    let input = &block["input"];
                    let target = match name {
                        "Read" | "Write" | "Edit" => ToolTarget::File { path: input["file_path"].as_str().unwrap().into(), writes: name != "Read" },
                        "Glob" | "Grep" => ToolTarget::Search { path: input["path"].as_str().unwrap_or(parts[2]).into() },
                        "Bash" => ToolTarget::Shell,
                        _ => ToolTarget::Unsupported,
                    };
                    next += 1;
                    let row = claude_row(name, &target, &input.to_string(), Some(&format!("t{next}")), Path::new(parts[2]), false);
                    started.insert(block["id"].as_str().unwrap().to_string(), (row.clone(), frame["timestamp"].as_str().map(String::from)));
                    claude_rows.extend(sanitize(&row));
                }
            } else if frame["type"] == "user" {
                if let Projection::ToolResults(results) = projection.project(&observe(&frame)).unwrap() {
                    for result in results {
                        // The recorded frames' own times stand in for the app's clock.
                        let at = |time: Option<&str>| time.and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok());
                        if let Some((row, asked)) = started.remove(&result.tool_use_id) {
                            let elapsed = match (at(asked.as_deref()), at(frame["timestamp"].as_str())) {
                                (Some(asked), Some(done)) => (done - asked).to_std().unwrap_or_default(),
                                _ => std::time::Duration::ZERO,
                            };
                            claude_rows.extend(claude_done(&row, &result, elapsed).as_ref().and_then(sanitize));
                        }
                    }
                }
            }
        }
        // Codex: the turn's notifications through the turn stream.
        let mut codex_rows = Vec::new();
        let mut tools = CodexTools::default();
        let mut stream: Option<TurnStream> = None;
        for line in std::fs::read_to_string(parts[1]).unwrap().lines() {
            let frame: Value = serde_json::from_str(line).unwrap();
            let Some(method) = frame["method"].as_str() else { continue };
            if frame.get("id").is_some() { continue; }
            if method == "turn/started" {
                stream = Some(TurnStream::with_budget(frame["params"]["threadId"].as_str().unwrap(), frame["params"]["turn"]["id"].as_str().unwrap(), elsecrew_core::provider::ProviderEventBudget::DurableV1).unwrap());
            }
            let Some(stream) = stream.as_mut() else { continue };
            match stream.observe(&observe(&json!({"method":method,"params":frame["params"]}))) {
                Ok(Observed::Tool(tool)) => codex_rows.extend(codex(&mut tools, &tool, Path::new(parts[3])).iter().filter_map(sanitize)),
                Ok(_) => {}
                Err(_) => panic!("codex replay stopped at {method}: {}", stream.diagnostic_stage()),
            }
        }
        std::fs::write(parts[4], serde_json::to_string_pretty(&json!({"claude":claude_rows,"codex":codex_rows})).unwrap()).unwrap();
    }
}
