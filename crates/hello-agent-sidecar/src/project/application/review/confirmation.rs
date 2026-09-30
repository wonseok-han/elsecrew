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
}

// No serialization or persisted approval: a new owner always starts empty.
#[allow(dead_code)]
impl Confirmations {
    pub(in crate::project) fn cancel(&mut self) {
        self.pending = None;
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
