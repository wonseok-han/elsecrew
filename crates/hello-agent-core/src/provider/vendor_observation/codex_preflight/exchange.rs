//! One connection's ordered preflight observations. No launch authority or prompt API.
//! The process owner must separately enforce profile isolation, deadlines and cleanup.

use super::{match_model_catalog, match_read_only_config, validate_profile, ExpectedProfile};
use crate::provider::vendor_observation::codex_rpc::{ModelFreeExchange, ObservationKind};
use crate::provider::{BoundedObservation, ProviderError};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightState {
    Checking,
    ObservationsMatched,
    Failed,
}

/// Holds only the requested tuple and profile identity, never raw account/config replies.
pub struct PreflightExchange {
    rpc: ModelFreeExchange,
    model: String,
    effort: String,
    expected_home: String,
    state: PreflightState,
}

fn invalid() -> ProviderError {
    ProviderError("Codex preflight connection did not match the requested profile".into())
}

impl PreflightExchange {
    pub fn new(profile: &ExpectedProfile<'_>, expected_home: &str) -> Result<Self, ProviderError> {
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
            rpc: ModelFreeExchange::default(),
            model: profile.model.into(),
            effort: profile.reasoning_effort.into(),
            expected_home: expected_home.into(),
            state: PreflightState::Checking,
        })
    }

    pub fn state(&self) -> PreflightState {
        self.state
    }

    pub fn invalidate(&mut self) {
        self.state = PreflightState::Failed;
    }

    pub fn next_request(&mut self) -> Result<Option<String>, ProviderError> {
        match self.state {
            PreflightState::Failed => Err(invalid()),
            PreflightState::ObservationsMatched => Ok(None),
            PreflightState::Checking => self.rpc.next_request(),
        }
    }

    pub fn accept(&mut self, input: &BoundedObservation) -> Result<PreflightState, ProviderError> {
        let result = self.accept_inner(input);
        if result.is_err() {
            self.invalidate();
        }
        result.map_err(|_| invalid())
    }

    fn accept_inner(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<PreflightState, ProviderError> {
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
            ObservationKind::Config => match_read_only_config(&observation, &profile)?,
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
        vec![
            json!({"userAgent":"test","codexHome":"/app/profile","platformFamily":"unix","platformOs":"macos"}),
            json!({"config":{"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only"}}),
            json!({"account":{"type":"chatgpt","email":"PRIVATE"},"requiresOpenaiAuth":true}),
            json!({"data":[{"id":"test-model","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}],"nextCursor":null}),
        ]
    }
    fn reply(index: usize, result: &Value) -> BoundedObservation {
        BoundedObservation::new(serde_json::to_vec(&json!({"id":index+1,"result":result})).unwrap())
            .unwrap()
    }

    #[test]
    fn matched_requires_all_four_ordered_observations_and_never_sends_user_input() {
        let mut exchange = exchange();
        for (index, result) in results().iter().enumerate() {
            let request = exchange.next_request().unwrap().unwrap();
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
}
