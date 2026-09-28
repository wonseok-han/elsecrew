//! One connection's ordered preflight observations. No launch authority or prompt API.
//! The process owner must separately enforce profile isolation, deadlines and cleanup.

use super::{
    match_model_catalog, match_thread, validate_profile, ExpectedPermission, ExpectedProfile,
};
use crate::provider::vendor_observation::codex_rpc::{ModelFreeExchange, ObservationKind};
use crate::provider::{BoundedObservation, ProviderError};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightState {
    Checking,
    ObservationsMatched,
    ThreadStarting,
    ThreadMatched,
    Closed,
    Failed,
}

/// Constructible only by the fixed model-free exchange, never by renderer input.
pub struct PreflightRequest(String);

impl PreflightRequest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Holds only the requested tuple and profile identity, never raw account/config replies.
pub struct PreflightExchange {
    rpc: ModelFreeExchange,
    model: String,
    effort: String,
    expected_home: String,
    permission: ExpectedPermission,
    state: PreflightState,
    started: bool,
    thread_cwd: Option<std::path::PathBuf>,
    thread_id: Option<String>,
    notified_thread_id: Option<String>,
}

fn invalid() -> ProviderError {
    ProviderError("Codex preflight connection did not match the requested profile".into())
}

impl PreflightExchange {
    pub fn new(profile: &ExpectedProfile<'_>, expected_home: &str) -> Result<Self, ProviderError> {
        Self::with_permission(profile, expected_home, ExpectedPermission::ReadOnly)
    }

