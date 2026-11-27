//! A session's provider stdout line over the observation line bound
//! (2026-11-26 D2). An agent that reads a large image or file gets it back as
//! one stream-json/JSON-RPC line (a 55 KB PNG became a ~148 KB line), which
//! used to end the session. In session mode the transport reads such a line up
//! to `PROVIDER_SESSION_LINE_RAW_MAX_BYTES` and this module shortens its long
//! strings so the parsers see an ordinary bounded observation.
//!
//! Shortening is for observation and display only. Every value an approval
//! uses (what the user is shown, the approval fingerprint, the input sent back
//! to the provider) must stay the provider's original text (PR #17 review B1):
//! - Claude: approvals come only from `control_request` frames, which are never
//!   shortened (the allow reply echoes their input as `updatedInput`). A
//!   permission request over the bound (a large Write) is passed on whole, and
//!   the permission parser bounds what it keeps (`claude_approval`). Only
//!   frames the session reads for the work history are shortened: tool results
//!   (`user`) and the private blocks of an assistant frame; public text stays.
//! - Codex: an approval is completed from the command and working folder its
//!   `item/started` notification carried, and the completion is compared with
//!   them. So only fields named here as discardable are shortened, per method;
//!   every other field, and every request or response, stays exact.
//!
//! A frame that cannot be brought under the bound this way still fails, as
//! before.
use super::{unique_json, BoundedObservation, PROVIDER_OBSERVATION_LINE_MAX_BYTES};
use serde_json::{Map, Value};

/// The largest raw line a session connection buffers before shortening it.
/// Anything larger still fails. How many such lines may be held at once
/// across all sessions is the transport's budget, not this module's.
pub const PROVIDER_SESSION_LINE_RAW_MAX_BYTES: usize = 8 * 1024 * 1024;
/// What one session connection may stream in each direction in total. It
/// matches the durable session output budget (`ProviderEventBudget::DurableV1`),
/// which the session parsers still enforce on what they observe. The 1 MiB
/// connection total fits a login or preflight exchange, not an agent session
/// that reads several images. It counts bytes; it does not hold them.
pub const PROVIDER_SESSION_STREAM_MAX_BYTES: usize = 256 * 1024 * 1024;
/// Most JSON values (objects, arrays, scalars) an oversized line may contain.
/// A line of many small values costs far more to parse than one long string
/// of the same size, so it is refused before any tree is built (review I1).
pub const PROVIDER_SESSION_LINE_MAX_VALUES: usize = 64 * 1024;
/// Strings up to this size are kept whole.
const KEEP_STRING_BYTES: usize = 4 * 1024;
/// How much of a shortened string stays, so activity can still name a target.
const KEPT_PREFIX_BYTES: usize = 256;
/// Most lines whose count can be kept when the text itself is dropped.
const KEPT_LINE_COUNT_MAX: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OversizedLinePolicy {
    ClaudeStreamJson,
    CodexAppServer,
}

/// Why a line could not be shortened. Fixed names for diagnostics: never the
/// provider's text (review M2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OversizedLineRefusal {
    OverRawBound,
    TooManyValues,
    NotJson,
    ProtectedFrame,
    UnknownShape,
    StillOverBound,
    LongStringKept,
    OverStreamBound,
}

impl OversizedLineRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OverRawBound => "over the session line bound",
            Self::TooManyValues => "too many JSON values",
            Self::NotJson => "not unique-key JSON",
            Self::ProtectedFrame => "a frame that is never shortened",
            Self::UnknownShape => "no discardable field for this frame",
            Self::StillOverBound => "still over the line bound after shortening",
            Self::LongStringKept => "a long string where nothing may be shortened",
            Self::OverStreamBound => "over the streamed line bound",
        }
    }
}

use OversizedLineRefusal as Refusal;

/// `line` is one complete frame, with or without its trailing newline.
pub fn reduce_oversized_line(
    line: &[u8],
    policy: OversizedLinePolicy,
) -> Result<BoundedObservation, OversizedLineRefusal> {
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    if body.len() > PROVIDER_SESSION_LINE_RAW_MAX_BYTES || body.contains(&b'\n') {
        return Err(Refusal::OverRawBound);
    }
    if count_values(body) > PROVIDER_SESSION_LINE_MAX_VALUES {
        return Err(Refusal::TooManyValues);
    }
    // Duplicate keys are refused before anything is rewritten, as for any frame.
    let root = unique_json::parse(body).map_err(|_| Refusal::NotJson)?;
    if policy == OversizedLinePolicy::ClaudeStreamJson && is_claude_permission_request(&root) {
        // Read exactly, never shortened: what the person approves and what
        // the allow reply echoes is the original (2026-11-26 big Write).
        drop(root);
        let mut bytes = body.to_vec();
        bytes.push(b'\n');
        return BoundedObservation::new_session_control(bytes).map_err(|_| Refusal::OverRawBound);
    }
    reduce_root(root, policy, &Longs::none())
}

/// Applies `policy` to a parsed frame and bounds the result. `longs` names
/// the strings a streamed line left out (`OversizedLineStream`); none may
/// remain where the policy keeps text exactly.
fn reduce_root(mut root: Value, policy: OversizedLinePolicy, longs: &Longs) -> Result<BoundedObservation, OversizedLineRefusal> {
    let object = root.as_object_mut().ok_or(Refusal::UnknownShape)?;
    match policy {
        OversizedLinePolicy::ClaudeStreamJson => reduce_claude(object, longs)?,
        OversizedLinePolicy::CodexAppServer => reduce_codex(object, longs)?,
    }
    if longs.remains_in(&root) {
        return Err(Refusal::LongStringKept);
    }
    let mut bytes = serde_json::to_vec(&root).map_err(|_| Refusal::NotJson)?;
    if bytes.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
        return Err(Refusal::StillOverBound);
    }
    bytes.push(b'\n');
    BoundedObservation::new(bytes).map_err(|_| Refusal::StillOverBound)
}

/// A tool permission request or PreToolUse hook check: the frames whose
/// input an approval shows and echoes.
fn is_claude_permission_request(root: &Value) -> bool {
    root.get("type").and_then(Value::as_str) == Some("control_request")
        && matches!(
            root.get("request").and_then(|request| request.get("subtype")).and_then(Value::as_str),
            Some("can_use_tool" | "hook_callback")
        )
}

/// An upper bound on the JSON values in `body`, counted without parsing:
/// every `{`, `[`, `,` and `:` outside a string starts at most one value. A
/// value is also counted for the root. Strings are skipped with their escapes.
fn count_values(body: &[u8]) -> usize {
    let mut count = 1;
    let mut in_string = false;
    let mut escaped = false;
    for byte in body {
        if in_string {
            if escaped { escaped = false; } else if *byte == b'\\' { escaped = true; } else if *byte == b'"' { in_string = false; }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' | b',' | b':' => count += 1,
            _ => {}
        }
    }
    count
}

