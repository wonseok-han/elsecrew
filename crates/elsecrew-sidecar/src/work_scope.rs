//! Which writes a running task blocks (ADE multi-project PR-A, P2′).
//!
//! An agent writes only inside its own task's workspace (Codex: the run
//! sandbox; Claude: the app's tool admission), and every Git write the app
//! makes itself — step commits, merges, discards — runs on this one
//! dispatcher thread. Two tasks therefore never write the same files at the
//! same time, even two tasks of the same repository. A save, merge, discard,
//! cleanup or content erasure waits only for its own task (a save to a
//! project's own view: for the tasks of that project).
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
        let Some(mut chats) = self.agent_runtime.run_chats() else {
            return BusyChats::Unknown;
        };
        chats.extend(self.workflow_chats.iter().cloned());
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

    /// A task of this project is busy. A chat whose record cannot be read
    /// counts as this project's (fail closed).
    fn project_busy(&self, busy: &BusyChats, project: &str) -> bool {
        let BusyChats::Known(chats) = busy else { return true };
        chats.iter().any(|chat| match self.storage.projection_record("chat", chat) {
            Ok(Some(record)) => record.get("project_id").and_then(Value::as_str).is_none_or(|owner| owner == project),
            _ => true,
        })
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
            Target::Chat(chat) => busy.contains(chat),
            Target::Project(project) => self.project_busy(&busy, project),
        };
        if !blocked {
            return None;
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
