//! Model-free renewal of this app's Claude OAuth credentials only.
//! Endpoint/client and response shape verified against official Claude Code 2.1.322.
use crate::{
    provider_claude_profile::ClaudeProfileBinding, provider_profile::LoginProfile, SidecarError,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
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
    let bytes = serde_json::to_vec(&value).map_err(|_| failure())?;
    if bytes.len() > MAX_BYTES {
        return Err(failure());
    }
    Ok(Some(bytes))
}

/// Reads the app-only Claude login through macOS's own `security` tool, the
/// same tool the official Claude Code uses to create and update this item.
/// The item's allow-list names only `/usr/bin/security` and its partition list
/// holds `apple-tool:`. A direct Keychain call from this sidecar is checked
/// against its own signature instead: without an Apple team ID that is the
/// binary's hash (`cdhash:`), so every rebuild was a new program and asked
/// again. `None` when the item does not exist. The secret travels on a private
/// pipe, never in argv.
fn read_app_credentials(service: &str) -> Result<Option<Vec<u8>>, SidecarError> {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-a", "unknown", "-w"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|_| failure())?;
    let mut bytes = output.stdout;
    match output.status.code() {
        Some(0) => {}
        // errSecItemNotFound as the tool reports it.
        Some(44) => { bytes.fill(0); return Ok(None); }
        _ => { bytes.fill(0); return Err(failure()); }
    }
    // `-w` prints the secret followed by one newline.
    if bytes.last() == Some(&b'\n') { bytes.pop(); }
    if bytes.is_empty() || bytes.len() > MAX_BYTES { bytes.fill(0); return Err(failure()); }
    Ok(Some(bytes))
}

/// The longest command line sent to `security -i`. The tool reads each line
/// into a 4096-byte buffer (4095 characters with the newline measured on
/// macOS 26), and a longer line is split: the rest runs as a separate command
/// whose "unknown command" error repeats it. The official Claude Code uses
/// the same bound.
const SECURITY_LINE_MAX: usize = 4032;

/// One `security -i` line that replaces the item's data, or `None` when the
/// service cannot be quoted safely or the line would be too long. The secret
/// is hex (`-X`), so it needs no quoting and cannot end the command early.
/// The caller zeroes the returned bytes.
fn security_update_line(
    service: &str,
    account: &str,
    secret: &[u8],
    keychain: Option<&str>,
) -> Option<Vec<u8>> {
    let quotable = |text: &str| !text.is_empty() && text.bytes().all(|b| (0x20..0x7f).contains(&b) && b != b'"' && b != b'\\');
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
        + 5 // `" -X `
        + 2 * secret.len()
        + keychain.map_or(0, |k| k.len() + 3)
        + 1;
    if length > SECURITY_LINE_MAX {
        return None;
    }
    let mut line = Vec::with_capacity(length);
    line.extend_from_slice(PREFIX);
    line.extend_from_slice(account.as_bytes());
    line.extend_from_slice(b"\" -s \"");
    line.extend_from_slice(service.as_bytes());
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

/// `security` in interactive mode: the command, secret included, arrives on
/// stdin, so argv is only `-i`. Output is discarded so an error can never
/// carry the line anywhere.
fn security_interactive() -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/security");
    command
        .arg("-i")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command
}

fn run_security_line(line: &[u8]) -> Result<(), SidecarError> {
    use std::io::Write;
    let mut child = security_interactive().spawn().map_err(|_| failure())?;
    // Dropping stdin ends the session after this one command.
    let wrote = child
        .stdin
        .take()
        .ok_or_else(failure)
        .and_then(|mut stdin| stdin.write_all(line).map_err(|_| failure()));
    let status = child.wait().map_err(|_| failure())?;
    wrote?;
    if status.success() {
        Ok(())
    } else {
        Err(failure())
    }
}

