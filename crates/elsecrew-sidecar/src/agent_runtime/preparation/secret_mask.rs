//! Hiding secrets in work history text before it is stored (2026-11-26).
//! The same rules run in the screen (`maskSecrets`, src/m5/secret-shape.ts);
//! both run the cases in tests/fixtures/m5/secret-mask-cases.json and must
//! give the same result. A new rule goes into both and gets a case there.
//!
//! Word by word (ASCII white space only, so both languages split alike):
//! - a known key shape keeps its prefix and hides the rest; a long mixed
//!   letter-and-digit run (32+) with no name is hidden whole;
//! - the value given to a secret-named setting (`GITHUB_TOKEN=…`,
//!   `--password …`, `"apiKey": "…"`, `?access_token=…`, `&sig=…`), also
//!   after `;`, `&`, `|`, `,` or `?` inside one word;
//! - an `Authorization` header to the end of its line, a `Bearer`/`Basic`
//!   token, a password in a URL (a long user name alone too);
//! - `-u`/`--user user:password` (the password), MySQL's `-p<password>`;
//! - a quoted value to its closing quote, past escaped quotes and line breaks;
//! - a private key block whole.
pub(super) const MASK: &str = "••••";

fn white(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0b}' | '\u{0c}')
}

pub(super) fn secret_name(name: &str) -> bool {
    const WORDS: &[&str] = &["token", "secret", "password", "passwd", "pwd", "pass", "passphrase", "apikey", "auth", "credential", "credentials", "cookie", "privatekey", "accesskey", "authorization", "sig", "signature"];
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

/// `%5F` → `_` (ASCII only), so an encoded name still reads as a name.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < text.len() {
        if bytes[index] == b'%' && index + 3 <= text.len() && bytes[index + 1].is_ascii_hexdigit() && bytes[index + 2].is_ascii_hexdigit() {
            let code = u8::from_str_radix(&text[index + 1..index + 3], 16).unwrap_or(0xff);
            if code < 128 { out.push(code as char); index += 3; continue; }
        }
        let c = text[index..].chars().next().unwrap();
        out.push(c);
        index += c.len_utf8();
    }
    out
}

/// The name part of `name=`/`name:`: percent-decoded, trailing quotes off,
/// and only the identifier at its end, so marks or words stuck in front
/// (`--`, `{"`, `$`, U+FFFD, `export\u{2028}`) do not hide it.
fn name_of(raw: &str) -> String {
    let decoded = percent_decode(raw.trim_end_matches(['"', '\'']));
    let start = decoded.char_indices().rev()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .last().map_or(decoded.len(), |(index, _)| index);
    decoded[start..].trim_start_matches(|c: char| !(c.is_ascii_alphabetic() || c == '_')).to_string()
}

