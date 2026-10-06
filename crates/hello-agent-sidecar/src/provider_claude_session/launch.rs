//! Signed parent invocation; not evidence of tool confinement or managed policy.
use super::{scoped_arguments, failure, ToolAccess};
use crate::{provider_executable::PinnedExecutable, provider_login_process::LoginProcess, SidecarError};
use std::{path::{Path, PathBuf}, process::{Child, Command}};

pub(crate) struct Scope {
    workspace: PathBuf,
    home: PathBuf,
    model: String,
    effort: String,
    access: ToolAccess,
    executable: PinnedExecutable,
}

impl Scope {
    pub(crate) fn revalidate(&self) -> Result<(), SidecarError> {
        self.executable.revalidate()?;
        crate::provider_profile::verify_runtime_files(&self.home)
    }

    pub(crate) fn matches(&self, workspace: &Path, home: &Path, model: &str, effort: &str, access: ToolAccess) -> bool {
        self.workspace == workspace && self.home == home && self.model == model
            && self.effort == effort && self.access == access && self.revalidate().is_ok()
    }
}

pub(crate) struct ClaudeCommand {
    command: Command,
    scope: Scope,
}

impl ClaudeCommand {
    pub(crate) fn new(executable: &Path, home: &Path, workspace: &Path, source: &Path, model: &str, effort: &str, access: ToolAccess) -> Result<Self, SidecarError> {
        let executable = PinnedExecutable::open_claude_signed(executable)?;
        let user_home = PathBuf::from(std::env::var_os("HOME").ok_or_else(failure)?);
        if !user_home.is_absolute() || user_home == home || !home.is_absolute() || !workspace.is_absolute()
            || home.starts_with(workspace) || workspace.starts_with(home)
            || executable.revalidate()?.starts_with(workspace)
        { return Err(failure()); }
        let args = scoped_arguments(model, effort, access, home, workspace, source)?;
        let borrowed: Vec<_> = args.iter().map(|arg| arg.as_os_str()).collect();
        let mut command = crate::provider_login_gate::command(executable.revalidate()?, &borrowed, workspace)
            .map_err(|_| failure())?;
        // Keep the OS HOME for Keychain, isolate only Claude's config namespace.
        // The gate clears the inherited environment; no API keys/proxies are copied.
        command.env("HOME", user_home)
            .env("CLAUDE_CONFIG_DIR", home)
            .env("TMPDIR", home.join("tmp"))
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("BROWSER", "/usr/bin/false")
            .env("DISABLE_AUTOUPDATER", "1")
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
        let scope = Scope {workspace:workspace.into(),home:home.into(),model:model.into(),effort:effort.into(),access,executable};
        scope.revalidate()?;
        Ok(Self {command,scope})
    }

    pub(crate) fn launch(self, owner: &mut LoginProcess<'_>, release: impl FnOnce(&mut Child, &[hello_agent_core::proctree::ProcIdentity]) -> std::io::Result<()>) -> Result<(), SidecarError> {
        owner.launch_claude(self.command, self.scope, release)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_binary_cannot_claim_claude_launch() {
        assert!(ClaudeCommand::new(Path::new("/usr/bin/true"), Path::new("/synthetic/profile"),
            Path::new("/synthetic/snapshot"), Path::new("/synthetic/source"), "test-model", "medium", ToolAccess::ReadOnly).is_err());
    }
}
