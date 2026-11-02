//! Actual-agent 실행 전에 공통으로 적용하는 fail-closed 경계.
//!
//! 공개 요청은 실행 파일·argv·cwd·env·prompt를 받지 않는다. 이 모듈의 `LaunchSpec`만
//! 검증된 adapter가 이미 고른 절대 실행 파일과 관리 snapshot을 token 단위로 보관한다.

use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

pub const MAX_AGENT_EVENT_BYTES: usize = 64 * 1024;
pub const MAX_AGENT_PROMPT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAdapterError(pub String);

impl fmt::Display for AgentAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AgentAdapterError {}

/// Rust 내부 전용 실행 명세. prompt는 `Debug`와 digest에서 의도적으로 제외한다.
pub struct LaunchSpec {
    executable: PathBuf,
    argv: Vec<OsString>,
    cwd: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    prompt: Vec<u8>,
    expected_handshake: &'static str,
    parser: &'static str,
}

impl fmt::Debug for LaunchSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LaunchSpec")
            .field("executable", &"<redacted:absolute>")
            .field("argvTokenCount", &self.argv.len())
            .field("cwd", &"<redacted:managed-snapshot>")
            .field("environmentKeys", &self.environment.len())
            .field("promptBytes", &"<redacted>")
            .field("expectedHandshake", &self.expected_handshake)
            .field("parser", &self.parser)
            .finish()
    }
}

