//! Private approval intent binding. An intent is not permission to send AcceptOnce.
//! No raw command/account context is serialized into the run ledger.
use super::*;
use hello_agent_core::provider::vendor_observation::codex_approval::{
    Action, CommandKind, Decision, PendingApproval, RequestId,
};

const MAX_INTENTS: usize = 128;
const FIELD: &str = "approval_intents_internal";

/// Live decisions only: never deserialize or reconstruct from durable intents.
#[derive(Default)]
pub(super) struct ApprovalQueue {
    entries: Vec<(ApprovalTicket, bool)>,
}

impl ApprovalQueue {
    /// Explicit transient inspection, never appended to an event/projection.
    pub(super) fn inspect(
        &mut self,
        identity: &RunIdentity,
        connection: &str,
        requests: &[&PendingApproval],
        id: &str,
    ) -> Result<Value, SidecarError> {
        let ticket = &mut self
            .entries
            .iter_mut()
            .find(|(ticket, queued)| ticket.id == id && !ticket.consumed && !queued)
            .ok_or_else(failure)?
            .0;
        for request in requests {
            if fingerprint(identity, connection, request)? != ticket.digest {
                continue;
            }
            ticket.inspected = true;
            let (command, cwd, grant_root, host, protocol) = match &request.action {
                Action::Command {
                    command,
                    cwd,
                    network,
                    ..
                } => (
                    command.as_deref(),
                    cwd.as_deref(),
                    None,
                    network.as_ref().map(|n| n.host.as_str()),
                    network.as_ref().map(|n| n.protocol.as_str()),
                ),
                Action::FileChange { grant_root } => {
                    (None, None, grant_root.as_deref(), None, None)
                }
            };
            return Ok(
                json!({"approvalId":id,"kind":request_kind(request),"command":command,"cwd":cwd,"reason":request.reason,"grantRoot":grant_root,"host":host,"protocol":protocol}),
            );
        }
        Err(failure())
    }

    pub(super) fn summaries(
        &self,
        identity: &RunIdentity,
        connection: &str,
        requests: &[&PendingApproval],
    ) -> Result<Vec<Value>, SidecarError> {
        let mut summaries = Vec::new();
        for request in requests {
            let digest = fingerprint(identity, connection, request)?;
            if let Some((ticket, _)) = self
                .entries
                .iter()
                .find(|(ticket, queued)| ticket.digest == digest && !ticket.consumed && !queued)
            {
                summaries.push(json!({"approvalId":ticket.id,"kind":request_kind(request)}));
            }
        }
        Ok(summaries)
    }

    pub(super) fn ids(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(ticket, queued)| !ticket.consumed && !queued)
            .map(|(ticket, _)| ticket.id.clone())
            .collect()
    }

    pub(super) fn decline(&mut self, id: &str) -> Result<(), SidecarError> {
        let (_, queued) = self
            .entries
            .iter_mut()
            .find(|(ticket, queued)| ticket.id == id && !ticket.consumed && !queued)
            .ok_or_else(failure)?;
        *queued = true;
        Ok(())
    }

    /// Requires a prior explicit inspection of this exact live fingerprint.
    pub(super) fn validate_acceptance(&self, id: &str) -> Result<(), SidecarError> {
        self.entries
            .iter()
            .any(|(ticket, queued)| {
                ticket.id == id && ticket.inspected && !ticket.consumed && !queued
            })
            .then_some(())
            .ok_or_else(failure)
    }

    pub(super) fn accept_once(&mut self, id: &str) -> Result<(), SidecarError> {
        let (ticket, queued) = self
            .entries
            .iter_mut()
            .find(|(ticket, queued)| {
                ticket.id == id && ticket.inspected && !ticket.consumed && !queued
            })
            .ok_or_else(failure)?;
        ticket.accept = true;
        *queued = true;
        Ok(())
    }

    /// Project consent replaces the per-request click, not delivery-time confinement.
    pub(super) fn poll_preapproved(&mut self, storage: &mut Storage, identity: &RunIdentity, connection: &str, request: &PendingApproval) -> Result<Option<Decision>, SidecarError> {
        let decision = self.poll(storage, identity, connection, request)?;
        if decision.is_some() { return Ok(decision); }
        let digest = fingerprint(identity, connection, request)?;
        let (ticket, queued) = self.entries.iter_mut().find(|(ticket, _)| ticket.digest == digest).ok_or_else(failure)?;
        ticket.inspected = true;
        ticket.accept = true;
        *queued = true;
        self.poll(storage, identity, connection, request)
    }

    pub(super) fn retain_live(
        &mut self,
        identity: &RunIdentity,
        connection: &str,
        requests: &[&PendingApproval],
    ) -> Result<(), SidecarError> {
        let live: Result<HashSet<_>, _> = requests
            .iter()
            .map(|request| fingerprint(identity, connection, request))
            .collect();
        let live = live?;
        self.entries
            .retain(|(ticket, _)| live.contains(&ticket.digest));
        Ok(())
    }

    pub(super) fn poll(
        &mut self,
        storage: &mut Storage,
        identity: &RunIdentity,
        connection: &str,
        request: &PendingApproval,
    ) -> Result<Option<Decision>, SidecarError> {
        let digest = fingerprint(identity, connection, request)?;
        if let Some((ticket, queued)) = self
            .entries
            .iter_mut()
            .find(|(ticket, _)| ticket.digest == digest)
        {
            if *queued {
                ticket.record_decision(storage, identity, connection, request)?;
                return Ok(Some(if ticket.accept {
                    Decision::AcceptOnce
                } else {
                    Decision::Decline
                }));
            }
            return Ok(None);
        }
        if self.entries.len() >= MAX_INTENTS {
            return Err(failure());
        }
        self.entries
            .push((ApprovalTicket::new(identity, connection, request)?, false));
        Ok(None)
    }
}

