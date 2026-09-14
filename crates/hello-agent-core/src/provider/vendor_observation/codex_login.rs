//! ChatGPT browser-login exchange for an already initialized, isolated app-server.
//! No process/browser launch, credentials, model request or implicit auth reuse.
//! Owner reserves the request ID and validates the returned URL before opening it.
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::value::RawValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginState {
    AwaitingStart,
    WaitingForBrowser,
    VerificationRequired,
    Canceled,
    Failed,
}

/// Transient browser handoff. No Debug/Serialize: the URL contains OAuth state.
pub struct BrowserLogin {
    pub login_id: String,
    pub auth_url: String,
}

/// An exact login request, constructed only by the correlated login exchange.
/// Arbitrary provider methods cannot be supplied to the login transport.
/// ```compile_fail
/// use hello_agent_core::provider::vendor_observation::codex_login::LoginRequest;
/// let request = LoginRequest("{\"method\":\"thread/start\"}\n".to_owned());
/// ```
/// Requests are non-Clone and consumed by the transport, including failed sends.
/// ```compile_fail
/// use hello_agent_core::provider::vendor_observation::codex_login::{LoginExchange, LoginRequest};
/// fn send(_: LoginRequest) {}
/// let (_, request) = LoginExchange::begin(1).unwrap();
/// send(request);
/// send(request);
/// ```
pub struct LoginRequest(String);

impl LoginRequest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct LoginExchange {
    request_id: u64,
    login_id: Option<String>,
    state: LoginState,
}

fn error() -> ProviderError {
    ProviderError("invalid Codex login exchange".into())
}

impl LoginExchange {
    /// Caller must reserve a unique ID on the connection, initialize the server,
    /// and obtain the user's login action before sending this exact request.
    pub fn begin(request_id: u64) -> Result<(Self, LoginRequest), ProviderError> {
        if request_id == 0 || request_id > 9_007_199_254_740_991 {
            return Err(error());
        }
        Ok((
            Self {
                request_id,
                login_id: None,
                state: LoginState::AwaitingStart,
            },
            LoginRequest(format!(
                "{}\n",
                serde_json::json!({"id":request_id,"method":"account/login/start","params":{"type":"chatgpt"}})
            )),
        ))
    }

    pub fn state(&self) -> LoginState {
        self.state
    }

