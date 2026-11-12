//! Ephemeral explicit-review confirmation. This is not source-write authority.
use super::*;
use std::time::{Duration, Instant};
use uuid::Uuid;

const LIFETIME: Duration = Duration::from_secs(300);

enum Target {
    Project(String),
    Chat(WorkspaceAddress, String),
}

impl Target {
    fn borrowed(&self) -> ReviewTarget<'_> {
        match self {
            Self::Project(id) => ReviewTarget::Project(id),
            Self::Chat(address, task_id) => ReviewTarget::Chat { address, task_id },
        }
    }
}

struct Pending {
    token: String,
    target: Target,
    review_id: String,
    expires: Instant,
}

#[derive(Default)]
pub(in crate::project) struct Confirmations {
    pending: Option<Pending>,
    generation: Option<Uuid>,
}

// Move-only, session-local handoff. It cannot be reconstructed from a ready
// backup record and does not authorize source writes by itself.
pub(in crate::project) struct PreparedApplication {
    pending: Pending,
    generation: Uuid,
    backup: Value,
}

// No serialization or persisted approval: a new owner always starts empty.
#[allow(dead_code)]
impl Confirmations {
    pub(in crate::project) fn has_pending(&self) -> bool { self.pending.is_some() }

    pub(in crate::project) fn project_for_token(
        &self,
        token: &str,
    ) -> Result<String, ProjectFailure> {
        let pending = self.pending.as_ref().ok_or_else(invalid)?;
        if pending.token != token || Instant::now() >= pending.expires {
            return Err(invalid());
        }
        Ok(pending.target.borrowed().project_id().to_owned())
    }
    pub(in crate::project) fn matches_chat(&self, token: &str, project: &str, chat: &str) -> bool {
        self.pending.as_ref().is_some_and(|pending| pending.token == token && Instant::now() < pending.expires
            && matches!(&pending.target, Target::Chat(address, _) if address.project_id == project && address.chat_id == chat))
    }
    pub(in crate::project) fn cancel_token(&mut self, token: &str) -> Result<(), ProjectFailure> {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.token != token)
        {
            return Err(invalid());
        }
        self.cancel();
        Ok(())
    }
    pub(in crate::project) fn cancel(&mut self) {
        self.pending = None;
        self.generation = None;
    }

    pub(in crate::project) fn open(
        &mut self,
        manager: &ProjectManager,
        storage: &Storage,
        target: ReviewTarget<'_>,
        review_id: &str,
    ) -> Result<(String, ReviewPlan), ProjectFailure> {
        self.cancel();
        let plan = persistence::reopen_target(manager, storage, target, review_id)?;
        let target = match target {
            ReviewTarget::Project(id) => Target::Project(id.to_owned()),
            ReviewTarget::Chat { address, task_id } => {
                Target::Chat(address.clone(), task_id.to_owned())
            }
        };
        let token = format!("confirmation_{}", Uuid::new_v4().simple());
        self.pending = Some(Pending {
            token: token.clone(),
            target,
            review_id: review_id.to_owned(),
            expires: Instant::now() + LIFETIME,
        });
        self.generation = Some(Uuid::new_v4());
        Ok((token, plan))
    }

    /// Consumes the attempt even on failure. The caller must still acquire the
    /// application transaction and revalidate before any source write.
    pub(in crate::project) fn confirm(
        &mut self,
        manager: &ProjectManager,
        storage: &Storage,
        token: &str,
    ) -> Result<ReviewPlan, ProjectFailure> {
        let pending = self.pending.take().ok_or_else(invalid)?;
        if token != pending.token || Instant::now() >= pending.expires {
            return Err(invalid());
        }
        let plan = persistence::reopen_target(
            manager,
            storage,
            pending.target.borrowed(),
            &pending.review_id,
        )?;
        if Instant::now() >= pending.expires {
            return Err(invalid());
        }
        Ok(plan)
    }

    pub(in crate::project) fn prepare_backup(
        &mut self,
        manager: &ProjectManager,
        storage: &mut Storage,
        token: &str,
        request_id: &str,
    ) -> Result<PreparedApplication, ProjectFailure> {
        #[cfg(all(test, windows))]
        let diagnostic_started = Instant::now();
        #[cfg(all(test, windows))]
        eprintln!("Windows preparation timing: stage=prepare_enter elapsedMs={}", diagnostic_started.elapsed().as_millis());
        manager.check_application_preparation()?;
        #[cfg(all(test, windows))]
        eprintln!("Windows preparation timing: stage=prepare_checked elapsedMs={}", diagnostic_started.elapsed().as_millis());
        let pending = self.pending.take().ok_or_else(invalid)?;
        if pending.token != token || Instant::now() >= pending.expires {
            return Err(invalid());
        }
        let generation = self.generation.ok_or_else(invalid)?;
        let target = pending.target.borrowed();
        // Consume the user's exact review before doing potentially lengthy backup IO.
        persistence::reopen_target(manager, storage, target, &pending.review_id)?;
        #[cfg(all(test, windows))]
        eprintln!("Windows preparation timing: stage=review_reopened elapsedMs={}", diagnostic_started.elapsed().as_millis());
        let project = crate::project::required_project(storage, target.project_id())?;
        let backup = match project
            .get("application_backups")
            .and_then(|v| v.get(&pending.review_id))
        {
            None => {
                backup::staging::prepare(manager, storage, target, &pending.review_id, request_id)?
            }
            Some(record) if record["state"] == "ready" => {
                backup::staging::resume(manager, storage, target, &pending.review_id, request_id)?
            }
            _ => return Err(invalid()), // Pending recovery needs a separate explicit action.
        };
        #[cfg(all(test, windows))]
        eprintln!("Windows preparation timing: stage=backup_ready elapsedMs={} expired={}", diagnostic_started.elapsed().as_millis(), Instant::now() >= pending.expires);
        manager.check_application_preparation()?;
        if Instant::now() >= pending.expires {
            return Err(invalid());
        }
        Ok(PreparedApplication {
            pending,
            generation,
            backup,
        })
    }

    /// Final read-only handoff to a future application transaction. The writer
    /// must separately acquire project exclusion and validate at the write boundary.
    pub(in crate::project) fn consume_prepared(
        &mut self,
        manager: &ProjectManager,
        storage: &Storage,
        prepared: PreparedApplication,
    ) -> Result<(ReviewPlan, Value), ProjectFailure> {
        #[cfg(all(test, windows))]
        let diagnostic_started = Instant::now();
        #[cfg(all(test, windows))]
        eprintln!("Windows preparation timing: stage=handoff_enter elapsedMs={}", diagnostic_started.elapsed().as_millis());
        manager.check_application_preparation()?;
        #[cfg(all(test, windows))]
        eprintln!("Windows preparation timing: stage=handoff_checked elapsedMs={}", diagnostic_started.elapsed().as_millis());
        if self.generation.take() != Some(prepared.generation)
            || Instant::now() >= prepared.pending.expires
        {
            return Err(invalid());
        }
        let target = prepared.pending.target.borrowed();
        let plan =
            persistence::reopen_target(manager, storage, target, &prepared.pending.review_id)?;
        #[cfg(all(test, windows))]
        eprintln!("Windows preparation timing: stage=handoff_review_reopened elapsedMs={}", diagnostic_started.elapsed().as_millis());
        let current = backup::staging::inspect(
            manager,
            storage,
            target.project_id(),
            &prepared.pending.review_id,
        )?;
        #[cfg(all(test, windows))]
        eprintln!("Windows preparation timing: stage=handoff_backup_checked elapsedMs={} expired={}", diagnostic_started.elapsed().as_millis(), Instant::now() >= prepared.pending.expires);
        if current != prepared.backup || Instant::now() >= prepared.pending.expires {
            return Err(invalid());
        }
        manager.check_application_preparation()?;
        Ok((plan, current))
    }
}

fn invalid() -> ProjectFailure {
    ProjectFailure::dirty("review confirmation is missing, expired, or already consumed")
}

#[cfg(test)]
impl Confirmations {
    pub(in crate::project) fn expire_for_test(&mut self) {
        self.pending.as_mut().unwrap().expires = Instant::now();
    }
}

#[cfg(test)]
impl PreparedApplication {
    pub(in crate::project) fn expire_for_test(&mut self) {
        self.pending.expires = Instant::now();
    }
}
