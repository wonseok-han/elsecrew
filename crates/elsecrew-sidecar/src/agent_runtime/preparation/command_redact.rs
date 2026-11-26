//! A command line as the work history may store it, fail-closed (review PR #11
//! B2, second round): instead of looking for secrets, it keeps only what is
//! known to be harmless and hides every other argument.
//!
//! Kept: the program name, the subcommands of a few multi-command tools
//! (`git status`, `npm run build`), short flag names, paths inside the
//! workspace, a short allowlist of plain words, small numbers, `$NAME`
//! references and the shell operators between commands. A URL keeps only its
//! scheme and host. Everything else becomes ••••: quoted strings, `=` values,
//! environment assignments, the value after a header/auth/password flag,
//! attached short-flag values, long or unknown tokens.
//!
//! When the line cannot be read safely (an unmatched quote, a backslash,
//! backticks, `$(`/`${`, a heredoc, parentheses or braces outside quotes,
//! several lines, or very long) only the program name is kept and the row
//! says the arguments are hidden.
//!
//! The screen runs the same algorithm (`redactCommand`, src/m5/secret-shape.ts)
//! on every stored command, and both run tests/fixtures/m5/secret-mask-cases.json.
use super::secret_mask::MASK;

pub(super) struct Redacted {
    pub(super) text: String,
    /// The line could not be read safely; only the program name is kept.
    pub(super) args_hidden: bool,
}

const LONGEST: usize = 2000;
const WITH_SUBCOMMANDS: &[&str] = &["git", "npm", "pnpm", "yarn", "npx", "gh", "cargo", "docker", "kubectl", "go", "pip", "pip3", "brew", "bun", "deno", "make", "rustup", "uv", "poetry", "pnpx"];
const PLAIN_WORDS: &[&str] = &["origin", "main", "master", "HEAD", "dev", "develop", "all", "true", "false", "yes", "no", "none", "on", "off", "install", "test", "build", "run", "start", "status", "diff", "log", "show", "add", "list", "help", "version", "/dev/null"];
/// Flags whose next argument is a credential or request content.
const VALUE_FLAGS: &[&str] = &["-H", "-u", "-U", "-p", "-b", "-d", "-F", "-E", "--header", "--proxy-header", "--user", "--proxy-user", "--password", "--pass", "--passwd", "--passphrase", "--token", "--key", "--secret", "--auth", "--oauth2-bearer", "--bearer", "--cookie", "--data", "--data-raw", "--data-binary", "--data-urlencode", "--form", "--api-key", "--access-token", "--cert", "--private-key", "--client-secret"];

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
            // A file descriptor right before the operator belongs to it (`2>`).
            let mut op = if !quoted && !word.is_empty() && word.chars().all(|d| d.is_ascii_digit()) { std::mem::take(&mut word) } else { flush(&mut tokens, &mut word, &mut quoted); String::new() };
            while index < chars.len() && operator(chars[index]) { op.push(chars[index]); index += 1; }
            // `>&1`, `2>&1`: the descriptor it points at.
            if op.ends_with('&') && op.contains('>') { while index < chars.len() && chars[index].is_ascii_digit() { op.push(chars[index]); index += 1; } }
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

fn ascii_all(text: &str, ok: impl Fn(char) -> bool) -> bool { !text.is_empty() && text.chars().all(ok) }
fn name_char(c: char) -> bool { c.is_ascii_alphanumeric() || c == '_' }
fn path_char(c: char) -> bool { c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | '+') }

/// `NAME=` at the start of a word (an environment assignment).
fn assignment(word: &str) -> Option<&str> {
    let (name, _) = word.split_once('=')?;
    (name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && ascii_all(name, name_char)).then_some(name)
}

