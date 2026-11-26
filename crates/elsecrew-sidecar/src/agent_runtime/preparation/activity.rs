//! Work history rows for the team chat: one compact line per tool use, so a
//! long run visibly moves. A row says what kind of work (read, edit,
//! command…) and on what: the file inside the workspace, the search, or the
//! command line. When the tool use ends a second row with the same `id`
//! carries how it ended (exit code, time, changed lines, the first lines of
//! a command's output); the screen shows it in place of the first.
//!
//! What is stored is display text, never authority: one bounded line for the
//! target, control and direction marks neutralised, and anything that looks
//! like a secret replaced by •••• before it is written (`mask_secrets`; the
//! screen masks again with the same rules, `src/m5/secret-shape.ts`). File
//! contents are never stored; only a command's own output is, and only its
//! first lines. A row that cannot be stored never stops the run.
use super::*;
use elsecrew_protocol::agent_adapter_v1::output_pages::{
    OutputItemChannel, OutputItemOperation, OutputItemUpdate,
};
use std::path::{Component, Path, PathBuf};

const TARGET_MAX_CHARS: usize = 160;
const BLOCK_MAX_CHARS: usize = 4000;
const NOTE_MAX_CHARS: usize = 300;
const MASK: &str = "••••";

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

/// Where a path is, as the screen shows it: inside the workspace relative to
/// it; elsewhere under the home folder as `~/…`; otherwise as given.
fn display_path(path: &str, cwd: &Path) -> Option<String> {
    relative(path, cwd).or_else(|| {
        let home = std::env::var_os("HOME").map(PathBuf::from).filter(|home| home.is_absolute() && home != Path::new("/"));
        match home.and_then(|home| Path::new(path).strip_prefix(home).ok().map(Path::to_path_buf)) {
            Some(rest) if rest.as_os_str().is_empty() => Some("~".into()),
            Some(rest) => bounded(&format!("~/{}", rest.to_string_lossy())),
            None => bounded(path),
        }
    })
}

/// Invisible format marks that can reorder or hide what the row shows.
fn hidden_format(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}')
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

/// One line: the first, trimmed, neutralised and bounded.
fn bounded(text: &str) -> Option<String> {
    let line = text.split(['\n', '\r']).find(|line| !line.trim().is_empty())?.trim();
    Some(cut(&neutralise(line, false), TARGET_MAX_CHARS))
}

/// Several lines (a whole command, an output head), masked and bounded.
fn block(text: &str, max: usize) -> Option<String> {
    let text = text.trim_matches('\n').trim_end();
    if text.trim().is_empty() { return None; }
    Some(cut(&mask_secrets(&neutralise(text, true)), max))
}

// ---- Secrets: the same rules as `maskSecrets` in src/m5/secret-shape.ts ----

