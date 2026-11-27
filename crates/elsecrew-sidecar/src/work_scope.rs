//! Which writes a running task blocks (ADE multi-project PR-A, P2′).
//!
//! An agent writes only inside its own task's workspace (Codex: the run
//! sandbox; Claude: the app's tool admission), and every Git write the app
//! makes itself — step commits, merges, discards — runs on this one
//! dispatcher thread. A save, merge, discard, cleanup or content erasure
//! therefore waits only for the busy tasks whose real write paths meet its
//! own (PR #18 review B1): the same repository's Git folder, or folders
//! inside one another — a workspace, a project's own folder — links
//! resolved (`project::WriteScope`). Tasks of one repository so share one
//! lock; tasks of separate repositories in separate folders go on side by
//! side. A path that cannot be resolved meets everything.
//!
//! Writing a copy into the user's own folder (apply, restore, retire, backup
//! cleanup, latest-wins apply) keeps the app-wide rule: it waits while any
//! team works, and `application_running` still owns the one write slot.
//!
//! Anything in flight that cannot be tied to a task makes every task count as
//! busy (fail closed), which is the earlier app-wide rule.
use super::*;

/// Applying to the user's own folder from a copy: still app-wide.
pub(crate) const SOURCE_APPLICATION_ACTIONS: [&str; 6] = [
    "openApplicationConfirmation",
    "startApplication",
    "restoreApplication",
    "retireApplication",
    "cleanupBackup",
    "applyChatLatest",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BusyChats {
    /// Exactly these tasks have a team step, a run, or a start/stop in flight.
    Known(BTreeSet<String>),
    /// Something in flight belongs to no known task: every task is busy.
    Unknown,
}

impl BusyChats {
    pub(crate) fn contains(&self, chat: &str) -> bool {
        match self {
            Self::Unknown => true,
            Self::Known(chats) => chats.contains(chat),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(self, Self::Known(chats) if chats.is_empty())
    }
}

impl Sidecar {
    /// Tasks whose workspace something may be writing now.
    pub(crate) fn busy_chats(&self) -> BusyChats {
        // A build hold this engine could not confirm after a restart: any
        // task may be written (PR #25 review B2, preview_build_holds.rs).
        if self.preview_build_holds.unknown {
            return BusyChats::Unknown;
        }
        let Some(mut chats) = self.agent_runtime.run_chats() else {
            return BusyChats::Unknown;
        };
        chats.extend(self.workflow_chats.iter().cloned());
        // A build-then-preview job writes its task's workspace too (PR #22
        // re-review), and so may one an earlier app run left (PR #25 review B2).
        chats.extend(self.preview_build_holds.chats().cloned());
        chats.extend(self.g5.active_chat_ids());
        chats.extend(self.pending_agent_starts.values().map(|start| start.input.chat_id.clone()));
        chats.extend(self.pending_agent_cancels.values().map(|(_, identity)| identity.chat_id.clone()));
        for request in self.pending_agent_capabilities.values() {
            // A model list for the settings has no task and runs in a
            // scratch folder of its own; a task's check names its task.
            match request.params.get("chatId") {
                None | Some(Value::Null) => {}
                Some(Value::String(chat)) => {
                    chats.insert(chat.clone());
                }
                Some(_) => return BusyChats::Unknown,
            }
        }
        BusyChats::Known(chats)
    }

    /// The workspace address a task's record names.
    fn chat_address(&self, chat: &str) -> Option<project::WorkspaceAddress> {
        let record = self.storage.projection_record("chat", chat).ok()??;
        let field = |key: &str| record["record"][key].as_str().map(str::to_owned);
        Some(project::WorkspaceAddress {
            chat_id: field("chatId")?,
            project_id: field("projectId")?,
            worktree_id: field("worktreeId")?,
            workspace_identity: field("workspaceIdentity")?,
        })
    }

    /// The write scope of each busy task; `None` when any cannot be resolved.
    fn busy_scopes(&self, busy: &BusyChats) -> Option<Vec<project::WriteScope>> {
        let BusyChats::Known(chats) = busy else { return None };
        chats.iter().map(|chat| {
            let address = self.chat_address(chat)?;
            self.projects.task_write_scope(&self.storage, &address).ok()
        }).collect()
    }

    /// Whether a write to `target` meets something a busy task may be writing.
    fn scope_busy(&self, busy: &BusyChats, target: Option<project::WriteScope>) -> bool {
        if busy.is_empty() {
            return false;
        }
        let (Some(target), Some(scopes)) = (target, self.busy_scopes(busy)) else { return true };
        scopes.iter().any(|scope| scope.meets(&target))
    }

    /// Why this write must wait for a running task — the error code each
    /// request has always answered with, and a detail — or `None` to go
    /// ahead. Requests this gate does not know pass to their own checks.
    pub(crate) fn write_refusal(&self, request: &RequestV1) -> Option<(&'static str, &'static str)> {
        let params = &request.params;
        let chat_of = |value: &Value| value.get("chatId").and_then(Value::as_str).map(str::to_owned);
        let target: Option<Target> = match request.method.as_str() {
            // Every step of a workspace save names its task.
            "workspace.file.save" => Some(chat_of(params).map_or(Target::Everything, Target::Chat)),
            // A save to a project's own view: its first step names the project,
            // the later ones name the save it began.
            "file.save" => Some(match params.get("projectId").and_then(Value::as_str) {
                Some(project) => Target::Project(project.to_owned()),
                None => match params.get("saveId").and_then(Value::as_str) {
                    Some(save) => match self.projects.pending_save_target(save) {
                        Some((_, Some(chat))) => Target::Chat(chat),
                        Some((project, None)) => Target::Project(project),
                        // Not a save in progress: its own check refuses it.
                        None => return None,
                    },
                    None => Target::Everything,
                },
            }),
            "project.catalog" => match params.get("action").and_then(Value::as_str) {
                Some(action) if SOURCE_APPLICATION_ACTIONS.contains(&action) => Some(Target::Everything),
                Some("inspectChatDiscard" | "mergeChatWorkspace" | "cleanupWorkspace" | "eraseChatContent") => {
                    Some(chat_of(params).map_or(Target::Everything, Target::Chat))
                }
                Some("discardChatWorkspace") => {
                    Some(params.get("inspection").and_then(chat_of).map_or(Target::Everything, Target::Chat))
                }
                _ => None,
            },
            _ => None,
        };
        let target = target?;
        let busy = self.busy_chats();
        let blocked = match &target {
            Target::Everything => !busy.is_empty(),
            Target::Chat(chat) => busy.contains(chat) || self.scope_busy(&busy,
                self.chat_address(chat).and_then(|address| self.projects.task_write_scope(&self.storage, &address).ok())),
            Target::Project(project) => self.scope_busy(&busy, self.projects.project_write_scope(&self.storage, project).ok()),
        };
        if !blocked {
            return None;
        }
        // Possibly an unconfirmed build: say so, and how it is released.
        if self.preview_build_holds.unconfirmed() {
            return Some(("orchestrationBusy", preview_build_holds::BLOCKED_DETAIL));
        }
        Some(match (request.method.as_str(), params.get("action").and_then(Value::as_str)) {
            (_, Some("inspectChatDiscard" | "discardChatWorkspace")) => ("requestBlocked", "workspace discard requires idle native execution of this task"),
            (_, Some("mergeChatWorkspace")) => ("requestBlocked", "merging requires this task's agents to be idle"),
            (_, Some("cleanupWorkspace" | "eraseChatContent")) => ("invalidParams", "this task's agents must be idle first"),
            _ if matches!(target, Target::Everything) => ("orchestrationBusy", "finish or stop the team task before changing its workspace or applying to the original"),
            _ => ("orchestrationBusy", "finish or stop this task's team before changing its workspace"),
        })
    }
}

enum Target {
    Chat(String),
    Project(String),
    Everything,
}

impl Sidecar {
    /// Reserves a task's workspace for a build-then-preview job (PR #22
    /// re-review): refused while anything may write there — its own team or
    /// run, a start or stop in flight, another build, or any busy task whose
    /// write scope meets it. Held, it keeps team runs and every write
    /// `write_refusal` knows away from that scope until main releases it.
    /// Main names the hold (PR #25 review B1): asked again with the same name
    /// and task (its answer was lost), it answers the same; the hold is
    /// written down before the answer, or refused.
    pub(crate) fn preview_build_reserve(&mut self, project_id: &str, chat_id: &str, hold: &str) -> Result<project::ProjectAction, project::ProjectFailure> {
        if !preview_build_holds::valid_hold(hold) || !preview_build_holds::valid_chat(chat_id) {
            return Err(project::ProjectFailure::manifest("build hold name is invalid"));
        }
        let address = self.chat_address(chat_id)
            .filter(|address| address.project_id == project_id)
            .ok_or_else(|| project::ProjectFailure::manifest("build hold names an unknown task"))?;
        let held = |hold: &str| serde_json::json!({"holdId": hold, "refusal": Value::Null});
        let busy = serde_json::json!({"holdId": Value::Null, "refusal": "busy"});
        match self.preview_build_holds.mine.get(hold) {
            Some(chat) if chat == chat_id => return Ok(project::ProjectAction { result: held(hold), ..project::ProjectAction::default() }),
            Some(_) => return Err(project::ProjectFailure::manifest("build hold name is taken")),
            None if self.preview_build_holds.stale.contains_key(hold) => return Err(project::ProjectFailure::manifest("build hold name is taken")),
            None => {}
        }
        let busy_chats = self.busy_chats();
        let refused = busy_chats.contains(chat_id)
            || self.scope_busy(&busy_chats, self.projects.task_write_scope(&self.storage, &address).ok())
            || self.preview_build_holds.mine.len() >= 64;
        let result = if refused {
            busy
        } else {
            let mut next = self.preview_build_holds.clone();
            next.mine.insert(hold.to_owned(), chat_id.to_owned());
            // Written before main may start the build: an engine restart
            // must find it (preview_build_holds.rs). Unwritten, it is refused.
            if self.build_hold_file.save(&next).is_ok() {
                self.preview_build_holds = next;
                held(hold)
            } else {
                busy
            }
        };
        Ok(project::ProjectAction { result, ..project::ProjectAction::default() })
    }

    /// Frees a hold only once that is written down (PR #25 review I1): an
    /// unwritten release answers an error, and main sends it again.
    pub(crate) fn preview_build_release(&mut self, hold: &str) -> Result<project::ProjectAction, project::ProjectFailure> {
        if !preview_build_holds::valid_hold(hold) {
            return Err(project::ProjectFailure::manifest("build hold name is invalid"));
        }
        let mut next = self.preview_build_holds.clone();
        let released = next.mine.remove(hold).is_some();
        if released {
            self.build_hold_file.save(&next)
                .map_err(|_| project::ProjectFailure::manifest("build hold release could not be recorded"))?;
            self.preview_build_holds = next;
        }
        Ok(project::ProjectAction { result: serde_json::json!({"holdId": hold, "released": released}), ..project::ProjectAction::default() })
    }

    /// Main's own list after an engine start (PR #25 review B2): it replaces
    /// this app's holds (one whose reserve answer was lost and that main
    /// never used goes; one main still uses comes back even when the record
    /// could not be read). Holds of an earlier app run and an unreadable
    /// record stop blocking only once main says every process an earlier
    /// build left is gone. Unwritten, nothing changes and an error answers.
    pub(crate) fn preview_build_sync(&mut self, holds: std::collections::BTreeMap<String, String>, leftovers_settled: bool) -> Result<project::ProjectAction, project::ProjectFailure> {
        if holds.len() > 64 || !holds.iter().all(|(hold, chat)| preview_build_holds::valid_hold(hold) && preview_build_holds::valid_chat(chat)) {
            return Err(project::ProjectFailure::manifest("build hold list is invalid"));
        }
        let mut next = preview_build_holds::Holds { mine: holds, ..self.preview_build_holds.clone() };
        if leftovers_settled {
            next.stale.clear();
            next.unknown = false;
        } else {
            next.stale.retain(|hold, _| !next.mine.contains_key(hold));
        }
        self.build_hold_file.save(&next)
            .map_err(|_| project::ProjectFailure::manifest("build holds could not be recorded"))?;
        self.preview_build_holds = next;
        Ok(project::ProjectAction {
            result: serde_json::json!({"held": self.preview_build_holds.mine.len(), "blocking": self.preview_build_holds.unconfirmed()}),
            ..project::ProjectAction::default()
        })
    }

    /// A check or start of a task whose workspace a build holds: answered
    /// before anything of the team is recorded (PR #22 re-review), so the
    /// same message can start the team once the build ends.
    pub(crate) fn preview_build_refusal(&self, request: &RequestV1) -> Option<HandledRequest> {
        if !matches!(request.method.as_str(), "agent.getCapabilities" | "run.startAgent") {
            return None;
        }
        let chat = request.params.get("chatId").and_then(Value::as_str)?;
        self.preview_build_blocks(chat).then(|| HandledRequest::response(error_response(request, "orchestrationBusy", "requestBlocked",
            if self.preview_build_holds.unconfirmed() { preview_build_holds::BLOCKED_DETAIL }
            else { "a build of this task's workspace is running; wait for it or stop it first" })))
    }

    /// Whether a build holds this task's workspace or one its writes meet.
    /// A scope that cannot be resolved meets everything (fail closed).
    pub(crate) fn preview_build_blocks(&self, chat: &str) -> bool {
        if self.preview_build_holds.unknown {
            return true;
        }
        if self.preview_build_holds.chats().next().is_none() {
            return false;
        }
        if self.preview_build_holds.chats().any(|held| held == chat) {
            return true;
        }
        let scope = |chat: &str| self.chat_address(chat).and_then(|address| self.projects.task_write_scope(&self.storage, &address).ok());
        let Some(target) = scope(chat) else { return true };
        self.preview_build_holds.chats().any(|held| scope(held).map_or(true, |held| held.meets(&target)))
    }
}
