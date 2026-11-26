//! Model-free renewal of this app's Claude OAuth credentials only.
//! Endpoint/client and response shape verified against official Claude Code 2.1.322.
use crate::{
    provider_claude_profile::ClaudeProfileBinding, provider_profile::LoginProfile, SidecarError,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, Zeroizing};
use unicode_normalization::UnicodeNormalization;

const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const MAX_BYTES: usize = 64 * 1024;
fn failure() -> SidecarError {
    SidecarError::new(
        "providerLoginFailed",
        "Claude credential renewal unavailable",
    )
}
fn token(value: &Value) -> Result<&str, SidecarError> {
    value
        .as_str()
        .filter(|s| {
            (16..=8192).contains(&s.len())
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._~+/-".contains(&b))
        })
        .ok_or_else(failure)
}
fn now() -> Result<u64, SidecarError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| failure())?
        .as_millis() as u64)
}

fn renewed(
    original: &[u8],
    at: u64,
    exchange: impl FnOnce(Value) -> Result<Value, SidecarError>,
) -> Result<Option<Vec<u8>>, SidecarError> {
    if original.len() > MAX_BYTES {
        return Err(failure());
    }
    let mut value: Value = serde_json::from_slice(original).map_err(|_| failure())?;
    let auth = value
        .get_mut("claudeAiOauth")
        .and_then(Value::as_object_mut)
        .ok_or_else(failure)?;
    token(auth.get("accessToken").ok_or_else(failure)?)?;
    let expiry = auth
        .get("expiresAt")
        .and_then(Value::as_u64)
        .ok_or_else(failure)?;
    if expiry > at {
        return Ok(None);
    }
    let refresh = token(auth.get("refreshToken").ok_or_else(failure)?)?;
    if auth
        .get("clientId")
        .is_some_and(|v| !v.is_null() && v.as_str() != Some(CLIENT_ID))
    {
        return Err(failure());
    }
    let scopes = auth
        .get("scopes")
        .and_then(Value::as_array)
        .ok_or_else(failure)?;
    if scopes.is_empty() || scopes.len() > 64 {
        return Err(failure());
    }
    let scopes: Vec<&str> = scopes
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 128
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b":_-".contains(&b))
                })
                .ok_or_else(failure)
        })
        .collect::<Result<_, _>>()?;
    let response = exchange(
        json!({"grant_type":"refresh_token","refresh_token":refresh,"client_id":CLIENT_ID,"scope":scopes.join(" ")}),
    )?;
    let access = token(&response["access_token"])?;
    let seconds = response["expires_in"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 31_536_000)
        .ok_or_else(failure)?;
    let expires = at.checked_add(seconds * 1000).ok_or_else(failure)?;
    if let Some(scope) = response.get("scope") {
        let returned = scope.as_str().ok_or_else(failure)?;
        if returned.split_whitespace().any(|s| !scopes.contains(&s)) {
            return Err(failure());
        }
    }
    let rotation = response
        .get("refresh_token")
        .map(token)
        .transpose()?
        .map(str::to_owned);
    auth.insert("accessToken".into(), access.into());
    auth.insert("expiresAt".into(), expires.into());
    if let Some(rotation) = rotation {
        auth.insert("refreshToken".into(), rotation.into());
    }
    if let Some(scope) = response.get("scope") {
        auth.insert(
            "scopes".into(),
            json!(scope
                .as_str()
                .ok_or_else(failure)?
                .split_whitespace()
                .collect::<Vec<_>>()),
        );
    }
    if let Some(seconds) = response.get("refresh_token_expires_in") {
        let seconds = seconds
            .as_u64()
            .filter(|n| *n <= 315_360_000)
            .ok_or_else(failure)?;
        auth.insert(
            "refreshTokenExpiresAt".into(),
            at.checked_add(seconds * 1000).ok_or_else(failure)?.into(),
        );
    }
    let bytes = ascii_json(&value)?;
    if bytes.len() > MAX_BYTES {
        return Err(failure());
    }
    Ok(Some(bytes))
}

/// The app-only Claude login is read and written only through macOS's own
/// `/usr/bin/security`, the tool the official Claude Code uses to create and
/// update this item. The item's allow-list names only that tool and its
/// partition list holds `apple-tool:`. A direct Keychain call from this
/// sidecar is checked against the sidecar's own signature instead: without an
/// Apple team ID that is the binary's hash (`cdhash:`), so every rebuild was a
/// new program and asked again.
///
/// Every call is a child process with a deadline (a locked keychain or a
/// pending "allow" prompt must not hold the profile reservation forever) and
/// no core dump. Secrets travel on private pipes only: never argv, stderr,
/// logs or files.
const KEYCHAIN_DEADLINE: Duration = Duration::from_secs(30);
const SECURITY: &str = "/usr/bin/security";
const ACCOUNT: &str = "unknown";