/// A program name: a plain word or a plain path to one.
fn program(word: &str) -> bool {
    word.chars().count() <= 60 && !word.starts_with('-') && !word.ends_with('/')
        && word.split('/').enumerate().all(|(index, part)| (index == 0 && part.is_empty() && word.starts_with('/')) || ascii_all(part, |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+')))
}

/// A segment that could be a key rather than a name: long and mixing cases and digits.
fn keylike(part: &str) -> bool {
    part.chars().count() >= 20 && part.chars().any(|c| c.is_ascii_uppercase()) && part.chars().any(|c| c.is_ascii_lowercase()) && part.chars().any(|c| c.is_ascii_digit())
}

/// A path inside the workspace: relative, plain parts, no `..`, and either
/// a folder separator or a file extension.
fn workspace_path(word: &str) -> bool {
    if word == "." { return true; }
    let body = word.strip_prefix("./").unwrap_or(word);
    let body = body.strip_suffix('/').unwrap_or(body);
    if word.chars().count() > 120 || body.is_empty() || word.starts_with('/') || word.starts_with('~') || word.starts_with('-') { return false; }
    let parts: Vec<&str> = body.split('/').collect();
    let extension = parts.last().and_then(|last| last.rsplit_once('.')).is_some_and(|(stem, ext)| !stem.is_empty() && (1..=8).contains(&ext.len()) && ascii_all(ext, |c| c.is_ascii_alphanumeric()));
    (parts.len() > 1 || extension || word.ends_with('/'))
        && parts.iter().all(|part| ascii_all(part, path_char) && *part != ".." && part.chars().count() <= 64 && !keylike(part))
}

/// Scheme and host only (`https://api.example.com/••••`).
fn url(word: &str) -> Option<String> {
    let (scheme, rest) = word.split_once("://")?;
    if !ascii_all(scheme, |c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) { return None; }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].rsplit('@').next().unwrap_or("");
    if !ascii_all(host, |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) { return None; }
    Some(format!("{scheme}://{host}{}", if rest.len() > host.len() { format!("/{MASK}") } else { String::new() }))
}

fn short_flag(word: &str) -> bool {
    word.len() >= 2 && word.len() <= 5 && word.starts_with('-') && ascii_all(&word[1..], |c| c.is_ascii_alphabetic())
}

fn long_flag(name: &str) -> bool {
    name.len() > 2 && name.len() <= 32 && name.starts_with("--")
        && ascii_all(&name[2..], |c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !name[2..].starts_with('-')
}

fn value_flag(flag: &str) -> bool {
    VALUE_FLAGS.contains(&flag) || super::secret_mask::secret_name(flag)
}

/// The program name of an unreadable line, if it has a plain one.
fn first_program(line: &str) -> Option<String> {
    line.split(|c: char| white(c) || operator(c) || c == '\n' || c == '\r').filter(|word| !word.is_empty())
        .find(|word| assignment(word).is_none())
        .filter(|word| program(word)).map(String::from)
}

pub(super) fn redact_command(line: &str) -> Redacted {
    let line = line.trim_matches(|c: char| white(c) || c == '\n' || c == '\r');
    let hidden = || Redacted { text: first_program(line).unwrap_or_default(), args_hidden: true };
    if line.is_empty() { return Redacted { text: String::new(), args_hidden: false }; }
    if line.chars().count() > LONGEST || line.contains(['\n', '\r']) { return hidden(); }
    let Some(tokens) = lex(line) else { return hidden() };
    let mut out: Vec<String> = Vec::new();
    let mut expect_program = true;
    let mut redirect = false;
    let mut secret_next = false;
    let mut subcommands = 0;
    for token in tokens {
        let word = token.text.as_str();
        if token.op {
            out.push(word.into());
            if matches!(word, "|" | "||" | "&&" | ";" | "&" | "|&" | ";;") { expect_program = true; secret_next = false; subcommands = 0; }
            else if word.contains(['<', '>']) && !word.ends_with(|c: char| c.is_ascii_digit()) { redirect = true; }
            continue;
        }
        let (was_secret, was_redirect, was_program) = (secret_next, redirect, expect_program);
        let shown = if token.quoted {
            // Quoted text is never read: a flag or name before `=` may stay.
            let before = word.split_once('=').map(|(name, _)| name).filter(|name| !name.contains(['\'', '"']));
            match before {
                Some(name) if expect_program && assignment(word).is_some() => format!("{name}={MASK}"),
                Some(name) if !expect_program && long_flag(name) => format!("{name}={MASK}"),
                _ => { expect_program = false; MASK.into() }
            }
        } else if redirect {
            if workspace_path(word) || PLAIN_WORDS.contains(&word) { word.into() } else { MASK.into() }
        } else if secret_next {
            MASK.into()
        } else if expect_program {
            if let Some(name) = assignment(word) { format!("{name}={MASK}") }
            else if program(word) {
                subcommands = if WITH_SUBCOMMANDS.contains(&word.rsplit('/').next().unwrap_or(word)) { 2 } else { 0 };
                expect_program = false;
                word.into()
            } else { expect_program = false; MASK.into() }
        } else if word.starts_with('-') {
            if word == "--" { word.into() }
            else if let Some((name, _)) = word.split_once('=') { if long_flag(name) { format!("{name}={MASK}") } else { MASK.into() } }
            else if long_flag(word) || short_flag(word) { word.into() }
            else if word.len() > 2 && word.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic()) { format!("{}{MASK}", &word[..2]) }
            else { MASK.into() }
        } else if let Some(shown) = url(word) {
            shown
        } else if subcommands > 0 && ascii_all(word, |c| c.is_ascii_lowercase() || matches!(c, ':' | '-')) && word.len() <= 24 {
            subcommands -= 1;
            word.into()
        } else if workspace_path(word) || PLAIN_WORDS.contains(&word)
            || (word.len() <= 4 && ascii_all(word, |c| c.is_ascii_digit()))
            || (word.starts_with('$') && word.len() <= 40 && word[1..].chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && ascii_all(&word[1..], name_char)) {
            word.into()
        } else {
            MASK.into()
        };
        // A value flag (shown as itself) guards exactly the one word after it.
        secret_next = !token.quoted && !was_secret && !was_redirect && !was_program
            && (long_flag(word) || short_flag(word)) && value_flag(word);
        redirect = false;
        out.push(shown);
    }
    Redacted { text: out.join(" "), args_hidden: false }
}
