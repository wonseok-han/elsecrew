//! Explicit project-scoped consent. Never inferred from login, a prompt, or a tool request.
use super::*;

#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AutomationPermissions {
    pub files: bool,
    pub commands: bool,
}
impl ProjectManager {
    pub(crate) fn automation_permissions(
        &self,
        storage: &Storage,
        project_id: &str,
    ) -> Result<AutomationPermissions, ProjectFailure> {
        let project = required_project(storage, project_id)?;
        match project.get("automation_permissions_v1") {
            None => Ok(AutomationPermissions::default()),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|e| ProjectFailure::manifest(e.to_string())),
        }
    }
    pub(crate) fn automation_settings(
        &self,
        storage: &mut Storage,
        project_id: &str,
        change: Option<AutomationPermissions>,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let mut project = required_project(storage, project_id)?;
        let action = if let Some(permissions) = change {
            project.insert(
                "automation_permissions_v1".into(),
                serde_json::to_value(permissions)
                    .map_err(|e| ProjectFailure::manifest(e.to_string()))?,
            );
            append_object(
                storage,
                "project",
                project_id,
                &format!("op_permissions_{request_id}"),
                request_id,
                Value::Object(project),
            )?;
            "setAutomationPermissions"
        } else {
            "getAutomationPermissions"
        };
        Ok(ProjectAction {
            result: json!({"schemaVersion":1,"action":action,"projectId":project_id,"permissions":self.automation_permissions(storage,project_id)?}),
            ..ProjectAction::default()
        })
    }
}