/// The longest command line sent to `security -i`. The tool reads each line
/// into a 4096-byte buffer (4095 characters with the newline measured on
/// macOS 26), and a longer line is split: the rest runs as a separate command
/// whose "unknown command" error repeats it. The official Claude Code uses
/// the same bound.
const SECURITY_LINE_MAX: usize = 4032;
/// A login this long or longer is not renewed at all: the renewed value could
/// outgrow one `security -i` line, and once the server has rotated the token
/// there would be no safe way left to store it. Checked before the exchange,
/// so the stored login stays usable. A real login is about 600 bytes.
const RENEWABLE_MAX: usize = 1400;

fn outcome_unknown() -> SidecarError {
    SidecarError::new(
        "providerLoginFailed",
        "Claude credential renewal outcome could not be confirmed",
    )
}

/// No core dump for this process from here on. The sidecar holds secrets in
/// memory while renewing; only the soft limit is lowered.
fn limit_core_dumps() {
    // SAFETY: plain libc calls on a stack value; no pointers are retained.
    unsafe {
        let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        if libc::getrlimit(libc::RLIMIT_CORE, &mut limit) == 0 && limit.rlim_cur != 0 {
            limit.rlim_cur = 0;
            libc::setrlimit(libc::RLIMIT_CORE, &limit);
        }
    }
}

/// A child that may hold a secret: no core dump at all (soft and hard limit 0,
/// set before exec), stdin piped, stdout piped, stderr discarded.
fn secret_child(program: &str) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(program);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // SAFETY: only setrlimit, which is async-signal-safe, runs between fork and exec.
    unsafe {
        command.pre_exec(|| {
            let none = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            if libc::setrlimit(libc::RLIMIT_CORE, &none) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

/// How a bounded child ended.
enum Ran {
    Exited(Option<i32>, Zeroizing<Vec<u8>>),
    /// Deadline or cancel: the child was killed and reaped, so it can no
    /// longer change anything. What it did before that is unknown.
    Stopped,
}

/// Runs `command` (from `secret_child`), sends `input` and wipes it as soon as
/// it is written, before waiting. Stops the child at `deadline` or when
/// `cancel` is set; never returns while the child could still be running.
fn run_bounded(
    mut command: std::process::Command,
    input: Option<&mut [u8]>,
    deadline: Duration,
    cancel: Option<&AtomicBool>,
) -> Result<Ran, SidecarError> {
    use std::io::Write;
    let mut child = command.spawn().map_err(|_| failure())?;
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut bytes = Zeroizing::new(Vec::with_capacity(4096));
        // Discarded output (`Stdio::null()`): nothing to read.
        let Some(mut stdout) = stdout else {
            return bytes;
        };
        let mut chunk = Zeroizing::new([0u8; 4096]);
        loop {
            match stdout.read(&mut chunk[..]) {
                Ok(0) | Err(_) => break,
                Ok(n) if bytes.len() + n <= 2 * MAX_BYTES + 1 => bytes.extend_from_slice(&chunk[..n]),
                // Too much output: keep draining so the child is never blocked.
                Ok(_) => bytes.clear(),
            }
        }
        bytes
    });
    let mut stdin = child.stdin.take();
    let wrote = match (input, stdin.as_mut()) {
        (Some(input), Some(pipe)) => {
            let wrote = pipe.write_all(input).is_ok();
            input.zeroize();
            wrote
        }
        (Some(input), None) => {
            input.zeroize();
            false
        }
        (None, _) => true,
    };
    // Closing stdin ends a `security -i` session after its one command.
    drop(stdin);
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if started.elapsed() >= deadline || cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let Some(status) = status else {
        let _ = child.kill();
        // Reaped: the reservation is only released after this point.
        let _ = child.wait();
        let _ = reader.join();
        return Ok(Ran::Stopped);
    };
    let output = reader.join().map_err(|_| failure())?;
    if !wrote {
        return Err(failure());
    }
    Ok(Ran::Exited(status.code(), output))
}

/// `find-generic-password -w` prints the stored bytes as text, or as hex when
/// any byte is not printable (Apple's keychain_find.c), e.g. UTF-8 JSON.
/// A JSON object starts with `{`, which is never a hex digit.
fn decode_password_output(mut out: Zeroizing<Vec<u8>>) -> Option<Zeroizing<Vec<u8>>> {
    if out.last() == Some(&b'\n') {
        out.pop();
    }
    if out.is_empty() {
        return None;
    }
    if out[0] == b'{' {
        return (out.len() <= MAX_BYTES).then_some(out);
    }
    if out.len() % 2 != 0 || out.len() > 2 * MAX_BYTES || !out.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let digit = |b: u8| (b as char).to_digit(16).unwrap_or(0) as u8;
    let mut bytes = Zeroizing::new(Vec::with_capacity(out.len() / 2));
    for pair in out.chunks_exact(2) {
        bytes.push(digit(pair[0]) << 4 | digit(pair[1]));
    }
    Some(bytes)
}

/// The value of one attribute line of `find-generic-password` (no secret is
/// printed without `-w`/`-g`), e.g. `"cdat"<timedate>=…` or `0x00000007 <blob>=…`.
fn attribute<'a>(listing: &'a str, key: &str) -> Option<&'a str> {
    listing
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(key))
        .and_then(|rest| rest.split_once('=').map(|(_, value)| value))
}