fn reduce_claude(object: &mut Map<String, Value>, longs: &Longs) -> Result<(), OversizedLineRefusal> {
    let kind = object.get("type").and_then(Value::as_str).ok_or(Refusal::UnknownShape)?;
    let subtype = object.get("subtype").and_then(Value::as_str);
    match (kind, subtype) {
        // Authority and handshake frames are read exactly or not at all.
        ("control_request" | "control_cancel_request" | "control_response" | "result", _)
        | ("system", Some("init")) => Err(Refusal::ProtectedFrame),
        ("assistant", _) => {
            for (key, value) in object.iter_mut() {
                if key != "message" {
                    shorten(value, longs);
                    continue;
                }
                let Some(message) = value.as_object_mut() else { return Err(Refusal::UnknownShape) };
                for (key, value) in message.iter_mut() {
                    if key != "content" {
                        shorten(value, longs);
                        continue;
                    }
                    let Some(blocks) = value.as_array_mut() else { return Err(Refusal::UnknownShape) };
                    for block in blocks {
                        let is_text = block.get("type").and_then(Value::as_str) == Some("text");
                        match block.as_object_mut() {
                            // Public text is kept exactly; its own bound applies later.
                            Some(fields) if is_text => {
                                for (key, value) in fields.iter_mut() {
                                    if key != "text" { shorten(value, longs); }
                                }
                            }
                            _ => shorten(block, longs),
                        }
                    }
                }
            }
            Ok(())
        }
        // Tool results (`user`) and frames the session ignores.
        _ => {
            let created = created_line_count(object, longs);
            for value in object.values_mut() { shorten(value, longs); }
            if let Some(result) = object.get_mut("tool_use_result").and_then(Value::as_object_mut) {
                match created {
                    // Only the count survives: one empty line each, no content.
                    Some(Some(lines)) => { result.insert("content".into(), Value::String("\n".repeat(lines as usize))); }
                    // Too many to keep: no count is better than a wrong one (review M1).
                    Some(None) => { result.remove("content"); }
                    None => {}
                }
            }
            Ok(())
        }
    }
}

/// The work history counts the lines of a file written whole from the tool
/// result's `content`. `Some(None)`: that content is dropped and its count
/// cannot be kept.
fn created_line_count(object: &Map<String, Value>, longs: &Longs) -> Option<Option<u64>> {
    let extra = object.get("tool_use_result")?.as_object()?;
    if extra.get("type")?.as_str()? != "create" { return None; }
    let content = extra.get("content")?.as_str()?;
    let lines = match longs.get(content) {
        Some(long) => long.lines(),
        None if content.len() <= KEEP_STRING_BYTES => return None,
        None => content.lines().count() as u64,
    };
    Some(Some(lines).filter(|lines| *lines <= KEPT_LINE_COUNT_MAX))
}

fn reduce_codex(object: &mut Map<String, Value>, longs: &Longs) -> Result<(), OversizedLineRefusal> {
    // Requests (approvals) and responses carry an id: never shortened.
    if object.contains_key("id") {
        return Err(Refusal::ProtectedFrame);
    }
    let method = object.get("method").and_then(Value::as_str).ok_or(Refusal::UnknownShape)?.to_owned();
    let params = object.get_mut("params").and_then(Value::as_object_mut).ok_or(Refusal::UnknownShape)?;
    match method.as_str() {
        // What a command printed. Its command and working folder are what an
        // approval and the completion check use, so they are never touched.
        "item/started" | "item/completed" => {
            let item = params.get_mut("item").and_then(Value::as_object_mut).ok_or(Refusal::UnknownShape)?;
            match item.get("type").and_then(Value::as_str) {
                Some("commandExecution") => {
                    if let Some(output) = item.get_mut("aggregatedOutput") { shorten_string(output, longs); }
                }
                Some("fileChange") => shorten_change_diffs(item, longs),
                _ => return Err(Refusal::UnknownShape),
            }
        }
        "turn/diff/updated" => shorten_string(params.get_mut("diff").ok_or(Refusal::UnknownShape)?, longs),
        // Tool output deltas are never shown; message and plan deltas are
        // public text and stay exact (review I4).
        "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
            shorten_string(params.get_mut("delta").ok_or(Refusal::UnknownShape)?, longs)
        }
        _ => return Err(Refusal::UnknownShape),
    }
    Ok(())
}

/// The work history counts each file change's added and removed lines from
/// its `diff`. A long diff is replaced by one that counts the same, or, when
/// that would not fit, removed so the count reads as unknown (review M1).
fn shorten_change_diffs(item: &mut Map<String, Value>, longs: &Longs) {
    let Some(changes) = item.get_mut("changes").and_then(Value::as_array_mut) else { return };
    for change in changes {
        let Some(fields) = change.as_object_mut() else { continue };
        let Some(diff) = fields.get("diff").and_then(Value::as_str) else { continue };
        let long = longs.get(diff);
        if long.is_none() && diff.len() <= KEEP_STRING_BYTES { continue; }
        let kind = fields.get("kind").and_then(|kind| kind.get("type")).and_then(Value::as_str).unwrap_or("");
        let (added, removed) = match long {
            Some(long) => long.diff_counts(kind),
            None => crate::provider::vendor_observation::codex_events::stream::diff_counts(kind, diff),
        };
        if added + removed <= KEPT_LINE_COUNT_MAX {
            // One hunk of bare `+`/`-` lines counts the same for every change kind.
            let compact = format!("@@\n{}{}", "+\n".repeat(added as usize), "-\n".repeat(removed as usize));
            fields.insert("diff".into(), Value::String(compact));
        } else {
            fields.remove("diff");
        }
    }
}

fn shorten_string(value: &mut Value, longs: &Longs) {
    if let Value::String(text) = value {
        if let Some(long) = longs.get(text) {
            *text = long.shortened();
            return;
        }
        if text.len() > KEEP_STRING_BYTES {
            let mut end = KEPT_PREFIX_BYTES;
            while !text.is_char_boundary(end) { end -= 1; }
            let omitted = text.len() - end;
            text.truncate(end);
            text.push_str(&format!("…[{omitted} bytes omitted]"));
        }
    }
}

fn shorten(value: &mut Value, longs: &Longs) {
    match value {
        Value::String(_) => shorten_string(value, longs),
        Value::Array(values) => values.iter_mut().for_each(|value| shorten(value, longs)),
        Value::Object(fields) => fields.values_mut().for_each(|value| shorten(value, longs)),
        _ => {}
    }
}

/// The most a single line may stream through `OversizedLineStream` (ADE
/// stabilization A). It is read, not held: memory stays at the skeleton.
pub const PROVIDER_SESSION_LINE_STREAM_MAX_BYTES: usize = 64 * 1024 * 1024;
/// The most a streamed line's skeleton (its structure and kept strings) may
/// hold. Past `LEAN_FROM_BYTES` only strings of up to `LEAN_KEEP_BYTES` raw
/// bytes are kept (keys, ids, names), so even `PROVIDER_SESSION_LINE_MAX_VALUES`
/// of them fit and a permission request still reaches its refusal (PR #25
/// review I4): 1 MiB + 64K × (130 + separators) < 10 MiB.
const SKELETON_MAX_BYTES: usize = 10 * 1024 * 1024;
const LEAN_FROM_BYTES: usize = 1024 * 1024;
const LEAN_KEEP_BYTES: usize = 128;
/// Bytes of a left-out string kept for its shortened text.
const LONG_PREFIX_BYTES: usize = KEPT_PREFIX_BYTES + 4;
/// How many left-out strings of one line keep their prefix; later ones are
/// shortened to their size alone. Bounds the summaries' memory (PR #25 review
/// M1): such a line cannot fit the observation bound anyway unless they are
/// dropped (a permission request).
const LONG_PREFIXES_MAX: usize = 4096;
/// Starts every placeholder; a per-line random nonce follows.
const LONG_MARK: char = '\u{F8FF}';

/// What a streamed line kept of each string it left out: enough for every
/// policy above (its shortened text, a new file's line count, a diff's
/// counts), never the text itself.
#[derive(Debug, Default)]
struct LongString {
    /// The first decoded bytes (`LONG_PREFIX_BYTES`).
    prefix: Vec<u8>,
    /// Decoded length in bytes.
    len: usize,
    newlines: u64,
    last: u8,
    /// The head of the current line (up to five bytes), for diff counts.
    head: [u8; 5],
    head_len: u8,
    /// Whether this string keeps a prefix (only the first
    /// `LONG_PREFIXES_MAX` of a line do, which bounds their memory).
    keeps_prefix: bool,
    /// Whether the text starts like a unified diff (decided on its head).
    unified: Option<bool>,
    in_hunk: bool,
    added: u64,
    removed: u64,
}

