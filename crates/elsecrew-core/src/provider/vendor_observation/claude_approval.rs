//! Claude SDK control permissions. Runtime admission is separate from wire replies.
//! Wire: anthropics/claude-agent-sdk-python/_internal/query.py (2026-10-06).
//! One instance belongs to one connection. Raw tool inputs are never public output.
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub const PRE_TOOL_CALLBACK: &str = "elsecrew_pre_tool";
/// What the agent reads when one tool request is refused, by the person or by
/// the role's boundary: enough to carry on instead of retrying the same call.
pub const DENIAL_GUIDANCE: &str = "Tool request not allowed: declined by the application user, or outside the permissions or working directory of this role (stay inside it, read only files that exist, keep out of .git, .claude, .codex and .agents). Continue without it.";

/// Request ids are remembered for the whole session so a replayed id is always
/// refused. Every tool call asks once (PreToolUse hook), and a session has no
/// time limit, so this bounds memory, not the work: it matches the durable
/// internal event budget (`ProviderEventBudget::DurableV1`), which a session
/// reaches first. Raise both together.
const MAX_SESSION_REQUESTS: usize = 65_536;

/// Tool inputs up to this size are kept like any other (the old line bound).
const SMALL_TOOL_INPUT_BYTES: usize = 128 * 1024;
/// The largest tool input a request may carry and still be approved: a file
/// written whole, say (2026-11-26 big Write). It is kept exactly, shown in
/// part with its size and SHA-256, and echoed whole by the allow reply.
pub const LARGE_TOOL_INPUT_MAX_BYTES: usize = 4 * 1024 * 1024;
/// How many bytes of large tool inputs all sessions in this process may hold
/// while they wait for an answer. A request past this, or past
/// `LARGE_TOOL_INPUT_MAX_BYTES`, keeps no input and is refused with
/// `LARGE_INPUT_GUIDANCE`; the session carries on.
pub const LARGE_TOOL_INPUTS_HELD_MAX_BYTES: usize = 16 * 1024 * 1024;
/// The largest allow reply: the echoed input plus its small envelope.
pub const LARGE_TOOL_REPLY_MAX_BYTES: usize = LARGE_TOOL_INPUT_MAX_BYTES + 64 * 1024;
static LARGE_TOOL_INPUTS_HELD: AtomicUsize = AtomicUsize::new(0);
/// What the agent reads when a request was too large to keep for approval.
pub const LARGE_INPUT_GUIDANCE: &str = "Tool request not allowed: its input is too large for this application to review (over 4 MiB, or too many large requests at once). Write the file in smaller parts, for example create it with a first part and append the rest with edits, then continue.";

/// Bytes of one large tool input counted against `LARGE_TOOL_INPUTS_HELD_MAX_BYTES`.
/// It stays counted from the request through its allow reply until that reply
/// is written or dropped (PR #19 review I1); returned when dropped, whatever
/// the path: answered, declined, withdrawn, the session or run ended, or a
/// write that failed. A new engine process starts with nothing held.
pub struct LargeInputHold(usize);
impl LargeInputHold {
    fn reserve(bytes: usize) -> Option<Self> {
        let mut held = LARGE_TOOL_INPUTS_HELD.load(Ordering::Acquire);
        loop {
            let next = held.checked_add(bytes)?;
            if next > LARGE_TOOL_INPUTS_HELD_MAX_BYTES {
                return None;
            }
            match LARGE_TOOL_INPUTS_HELD.compare_exchange_weak(held, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(Self(bytes)),
                Err(current) => held = current,
            }
        }
    }
}
impl Drop for LargeInputHold {
    fn drop(&mut self) {
        LARGE_TOOL_INPUTS_HELD.fetch_sub(self.0, Ordering::AcqRel);
    }
}

/// Bytes of large tool inputs (and their allow replies) held in this process.
pub fn large_tool_inputs_held() -> usize {
    LARGE_TOOL_INPUTS_HELD.load(Ordering::Acquire)
}

/// Tests that hold large tool inputs share the process budget; they take this
/// lock so their counts do not mix.
#[cfg(test)]
pub(crate) static LARGE_INPUT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Default)]
pub struct Permissions {
    seen: BTreeSet<String>,
    pending: BTreeMap<String, Pending>,
    hook_scope: Option<(String, String)>,
    failed: bool,
}

struct Pending {
    hook: bool,
    token: Arc<()>,
    tool_name: String,
    tool_use_id: Option<String>,
    withheld: bool,
}