/// A stored login: its bytes, the item's creation time (its identity: an item
/// deleted and created again has a new one) and its label, kept on update.
struct StoredLogin {
    secret: Zeroizing<Vec<u8>>,
    created: String,
    label: Option<String>,
}

trait LoginStore {
    /// `None`: no such item. `cancel` applies before anything has changed.
    fn read(&self, cancel: Option<&AtomicBool>) -> Result<Option<StoredLogin>, SidecarError>;
    /// Replaces the data of the item; the outcome is confirmed by reading.
    fn write(&self, secret: &[u8], label: Option<&str>) -> Result<(), SidecarError>;
}

struct SecurityTool<'a> {
    service: &'a str,
    keychain: Option<&'a str>,
}

impl SecurityTool<'_> {
    fn find(&self, password: bool, cancel: Option<&AtomicBool>) -> Result<Option<Zeroizing<Vec<u8>>>, SidecarError> {
        let mut command = secret_child(SECURITY);
        command.args(["find-generic-password", "-s", self.service, "-a", ACCOUNT]);
        if password {
            command.arg("-w");
        }
        if let Some(keychain) = self.keychain {
            command.arg(keychain);
        }
        command.stdin(std::process::Stdio::null());
        match run_bounded(command, None, KEYCHAIN_DEADLINE, cancel)? {
            Ran::Exited(Some(0), output) => Ok(Some(output)),
            // errSecItemNotFound as the tool reports it.
            Ran::Exited(Some(44), _) => Ok(None),
            Ran::Exited(..) | Ran::Stopped => Err(failure()),
        }
    }
}

impl LoginStore for SecurityTool<'_> {
    fn read(&self, cancel: Option<&AtomicBool>) -> Result<Option<StoredLogin>, SidecarError> {
        let Some(listing) = self.find(false, cancel)? else {
            return Ok(None);
        };
        let listing = String::from_utf8_lossy(&listing).into_owned();
        let created = attribute(&listing, "\"cdat\"<timedate>").ok_or_else(failure)?.to_owned();
        let label = attribute(&listing, "0x00000007 <blob>")
            .and_then(|v| v.strip_prefix('"')?.strip_suffix('"'))
            .map(str::to_owned);
        let Some(secret) = self.find(true, cancel)? else {
            return Ok(None);
        };
        let secret = decode_password_output(secret).ok_or_else(failure)?;
        Ok(Some(StoredLogin { secret, created, label }))
    }

    fn write(&self, secret: &[u8], label: Option<&str>) -> Result<(), SidecarError> {
        let mut line = security_update_line(self.service, ACCOUNT, label, secret, self.keychain)
            .ok_or_else(failure)?;
        let mut command = secret_child(SECURITY);
        command.arg("-i").stdout(std::process::Stdio::null());
        // Not cancelable: after the server has rotated the token this write
        // is the only copy. The deadline still bounds it.
        match run_bounded(command, Some(&mut line[..]), KEYCHAIN_DEADLINE, None)? {
            Ran::Exited(Some(0), _) => Ok(()),
            _ => Err(failure()),
        }
    }
}

/// One `security -i` line that replaces the item's data, or `None` when a
/// field cannot be quoted safely or the line would be too long. The secret is
/// hex (`-X`), so it needs no quoting and cannot end the command early. The
/// label is passed back so the update keeps it (the tool otherwise resets it
/// to the service name, as Claude Code's own updates do).
fn security_update_line(
    service: &str,
    account: &str,
    label: Option<&str>,
    secret: &[u8],
    keychain: Option<&str>,
) -> Option<Zeroizing<Vec<u8>>> {
    let quotable = |text: &str| {
        !text.is_empty() && text.bytes().all(|b| (0x20..0x7f).contains(&b) && b != b'"' && b != b'\\')
    };
    // A label that cannot be quoted is left to the tool's default.
    let label = label.filter(|l| quotable(l));
    if secret.is_empty()
        || !quotable(service)
        || !quotable(account)
        || keychain.is_some_and(|k| !quotable(k))
    {
        return None;
    }
    const PREFIX: &[u8] = b"add-generic-password -U -a \"";
    const HEX: &[u8; 16] = b"0123456789abcdef";
    // Sized before any secret byte is copied, so the buffer never grows and
    // leaves an unzeroed copy behind.
    let length = PREFIX.len()
        + account.len()
        + 6 // `" -s "`
        + service.len()
        + label.map_or(0, |l| 6 + l.len()) // `" -l "` + label
        + 5 // `" -X `
        + 2 * secret.len()
        + keychain.map_or(0, |k| k.len() + 3)
        + 1;
    if length > SECURITY_LINE_MAX {
        return None;
    }
    let mut line = Zeroizing::new(Vec::with_capacity(length));
    line.extend_from_slice(PREFIX);
    line.extend_from_slice(account.as_bytes());
    line.extend_from_slice(b"\" -s \"");
    line.extend_from_slice(service.as_bytes());
    if let Some(label) = label {
        line.extend_from_slice(b"\" -l \"");
        line.extend_from_slice(label.as_bytes());
    }
    line.extend_from_slice(b"\" -X ");
    for byte in secret {
        line.push(HEX[usize::from(byte >> 4)]);
        line.push(HEX[usize::from(byte & 15)]);
    }
    if let Some(keychain) = keychain {
        line.extend_from_slice(b" \"");
        line.extend_from_slice(keychain.as_bytes());
        line.push(b'"');
    }
    line.push(b'\n');
    debug_assert_eq!(line.len(), length);
    Some(line)
}