fn secret_name(name: &str) -> bool {
    const WORDS: &[&str] = &["token", "secret", "password", "passwd", "pwd", "pass", "passphrase", "apikey", "auth", "credential", "credentials", "cookie", "privatekey", "accesskey", "authorization"];
    const KEY_PARTS: &[&str] = &["api", "access", "private", "secret", "signing", "encryption"];
    const ENDINGS: &[&str] = &["token", "secret", "password", "apikey"];
    let name = name.trim_start_matches('-');
    let mut words = String::new();
    let mut previous: Option<char> = None;
    for c in name.chars() {
        if c.is_ascii_uppercase() && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) { words.push('_'); }
        words.push(c.to_ascii_lowercase());
        previous = Some(c);
    }
    let segments: Vec<&str> = words.split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit()).filter(|s| !s.is_empty()).collect();
    let flat = segments.concat();
    segments.iter().any(|segment| WORDS.contains(segment))
        || segments.windows(2).any(|pair| pair[1] == "key" && KEY_PARTS.contains(&pair[0]))
        || ENDINGS.iter().any(|ending| flat.ends_with(ending))
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn mask_shapes(word: &str) -> String {
    struct Shape { prefix: &'static str, min: usize, chars: fn(char) -> bool, exact: bool }
    fn token(c: char) -> bool { c.is_ascii_alphanumeric() || c == '_' || c == '-' }
    fn alnum(c: char) -> bool { c.is_ascii_alphanumeric() }
    let shapes = [
        Shape { prefix: "github_pat_", min: 20, chars: |c| c.is_ascii_alphanumeric() || c == '_', exact: false },
        Shape { prefix: "sk-", min: 16, chars: token, exact: false },
        Shape { prefix: "ghp_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "gho_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "ghu_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "ghs_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "ghr_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "xoxa-", min: 10, chars: |c| c.is_ascii_alphanumeric() || c == '-', exact: false },
        Shape { prefix: "xoxb-", min: 10, chars: |c| c.is_ascii_alphanumeric() || c == '-', exact: false },
        Shape { prefix: "xoxp-", min: 10, chars: |c| c.is_ascii_alphanumeric() || c == '-', exact: false },
        Shape { prefix: "xoxo-", min: 10, chars: |c| c.is_ascii_alphanumeric() || c == '-', exact: false },
        Shape { prefix: "xoxs-", min: 10, chars: |c| c.is_ascii_alphanumeric() || c == '-', exact: false },
        Shape { prefix: "xoxr-", min: 10, chars: |c| c.is_ascii_alphanumeric() || c == '-', exact: false },
        Shape { prefix: "glpat-", min: 20, chars: token, exact: false },
        Shape { prefix: "AIza", min: 30, chars: token, exact: false },
        Shape { prefix: "AKIA", min: 16, chars: |c| c.is_ascii_digit() || c.is_ascii_uppercase(), exact: true },
        Shape { prefix: "ASIA", min: 16, chars: |c| c.is_ascii_digit() || c.is_ascii_uppercase(), exact: true },
        Shape { prefix: "eyJ", min: 30, chars: |c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'), exact: false },
    ];
    let mut out = String::new();
    let mut index = 0;
    let mut previous: Option<char> = None;
    'outer: while index < word.len() {
        if !previous.is_some_and(|p| p.is_ascii_alphanumeric()) {
            for shape in &shapes {
                if !word[index..].starts_with(shape.prefix) { continue; }
                let start = index + shape.prefix.len();
                let run: String = word[start..].chars().take_while(|c| (shape.chars)(*c)).collect();
                let end = start + run.len();
                let shaped = if shape.exact {
                    run.len() == shape.min && !word[end..].chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
                } else {
                    run.len() >= shape.min && (shape.prefix != "eyJ" || run.split('.').count() >= 3)
                };
                if !shaped { continue; }
                out.push_str(shape.prefix);
                out.push_str(MASK);
                previous = run.chars().last();
                index = end;
                continue 'outer;
            }
        }
        let c = word[index..].chars().next().unwrap();
        out.push(c);
        previous = Some(c);
        index += c.len_utf8();
    }
    out
}

fn mask_url(word: &str) -> String {
    let Some(scheme) = word.find("://") else { return word.into() };
    let start = scheme + 3;
    let Some(at) = word[start..].find('@').map(|i| i + start) else { return word.into() };
    if word[start..].find('/').map(|i| i + start).is_some_and(|slash| slash < at) { return word.into(); }
    match word[start..].find(':').map(|i| i + start) {
        Some(colon) if colon < at && colon + 1 != at => format!("{}{MASK}{}", &word[..=colon], &word[at..]),
        _ => word.into(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Expect { Value, Long }
#[derive(Default)]
struct MaskState { expect: Option<Expect>, quote: Option<char>, line: bool }

fn mask_value(value: &str, state: &mut MaskState) -> String {
    match value.chars().next() {
        Some(quote @ ('"' | '\'')) => match value[1..].find(quote) {
            None => { state.quote = Some(quote); format!("{quote}{MASK}") }
            Some(close) => format!("{quote}{MASK}{}", &value[1 + close..]),
        },
        _ => {
            let body = value.trim_end_matches([',', ';', ')', '}', ']']);
            format!("{MASK}{}", &value[body.len()..])
        }
    }
}

fn strip_name(text: &str) -> &str {
    text.trim_start_matches(['-', '{', '(', '[', '"', '\'']).trim_end_matches(['"', '\''])
}

fn mask_word(word: &str, state: &mut MaskState) -> String {
    let core = word.trim_start_matches(['{', '(', '[', '"', '\'']);
    let opening = &word[..word.len() - core.len()];
    let lower = core.to_ascii_lowercase();
    if lower.starts_with("authorization:") || lower.starts_with("authorization=") {
        let head = &core[.."authorization:".len()];
        let rest = &core["authorization:".len()..];
        state.line = true;
        if rest.is_empty() { return word.into(); }
        let quote = rest.chars().last().filter(|c| matches!(c, '"' | '\''));
        if quote.is_some() { state.line = false; }
        return format!("{opening}{head}{MASK}{}", quote.map(String::from).unwrap_or_default());
    }
    if let Some(equals) = word.find('=').filter(|&i| i > 0) {
        let name = strip_name(&word[..equals]);
        if identifier(name) && secret_name(name) {
            let value = &word[equals + 1..];
            if value.is_empty() { state.expect = Some(Expect::Value); return word.into(); }
            return format!("{}{}", &word[..=equals], mask_value(value, state));
        }
    }
    if let Some(colon) = word.find(':').filter(|&i| i > 0 && !word[i..].starts_with("://")) {
        let name = strip_name(&word[..colon]);
        if identifier(name) && secret_name(name) {
            let value = &word[colon + 1..];
            if value.is_empty() { state.expect = Some(Expect::Value); return word.into(); }
            return format!("{}{}", &word[..=colon], mask_value(value, state));
        }
    }
    let flag = word.trim_start_matches('-');
    if word.starts_with('-') && word.len() - flag.len() <= 2 && flag.starts_with(|c: char| c.is_ascii_alphabetic())
        && identifier(flag) && secret_name(word) {
        state.expect = Some(Expect::Value);
        return word.into();
    }
    if word == "Bearer" || word == "Basic" { state.expect = Some(Expect::Long); return word.into(); }
    mask_url(&mask_shapes(word))
}

fn mask_private_keys(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(begin) = rest.find("-----BEGIN ") {
        let after = &rest[begin + "-----BEGIN ".len()..];
        let label_len = after.chars().take_while(|c| c.is_ascii_uppercase() || *c == ' ').map(char::len_utf8).sum::<usize>();
        let label = &after[..label_len];
        if !(label.ends_with("PRIVATE KEY") && after[label_len..].starts_with("-----")) {
            out.push_str(&rest[..begin + "-----BEGIN ".len()]);
            rest = after;
            continue;
        }
        out.push_str(&rest[..begin]);
        // Not a BEGIN line itself, so masking twice changes nothing.
        out.push_str("PRIVATE KEY ");
        out.push_str(MASK);
        let body = &after[label_len + 5..];
        rest = match find_key_end(body) { Some(end) => &body[end..], None => "" };
    }
    out.push_str(rest);
    out
}

/// The end of `-----END … PRIVATE KEY-----` in `body`, if it is there.
fn find_key_end(body: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = body[from..].find("-----END ") {
        let start = from + found + "-----END ".len();
        let label_len = body[start..].chars().take_while(|c| c.is_ascii_uppercase() || *c == ' ').map(char::len_utf8).sum::<usize>();
        if body[start..start + label_len].ends_with("PRIVATE KEY") && body[start + label_len..].starts_with("-----") {
            return Some(start + label_len + 5);
        }
        from = start;
    }
    None
}

/// Text with the parts that look like secrets replaced by ••••.
pub(super) fn mask_secrets(text: &str) -> String {
    let text = mask_private_keys(text);
    let mut state = MaskState::default();
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(first) = rest.chars().next() {
        let space = first.is_whitespace();
        let len = rest.find(|c: char| c.is_whitespace() != space).unwrap_or(rest.len());
        let piece = &rest[..len];
        rest = &rest[len..];
        if space {
            if piece.contains('\n') { state = MaskState::default(); }
            out.push_str(piece);
            continue;
        }
        if state.line {
            let quote = piece.chars().last().filter(|c| matches!(c, '"' | '\''));
            if quote.is_some() { state.line = false; }
            out.push_str(MASK);
            if let Some(quote) = quote { out.push(quote); }
            continue;
        }
        if let Some(quote) = state.quote {
            match piece.find(quote) {
                None => out.push_str(MASK),
                Some(close) => { out.push_str(MASK); out.push_str(&piece[close..]); state.quote = None; }
            }
            continue;
        }
        if let Some(expect) = state.expect.take() {
            if (expect == Expect::Value && !piece.starts_with('-')) || (expect == Expect::Long && piece.encode_utf16().count() >= 8) {
                out.push_str(&mask_value(piece, &mut state));
                continue;
            }
        }
        out.push_str(&mask_word(piece, &mut state));
    }
    out
}

// ---- Rows ----

fn row(action: &str, target: Option<String>, tool: Option<&str>, outcome: Option<&str>) -> Value {
    let mut value = json!({"action": action});
    if let Some(target) = target { value["target"] = json!(target); }
    if let Some(tool) = tool { value["tool"] = json!(tool); }
    if let Some(outcome) = outcome { value["outcome"] = json!(outcome); }
    value
}

fn with_id(mut value: Value, id: Option<&str>) -> Value {
    if let Some(id) = id.filter(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(|c| c.is_control() || hidden_format(c))) {
        value["id"] = json!(id);
    }
    value
}

/// A command as a row: the first line as the target, the whole (masked)
/// command and where it ran for the detail.
fn command_row(command: &str, cwd: Option<String>) -> Value {
    let masked = mask_secrets(&neutralise(command, true));
    let mut value = row("command", bounded(&masked), None, None);
    if let Some(command) = block(command, BLOCK_MAX_CHARS) { value["command"] = json!(command); }
    if let Some(cwd) = cwd { value["cwd"] = json!(cwd); }
    value
}

/// "pattern · folder" for a search: the model's own pattern (never file
/// contents), and the folder only when it is not the whole workspace.
fn search_target(pattern: Option<&str>, path: Option<&str>, cwd: &Path) -> Option<String> {
    let pattern = pattern.and_then(bounded).map(|pattern| mask_secrets(&pattern));
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

/// Claude asks before every tool use; record the app's answer.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn claude(
    request: &elsecrew_core::provider::vendor_observation::claude_approval::Request,
    cwd: &Path,
    refused: bool,
) -> Value {
    claude_row(&request.tool_name, request.target(), request.inspect_input(), request.tool_use_id.as_deref(), cwd, refused)
}

fn claude_row(
    tool_name: &str,
    target: &elsecrew_core::provider::vendor_observation::claude_approval::ToolTarget,
    input: &str,
    tool_use_id: Option<&str>,
    cwd: &Path,
    refused: bool,
) -> Value {
    use elsecrew_core::provider::vendor_observation::claude_approval::ToolTarget;
    let outcome = refused.then_some("refused");
    let input = serde_json::from_str::<Value>(input).unwrap_or(Value::Null);
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let value = match target {
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
                "WebSearch" => text("query").and_then(bounded).map(|query| mask_secrets(&query)),
                _ => None,
            };
            row("other", target, Some(tool_name), outcome)
        }
    };
    with_id(value, tool_use_id)
}

/// How a Claude tool use ended, as the row that replaces its first row.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn claude_done(
    started: &Value,
    result: &elsecrew_core::provider::vendor_observation::claude_events::ToolResult,
    elapsed: std::time::Duration,
) -> Value {
    let mut value = started.clone();
    value["phase"] = json!("done");
    value["ms"] = json!(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
    let action = started["action"].as_str().unwrap_or("other");
    if result.is_error && value.get("outcome").is_none() { value["outcome"] = json!("failed"); }
    if action == "command" {
        // A failed command says its exit code; a clean one ended with 0.
        match (result.exit_code, result.is_error) {
            (Some(code), _) => value["exit"] = json!(code),
            (None, false) => value["exit"] = json!(0),
            (None, true) => {}
        }
        if let Some(output) = result.output.as_deref().and_then(|output| block(output, BLOCK_MAX_CHARS)) { value["output"] = json!(output); }
    }
    if matches!(action, "edit" | "write") {
        if let Some((added, removed)) = result.lines { value["added"] = json!(added); value["removed"] = json!(removed); }
    }
    value
}

/// Codex reports each command and file change when it starts and ends.
pub(super) fn codex(tool: &elsecrew_core::provider::vendor_observation::codex_events::stream::ToolEvent, cwd: &Path) -> Vec<Value> {
    use elsecrew_core::provider::vendor_observation::codex_events::stream::{ToolKind, ToolPhase};
    let outcome = match tool.phase {
        ToolPhase::Started | ToolPhase::Completed => None,
        ToolPhase::Failed => Some("failed"),
        ToolPhase::Declined => Some("refused"),
        ToolPhase::Output => return Vec::new(),
    };
    let done = tool.phase != ToolPhase::Started;
    let detail = &tool.detail;
    let finish = |mut value: Value, id: &str| {
        if let Some(outcome) = outcome { value["outcome"] = json!(outcome); }
        if done { value["phase"] = json!("done"); }
        with_id(value, Some(id))
    };
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
                    // Without a place (a bare `ls`), the command line stays the target.
                    if let Some(target) = target { value["target"] = json!(target); }
                }
            }
            if done {
                if let Some(exit) = detail.exit_code { value["exit"] = json!(exit); }
                if let Some(ms) = detail.duration_ms { value["ms"] = json!(ms); }
                if let Some(output) = detail.output.as_deref().and_then(|output| block(output, BLOCK_MAX_CHARS)) { value["output"] = json!(output); }
            }
            vec![finish(value, &tool.item_id)]
        }
        ToolKind::FileChange => {
            if detail.changes.is_empty() { return vec![finish(row("edit", None, None, None), &tool.item_id)]; }
            detail.changes.iter().enumerate().map(|(index, change)| {
                let action = match change.kind.as_str() { "add" => "write", "delete" => "delete", _ => "edit" };
                let mut value = row(action, display_path(&change.path, cwd), None, None);
                if done && outcome.is_none() { value["added"] = json!(change.added); value["removed"] = json!(change.removed); }
                finish(value, &format!("{}:{index}", tool.item_id))
            }).collect()
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
    fn paths_stay_inside_the_workspace_or_read_from_home() {
        let cwd = Path::new("/work/copy");
        assert_eq!(relative("/work/copy/src/app.ts", cwd).as_deref(), Some("src/app.ts"));
        assert_eq!(relative("src/app.ts", cwd).as_deref(), Some("src/app.ts"));
        assert_eq!(relative("/work/copy", cwd).as_deref(), Some("."));
        assert_eq!(relative("/elsewhere/secret.txt", cwd), None);
        assert_eq!(relative("/work/copy/../secret.txt", cwd), None);
        assert_eq!(relative("../secret.txt", cwd), None);
        assert_eq!(display_path("/elsewhere/notes.txt", cwd).as_deref(), Some("/elsewhere/notes.txt"));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from).filter(|home| home.is_absolute() && home != Path::new("/")) {
            let inside = home.join(".ssh/config");
            assert_eq!(display_path(inside.to_str().unwrap(), cwd).as_deref(), Some("~/.ssh/config"));
        }
    }

    #[test]
    fn lines_are_one_bounded_line_with_marks_neutralised() {
        assert_eq!(bounded("Run tests\nrm -rf /").as_deref(), Some("Run tests"));
        assert_eq!(bounded("  ").as_deref(), None);
        assert_eq!(bounded("file\u{202e}txt.exe").as_deref(), Some("file\u{fffd}txt.exe"));
        assert_eq!(bounded("a\u{1b}[31mb").as_deref(), Some("a\u{fffd}[31mb"));
        assert_eq!(bounded(&"a".repeat(400)).map(|text| text.chars().count()), Some(TARGET_MAX_CHARS + 1));
        assert_eq!(block(&format!("{}\n", "x".repeat(9000)), BLOCK_MAX_CHARS).map(|text| text.chars().count()), Some(BLOCK_MAX_CHARS + 1));
        assert_eq!(block("one\r\ntwo\u{7}", 100).as_deref(), Some("one\ntwo\u{fffd}"));
    }

    #[test]
    fn secrets_are_masked_like_the_screen_does() {
        let cases: Vec<Value> = serde_json::from_str(include_str!("../../../../../tests/fixtures/m5/secret-mask-cases.json")).unwrap();
        // Inputs that look like secrets are stored as hex so secret scanners do not flag the fixture.
        let hex = |text: &str| String::from_utf8((0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()).unwrap();
        for case in cases {
            let text = case["text"].as_str().map(String::from).unwrap_or_else(|| hex(case["textHex"].as_str().unwrap()));
            assert_eq!(mask_secrets(&text), case["masked"].as_str().unwrap(), "{text}");
        }
    }

    #[test]
    fn searches_show_what_was_looked_for_and_where() {
        let cwd = Path::new("/work/copy");
        assert_eq!(search_target(Some("**/*.md"), Some("/work/copy"), cwd).as_deref(), Some("**/*.md"));
        assert_eq!(search_target(Some("TODO"), Some("/work/copy/src"), cwd).as_deref(), Some("TODO · src"));
        assert_eq!(search_target(Some("a\nrm -rf /"), Some("/work/copy"), cwd).as_deref(), Some("a"));
        assert_eq!(search_target(None, Some("/work/copy/docs"), cwd).as_deref(), Some("docs"));
        assert_eq!(domain("https://user:pw@docs.example.com/a?b").as_deref(), Some("docs.example.com"));
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
        let started = |command: &str| codex(&event(ToolKind::Command, ToolPhase::Started, Some(command), ToolDetail { cwd: Some("/work/copy".into()), ..Default::default() }), cwd);
        assert_eq!(started(r#"/bin/zsh -lc "npm test""#), vec![json!({"action":"command","target":"npm test","command":"npm test","cwd":".","id":"i"})]);
        assert_eq!(started("/bin/bash -lc 'node tests/check.js'")[0]["target"], "node tests/check.js");
        assert_eq!(started("echo one\nrm -rf /")[0]["target"], "echo one");
        assert_eq!(started("echo one\nrm -rf /")[0]["command"], "echo one\nrm -rf /");
        assert_eq!(started("GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz gh pr list")[0]["target"], "GITHUB_TOKEN=•••• gh pr list");
        assert_eq!(codex(&event(ToolKind::Command, ToolPhase::Output, None, ToolDetail::default()), cwd), Vec::<Value>::new());
        assert_eq!(codex(&event(ToolKind::Command, ToolPhase::Declined, None, ToolDetail::default()), cwd), vec![json!({"action":"command","outcome":"refused","phase":"done","id":"i"})]);
        let done = codex(&event(ToolKind::Command, ToolPhase::Completed, Some("/bin/zsh -lc 'cat .env'"), ToolDetail {
            cwd: Some("/work/copy/web".into()), exit_code: Some(0), duration_ms: Some(1200), output: Some("API_KEY=abc123\nPORT=3000".into()), ..Default::default()
        }), cwd);
        assert_eq!(done, vec![json!({"action":"command","target":"cat .env","command":"cat .env","cwd":"web","exit":0,"ms":1200,"output":"API_KEY=••••\nPORT=3000","phase":"done","id":"i"})]);
        let failed = codex(&event(ToolKind::Command, ToolPhase::Failed, Some("false"), ToolDetail { exit_code: Some(1), ..Default::default() }), cwd);
        assert_eq!(failed[0]["outcome"], "failed");
        assert_eq!(failed[0]["exit"], 1);
        let read = codex(&event(ToolKind::Command, ToolPhase::Started, Some("/bin/zsh -lc 'sed -n 1,80p src/app.js'"), ToolDetail {
            cwd: Some("/work/copy".into()), actions: vec![CommandAction { kind: "read".into(), path: Some("/work/copy/src/app.js".into()), query: None }], ..Default::default()
        }), cwd);
        assert_eq!(read, vec![json!({"action":"read","target":"src/app.js","command":"sed -n 1,80p src/app.js","cwd":".","id":"i"})]);
        let search = codex(&event(ToolKind::Command, ToolPhase::Started, Some("rg TODO src"), ToolDetail {
            actions: vec![CommandAction { kind: "search".into(), path: Some("src".into()), query: Some("TODO".into()) }], ..Default::default()
        }), cwd);
        assert_eq!(search[0]["target"], "TODO · src");
        assert_eq!(search[0]["tool"], "Grep");
    }

    #[test]
    fn codex_file_changes_name_each_file_and_its_lines() {
        use elsecrew_core::provider::vendor_observation::codex_events::stream::{FileChange, ToolDetail, ToolKind, ToolPhase};
        let cwd = Path::new("/work/copy");
        let detail = ToolDetail { changes: vec![
            FileChange { path: "/work/copy/src/app.ts".into(), kind: "update".into(), added: 3, removed: 1 },
            FileChange { path: "/work/copy/new.md".into(), kind: "add".into(), added: 10, removed: 0 },
            FileChange { path: "/work/copy/old.md".into(), kind: "delete".into(), added: 0, removed: 4 },
        ], ..Default::default() };
        assert_eq!(codex(&event(ToolKind::FileChange, ToolPhase::Started, None, detail.clone()), cwd), vec![
            json!({"action":"edit","target":"src/app.ts","id":"i:0"}),
            json!({"action":"write","target":"new.md","id":"i:1"}),
            json!({"action":"delete","target":"old.md","id":"i:2"}),
        ]);
        let done = codex(&event(ToolKind::FileChange, ToolPhase::Completed, None, detail), cwd);
        assert_eq!(done[0], json!({"action":"edit","target":"src/app.ts","added":3,"removed":1,"phase":"done","id":"i:0"}));
        assert_eq!(codex(&event(ToolKind::FileChange, ToolPhase::Started, None, ToolDetail::default()), cwd), vec![json!({"action":"edit","id":"i"})]);
    }

    #[test]
    fn claude_done_rows_keep_the_start_and_add_how_it_ended() {
        use elsecrew_core::provider::vendor_observation::claude_events::ToolResult;
        let started = json!({"action":"command","target":"npm test","command":"npm test","id":"t"});
        let result = ToolResult { tool_use_id: "t".into(), is_error: true, exit_code: Some(2), output: Some("FAIL token=abc".into()), lines: None };
        assert_eq!(claude_done(&started, &result, std::time::Duration::from_millis(1500)),
            json!({"action":"command","target":"npm test","command":"npm test","id":"t","phase":"done","ms":1500,"outcome":"failed","exit":2,"output":"FAIL token=••••"}));
        let ok = ToolResult { tool_use_id: "t".into(), output: Some("ok".into()), ..Default::default() };
        assert_eq!(claude_done(&started, &ok, std::time::Duration::from_millis(5))["exit"], 0);
        let edit = json!({"action":"edit","target":"src/a.ts","id":"e"});
        let lines = ToolResult { tool_use_id: "e".into(), lines: Some((4, 2)), output: Some("file contents".into()), ..Default::default() };
        let done = claude_done(&edit, &lines, std::time::Duration::from_millis(5));
        assert_eq!((done["added"].clone(), done["removed"].clone(), done.get("output")), (json!(4), json!(2), None), "file contents are never stored");
    }

    /// Development replay, not a gate: recorded provider frames (Claude
    /// stream-json, Codex app-server JSON-RPC) through the real parsers and
    /// row builders, written as JSON lines for a screen capture.
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
        let mut started = std::collections::BTreeMap::new();
        let mut projection: Option<SessionProjection> = None;
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
                    let row = claude_row(name, &target, &input.to_string(), block["id"].as_str(), Path::new(parts[2]), false);
                    started.insert(block["id"].as_str().unwrap().to_string(), (row.clone(), frame["timestamp"].as_str().map(String::from)));
                    claude_rows.push(row);
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
                            claude_rows.push(claude_done(&row, &result, elapsed));
                        }
                    }
                }
            }
        }
        // Codex: the turn's notifications through the turn stream.
        let mut codex_rows = Vec::new();
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
                Ok(Observed::Tool(tool)) => codex_rows.extend(codex(&tool, Path::new(parts[3]))),
                Ok(_) => {}
                Err(_) => panic!("codex replay stopped at {method}: {}", stream.diagnostic_stage()),
            }
        }
        std::fs::write(parts[4], serde_json::to_string_pretty(&json!({"claude":claude_rows,"codex":codex_rows})).unwrap()).unwrap();
    }
}