/// Private inspection, not authorization. Never serialize into chat or run metadata.
pub struct Request {
    pub request_id: String,
    pub tool_name: String,
    pub tool_use_id: Option<String>,
    target: ToolTarget,
    tool_input: String,
    token: Arc<()>,
    /// The input was too large to keep: it can only be refused.
    withheld: bool,
    /// Moved into the allow reply, so the bytes stay counted while it is sent.
    hold: std::sync::Mutex<Option<LargeInputHold>>,
}

/// Paths are inspection data. Only the run's work history shows one, and only
/// relative to the workspace (`agent_runtime/preparation/activity.rs`); tool
/// input itself is never public.
pub enum ToolTarget {
    File { path: String, writes: bool },
    Search { path: String },
    Shell,
    Unsupported,
}
impl Request {
    /// Only expose following an explicit, live approval inspection request.
    pub fn inspect_input(&self) -> &str {
        &self.tool_input
    }
    pub fn target(&self) -> &ToolTarget {
        &self.target
    }
    /// True when the input was too large to keep for review; the request can
    /// only be refused (with `LARGE_INPUT_GUIDANCE`).
    pub fn withheld(&self) -> bool {
        self.withheld
    }
}
fn tool_target(tool: &str, input: &Value, cwd: &str) -> Result<ToolTarget, ProviderError> {
    fn path(value: &Value) -> Result<String, ProviderError> {
        let path = value.as_str().ok_or_else(invalid)?;
        if path.is_empty() || path.len() > 16 * 1024 || path.chars().any(char::is_control) {
            return Err(invalid());
        }
        Ok(path.into())
    }
    match tool {
        "Read" | "Write" | "Edit" => Ok(ToolTarget::File {
            path: path(input.get("file_path").ok_or_else(invalid)?)?,
            writes: tool != "Read",
        }),
        "Glob" | "Grep" => {
            if !input.get("pattern").is_some_and(Value::is_string) {
                return Err(invalid());
            }
            let root = match input.get("path") {
                Some(value) => path(value)?,
                None => cwd.into(),
            };
            Ok(ToolTarget::Search { path: root })
        }
        "Bash" => Ok(ToolTarget::Shell),
        _ => Ok(ToolTarget::Unsupported),
    }
}

pub enum Observation {
    Requested(Request),
    ToolCheck(Request),
    Cancelled,
}

