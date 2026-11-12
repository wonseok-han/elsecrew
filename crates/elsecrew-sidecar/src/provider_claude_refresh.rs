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

pub(crate) fn refresh_if_expired(profile: &ClaudeProfileBinding) -> Result<(), SidecarError> {
    crate::provider_login_process::with_credentials_reserved(profile, || {
        let path = profile.revalidate()?;
        let hash = format!(
            "{:x}",
            Sha256::digest(path.to_string_lossy().nfc().collect::<String>().as_bytes())
        );
        let service = format!("Claude Code-credentials-{}", &hash[..8]);
        let mut original =
            match security_framework::passwords::get_generic_password(&service, "unknown") {
                Ok(bytes) => bytes,
                // No app keychain login yet: let the official auth status report idle.
                Err(e) if e.code() == -25300 => return Ok(()),
                Err(_) => return Err(failure()),
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
                    let mut current =
                        security_framework::passwords::get_generic_password(&service, "unknown")
                            .map_err(|_| failure())?;
                    let same = current == original;
                    current.fill(0);
                    if !same {
                        return Err(failure());
                    }
                    // Native Keychain update: no credential in argv, logs, files or renderer.
                    security_framework::passwords::set_generic_password(
                        &service,
                        "unknown",
                        &replacement,
                    )
                    .map_err(|_| failure())
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