/// Replaces the app-only Claude login through `/usr/bin/security`, as Claude
/// Code itself does, so the item keeps its allow-list and partition list and
/// a rebuilt sidecar is never asked about. Only a login too long for one
/// `security -i` line (about 1.9 KB; a real one is about 600 bytes) falls back
/// to the native call, which may ask once for this build. Never argv.
fn write_app_credentials(service: &str, account: &str, secret: &[u8]) -> Result<(), SidecarError> {
    match security_update_line(service, account, secret, None) {
        Some(mut line) => {
            let result = run_security_line(&line);
            line.fill(0);
            result
        }
        None => security_framework::passwords::set_generic_password(service, account, secret)
            .map_err(|_| failure()),
    }
}

pub(crate) fn refresh_if_expired(profile: &ClaudeProfileBinding) -> Result<(), SidecarError> {
    crate::provider_login_process::with_credentials_reserved(profile, || {
        let path = profile.revalidate()?;
        let hash = format!(
            "{:x}",
            Sha256::digest(path.to_string_lossy().nfc().collect::<String>().as_bytes())
        );
        let service = format!("Claude Code-credentials-{}", &hash[..8]);
        let mut original = match read_app_credentials(&service)? {
            Some(bytes) => bytes,
            // No app keychain login yet: let the official auth status report idle.
            None => return Ok(()),
        };
        let result = (|| {
            let replacement = renewed(&original, now()?, |body| {
                let client = reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .no_proxy()
                    .timeout(Duration::from_secs(20))
                    .build()
                    .map_err(|_| failure())?;
                let response = client
                    .post(TOKEN_URL)
                    .json(&body)
                    .send()
                    .map_err(|_| failure())?;
                if response.status() != reqwest::StatusCode::OK {
                    return Err(failure());
                }
                let mut bytes = Vec::new();
                response
                    .take(MAX_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| failure())?;
                let result = if bytes.len() > MAX_BYTES {
                    Err(failure())
                } else {
                    serde_json::from_slice(&bytes).map_err(|_| failure())
                };
                bytes.fill(0);
                result
            })?;
            if let Some(mut replacement) = replacement {
                let saved = (|| {
                    if profile.revalidate()? != path {
                        return Err(failure());
                    }
                    let mut current = read_app_credentials(&service)?.ok_or_else(failure)?;
                    let same = current == original;
                    current.fill(0);
                    if !same {
                        return Err(failure());
                    }
                    // No credential in argv, logs, files or renderer.
                    write_app_credentials(&service, "unknown", &replacement)?;
                    // The tool's exit status is all it reports: confirm the store.
                    let mut stored = read_app_credentials(&service)?.ok_or_else(failure)?;
                    let written = stored == replacement;
                    stored.fill(0);
                    if written {
                        Ok(())
                    } else {
                        Err(failure())
                    }
                })();
                replacement.fill(0);
                saved?;
            }
            Ok(())
        })();
        original.fill(0);
        result
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
        let line = security_update_line("Claude Code-credentials-0123abcd", "unknown", secret, None).unwrap();
        let text = String::from_utf8(line.clone()).unwrap();
        let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            text,
            format!("add-generic-password -U -a \"unknown\" -s \"Claude Code-credentials-0123abcd\" -X {hex}\n")
        );
        assert_eq!(text.matches('\n').count(), 1);
        assert!(!text.contains("accessToken"));
        // A real login (about 600 bytes) fits; one past the bound does not.
        let fits = (SECURITY_LINE_MAX - 1 - "add-generic-password -U -a \"unknown\" -s \"Claude Code-credentials-0123abcd\" -X ".len()) / 2;
        assert!(fits > 1900);
        let long = vec![b'x'; fits];
        assert_eq!(security_update_line("Claude Code-credentials-0123abcd", "unknown", &long, None).unwrap().len(), SECURITY_LINE_MAX - 1);
        assert!(security_update_line("Claude Code-credentials-0123abcd", "unknown", &[b'x'; 2000], None).is_none());
        for service in ["", "bad\"quote", "bad\\slash", "new\nline"] {
            assert!(security_update_line(service, "unknown", secret, None).is_none());
            assert!(security_update_line("ok", service, secret, None).is_none());
        }
        assert!(security_update_line("ok", "unknown", b"", None).is_none());
        assert!(security_update_line("ok", "unknown", secret, Some("/tmp/a\"b")).is_none());
    }

    /// A throwaway keychain file, never the login keychain.
    #[cfg(target_os = "macos")]
    struct TempKeychain(std::path::PathBuf);
    #[cfg(target_os = "macos")]
    impl Drop for TempKeychain {
        fn drop(&mut self) {
            let _ = std::process::Command::new("/usr/bin/security")
                .arg("delete-keychain")
                .arg(&self.0)
                .output();
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn security_update_keeps_the_secret_out_of_argv_and_the_item_acl_unchanged() {
        use std::{io::Write, process::Command};
        let security = |args: &[&str]| Command::new("/usr/bin/security").args(args).output().unwrap();
        let dir = std::env::temp_dir().join(format!("elsecrew-kc-{}-{}", std::process::id(), now().unwrap()));
        std::fs::create_dir_all(&dir).unwrap();
        let keychain = TempKeychain(dir.join("test.keychain-db"));
        let path = keychain.0.to_str().unwrap().to_owned();
        assert!(security(&["create-keychain", "-p", "elsecrew-test", &path]).status.success());
        assert!(security(&["unlock-keychain", "-p", "elsecrew-test", &path]).status.success());
        let service = "Elsecrew argv probe";
        // Created by the tool, as Claude Code creates the real item.
        assert!(security(&["add-generic-password", "-a", "unknown", "-s", service, "-w", "initial-value", &path]).status.success());
        let acl = || {
            let dump = String::from_utf8(security(&["dump-keychain", "-a", &path]).stdout).unwrap();
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
        let before = acl();
        assert!(before.iter().any(|l| l.contains("/usr/bin/security")));

        let marker = format!("argv-probe-{}-secret", now().unwrap());
        let secret = format!(r#"{{"claudeAiOauth":{{"accessToken":"{marker}"}}}}"#);
        let hex: String = secret.bytes().map(|b| format!("{b:02x}")).collect();
        let mut line = security_update_line(service, "unknown", secret.as_bytes(), Some(&path)).unwrap();
        let mut child = security_interactive().spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&line).unwrap();
        stdin.flush().unwrap();
        line.fill(0);
        // While the tool holds the secret, no process lists it in its arguments.
        let listing = Command::new("/bin/ps").args(["-axww", "-o", "pid=,args="]).output().unwrap();
        let listing = String::from_utf8_lossy(&listing.stdout);
        assert!(listing.lines().any(|l| l.ends_with("/usr/bin/security -i")));
        assert!(!listing.contains(&marker) && !listing.contains(&hex));
        drop(stdin);
        assert!(child.wait().unwrap().success());

        let stored = security(&["find-generic-password", "-a", "unknown", "-s", service, "-w", &path]);
        assert_eq!(String::from_utf8(stored.stdout).unwrap().trim_end(), secret);
        // Updating in place keeps the tool-only allow-list (and so its partition).
        assert_eq!(acl(), before);
    }

    /// Manual probe in the login keychain (user-approved 2026-11-26), where
    /// items carry a partition list. `ELSECREW_KEYCHAIN_PROBE_ACCOUNT` names a
    /// dummy item the operator created with `/usr/bin/security`, as Claude Code
    /// creates the real one. Rebuilding with another `ELSECREW_PROBE_BUILD`
    /// gives this binary a new hash; the update must still raise no prompt and
    /// leave the partition list as `apple-tool:`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "writes a dummy item in the login keychain; run by hand"]
    fn login_keychain_probe_updates_without_a_new_partition() {
        let Ok(account) = std::env::var("ELSECREW_KEYCHAIN_PROBE_ACCOUNT") else {
            return;
        };
        let value = format!("dummy-{}-{}", option_env!("ELSECREW_PROBE_BUILD").unwrap_or("0"), now().unwrap());
        write_app_credentials("Elsecrew keychain-fix probe", &account, value.as_bytes()).unwrap();
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