/// Stores `replacement` only over the exact login `before` that was renewed,
/// and confirms the result by reading back.
///
/// - Before writing: the item must still be the same item (creation time)
///   with the same bytes, or nothing is written.
/// - After writing (also after a failed or stopped write): the item must hold
///   `replacement` and still be the same item. If it still holds `before`,
///   nothing was written and the stored login is intact.
///
/// Limit: the tool cannot update conditionally (`-U` updates, or creates when
/// the item is missing). A change by another program in the milliseconds
/// between the check and the write is not prevented: an overwritten newer
/// login cannot be detected afterwards, and an item deleted in that window is
/// created again, which is detected (new creation time) and reported as an
/// unconfirmed outcome.
fn save_replacement(store: &dyn LoginStore, before: &StoredLogin, replacement: &[u8]) -> Result<(), SidecarError> {
    let current = store.read(None)?.ok_or_else(failure)?;
    if current.created != before.created || current.secret != before.secret {
        return Err(failure());
    }
    let _ = store.write(replacement, current.label.as_deref());
    match store.read(None) {
        Ok(Some(after)) if after.created == before.created && after.secret[..] == *replacement => Ok(()),
        Ok(Some(after)) if after.created == before.created && after.secret == before.secret => Err(failure()),
        _ => Err(outcome_unknown()),
    }
}

/// JSON with every non-ASCII character escaped (`\uXXXX`): the stored bytes
/// stay printable, so `find-generic-password -w` returns them as they are.
struct AsciiJson;
impl serde_json::ser::Formatter for AsciiJson {
    fn write_string_fragment<W: ?Sized + std::io::Write>(&mut self, writer: &mut W, fragment: &str) -> std::io::Result<()> {
        for ch in fragment.chars() {
            if ch.is_ascii() {
                writer.write_all(&[ch as u8])?;
            } else {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    write!(writer, "\\u{unit:04x}")?;
                }
            }
        }
        Ok(())
    }
}

fn ascii_json(value: &Value) -> Result<Vec<u8>, SidecarError> {
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, AsciiJson);
    serde::Serialize::serialize(value, &mut serializer).map_err(|_| failure())?;
    Ok(bytes)
}

fn exchange_token(body: Value) -> Result<Value, SidecarError> {
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| failure())?;
    let response = client.post(TOKEN_URL).json(&body).send().map_err(|_| failure())?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(failure());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    response
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() > MAX_BYTES {
        return Err(failure());
    }
    serde_json::from_slice(&bytes).map_err(|_| failure())
}

/// Reads, renews when expired and stores. `cancel` is honored until the token
/// exchange starts; after it the renewed login is stored (bounded by the
/// deadline), because the server may already have retired the old one.
fn renew_stored(
    store: &dyn LoginStore,
    cancel: &AtomicBool,
    still_bound: &dyn Fn() -> Result<(), SidecarError>,
    exchange: impl FnOnce(Value) -> Result<Value, SidecarError>,
) -> Result<(), SidecarError> {
    let Some(before) = store.read(Some(cancel))? else {
        // No app keychain login yet: let the official auth status report idle.
        return Ok(());
    };
    if before.secret.len() >= RENEWABLE_MAX {
        // Too long to renew safely; leave it for the official CLI.
        return renewed(&before.secret, now()?, |_| Err(failure())).map(|_| ());
    }
    let replacement = renewed(&before.secret, now()?, |body| {
        if cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        exchange(body)
    })?;
    let Some(replacement) = replacement else {
        return Ok(());
    };
    let replacement = Zeroizing::new(replacement);
    still_bound()?;
    save_replacement(store, &before, &replacement)
}