impl LongString {
    fn push(&mut self, byte: u8) {
        if self.keeps_prefix && self.prefix.len() < LONG_PREFIX_BYTES { self.prefix.push(byte); }
        self.len += 1;
        self.last = byte;
        if byte == b'\n' {
            self.newlines += 1;
            self.end_line();
        } else if self.head_len < 5 {
            self.head[usize::from(self.head_len)] = byte;
            self.head_len += 1;
            if self.unified.is_none() && self.head_len == 5 { self.decide_unified(); }
        }
    }

    fn head(&self) -> &[u8] {
        &self.head[..usize::from(self.head_len)]
    }

    fn decide_unified(&mut self) {
        if self.unified.is_some() { return; }
        let head = self.head();
        let unified = head.starts_with(b"@@") || head.starts_with(b"---") || head.starts_with(b"diff ");
        self.unified = Some(unified);
        self.in_hunk = !unified;
    }

    /// One line of `diff_counts`, judged on its head.
    fn end_line(&mut self) {
        self.decide_unified();
        let head = self.head[..usize::from(self.head_len)].to_vec();
        self.head_len = 0;
        if head.starts_with(b"diff ") { self.in_hunk = false; }
        else if head.starts_with(b"@@") { self.in_hunk = true; }
        else if self.in_hunk {
            if head.first() == Some(&b'+') { self.added += 1; } else if head.first() == Some(&b'-') { self.removed += 1; }
        }
    }

    /// The end of the text closes its last line, as `str::lines` does.
    fn finish(&mut self) {
        if self.len > 0 && self.last != b'\n' { self.end_line(); } else { self.decide_unified(); }
    }

    /// `str::lines().count()`.
    fn lines(&self) -> u64 {
        self.newlines + u64::from(self.len > 0 && self.last != b'\n')
    }

    /// `codex_events::stream::diff_counts`.
    fn diff_counts(&self, kind: &str) -> (u64, u64) {
        match kind {
            "add" if self.unified != Some(true) => (self.lines(), 0),
            "delete" if self.unified != Some(true) => (0, self.lines()),
            _ => (self.added, self.removed),
        }
    }

    /// What `shorten_string` makes of the whole text.
    fn shortened(&self) -> String {
        let mut end = KEPT_PREFIX_BYTES.min(self.prefix.len());
        let text = String::from_utf8_lossy(&self.prefix[..]).into_owned();
        while end > 0 && !text.is_char_boundary(end) { end -= 1; }
        format!("{}…[{} bytes omitted]", &text[..end], self.len - end)
    }
}

/// The strings a streamed line left out, by placeholder.
struct Longs {
    mark: String,
    strings: Vec<LongString>,
}

impl Longs {
    fn none() -> Self {
        Self { mark: String::new(), strings: Vec::new() }
    }

    fn get(&self, text: &str) -> Option<&LongString> {
        if self.mark.is_empty() { return None; }
        let index: usize = text.strip_prefix(self.mark.as_str())?.parse().ok()?;
        self.strings.get(index)
    }

    /// Whether any placeholder is still in `value` (somewhere nothing may be
    /// shortened).
    fn remains_in(&self, value: &Value) -> bool {
        if self.mark.is_empty() { return false; }
        match value {
            Value::String(text) => text.contains(self.mark.as_str()),
            Value::Array(values) => values.iter().any(|value| self.remains_in(value)),
            Value::Object(fields) => fields.iter().any(|(key, value)| key.contains(self.mark.as_str()) || self.remains_in(value)),
            _ => false,
        }
    }

    /// Replaces every placeholder with an empty string.
    fn drop_all(&self, value: &mut Value) {
        match value {
            Value::String(text) if self.get(text).is_some() => text.clear(),
            Value::Array(values) => values.iter_mut().for_each(|value| self.drop_all(value)),
            Value::Object(fields) => fields.values_mut().for_each(|value| self.drop_all(value)),
            _ => {}
        }
    }
}

/// A string being read.
struct StringScan {
    /// Raw bytes (with the opening quote) while the decoded text is short.
    raw: Vec<u8>,
    stats: LongString,
    /// An escape in progress: `\` seen, then up to four hex digits of `\u`.
    escape: Option<Vec<u8>>,
    /// A high surrogate waiting for its low half.
    high: Option<u16>,
    /// The incomplete end of a UTF-8 character in raw text.
    utf8: Vec<u8>,
}

/// A session line over `PROVIDER_SESSION_LINE_RAW_MAX_BYTES` (ADE stabilization
/// A; before, such a line ended the session). The transport feeds it in
/// pieces as it arrives and never holds it: this keeps the line's structure
/// and every string of up to `KEEP_STRING_BYTES` exactly, and of each longer
/// string only what the policies use. Then the same policy as for a line read
/// whole decides, and a left-out string anywhere the policy keeps text exactly
/// refuses the line.
///
/// A Claude permission request this large cannot be approved: what a person
/// approves and the reply echoes must be the original (PR #17 review B1), and
/// inputs over `LARGE_TOOL_INPUT_MAX_BYTES` (4 MiB) are already refused with
/// guidance. Such a request is handed on with its input withheld; it can only
/// be declined, and the session carries on. Codex requests and responses (and
/// Claude's other protected frames) still fail, as before.
pub struct OversizedLineStream {
    policy: OversizedLinePolicy,
    skeleton: Vec<u8>,
    raw: usize,
    values: usize,
    string: Option<StringScan>,
    longs: Longs,
    failed: Option<OversizedLineRefusal>,
}