pub struct Denial(String);
impl Denial {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single response to the exact inspected request, not a persistent permission.
pub struct AllowOnce(String, Option<LargeInputHold>);
impl AllowOnce {
    pub fn as_str(&self) -> &str { &self.0 }
    /// The budget the echoed input is counted against, for the transport to
    /// keep until the reply is written (or dropped with the connection).
    pub fn take_hold(&mut self) -> Option<LargeInputHold> { self.1.take() }
}

fn invalid() -> ProviderError {
    ProviderError("Claude permission request did not match".into())
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

impl Permissions {
    pub fn is_pending(&self, request_id: &str) -> bool {
        !self.failed && self.pending.contains_key(request_id)
    }

    pub(super) fn bind_hook_scope(&mut self, session: &str, cwd: &str) {
        self.hook_scope = Some((session.into(), cwd.into()));
    }
    pub fn observe(&mut self, input: &BoundedObservation) -> Result<Observation, ProviderError> {
        let result = self.observe_inner(input);
        if result.is_err() {
            self.failed = true;
            self.pending.clear();
        }
        result
    }

    fn observe_inner(&mut self, input: &BoundedObservation) -> Result<Observation, ProviderError> {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(rename = "type")]
            kind: String,
            request_id: String,
            request: Option<ToolRequest>,
        }
        #[derive(Deserialize)]
        struct ToolRequest {
            subtype: String,
            tool_name: Option<String>,
            input: Value,
            tool_use_id: Option<String>,
            callback_id: Option<String>,
            agent_id: Option<String>,
        }
        if self.failed {
            return Err(invalid());
        }
        let wire: Envelope = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if !valid_id(&wire.request_id) {
            return Err(invalid());
        }
        if wire.kind == "control_cancel_request" {
            // Cancellation can race a completed reply. Never reply to cancellation.
            if wire.request.is_some() || !self.seen.contains(&wire.request_id) {
                return Err(invalid());
            }
            self.pending.remove(&wire.request_id);
            return Ok(Observation::Cancelled);
        }
        let request = wire.request.ok_or_else(invalid)?;
        let hook = request.subtype == "hook_callback";
        let (tool_name, tool_use_id, target, tool_input) = if hook {
            #[derive(Deserialize)]
            struct HookInput {
                session_id: String,
                cwd: String,
                hook_event_name: String,
                tool_name: String,
                tool_use_id: String,
                tool_input: Value,
                agent_id: Option<String>,
            }
            let data: HookInput =
                serde_json::from_value(request.input.clone()).map_err(|_| invalid())?;
            let (session, cwd) = self.hook_scope.as_ref().ok_or_else(invalid)?;
            if request.callback_id.as_deref() != Some(PRE_TOOL_CALLBACK)
                || data.hook_event_name != "PreToolUse"
                || &data.session_id != session
                || &data.cwd != cwd
                || data.agent_id.is_some()
                || !data.tool_input.is_object()
                || request.tool_use_id.as_deref() != Some(data.tool_use_id.as_str())
            {
                return Err(invalid());
            }
            let target = tool_target(&data.tool_name, &data.tool_input, cwd)?;
            (data.tool_name, Some(data.tool_use_id), target, data.tool_input)
        } else {
            let tool_name = request.tool_name.ok_or_else(invalid)?;
            let target = match self.hook_scope.as_ref() {
                Some((_, cwd)) => tool_target(&tool_name, &request.input, cwd)?,
                None => ToolTarget::Unsupported,
            };
            (
                tool_name,
                request.tool_use_id,
                target,
                request.input.clone(),
            )
        };
        if wire.kind != "control_request"
            || (!hook && request.subtype != "can_use_tool")
            || request.agent_id.is_some()
            || !valid_id(&tool_name)
            || !request.input.is_object()
            || tool_use_id.as_deref().is_some_and(|id| !valid_id(id))
            || self.seen.len() >= MAX_SESSION_REQUESTS
            || !self.seen.insert(wire.request_id.clone())
        {
            return Err(invalid());
        }
        let tool_input = tool_input.to_string();
        // A large input is kept only within its own bound and the process
        // budget; otherwise nothing of it is kept and the request can only be
        // refused, which tells the agent to write in smaller parts.
        let (tool_input, hold, withheld) = if tool_input.len() <= SMALL_TOOL_INPUT_BYTES {
            (tool_input, None, false)
        } else if tool_input.len() <= LARGE_TOOL_INPUT_MAX_BYTES {
            match LargeInputHold::reserve(tool_input.len()) {
                Some(hold) => (tool_input, Some(hold), false),
                None => (String::new(), None, true),
            }
        } else {
            (String::new(), None, true)
        };
        let token = Arc::new(());
        self.pending.insert(wire.request_id.clone(), Pending {
            hook, token: token.clone(), tool_name: tool_name.clone(), tool_use_id: tool_use_id.clone(), withheld,
        });
        let request = Request {
            request_id: wire.request_id,
            tool_name,
            tool_use_id,
            target,
            tool_input,
            token,
            withheld,
            hold: std::sync::Mutex::new(hold),
        };
        Ok(if hook {
            Observation::ToolCheck(request)
        } else {
            Observation::Requested(request)
        })
    }