fn request_kind(request: &PendingApproval) -> &'static str {
    match &request.action {
        Action::FileChange { .. } => "fileChange",
        Action::Command {
            network: Some(_), ..
        } => "network",
        Action::Command {
            kind: CommandKind::WriteStdin,
            ..
        } => "writeStdin",
        Action::Command {
            kind: CommandKind::Command,
            ..
        } => "command",
    }
}

fn valid_entries(entries: &[Value]) -> bool {
    let mut ids = HashSet::new();
    let mut bindings = HashSet::new();
    entries.len() <= MAX_INTENTS
        && entries.iter().all(|entry| {
            let Some(object) = entry.as_object() else {
                return false;
            };
            let Some(id) = entry["id"].as_str() else {
                return false;
            };
            let Some(binding) = entry["binding"].as_str() else {
                return false;
            };
            object.len() == 4
                && id.strip_prefix("approval_").is_some_and(|value| {
                    value.len() == 32
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
                && binding.len() == 64
                && binding
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && matches!(entry["decision"].as_str(), Some("decline" | "acceptOnce"))
                && entry["state"] == "intentRecorded"
                && ids.insert(id)
                && bindings.insert(binding)
        })
}

fn failure() -> SidecarError {
    SidecarError::new(
        "agentApprovalChanged",
        "approval context is no longer current",
    )
}

fn fingerprint(
    identity: &RunIdentity,
    connection: &str,
    request: &PendingApproval,
) -> Result<String, SidecarError> {
    if connection.is_empty() || connection.len() > 128 || connection.chars().any(char::is_control) {
        return Err(failure());
    }
    let request_id = match &request.request_id {
        RequestId::Number(value) => json!({"number":value}),
        RequestId::Text(value) => json!({"text":value}),
    };
    let action = match &request.action {
        Action::Command {
            kind,
            command,
            cwd,
            approval_id,
            environment_id,
            network,
        } => json!({
            "kind":match kind { CommandKind::Command => "command", CommandKind::WriteStdin => "writeStdin" },
            "command":command,"cwd":cwd,"approvalId":approval_id,"environmentId":environment_id,
            "network":network.as_ref().map(|value| json!({"host":value.host,"protocol":value.protocol})),
        }),
        Action::FileChange { grant_root } => json!({"kind":"fileChange","grantRoot":grant_root}),
    };
    let context = json!({"run":identity_value(identity),"connection":connection,
        "thread":request.thread_id,"turn":request.turn_id,"request":request_id,
        "item":request.item_id,"startedAt":request.started_at_ms,"reason":request.reason,
        "commandActions":request.command_actions,"action":action});
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&context).map_err(|_| failure())?)
    ))
}