impl OversizedLineStream {
    pub fn new(policy: OversizedLinePolicy) -> Self {
        use std::hash::{BuildHasher, Hasher};
        // Process-random keys (OS randomness): provider text cannot name a
        // placeholder it has not seen.
        let nonce = [0u8, 1].map(|salt| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u8(salt);
            hasher.finish()
        });
        Self {
            policy,
            skeleton: Vec::new(),
            raw: 0,
            values: 1,
            string: None,
            longs: Longs { mark: format!("{LONG_MARK}{:016x}{:016x}:", nonce[0], nonce[1]), strings: Vec::new() },
            failed: None,
        }
    }

    /// Bytes read so far.
    pub fn len(&self) -> usize {
        self.raw
    }

    pub fn is_empty(&self) -> bool {
        self.raw == 0
    }

    /// More of the line, without its newline. A refusal is remembered.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), OversizedLineRefusal> {
        if let Some(refusal) = self.failed { return Err(refusal); }
        let result = self.feed_inner(bytes);
        if let Err(refusal) = result { self.failed = Some(refusal); }
        result
    }

    fn feed_inner(&mut self, bytes: &[u8]) -> Result<(), OversizedLineRefusal> {
        self.raw = self.raw.checked_add(bytes.len()).ok_or(Refusal::OverStreamBound)?;
        if self.raw > PROVIDER_SESSION_LINE_STREAM_MAX_BYTES { return Err(Refusal::OverStreamBound); }
        let mut start = 0;
        while start < bytes.len() {
            if self.string.is_some() {
                start += self.feed_string(&bytes[start..])?;
                continue;
            }
            // Outside strings: structure, numbers, literals, copied as they are.
            let rest = &bytes[start..];
            let end = rest.iter().position(|byte| *byte == b'"').unwrap_or(rest.len());
            for byte in &rest[..end] {
                match byte {
                    b'{' | b'[' | b',' | b':' => self.values += 1,
                    b'\n' => return Err(Refusal::NotJson),
                    _ => {}
                }
            }
            if self.values > PROVIDER_SESSION_LINE_MAX_VALUES { return Err(Refusal::TooManyValues); }
            // Bounded before it grows (PR #25 review M1).
            if self.skeleton.len() + end > SKELETON_MAX_BYTES { return Err(Refusal::StillOverBound); }
            self.skeleton.extend_from_slice(&rest[..end]);
            if end < rest.len() {
                let stats = LongString { keeps_prefix: self.longs.strings.len() < LONG_PREFIXES_MAX, ..LongString::default() };
                self.string = Some(StringScan { raw: vec![b'"'], stats, escape: None, high: None, utf8: Vec::new() });
                start += end + 1;
            } else {
                start = bytes.len();
            }
            if self.skeleton.len() > SKELETON_MAX_BYTES { return Err(Refusal::StillOverBound); }
        }
        Ok(())
    }

    /// Reads string bytes; returns how many were used (up to and including
    /// the closing quote).
    fn feed_string(&mut self, bytes: &[u8]) -> Result<usize, OversizedLineRefusal> {
        let scan = self.string.as_mut().expect("string in progress");
        let mut used = 0;
        for &byte in bytes {
            used += 1;
            if scan.raw.len() <= RAW_KEEP_MAX && scan.stats.len <= KEEP_STRING_BYTES { scan.raw.push(byte); }
            if let Some(escape) = scan.escape.as_mut() {
                if escape.is_empty() && byte != b'u' {
                    let decoded = match byte {
                        b'"' => b'"', b'\\' => b'\\', b'/' => b'/', b'b' => 0x08, b'f' => 0x0c,
                        b'n' => b'\n', b'r' => b'\r', b't' => b'\t',
                        _ => return Err(Refusal::NotJson),
                    };
                    if scan.high.is_some() { return Err(Refusal::NotJson); }
                    scan.escape = None;
                    scan.stats.push(decoded);
                    continue;
                }
                escape.push(byte);
                if escape.len() < 5 { continue; }
                let hex = std::str::from_utf8(&escape[1..]).map_err(|_| Refusal::NotJson)?;
                let unit = u16::from_str_radix(hex, 16).map_err(|_| Refusal::NotJson)?;
                scan.escape = None;
                let character = match (scan.high.take(), unit) {
                    (None, 0xD800..=0xDBFF) => { scan.high = Some(unit); continue; }
                    (None, 0xDC00..=0xDFFF) => return Err(Refusal::NotJson),
                    (None, unit) => char::from_u32(u32::from(unit)),
                    (Some(high), 0xDC00..=0xDFFF) => char::from_u32(0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00)),
                    (Some(_), _) => return Err(Refusal::NotJson),
                }.ok_or(Refusal::NotJson)?;
                let mut buffer = [0u8; 4];
                for byte in character.encode_utf8(&mut buffer).bytes() { scan.stats.push(byte); }
                continue;
            }
            if scan.high.is_some() && byte != b'\\' { return Err(Refusal::NotJson); }
            match byte {
                b'\\' => {
                    if !scan.utf8.is_empty() { return Err(Refusal::NotJson); }
                    scan.escape = Some(Vec::new());
                }
                b'"' => {
                    if !scan.utf8.is_empty() || scan.high.is_some() { return Err(Refusal::NotJson); }
                    let mut scan = self.string.take().expect("string in progress");
                    let lean = self.skeleton.len() > LEAN_FROM_BYTES;
                    let kept = if lean { scan.raw.len() <= LEAN_KEEP_BYTES + 2 && scan.stats.len <= LEAN_KEEP_BYTES } else { scan.stats.len <= KEEP_STRING_BYTES };
                    let piece = if kept {
                        // Kept whole, exactly as written; parsed later with the rest.
                        std::mem::take(&mut scan.raw)
                    } else {
                        scan.stats.finish();
                        let placeholder = format!("\"{}{}\"", self.longs.mark, self.longs.strings.len());
                        self.longs.strings.push(scan.stats);
                        placeholder.into_bytes()
                    };
                    if self.skeleton.len() + piece.len() > SKELETON_MAX_BYTES { return Err(Refusal::StillOverBound); }
                    self.skeleton.extend_from_slice(&piece);
                    return Ok(used);
                }
                0x00..=0x1f => return Err(Refusal::NotJson),
                _ => {
                    // Raw text must be UTF-8; a character may span two feeds.
                    scan.utf8.push(byte);
                    match std::str::from_utf8(&scan.utf8) {
                        Ok(_) => {
                            for byte in std::mem::take(&mut scan.utf8) { scan.stats.push(byte); }
                        }
                        Err(error) if error.error_len().is_none() && scan.utf8.len() < 4 => {}
                        Err(_) => return Err(Refusal::NotJson),
                    }
                }
            }
        }
        Ok(used)
    }

    /// The whole line has been fed: the bounded observation the parsers see.
    pub fn finish(mut self) -> Result<BoundedObservation, OversizedLineRefusal> {
        if let Some(refusal) = self.failed { return Err(refusal); }
        if self.string.is_some() { return Err(Refusal::NotJson); }
        let mut root = unique_json::parse(&self.skeleton).map_err(|_| Refusal::NotJson)?;
        self.skeleton = Vec::new();
        if self.policy == OversizedLinePolicy::ClaudeStreamJson && is_claude_permission_request(&root) {
            // Never approvable at this size: hand on only what a refusal
            // needs (its id, kind, tool name and use id, and for a hook our
            // callback and the event name), marked withheld, so it can only be
            // declined. Nothing of the tool input is kept (PR #25 review I4).
            let pick = |value: &Value, keys: &[&str]| -> Value {
                let mut kept = Map::new();
                if let Some(object) = value.as_object() {
                    for key in keys {
                        if let Some(field) = object.get(*key) { kept.insert((*key).to_owned(), field.clone()); }
                    }
                }
                Value::Object(kept)
            };
            let request = root.get("request").cloned().unwrap_or(Value::Null);
            let mut kept_request = pick(&request, &["subtype", "tool_name", "tool_use_id", "callback_id", "agent_id"]);
            kept_request.as_object_mut().ok_or(Refusal::UnknownShape)?.insert("input".into(),
                pick(request.get("input").unwrap_or(&Value::Null), &["hook_event_name", "tool_name", "tool_use_id", "agent_id"]));
            root = Value::Object(Map::from_iter([
                ("type".to_owned(), root["type"].clone()),
                ("request_id".to_owned(), root.get("request_id").cloned().unwrap_or(Value::Null)),
                ("request".to_owned(), kept_request),
            ]));
            self.longs.drop_all(&mut root);
            if self.longs.remains_in(&root) { return Err(Refusal::LongStringKept); }
            let mut bytes = serde_json::to_vec(&root).map_err(|_| Refusal::NotJson)?;
            bytes.push(b'\n');
            return BoundedObservation::new_session_control_withheld(bytes).map_err(|_| Refusal::StillOverBound);
        }
        reduce_root(root, self.policy, &self.longs)
    }
}

