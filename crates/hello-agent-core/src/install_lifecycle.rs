//! Installation lifecycle, independent of the installer process and UI.
//! The worker supplies observed outcomes; none of these transitions execute a
//! command or prove that a process has stopped. The owner must retain its durable
//! recovery fence until cleanup and installation validation have both completed.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallPhase {
    AwaitingConsent,
    Downloading,
    Installing,
    Verifying,
    Stopping,
    Installed,
    Canceled,
    Failed,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallFailure {
    Download,
    Installer,
    Verification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidInstallTransition;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallLifecycle {
    phase: InstallPhase,
    failure: Option<InstallFailure>,
    canceled: bool,
}

impl Default for InstallLifecycle {
    fn default() -> Self {
        Self {
            phase: InstallPhase::AwaitingConsent,
            failure: None,
            canceled: false,
        }
    }
}

impl InstallLifecycle {
    pub fn phase(&self) -> InstallPhase {
        self.phase
    }
    pub fn failure(&self) -> Option<InstallFailure> {
        self.failure
    }

    /// Call only after the owner validates and consumes consent for this exact
    /// provider, destination and plan. This state is not itself a consent token.
    pub fn accept_consent(&mut self) -> Result<(), InvalidInstallTransition> {
        self.require(InstallPhase::AwaitingConsent)?;
        self.phase = InstallPhase::Downloading;
        Ok(())
    }

    /// Download completion includes confirmed downloader termination. A worker
    /// with unknown termination must call require_recovery instead.
    pub fn downloaded(&mut self, verified: bool) -> Result<(), InvalidInstallTransition> {
        self.require(InstallPhase::Downloading)?;
        if verified {
            self.phase = InstallPhase::Installing;
        } else {
            self.failure = Some(InstallFailure::Download);
            self.phase = InstallPhase::Stopping;
        }
        Ok(())
    }

    /// An installer exit code alone cannot reach Installed. The owner must also
    /// confirm its descendants have stopped before recording this event.
    pub fn installer_exited(&mut self, success: bool) -> Result<(), InvalidInstallTransition> {
        self.require(InstallPhase::Installing)?;
        if success {
            self.phase = InstallPhase::Verifying;
        } else {
            self.failure = Some(InstallFailure::Installer);
            self.phase = InstallPhase::Stopping;
        }
        Ok(())
    }

    /// Verification includes the installed executable pair and expected target,
    /// not authentication or permission to start an agent/model task.
    pub fn verified(&mut self, success: bool) -> Result<(), InvalidInstallTransition> {
        self.require(InstallPhase::Verifying)?;
        if !success {
            self.failure = Some(InstallFailure::Verification);
        }
        self.phase = InstallPhase::Stopping;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), InvalidInstallTransition> {
        match self.phase {
            InstallPhase::AwaitingConsent => {
                self.canceled = true;
                self.phase = InstallPhase::Canceled;
            }
            InstallPhase::Downloading
            | InstallPhase::Installing
            | InstallPhase::Verifying
            | InstallPhase::Stopping => {
                self.canceled = true;
                self.phase = InstallPhase::Stopping;
            }
            InstallPhase::Canceled => {}
            _ => return Err(InvalidInstallTransition),
        }
        Ok(())
    }

    /// Cleanup includes confirmed process termination and durable transaction
    /// finalization. False is sticky: a late success cannot clear recovery.
    pub fn cleanup_finished(&mut self, confirmed: bool) -> Result<(), InvalidInstallTransition> {
        self.require(InstallPhase::Stopping)?;
        self.phase = if !confirmed {
            InstallPhase::RecoveryRequired
        } else if self.failure.is_some() {
            InstallPhase::Failed
        } else if self.canceled {
            InstallPhase::Canceled
        } else {
            InstallPhase::Installed
        };
        Ok(())
    }

    pub fn require_recovery(&mut self) -> Result<(), InvalidInstallTransition> {
        match self.phase {
            InstallPhase::Downloading
            | InstallPhase::Installing
            | InstallPhase::Verifying
            | InstallPhase::Stopping
            | InstallPhase::RecoveryRequired => {
                self.phase = InstallPhase::RecoveryRequired;
                Ok(())
            }
            _ => Err(InvalidInstallTransition),
        }
    }

    pub fn can_release_owner(&self) -> bool {
        matches!(
            self.phase,
            InstallPhase::AwaitingConsent
                | InstallPhase::Installed
                | InstallPhase::Canceled
                | InstallPhase::Failed
        )
    }

    fn require(&self, phase: InstallPhase) -> Result<(), InvalidInstallTransition> {
        if self.phase == phase {
            Ok(())
        } else {
            Err(InvalidInstallTransition)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_verified_and_cleaned_installation_can_succeed() {
        let mut install = InstallLifecycle::default();
        assert!(install.downloaded(true).is_err());
        install.accept_consent().unwrap();
        assert!(install.accept_consent().is_err());
        install.downloaded(true).unwrap();
        assert!(install.cleanup_finished(true).is_err());
        install.installer_exited(true).unwrap();
        assert!(!install.can_release_owner());
        install.verified(true).unwrap();
        assert_eq!(install.phase(), InstallPhase::Stopping);
        install.cleanup_finished(true).unwrap();
        assert_eq!(install.phase(), InstallPhase::Installed);
        assert!(install.accept_consent().is_err());
    }

    #[test]
    fn cancel_waits_for_cleanup_and_cannot_hide_failure() {
        let mut install = InstallLifecycle::default();
        install.accept_consent().unwrap();
        install.cancel().unwrap();
        assert!(install.downloaded(true).is_err());
        assert!(!install.can_release_owner());
        install.cleanup_finished(true).unwrap();
        assert_eq!(install.phase(), InstallPhase::Canceled);
        let mut failed = InstallLifecycle::default();
        failed.accept_consent().unwrap();
        failed.downloaded(false).unwrap();
        failed.cancel().unwrap();
        failed.cleanup_finished(true).unwrap();
        assert_eq!(failed.phase(), InstallPhase::Failed);
        assert_eq!(failed.failure(), Some(InstallFailure::Download));
    }

    #[test]
    fn uncertainty_is_sticky_even_after_late_success() {
        let mut install = InstallLifecycle::default();
        install.accept_consent().unwrap();
        install.downloaded(true).unwrap();
        install.require_recovery().unwrap();
        assert!(install.installer_exited(true).is_err());
        assert!(install.cleanup_finished(true).is_err());
        assert!(install.cancel().is_err());
        assert!(!install.can_release_owner());
    }
}
