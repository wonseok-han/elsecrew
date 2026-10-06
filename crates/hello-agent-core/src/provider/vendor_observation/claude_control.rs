//! Model-free control initialization. No prompt, launch, or permission authority.
//! Wire source: anthropics/claude-agent-sdk-python, _internal/query.py (2026-10-06).
//! System init and launch-policy observations remain separate from this handshake.
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::{json, value::RawValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    New,
    AwaitingResponse,
    Ready,
    Failed,
    Closed,
}

pub struct InitializeExchange {
    request_id: String,
    state: State,
}

/// Only fixed initialization can construct this wire frame. No caller command payload.
pub struct InitializeRequest(String);
impl InitializeRequest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn invalid() -> ProviderError {
    ProviderError("Claude control initialization did not match".into())
}

impl InitializeExchange {
    pub fn new(request_id: &str) -> Result<Self, ProviderError> {
        if request_id.is_empty()
            || request_id.len() > 80
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(invalid());
        }
        Ok(Self {
            request_id: request_id.into(),
            state: State::New,
        })
    }
    pub fn state(&self) -> State {
        self.state
    }

    pub fn request(&mut self) -> Result<InitializeRequest, ProviderError> {
        if self.state != State::New {
            return Err(invalid());
        }
        self.state = State::AwaitingResponse;
        // hooks:null registers no SDK hooks; it does not disable on-disk hooks.
        let frame = json!({"type":"control_request","request_id":self.request_id,
            "request":{"subtype":"initialize","hooks":null,"skills":[]}});
        Ok(InitializeRequest(format!("{frame}\n")))
    }

    /// The transport owner routes non-response frames separately and enforces
    /// deadlines/output budgets. This method accepts exactly the pending response.
    pub fn accept_response(&mut self, input: &BoundedObservation) -> Result<(), ProviderError> {
        if self.state != State::AwaitingResponse {
            self.state = State::Failed;
            return Err(invalid());
        }
        let result = self.accept_inner(input);
        self.state = if result.is_ok() {
            State::Ready
        } else {
            State::Failed
        };
        result
    }

    fn accept_inner(&self, input: &BoundedObservation) -> Result<(), ProviderError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            #[serde(rename = "type")]
            kind: String,
            response: Response,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Response {
            subtype: String,
            request_id: String,
            response: Box<RawValue>,
        }
        let wire: Envelope = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if wire.kind != "control_response"
            || wire.response.subtype != "success"
            || wire.response.request_id != self.request_id
            || !serde_json::from_str::<serde_json::Value>(wire.response.response.get())
                .map_err(|_| invalid())?
                .is_object()
        {
            return Err(invalid());
        }
        // Do not retain raw account/model/catalog data or infer permission from it.
        Ok(())
    }

    pub fn close(&mut self) {
        self.state = State::Closed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response() -> serde_json::Value {
        json!({"type":"control_response","response":{"subtype":"success","request_id":"init_a","response":{"account":{"private":"SECRET"}}}})
    }
    fn input(value: &serde_json::Value) -> BoundedObservation {
        BoundedObservation::new(value.to_string().into_bytes()).unwrap()
    }
    #[test]
    fn initializes_once_without_a_user_prompt_and_cannot_reuse_a_closed_connection() {
        let mut exchange = InitializeExchange::new("init_a").unwrap();
        let frame = exchange.request().unwrap();
        assert_eq!(frame.as_str().lines().count(), 1);
        let wire: serde_json::Value = serde_json::from_str(frame.as_str()).unwrap();
        assert_eq!(
            wire,
            json!({"type":"control_request","request_id":"init_a","request":{"subtype":"initialize","hooks":null,"skills":[]}})
        );
        assert!(exchange.request().is_err());
        exchange.accept_response(&input(&response())).unwrap();
        assert_eq!(exchange.state(), State::Ready);
        exchange.close();
        assert!(exchange.request().is_err());
        assert!(exchange.accept_response(&input(&response())).is_err());
    }
    #[test]
    fn wrong_correlation_errors_and_malformed_envelopes_fail_without_private_details() {
        for case in ["id", "error", "missing", "type", "duplicate", "unsolicited"] {
            let mut wire = response();
            match case {
                "id" => wire["response"]["request_id"] = json!("other"),
                "error" => {
                    wire["response"]["subtype"] = json!("error");
                    wire["response"]["error"] = json!("SECRET");
                }
                "missing" => {
                    wire["response"].as_object_mut().unwrap().remove("response");
                }
                "type" => wire["type"] = json!("control_request"),
                _ => {}
            }
            let mut exchange = InitializeExchange::new("init_a").unwrap();
            if case != "unsolicited" {
                exchange.request().unwrap();
            }
            let observation = if case == "duplicate" {
                BoundedObservation::new(
                    wire.to_string()
                        .replacen(
                            "\"request_id\":",
                            "\"request_id\":\"other\",\"request_id\":",
                            1,
                        )
                        .into_bytes(),
                )
                .unwrap()
            } else {
                input(&wire)
            };
            let error = exchange.accept_response(&observation).unwrap_err();
            assert!(!error.to_string().contains("SECRET"));
            assert_eq!(exchange.state(), State::Failed);
            assert!(exchange.accept_response(&input(&response())).is_err());
        }
    }
}
