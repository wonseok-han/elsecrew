//! One fresh macOS connection, one explicit browser login. No models or threads.
//! Returned requests must be sent in order; transport failure must invalidate us.
use super::{error, BrowserLogin, LoginExchange, LoginRequest, LoginState};
use crate::provider::vendor_observation::{codex_rpc::ModelFreeExchange, AuthObservation, Vendor};
use crate::provider::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::value::RawValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Initializing,
    ReadyToLogin,
    Starting,
    WaitingForBrowser,
    Verifying,
    CredentialsObserved,
    CredentialsAbsent,
    Canceled,
    Failed,
}

/// No Debug: Browser carries transient OAuth state. CredentialsObserved is not
/// launch readiness: the owner must also confirm process cleanup and isolation.
pub enum LoginAction {
    Send(LoginRequest),
    Browser(BrowserLogin),
    AccountUpdated,
    CredentialsObserved,
    CredentialsAbsent,
}

pub struct LoginConnection {
    init: ModelFreeExchange,
    login: Option<LoginExchange>,
    expected_home: String,
    state: ConnectionState,
    frames: usize,
}

impl LoginConnection {
    /// Dedicated connection IDs: 1 initialize, 2 login, 3 account read, 4 cancel.
    pub fn begin(expected_home: &str) -> Result<(Self, LoginRequest), ProviderError> {
        if !expected_home.starts_with('/')
            || expected_home.len() > 16 * 1024
            || expected_home.chars().any(char::is_control)
        {
            return Err(error());
        }
        let mut init = ModelFreeExchange::default();
        let request = init.next_request()?.ok_or_else(error)?;
        let mut request: serde_json::Value = serde_json::from_str(&request).map_err(|_| error())?;
        request["params"]["capabilities"] = serde_json::json!({"experimentalApi":false,"requestAttestation":false,"optOutNotificationMethods":["remoteControl/status/changed"]});
        let request = format!("{request}\n");
        Ok((
            Self {
                init,
                login: None,
                expected_home: expected_home.into(),
                state: ConnectionState::Initializing,
                frames: 0,
            },
            LoginRequest(request),
        ))
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Only call after sending initialized, and after explicit user login action.
    pub fn start_login(&mut self) -> Result<LoginRequest, ProviderError> {
        if self.state != ConnectionState::ReadyToLogin {
            return Err(error());
        }
        let (login, request) = LoginExchange::begin(2)?;
        self.login = Some(login);
        self.state = ConnectionState::Starting;
        Ok(request)
    }

    /// Observe this profile without starting OAuth or a model. A missing account
    /// is normal only on this path, never after a successful browser completion.
    pub fn check_account(&mut self) -> Result<LoginRequest, ProviderError> {
        if self.state != ConnectionState::ReadyToLogin {
            return Err(error());
        }
        self.state = ConnectionState::Verifying;
        Ok(LoginRequest(
            "{\"id\":3,\"method\":\"account/read\",\"params\":{\"refreshToken\":false}}\n".into(),
        ))
    }

    pub fn accept(&mut self, input: &BoundedObservation) -> Result<LoginAction, ProviderError> {
        let result = self.accept_inner(input);
        if result.is_err() {
            self.invalidate();
        }
        result
    }

    fn accept_inner(&mut self, input: &BoundedObservation) -> Result<LoginAction, ProviderError> {
        self.frames += 1;
        if self.frames > 256 {
            return Err(error());
        }
        match self.state {
            ConnectionState::Initializing => {
                let (_, result) = self.init.accept(input)?;
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Init {
                    codex_home: String,
                    platform_family: String,
                    platform_os: String,
                }
                let init: Init = serde_json::from_slice(&result.bytes).map_err(|_| error())?;
                if init.codex_home != self.expected_home
                    || init.platform_family != "unix"
                    || init.platform_os != "macos"
                {
                    return Err(error());
                }
                self.state = ConnectionState::ReadyToLogin;
                Ok(LoginAction::Send(LoginRequest(
                    "{\"method\":\"initialized\",\"params\":{}}\n".into(),
                )))
            }
            ConnectionState::Starting => {
                let browser = self.login.as_mut().ok_or_else(error)?.accept_start(input)?;
                self.state = ConnectionState::WaitingForBrowser;
                Ok(LoginAction::Browser(browser))
            }
            ConnectionState::WaitingForBrowser | ConnectionState::Verifying => {
                #[derive(Deserialize)]
                struct Header {
                    method: Option<String>,
                }
                let header: Header = serde_json::from_slice(&input.bytes).map_err(|_| error())?;
                if header.method.as_deref() == Some("account/updated") {
                    validate_update(input)?;
                    return Ok(LoginAction::AccountUpdated);
                }
                if self.state == ConnectionState::WaitingForBrowser {
                    if self
                        .login
                        .as_mut()
                        .ok_or_else(error)?
                        .accept_completion(input)?
                        != LoginState::VerificationRequired
                    {
                        return Err(error());
                    }
                    self.state = ConnectionState::Verifying;
                    return Ok(LoginAction::Send(LoginRequest("{\"id\":3,\"method\":\"account/read\",\"params\":{\"refreshToken\":false}}\n".into())));
                }
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Reply {
                    id: u64,
                    result: Box<RawValue>,
                }
                let reply: Reply = serde_json::from_slice(&input.bytes).map_err(|_| error())?;
                if reply.id != 3 {
                    return Err(error());
                }
                let result = BoundedObservation::new(reply.result.get().as_bytes().to_vec())?;
                let observation = Vendor::Codex01491.observe_auth(&result, 0)?;
                if self.login.is_none() && observation == AuthObservation::NotAuthenticated {
                    self.state = ConnectionState::CredentialsAbsent;
                    return Ok(LoginAction::CredentialsAbsent);
                }
                if observation != AuthObservation::CredentialsPresent {
                    return Err(error());
                }
                #[derive(Deserialize)]
                struct Account {
                    #[serde(rename = "type")]
                    kind: String,
                }
                #[derive(Deserialize)]
                struct AccountResult {
                    account: Account,
                }
                let account: AccountResult =
                    serde_json::from_slice(&result.bytes).map_err(|_| error())?;
                if account.account.kind != "chatgpt" {
                    return Err(error());
                }
                self.state = ConnectionState::CredentialsObserved;
                Ok(LoginAction::CredentialsObserved)
            }
            _ => Err(error()),
        }
    }

    /// Local revocation is immediate; optional cancel delivery is not exit proof.
    pub fn cancel(&mut self) -> Result<Option<LoginRequest>, ProviderError> {
        if matches!(
            self.state,
            ConnectionState::Canceled | ConnectionState::Failed
        ) {
            return Err(error());
        }
        let request = if self.state == ConnectionState::WaitingForBrowser {
            Some(self.login.as_mut().ok_or_else(error)?.cancel(4)?)
        } else {
            None
        };
        self.state = ConnectionState::Canceled;
        Ok(request)
    }

    pub fn invalidate(&mut self) {
        self.state = ConnectionState::Failed;
        if let Some(login) = &mut self.login {
            login.invalidate();
        }
    }
}

fn validate_update(input: &BoundedObservation) -> Result<(), ProviderError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Params {
        auth_mode: Box<RawValue>,
        plan_type: Box<RawValue>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Notice {
        method: String,
        params: Params,
        #[serde(default, rename = "emittedAtMs")]
        emitted_at_ms: u64,
    }
    let notice: Notice = serde_json::from_slice(&input.bytes).map_err(|_| error())?;
    let mode: Option<String> =
        serde_json::from_str(notice.params.auth_mode.get()).map_err(|_| error())?;
    let plan: Option<String> =
        serde_json::from_str(notice.params.plan_type.get()).map_err(|_| error())?;
    if notice.method != "account/updated"
        || notice.emitted_at_ms > 9_007_199_254_740_991
        || !matches!(mode.as_deref(), None | Some("chatgpt"))
        || plan.is_some_and(|plan| plan.len() > 128 || plan.chars().any(char::is_control))
    {
        return Err(error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn obs(raw: &str) -> BoundedObservation {
        BoundedObservation::new(raw.as_bytes().to_vec()).unwrap()
    }
    const INIT: &str = r#"{"id":1,"result":{"userAgent":"test","codexHome":"/app/profile","platformFamily":"unix","platformOs":"macos"}}"#;
    const START: &str = r#"{"id":2,"result":{"type":"chatgpt","loginId":"attempt","authUrl":"https://auth.openai.com/authorize?state=SECRET"}}"#;
    const DONE: &str = r#"{"method":"account/login/completed","emittedAtMs":1,"params":{"loginId":"attempt","success":true,"error":null,"onboardingEntrypoint":null}}"#;
    const UPDATE: &str =
        r#"{"method":"account/updated","params":{"authMode":"chatgpt","planType":"plus"}}"#;
    const ACCOUNT: &str = r#"{"id":3,"result":{"account":{"type":"chatgpt","email":"PRIVATE"},"requiresOpenaiAuth":true}}"#;
    #[test]
    fn account_check_never_starts_login_and_absence_is_not_browser_success() {
        for (reply, present) in [
            (ACCOUNT, true),
            (
                r#"{"id":3,"result":{"account":null,"requiresOpenaiAuth":true}}"#,
                false,
            ),
        ] {
            let (mut flow, _) = LoginConnection::begin("/app/profile").unwrap();
            flow.accept(&obs(INIT)).unwrap();
            let request = flow.check_account().unwrap();
            assert_eq!(
                request.as_str(),
                "{\"id\":3,\"method\":\"account/read\",\"params\":{\"refreshToken\":false}}\n"
            );
            assert!(flow.start_login().is_err());
            let action = flow.accept(&obs(reply)).unwrap();
            assert!(matches!(
                (action, present),
                (LoginAction::CredentialsObserved, true) | (LoginAction::CredentialsAbsent, false)
            ));
            assert!(flow.accept(&obs(reply)).is_err());
        }
        let mut flow = waiting();
        flow.accept(&obs(DONE)).unwrap();
        assert!(flow
            .accept(&obs(
                r#"{"id":3,"result":{"account":null,"requiresOpenaiAuth":true}}"#
            ))
            .is_err());
    }
    fn waiting() -> LoginConnection {
        let (mut flow, _) = LoginConnection::begin("/app/profile").unwrap();
        assert!(matches!(
            flow.accept(&obs(INIT)).unwrap(),
            LoginAction::Send(_)
        ));
        flow.start_login().unwrap();
        assert!(matches!(
            flow.accept(&obs(START)).unwrap(),
            LoginAction::Browser(_)
        ));
        flow
    }

    #[test]
    fn handshake_login_verification_order_and_interleaved_update() {
        let (mut flow, init) = LoginConnection::begin("/app/profile").unwrap();
        assert!(init.as_str().contains("initialize"));
        assert!(flow.start_login().is_err());
        let LoginAction::Send(initialized) = flow.accept(&obs(INIT)).unwrap() else {
            panic!()
        };
        assert_eq!(
            initialized.as_str(),
            "{\"method\":\"initialized\",\"params\":{}}\n"
        );
        let start = flow.start_login().unwrap();
        assert!(start.as_str().contains("account/login/start"));
        assert!(flow.start_login().is_err());
        flow.accept(&obs(START)).unwrap();
        assert!(matches!(
            flow.accept(&obs(UPDATE)).unwrap(),
            LoginAction::AccountUpdated
        ));
        let LoginAction::Send(verify) = flow.accept(&obs(DONE)).unwrap() else {
            panic!()
        };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(verify.as_str()).unwrap(),
            serde_json::json!({"id":3,"method":"account/read","params":{"refreshToken":false}})
        );
        assert_eq!(flow.state(), ConnectionState::Verifying);
        flow.accept(&obs(UPDATE)).unwrap();
        assert!(matches!(
            flow.accept(&obs(ACCOUNT)).unwrap(),
            LoginAction::CredentialsObserved
        ));
    }

    #[test]
    fn wrong_home_wrong_reply_and_wrong_auth_mode_fail_closed() {
        let (mut flow, _) = LoginConnection::begin("/app/other").unwrap();
        assert!(flow.accept(&obs(INIT)).is_err());
        for raw in [
            ACCOUNT.replace("\"id\":3", "\"id\":2"),
            ACCOUNT.replace("chatgpt", "apiKey"),
            ACCOUNT.replace("true", "false"),
            ACCOUNT.replace(
                "\"type\":\"chatgpt\"",
                "\"type\":\"chatgpt\",\"type\":\"chatgpt\"",
            ),
        ] {
            let mut flow = waiting();
            flow.accept(&obs(DONE)).unwrap();
            assert!(flow.accept(&obs(&raw)).is_err());
            assert_eq!(flow.state(), ConnectionState::Failed);
            assert!(flow.accept(&obs(ACCOUNT)).is_err());
        }
    }

    #[test]
    fn cancellation_late_success_and_noise_budget() {
        let mut flow = waiting();
        let cancel = flow.cancel().unwrap().unwrap();
        assert!(cancel.as_str().contains("account/login/cancel"));
        assert!(flow.accept(&obs(DONE)).is_err());
        let mut flow = waiting();
        for _ in 0..254 {
            flow.accept(&obs(UPDATE)).unwrap();
        }
        assert!(flow.accept(&obs(UPDATE)).is_err());
        let mut flow = waiting();
        assert!(flow
            .accept(&obs(&UPDATE.replace("account/updated", "thread/started")))
            .is_err());
    }
}