    pub fn with_permission(
        profile: &ExpectedProfile<'_>,
        expected_home: &str,
        permission: ExpectedPermission,
    ) -> Result<Self, ProviderError> {
        validate_profile(profile)?;
        let path = std::path::Path::new(expected_home);
        if !path.is_absolute()
            || expected_home.len() > 16 * 1024
            || expected_home.chars().any(char::is_control)
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(invalid());
        }
        Ok(Self {
            rpc: ModelFreeExchange::with_config_layers(),
            model: profile.model.into(),
            effort: profile.reasoning_effort.into(),
            expected_home: expected_home.into(),
            permission,
            state: PreflightState::Checking,
            started: false,
            thread_cwd: None,
            thread_id: None,
            notified_thread_id: None,
        })
    }

    pub fn state(&self) -> PreflightState {
        self.state
    }

    pub fn is_fresh(&self) -> bool {
        !self.started && self.state == PreflightState::Checking
    }

    pub fn invalidate(&mut self) {
        self.state = PreflightState::Failed;
        self.thread_id = None;
        self.notified_thread_id = None;
    }

    /// A finished process cannot lend its observations to a new connection.
    pub fn close(&mut self) {
        if self.state != PreflightState::Failed {
            self.state = PreflightState::Closed;
        }
        self.thread_id = None;
        self.notified_thread_id = None;
    }

    pub fn next_request(&mut self) -> Result<Option<String>, ProviderError> {
        self.started = true;
        match self.state {
            PreflightState::Failed | PreflightState::Closed => Err(invalid()),
            PreflightState::ObservationsMatched => Ok(None),
            PreflightState::ThreadStarting | PreflightState::ThreadMatched => Err(invalid()),
            PreflightState::Checking => self.rpc.next_request(),
        }
    }

    /// Same connection only. The owner must hold the managed cwd and enforce isolation
    /// before sending this: thread creation may initialize configured integrations.
    pub fn start_read_only_thread(
        &mut self,
        cwd: &std::path::Path,
    ) -> Result<PreflightRequest, ProviderError> {
        if self.permission != ExpectedPermission::ReadOnly {
            return Err(invalid());
        }
        self.start_thread(cwd)
    }

    pub fn start_thread(
        &mut self,
        cwd: &std::path::Path,
    ) -> Result<PreflightRequest, ProviderError> {
        if self.state != PreflightState::ObservationsMatched {
            return Err(invalid());
        }
        let path = Self::validate_thread_cwd(cwd)?;
        let mut request = serde_json::json!({"id":5,"method":"thread/start","params":{
            "model":self.model,"modelProvider":"openai","cwd":path,
            "approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":self.permission.sandbox_mode(),
            "config":{"model_reasoning_effort":self.effort},"ephemeral":true,
            "baseInstructions":"","developerInstructions":""
        }});
        if self.permission == ExpectedPermission::WorkspaceWrite {
            request["params"]["config"]["sandbox_workspace_write"] = serde_json::json!({
                "writable_roots":[], "network_access":false,
                "exclude_tmpdir_env_var":true, "exclude_slash_tmp":true
            });
        }
        self.thread_cwd = Some(cwd.to_owned());
        self.state = PreflightState::ThreadStarting;
        Ok(PreflightRequest(format!("{request}\n")))
    }

    /// Syntax only; the runtime must separately admit the snapshot identity.
    pub fn validate_thread_cwd(cwd: &std::path::Path) -> Result<&str, ProviderError> {
        let path = cwd.to_str().ok_or_else(invalid)?;
        if !cwd.is_absolute()
            || path.len() > 16 * 1024
            || path.chars().any(char::is_control)
            || cwd
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(invalid());
        }
        Ok(path)
    }

    /// Correlated identity, not permission to execute a turn or proof of filesystem isolation.
    pub fn matched_thread_id(&self) -> Option<&str> {
        if self.state == PreflightState::ThreadMatched {
            self.thread_id.as_deref()
        } else {
            None
        }
    }

    /// Consume this connection's observations to prepare one text turn. This is
    /// not process/snapshot admission; the runtime must acquire that separately.
    pub fn into_text_turn(
        self,
        text: &str,
    ) -> Result<crate::provider::vendor_observation::codex_turn::TurnStart, ProviderError> {
        let thread = self.matched_thread_id().ok_or_else(invalid)?;
        crate::provider::vendor_observation::codex_turn::TurnStart::new(thread, text)
    }

    /// Split initialized and its following request into individual JSONL frames.
    pub fn next_frames(&mut self) -> Result<Vec<PreflightRequest>, ProviderError> {
        Ok(self
            .next_request()?
            .into_iter()
            .flat_map(|batch| {
                batch
                    .lines()
                    .map(|line| PreflightRequest(format!("{line}\n")))
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    pub fn accept(&mut self, input: &BoundedObservation) -> Result<PreflightState, ProviderError> {
        let result = self.accept_inner(input);
        if result.is_err() {
            self.invalidate();
        }
        result.map_err(|_| invalid())
    }

    fn match_thread_pair(&mut self) -> Result<PreflightState, ProviderError> {
        if let (Some(reply), Some(note)) = (&self.thread_id, &self.notified_thread_id) {
            if reply != note {
                return Err(invalid());
            }
            self.state = PreflightState::ThreadMatched;
        }
        Ok(self.state)
    }

    fn accept_inner(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<PreflightState, ProviderError> {
        if self.state == PreflightState::ThreadStarting {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Reply {
                id: u64,
                result: Box<serde_json::value::RawValue>,
            }
            #[derive(Deserialize)]
            struct Thread {
                id: String,
            }
            #[derive(Deserialize)]
            struct ResultWire {
                thread: Thread,
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Notification {
                method: String,
                params: ResultWire,
            }
            if input.json()?.get("method").is_some() {
                let note: Notification =
                    serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
                let id = note.params.thread.id;
                if note.method != "thread/started"
                    || self.notified_thread_id.is_some()
                    || id.is_empty()
                    || id.len() > 128
                    || id.chars().any(char::is_control)
                {
                    return Err(invalid());
                }
                self.notified_thread_id = Some(id);
                return self.match_thread_pair();
            }
            if self.thread_id.is_some() {
                return Err(invalid());
            }
            let reply: Reply = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
            if reply.id != 5 {
                return Err(invalid());
            }
            let observation = BoundedObservation::new(reply.result.get().as_bytes().to_vec())?;
            match_thread(
                &observation,
                &ExpectedProfile {
                    model: &self.model,
                    reasoning_effort: &self.effort,
                },
                self.thread_cwd.as_deref().ok_or_else(invalid)?,
                self.permission,
            )?;
            let result: ResultWire =
                serde_json::from_slice(&observation.bytes).map_err(|_| invalid())?;
            self.thread_id = Some(result.thread.id);
            return self.match_thread_pair();
        }
        if self.state != PreflightState::Checking {
            return Err(invalid());
        }
        let (kind, observation) = self.rpc.accept(input)?;
        let profile = ExpectedProfile {
            model: &self.model,
            reasoning_effort: &self.effort,
        };
        match kind {
            ObservationKind::Initialize => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Init {
                    codex_home: String,
                    platform_family: String,
                    platform_os: String,
                }
                let init: Init =
                    serde_json::from_slice(&observation.bytes).map_err(|_| invalid())?;
                if init.codex_home != self.expected_home
                    || init.platform_family != "unix"
                    || init.platform_os != "macos"
                {
                    return Err(invalid());
                }
            }
            ObservationKind::Config => super::layers::match_layers(
                &observation,
                &profile,
                &self.expected_home,
                self.permission,
            )?,
            ObservationKind::Account => {
                // This product uses its own ChatGPT login, not arbitrary API keys or tokens.
                #[derive(Deserialize)]
                struct Account {
                    #[serde(rename = "type")]
                    kind: String,
                }
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Auth {
                    account: Account,
                    requires_openai_auth: bool,
                }
                let auth: Auth =
                    serde_json::from_slice(&observation.bytes).map_err(|_| invalid())?;
                if auth.account.kind != "chatgpt" || !auth.requires_openai_auth {
                    return Err(invalid());
                }
            }
            ObservationKind::Models => {
                match_model_catalog(&observation, &profile)?;
                self.state = PreflightState::ObservationsMatched;
            }
        }
        Ok(self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn exchange() -> PreflightExchange {
        PreflightExchange::new(
            &ExpectedProfile {
                model: "test-model",
                reasoning_effort: "medium",
            },
            "/app/profile",
        )
        .unwrap()
    }
    fn results() -> Vec<Value> {
        let mut values = vec![
            json!({"userAgent":"test","codexHome":"/app/profile","platformFamily":"unix","platformOs":"macos"}),
            json!({"config":{"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only"}}),
            json!({"account":{"type":"chatgpt","email":"PRIVATE"},"requiresOpenaiAuth":true}),
            json!({"data":[{"id":"test-model","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}],"nextCursor":null}),
        ];
        values[1]["config"]["features"] = json!({"hooks":false});
        let config = values[1]["config"].clone();
        values[1]["layers"] = json!([
            {"name":{"type":"sessionFlags"},"config":config},
            {"name":{"type":"user","file":"/app/profile/config.toml"},"config":{"cli_auth_credentials_store":"file"}},
            {"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}
        ]);
        values
    }
    fn reply(index: usize, result: &Value) -> BoundedObservation {
        BoundedObservation::new(serde_json::to_vec(&json!({"id":index+1,"result":result})).unwrap())
            .unwrap()
    }

    fn write_results() -> Vec<Value> {
        let mut values = results();
        values[1]["config"]["sandbox_mode"] = json!("workspace-write");
        values[1]["config"]["sandbox_workspace_write"] = json!({
            "writable_roots":[], "network_access":false,
            "exclude_tmpdir_env_var":true, "exclude_slash_tmp":true
        });
        values[1]["layers"][0]["config"] = values[1]["config"].clone();
        values
    }

    fn write_exchange() -> PreflightExchange {
        PreflightExchange::with_permission(
            &ExpectedProfile {
                model: "test-model",
                reasoning_effort: "medium",
            },
            "/app/profile",
            ExpectedPermission::WorkspaceWrite,
        )
        .unwrap()
    }

    #[test]
    fn write_config_requires_exact_effective_and_flag_permissions() {
        for scope in ["/config", "/layers/0/config"] {
            for (field, replacement) in [
                ("sandbox_mode", json!("read-only")),
                ("sandbox_workspace_write", Value::Null),
                ("sandbox_workspace_write/network_access", json!(true)),
                ("sandbox_workspace_write/exclude_slash_tmp", json!(false)),
                (
                    "sandbox_workspace_write/exclude_tmpdir_env_var",
                    json!(false),
                ),
                (
                    "sandbox_workspace_write/writable_roots",
                    json!(["/outside"]),
                ),
            ] {
                let mut flow = write_exchange();
                let mut values = write_results();
                *values[1].pointer_mut(&format!("{scope}/{field}")).unwrap() = replacement;
                flow.next_frames().unwrap();
                flow.accept(&reply(0, &values[0])).unwrap();
                flow.next_frames().unwrap();
                assert!(
                    flow.accept(&reply(1, &values[1])).is_err(),
                    "{scope}/{field}"
                );
                assert_eq!(flow.state(), PreflightState::Failed);
                assert!(flow.next_frames().is_err());
            }
        }
    }

    #[test]
    fn write_thread_request_and_response_keep_the_same_permission() {
        for case in [
            "valid",
            "read-only",
            "network",
            "root",
            "tmp-env",
            "slash-tmp",
            "missing",
            "unknown",
            "duplicate",
        ] {
            let mut flow = write_exchange();
            for (index, value) in write_results().iter().enumerate() {
                flow.next_frames().unwrap();
                flow.accept(&reply(index, value)).unwrap();
            }
            let cwd = std::path::Path::new("/managed/snapshot");
            assert!(flow.start_read_only_thread(cwd).is_err());
            let request = flow.start_thread(cwd).unwrap();
            let request: Value = serde_json::from_str(request.as_str()).unwrap();
            assert_eq!(request["params"]["sandbox"], "workspace-write");
            assert_eq!(
                request["params"]["config"]["sandbox_workspace_write"],
                json!({
                    "writable_roots":[], "network_access":false,
                    "exclude_tmpdir_env_var":true, "exclude_slash_tmp":true
                })
            );
            let mut response = json!({"thread":{"id":"write-1"},"model":"test-model","modelProvider":"openai","cwd":"/managed/snapshot","instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"workspaceWrite","writableRoots":[],"networkAccess":false,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true},"reasoningEffort":"medium"});
            match case {
                "read-only" => {
                    response["sandbox"] = json!({"type":"readOnly","networkAccess":false})
                }
                "network" => response["sandbox"]["networkAccess"] = json!(true),
                "root" => response["sandbox"]["writableRoots"] = json!(["/outside"]),
                "tmp-env" => response["sandbox"]["excludeTmpdirEnvVar"] = json!(false),
                "slash-tmp" => response["sandbox"]["excludeSlashTmp"] = json!(false),
                "missing" => {
                    response["sandbox"]
                        .as_object_mut()
                        .unwrap()
                        .remove("excludeSlashTmp");
                }
                "unknown" => response["sandbox"]["extra"] = json!(true),
                _ => {}
            }
            let wire = reply(4, &response);
            let wire = if case == "duplicate" {
                BoundedObservation::new(
                    String::from_utf8(wire.bytes)
                        .unwrap()
                        .replace(
                            "\"networkAccess\":false",
                            "\"networkAccess\":true,\"networkAccess\":false",
                        )
                        .into_bytes(),
                )
                .unwrap()
            } else {
                wire
            };
            assert_eq!(flow.accept(&wire).is_ok(), case == "valid", "{case}");
            if case == "valid" {
                assert!(flow.matched_thread_id().is_none());
                flow.accept(
                    &BoundedObservation::new(
                        br#"{"method":"thread/started","params":{"thread":{"id":"write-1"}}}"#
                            .to_vec(),
                    )
                    .unwrap(),
                )
                .unwrap();
                let mut turn = flow.into_text_turn("edit the managed copy").unwrap();
                assert!(turn.take_request().unwrap().contains("write-1"));
            } else {
                assert_eq!(flow.state(), PreflightState::Failed);
                assert!(flow.into_text_turn("goal").is_err());
            }
        }
    }

    #[test]
    fn matched_requires_all_four_ordered_observations_and_never_sends_user_input() {
        let mut exchange = exchange();
        for (index, result) in results().iter().enumerate() {
            let request = exchange.next_request().unwrap().unwrap();
            if index == 1 {
                assert!(request.contains("\"includeLayers\":true"));
            }
            assert!(
                !request.contains("turn/")
                    && !request.contains("thread/")
                    && !request.contains("login/")
            );
            assert_eq!(exchange.state(), PreflightState::Checking);
            exchange.accept(&reply(index, result)).unwrap();
        }
        assert_eq!(exchange.state(), PreflightState::ObservationsMatched);
        assert!(exchange.next_request().unwrap().is_none());
        exchange.invalidate();
        assert!(exchange.next_request().is_err());
    }

    #[test]
    fn mismatch_at_each_stage_permanently_stops_followup_requests() {
        for (stage, field, value) in [
            (0, "codexHome", json!("/other")),
            (0, "platformOs", json!("windows")),
            (1, "config", json!({})),
            (1, "layers", Value::Null),
            (1, "layers", json!([])),
            (2, "account", Value::Null),
            (2, "account", json!({"type":"apiKey"})),
            (3, "nextCursor", json!("more")),
            (3, "data", json!([])),
        ] {
            let mut exchange = exchange();
            let mut values = results();
            values[stage][field] = value;
            for (index, result) in values.iter().enumerate().take(stage + 1) {
                exchange.next_request().unwrap();
                let accepted = exchange.accept(&reply(index, result));
                assert_eq!(accepted.is_err(), index == stage);
            }
            assert_eq!(exchange.state(), PreflightState::Failed);
            assert!(exchange.next_request().is_err());
        }
    }

    #[test]
    fn duplicate_private_fields_and_unsolicited_responses_are_not_recoverable() {
        let mut exchange = exchange();
        exchange.next_request().unwrap();
        let input = BoundedObservation::new(br#"{"id":1,"result":{"userAgent":"test","codexHome":"SECRET","codexHome":"/app/profile","platformFamily":"unix","platformOs":"macos"}}"#.to_vec()).unwrap();
        let error = exchange.accept(&input).unwrap_err();
        assert!(!error.to_string().contains("SECRET"));
        assert!(exchange.next_request().is_err());
        let mut another = super::tests::exchange();
        assert!(another.accept(&reply(0, &results()[0])).is_err());
        assert!(another.next_request().is_err());
    }

    #[test]
    fn turn_preparation_consumes_only_a_live_matched_thread() {
        for state in [
            PreflightState::Checking,
            PreflightState::ObservationsMatched,
            PreflightState::ThreadStarting,
            PreflightState::Failed,
            PreflightState::Closed,
        ] {
            let mut flow = exchange();
            flow.state = state;
            flow.thread_id = Some("thread-1".into());
            assert!(flow.into_text_turn("goal").is_err());
        }
        let mut flow = exchange();
        for (index, result) in results().iter().enumerate() {
            flow.next_frames().unwrap();
            flow.accept(&reply(index, result)).unwrap();
        }
        flow.start_read_only_thread(std::path::Path::new("/managed/snapshot"))
            .unwrap();
        let result = json!({"thread":{"id":"thread-1"},"model":"test-model","modelProvider":"openai","cwd":"/managed/snapshot","instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"});
        flow.accept(&reply(4, &result)).unwrap();
        flow.accept(
            &BoundedObservation::new(
                br#"{"method":"thread/started","params":{"thread":{"id":"thread-1"}}}"#.to_vec(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut turn = flow.into_text_turn("goal").unwrap();
        let request: Value = serde_json::from_str(&turn.take_request().unwrap()).unwrap();
        assert_eq!(request["params"]["threadId"], "thread-1");
    }

    #[test]
    fn thread_requires_preflight_and_exact_correlated_workspace_settings() {
        let cwd = std::path::Path::new("/managed/snapshot");
        assert!(exchange().start_read_only_thread(cwd).is_err());
        for mismatch in ["none", "id", "cwd", "model", "sandbox", "instructions"] {
            let mut flow = exchange();
            for (index, result) in results().iter().enumerate() {
                flow.next_frames().unwrap();
                flow.accept(&reply(index, result)).unwrap();
            }
            assert!(flow
                .start_read_only_thread(std::path::Path::new("/managed/../other"))
                .is_err());
            let request = flow.start_read_only_thread(cwd).unwrap();
            let request: Value = serde_json::from_str(request.as_str()).unwrap();
            assert_eq!(request["id"], 5);
            assert_eq!(request["method"], "thread/start");
            assert_eq!(request["params"]["sandbox"], "read-only");
            assert_eq!(
                request["params"]["config"]["model_reasoning_effort"],
                "medium"
            );
            assert_eq!(request["params"]["ephemeral"], true);
            assert!(flow.start_read_only_thread(cwd).is_err());
            assert_eq!(flow.matched_thread_id(), None);
            let mut result = json!({"thread":{"id":"thread-1"},"model":"test-model","modelProvider":"openai","cwd":"/managed/snapshot","instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"});
            match mismatch {
                "cwd" => result["cwd"] = json!("/other"),
                "model" => result["model"] = json!("other"),
                "sandbox" => result["sandbox"] = json!({"type":"workspaceWrite"}),
                "instructions" => result["instructionSources"] = json!(["/user/AGENTS.md"]),
                _ => {}
            }
            let accepted = flow.accept(&reply(if mismatch == "id" { 3 } else { 4 }, &result));
            if mismatch == "none" {
                assert_eq!(accepted.unwrap(), PreflightState::ThreadStarting);
                assert_eq!(flow.matched_thread_id(), None);
                let note = BoundedObservation::new(
                    br#"{"method":"thread/started","params":{"thread":{"id":"thread-1"}}}"#
                        .to_vec(),
                )
                .unwrap();
                assert_eq!(flow.accept(&note).unwrap(), PreflightState::ThreadMatched);
                assert_eq!(flow.matched_thread_id(), Some("thread-1"));
                flow.invalidate();
                assert_eq!(flow.matched_thread_id(), None);
            } else {
                assert!(accepted.is_err());
                assert_eq!(flow.state(), PreflightState::Failed);
                assert_eq!(flow.matched_thread_id(), None);
                assert!(flow.next_request().is_err());
            }
        }
    }

    #[test]
    fn thread_notification_is_bounded_correlated_and_never_authority_alone() {
        let result = json!({"thread":{"id":"thread-1"},"model":"test-model","modelProvider":"openai","cwd":"/managed/snapshot","instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"});
        for case in [
            "before",
            "after",
            "different",
            "duplicate",
            "request",
            "unknown",
            "empty",
        ] {
            let mut flow = exchange();
            for (index, result) in results().iter().enumerate() {
                flow.next_frames().unwrap();
                flow.accept(&reply(index, result)).unwrap();
            }
            flow.start_read_only_thread(std::path::Path::new("/managed/snapshot"))
                .unwrap();
            let mut note = json!({"method":"thread/started","params":{"thread":{"id":"thread-1"}}});
            match case {
                "different" => note["params"]["thread"]["id"] = json!("other"),
                "request" => note["id"] = json!(99),
                "unknown" => note["method"] = json!("thread/unknown"),
                "empty" => note["params"]["thread"]["id"] = json!(""),
                _ => {}
            }
            let note = BoundedObservation::new(note.to_string().into_bytes()).unwrap();
            if matches!(case, "request" | "unknown" | "empty") {
                assert!(flow.accept(&note).is_err());
            } else if case == "after" {
                assert_eq!(
                    flow.accept(&reply(4, &result)).unwrap(),
                    PreflightState::ThreadStarting
                );
                assert_eq!(flow.matched_thread_id(), None);
                assert_eq!(flow.accept(&note).unwrap(), PreflightState::ThreadMatched);
            } else {
                assert_eq!(flow.accept(&note).unwrap(), PreflightState::ThreadStarting);
                assert_eq!(flow.matched_thread_id(), None);
                if case == "duplicate" {
                    assert!(flow.accept(&note).is_err());
                } else if case == "different" {
                    assert!(flow.accept(&reply(4, &result)).is_err());
                } else {
                    assert_eq!(
                        flow.accept(&reply(4, &result)).unwrap(),
                        PreflightState::ThreadMatched
                    );
                }
            }
            if matches!(case, "before" | "after") {
                assert_eq!(flow.matched_thread_id(), Some("thread-1"));
            } else {
                assert_eq!(flow.state(), PreflightState::Failed);
                assert_eq!(flow.matched_thread_id(), None);
            }
        }
    }
}