impl LaunchSpec {
    /// Adapter 구현만 호출한다. 값은 shell을 거치지 않으며 상대 경로와 위험한 override를 거부한다.
    pub fn new_internal(
        executable: PathBuf,
        argv: Vec<OsString>,
        cwd: PathBuf,
        environment: BTreeMap<OsString, OsString>,
        prompt: Vec<u8>,
        expected_handshake: &'static str,
        parser: &'static str,
    ) -> Result<Self, AgentAdapterError> {
        if !executable.is_absolute() || !cwd.is_absolute() {
            return Err(AgentAdapterError(
                "agent executable and managed snapshot cwd must be absolute".into(),
            ));
        }
        if !cwd.is_dir() {
            return Err(AgentAdapterError(
                "managed snapshot cwd must exist and be a directory".into(),
            ));
        }
        if argv.is_empty()
            || argv.len() > 128
            || prompt.is_empty()
            || prompt.len() > MAX_AGENT_PROMPT_BYTES
        {
            return Err(AgentAdapterError(
                "agent argv and prompt must be nonempty and bounded".into(),
            ));
        }
        for token in &argv {
            if token.is_empty() || token.as_encoded_bytes().contains(&0) {
                return Err(AgentAdapterError(
                    "agent argv contains an invalid token".into(),
                ));
            }
            let text = token.to_string_lossy();
            if matches!(text.as_ref(), "--add-dir" | "--config" | "--config-file")
                || text.starts_with("--add-dir=")
                || text.starts_with("--config=")
            {
                return Err(AgentAdapterError(
                    "agent argv contains a forbidden workspace or config override".into(),
                ));
            }
        }
        if environment.len() > 16
            || environment.keys().any(|key| {
                !matches!(
                    key.to_str(),
                    Some("PATH" | "LANG" | "LC_ALL" | "TERM" | "NO_COLOR")
                )
            })
        {
            return Err(AgentAdapterError(
                "agent environment is not the minimal allowlist".into(),
            ));
        }
        if expected_handshake.trim().is_empty() || parser.trim().is_empty() {
            return Err(AgentAdapterError(
                "agent handshake and parser identities are required".into(),
            ));
        }
        Ok(Self {
            executable,
            argv,
            cwd,
            environment,
            prompt,
            expected_handshake,
            parser,
        })
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn argv(&self) -> &[OsString] {
        &self.argv
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn environment(&self) -> &BTreeMap<OsString, OsString> {
        &self.environment
    }

    pub fn prompt(&self) -> &[u8] {
        &self.prompt
    }

    pub fn expected_handshake(&self) -> &str {
        self.expected_handshake
    }

    pub fn parser(&self) -> &str {
        self.parser
    }

    /// Hash input for the owning runtime. The prompt is deliberately absent.
    pub fn redacted_digest_material(&self) -> Vec<u8> {
        let mut material = Vec::new();
        digest_os(&mut material, self.executable.as_os_str());
        for token in &self.argv {
            digest_os(&mut material, token);
        }
        digest_os(&mut material, self.cwd.as_os_str());
        for (key, value) in &self.environment {
            digest_os(&mut material, key);
            digest_os(&mut material, value);
        }
        material.extend_from_slice(self.expected_handshake.as_bytes());
        material.extend_from_slice(self.parser.as_bytes());
        // prompt bytes are secret and must not be derivable from persisted evidence.
        material
    }
}

fn digest_os(material: &mut Vec<u8>, value: &OsStr) {
    let bytes = value.as_encoded_bytes();
    material.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    material.extend_from_slice(bytes);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedVendorEvent {
    Public { kind: &'static str, text: String },
    Private,
}

/// Raw vendor JSON is returned nowhere. Only bounded public text survives normalization.
pub fn normalize_vendor_event(line: &[u8]) -> Result<NormalizedVendorEvent, AgentAdapterError> {
    if line.is_empty() || line.len() > MAX_AGENT_EVENT_BYTES {
        return Err(AgentAdapterError(
            "agent event is empty or oversized".into(),
        ));
    }
    let value: Value = serde_json::from_slice(line)
        .map_err(|_| AgentAdapterError("agent event is malformed JSON".into()))?;
    let object = value
        .as_object()
        .ok_or_else(|| AgentAdapterError("agent event must be an object".into()))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| AgentAdapterError("agent event type is missing".into()))?;
    match kind {
        "status" | "question" | "final" => {
            if object.len() != 2 {
                return Err(AgentAdapterError(
                    "public agent event contains unapproved fields".into(),
                ));
            }
            let text = object
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| AgentAdapterError("public agent event text is missing".into()))?;
            if text.trim().is_empty() || text.len() > 16 * 1024 {
                return Err(AgentAdapterError(
                    "public agent event text is empty or oversized".into(),
                ));
            }
            Ok(NormalizedVendorEvent::Public {
                kind: match kind {
                    "status" => "status",
                    "question" => "question",
                    _ => "final",
                },
                text: text.to_string(),
            })
        }
        "reasoning" | "tool" | "usage" => Ok(NormalizedVendorEvent::Private),
        _ => Err(AgentAdapterError("unknown agent event type".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn launch_spec_is_tokenized_redacted_and_rejects_overrides() {
        let root = std::env::temp_dir().join(format!("hello-agent-launch-{}", std::process::id()));
        fs::create_dir_all(&root).expect("temp cwd");
        let spec = LaunchSpec::new_internal(
            PathBuf::from(test_path!("/bin/echo")),
            vec![
                OsString::from("--fixture"),
                OsString::from("value; touch /tmp/no"),
            ],
            root.clone(),
            BTreeMap::new(),
            b"secret prompt".to_vec(),
            "fixture-ready",
            "fixture-jsonl-v1",
        )
        .expect("safe tokenized spec");
        let debug = format!("{spec:?}");
        assert!(!debug.contains("secret prompt"));
        assert!(
            !String::from_utf8_lossy(&spec.redacted_digest_material()).contains("secret prompt")
        );
        assert!(LaunchSpec::new_internal(
            PathBuf::from(test_path!("/bin/echo")),
            vec![
                OsString::from("--add-dir"),
                OsString::from(test_path!("/tmp"))
            ],
            root.clone(),
            BTreeMap::new(),
            b"prompt".to_vec(),
            "ready",
            "parser",
        )
        .is_err());
        let _ = fs::remove_dir(&root);
    }

    #[test]
    fn vendor_normalization_filters_private_data_and_fails_closed() {
        assert_eq!(
            normalize_vendor_event(br#"{"type":"reasoning","secret":"token-123"}"#)
                .expect("known private event"),
            NormalizedVendorEvent::Private
        );
        assert_eq!(
            normalize_vendor_event(br#"{"type":"status","text":"working"}"#).expect("public event"),
            NormalizedVendorEvent::Public {
                kind: "status",
                text: "working".into()
            }
        );
        assert!(
            normalize_vendor_event(br#"{"type":"status","text":"ok","raw":"secret"}"#).is_err()
        );
        assert!(normalize_vendor_event(b"not-json").is_err());
    }
}
