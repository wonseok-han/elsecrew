//! What the work history keeps of a command line (review PR #11 B2, third
//! round): no text of the line at all, only
//! 1. the program name: the first word of the line, when it matches
//!    `^[A-Za-z0-9._+-]{1,32}$` and is neither an assignment (`X=…`) nor a
//!    path (`./x`, `/bin/x` hold `/` and do not match);
//! 2. one subcommand, only from a fixed list per tool (`git diff`,
//!    `npm test`, `cargo check`, `python -m pytest`);
//! 3. how many other words the line has (`hiddenArgs`): every argument,
//!    number, path, host, flag and flag value is counted, never kept.
//!
//! A line that is several lines long, starts with an assignment, has an
//! unmatched quote, a backslash, backticks, `$(`/`${`, a heredoc or
//! parentheses/braces outside quotes, or is very long keeps nothing: the row
//! only says a command ran. File paths never come from a command line; they
//! are shown only from the structured path of a file event.
//!
//! The screen runs the same algorithm (`summarizeCommand`,
//! src/m5/secret-shape.ts) on stored rows of older builds, and both run the
//! `summary` cases of tests/fixtures/m5/secret-mask-cases.json.

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct CommandSummary {
    pub(super) program: Option<String>,
    pub(super) subcommand: Option<String>,
    pub(super) hidden: usize,
}

const LONGEST: usize = 2000;

/// The subcommands kept for each tool; anything else after the program is counted.
fn subcommands(program: &str) -> &'static [&'static str] {
    match program {
        "git" => &["status", "diff", "log", "show", "add", "commit", "branch", "checkout", "switch", "fetch", "pull", "push", "restore", "stash", "merge", "rebase", "init"],
        "npm" | "pnpm" | "yarn" | "bun" => &["run", "test", "install", "build", "ci", "start", "add", "exec", "lint"],
        "cargo" => &["build", "test", "check", "run", "fmt", "clippy"],
        "go" => &["build", "test", "run", "vet", "mod"],
        "gh" => &["pr", "issue", "repo", "run", "auth", "workflow"],
        "docker" => &["build", "run", "ps", "compose", "images", "logs"],
        _ => &[],
    }
}

/// Modules kept after `python -m`.
const PYTHON_MODULES: &[&str] = &["pytest", "unittest", "venv", "pip", "http.server", "json.tool", "mypy", "black", "ruff", "flake8"];

/// A subcommand of `program` from its fixed list, as stored.
pub(super) fn allowed_subcommand(program: &str, subcommand: &str) -> bool {
    if matches!(program, "python" | "python3") {
        return subcommand.strip_prefix("-m ").is_some_and(|module| PYTHON_MODULES.contains(&module));
    }
    subcommands(program).contains(&subcommand)
}

/// A program name as stored: the pattern, not dots only, not a flag, and not
/// shaped like a key (long, mixing upper case and digits).
pub(super) fn program_name(word: &str) -> bool {
    let length = word.chars().count();
    (1..=32).contains(&length) && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
        && !word.starts_with('-') && !word.chars().all(|c| c == '.')
        && !(length >= 12 && word.chars().any(|c| c.is_ascii_uppercase()) && word.chars().any(|c| c.is_ascii_digit()))
}

fn white(c: char) -> bool { c == ' ' || c == '\t' }
fn operator(c: char) -> bool { matches!(c, '|' | '&' | ';' | '<' | '>') }

struct Token { text: String, quoted: bool, op: bool }

/// Shell-like words, or None when the line is not safe to read.
fn lex(line: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut quote: Option<char> = None;
    let mut index = 0;
    let flush = |tokens: &mut Vec<Token>, word: &mut String, quoted: &mut bool| {
        if !word.is_empty() || *quoted { tokens.push(Token { text: std::mem::take(word), quoted: *quoted, op: false }); }
        *quoted = false;
    };
    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();
        if let Some(q) = quote {
            if c == q { quote = None; }
            else if q == '"' && (c == '\\' || c == '`' || (c == '$' && matches!(next, Some('(' | '{')))) { return None; }
            word.push(c);
            index += 1;
            continue;
        }
        if c == '\'' || c == '"' { quote = Some(c); quoted = true; word.push(c); index += 1; continue; }
        if matches!(c, '\\' | '`' | '(' | ')' | '{' | '}') { return None; }
        if c == '$' && matches!(next, Some('(' | '{')) { return None; }
        if white(c) { flush(&mut tokens, &mut word, &mut quoted); index += 1; continue; }
        if operator(c) {
            if c == '<' && next == Some('<') { return None; }
            flush(&mut tokens, &mut word, &mut quoted);
            let mut op = String::new();
            while index < chars.len() && operator(chars[index]) { op.push(chars[index]); index += 1; }
            tokens.push(Token { text: op, quoted: false, op: true });
            continue;
        }
        word.push(c);
        index += 1;
    }
    if quote.is_some() { return None; }
    flush(&mut tokens, &mut word, &mut quoted);
    Some(tokens)
}

/// `NAME=` at the start of a word (an environment assignment).
fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

pub(super) fn summarize_command(line: &str) -> CommandSummary {
    let line = line.trim_matches(|c: char| white(c) || c == '\n' || c == '\r');
    if line.is_empty() || line.chars().count() > LONGEST || line.contains(['\n', '\r']) { return CommandSummary::default(); }
    let Some(tokens) = lex(line) else { return CommandSummary::default() };
    let Some(first) = tokens.first() else { return CommandSummary::default() };
    if first.op || first.quoted || assignment(&first.text) || !program_name(&first.text) { return CommandSummary::default(); }
    let program = first.text.clone();
    let plain = |index: usize| tokens.get(index).filter(|token| !token.op && !token.quoted).map(|token| token.text.as_str());
    let subcommand = match (plain(1), plain(2)) {
        (Some("-m"), Some(module)) if allowed_subcommand(&program, &format!("-m {module}")) => Some(format!("-m {module}")),
        (Some(word), _) if allowed_subcommand(&program, word) => Some(word.to_string()),
        _ => None,
    };
    let used = 1 + subcommand.as_deref().map_or(0, |sub| sub.split(' ').count());
    CommandSummary { program: Some(program), subcommand, hidden: tokens.len() - used }
}