/// Opaque, process-local and single-use. Never restore a live ticket from disk.
pub(super) struct ApprovalTicket {
    id: String,
    digest: String,
    consumed: bool,
    inspected: bool,
    accept: bool,
}

impl ApprovalTicket {
    pub(super) fn new(
        identity: &RunIdentity,
        connection: &str,
        request: &PendingApproval,
    ) -> Result<Self, SidecarError> {
        Ok(Self {
            id: format!("approval_{}", Uuid::new_v4().simple()),
            digest: fingerprint(identity, connection, request)?,
            consumed: false,
            inspected: false,
            accept: false,
        })
    }

    /// Persist only intent; transport must independently recheck its boundary.
    pub(super) fn record_decision(
        &mut self,
        storage: &mut Storage,
        identity: &RunIdentity,
        connection: &str,
        request: &PendingApproval,
    ) -> Result<(), SidecarError> {
        let decision = if self.accept { "acceptOnce" } else { "decline" };
        self.consume(identity, connection, request, |id, digest| {
            let mut projection =
                actual_run_projection(storage, &identity.run_id).map_err(|_| failure())?;
            let record = projection
                .get("record")
                .and_then(Value::as_object)
                .ok_or_else(failure)?;
            if identity_value(identity)
                .as_object()
                .unwrap()
                .iter()
                .any(|(key, value)| record.get(key) != Some(value))
                || !matches!(
                    projection.get("state").and_then(Value::as_str),
                    Some("preparing" | "running")
                )
            {
                return Err(failure());
            }
            let entries = projection
                .entry(FIELD)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(failure)?;
            if !valid_entries(entries)
                || entries.len() >= MAX_INTENTS
                || entries.iter().any(|entry| entry["binding"] == digest)
            {
                return Err(failure());
            }
            entries.push(
                json!({"id":id,"binding":digest,"decision":decision,"state":"intentRecorded"}),
            );
            storage
                .upsert_projection_records_atomic(vec![ProjectionUpsert {
                    aggregate_type: "run",
                    aggregate_id: identity.run_id.clone(),
                    operation_id: format!("op_{id}"),
                    request_id: None,
                    payload: projection,
                }])
                .map_err(|_| failure())?;
            Ok(())
        })
    }

