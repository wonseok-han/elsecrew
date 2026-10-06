//! Trusted signed Codex parent with provider-enforced named tool permissions.
//! This is not the legacy outer sandbox and does not confine parent networking.
use super::*;
use crate::provider_executable::PinnedExecutable;
use std::path::{Path, PathBuf};

pub(crate) struct Scope {
    workspace: PathBuf,
    home: PathBuf,
    permission: ExpectedPermission,
    executable: PinnedExecutable,
    helper: PinnedExecutable,
}

impl Scope {
    pub(crate) fn matches(
        &self,
        workspace: &Path,
        home: &Path,
        permission: ExpectedPermission,
    ) -> bool {
        self.workspace == workspace
            && self.home == home
            && self.permission == permission
            && self.revalidate().is_ok()
    }
    pub(crate) fn revalidate(&self) -> Result<(), SidecarError> {
        self.executable.revalidate()?;
        self.helper.revalidate()?;
        crate::provider_profile::verify_runtime_files(&self.home)
    }
}

pub(crate) struct CodexCommand {
    command: std::process::Command,
    scope: Scope,
}

impl CodexCommand {
    pub(crate) fn new(
        executable: &Path,
        helper: &Path,
        home: &Path,
        workspace: &Path,
        expected: &hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile<
            '_,
        >,
        permission: ExpectedPermission,
    ) -> Result<Self, SidecarError> {
        let executable = PinnedExecutable::open_codex_signed(executable, false)?;
        let helper = PinnedExecutable::open_codex_signed(helper, true)?;
        let mut args = super::named_discovery_arguments(home, workspace, expected, permission)
            .map_err(|_| failure())?;
        args.push("--strict-config".into());
        let borrowed: Vec<_> = args.iter().map(|arg| arg.as_os_str()).collect();
        let mut command =
            crate::provider_login_gate::command(executable.revalidate()?, &borrowed, workspace)
                .map_err(|_| failure())?;
        super::discovery_environment(&mut command, home);
        let scope = Scope {
            workspace: workspace.into(),
            home: home.into(),
            permission,
            executable,
            helper,
        };
        scope.revalidate()?;
        Ok(Self { command, scope })
    }

    pub(crate) fn launch(
        self,
        owner: &mut LoginProcess<'_>,
        release: impl FnOnce(
            &mut std::process::Child,
            &[hello_agent_core::proctree::ProcIdentity],
        ) -> std::io::Result<()>,
    ) -> Result<(), SidecarError> {
        owner.launch_named_codex(self.command, self.scope, release)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_binary_cannot_claim_signed_codex_launch_scope() {
        let profile =
            hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile {
                model: "test-model",
                reasoning_effort: "medium",
            };
        assert!(CodexCommand::new(
            Path::new("/usr/bin/true"),
            Path::new("/usr/bin/true"),
            Path::new("/synthetic/profile"),
            Path::new("/synthetic/snapshot"),
            &profile,
            ExpectedPermission::ReadOnly
        )
        .is_err());
    }
}
