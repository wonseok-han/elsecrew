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
    ConfigurationMatched,
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
    configuration_only: bool,
    account_note_seen: bool,
    named_cwd: Option<std::path::PathBuf>,
    isolation_cwd: Option<std::path::PathBuf>,
    isolation_warning: Option<super::super::codex_isolation_warning::IsolationWarning>,
    configuration_failure: Option<&'static str>,
    display_models: Option<Vec<super::CatalogModel>>,
}

fn invalid() -> ProviderError {
    ProviderError("Codex preflight connection did not match the requested profile".into())
}

impl PreflightExchange {
    /// Native launchers bind the required startup overrides to their actual cwd.
    /// Must be set before exchanging any frames; absence remains fixture/legacy
    /// observation compatibility, never an assertion of isolated startup.
    pub fn require_startup_isolation(&mut self, cwd: &std::path::Path) -> Result<(), ProviderError> {
        Self::validate_thread_cwd(cwd)?;
        if self.started || self.state != PreflightState::Checking {
            return Err(invalid());
        }
        self.isolation_cwd = Some(cwd.into());
        Ok(())
    }
    /// Inspect initialization and configuration only. This cannot establish
    /// account/model readiness or be upgraded to thread/turn authority.
    pub fn configuration_only(
        profile: &ExpectedProfile<'_>,
        expected_home: &str,
    ) -> Result<Self, ProviderError> {
        let mut exchange = Self::new(profile, expected_home)?;
        exchange.configuration_only = true;
        Ok(exchange)
    }

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
            configuration_only: false,
            account_note_seen: false,
            named_cwd: None,
            isolation_cwd: None,
            isolation_warning: None,
            configuration_failure: None,
            display_models: None,
        })
    }

    /// Named policy preflight on this exact connection and snapshot path.
    /// The caller still owns executable/profile/snapshot admission and cleanup.
    pub fn with_named_permission(
        profile: &ExpectedProfile<'_>,
        expected_home: &str,
        cwd: &std::path::Path,
        permission: ExpectedPermission,
    ) -> Result<Self, ProviderError> {
        let mut flow = Self::with_permission(profile, expected_home, permission)?;
        let path = Self::validate_thread_cwd(cwd)?;
        let home = std::path::Path::new(expected_home);
        if cwd.starts_with(home)
            || home.starts_with(cwd)
            || [path, expected_home]
                .iter()
                .any(|value| value.contains(['*', '?', '[', ']', '{', '}']))
        {
            return Err(invalid());
        }
        flow.rpc = ModelFreeExchange::with_named_permission_config(cwd)?;
        flow.named_cwd = Some(cwd.into());
        Ok(flow)
    }

    pub fn state(&self) -> PreflightState {
        self.state
    }

    /// Display-only alternatives from a validated complete model/list reply.
    /// Available even when the selected model was rejected; does not advance
    /// the exchange, change the selected tuple, or grant start authority.
    pub fn display_models(&self) -> Option<&[super::CatalogModel]> {
        self.display_models.as_deref()
    }

    pub fn diagnostic_stage(&self) -> &'static str {
        if let Some(reason) = self.configuration_failure {
            return reason;
        }
        match self.state {
            PreflightState::Checking => self.rpc.diagnostic_stage(),
            PreflightState::ThreadStarting | PreflightState::ThreadMatched => "thread/start",
            _ => "exchange state",
        }
    }

    /// An account notification is not the correlated RPC response. Keep reading
    /// under the same owner/deadline without sending another request.
    pub fn awaiting_response(&self) -> bool {
        self.state == PreflightState::Checking && self.rpc.awaiting_reply()
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
            PreflightState::ConfigurationMatched | PreflightState::ObservationsMatched => Ok(None),
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
        if self
            .named_cwd
            .as_deref()
            .is_some_and(|expected| expected != cwd)
        {
            return Err(invalid());
        }
        let mut request = serde_json::json!({"id":5,"method":"thread/start","params":{
            "model":self.model,"modelProvider":"openai","cwd":path,
            "approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":self.permission.sandbox_mode(),
            "config":{"model_reasoning_effort":self.effort},"ephemeral":true,
            "baseInstructions":"","developerInstructions":""
        }});
        if self.named_cwd.is_some() {
            request["params"]
                .as_object_mut()
                .ok_or_else(invalid)?
                .remove("sandbox");
            request["params"]["permissions"] = serde_json::json!("hello-agent-snapshot");
            request["params"]["runtimeWorkspaceRoots"] = serde_json::json!([path]);
        } else if self.permission == ExpectedPermission::WorkspaceWrite {
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
                #[serde(default, rename = "emittedAtMs")]
                emitted_at_ms: u64,
            }
            if input.json()?.get("method").is_some() {
                let note: Notification =
                    serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
                let id = note.params.thread.id;
                if note.method != "thread/started"
                    || note.emitted_at_ms > 9_007_199_254_740_991
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
            if self.named_cwd.is_some() {
                super::match_named_thread(
                    &observation,
                    &ExpectedProfile {
                        model: &self.model,
                        reasoning_effort: &self.effort,
                    },
                    self.thread_cwd.as_deref().ok_or_else(invalid)?,
                )?;
            } else {
                match_thread(
                    &observation,
                    &ExpectedProfile {
                        model: &self.model,
                        reasoning_effort: &self.effort,
                    },
                    self.thread_cwd.as_deref().ok_or_else(invalid)?,
                    self.permission,
                )?;
            }
            let result: ResultWire =
                serde_json::from_slice(&observation.bytes).map_err(|_| invalid())?;
            self.thread_id = Some(result.thread.id);
            return self.match_thread_pair();
        }
        if self.state != PreflightState::Checking {
            return Err(invalid());
        }
        if input.json()?.get("method").is_some() {
            if input.json()?["method"] == "configWarning" {
                if !self.rpc.awaiting_configuration_reply() || self.isolation_warning.is_some() {
                    return Err(invalid());
                }
                let cwd = self.isolation_cwd.as_deref().ok_or_else(invalid)?;
                self.isolation_warning = Some(super::super::codex_isolation_warning::IsolationWarning::parse(
                    input, std::path::Path::new(&self.expected_home), cwd,
                ).map_err(|_| invalid())?);
                return Ok(self.state);
            }
            self.configuration_failure = Some(match input.json()?["method"].as_str() {
                Some("account/updated") => "account notification phase or envelope",
                Some("configWarning") => "configuration warning notification",
                _ => "unexpected preflight notification",
            });
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields, rename_all = "camelCase")]
            struct Params {
                auth_mode: String,
                plan_type: Option<String>,
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Note {
                method: String,
                params: Params,
                #[serde(default, rename = "emittedAtMs")]
                emitted_at_ms: u64,
            }
            let note: Note = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
            // This asynchronous note may precede config/read's response after
            // initialized. It never advances RPC order or proves account state.
            if !self.rpc.awaiting_initialized_reply()
                || self.account_note_seen
                || note.method != "account/updated"
                || note.params.auth_mode != "chatgpt"
                || note.emitted_at_ms > 9_007_199_254_740_991
                || note.params.plan_type.as_ref().is_some_and(|value| {
                    value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
                })
            {
                return Err(invalid());
            }
            self.account_note_seen = true;
            self.configuration_failure = None;
            return Ok(self.state);
        }
        let (kind, observation) = self.rpc.accept(input).map_err(|error| {
            self.configuration_failure = Some(if input.json().ok().is_some_and(|v| v.get("error").is_some()) {
                "provider RPC error response"
            } else { "RPC response envelope" });
            error
        })?;
        let profile = ExpectedProfile {
            model: &self.model,
            reasoning_effort: &self.effort,
        };
        match kind {
            ObservationKind::PermissionProfiles => {
                if self.named_cwd.is_none() {
                    return Err(invalid());
                }
                super::match_named_permission_profile(&observation, "hello-agent-snapshot")?;
                self.state = PreflightState::ObservationsMatched;
            }
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
            ObservationKind::Config => {
                let observation = if let Some(cwd) = &self.isolation_cwd {
                    let mut value = observation.json()?;
                    super::super::codex_profile_config::validate_disabled_project_layers(
                        &value, std::path::Path::new(&self.expected_home), cwd,
                    ).map_err(|_| invalid())?;
                    if self.isolation_warning.as_ref().is_some_and(|warning| !warning.matches_configuration(&value)) {
                        return Err(invalid());
                    }
                    // Only validated, disabled launch-ancestor project layers
                    // are excluded. All remaining layers still pass the strict
                    // existing source/config checks below (including managed).
                    value["layers"].as_array_mut().ok_or_else(invalid)?
                        .retain(|layer| layer["name"]["type"] != "project");
                    BoundedObservation::new(serde_json::to_vec(&value).map_err(|_| invalid())?)?
                } else { observation };
                if let Some(cwd) = &self.named_cwd {
                    super::named_config::match_configuration(
                        &observation,
                        std::path::Path::new(&self.expected_home),
                        cwd,
                        &profile,
                        self.permission,
                    )
                    .map_err(|reason| {
                        self.configuration_failure = Some(reason);
                        invalid()
                    })?;
                } else {
                    super::layers::match_layers(
                        &observation,
                        &profile,
                        &self.expected_home,
                        self.permission,
                    )?;
                }
                if self.configuration_only {
                    self.state = PreflightState::ConfigurationMatched;
                }
            }
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
                self.display_models = super::read_model_catalog(&observation).ok();
                match_model_catalog(&observation, &profile).map_err(|error| {
                    self.configuration_failure = Some("model catalog schema or effort");
                    if let Ok(value) = observation.json() {
                        if value.get("nextCursor").is_none() || !value["nextCursor"].is_null() {
                            self.configuration_failure = Some("model catalog pagination");
                        } else if value["data"].as_array().is_some_and(|models|
                            !models.iter().any(|model| model["model"] == profile.model)) {
                            self.configuration_failure = Some("selected model absent from catalog");
                        }
                    }
                    error
                })?;
            }
            ObservationKind::Requirements => {
                // Managed policies are not supported by this isolated candidate.
                // Null is an explicit report, not proof that inaccessible MDM
                // policy was successfully loaded by the vendor.
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Requirements {
                    requirements: Box<serde_json::value::RawValue>,
                    // Capability metadata added by newer servers. We request
                    // no speed override and still reject all managed requirements.
                    #[serde(rename = "supportsIndependentSpeedModes")]
                    _supports_independent_speed_modes: Option<bool>,
                }
                let result: Requirements =
                    serde_json::from_slice(&observation.bytes).map_err(|_| {
                        self.configuration_failure = Some("requirements response shape");
                        invalid()
                    })?;
                if result.requirements.get() != "null" {
                    self.configuration_failure = Some(if result.requirements.get().trim_start().starts_with('{') {
                        "managed requirements present"
                    } else {
                        "requirements value shape"
                    });
                    return Err(invalid());
                }
                if self.named_cwd.is_none() {
                    self.state = PreflightState::ObservationsMatched;
                }
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
            test_path!("/app/profile"),
        )
        .unwrap()
    }
    fn results() -> Vec<Value> {
        let mut values = vec![
            json!({"userAgent":"test","codexHome":test_path!("/app/profile"),"platformFamily":"unix","platformOs":"macos"}),
            json!({"config":{"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only"}}),
            json!({"account":{"type":"chatgpt","email":"PRIVATE"},"requiresOpenaiAuth":true}),
            json!({"data":[{"id":"test-model","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}],"nextCursor":null}),
            json!({"requirements":null}),
        ];
        values[1]["config"]["features"] = json!({"hooks":false,"apps":false,"plugins":false});
        let config = values[1]["config"].clone();
        values[1]["layers"] = json!([
            {"name":{"type":"sessionFlags"},"config":config},
            {"name":{"type":"user","file":test_path!("/app/profile/config.toml")},"config":{"cli_auth_credentials_store":"file"}},
            {"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}
        ]);
        values
    }
    fn reply(index: usize, result: &Value) -> BoundedObservation {
        BoundedObservation::new(
            serde_json::to_vec(
                &json!({"id":if index == 4 { 50 } else { index+1 },"result":result}),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn thread_reply(result: &Value) -> BoundedObservation {
        BoundedObservation::new(serde_json::to_vec(&json!({"id":5,"result":result})).unwrap())
            .unwrap()
    }

    #[test]
    fn isolated_disabled_project_layers_keep_other_source_checks() {
        let home = std::path::Path::new(test_path!("/app/profile"));
        let cwd = std::path::Path::new(test_path!("/app/workspace"));
        let config = super::super::super::codex_profile_config::ProfileConfig::parse(
            b"cli_auth_credentials_store='file'\n",
        )
        .unwrap();
        let projects = config.isolated_projects(&[home, cwd]).unwrap();
        for case in [
            "disabled",
            "enabled",
            "legacy",
            "managed",
            "system",
            "duplicate-json",
        ] {
            let mut flow = exchange();
            if case != "legacy" {
                flow.require_startup_isolation(cwd).unwrap();
            }
            let mut values = results();
            values[1]["config"]["projects"] = projects.clone();
            values[1]["layers"][0]["config"]["projects"] = projects.clone();
            values[1]["layers"].as_array_mut().unwrap().push(json!({
                "name":{"type":"project","dotCodexFolder":test_path!("/app/workspace", ".codex")},
                "config":{"private_marker":"unused"},"disabledReason":if case == "enabled" { Value::Null } else { json!("Not trusted") }
            }));
            if case == "managed" {
                values[1]["layers"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"name":{"type":"mdm"},"config":{},"disabledReason":"ignored"}));
            }
            if case == "system" {
                values[1]["layers"][2]["config"] = json!({"hooks":{"private":"must reject"}});
            }
            flow.next_frames().unwrap();
            flow.accept(&reply(0, &values[0])).unwrap();
            flow.next_frames().unwrap();
            let input = if case == "duplicate-json" {
                let text = json!({"id":2,"result":values[1]}).to_string().replace(
                    "\"disabledReason\":",
                    "\"disabledReason\":null,\"disabledReason\":",
                );
                BoundedObservation::new(text.into_bytes()).unwrap()
            } else {
                reply(1, &values[1])
            };
            assert_eq!(flow.accept(&input).is_ok(), case == "disabled", "{case}");
            if case != "disabled" {
                assert_eq!(flow.state(), PreflightState::Failed);
            }
        }
    }

    #[test]
    fn isolation_warning_is_pending_evidence_not_configuration_authority() {
        let home = std::path::Path::new(test_path!("/app/profile"));
        let cwd = std::path::Path::new(test_path!("/app/workspace"));
        let profile = super::super::super::codex_profile_config::ProfileConfig::parse(
            b"cli_auth_credentials_store='file'\n",
        )
        .unwrap();
        let projects = profile.isolated_projects(&[home, cwd]).unwrap();
        let reason = test_path!("/app/workspace is marked as untrusted in the effective configuration. To load project-local config, hooks, and exec policies, update its trust setting. If that setting is managed by your organization, contact your administrator.");
        let summary = format!("Project-local config, hooks, and exec policies are disabled in the following folders until the project is trusted, but skills still load.\n    1. {}\n       {reason}\n", test_path!("/app/workspace", ".codex"));
        for case in [
            "valid",
            "early",
            "late",
            "account",
            "models",
            "requirements",
            "duplicate",
            "unknown",
            "missing",
            "changed",
            "active",
            "legacy",
        ] {
            let mut flow = exchange();
            if case != "legacy" {
                flow.require_startup_isolation(cwd).unwrap();
            }
            let mut values = results();
            values[1]["config"]["projects"] = projects.clone();
            values[1]["layers"][0]["config"]["projects"] = projects.clone();
            if case != "missing" {
                values[1]["layers"].as_array_mut().unwrap().push(json!({
                    "name":{"type":"project","dotCodexFolder":test_path!("/app/workspace", ".codex")},
                    "config":{}, "disabledReason":reason
                }));
            }
            if case == "changed" {
                values[1]["layers"][3]["disabledReason"] = json!("Other reason");
            }
            if case == "active" {
                values[1]["layers"][3]["disabledReason"] = Value::Null;
            }
            let warning = BoundedObservation::new(json!({"method":"configWarning","params":{"summary":if case == "unknown" { "Other warning" } else { &summary }}}).to_string().into_bytes()).unwrap();
            flow.next_frames().unwrap();
            if case != "early" {
                flow.accept(&reply(0, &values[0])).unwrap();
                flow.next_frames().unwrap();
            }
            if case == "late" {
                flow.accept(&reply(1, &values[1])).unwrap();
            }
            if let Some(index) = ["account", "models", "requirements"]
                .iter()
                .position(|value| *value == case)
            {
                for completed in 1..=index + 1 {
                    flow.accept(&reply(completed, &values[completed])).unwrap();
                    flow.next_frames().unwrap();
                }
            }
            if [
                "early",
                "late",
                "account",
                "models",
                "requirements",
                "unknown",
                "legacy",
            ]
            .contains(&case)
            {
                assert!(flow.accept(&warning).is_err(), "{case}");
                assert_eq!(flow.state(), PreflightState::Failed);
                continue;
            }
            assert_eq!(flow.accept(&warning).unwrap(), PreflightState::Checking);
            assert!(flow.display_models().is_none());
            if case == "duplicate" {
                assert!(flow.accept(&warning).is_err());
            } else {
                assert_eq!(
                    flow.accept(&reply(1, &values[1])).is_ok(),
                    case == "valid",
                    "{case}"
                );
            }
            assert_eq!(
                flow.state(),
                if case == "valid" {
                    PreflightState::Checking
                } else {
                    PreflightState::Failed
                }
            );
            assert!(flow.into_text_turn("must not send").is_err());
        }
    }

    #[test]
    fn rejected_selection_retains_display_choices_without_run_authority() {
        let mut flow = exchange();
        let mut values = results();
        values[3]["data"][0]["model"] = json!("available-model");
        assert!(flow.display_models().is_none());
        for (index, value) in values.iter().take(3).enumerate() {
            flow.next_frames().unwrap();
            flow.accept(&reply(index, value)).unwrap();
        }
        flow.next_frames().unwrap();
        assert!(flow.accept(&reply(3, &values[3])).is_err());
        assert_eq!(flow.state(), PreflightState::Failed);
        assert_eq!(flow.display_models().unwrap()[0].model, "available-model");
        assert_eq!(flow.model, "test-model");
        assert!(flow.next_frames().is_err());
        assert!(flow
            .start_thread(std::path::Path::new(test_path!("/managed/snapshot")))
            .is_err());
        assert!(flow.into_text_turn("must not send").is_err());
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
            test_path!("/app/profile"),
            ExpectedPermission::WorkspaceWrite,
        )
        .unwrap()
    }

    #[test]
    fn named_preflight_requires_profile_selection_before_starting_same_snapshot() {
        let cwd = std::path::Path::new(test_path!("/managed/snapshot"));
        for permission in [
            ExpectedPermission::ReadOnly,
            ExpectedPermission::WorkspaceWrite,
        ] {
            for allowed in [false, true] {
                let mut flow = PreflightExchange::with_named_permission(
                    &ExpectedProfile {
                        model: "test-model",
                        reasoning_effort: "medium",
                    },
                    test_path!("/app/profile"),
                    cwd,
                    permission,
                )
                .unwrap();
                let mut values = results();
                let config = &mut values[1]["config"];
                config.as_object_mut().unwrap().remove("sandbox_mode");
                config["default_permissions"] = json!("hello-agent-snapshot");
                let mut policy = super::super::named_config::expected_write_policy(
                    std::path::Path::new(test_path!("/app/profile")),
                    cwd,
                );
                if permission == ExpectedPermission::ReadOnly {
                    policy["filesystem"][test_path!("/managed/snapshot")]["."] = json!("read");
                }
                config["permissions"] = json!({"hello-agent-snapshot":policy});
                values[1]["layers"][0]["config"] = values[1]["config"].clone();
                for (index, result) in values.iter().enumerate() {
                    flow.next_frames().unwrap();
                    assert_eq!(
                        flow.accept(&reply(index, result)).unwrap(),
                        PreflightState::Checking
                    );
                }
                assert!(flow.start_thread(cwd).is_err());
                let frames = flow.next_frames().unwrap();
                let query: Value = serde_json::from_str(frames[0].as_str()).unwrap();
                assert_eq!(query["method"], "permissionProfile/list");
                assert_eq!(query["params"]["cwd"], test_path!("/managed/snapshot"));
                let answer = BoundedObservation::new(serde_json::to_vec(&json!({"id":51,"result":{"data":[{"id":"hello-agent-snapshot","allowed":allowed}],"nextCursor":null}})).unwrap()).unwrap();
                if !allowed {
                    assert!(flow.accept(&answer).is_err());
                    assert_eq!(flow.state(), PreflightState::Failed);
                    continue;
                }
                assert_eq!(
                    flow.accept(&answer).unwrap(),
                    PreflightState::ObservationsMatched
                );
                assert!(flow
                    .start_thread(std::path::Path::new(test_path!("/other")))
                    .is_err());
                let request = flow.start_thread(cwd).unwrap();
                let wire: Value = serde_json::from_str(request.as_str()).unwrap();
                assert_eq!(wire["params"]["permissions"], "hello-agent-snapshot");
                assert_eq!(
                    wire["params"]["runtimeWorkspaceRoots"],
                    json!([test_path!("/managed/snapshot")])
                );
                assert!(wire["params"].get("sandbox").is_none());
                assert!(wire["params"]["config"]
                    .get("sandbox_workspace_write")
                    .is_none());
                assert!(flow.start_thread(cwd).is_err());
            }
        }
    }

    #[test]
    fn reported_managed_layers_cannot_be_ignored_or_recovered_on_same_connection() {
        // This tests reported provenance, not visibility of host MDM policy.
        // Inaccessible/omitted requirements need separate admission evidence.
        for source in [
            "mdm",
            "cloudRequirements",
            "cloudManagedConfig",
            "legacyManagedConfigFile",
            "legacyManagedConfigMdm",
            "unknown",
        ] {
            for append in [false, true] {
                for disabled in [false, true] {
                    let mut flow = PreflightExchange::new(
                        &ExpectedProfile {
                            model: "test-model",
                            reasoning_effort: "medium",
                        },
                        test_path!("/app/profile"),
                    )
                    .unwrap();
                    let values = results();
                    flow.next_frames().unwrap();
                    flow.accept(&reply(0, &values[0])).unwrap();
                    flow.next_frames().unwrap();
                    let mut config = values[1].clone();
                    let layer = json!({
                        "name": {"type": source},
                        "config": {"private_marker": "PRIVATE_MANAGED_POLICY"},
                        "disabledReason": if disabled { Some("PRIVATE_MANAGED_REASON") } else { None },
                    });
                    if append {
                        config["layers"].as_array_mut().unwrap().push(layer);
                    } else {
                        config["layers"][2] = layer;
                    }
                    let error = flow.accept(&reply(1, &config)).unwrap_err();
                    assert!(!error.to_string().contains("PRIVATE_MANAGED"));
                    assert_eq!(
                        flow.state(),
                        PreflightState::Failed,
                        "{source}/{append}/{disabled}"
                    );
                    assert!(flow.accept(&reply(1, &values[1])).is_err());
                    assert!(flow.next_frames().is_err());
                    assert!(flow
                        .start_thread(std::path::Path::new(test_path!("/managed/snapshot")))
                        .is_err());
                    assert!(flow.into_text_turn("must not send").is_err());
                }
            }
        }
    }

    #[test]
    fn configuration_only_cannot_query_accounts_models_or_start_threads() {
        let mut flow = PreflightExchange::configuration_only(
            &ExpectedProfile {
                model: "test-model",
                reasoning_effort: "medium",
            },
            test_path!("/app/profile"),
        )
        .unwrap();
        let mut methods = Vec::new();
        for (index, value) in results().iter().take(2).enumerate() {
            for frame in flow.next_frames().unwrap() {
                let value: Value = serde_json::from_str(frame.as_str()).unwrap();
                methods.push(value["method"].as_str().unwrap().to_owned());
            }
            flow.accept(&reply(index, value)).unwrap();
        }
        assert_eq!(methods, ["initialize", "initialized", "config/read"]);
        assert_eq!(flow.state(), PreflightState::ConfigurationMatched);
        for _ in 0..3 {
            assert!(flow.next_frames().unwrap().is_empty());
            assert!(flow
                .start_thread(std::path::Path::new(test_path!("/managed/snapshot")))
                .is_err());
        }
        assert!(flow.matched_thread_id().is_none());
        assert!(flow.into_text_turn("must not send").is_err());
    }

    #[test]
    fn configuration_only_rejects_mismatches_and_unsolicited_later_responses() {
        for invalid_config in [false, true] {
            let mut flow = PreflightExchange::configuration_only(
                &ExpectedProfile {
                    model: "test-model",
                    reasoning_effort: "medium",
                },
                test_path!("/app/profile"),
            )
            .unwrap();
            let mut values = results();
            flow.next_frames().unwrap();
            flow.accept(&reply(0, &values[0])).unwrap();
            flow.next_frames().unwrap();
            if invalid_config {
                values[1]["config"]["model"] = json!("unexpected-model");
                assert!(flow.accept(&reply(1, &values[1])).is_err());
            } else {
                flow.accept(&reply(1, &values[1])).unwrap();
                assert!(flow.accept(&reply(2, &values[2])).is_err());
            }
            assert_eq!(flow.state(), PreflightState::Failed);
            assert!(flow.next_frames().is_err());
        }
    }

    #[test]
    fn configuration_warning_cannot_fall_back_to_defaults_and_continue() {
        let mut flow = PreflightExchange::configuration_only(
            &ExpectedProfile {
                model: "test-model",
                reasoning_effort: "medium",
            },
            test_path!("/app/profile"),
        )
        .unwrap();
        flow.next_frames().unwrap();
        flow.accept(&reply(0, &results()[0])).unwrap();
        flow.next_frames().unwrap();
        let warning = BoundedObservation::new(br#"{"method":"configWarning","params":{"summary":"Invalid configuration; using defaults.","details":"PRIVATE_MANAGED_CONFIG"}}"#.to_vec()).unwrap();
        let error = flow.accept(&warning).unwrap_err();
        assert!(!error.to_string().contains("PRIVATE_MANAGED_CONFIG"));
        assert_eq!(flow.state(), PreflightState::Failed);
        assert!(flow.accept(&reply(1, &results()[1])).is_err());
        assert!(flow.next_frames().is_err());
        assert!(flow
            .start_thread(std::path::Path::new(test_path!("/managed/snapshot")))
            .is_err());
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
                    json!([test_path!("/outside")]),
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
            let cwd = std::path::Path::new(test_path!("/managed/snapshot"));
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
            let mut response = json!({"thread":{"id":"write-1"},"model":"test-model","modelProvider":"openai","cwd":test_path!("/managed/snapshot"),"instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"workspaceWrite","writableRoots":[],"networkAccess":false,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true},"reasoningEffort":"medium"});
            match case {
                "read-only" => {
                    response["sandbox"] = json!({"type":"readOnly","networkAccess":false})
                }
                "network" => response["sandbox"]["networkAccess"] = json!(true),
                "root" => response["sandbox"]["writableRoots"] = json!([test_path!("/outside")]),
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
            let wire = thread_reply(&response);
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
    fn matched_requires_all_five_ordered_observations_and_never_sends_user_input() {
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
    fn account_note_is_bounded_and_never_substitutes_for_the_reply() {
        let valid =
            r#"{"method":"account/updated","params":{"authMode":"chatgpt","planType":"pro"}}"#;
        let timed = r#"{"method":"account/updated","emittedAtMs":1790643701003,"params":{"authMode":"chatgpt","planType":"pro"}}"#;
        for note in [
            valid,
            timed,
            r#"{"method":"account/updated","emittedAtMs":-1,"params":{"authMode":"chatgpt"}}"#,
            r#"{"method":"account/updated","emittedAtMs":9007199254740992,"params":{"authMode":"chatgpt"}}"#,
            r#"{"method":"account/updated","emittedAtMs":1,"emittedAtMs":2,"params":{"authMode":"chatgpt"}}"#,
            r#"{"method":"account/updated","params":{"authMode":null}}"#,
            r#"{"method":"account/updated","params":{"authMode":"apikey"}}"#,
            r#"{"method":"account/updated","params":{"authMode":"chatgpt","authMode":"chatgpt"}}"#,
            r#"{"method":"account/updated","params":{"authMode":"chatgpt","extra":"PRIVATE"}}"#,
            r#"{"id":3,"method":"account/updated","params":{"authMode":"chatgpt"}}"#,
            r#"{"method":"account/unknown","params":{"authMode":"chatgpt"}}"#,
        ] {
            let mut flow = exchange();
            let values = results();
            for (index, value) in values.iter().take(2).enumerate() {
                flow.next_frames().unwrap();
                flow.accept(&reply(index, value)).unwrap();
            }
            flow.next_frames().unwrap();
            let note = BoundedObservation::new(note.as_bytes().to_vec()).unwrap();
            let accepted = flow.accept(&note);
            if note.bytes == valid.as_bytes() || note.bytes == timed.as_bytes() {
                assert_eq!(accepted.unwrap(), PreflightState::Checking);
                assert!(flow.awaiting_response());
                assert!(flow.next_frames().is_err());
                assert!(flow
                    .start_thread(std::path::Path::new(test_path!("/managed/snapshot")))
                    .is_err());
                flow.accept(&reply(2, &values[2])).unwrap();
                assert!(!flow.awaiting_response());
                for (index, value) in values.iter().enumerate().skip(3) {
                    flow.next_frames().unwrap();
                    flow.accept(&reply(index, value)).unwrap();
                }
                assert_eq!(flow.state(), PreflightState::ObservationsMatched);
            } else {
                assert!(!accepted.unwrap_err().to_string().contains("PRIVATE"));
                assert_eq!(flow.state(), PreflightState::Failed);
                assert!(flow.accept(&reply(2, &values[2])).is_err());
            }
        }
        for at in [0, 1, 2, 3] {
            let mut flow = exchange();
            let values = results();
            for (index, value) in values.iter().enumerate().take(at) {
                flow.next_frames().unwrap();
                flow.accept(&reply(index, value)).unwrap();
            }
            flow.next_frames().unwrap();
            let note = BoundedObservation::new(valid.as_bytes().to_vec()).unwrap();
            if at > 0 {
                flow.accept(&note).unwrap();
                assert!(flow.awaiting_response());
            }
            assert!(flow.accept(&note).is_err()); // wrong stage or duplicate
            assert_eq!(flow.state(), PreflightState::Failed);
        }
    }

    #[test]
    fn early_account_note_does_not_skip_config_or_account_observation() {
        let mut flow = exchange();
        let values = results();
        flow.next_frames().unwrap();
        flow.accept(&reply(0, &values[0])).unwrap();
        flow.next_frames().unwrap();
        let note = BoundedObservation::new(
            br#"{"method":"account/updated","params":{"authMode":"chatgpt"}}"#.to_vec(),
        )
        .unwrap();
        flow.accept(&note).unwrap();
        assert!(flow.awaiting_response());
        assert!(flow.next_frames().is_err());
        for (index, value) in values.iter().enumerate().skip(1) {
            if index > 1 {
                flow.next_frames().unwrap();
            }
            flow.accept(&reply(index, value)).unwrap();
        }
        assert_eq!(flow.state(), PreflightState::ObservationsMatched);
    }

    #[test]
    fn requirements_must_be_explicit_null_before_thread_or_turn() {
        for result in [
            r#"{"requirements":null}"#,
            r#"{"requirements":null,"supportsIndependentSpeedModes":true}"#,
            r#"{"requirements":null,"supportsIndependentSpeedModes":false}"#,
            r#"{"requirements":null,"supportsIndependentSpeedModes":null}"#,
            r#"{}"#,
            r#"{"requirements":{}}"#,
            r#"{"requirements":{"allowedSandboxModes":["readOnly"],"private":"PRIVATE_POLICY"}}"#,
            r#"{"requirements":false}"#,
            r#"{"requirements":[]}"#,
            r#"{"requirements":"null"}"#,
            r#"{"requirements":null,"requirements":null}"#,
            r#"{"requirements":{},"requirements":null}"#,
            r#"{"requirements":null,"extra":"PRIVATE_POLICY"}"#,
            r#"{"requirements":null,"supportsIndependentSpeedModes":"true"}"#,
            r#"{"requirements":{},"supportsIndependentSpeedModes":true}"#,
            r#"{"requirements":null,"supportsIndependentSpeedModes":true,"supportsIndependentSpeedModes":false}"#,
        ] {
            let mut flow = exchange();
            for (index, value) in results().iter().take(4).enumerate() {
                flow.next_frames().unwrap();
                flow.accept(&reply(index, value)).unwrap();
            }
            assert_eq!(flow.state(), PreflightState::Checking);
            assert!(flow
                .start_thread(std::path::Path::new(test_path!("/managed/snapshot")))
                .is_err());
            let request = flow.next_frames().unwrap();
            assert_eq!(request.len(), 1);
            assert_eq!(
                serde_json::from_str::<Value>(request[0].as_str()).unwrap(),
                json!({"id":50,"method":"configRequirements/read","params":{}})
            );
            assert!(flow.next_frames().is_err());
            let wire =
                BoundedObservation::new(format!(r#"{{"id":50,"result":{result}}}"#).into_bytes())
                    .unwrap();
            let accepted = flow.accept(&wire);
            if [
                r#"{"requirements":null}"#,
                r#"{"requirements":null,"supportsIndependentSpeedModes":true}"#,
                r#"{"requirements":null,"supportsIndependentSpeedModes":false}"#,
                r#"{"requirements":null,"supportsIndependentSpeedModes":null}"#,
            ]
            .contains(&result)
            {
                assert_eq!(accepted.unwrap(), PreflightState::ObservationsMatched);
                assert!(flow.next_frames().unwrap().is_empty());
            } else {
                let error = accepted.unwrap_err();
                assert!(!error.to_string().contains("PRIVATE_POLICY"));
                if result == r#"{"requirements":{}}"# || result.contains("allowedSandboxModes") {
                    assert_eq!(flow.diagnostic_stage(), "managed requirements present");
                } else if [
                    r#"{"requirements":false}"#,
                    r#"{"requirements":[]}"#,
                    r#"{"requirements":"null"}"#,
                ]
                .contains(&result)
                {
                    assert_eq!(flow.diagnostic_stage(), "requirements value shape");
                }
                assert!(!flow.diagnostic_stage().contains("PRIVATE_POLICY"));
                assert_eq!(flow.state(), PreflightState::Failed);
                assert!(flow
                    .accept(&reply(4, &json!({"requirements":null})))
                    .is_err());
                assert!(flow.next_frames().is_err());
                assert!(flow
                    .start_thread(std::path::Path::new(test_path!("/managed/snapshot")))
                    .is_err());
                assert!(flow.into_text_turn("goal").is_err());
            }
        }
    }

    #[test]
    fn mismatch_at_each_stage_permanently_stops_followup_requests() {
        for (stage, field, value) in [
            (0, "codexHome", json!(test_path!("/other"))),
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
            PreflightState::ConfigurationMatched,
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
        flow.start_read_only_thread(std::path::Path::new(test_path!("/managed/snapshot")))
            .unwrap();
        let result = json!({"thread":{"id":"thread-1"},"model":"test-model","modelProvider":"openai","cwd":test_path!("/managed/snapshot"),"instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"});
        flow.accept(&thread_reply(&result)).unwrap();
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
        let cwd = std::path::Path::new(test_path!("/managed/snapshot"));
        assert!(exchange().start_read_only_thread(cwd).is_err());
        for mismatch in ["none", "id", "cwd", "model", "sandbox", "instructions"] {
            let mut flow = exchange();
            for (index, result) in results().iter().enumerate() {
                flow.next_frames().unwrap();
                flow.accept(&reply(index, result)).unwrap();
            }
            assert!(flow
                .start_read_only_thread(std::path::Path::new(test_path!("/managed/../other")))
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
            let mut result = json!({"thread":{"id":"thread-1"},"model":"test-model","modelProvider":"openai","cwd":test_path!("/managed/snapshot"),"instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"});
            match mismatch {
                "cwd" => result["cwd"] = json!(test_path!("/other")),
                "model" => result["model"] = json!("other"),
                "sandbox" => result["sandbox"] = json!({"type":"workspaceWrite"}),
                "instructions" => {
                    result["instructionSources"] = json!([test_path!("/user/AGENTS.md")])
                }
                _ => {}
            }
            let wire = if mismatch == "id" {
                reply(3, &result)
            } else {
                thread_reply(&result)
            };
            let accepted = flow.accept(&wire);
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
        let result = json!({"thread":{"id":"thread-1"},"model":"test-model","modelProvider":"openai","cwd":test_path!("/managed/snapshot"),"instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"});
        for case in [
            "before",
            "after",
            "different",
            "duplicate",
            "request",
            "unknown",
            "empty",
            "timedBefore",
            "timedAfter",
            "negativeTime",
            "overflowTime",
            "stringTime",
            "nullTime",
            "duplicateTime",
            "extraEnvelope",
        ] {
            let mut flow = exchange();
            for (index, result) in results().iter().enumerate() {
                flow.next_frames().unwrap();
                flow.accept(&reply(index, result)).unwrap();
            }
            flow.start_read_only_thread(std::path::Path::new(test_path!("/managed/snapshot")))
                .unwrap();
            let mut note = json!({"method":"thread/started","params":{"thread":{"id":"thread-1"}}});
            match case {
                "different" => note["params"]["thread"]["id"] = json!("other"),
                "request" => note["id"] = json!(99),
                "unknown" => note["method"] = json!("thread/unknown"),
                "empty" => note["params"]["thread"]["id"] = json!(""),
                "timedBefore" | "timedAfter" | "duplicateTime" => {
                    note["emittedAtMs"] = json!(1790643701003_u64)
                }
                "negativeTime" => note["emittedAtMs"] = json!(-1),
                "overflowTime" => note["emittedAtMs"] = json!(9_007_199_254_740_992_u64),
                "stringTime" => note["emittedAtMs"] = json!("123"),
                "nullTime" => note["emittedAtMs"] = Value::Null,
                "extraEnvelope" => note["unexpected"] = json!(true),
                _ => {}
            }
            let raw = if case == "duplicateTime" {
                note.to_string().replacen('{', "{\"emittedAtMs\":1,", 1)
            } else {
                note.to_string()
            };
            let note = BoundedObservation::new(raw.into_bytes()).unwrap();
            if matches!(
                case,
                "request"
                    | "unknown"
                    | "empty"
                    | "negativeTime"
                    | "overflowTime"
                    | "stringTime"
                    | "nullTime"
                    | "duplicateTime"
                    | "extraEnvelope"
            ) {
                assert!(flow.accept(&note).is_err());
            } else if matches!(case, "after" | "timedAfter") {
                assert_eq!(
                    flow.accept(&thread_reply(&result)).unwrap(),
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
                    assert!(flow.accept(&thread_reply(&result)).is_err());
                } else {
                    assert_eq!(
                        flow.accept(&thread_reply(&result)).unwrap(),
                        PreflightState::ThreadMatched
                    );
                }
            }
            if matches!(case, "before" | "after" | "timedBefore" | "timedAfter") {
                assert_eq!(flow.matched_thread_id(), Some("thread-1"));
            } else {
                assert_eq!(flow.state(), PreflightState::Failed);
                assert_eq!(flow.matched_thread_id(), None);
            }
        }
    }
}