pub(crate) fn refresh_if_expired(profile: &ClaudeProfileBinding, cancel: &AtomicBool) -> Result<(), SidecarError> {
    limit_core_dumps();
    crate::provider_login_process::with_credentials_reserved(profile, || {
        let path = profile.revalidate()?;
        let hash = format!(
            "{:x}",
            Sha256::digest(path.to_string_lossy().nfc().collect::<String>().as_bytes())
        );
        let service = format!("Claude Code-credentials-{}", &hash[..8]);
        let store = SecurityTool { service: &service, keychain: None };
        let still_bound = || {
            if profile.revalidate()? != path {
                return Err(failure());
            }
            Ok(())
        };
        renew_stored(&store, cancel, &still_bound, exchange_token)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn credential() -> Vec<u8> {
        serde_json::to_vec(&json!({"other":{"keep":true},"claudeAiOauth":{"accessToken":"old-access-token-123","refreshToken":"old-refresh-token-123","expiresAt":100,"scopes":["user:profile"],"subscriptionType":"max"}})).unwrap()
    }
    #[test]
    fn valid_credential_does_not_refresh() {
        assert!(renewed(&credential(), 99, |_| panic!("unexpected request"))
            .unwrap()
            .is_none());
    }
    #[test]
    fn expiry_renews_and_preserves_unrelated_fields() {
        let bytes = renewed(&credential(),100,|request| { assert_eq!(request["scope"],"user:profile"); assert_eq!(request["client_id"], CLIENT_ID); Ok(json!({"access_token":"new-access-token-123","refresh_token":"new-refresh-token-123","expires_in":3600,"scope":"user:profile"})) }).unwrap().unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["claudeAiOauth"]["expiresAt"], 3_600_100);
        assert_eq!(
            value["claudeAiOauth"]["refreshToken"],
            "new-refresh-token-123"
        );
        assert_eq!(value["other"]["keep"], true);
        assert_eq!(value["claudeAiOauth"]["subscriptionType"], "max");
    }
    #[test]
    fn missing_rotation_retains_refresh_token() {
        let bytes = renewed(&credential(), 100, |_| {
            Ok(json!({"access_token":"new-access-token-123","expires_in":3600}))
        })
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value["claudeAiOauth"]["refreshToken"],
            "old-refresh-token-123"
        );
    }
    #[test]
    fn security_line_carries_only_hex_and_stays_within_one_line() {
        let secret = br#"{"claudeAiOauth":{"accessToken":"a b\"c\\d"}}"#;
        let line = security_update_line("Claude Code-credentials-0123abcd", "unknown", None, secret, None).unwrap();
        let text = String::from_utf8(line.to_vec()).unwrap();
        let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            text,
            format!("add-generic-password -U -a \"unknown\" -s \"Claude Code-credentials-0123abcd\" -X {hex}\n")
        );
        assert_eq!(text.matches('\n').count(), 1);
        assert!(!text.contains("accessToken"));
        // Anything renewable fits; one byte past the bound does not.
        let label = Some("Claude Code-credentials-0123abcd");
        let fixed = "add-generic-password -U -a \"unknown\" -s \"Claude Code-credentials-0123abcd\" -l \"Claude Code-credentials-0123abcd\" -X ".len();
        let fits = (SECURITY_LINE_MAX - 1 - fixed) / 2;
        assert!(fits > RENEWABLE_MAX + 400);
        let line = security_update_line("Claude Code-credentials-0123abcd", "unknown", label, &vec![b'x'; fits], None).unwrap();
        assert!(line.len() <= SECURITY_LINE_MAX);
        assert!(security_update_line("Claude Code-credentials-0123abcd", "unknown", label, &vec![b'x'; fits + 1], None).is_none());
        for service in ["", "bad\"quote", "bad\\slash", "new\nline", "비ASCII"] {
            assert!(security_update_line(service, "unknown", None, secret, None).is_none());
            assert!(security_update_line("ok", service, None, secret, None).is_none());
        }
        assert!(security_update_line("ok", "unknown", None, b"", None).is_none());
        assert!(security_update_line("ok", "unknown", None, secret, Some("/tmp/a\"b")).is_none());
    }

    /// Review minor 2: an update passes the item's own label back.
    #[test]
    fn security_line_keeps_a_quotable_label_and_drops_an_unsafe_one() {
        let line = security_update_line("svc", "unknown", Some("My 'label'"), b"x", None).unwrap();
        assert_eq!(&line[..], b"add-generic-password -U -a \"unknown\" -s \"svc\" -l \"My 'label'\" -X 78\n");
        for unsafe_label in ["a\"b", "a\\b", "", "줄"] {
            let line = security_update_line("svc", "unknown", Some(unsafe_label), b"x", None).unwrap();
            assert_eq!(&line[..], b"add-generic-password -U -a \"unknown\" -s \"svc\" -X 78\n");
        }
    }

    // ---- Review I1: every Keychain call is bounded and reaped ----

    fn pid_alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks for existence.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    fn sleeper(pid_file: &std::path::Path) -> std::process::Command {
        let mut command = secret_child("/bin/sh");
        command.arg("-c").arg(format!("echo $$ > '{}'; exec /bin/sleep 30", pid_file.display()));
        command.stdin(std::process::Stdio::null());
        command
    }

    #[test]
    fn a_keychain_call_waiting_past_its_deadline_is_killed_and_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let started = std::time::Instant::now();
        let ran = run_bounded(sleeper(&pid_file), None, Duration::from_millis(300), None).unwrap();
        assert!(matches!(ran, Ran::Stopped));
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        // Reaped before returning: nothing can still write to the keychain.
        assert!(!pid_alive(pid));
    }

    #[test]
    fn a_keychain_call_is_stopped_when_the_login_is_canceled() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let setter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            flag.store(true, Ordering::Release);
        });
        let started = std::time::Instant::now();
        let ran = run_bounded(sleeper(&pid_file), None, Duration::from_secs(60), Some(&cancel)).unwrap();
        setter.join().unwrap();
        assert!(matches!(ran, Ran::Stopped));
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        assert!(!pid_alive(pid));
    }

    /// What the fake keychain does when asked to write.
    #[derive(Clone, Copy)]
    enum FakeWrite {
        Apply,
        /// Stopped at the deadline before the tool changed anything.
        StopBeforeApplying,
        /// Stopped at the deadline after the tool had already stored it.
        StopAfterApplying,
        /// Another program deleted the item just before the write; `-U`
        /// creates it again.
        ItemDeletedFirst,
    }

    struct FakeStore {
        item: std::cell::RefCell<Option<(Vec<u8>, String)>>,
        mode: FakeWrite,
        /// Another program stores these bytes just before this read (1-based).
        changed_before_read: Option<(usize, Vec<u8>)>,
        reads: std::cell::Cell<usize>,
        writes: std::cell::Cell<usize>,
    }

    impl FakeStore {
        fn new(bytes: Vec<u8>, mode: FakeWrite) -> Self {
            Self {
                item: std::cell::RefCell::new(Some((bytes, "created-1".into()))),
                mode,
                changed_before_read: None,
                reads: Default::default(),
                writes: Default::default(),
            }
        }
        fn bytes(&self) -> Vec<u8> {
            self.item.borrow().as_ref().unwrap().0.clone()
        }
    }

    impl LoginStore for FakeStore {
        fn read(&self, _: Option<&AtomicBool>) -> Result<Option<StoredLogin>, SidecarError> {
            let n = self.reads.get() + 1;
            self.reads.set(n);
            if let Some((at, bytes)) = &self.changed_before_read {
                if *at == n {
                    self.item.borrow_mut().as_mut().unwrap().0 = bytes.clone();
                }
            }
            Ok(self.item.borrow().clone().map(|(secret, created)| StoredLogin {
                secret: Zeroizing::new(secret),
                created,
                label: None,
            }))
        }
        fn write(&self, secret: &[u8], _: Option<&str>) -> Result<(), SidecarError> {
            self.writes.set(self.writes.get() + 1);
            let mut item = self.item.borrow_mut();
            match self.mode {
                FakeWrite::Apply => item.as_mut().unwrap().0 = secret.to_vec(),
                FakeWrite::StopBeforeApplying => return Err(failure()),
                FakeWrite::StopAfterApplying => {
                    item.as_mut().unwrap().0 = secret.to_vec();
                    return Err(failure());
                }
                FakeWrite::ItemDeletedFirst => *item = Some((secret.to_vec(), "created-2".into())),
            }
            Ok(())
        }
    }

    fn new_token(_: Value) -> Result<Value, SidecarError> {
        Ok(json!({"access_token":"new-access-token-123","refresh_token":"new-refresh-token-123","expires_in":3600}))
    }
    fn bound() -> Result<(), SidecarError> {
        Ok(())
    }
    fn holds_new_token(bytes: &[u8]) -> bool {
        serde_json::from_slice::<Value>(bytes).unwrap()["claudeAiOauth"]["refreshToken"] == "new-refresh-token-123"
    }

    #[test]
    fn renewal_stores_and_confirms_the_new_login() {
        let store = FakeStore::new(credential(), FakeWrite::Apply);
        renew_stored(&store, &AtomicBool::new(false), &bound, new_token).unwrap();
        assert!(holds_new_token(&store.bytes()));
        // Original read, check before writing, confirmation after writing.
        assert_eq!((store.reads.get(), store.writes.get()), (3, 1));
    }

    #[test]
    fn a_write_stopped_at_its_deadline_keeps_the_stored_login() {
        let store = FakeStore::new(credential(), FakeWrite::StopBeforeApplying);
        let error = renew_stored(&store, &AtomicBool::new(false), &bound, new_token).unwrap_err();
        assert_eq!(error.detail, failure().detail);
        assert_eq!(store.bytes(), credential());
    }

    #[test]
    fn a_write_stopped_after_it_landed_is_confirmed_by_reading_back() {
        let store = FakeStore::new(credential(), FakeWrite::StopAfterApplying);
        renew_stored(&store, &AtomicBool::new(false), &bound, new_token).unwrap();
        assert!(holds_new_token(&store.bytes()));
    }

    #[test]
    fn canceling_before_the_exchange_contacts_no_server_and_writes_nothing() {
        let store = FakeStore::new(credential(), FakeWrite::Apply);
        let error = renew_stored(&store, &AtomicBool::new(true), &bound, |_| panic!("no exchange after cancel"));
        assert!(error.is_err());
        assert_eq!((store.writes.get(), store.bytes()), (0, credential()));
    }

    #[test]
    fn canceling_during_the_exchange_still_stores_the_rotated_login() {
        let store = FakeStore::new(credential(), FakeWrite::Apply);
        let cancel = AtomicBool::new(false);
        renew_stored(&store, &cancel, &bound, |body| {
            cancel.store(true, Ordering::Release);
            new_token(body)
        })
        .unwrap();
        assert!(holds_new_token(&store.bytes()));
    }

    #[test]
    fn a_login_too_long_to_store_safely_is_never_exchanged() {
        let mut value: Value = serde_json::from_slice(&credential()).unwrap();
        value["padding"] = json!("x".repeat(RENEWABLE_MAX));
        let long = serde_json::to_vec(&value).unwrap();
        let store = FakeStore::new(long.clone(), FakeWrite::Apply);
        assert!(renew_stored(&store, &AtomicBool::new(false), &bound, |_| panic!("not exchanged")).is_err());
        assert_eq!((store.writes.get(), store.bytes()), (0, long));
    }

    // ---- Review I2: the write lands only on the login that was renewed ----

    #[test]
    fn a_login_changed_during_the_exchange_is_not_overwritten() {
        let mut store = FakeStore::new(credential(), FakeWrite::Apply);
        // Another client stores a newer login while the server answers.
        store.changed_before_read = Some((2, b"{\"newer\":true}".to_vec()));
        assert!(renew_stored(&store, &AtomicBool::new(false), &bound, new_token).is_err());
        assert_eq!((store.writes.get(), store.bytes()), (0, b"{\"newer\":true}".to_vec()));
    }

    #[test]
    fn an_item_deleted_just_before_the_write_is_reported_unconfirmed() {
        let store = FakeStore::new(credential(), FakeWrite::ItemDeletedFirst);
        let error = renew_stored(&store, &AtomicBool::new(false), &bound, new_token).unwrap_err();
        assert_eq!(error.detail, outcome_unknown().detail);
    }

    #[test]
    fn a_profile_moved_during_the_exchange_writes_nothing() {
        let store = FakeStore::new(credential(), FakeWrite::Apply);
        assert!(renew_stored(&store, &AtomicBool::new(false), &|| Err(failure()), new_token).is_err());
        assert_eq!((store.writes.get(), store.bytes()), (0, credential()));
    }

    // ---- Review I3: what is stored reads back byte for byte ----

    #[test]
    fn renewal_keeps_escaped_non_ascii_metadata_ascii() {
        let original = br#"{"claudeAiOauth":{"accessToken":"old-access-token-123","refreshToken":"old-refresh-token-123","expiresAt":100,"scopes":["user:profile"]},"account":{"name":"\ud55c\uae00 \ud83d\ude00"}}"#;
        let bytes = renewed(original, 100, new_token).unwrap().unwrap();
        assert!(bytes.is_ascii());
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["account"]["name"], "한글 😀");
        assert!(String::from_utf8(bytes).unwrap().contains(r#""\ud55c\uae00 \ud83d\ude00""#));
    }

    #[test]
    fn password_output_in_hex_is_decoded() {
        let utf8 = "{\"a\":\"한\"}".as_bytes();
        let hex: String = utf8.iter().map(|b| format!("{b:02x}")).collect();
        let decode = |text: &str| decode_password_output(Zeroizing::new(text.as_bytes().to_vec())).map(|v| v.to_vec());
        assert_eq!(decode(&format!("{hex}\n")).unwrap(), utf8);
        assert_eq!(decode("{\"a\":1}\n").unwrap(), b"{\"a\":1}");
        assert!(decode("\n").is_none());
        assert!(decode("abc\n").is_none());
        assert!(decode("zz\n").is_none());
    }

    // ---- Review I4: no core dump of a process holding the secret ----

    #[test]
    fn secret_children_cannot_dump_core() {
        let mut command = secret_child("/bin/sh");
        command.args(["-c", "ulimit -c; ulimit -Hc"]).stdin(std::process::Stdio::null());
        let Ran::Exited(Some(0), out) = run_bounded(command, None, Duration::from_secs(10), None).unwrap() else {
            panic!("shell failed");
        };
        assert_eq!(String::from_utf8_lossy(&out), "0\n0\n");
    }

    #[test]
    fn renewing_turns_off_this_process_core_dumps() {
        limit_core_dumps();
        let mut limit = libc::rlimit { rlim_cur: 1, rlim_max: 1 };
        // SAFETY: reads into a stack value.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
        assert_eq!(limit.rlim_cur, 0);
    }

    #[test]
    fn the_secret_line_is_wiped_once_written_even_before_the_child_exits() {
        let mut line = b"add-generic-password -X 7365637265740a\n".to_vec();
        let mut command = secret_child("/bin/cat");
        command.arg("-u");
        let Ran::Exited(Some(0), echoed) = run_bounded(command, Some(&mut line[..]), Duration::from_secs(10), None).unwrap()
        else {
            panic!("cat failed");
        };
        assert_eq!(&echoed[..], b"add-generic-password -X 7365637265740a\n");
        assert!(line.iter().all(|b| *b == 0));
    }

    // ---- The real tool, on a throwaway keychain file ----

    /// A throwaway keychain file, never the login keychain. Items in such a
    /// file have no partition list (format 256); that part was checked once
    /// by hand in the login keychain (docs/history.md, 2026-11-26).
    struct TempKeychain(std::path::PathBuf);
    impl TempKeychain {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("elsecrew-kc-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let keychain = Self(dir.join("test.keychain-db"));
            assert!(security(&["create-keychain", "-p", "elsecrew-test", keychain.path()]).status.success());
            assert!(security(&["unlock-keychain", "-p", "elsecrew-test", keychain.path()]).status.success());
            keychain
        }
        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }
    impl Drop for TempKeychain {
        fn drop(&mut self) {
            let _ = security(&["delete-keychain", self.path()]);
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
        }
    }
    fn security(args: &[&str]) -> std::process::Output {
        std::process::Command::new(SECURITY).args(args).output().unwrap()
    }

    #[test]
    fn security_update_keeps_the_secret_out_of_argv_the_acl_and_the_label() {
        use std::io::Write;
        let keychain = TempKeychain::new();
        let path = keychain.path();
        let service = "Elsecrew argv probe";
        // Created by the tool, as Claude Code creates the real item, with a
        // label of the user's own.
        assert!(security(&["add-generic-password", "-a", ACCOUNT, "-s", service, "-l", "My label", "-w", "{\"initial\":1}", path]).status.success());
        let acl = || {
            let dump = String::from_utf8(security(&["dump-keychain", "-a", path]).stdout).unwrap();
            // Data changes move the modification date and the integrity hash only.
            let integrity_hash = |l: &str| {
                l.trim_start()
                    .strip_prefix("description: ")
                    .is_some_and(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
            };
            dump.lines()
                .filter(|l| !l.contains("\"mdat\"") && !integrity_hash(l))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let before_acl = acl();
        assert!(before_acl.iter().any(|l| l.contains("/usr/bin/security")));
        assert!(before_acl.iter().any(|l| l.contains("0x00000007 <blob>=\"My label\"")));

        let marker = format!("argv-probe-{}-secret", uuid::Uuid::new_v4());
        let secret = format!(r#"{{"claudeAiOauth":{{"accessToken":"{marker}"}}}}"#);
        let hex: String = secret.bytes().map(|b| format!("{b:02x}")).collect();
        let line = security_update_line(service, ACCOUNT, Some("My label"), secret.as_bytes(), Some(path)).unwrap();
        let mut command = secret_child(SECURITY);
        let mut child = command.arg("-i").stdout(std::process::Stdio::null()).spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&line).unwrap();
        stdin.flush().unwrap();
        drop(line);
        // While the tool holds the secret, no process lists it in its arguments.
        let listing = std::process::Command::new("/bin/ps").args(["-axww", "-o", "pid=,args="]).output().unwrap();
        let listing = String::from_utf8_lossy(&listing.stdout);
        assert!(listing.lines().any(|l| l.ends_with("/usr/bin/security -i")));
        assert!(!listing.contains(&marker) && !listing.contains(&hex));
        drop(stdin);
        assert!(child.wait().unwrap().success());

        let store = SecurityTool { service, keychain: Some(path) };
        let stored = store.read(None).unwrap().unwrap();
        assert_eq!(&stored.secret[..], secret.as_bytes());
        assert_eq!(stored.label.as_deref(), Some("My label"));
        // Updating in place keeps the tool-only allow-list and the label.
        assert_eq!(acl(), before_acl);
    }

    #[test]
    fn the_real_tool_renews_confirms_and_reads_non_ascii_back() {
        let keychain = TempKeychain::new();
        let path = keychain.path();
        let service = "Elsecrew renewal probe";
        let original = credential();
        assert!(security(&["add-generic-password", "-a", ACCOUNT, "-s", service, "-w", std::str::from_utf8(&original).unwrap(), path]).status.success());
        let store = SecurityTool { service, keychain: Some(path) };
        renew_stored(&store, &AtomicBool::new(false), &bound, new_token).unwrap();
        let stored = store.read(None).unwrap().unwrap();
        assert!(holds_new_token(&stored.secret));
        // Raw UTF-8 (as another writer may store it) comes back as hex from
        // the tool and is still read as the same bytes.
        let utf8 = "{\"claudeAiOauth\":{},\"name\":\"한글\"}".as_bytes();
        let before = store.read(None).unwrap().unwrap();
        save_replacement(&store, &before, utf8).unwrap();
        assert_eq!(&store.read(None).unwrap().unwrap().secret[..], utf8);
        // A missing item is reported as missing, not as an error.
        let missing = SecurityTool { service: "Elsecrew missing probe", keychain: Some(path) };
        assert!(missing.read(None).unwrap().is_none());
    }

    #[test]
    fn failures_never_produce_a_replacement() {
        let original = credential();
        assert!(renewed(&original, 100, |_| Err(failure())).is_err());
        for response in [
            json!({"access_token":"short","expires_in":3600}),
            json!({"access_token":"new-access-token-123","expires_in":0}),
            json!({"access_token":"new-access-token-123","expires_in":3600,"scope":"unrequested:scope"}),
        ] {
            assert!(renewed(&original, 100, |_| Ok(response)).is_err());
        }
        assert_eq!(original, credential());
    }
}