/// Raw bytes of a string kept while it may still be short: a decoded byte
/// takes at most six raw ones (`\uXXXX`).
const RAW_KEEP_MAX: usize = 6 * KEEP_STRING_BYTES + 6;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::vendor_observation::{claude_events, claude_session};
    use serde_json::json;

    fn line(value: &Value) -> Vec<u8> {
        let mut bytes = value.to_string().into_bytes();
        bytes.push(b'\n');
        bytes
    }

    fn image_result(bytes: usize) -> Value {
        json!({"type":"user","uuid":"u1","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1",
                "content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"A".repeat(bytes)}}]}]},
            "tool_use_result":{"type":"image","file":{"base64":"A".repeat(bytes),"type":"image/png"}}})
    }

    #[test]
    fn a_large_image_tool_result_is_shortened_to_a_bounded_observation() {
        let raw = line(&image_result(148 * 1024));
        assert!(raw.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
        assert!(BoundedObservation::new(raw.clone()).is_err(), "the old bound refuses it");
        let reduced = reduce_oversized_line(&raw, OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let value: Value = reduced.json().unwrap();
        let data = value["message"]["content"][0]["content"][0]["source"]["data"].as_str().unwrap();
        assert!(data.len() < 1024 && data.ends_with("bytes omitted]"), "{data}");
        assert_eq!(value["message"]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(value["type"], "user");
    }

    #[test]
    fn a_large_new_file_keeps_its_line_count_and_an_error_keeps_its_exit_code() {
        let file = "line of the new file\n".repeat(10_000);
        let frame = json!({"type":"user","uuid":"u2","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_3","content":"done"}]},
            "tool_use_result":{"type":"create","filePath":"big.txt","content":file}});
        let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let mut projection = claude_events::SessionProjection::new("s").unwrap();
        match projection.project(&reduced).unwrap() {
            claude_events::Projection::ToolResults(results) => assert_eq!(results[0].lines, Some((10_000, 0))),
            _ => panic!("expected tool results"),
        }
        let output = format!("Exit code 3\n{}", "noise ".repeat(40_000));
        let frame = json!({"type":"user","uuid":"u3","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_4","is_error":true,"content":output}]},
            "tool_use_result":{"stdout":"","stderr":output,"interrupted":false}});
        let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        match projection.project(&reduced).unwrap() {
            claude_events::Projection::ToolResults(results) => assert_eq!(results[0].exit_code, Some(3)),
            _ => panic!("expected tool results"),
        }
    }

    #[test]
    fn assistant_public_text_is_kept_exactly_and_only_private_blocks_shrink() {
        let text = "보고 ".repeat(5000);
        let frame = json!({"type":"assistant","uuid":"a1","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m1","role":"assistant","content":[
                {"type":"text","text":text},
                {"type":"tool_use","id":"toolu_2","name":"Write","input":{"file_path":"big.txt","content":"x".repeat(200_000)}}]}});
        let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let mut projection = claude_events::SessionProjection::new("s").unwrap();
        match projection.project(&reduced).unwrap() {
            claude_events::Projection::Assistant { text: projected, has_private_blocks, .. } => {
                assert_eq!(projected, text);
                assert!(has_private_blocks);
            }
            _ => panic!("expected the assistant text"),
        }
    }

    #[test]
    fn permission_requests_handshake_and_results_are_never_shortened() {
        let big = "x".repeat(200_000);
        // A permission request over the bound is passed on whole, byte for byte.
        for subtype in ["can_use_tool", "hook_callback"] {
            let request = line(&json!({"type":"control_request","request_id":"r1","request":{"subtype":subtype,"tool_name":"Write","input":{"file_path":"a","content":big}}}));
            let observed = reduce_oversized_line(&request, OversizedLinePolicy::ClaudeStreamJson).unwrap();
            assert_eq!(observed.bytes, request, "{subtype}");
        }
        for frame in [
            json!({"type":"control_request","request_id":"r1","request":{"subtype":"interrupt","pad":big}}),
            json!({"type":"control_cancel_request","request_id":"r1","pad":big}),
            json!({"type":"control_response","response":{"pad":big}}),
            json!({"type":"result","subtype":"success","result":big}),
            json!({"type":"system","subtype":"init","pad":big}),
        ] {
            assert!(reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).is_err(), "{}", frame["type"]);
        }
    }

    #[test]
    fn duplicate_keys_bad_json_and_huge_lines_still_fail() {
        let big = "A".repeat(200_000);
        let duplicate = format!(r#"{{"type":"user","type":"user","pad":"{big}"}}"#);
        assert!(reduce_oversized_line(duplicate.as_bytes(), OversizedLinePolicy::ClaudeStreamJson).is_err());
        assert!(reduce_oversized_line(b"{not json", OversizedLinePolicy::ClaudeStreamJson).is_err());
        let huge = line(&image_result(PROVIDER_SESSION_LINE_RAW_MAX_BYTES));
        assert!(reduce_oversized_line(&huge, OversizedLinePolicy::ClaudeStreamJson).is_err());
        // Many small strings cannot be shortened under the bound: still refused.
        let many = json!({"type":"user","items":vec!["x".repeat(100); 2000]});
        assert!(reduce_oversized_line(&line(&many), OversizedLinePolicy::ClaudeStreamJson).is_err());
    }

    #[test]
    fn codex_notifications_shrink_but_requests_and_public_text_do_not() {
        let big = "B".repeat(300_000);
        let completed = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"i","type":"commandExecution","command":"cat big.log","aggregatedOutput":big,"status":"completed"}}});
        let reduced = reduce_oversized_line(&line(&completed), OversizedLinePolicy::CodexAppServer).unwrap();
        let value = reduced.json().unwrap();
        assert_eq!(value["params"]["item"]["command"], "cat big.log");
        assert!(value["params"]["item"]["aggregatedOutput"].as_str().unwrap().ends_with("bytes omitted]"));
        // A large file change keeps its added/removed line counts.
        let patch = format!("@@ -1,2 +1,3 @@\n{}{}", "+added line\n".repeat(9000), "-removed line\n".repeat(4000));
        let change = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"c","type":"fileChange","status":"completed","changes":[
                {"path":"src/big.ts","kind":{"type":"update"},"diff":patch},
                {"path":"src/new.ts","kind":{"type":"add"},"diff":"new line\n".repeat(12_000)},
                {"path":"src/small.ts","kind":{"type":"update"},"diff":"@@\n+a\n"}]}}});
        let value = reduce_oversized_line(&line(&change), OversizedLinePolicy::CodexAppServer).unwrap().json().unwrap();
        let counts = |index: usize| {
            let change = &value["params"]["item"]["changes"][index];
            crate::provider::vendor_observation::codex_events::stream::diff_counts(
                change["kind"]["type"].as_str().unwrap(), change["diff"].as_str().unwrap())
        };
        assert_eq!(counts(0), (9000, 4000));
        assert_eq!(counts(1), (12_000, 0));
        assert_eq!(value["params"]["item"]["changes"][2]["diff"], "@@\n+a\n");
        assert_eq!(value["params"]["item"]["changes"][0]["path"], "src/big.ts");
        // Too many lines to keep a count: the diff is dropped, so the count reads
        // as unknown instead of being counted from a prefix (review M1).
        let huge = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"c","type":"fileChange","status":"completed","changes":[
                {"path":"src/huge.ts","kind":{"type":"add"},"diff":"x\n".repeat(40_000)}]}}});
        let value = reduce_oversized_line(&line(&huge), OversizedLinePolicy::CodexAppServer).unwrap().json().unwrap();
        assert!(value["params"]["item"]["changes"][0].get("diff").is_none());
        assert_eq!(value["params"]["item"]["changes"][0]["path"], "src/huge.ts");
        let diff = json!({"method":"turn/diff/updated","params":{"threadId":"t","turnId":"r","diff":big}});
        assert!(reduce_oversized_line(&line(&diff), OversizedLinePolicy::CodexAppServer).is_ok());
        let approval = json!({"id":7,"method":"item/fileChange/requestApproval","params":{"pad":big}});
        assert!(reduce_oversized_line(&line(&approval), OversizedLinePolicy::CodexAppServer).is_err());
        let response = json!({"id":7,"result":{"pad":big}});
        assert!(reduce_oversized_line(&line(&response), OversizedLinePolicy::CodexAppServer).is_err());
        // An agent message whose own text is over the bound is not shortened.
        let message = json!({"method":"item/completed","params":{"item":{"id":"i","type":"agentMessage","text":big}}});
        assert_eq!(reduce_oversized_line(&line(&message), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::UnknownShape);
        // Nothing outside the named discardable fields is shortened: an item kind
        // or method without one, or a large non-discardable field, still fails.
        let unknown = json!({"method":"item/completed","params":{"item":{"id":"i","type":"webSearch","query":big}}});
        assert_eq!(reduce_oversized_line(&line(&unknown), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::UnknownShape);
        let method = json!({"method":"future/notification","params":{"pad":big}});
        assert_eq!(reduce_oversized_line(&line(&method), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::UnknownShape);
        let actions = json!({"method":"item/started","params":{"threadId":"t","turnId":"r","item":{"id":"i","type":"commandExecution",
            "command":"ls","cwd":"/w","status":"inProgress","commandActions":[{"type":"unknown","command":big}]}}});
        assert_eq!(reduce_oversized_line(&line(&actions), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::StillOverBound);
    }

    fn codex_frame(value: &Value) -> BoundedObservation {
        let raw = line(value);
        if raw.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
            reduce_oversized_line(&raw, OversizedLinePolicy::CodexAppServer).unwrap()
        } else {
            BoundedObservation::new(raw).unwrap()
        }
    }

    /// Review B1: a Codex approval is completed from the command and working
    /// folder of the item's start notification, and the completion is compared
    /// with them. Both notifications are oversized here (their output is), and
    /// the command and folder are longer than the shortening threshold: the
    /// approval and the completion must still see the original text.
    #[test]
    fn codex_approval_and_completion_use_the_original_command_after_shortening() {
        use crate::provider::vendor_observation::codex_approval::Action;
        use crate::provider::vendor_observation::codex_events::stream::{Observed, TurnOutcome, TurnStream};
        let command = format!("printf '%s' {}", "c".repeat(10_000));
        let cwd = format!("/snapshot/{}", "d".repeat(6_000));
        assert!(command.len() > KEEP_STRING_BYTES && cwd.len() > KEEP_STRING_BYTES);
        let mut stream = TurnStream::with_budget("t", "r", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        stream.observe(&codex_frame(&json!({"method":"turn/started","params":{"threadId":"t","turn":{"id":"r","status":"inProgress","items":[],"error":null}}}))).unwrap();
        let item = |status: &str, output: usize| json!({"method":if status == "inProgress" {"item/started"} else {"item/completed"},
            "params":{"threadId":"t","turnId":"r","item":{"id":"i","type":"commandExecution","command":command,"cwd":cwd,
                "status":status,"aggregatedOutput":"o".repeat(output)}}});
        let started = item("inProgress", 200_000);
        assert!(line(&started).len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
        match stream.observe(&codex_frame(&started)).unwrap() {
            Observed::Tool(tool) => assert_eq!(tool.command.as_deref(), Some(command.as_str())),
            _ => panic!("expected the command start"),
        }
        // The request names the command itself: compared with the original.
        let request = json!({"id":10,"method":"item/commandExecution/requestApproval","params":{"threadId":"t","turnId":"r",
            "itemId":"i","startedAtMs":1,"kind":"command","environmentId":null,"command":command,"cwd":cwd}});
        let approval = match stream.observe(&codex_frame(&request)).unwrap() {
            Observed::Approval(approval) => approval,
            _ => panic!("expected the approval"),
        };
        // What the user is shown and what the fingerprint binds is the original.
        match &approval.action {
            Action::Command { command: Some(shown), cwd: Some(folder), .. } => {
                assert_eq!(shown, &command);
                assert_eq!(folder, &cwd);
            }
            _ => panic!("expected a command approval"),
        }
        stream.observe(&codex_frame(&json!({"method":"serverRequest/resolved","params":{"threadId":"t","requestId":10}}))).unwrap();
        match stream.observe(&codex_frame(&item("completed", 300_000))).unwrap() {
            Observed::Tool(tool) => assert_eq!(tool.command.as_deref(), Some(command.as_str())),
            _ => panic!("expected the command end"),
        }
        stream.observe(&codex_frame(&json!({"method":"turn/completed","params":{"threadId":"t","turn":{"id":"r","status":"completed","items":[],"error":null}}}))).unwrap();
        assert_eq!(stream.finish().unwrap(), TurnOutcome::Completed);
    }

    /// The same start with a request that omits the command: the approval is
    /// filled from the original, never from a shortened copy.
    #[test]
    fn codex_approval_filled_from_the_start_keeps_the_original_command() {
        use crate::provider::vendor_observation::codex_approval::Action;
        use crate::provider::vendor_observation::codex_events::stream::{Observed, TurnStream};
        let command = format!("echo {}", "e".repeat(20_000));
        let mut stream = TurnStream::with_budget("t", "r", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        stream.observe(&codex_frame(&json!({"method":"turn/started","params":{"threadId":"t","turn":{"id":"r","status":"inProgress","items":[],"error":null}}}))).unwrap();
        stream.observe(&codex_frame(&json!({"method":"item/started","params":{"threadId":"t","turnId":"r","item":{"id":"i",
            "type":"commandExecution","command":command,"cwd":"/w","status":"inProgress","aggregatedOutput":"o".repeat(200_000)}}}))).unwrap();
        let request = json!({"id":11,"method":"item/commandExecution/requestApproval","params":{"threadId":"t","turnId":"r",
            "itemId":"i","startedAtMs":1,"kind":"command","environmentId":null}});
        let Observed::Approval(approval) = stream.observe(&codex_frame(&request)).unwrap() else { panic!("expected the approval") };
        assert!(matches!(&approval.action, Action::Command { command: Some(shown), .. } if shown == &command));
    }

    /// Review I4: a large tool output delta is shortened; a message delta, which
    /// is public text, never is.
    #[test]
    fn codex_tool_output_deltas_shrink_and_message_deltas_do_not() {
        use crate::provider::vendor_observation::codex_events::stream::{Observed, TurnStream};
        let mut stream = TurnStream::with_budget("t", "r", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        stream.observe(&codex_frame(&json!({"method":"turn/started","params":{"threadId":"t","turn":{"id":"r","status":"inProgress","items":[],"error":null}}}))).unwrap();
        stream.observe(&codex_frame(&json!({"method":"item/started","params":{"threadId":"t","turnId":"r","item":{"id":"i",
            "type":"commandExecution","command":"cat big.log","cwd":"/w","status":"inProgress"}}}))).unwrap();
        for method in ["item/commandExecution/outputDelta"] {
            let delta = json!({"method":method,"params":{"threadId":"t","turnId":"r","itemId":"i","delta":"z".repeat(400_000)}});
            assert!(matches!(stream.observe(&codex_frame(&delta)).unwrap(), Observed::Tool(_)));
        }
        let file = json!({"method":"item/fileChange/outputDelta","params":{"threadId":"t","turnId":"r","itemId":"f","delta":"z".repeat(400_000)}});
        assert!(reduce_oversized_line(&line(&file), OversizedLinePolicy::CodexAppServer).is_ok());
        let message = json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"r","itemId":"m","delta":"z".repeat(200_000)}});
        assert_eq!(reduce_oversized_line(&line(&message), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::UnknownShape);
    }

    /// Review B1, Claude side: an approval comes only from a `control_request`,
    /// which is never shortened, even after shortened frames about the same
    /// tool use. The allow reply echoes the original input exactly.
    #[test]
    fn claude_allow_echoes_the_original_input_after_shortened_frames() {
        use crate::provider::vendor_observation::claude_approval::Observation;
        let expected = crate::provider::vendor_observation::claude_init::ExpectedInit {
            cwd: test_path!("/snapshot"), model: "model", permission_mode: "default", tools: &["Write"],
        };
        let mut session = claude_session::Session::new(&expected);
        let init = json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":test_path!("/snapshot"),"model":"model","permissionMode":"default","tools":["Write"],"mcp_servers":[],"skills":[],"plugins":[]});
        session.observe(&BoundedObservation::new(line(&init)).unwrap()).unwrap();
        let content = "a line of the file being written\n".repeat(3_000);
        // The assistant frame naming the tool use is oversized and shortened.
        let assistant = json!({"type":"assistant","uuid":"a1","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m1","role":"assistant","content":[
                {"type":"tool_use","id":"toolu_w","name":"Write","input":{"file_path":"notes.md","content":content.repeat(2)}}]}});
        let reduced = reduce_oversized_line(&line(&assistant), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        session.observe(&reduced).unwrap();
        // The permission request itself fits and is read exactly.
        let request = json!({"type":"control_request","request_id":"perm_w","request":{"subtype":"can_use_tool",
            "tool_name":"Write","input":{"file_path":"notes.md","content":content},"tool_use_id":"toolu_w"}});
        let raw = line(&request);
        assert!(raw.len() < PROVIDER_OBSERVATION_LINE_MAX_BYTES && content.len() > KEEP_STRING_BYTES);
        let claude_session::Event::Permission(Observation::Requested(asked)) = session.observe(&BoundedObservation::new(raw).unwrap()).unwrap()
            else { panic!("expected the permission request") };
        let reply = session.allow_once(&asked).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["response"]["updatedInput"]["content"].as_str(), Some(content.as_str()));
        // The same request over the line bound is kept whole, never shortened,
        // and its allow reply echoes the whole original (2026-11-26 big Write).
        let _held = crate::provider::vendor_observation::claude_approval::LARGE_INPUT_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut over = request.clone();
        over["request_id"] = json!("perm_big");
        over["request"]["tool_use_id"] = json!("toolu_big");
        over["request"]["input"]["content"] = Value::String(content.repeat(2));
        let raw = line(&over);
        assert!(raw.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
        let observed = reduce_oversized_line(&raw, OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let claude_session::Event::Permission(Observation::Requested(asked)) = session.observe(&observed).unwrap()
            else { panic!("expected the large permission request") };
        assert!(!asked.withheld());
        let reply = session.allow_once(&asked).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["response"]["updatedInput"]["content"].as_str(), Some(content.repeat(2).as_str()));
        assert!(reply.as_str().len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
    }

    /// Review M1: a new file with too many lines to keep a count drops the
    /// content, so the work history shows no count rather than a wrong one.
    #[test]
    fn a_new_file_over_the_count_cap_has_no_line_count() {
        let frame = json!({"type":"user","uuid":"u9","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_9","content":"done"}]},
            "tool_use_result":{"type":"create","filePath":"huge.txt","content":"x\n".repeat(40_000)}});
        let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let mut projection = claude_events::SessionProjection::new("s").unwrap();
        match projection.project(&reduced).unwrap() {
            claude_events::Projection::ToolResults(results) => assert_eq!(results[0].lines, None),
            _ => panic!("expected tool results"),
        }
    }

    /// Review I1: shortening runs on the session's polling thread. A line near
    /// the raw bound is handled well within a poll's budget even in a debug build.
    #[test]
    fn a_line_near_the_raw_bound_is_shortened_quickly() {
        let frame = image_result(3 * 1024 * 1024 + 900 * 1024);
        let raw = line(&frame);
        assert!(raw.len() > 7 * 1024 * 1024 && raw.len() < PROVIDER_SESSION_LINE_RAW_MAX_BYTES);
        let started = std::time::Instant::now();
        let reduced = reduce_oversized_line(&raw, OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let took = started.elapsed();
        assert!(reduced.byte_len() < 4096);
        eprintln!("near-bound line ({} bytes) shortened in {took:?}", raw.len());
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    }

    /// Review I1: a line of many small values costs far more to parse than a
    /// long string; it is refused before any JSON tree is built.
    #[test]
    fn many_small_values_are_refused_before_parsing() {
        let numbers = format!("{{\"type\":\"user\",\"items\":[{}]}}", vec!["1"; 200_000].join(","));
        assert!(numbers.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
        let started = std::time::Instant::now();
        assert_eq!(reduce_oversized_line(numbers.as_bytes(), OversizedLinePolicy::ClaudeStreamJson).unwrap_err(), OversizedLineRefusal::TooManyValues);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        // Separators inside strings are not counted.
        assert_eq!(count_values(br#"{"a":"x,y:z[{","b":[1,2]}"#), 7);
        assert_eq!(count_values(br#"{"a":"q\"[,"}"#), 3);
    }

    #[test]
    fn a_session_keeps_going_after_several_large_images() {
        let expected = crate::provider::vendor_observation::claude_init::ExpectedInit {
            cwd: test_path!("/snapshot"), model: "model", permission_mode: "default", tools: &["Read"],
        };
        let mut session = claude_session::Session::new(&expected);
        let init = json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":test_path!("/snapshot"),"model":"model","permissionMode":"default","tools":["Read"],"mcp_servers":[],"skills":[],"plugins":[]});
        session.observe(&BoundedObservation::new(line(&init)).unwrap()).unwrap();
        for index in 0..10 {
            let mut frame = image_result(148 * 1024);
            frame["uuid"] = json!(format!("u{index}"));
            let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
            match session.observe(&reduced).unwrap() {
                // The work history still learns how the tool use ended.
                claude_session::Event::Output(claude_events::Projection::ToolResults(results)) => {
                    assert_eq!(results.len(), 1);
                    assert_eq!(results[0].tool_use_id, "toolu_1");
                    assert_eq!(results[0].is_error, Some(false));
                }
                _ => panic!("expected the tool result"),
            }
        }
        assert!(!session.is_closed());
    }

    /// Feeds a frame to the stream in uneven pieces (some split inside
    /// escapes and multi-byte characters).
    fn streamed(frame: &[u8], policy: OversizedLinePolicy) -> Result<BoundedObservation, OversizedLineRefusal> {
        let body = frame.strip_suffix(b"\n").unwrap_or(frame);
        let mut stream = OversizedLineStream::new(policy);
        let mut at = 0;
        let mut step = 1;
        while at < body.len() {
            let end = (at + step).min(body.len());
            stream.feed(&body[at..end])?;
            at = end;
            step = (step * 7 + 3) % 70_001 + 1;
        }
        stream.finish()
    }

    /// ADE stabilization A: a streamed line becomes exactly what the same
    /// line read whole becomes, for every shortening rule (a long string's
    /// shortened text, a new file's line count, each kind of diff count,
    /// escapes and characters split between pieces), and a frame either path
    /// refuses the other refuses too.
    #[test]
    fn a_streamed_line_reduces_to_the_same_bytes_as_a_line_read_whole() {
        let escaped = format!("한글 \"quote\" \\ tab\t {}\n{}", "😀".repeat(3000), "line\r\n".repeat(2000));
        let created = json!({"type":"user","uuid":"u2","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_3","content":"done"}]},
            "tool_use_result":{"type":"create","filePath":"big.txt","content":"line of the new file\n".repeat(10_000)}});
        let no_trailing = json!({"type":"user","uuid":"u4","tool_use_result":{"type":"create","filePath":"b.txt","content":format!("{}last", "x\n".repeat(5000))}});
        let unicode = json!({"type":"user","uuid":"u5","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":escaped}]}});
        let patch = format!("@@ -1,2 +1,3 @@\n{}{} context\n", "+added line\n".repeat(9000), "-removed line\n".repeat(4000));
        let headed = format!("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n{}", "+new\n-old\n".repeat(3000));
        let change = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"c","type":"fileChange","status":"completed","changes":[
                {"path":"src/big.ts","kind":{"type":"update"},"diff":patch},
                {"path":"src/headed.ts","kind":{"type":"update"},"diff":headed},
                {"path":"src/new.ts","kind":{"type":"add"},"diff":"new line\n".repeat(12_000)},
                {"path":"src/gone.ts","kind":{"type":"delete"},"diff":"old line\n".repeat(6_000)},
                {"path":"src/small.ts","kind":{"type":"update"},"diff":"@@\n+a\n"}]}}});
        let command = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"i","type":"commandExecution","command":"cat big.log","aggregatedOutput":"B".repeat(300_000),"status":"completed"}}});
        let delta = json!({"method":"item/commandExecution/outputDelta","params":{"threadId":"t","turnId":"r","itemId":"i","delta":"é".repeat(90_000)}});
        let assistant = json!({"type":"assistant","uuid":"a1","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"short public text"},
                {"type":"tool_use","id":"toolu_2","name":"Write","input":{"file_path":"big.txt","content":"x".repeat(200_000)}}]}});
        for (frame, policy) in [
            (image_result(148 * 1024), OversizedLinePolicy::ClaudeStreamJson),
            (created, OversizedLinePolicy::ClaudeStreamJson),
            (no_trailing, OversizedLinePolicy::ClaudeStreamJson),
            (unicode, OversizedLinePolicy::ClaudeStreamJson),
            (assistant, OversizedLinePolicy::ClaudeStreamJson),
            (change, OversizedLinePolicy::CodexAppServer),
            (command, OversizedLinePolicy::CodexAppServer),
            (delta, OversizedLinePolicy::CodexAppServer),
        ] {
            let raw = line(&frame);
            let whole = reduce_oversized_line(&raw, policy).unwrap();
            let streamed = streamed(&raw, policy).unwrap();
            assert_eq!(streamed.bytes, whole.bytes, "{}", String::from_utf8_lossy(&whole.bytes));
            assert!(!streamed.withheld());
        }
        // Refused alike: public text over the keep size, protected frames.
        for (frame, policy) in [
            (json!({"type":"assistant","uuid":"a","message":{"content":[{"type":"text","text":"y".repeat(200_000)}]}}), OversizedLinePolicy::ClaudeStreamJson),
            (json!({"type":"result","subtype":"success","result":"r".repeat(200_000)}), OversizedLinePolicy::ClaudeStreamJson),
            (json!({"type":"control_response","response":{"pad":"p".repeat(200_000)}}), OversizedLinePolicy::ClaudeStreamJson),
            (json!({"id":3,"method":"item/commandExecution/requestApproval","params":{"pad":"p".repeat(200_000)}}), OversizedLinePolicy::CodexAppServer),
            (json!({"method":"item/started","params":{"item":{"type":"commandExecution","command":"c".repeat(200_000)}}}), OversizedLinePolicy::CodexAppServer),
        ] {
            let raw = line(&frame);
            assert!(reduce_oversized_line(&raw, policy).is_err());
            assert!(streamed(&raw, policy).is_err(), "{}", &frame.to_string()[..60]);
        }
    }

    /// A line far past the session bound streams through in pieces and keeps
    /// the session going; memory stays at its skeleton.
    #[test]
    fn a_line_past_the_session_bound_streams_through_with_a_bounded_skeleton() {
        let size = 20 * 1024 * 1024;
        let head = br#"{"type":"user","uuid":"big","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_9","content":[{"type":"image","source":{"type":"base64","data":""#;
        let tail = br#""}}]}]}}"#;
        let mut stream = OversizedLineStream::new(OversizedLinePolicy::ClaudeStreamJson);
        stream.feed(head).unwrap();
        let piece = vec![b'A'; 1024 * 1024];
        for _ in 0..size / piece.len() { stream.feed(&piece).unwrap(); }
        stream.feed(tail).unwrap();
        assert!(stream.len() > PROVIDER_SESSION_LINE_RAW_MAX_BYTES);
        assert!(stream.skeleton.len() < 4096, "the skeleton holds structure only: {}", stream.skeleton.len());
        let observed = stream.finish().unwrap();
        let value = observed.json().unwrap();
        let data = value["message"]["content"][0]["content"][0]["source"]["data"].as_str().unwrap();
        assert_eq!(data, format!("{}…[{} bytes omitted]", "A".repeat(KEPT_PREFIX_BYTES), size - KEPT_PREFIX_BYTES));
        assert_eq!(value["message"]["content"][0]["tool_use_id"], "toolu_9");
        // Past the streamed bound, or with too many small strings to hold.
        let mut runaway = OversizedLineStream::new(OversizedLinePolicy::ClaudeStreamJson);
        runaway.feed(head).unwrap();
        let mut refused = None;
        for _ in 0..=PROVIDER_SESSION_LINE_STREAM_MAX_BYTES / piece.len() { if let Err(refusal) = runaway.feed(&piece) { refused = Some(refusal); break; } }
        assert_eq!(refused, Some(OversizedLineRefusal::OverStreamBound));
        assert!(runaway.feed(b"x").is_err() && runaway.finish().is_err(), "a refusal is final");
        // Many kept-size strings (PR #25 review I4): past 1 MiB the skeleton
        // keeps only short ones, so it stays bounded while reading; such a
        // frame then does not fit the observation bound and is refused at
        // the end, not halfway.
        let mut many = OversizedLineStream::new(OversizedLinePolicy::ClaudeStreamJson);
        many.feed(br#"{"type":"user","items":["#).unwrap();
        let small = format!("\"{}\",", "s".repeat(4000));
        for _ in 0..4000 { many.feed(small.as_bytes()).unwrap(); }
        many.feed(br#""end"]}"#).unwrap();
        assert!(many.len() > PROVIDER_SESSION_LINE_RAW_MAX_BYTES && many.skeleton.len() < 2 * LEAN_FROM_BYTES, "{}", many.skeleton.len());
        assert_eq!(many.finish().unwrap_err(), OversizedLineRefusal::StillOverBound);
    }

    /// Malformed text inside a left-out string is refused like any malformed
    /// line, even split between pieces.
    #[test]
    fn a_streamed_line_with_bad_json_inside_a_long_string_is_refused() {
        let long = "z".repeat(10_000);
        for bad in [
            format!(r#"{{"type":"user","pad":"{long}\q"}}"#),
            format!(r#"{{"type":"user","pad":"{long}\ud800x"}}"#),
            format!(r#"{{"type":"user","pad":"{long}\udc00"}}"#),
            format!("{{\"type\":\"user\",\"pad\":\"{long}\u{1}\"}}"),
            format!(r#"{{"type":"user","pad":"{long}"#),
            format!(r#"{{"type":"user","type":"user","pad":"{long}"}}"#),
        ] {
            assert!(streamed(bad.as_bytes(), OversizedLinePolicy::ClaudeStreamJson).is_err(), "{}", &bad[bad.len() - 12..]);
        }
        let mut invalid_utf8 = format!(r#"{{"type":"user","pad":"{long}"#).into_bytes();
        invalid_utf8.extend_from_slice(&[0xE2, 0x28, 0xA1]);
        invalid_utf8.extend_from_slice(b"\"}");
        assert!(streamed(&invalid_utf8, OversizedLinePolicy::ClaudeStreamJson).is_err());
        // A placeholder named in provider text is just text: a guess cannot
        // stand in for a left-out string.
        let forged = json!({"type":"user","pad":"x".repeat(10_000),"note":format!("{LONG_MARK}0000000000000000000000000000000:0")});
        let observed = streamed(&line(&forged), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        assert!(observed.json().unwrap()["note"].as_str().unwrap().starts_with(LONG_MARK));
    }
}