    pub fn accept_start(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<BrowserLogin, ProviderError> {
        let result = self.accept_start_inner(input);
        if result.is_err() {
            self.state = LoginState::Failed;
        }
        result
    }

    fn accept_start_inner(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<BrowserLogin, ProviderError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            id: u64,
            result: Box<RawValue>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Started {
            #[serde(rename = "type")]
            kind: String,
            login_id: String,
            auth_url: String,
        }
        if self.state != LoginState::AwaitingStart {
            return Err(error());
        }
        let reply: Reply = serde_json::from_slice(&input.bytes).map_err(|_| error())?;
        if reply.id != self.request_id {
            return Err(error());
        }
        let started: Started = serde_json::from_str(reply.result.get()).map_err(|_| error())?;
        if started.kind != "chatgpt"
            || started.login_id.is_empty()
            || started.login_id.len() > 128
            || started.login_id.chars().any(char::is_control)
            || started.auth_url.len() > 16 * 1024
            || started.auth_url.contains('\\')
            || started.auth_url.chars().any(char::is_whitespace)
            || started.auth_url.chars().any(char::is_control)
            || !(started.auth_url.starts_with("https://auth.openai.com/")
                || started.auth_url.starts_with("https://chatgpt.com/"))
        {
            return Err(error());
        }
        self.login_id = Some(started.login_id.clone());
        self.state = LoginState::WaitingForBrowser;
        Ok(BrowserLogin {
            login_id: started.login_id,
            auth_url: started.auth_url,
        })
    }

    /// Only dispatch account/login/completed here, not account/updated. Successful
    /// callback requires a fresh account/read check by the owner, not a ready flag.
    pub fn accept_completion(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<LoginState, ProviderError> {
        let result = self.accept_completion_inner(input);
        if result.is_err() {
            self.state = LoginState::Failed;
        }
        result
    }

    fn accept_completion_inner(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<LoginState, ProviderError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Notice {
            method: String,
            params: Box<RawValue>,
            #[serde(default, rename = "emittedAtMs")]
            emitted_at_ms: u64,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Completion {
            login_id: String,
            success: bool,
            error: Box<RawValue>,
            onboarding_entrypoint: Box<RawValue>,
        }
        if self.state != LoginState::WaitingForBrowser {
            return Err(error());
        }
        let notice: Notice = serde_json::from_slice(&input.bytes).map_err(|_| error())?;
        if notice.method != "account/login/completed"
            || notice.emitted_at_ms > 9_007_199_254_740_991
        {
            return Err(error());
        }
        let completion: Completion =
            serde_json::from_str(notice.params.get()).map_err(|_| error())?;
        if Some(&completion.login_id) != self.login_id.as_ref()
            || completion.onboarding_entrypoint.get() != "null"
        {
            return Err(error());
        }
        let detail: Option<String> =
            serde_json::from_str(completion.error.get()).map_err(|_| error())?;
        if completion.success && detail.is_some() {
            return Err(error());
        }
        self.state = if completion.success {
            LoginState::VerificationRequired
        } else {
            LoginState::Failed
        };
        Ok(self.state)
    }

    /// Local cancellation immediately revokes this attempt's eligibility. The owner
    /// must still deliver this request and confirm vendor cancellation/process exit.
    /// Cancellation before start response requires closing the owning transport.
    pub fn cancel(&mut self, request_id: u64) -> Result<LoginRequest, ProviderError> {
        if self.state != LoginState::WaitingForBrowser
            || request_id == 0
            || request_id == self.request_id
            || request_id > 9_007_199_254_740_991
        {
            return Err(error());
        }
        self.state = LoginState::Canceled;
        Ok(LoginRequest(format!(
            "{}\n",
            serde_json::json!({"id":request_id,"method":"account/login/cancel","params":{"loginId":self.login_id}})
        )))
    }

    /// Called on EOF, timeout or owner failure; never turns missing evidence into success.
    pub fn invalidate(&mut self) {
        self.state = LoginState::Failed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn obs(raw: &str) -> BoundedObservation {
        BoundedObservation::new(raw.as_bytes().to_vec()).unwrap()
    }
    fn reply() -> BoundedObservation {
        obs(
            r#"{"id":9,"result":{"type":"chatgpt","loginId":"attempt","authUrl":"https://auth.openai.com/authorize?state=SECRET"}}"#,
        )
    }
    fn done() -> BoundedObservation {
        obs(
            r#"{"method":"account/login/completed","emittedAtMs":1789360000000,"params":{"loginId":"attempt","success":true,"error":null,"onboardingEntrypoint":null}}"#,
        )
    }
    #[test]
    fn login_is_correlated_and_callback_is_not_ready() {
        let (mut exchange, request) = LoginExchange::begin(9).unwrap();
        let value: serde_json::Value = serde_json::from_str(request.as_str()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"id":9,"method":"account/login/start","params":{"type":"chatgpt"}})
        );
        let handoff = exchange.accept_start(&reply()).unwrap();
        assert_eq!(handoff.login_id, "attempt");
        assert!(handoff.auth_url.starts_with("https://auth.openai.com/"));
        assert_eq!(
            exchange.accept_completion(&done()).unwrap(),
            LoginState::VerificationRequired
        );
        exchange.invalidate();
        assert_eq!(exchange.state(), LoginState::Failed);
    }
    #[test]
    fn rejects_untrusted_urls_ids_duplicate_fields_and_wrong_modes() {
        for raw in [
            r#"{"id":8,"result":{"type":"chatgpt","loginId":"attempt","authUrl":"https://auth.openai.com/"}}"#,
            r#"{"id":9,"result":{"type":"chatgpt","loginId":"attempt","authUrl":"https://auth.openai.com.evil/SECRET"}}"#,
            r#"{"id":9,"result":{"type":"chatgpt","loginId":"attempt","authUrl":"https://auth.openai.com@evil/SECRET"}}"#,
            r#"{"id":9,"result":{"type":"chatgpt","loginId":"attempt","loginId":"other","authUrl":"https://auth.openai.com/"}}"#,
            r#"{"id":9,"result":{"type":"apiKey","apiKey":"SECRET"}}"#,
        ] {
            let (mut exchange, _) = LoginExchange::begin(9).unwrap();
            let failure = exchange.accept_start(&obs(raw)).err().unwrap();
            assert!(!format!("{failure:?}").contains("SECRET"));
            assert!(exchange.accept_start(&reply()).is_err());
        }
    }
    #[test]
    fn cancellation_and_cross_attempt_completion_cannot_succeed() {
        let (mut exchange, _) = LoginExchange::begin(9).unwrap();
        exchange.accept_start(&reply()).unwrap();
        let request: serde_json::Value =
            serde_json::from_str(exchange.cancel(10).unwrap().as_str()).unwrap();
        assert_eq!(
            request,
            serde_json::json!({"id":10,"method":"account/login/cancel","params":{"loginId":"attempt"}})
        );
        assert_eq!(exchange.state(), LoginState::Canceled);
        assert!(exchange.accept_completion(&done()).is_err());
        let (mut exchange, _) = LoginExchange::begin(9).unwrap();
        exchange.accept_start(&reply()).unwrap();
        assert!(exchange.accept_completion(&obs(r#"{"method":"account/login/completed","params":{"loginId":"other","success":true,"error":null,"onboardingEntrypoint":null}}"#)).is_err());
        assert!(exchange.accept_completion(&done()).is_err());
        let (mut exchange, _) = LoginExchange::begin(9).unwrap();
        assert!(exchange.accept_completion(&done()).is_err());
    }

    #[test]
    fn rejected_login_is_redacted_and_contradictory_success_is_invalid() {
        for success in [false, true] {
            let (mut exchange, _) = LoginExchange::begin(9).unwrap();
            exchange.accept_start(&reply()).unwrap();
            let message = serde_json::json!({"method":"account/login/completed","params":{"loginId":"attempt","success":success,"error":"SECRET","onboardingEntrypoint":null}}).to_string();
            let result = exchange.accept_completion(&obs(&message));
            if success {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap(), LoginState::Failed);
            }
            assert_eq!(exchange.state(), LoginState::Failed);
            assert!(exchange.accept_completion(&done()).is_err());
        }
    }

    #[test]
    fn current_schema_rejects_bad_metadata_and_onboarding() {
        let valid = String::from_utf8(done().bytes).unwrap();
        for raw in [
            valid.replace("1789360000000", "null"),
            valid.replace("1789360000000", "-1"),
            valid.replace("1789360000000", "9007199254740992"),
            valid.replace(
                "\"emittedAtMs\":1789360000000",
                "\"emittedAtMs\":1,\"emittedAtMs\":2",
            ),
            valid.replace(",\"onboardingEntrypoint\":null", ""),
            valid.replace(
                "\"onboardingEntrypoint\":null",
                "\"onboardingEntrypoint\":{}",
            ),
            valid.replace(
                "\"onboardingEntrypoint\":null",
                "\"onboardingEntrypoint\":null,\"onboardingEntrypoint\":null",
            ),
        ] {
            let (mut exchange, _) = LoginExchange::begin(9).unwrap();
            exchange.accept_start(&reply()).unwrap();
            assert!(exchange.accept_completion(&obs(&raw)).is_err());
        }
    }
}