fn mask_shapes(word: &str) -> String {
    struct Shape { prefix: &'static str, min: usize, chars: fn(char) -> bool, exact: bool }
    fn token(c: char) -> bool { c.is_ascii_alphanumeric() || c == '_' || c == '-' }
    fn alnum(c: char) -> bool { c.is_ascii_alphanumeric() }
    fn slack(c: char) -> bool { c.is_ascii_alphanumeric() || c == '-' }
    fn upper(c: char) -> bool { c.is_ascii_digit() || c.is_ascii_uppercase() }
    let shapes = [
        Shape { prefix: "github_pat_", min: 20, chars: |c| c.is_ascii_alphanumeric() || c == '_', exact: false },
        Shape { prefix: "sk-", min: 16, chars: token, exact: false },
        Shape { prefix: "ghp_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "gho_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "ghu_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "ghs_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "ghr_", min: 20, chars: alnum, exact: false },
        Shape { prefix: "xoxa-", min: 10, chars: slack, exact: false },
        Shape { prefix: "xoxb-", min: 10, chars: slack, exact: false },
        Shape { prefix: "xoxp-", min: 10, chars: slack, exact: false },
        Shape { prefix: "xoxo-", min: 10, chars: slack, exact: false },
        Shape { prefix: "xoxs-", min: 10, chars: slack, exact: false },
        Shape { prefix: "xoxr-", min: 10, chars: slack, exact: false },
        Shape { prefix: "glpat-", min: 20, chars: token, exact: false },
        Shape { prefix: "AIza", min: 30, chars: token, exact: false },
        Shape { prefix: "AKIA", min: 16, chars: upper, exact: true },
        Shape { prefix: "ASIA", min: 16, chars: upper, exact: true },
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

/// A long run of letters and digits with no name (a token or key pasted
/// bare): 32 or more, with upper case, lower case and digits.
fn mask_blobs(word: &str) -> String {
    fn blob(c: char) -> bool { c.is_ascii_alphanumeric() || matches!(c, '+' | '=' | '_' | '-') }
    let mut out = String::new();
    let mut rest = word;
    while let Some(start) = rest.find(blob) {
        out.push_str(&rest[..start]);
        let len = rest[start..].find(|c: char| !blob(c)).unwrap_or(rest.len() - start);
        let run = &rest[start..start + len];
        let mixed = run.chars().any(|c| c.is_ascii_uppercase()) && run.chars().any(|c| c.is_ascii_lowercase()) && run.chars().any(|c| c.is_ascii_digit());
        out.push_str(if run.len() >= 32 && mixed { MASK } else { run });
        rest = &rest[start + len..];
    }
    out.push_str(rest);
    out
}

/// A password in a URL (`scheme://user:••••@host`), or a long user name
/// standing alone (a token used as the user).
fn mask_url(word: &str) -> String {
    let Some(scheme) = word.find("://") else { return word.into() };
    let start = scheme + 3;
    let Some(at) = word[start..].find('@').map(|i| i + start) else { return word.into() };
    if word[start..].find('/').map(|i| i + start).is_some_and(|slash| slash < at) { return word.into(); }
    let user = &word[start..at];
    match user.find(':') {
        Some(colon) if colon + 1 < user.len() => format!("{}{MASK}{}", &word[..=start + colon], &word[at..]),
        None if user.encode_utf16().count() >= 16 => format!("{}{MASK}{}", &word[..start], &word[at..]),
        _ => word.into(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Expect { Value, Long, UserPass }
#[derive(Default)]
struct MaskState { expect: Option<Expect>, quote: Option<char>, line: bool, mysql: bool }

/// The closing `quote` in `text` that is not escaped with a backslash.
fn close_quote(text: &str, quote: char) -> Option<usize> {
    let mut previous = None;
    for (index, c) in text.char_indices() {
        if c == quote && previous != Some('\\') { return Some(index); }
        previous = Some(c);
    }
    None
}

fn mask_value(value: &str, state: &mut MaskState) -> String {
    match value.chars().next() {
        Some(quote @ ('"' | '\'')) => match close_quote(&value[1..], quote) {
            None => { state.quote = Some(quote); format!("{quote}{MASK}") }
            Some(close) => format!("{quote}{MASK}{}", &value[1 + close..]),
        },
        _ => {
            let body = value.trim_end_matches([')', '}', ']']);
            format!("{MASK}{}", &value[body.len()..])
        }
    }
}

/// `user:password` → `user:••••`.
fn mask_userpass(value: &str) -> String {
    match value.find(':') {
        Some(colon) if colon + 1 < value.len() => {
            let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''));
            let tail = quote.filter(|q| value.len() > 1 && value.ends_with(*q)).map(String::from).unwrap_or_default();
            format!("{}{MASK}{tail}", &value[..=colon])
        }
        _ => value.into(),
    }
}

fn long_like(word: &str) -> bool {
    let length = word.encode_utf16().count();
    length >= 16 || (length >= 8 && word.chars().any(|c| c.is_ascii_digit()))
}

fn mask_segment(segment: &str, state: &mut MaskState) -> String {
    for flag in ["--user=", "--proxy-user="] {
        if let Some(rest) = segment.strip_prefix(flag) { return format!("{flag}{}", mask_userpass(rest)); }
    }
    if let Some(equals) = segment.find('=').filter(|&i| i > 0) {
        let name = name_of(&segment[..equals]);
        if identifier(&name) && secret_name(&name) {
            let value = &segment[equals + 1..];
            if value.is_empty() { state.expect = Some(Expect::Value); return segment.into(); }
            return format!("{}{}", &segment[..=equals], mask_value(value, state));
        }
    }
    if let Some(colon) = segment.find(':').filter(|&i| i > 0 && !segment[i..].starts_with("://")) {
        let name = name_of(&segment[..colon]);
        if identifier(&name) && secret_name(&name) {
            let value = &segment[colon + 1..];
            if value.is_empty() { state.expect = Some(Expect::Value); return segment.into(); }
            return format!("{}{}", &segment[..=colon], mask_value(value, state));
        }
    }
    if matches!(segment, "-u" | "-U" | "--user" | "--proxy-user") { state.expect = Some(Expect::UserPass); return segment.into(); }
    if segment.len() > 2 && segment.starts_with("-u") && !segment.starts_with("--") && segment.contains(':') {
        return format!("-u{}", mask_userpass(&segment[2..]));
    }
    if state.mysql && segment.len() > 2 && segment.starts_with("-p") && !segment.starts_with("--") {
        return format!("-p{MASK}");
    }
    let flag = segment.trim_start_matches('-');
    if segment.starts_with('-') && segment.len() - flag.len() <= 2 && flag.starts_with(|c: char| c.is_ascii_alphabetic())
        && identifier(flag) && secret_name(flag) {
        state.expect = Some(Expect::Value);
        return segment.into();
    }
    if segment.eq_ignore_ascii_case("bearer") || segment.eq_ignore_ascii_case("basic") {
        state.expect = Some(Expect::Long);
        return segment.into();
    }
    mask_blobs(&mask_shapes(segment))
}

fn mask_word(word: &str, state: &mut MaskState) -> String {
    let core = word.trim_start_matches(['{', '(', '[', '"', '\'']);
    let opening = &word[..word.len() - core.len()];
    let lower = core.to_ascii_lowercase();
    // Authorization: everything after it on the line (up to a closing quote).
    if lower.starts_with("authorization:") || lower.starts_with("authorization=") {
        let head = &core[.."authorization:".len()];
        let rest = &core["authorization:".len()..];
        state.line = true;
        if rest.is_empty() { return word.into(); }
        let quote = rest.chars().last().filter(|c| matches!(c, '"' | '\''));
        if quote.is_some() { state.line = false; }
        return format!("{opening}{head}{MASK}{}", quote.map(String::from).unwrap_or_default());
    }
    let word = mask_url(word);
    let mut out = String::new();
    let mut rest = word.as_str();
    loop {
        if let Some(quote) = state.quote {
            match close_quote(rest, quote) {
                None => { out.push_str(MASK); return out; }
                Some(close) => { out.push_str(MASK); out.push(quote); rest = &rest[close + 1..]; state.quote = None; continue; }
            }
        }
        if rest.is_empty() { return out; }
        let split = rest.find([';', '&', '|', ',', '?']).unwrap_or(rest.len());
        let segment = &rest[..split];
        if !segment.is_empty() { out.push_str(&mask_segment(segment, state)); }
        if split == rest.len() { return out; }
        out.push_str(&rest[split..split + 1]);
        rest = &rest[split + 1..];
    }
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
    let lower = text.to_ascii_lowercase();
    let mut state = MaskState { mysql: lower.contains("mysql") || lower.contains("mariadb"), ..Default::default() };
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(first) = rest.chars().next() {
        let space = white(first);
        let len = rest.find(|c: char| white(c) != space).unwrap_or(rest.len());
        let piece = &rest[..len];
        rest = &rest[len..];
        if space {
            // A line break ends a header or an expected value; a quoted value goes on.
            if piece.contains('\n') { state.expect = None; state.line = false; }
            out.push_str(piece);
            continue;
        }
        if state.line && state.quote.is_none() {
            let quote = piece.chars().last().filter(|c| matches!(c, '"' | '\''));
            if quote.is_some() { state.line = false; }
            out.push_str(MASK);
            if let Some(quote) = quote { out.push(quote); }
            continue;
        }
        if state.quote.is_none() {
            if let Some(expect) = state.expect.take() {
                match expect {
                    Expect::Value => { out.push_str(&mask_value(piece, &mut state)); continue; }
                    Expect::UserPass => { out.push_str(&mask_userpass(piece)); continue; }
                    Expect::Long if long_like(piece) => { out.push_str(&mask_value(piece, &mut state)); continue; }
                    Expect::Long => {}
                }
            }
        }
        out.push_str(&mask_word(piece, &mut state));
    }
    out
}