    fn consume(
        &mut self,
        identity: &RunIdentity,
        connection: &str,
        request: &PendingApproval,
        persist: impl FnOnce(&str, &str) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        if self.consumed {
            return Err(failure());
        }
        // Failed persistence or a mismatched response cannot be retried as a new
        // choice on this ticket. The caller must terminate/recover the run.
        self.consumed = true;
        if fingerprint(identity, connection, request)? != self.digest {
            return Err(failure());
        }
        persist(&self.id, &self.digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> (RunIdentity, PendingApproval) {
        let identity = RunIdentity {
            project_id: "p".into(),
            chat_id: "c".into(),
            task_id: "t".into(),
            worktree_id: "w".into(),
            workspace_identity: "wi".into(),
            run_id: "r".into(),
        };
        let request = PendingApproval::parse(&BoundedObservation::new(json!({"id":1,"method":"item/commandExecution/requestApproval","params":{"threadId":"thread","turnId":"turn","itemId":"item","startedAtMs":1,"kind":"command","command":"echo PRIVATE","cwd":"/snapshot"}}).to_string().into_bytes()).unwrap(), "thread", "turn").unwrap();
        (identity, request)
    }

    #[test]
    fn project_preapproval_excludes_wider_and_network_requests() {
        use crate::project::automation_permissions::AutomationPermissions;
        let (_, mut request) = context();
        let all = AutomationPermissions {files:true,commands:true};
        assert!(super::super::project_auto_approval(all, &request, "/snapshot"));
        assert!(!super::super::project_auto_approval(AutomationPermissions::default(), &request, "/snapshot"));
        assert!(!super::super::project_auto_approval(all, &request, "/other"));
        if let Action::Command {network, ..} = &mut request.action {
            *network = Some(hello_agent_core::provider::vendor_observation::codex_approval::Network {host:"example.test".into(),protocol:"https".into()});
        }
        assert!(!super::super::project_auto_approval(all, &request, "/snapshot"));
        request.action = Action::FileChange {grant_root:Some("/original".into())};
        assert!(!super::super::project_auto_approval(all, &request, "/snapshot"));
        request.action = Action::FileChange {grant_root:None};
        assert!(!super::super::project_auto_approval(all, &request, "/snapshot"));
        request.action = Action::FileChange {grant_root:Some("/snapshot".into())};
        assert!(super::super::project_auto_approval(all, &request, "/snapshot"));
        assert!(!super::super::project_auto_approval(all, &request, ""));
    }

    #[test]
    fn acceptance_requires_inspection_and_is_single_use() {
        let (identity, request) = context();
        let ticket = ApprovalTicket::new(&identity, "connection", &request).unwrap();
        let id = ticket.id.clone();
        let mut queue = ApprovalQueue {
            entries: vec![(ticket, false)],
        };
        assert!(queue.accept_once(&id).is_err());
        queue
            .inspect(&identity, "connection", &[&request], &id)
            .unwrap();
        queue.accept_once(&id).unwrap();
        assert!(queue.ids().is_empty());
        assert!(queue.accept_once(&id).is_err());
        assert!(queue.decline(&id).is_err());
        assert!(queue
            .inspect(&identity, "connection", &[&request], &id)
            .is_err());
        queue
            .retain_live(&identity, "replacement", &[&request])
            .unwrap();
        assert!(queue.entries.is_empty());
    }

    #[test]
    fn inspection_is_explicit_live_and_does_not_consume_a_ticket() {
        let (identity, request) = context();
        let ticket = ApprovalTicket::new(&identity, "connection", &request).unwrap();
        let id = ticket.id.clone();
        let mut queue = ApprovalQueue {
            entries: vec![(ticket, false)],
        };
        let detail = queue
            .inspect(&identity, "connection", &[&request], &id)
            .unwrap();
        assert_eq!(detail["command"], "echo PRIVATE");
        assert_eq!(detail.as_object().unwrap().len(), 8);
        assert_eq!(queue.ids(), vec![id.clone()]);
        assert!(queue.inspect(&identity, "other", &[&request], &id).is_err());
        assert!(queue.inspect(&identity, "connection", &[], &id).is_err());
        assert!(queue
            .inspect(&identity, "connection", &[&request], "unknown")
            .is_err());
        queue.decline(&id).unwrap();
        assert!(queue
            .inspect(&identity, "connection", &[&request], &id)
            .is_err());
    }

    #[test]
    fn summaries_classify_live_requests_without_disclosing_raw_context() {
        for expected in ["command", "writeStdin", "network", "fileChange"] {
            let (identity, mut request) = context();
            if expected == "fileChange" {
                request.action = Action::FileChange {
                    grant_root: Some("/PRIVATE".into()),
                };
            } else if let Action::Command { kind, network, .. } = &mut request.action {
                if expected == "writeStdin" {
                    *kind = CommandKind::WriteStdin;
                }
                if expected == "network" {
                    *network = Some(
                        hello_agent_core::provider::vendor_observation::codex_approval::Network {
                            host: "PRIVATE.example".into(),
                            protocol: "https".into(),
                        },
                    );
                }
            }
            request.reason = Some("PRIVATE reason".into());
            let ticket = ApprovalTicket::new(&identity, "connection", &request).unwrap();
            let id = ticket.id.clone();
            let mut queue = ApprovalQueue {
                entries: vec![(ticket, false)],
            };
            let summary = queue
                .summaries(&identity, "connection", &[&request])
                .unwrap();
            assert_eq!(summary, vec![json!({"approvalId":id,"kind":expected})]);
            assert!(!serde_json::to_string(&summary).unwrap().contains("PRIVATE"));
            assert!(queue
                .summaries(&identity, "other", &[&request])
                .unwrap()
                .is_empty());
            queue.decline(&id).unwrap();
            assert!(queue
                .summaries(&identity, "connection", &[&request])
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn live_queue_rejects_duplicate_unknown_and_expired_decisions() {
        let (identity, request) = context();
        let ticket = ApprovalTicket::new(&identity, "connection", &request).unwrap();
        let id = ticket.id.clone();
        let mut queue = ApprovalQueue {
            entries: vec![(ticket, false)],
        };
        queue
            .retain_live(&identity, "connection", &[&request])
            .unwrap();
        assert_eq!(queue.ids(), vec![id.clone()]);
        assert!(queue.decline("unknown").is_err());
        queue.decline(&id).unwrap();
        assert!(queue.ids().is_empty());
        assert!(queue.decline(&id).is_err());
        queue.retain_live(&identity, "connection", &[]).unwrap();
        assert!(queue.entries.is_empty());
        assert!(queue.decline(&id).is_err());
        let ticket = ApprovalTicket::new(&identity, "connection", &request).unwrap();
        queue.entries.push((ticket, false));
        queue
            .retain_live(&identity, "replacement", &[&request])
            .unwrap();
        assert!(queue.ids().is_empty());
    }

    #[test]
    fn same_stream_cannot_reuse_a_ticket_on_another_process_connection() {
        let (identity, request) = context();
        let stream = format!("agent_stream_{}", Uuid::new_v4().simple());
        let original = format!("{stream}:{}", Uuid::new_v4());
        let replacement = format!("{stream}:{}", Uuid::new_v4());
        let mut ticket = ApprovalTicket::new(&identity, &original, &request).unwrap();
        assert!(ticket
            .consume(&identity, &replacement, &request, |_, _| panic!(
                "wrong process"
            ))
            .is_err());
        assert!(ticket
            .consume(&identity, &original, &request, |_, _| panic!(
                "consumed ticket"
            ))
            .is_err());
    }

    #[test]
    fn stored_intents_are_bounded_exact_and_unique() {
        let entry = json!({"id":format!("approval_{}", "a".repeat(32)),"binding":"b".repeat(64),"decision":"decline","state":"intentRecorded"});
        assert!(valid_entries(std::slice::from_ref(&entry)));
        assert!(!valid_entries(&[entry.clone(), entry.clone()]));
        for key in ["id", "binding", "decision", "state", "extra"] {
            let mut changed = entry.clone();
            changed[key] = json!("invalid");
            assert!(!valid_entries(&[changed]));
        }
        let entries: Vec<_> = (0..MAX_INTENTS).map(|index| json!({"id":format!("approval_{index:032x}"),"binding":format!("{index:064x}"),"decision":"decline","state":"intentRecorded"})).collect();
        assert!(valid_entries(&entries));
        let mut over = entries;
        over.push(entry);
        assert!(!valid_entries(&over));
    }
    #[test]
    fn ticket_is_single_use_and_does_not_authorize_changed_context() {
        for mutation in 0..12 {
            let (mut identity, mut request) = context();
            let mut ticket = ApprovalTicket::new(&identity, "connection", &request).unwrap();
            let mut connection = "connection";
            match mutation {
                1 => identity.run_id.push('x'),
                2 => connection = "other",
                3 => request.turn_id.push('x'),
                4 => request.item_id.push('x'),
                5 => request.started_at_ms += 1,
                6 => request.reason = Some("changed".into()),
                7 => request.thread_id.push('x'),
                8 => request.request_id = RequestId::Text("1".into()),
                9 => request.command_actions = Some(json!([{"type":"unknown"}])),
                10 => {
                    if let Action::Command { command, .. } = &mut request.action {
                        *command = Some("changed".into());
                    }
                }
                11 => {
                    if let Action::Command { cwd, .. } = &mut request.action {
                        *cwd = Some("/other".into());
                    }
                }
                _ => {}
            }
            let mut calls = 0;
            let result = ticket.consume(&identity, connection, &request, |_, digest| {
                calls += 1;
                assert_eq!(digest.len(), 64);
                Ok(())
            });
            assert_eq!(result.is_ok(), mutation == 0);
            assert_eq!(calls, usize::from(mutation == 0));
            assert!(ticket
                .consume(&identity, connection, &request, |_, _| panic!("replay"))
                .is_err());
        }
        let (identity, request) = context();
        let mut ticket = ApprovalTicket::new(&identity, "connection", &request).unwrap();
        assert!(ticket
            .consume(&identity, "connection", &request, |_, _| Err(failure()))
            .is_err());
        assert!(ticket
            .consume(&identity, "connection", &request, |_, _| panic!("retry"))
            .is_err());
    }
}
