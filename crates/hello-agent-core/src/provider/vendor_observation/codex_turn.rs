//! Text-turn wire preparation and correlation, not runtime execution authority.
//! No process caller yet. Requires the same admitted connection as preflight.
use super::codex_events::stream::{Observed, TurnStream};
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::value::RawValue;

pub struct TurnStart {
    thread: String,
    request: Option<String>,
    reply: Option<String>,
    started: Option<BoundedObservation>,
    failed: bool,
    complete: bool,
}

/// Typed text request. No arbitrary renderer JSON can construct it.
pub struct TurnRequest(String);
impl TurnRequest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn invalid() -> ProviderError {
    ProviderError("Codex turn start did not match the requested conversation".into())
}

#[derive(Deserialize)]
struct Turn {
    id: String,
    status: String,
    items: Vec<serde::de::IgnoredAny>,
    error: Box<RawValue>,
}

impl Turn {
    fn validate(&self) -> Result<(), ProviderError> {
        if self.id.is_empty()
            || self.id.len() > 128
            || self.id.chars().any(char::is_control)
            || self.status != "inProgress"
            || !self.items.is_empty()
            || self.error.get() != "null"
        {
            return Err(invalid());
        }
        Ok(())
    }
}

impl TurnStart {
    /// Validate before acquiring a process or sending initialization frames.
    pub fn validate_text(text: &str) -> Result<(), ProviderError> {
        if text.trim().is_empty() || text.len() > 64 * 1024 || text.contains('\0') {
            return Err(invalid());
        }
        Ok(())
    }

    pub(super) fn new(thread: &str, text: &str) -> Result<Self, ProviderError> {
        Self::validate_text(text)?;
        // Inherit the exact validated thread policy; no caller-supplied overrides.
        let request = serde_json::json!({"id":6,"method":"turn/start","params":{
            "threadId":thread,"input":[{"type":"text","text":text,"text_elements":[]}]
        }});
        Ok(Self {
            thread: thread.into(),
            request: Some(format!("{request}\n")),
            reply: None,
            started: None,
            failed: false,
            complete: false,
        })
    }

    /// May contain private user text. Never log it. Can be taken only once.
    pub fn take_request(&mut self) -> Result<String, ProviderError> {
        if self.failed || self.complete {
            return Err(invalid());
        }
        self.request.take().ok_or_else(invalid)
    }

    pub fn take_frame(&mut self) -> Result<TurnRequest, ProviderError> {
        self.take_request().map(TurnRequest)
    }

    /// Exactly one response and one started notification, in either order.
    /// The returned stream has already consumed its turn/started notification.
    pub fn accept(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<Option<TurnStream>, ProviderError> {
        let result = self.accept_inner(input);
        if result.is_err() {
            self.failed = true;
            self.request = None;
            self.reply = None;
            self.started = None;
        }
        result
    }

    fn accept_inner(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<Option<TurnStream>, ProviderError> {
        if self.failed || self.complete || self.request.is_some() {
            return Err(invalid());
        }
        #[derive(Deserialize)]
        struct ResultWire {
            turn: Turn,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            id: u64,
            result: ResultWire,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            thread_id: String,
            turn: Turn,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Note {
            method: String,
            params: Params,
        }
        if input.json()?.get("method").is_some() {
            let note: Note = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
            if note.method != "turn/started"
                || note.params.thread_id != self.thread
                || self.started.is_some()
            {
                return Err(invalid());
            }
            note.params.turn.validate()?;
            self.started = Some(BoundedObservation::new(input.bytes.clone())?);
        } else {
            let reply: Reply = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
            if reply.id != 6 || self.reply.is_some() {
                return Err(invalid());
            }
            reply.result.turn.validate()?;
            self.reply = Some(reply.result.turn.id);
        }
        if let (Some(turn), Some(started)) = (&self.reply, &self.started) {
            let mut stream = TurnStream::new(&self.thread, turn)?;
            if !matches!(stream.observe(started)?, Observed::Started) {
                return Err(invalid());
            }
            self.complete = true;
            self.reply = None;
            self.started = None;
            return Ok(Some(stream));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn input(v: serde_json::Value) -> BoundedObservation {
        BoundedObservation::new(v.to_string().into_bytes()).unwrap()
    }
    fn frames() -> [serde_json::Value; 2] {
        let turn = json!({"id":"turn-1","status":"inProgress","items":[],"error":null});
        [
            json!({"id":6,"result":{"turn":turn}}),
            json!({"method":"turn/started","params":{"threadId":"thread-1","turn":turn}}),
        ]
    }
    #[test]
    fn text_request_is_single_use_and_cannot_override_policy() {
        for text in ["", " \n", "bad\0text", &"x".repeat(65537)] {
            assert!(TurnStart::new("thread-1", text).is_err());
        }
        let mut start = TurnStart::new("thread-1", "목표\n\"test\"").unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&start.take_request().unwrap()).unwrap();
        assert_eq!(
            value,
            json!({"id":6,"method":"turn/start","params":{"threadId":"thread-1","input":[{"type":"text","text":"목표\n\"test\"","text_elements":[]}]}})
        );
        assert!(start.take_request().is_err());
    }
    #[test]
    fn either_order_requires_both_and_hands_started_stream_to_output_parser() {
        for reverse in [false, true] {
            let mut start = TurnStart::new("thread-1", "goal").unwrap();
            start.take_request().unwrap();
            let mut values = frames();
            if reverse {
                values.reverse();
            }
            assert!(start.accept(&input(values[0].clone())).unwrap().is_none());
            let mut stream = start.accept(&input(values[1].clone())).unwrap().unwrap();
            assert!(start.accept(&input(values[1].clone())).is_err());
            let done = input(
                json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed","items":[],"error":null}}}),
            );
            assert!(matches!(
                stream.observe(&done).unwrap(),
                Observed::Finished(_)
            ));
            assert!(stream.finish().is_ok());
        }
    }
    #[test]
    fn malformed_cross_thread_duplicate_and_unsolicited_frames_poison_start() {
        for case in [
            "wrong-id",
            "wrong-thread",
            "wrong-turn",
            "status",
            "items",
            "error",
            "request",
            "duplicate",
            "unsent",
        ] {
            let mut start = TurnStart::new("thread-1", "PRIVATE").unwrap();
            if case != "unsent" {
                start.take_request().unwrap();
            }
            let mut values = frames();
            match case {
                "wrong-id" => values[0]["id"] = json!(9),
                "wrong-thread" => values[1]["params"]["threadId"] = json!("other"),
                "wrong-turn" => values[1]["params"]["turn"]["id"] = json!("other"),
                "status" => values[0]["result"]["turn"]["status"] = json!("completed"),
                "items" => values[0]["result"]["turn"]["items"] = json!([{}]),
                "error" => values[0]["result"]["turn"]["error"] = json!({"message":"PRIVATE"}),
                "request" => values[1]["id"] = json!(6),
                "duplicate" => values[1] = values[0].clone(),
                _ => {}
            }
            let first = start.accept(&input(values[0].clone()));
            let error = match first {
                Err(e) => e,
                Ok(_) => match start.accept(&input(values[1].clone())) {
                    Err(e) => e,
                    Ok(_) => panic!("accepted {case}"),
                },
            };
            assert!(!error.to_string().contains("PRIVATE"));
            assert!(start.take_request().is_err());
            assert!(start.accept(&input(frames()[0].clone())).is_err());
        }
    }
}
