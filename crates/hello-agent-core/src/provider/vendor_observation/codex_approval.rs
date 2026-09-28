//! Bounded pending approval observations, not permission or reply authority.
//! The owner must match a pending tool item and present its full context before
//! any decision. These values contain user data: no Debug/log/ledger serialization.
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::value::RawValue;

#[derive(Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    Text(String),
}

pub enum Action {
    Command {
        kind: CommandKind,
        command: Option<String>,
        cwd: Option<String>,
        approval_id: Option<String>,
        environment_id: Option<String>,
        network: Option<Network>,
    },
    FileChange {
        grant_root: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandKind {
    Command,
    WriteStdin,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub host: String,
    pub protocol: String,
}

pub struct PendingApproval {
    pub request_id: RequestId,
    pub item_id: String,
    pub reason: Option<String>,
    pub started_at_ms: u64,
    pub action: Action,
}

/// Explicit choices only: never session-wide or policy-amending approval.
/// AcceptOnce is a requested UI choice, not a supported sandbox permission.
/// A choice is not admission: the runtime must enforce role/snapshot policy
/// and bind an actual user action before preparing or transmitting a reply.
pub enum Decision {
    AcceptOnce,
    Decline,
    Cancel,
}

/// No Clone/Debug/Deserialize: a fixed response to one pending server request.
pub struct ApprovalReply(String);
impl ApprovalReply {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub(super) fn new(id: &RequestId, decision: Decision) -> Result<Self, ProviderError> {
        let id = match id {
            RequestId::Number(n) => serde_json::json!(n),
            RequestId::Text(s) => serde_json::json!(s),
        };
        let decision = match decision {
            // Approval requests can cross the vendor sandbox. No independent
            // enclosing execution boundary is admitted yet, so even an explicit
            // UI choice cannot authorize this wire reply in the current candidate.
            Decision::AcceptOnce => return Err(invalid()),
            Decision::Decline => "decline",
            Decision::Cancel => "cancel",
        };
        Ok(Self(format!(
            "{}\n",
            serde_json::json!({"id":id,"result":{"decision":decision}})
        )))
    }
}

fn invalid() -> ProviderError {
    ProviderError("invalid or mismatched Codex approval request".into())
}
fn id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

impl PendingApproval {
    /// Expected IDs must come from the same live owner, never this request.
    /// Parsing is deliberately independent of public-chat event projection.
    pub fn parse(
        input: &BoundedObservation,
        thread: &str,
        turn: &str,
    ) -> Result<Self, ProviderError> {
        if !id(thread) || !id(turn) {
            return Err(invalid());
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            id: RequestId,
            method: String,
            params: Box<RawValue>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Scope {
            thread_id: String,
            turn_id: String,
            item_id: String,
            started_at_ms: u64,
            reason: Option<String>,
        }
        let request: Request = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        match &request.id {
            RequestId::Number(value) if *value <= 9_007_199_254_740_991 => {}
            RequestId::Text(value) if id(value) => {}
            _ => return Err(invalid()),
        }
        let scope: Scope = serde_json::from_str(request.params.get()).map_err(|_| invalid())?;
        // Never hide new permission metadata or policy amendments from a future UI.
        // Known optional null amendments are harmless; non-null proposals are unsupported.
        let params: serde_json::Value =
            serde_json::from_str(request.params.get()).map_err(|_| invalid())?;
        let params = params.as_object().ok_or_else(invalid)?;
        for (key, value) in params {
            let common = matches!(
                key.as_str(),
                "threadId" | "turnId" | "itemId" | "startedAtMs" | "reason"
            );
            let action = match request.method.as_str() {
                "item/commandExecution/requestApproval" => matches!(
                    key.as_str(),
                    "kind"
                        | "command"
                        | "cwd"
                        | "approvalId"
                        | "environmentId"
                        | "networkApprovalContext"
                        | "commandActions"
                        | "proposedExecpolicyAmendment"
                        | "proposedNetworkPolicyAmendments"
                ),
                "item/fileChange/requestApproval" => key == "grantRoot",
                _ => false,
            };
            if !(common || action)
                || (matches!(
                    key.as_str(),
                    "proposedExecpolicyAmendment" | "proposedNetworkPolicyAmendments"
                ) && !value.is_null())
            {
                return Err(invalid());
            }
        }
        if scope.thread_id != thread
            || scope.turn_id != turn
            || !id(&scope.item_id)
            || scope.started_at_ms > 9_007_199_254_740_991
            || scope.reason.as_ref().is_some_and(|s| s.len() > 16 * 1024)
        {
            return Err(invalid());
        }
        let action = match request.method.as_str() {
            "item/commandExecution/requestApproval" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Command {
                    kind: CommandKind,
                    command: Option<String>,
                    cwd: Option<String>,
                    approval_id: Option<String>,
                    environment_id: Option<String>,
                    network_approval_context: Option<Network>,
                }
                let value: Command =
                    serde_json::from_str(request.params.get()).map_err(|_| invalid())?;
                if value
                    .command
                    .as_ref()
                    .is_some_and(|s| s.len() > 64 * 1024 || s.contains('\0'))
                    || value
                        .cwd
                        .as_ref()
                        .is_some_and(|s| s.len() > 16 * 1024 || s.chars().any(char::is_control))
                    || value.approval_id.as_ref().is_some_and(|s| !id(s))
                    || value.environment_id.as_ref().is_some_and(|s| !id(s))
                    || value.network_approval_context.as_ref().is_some_and(|n| {
                        n.host.is_empty()
                            || n.host.len() > 1024
                            || n.host.chars().any(char::is_control)
                            || n.protocol.is_empty()
                            || n.protocol.len() > 64
                            || n.protocol.chars().any(char::is_control)
                    })
                {
                    return Err(invalid());
                }
                Action::Command {
                    kind: value.kind,
                    command: value.command,
                    cwd: value.cwd,
                    approval_id: value.approval_id,
                    environment_id: value.environment_id,
                    network: value.network_approval_context,
                }
            }
            "item/fileChange/requestApproval" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct File {
                    grant_root: Option<String>,
                }
                let value: File =
                    serde_json::from_str(request.params.get()).map_err(|_| invalid())?;
                if value
                    .grant_root
                    .as_ref()
                    .is_some_and(|s| s.len() > 16 * 1024 || s.chars().any(char::is_control))
                {
                    return Err(invalid());
                }
                Action::FileChange {
                    grant_root: value.grant_root,
                }
            }
            _ => return Err(invalid()),
        };
        Ok(Self {
            request_id: request.id,
            item_id: scope.item_id,
            reason: scope.reason,
            started_at_ms: scope.started_at_ms,
            action,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn base() -> Value {
        json!({"id":"req-1","method":"item/commandExecution/requestApproval","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"item-1","startedAtMs":1,"kind":"command","command":"echo PRIVATE","cwd":"/snapshot","environmentId":null}})
    }
    fn parse(value: Value) -> Result<PendingApproval, ProviderError> {
        PendingApproval::parse(
            &BoundedObservation::new(value.to_string().into_bytes()).unwrap(),
            "thread-1",
            "turn-1",
        )
    }
    #[test]
    fn separates_command_stdin_network_and_file_context_without_deciding() {
        for case in ["command", "stdin", "network", "file"] {
            let mut value = base();
            match case {
                "stdin" => {
                    value["params"]["kind"] = json!("writeStdin");
                    value["params"]["approvalId"] = json!("callback-1");
                }
                "network" => {
                    value["params"]["networkApprovalContext"] =
                        json!({"host":"example.com","protocol":"https"});
                    value["params"]["command"] = Value::Null;
                }
                "file" => {
                    value["method"] = json!("item/fileChange/requestApproval");
                    for key in ["kind", "command", "cwd", "environmentId"] {
                        value["params"].as_object_mut().unwrap().remove(key);
                    }
                    value["params"]["grantRoot"] = json!("/other-root");
                }
                _ => {}
            }
            let parsed = parse(value).unwrap();
            assert_eq!(parsed.item_id, "item-1");
            match (case, parsed.action) {
                (
                    "command",
                    Action::Command {
                        kind: CommandKind::Command,
                        command: Some(s),
                        ..
                    },
                ) => assert_eq!(s, "echo PRIVATE"),
                (
                    "stdin",
                    Action::Command {
                        kind: CommandKind::WriteStdin,
                        approval_id: Some(s),
                        ..
                    },
                ) => assert_eq!(s, "callback-1"),
                (
                    "network",
                    Action::Command {
                        network: Some(n), ..
                    },
                ) => assert_eq!(n.host, "example.com"),
                (
                    "file",
                    Action::FileChange {
                        grant_root: Some(s),
                    },
                ) => assert_eq!(s, "/other-root"),
                _ => panic!("wrong kind"),
            }
        }
    }
    #[test]
    fn rejects_cross_run_bad_ids_notification_shapes_and_unknown_methods() {
        for case in [
            "thread",
            "turn",
            "item",
            "id",
            "stamp",
            "method",
            "notification",
            "result",
            "kind",
            "network",
            "extra-permission",
            "policy",
        ] {
            let mut value = base();
            match case {
                "thread" => value["params"]["threadId"] = json!("other"),
                "turn" => value["params"]["turnId"] = json!("other"),
                "item" => value["params"]["itemId"] = json!(""),
                "id" => value["id"] = json!(-1),
                "stamp" => value["params"]["startedAtMs"] = json!(9_007_199_254_740_992u64),
                "method" => value["method"] = json!("item/permissions/requestApproval"),
                "notification" => {
                    value.as_object_mut().unwrap().remove("id");
                }
                "result" => value["result"] = json!({}),
                "kind" => value["params"]["kind"] = json!("unknown"),
                "network" => {
                    value["params"]["networkApprovalContext"] =
                        json!({"host":"x","protocol":"https","hidden":true})
                }
                "extra-permission" => {
                    value["params"]["additionalPermissions"] = json!({"network":true})
                }
                "policy" => value["params"]["proposedExecpolicyAmendment"] = json!(["sh"]),
                _ => {}
            }
            let error = parse(value).err().unwrap();
            assert!(!error.to_string().contains("PRIVATE"));
        }
    }
    #[test]
    fn duplicate_scope_or_action_fields_are_rejected() {
        for key in ["threadId", "command", "kind"] {
            let raw = base().to_string();
            let marker = format!("\"{key}\":");
            let duplicated = raw.replacen(&marker, &format!("\"{key}\":\"PRIVATE\",{marker}"), 1);
            assert!(PendingApproval::parse(
                &BoundedObservation::new(duplicated.into_bytes()).unwrap(),
                "thread-1",
                "turn-1"
            )
            .is_err());
        }
    }
}