    /// Consumes exactly one outstanding request. No permission update or allow path.
    pub fn deny(&mut self, request_id: &str, interrupt: bool) -> Result<Denial, ProviderError> {
        if self.failed {
            return Err(invalid());
        }
        let pending = self.pending.remove(request_id).ok_or_else(invalid)?;
        let guidance = if pending.withheld { LARGE_INPUT_GUIDANCE } else { DENIAL_GUIDANCE };
        let response = if pending.hook {
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny",
                "permissionDecisionReason":guidance}})
        } else {
            json!({"behavior":"deny","message":guidance,"interrupt":interrupt})
        };
        let frame = json!({"type":"control_response","response":{
            "subtype":"success","request_id":request_id,
            "response":response
        }});
        Ok(Denial(format!("{frame}\n")))
    }

    /// Caller must first admit the tool against the execution boundary and user decision.
    /// No caller-supplied replacement input or persistent permission updates are accepted.
    pub fn allow_once(&mut self, request: &Request) -> Result<AllowOnce, ProviderError> {
        let pending = self.pending.get(&request.request_id).ok_or_else(invalid)?;
        if self.failed || self.hook_scope.is_none()
            || !Arc::ptr_eq(&pending.token, &request.token)
            || pending.tool_name != request.tool_name || pending.tool_use_id != request.tool_use_id
            || matches!(request.target, ToolTarget::Unsupported)
            || pending.withheld || request.withheld
        { return Err(invalid()); }
        let input: Value = serde_json::from_str(&request.tool_input).map_err(|_| invalid())?;
        let response = if pending.hook {
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse",
                "permissionDecision":"allow","updatedInput":input}})
        } else {
            json!({"behavior":"allow","updatedInput":input})
        };
        let frame = format!("{}\n", json!({"type":"control_response","response":{
            "subtype":"success","request_id":request.request_id,"response":response
        }}));
        if frame.len() > LARGE_TOOL_REPLY_MAX_BYTES { return Err(invalid()); }
        self.pending.remove(&request.request_id);
        let hold = request.hold.lock().ok().and_then(|mut hold| hold.take());
        Ok(AllowOnce(frame, hold))
    }

    pub fn close(&mut self) {
        self.failed = true;
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(value: Value) -> BoundedObservation {
        BoundedObservation::new(value.to_string().into_bytes()).unwrap()
    }
    fn request(id: &str) -> BoundedObservation {
        input(json!({"type":"control_request","request_id":id,"request":{
            "subtype":"can_use_tool","tool_name":"Bash","input":{"command":"SECRET"},"tool_use_id":"tool1"
        }}))
    }
    fn large_request(id: &str, hook: bool, content: &str) -> BoundedObservation {
        let value = if hook {
            json!({"type":"control_request","request_id":id,"request":{"subtype":"hook_callback","callback_id":PRE_TOOL_CALLBACK,"tool_use_id":format!("t_{id}"),
                "input":{"session_id":"s","cwd":"/snapshot","hook_event_name":"PreToolUse","tool_name":"Write","tool_use_id":format!("t_{id}"),
                    "tool_input":{"file_path":"big.txt","content":content}}}})
        } else {
            json!({"type":"control_request","request_id":id,"request":{"subtype":"can_use_tool","tool_name":"Write",
                "input":{"file_path":"big.txt","content":content},"tool_use_id":format!("t_{id}")}})
        };
        let mut bytes = value.to_string().into_bytes();
        bytes.push(b'\n');
        if bytes.len() <= crate::provider::PROVIDER_OBSERVATION_LINE_MAX_BYTES {
            BoundedObservation::new(bytes).unwrap()
        } else {
            crate::provider::oversized_line::reduce_oversized_line(&bytes, crate::provider::oversized_line::OversizedLinePolicy::ClaudeStreamJson).unwrap()
        }
    }

    /// 2026-11-26 big Write: a 300 KB file written whole (hook check and
    /// permission request) is kept exactly, held against the process budget,
    /// and the allow reply echoes the original; answering returns the budget.
    #[test]
    fn a_large_write_is_kept_exactly_and_echoed_whole() {
        let _held = LARGE_INPUT_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let content = "0123456789abcdef\n".repeat(18_000);
        for hook in [true, false] {
            let mut permissions = Permissions::default();
            permissions.bind_hook_scope("s", "/snapshot");
            let before = large_tool_inputs_held();
            let request = match permissions.observe(&large_request("big", hook, &content)).unwrap() {
                Observation::ToolCheck(request) | Observation::Requested(request) => request,
                Observation::Cancelled => panic!("expected a request"),
            };
            assert!(!request.withheld());
            assert_eq!(serde_json::from_str::<Value>(request.inspect_input()).unwrap()["content"], json!(content));
            assert!(large_tool_inputs_held() >= before + content.len());
            let mut reply = permissions.allow_once(&request).unwrap();
            let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
            let echoed = if hook { &wire["response"]["response"]["hookSpecificOutput"]["updatedInput"] } else { &wire["response"]["response"]["updatedInput"] };
            assert_eq!(echoed["content"].as_str(), Some(content.as_str()), "hook {hook}");
            // The reply echoes exactly the text the card pages through.
            assert!(reply.as_str().contains(request.inspect_input()));
            // The budget moves with the reply: still counted while it is sent.
            drop(request);
            assert!(large_tool_inputs_held() >= before + content.len(), "counted until the reply is written");
            let hold = reply.take_hold().expect("a large reply carries its hold");
            drop(reply);
            assert!(large_tool_inputs_held() >= before + content.len());
            drop(hold);
            assert_eq!(large_tool_inputs_held(), before, "writing (or dropping) the reply returns the budget");
        }
    }

    /// Past the per-request bound or the process budget nothing of the input
    /// is kept: the request can only be refused, with guidance the agent can
    /// act on, and the session carries on.
    #[test]
    fn a_request_too_large_to_keep_is_refused_with_guidance() {
        let _held = LARGE_INPUT_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut permissions = Permissions::default();
        permissions.bind_hook_scope("s", "/snapshot");
        let huge = "z".repeat(LARGE_TOOL_INPUT_MAX_BYTES + 1);
        let Observation::Requested(request) = permissions.observe(&large_request("huge", false, &huge)).unwrap() else { panic!() };
        assert!(request.withheld() && request.inspect_input().is_empty());
        assert!(matches!(request.target(), ToolTarget::File { path, writes: true } if path == "big.txt"));
        assert!(permissions.allow_once(&request).is_err(), "a withheld request is never allowed");
        let denial = permissions.deny("huge", false).unwrap();
        let wire: Value = serde_json::from_str(denial.as_str()).unwrap();
        assert_eq!(wire["response"]["response"]["message"], LARGE_INPUT_GUIDANCE);
        // The session is still open: an ordinary request is read next.
        assert!(matches!(permissions.observe(&request_small("next")).unwrap(), Observation::Requested(_)));

        // The process budget: hold large inputs until it is full; the next
        // one is withheld, and answering one makes room again.
        let mut held = Vec::new();
        let part = "p".repeat(LARGE_TOOL_INPUT_MAX_BYTES - 1024);
        let mut withheld = None;
        for index in 0..8 {
            let id = format!("hold{index}");
            let Observation::Requested(request) = permissions.observe(&large_request(&id, false, &part)).unwrap() else { panic!() };
            if request.withheld() { withheld = Some(request); break; }
            held.push(request);
        }
        let withheld = withheld.expect("the budget filled up");
        assert!(large_tool_inputs_held() <= LARGE_TOOL_INPUTS_HELD_MAX_BYTES);
        assert_eq!(held.len(), LARGE_TOOL_INPUTS_HELD_MAX_BYTES / part.len());
        let denial = permissions.deny(&withheld.request_id, false).unwrap();
        assert!(denial.as_str().contains("smaller parts"));
        let first = held.remove(0);
        permissions.deny(&first.request_id, false).unwrap();
        drop(first);
        let Observation::Requested(again) = permissions.observe(&large_request("again", false, &part)).unwrap() else { panic!() };
        assert!(!again.withheld(), "answering a held request makes room");
        drop((held, again, withheld));
        assert_eq!(large_tool_inputs_held(), 0);
    }

    fn request_small(id: &str) -> BoundedObservation {
        input(json!({"type":"control_request","request_id":id,"request":{
            "subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"},"tool_use_id":format!("t_{id}")
        }}))
    }

    #[test]
    fn allow_once_binds_connection_request_and_original_input() {
        let mut first = Permissions::default();
        let mut second = Permissions::default();
        first.bind_hook_scope("s", "/snapshot");
        second.bind_hook_scope("s", "/snapshot");
        let Observation::Requested(mut a) = first.observe(&request("a")).unwrap() else { panic!() };
        let Observation::Requested(b) = second.observe(&request("a")).unwrap() else { panic!() };
        assert!(first.allow_once(&b).is_err());
        a.tool_name = "Read".into();
        assert!(first.allow_once(&a).is_err());
        a.tool_name = "Bash".into();
        let reply = first.allow_once(&a).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["response"], json!({"behavior":"allow","updatedInput":{"command":"SECRET"}}));
        assert!(first.allow_once(&a).is_err());
        assert!(first.deny("a", false).is_err());
        second.observe(&input(json!({"type":"control_cancel_request","request_id":"a"}))).unwrap();
        assert!(second.allow_once(&b).is_err());
    }
    #[test]
    fn hook_allow_keeps_input_and_does_not_add_persistent_permissions() {
        let mut permissions = Permissions::default();
        permissions.bind_hook_scope("s", "/snapshot");
        let hook = input(json!({"type":"control_request","request_id":"h","request":{
            "subtype":"hook_callback","callback_id":PRE_TOOL_CALLBACK,"tool_use_id":"t",
            "input":{"session_id":"s","cwd":"/snapshot","hook_event_name":"PreToolUse",
                "tool_name":"Read","tool_use_id":"t","tool_input":{"file_path":"src/lib.rs"}}
        }}));
        let Observation::ToolCheck(request) = permissions.observe(&hook).unwrap() else { panic!() };
        let reply = permissions.allow_once(&request).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["response"], json!({"hookSpecificOutput":{
            "hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"file_path":"src/lib.rs"}
        }}));
        assert!(permissions.allow_once(&request).is_err());
    }
    #[test]
    fn explicit_inspection_retains_input_without_echoing_in_denial() {
        let mut permissions = Permissions::default();
        permissions.bind_hook_scope("s", "/snapshot");
        let wire = input(json!({"type":"control_request","request_id":"edit","request":{
            "subtype":"can_use_tool","tool_name":"Edit","input":{
                "file_path":"src/main.rs","old_string":"OLD_PRIVATE","new_string":"NEW_PRIVATE"
            },"tool_use_id":"tool1"
        }}));
        let Observation::Requested(request) = permissions.observe(&wire).unwrap() else { panic!() };
        assert!(matches!(request.target(), ToolTarget::File { path, writes:true } if path == "src/main.rs"));
        let content: Value = serde_json::from_str(request.inspect_input()).unwrap();
        assert_eq!(content["new_string"], "NEW_PRIVATE");
        assert!(!permissions.deny("edit", false).unwrap().as_str().contains("PRIVATE"));
    }
    #[test]
    fn pre_tool_checks_require_bound_scope_and_use_hook_denials() {
        let hook = json!({"type":"control_request","request_id":"hook-r","request":{
            "subtype":"hook_callback","callback_id":PRE_TOOL_CALLBACK,"tool_use_id":"tool1",
            "input":{"session_id":"s","cwd":"/snapshot","hook_event_name":"PreToolUse",
                "tool_name":"Read","tool_use_id":"tool1","tool_input":{"file_path":"SECRET"}}
        }});
        assert!(Permissions::default()
            .observe(&input(hook.clone()))
            .is_err());
        let mut permissions = Permissions::default();
        permissions.bind_hook_scope("s", "/snapshot");
        assert!(matches!(
            permissions.observe(&input(hook.clone())).unwrap(),
            Observation::ToolCheck(_)
        ));
        let reply = permissions.deny("hook-r", false).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(
            wire["response"]["response"]["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert!(wire["response"]["response"].get("behavior").is_none());
        assert!(!reply.as_str().contains("SECRET"));
        for field in ["session_id", "cwd", "tool_use_id", "hook_event_name"] {
            let mut permissions = Permissions::default();
            permissions.bind_hook_scope("s", "/snapshot");
            let mut wrong = hook.clone();
            wrong["request"]["input"][field] = json!("other");
            assert!(permissions.observe(&input(wrong)).is_err());
            assert!(permissions.deny("hook-r", false).is_err());
        }
    }
    #[test]
    fn denial_is_correlated_single_use_and_does_not_echo_private_input() {
        let mut permissions = Permissions::default();
        permissions.observe(&request("a")).unwrap();
        permissions.observe(&request("b")).unwrap();
        assert!(permissions.deny("unknown", false).is_err());
        let reply = permissions.deny("a", false).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["request_id"], "a");
        assert_eq!(wire["response"]["response"]["behavior"], "deny");
        assert!(!reply.as_str().contains("SECRET"));
        assert!(permissions.deny("a", false).is_err());
        assert!(permissions.deny("b", true).is_ok());
    }
    #[test]
    fn a_long_session_keeps_asking_and_still_refuses_replayed_ids() {
        let mut permissions = Permissions::default();
        for index in 0..2_000 {
            let id = format!("r{index}");
            permissions.observe(&request(&id)).unwrap();
            permissions.deny(&id, false).unwrap();
        }
        assert!(permissions.observe(&request("r7")).is_err());
    }
    #[test]
    fn cancel_close_and_duplicate_prevent_replies() {
        let mut permissions = Permissions::default();
        permissions.observe(&request("a")).unwrap();
        permissions
            .observe(&input(
                json!({"type":"control_cancel_request","request_id":"a"}),
            ))
            .unwrap();
        assert!(permissions.deny("a", false).is_err());
        assert!(permissions.observe(&request("a")).is_err());
        assert!(permissions.observe(&request("b")).is_err());
        let mut permissions = Permissions::default();
        permissions.observe(&request("b")).unwrap();
        permissions.close();
        assert!(permissions.deny("b", false).is_err());
    }
}
