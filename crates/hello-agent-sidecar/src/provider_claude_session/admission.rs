//! App-owned inputs under the approved signed-parent/official-tool-sandbox model.
//! These checks do not claim to observe effective managed policy or its reloads.
use super::*;
use crate::provider_profile::{open_absolute_directory, same_directory, verify_runtime_files};
use std::path::{Path, PathBuf};

pub(crate) struct Admission {
    workspace: PathBuf,
    home: PathBuf,
    source: PathBuf,
    directories: [cap_std::fs::Dir; 3],
    model: String,
    effort: Option<String>,
    access: ToolAccess,
    policy: String,
}

impl Admission {
    pub(crate) fn new(
        workspace: &Path,
        home: &Path,
        source: &Path,
        model: &str,
        effort: Option<&str>,
        access: ToolAccess,
    ) -> Result<Self, SidecarError> {
        let policy = requested_policy_digest(model, effort, access, home, workspace, source)?;
        let admission = Self {
            workspace: workspace.into(),
            home: home.into(),
            source: source.into(),
            directories: [
                open_absolute_directory(workspace)?,
                open_absolute_directory(home)?,
                open_absolute_directory(source)?,
            ],
            model: model.into(),
            effort: effort.map(str::to_owned),
            access,
            policy,
        };
        admission.revalidate()?;
        Ok(admission)
    }

    fn revalidate(&self) -> Result<(), SidecarError> {
        verify_runtime_files(&self.home)?;
        self.revalidate_bound_inputs()
    }

    fn revalidate_bound_inputs(&self) -> Result<(), SidecarError> {
        for (path, directory) in [&self.workspace, &self.home, &self.source]
            .into_iter()
            .zip(&self.directories)
        {
            if !same_directory(&open_absolute_directory(path)?, directory)? {
                return Err(failure());
            }
        }
        verify_runtime_files(&self.workspace)?;
        if requested_policy_digest(
            &self.model,
            self.effort.as_deref(),
            self.access,
            &self.home,
            &self.workspace,
            &self.source,
        )? != self.policy
        {
            return Err(failure());
        }
        Ok(())
    }

    pub(crate) fn launch(
        &self,
        workspace: &Path,
        home: &Path,
        model: &str,
        effort: Option<&str>,
        access: ToolAccess,
    ) -> Result<(), SidecarError> {
        if workspace != self.workspace
            || home != self.home
            || model != self.model
            || effort != self.effort.as_deref()
            || access != self.access
        {
            return Err(failure());
        }
        self.revalidate()
    }

    pub(crate) fn process(&self, process: &mut LoginProcess<'_>) -> Result<(), SidecarError> {
        // The live signed-parent scope checks profile files with its exact PID.
        // Before launch, revalidate() remains strict and admits no sockets.
        self.revalidate_bound_inputs()?;
        if process.claude_preflight_home()? != self.home
            || !process.matches_claude_scope(
                &self.workspace,
                &self.model,
                self.effort.as_deref(),
                self.access,
            )
        {
            return Err(failure());
        }
        process.tick()
    }

