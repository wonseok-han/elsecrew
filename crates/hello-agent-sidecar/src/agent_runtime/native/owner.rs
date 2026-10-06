use super::*;
use preparation::OwnedClaudeRun;

pub(super) enum NativeOwner {
    Codex(Box<OwnedTextRun>),
    Claude(Box<OwnedClaudeRun>),
}

impl NativeOwner {
    pub(super) fn polling_finished(&self) -> bool {
        match self {
            Self::Codex(owner) => owner.polling_finished(),
            Self::Claude(owner) => owner.polling_finished(),
        }
    }

    pub(super) fn connection_id(&self) -> Uuid {
        match self {
            Self::Codex(owner) => owner.connection_id(),
            Self::Claude(owner) => owner.connection_id(),
        }
    }
    pub(super) fn registration_identity(
        &self,
        storage: &Storage,
        projects: &ProjectManager,
    ) -> Result<RunIdentity, SidecarError> {
        match self {
            Self::Codex(owner) => owner.registration_identity(storage, projects),
            Self::Claude(owner) => owner.registration_identity(storage, projects),
        }
    }
    pub(super) fn request_cancel(&self, identity: &RunIdentity) -> Result<(), SidecarError> {
        match self {
            Self::Codex(owner) => owner.request_cancel(identity),
            Self::Claude(owner) => owner.request_cancel(identity),
        }
    }
    pub(super) fn pending_approvals(
        &self,
    ) -> Vec<&hello_agent_core::provider::vendor_observation::codex_approval::PendingApproval> {
        match self {
            Self::Codex(owner) => owner.pending_approvals(),
            Self::Claude(_) => Vec::new(),
        }
    }
    pub(super) fn verify_approval_boundary(
        &mut self,
        storage: &mut Storage,
        projects: &ProjectManager,
    ) -> Result<(), SidecarError> {
        match self {
            Self::Codex(owner) => owner.verify_approval_boundary(storage, projects),
            Self::Claude(_) => Err(SidecarError::new(
                "agentInteractionUnavailable",
                "Claude does not accept Codex approval decisions",
            )),
        }
    }
    pub(super) fn advance(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        handle: &mut impl FnMut(
            &mut Storage,
            crate::provider_preflight_session::TextAction<'_>,
        ) -> Result<(), SidecarError>,
    ) -> Result<Option<()>, SidecarError> {
        match self {
            Self::Codex(owner) => owner
                .advance(storage, projects, handle)
                .map(|outcome| outcome.map(|_| ())),
            Self::Claude(owner) => owner
                .advance(storage, projects, &mut |_, _, event| {
                    use hello_agent_core::provider::vendor_observation::claude_session::Event;
                    match event {
                        Event::Initialized { .. } => Ok(()),
                        _ => Err(SidecarError::new(
                            "agentInteractionUnavailable",
                            "Claude tool/control admission is not connected",
                        )),
                    }
                })
                .map(|finished| finished.then_some(())),
        }
    }
}