    pub(crate) fn tool(
        &self,
        process: &mut LoginProcess<'_>,
        request: &hello_agent_core::provider::vendor_observation::claude_approval::Request,
    ) -> Result<(), SidecarError> {
        use hello_agent_core::provider::vendor_observation::claude_approval::ToolTarget;
        self.process(process)?;
        match request.target() {
            ToolTarget::File { path, writes } => {
                if *writes && self.access == ToolAccess::ReadOnly {
                    return Err(failure());
                }
                inspect_snapshot_file(&self.workspace, path, request.tool_name == "Write")
            }
            ToolTarget::Search { path } => {
                let path = Path::new(path);
                let full = if path.is_absolute() {
                    path.to_owned()
                } else {
                    self.workspace.join(path)
                };
                let relative = full.strip_prefix(&self.workspace).map_err(|_| failure())?;
                if relative.components().any(|part| match part {
                    std::path::Component::Normal(name) => [".git", ".claude", ".codex", ".agents"]
                        .iter()
                        .any(|blocked| name == *blocked),
                    std::path::Component::CurDir => false,
                    _ => true,
                }) {
                    return Err(failure());
                }
                // No symlink-root traversal. Recursive traversal remains subject
                // to the official restricted file-tool policy, not this check.
                match open_absolute_directory(&full) {
                    Ok(_) => Ok(()),
                    Err(_) if request.tool_name == "Grep" => inspect_snapshot_file(
                        &self.workspace,
                        path.to_str().ok_or_else(failure)?,
                        false,
                    ),
                    Err(error) => Err(error),
                }
            }
            ToolTarget::Shell => {
                let input: serde_json::Value =
                    serde_json::from_str(request.inspect_input()).map_err(|_| failure())?;
                if self.access != ToolAccess::Edit
                    || !matches!(
                        input.get("dangerouslyDisableSandbox"),
                        None | Some(serde_json::Value::Bool(false))
                    )
                {
                    return Err(failure());
                }
                // Explicit user decision; official sandbox stays enabled by the
                // fixed launch request. Never a whole-process confinement claim.
                Ok(())
            }
            ToolTarget::Unsupported => Err(failure()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_binds_role_and_retained_directories() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        let home = root.join("profile");
        let source = root.join("source");
        for path in [&workspace, &home, &source] {
            std::fs::create_dir(path).unwrap();
        }
        let admission = Admission::new(
            &workspace,
            &home,
            &source,
            "opus",
            Some("medium"),
            ToolAccess::Edit,
        )
        .unwrap();
        let defaults =
            Admission::new(&workspace, &home, &source, "opus", None, ToolAccess::Edit).unwrap();
        assert!(defaults
            .launch(&workspace, &home, "opus", None, ToolAccess::Edit)
            .is_ok());
        assert!(defaults
            .launch(&workspace, &home, "opus", Some("medium"), ToolAccess::Edit)
            .is_err());
        assert!(admission
            .launch(&workspace, &home, "opus", None, ToolAccess::Edit)
            .is_err());
        assert_ne!(defaults.policy, admission.policy);
        assert!(admission
            .launch(&workspace, &home, "opus", Some("medium"), ToolAccess::Edit)
            .is_ok());
        assert!(admission
            .launch(
                &workspace,
                &home,
                "sonnet",
                Some("medium"),
                ToolAccess::Edit
            )
            .is_err());
        assert!(admission
            .launch(&workspace, &home, "opus", Some("high"), ToolAccess::Edit)
            .is_err());
        assert!(admission
            .launch(
                &workspace,
                &home,
                "opus",
                Some("medium"),
                ToolAccess::ReadOnly
            )
            .is_err());
        assert!(admission
            .launch(&source, &home, "opus", Some("medium"), ToolAccess::Edit)
            .is_err());
        for path in [&workspace, &home, &source] {
            let moved = path.with_extension("retained");
            std::fs::rename(path, &moved).unwrap();
            std::fs::create_dir(path).unwrap();
            assert!(admission.revalidate().is_err());
            std::fs::remove_dir(path).unwrap();
            std::fs::rename(moved, path).unwrap();
            assert!(admission.revalidate().is_ok());
        }
    }

    #[cfg(unix)]
    #[test]
    fn admission_refuses_late_symlink_in_workspace_or_profile() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        let home = root.join("profile");
        let source = root.join("source");
        for path in [&workspace, &home, &source] {
            std::fs::create_dir(path).unwrap();
        }
        let admission = Admission::new(
            &workspace,
            &home,
            &source,
            "opus",
            Some("medium"),
            ToolAccess::Edit,
        )
        .unwrap();
        for path in [&workspace, &home] {
            let link = path.join("outside");
            std::os::unix::fs::symlink(&source, &link).unwrap();
            assert!(admission.revalidate().is_err());
            std::fs::remove_file(link).unwrap();
            assert!(admission.revalidate().is_ok());
        }
    }
}
