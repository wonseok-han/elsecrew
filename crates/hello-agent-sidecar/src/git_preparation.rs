//! One bounded preparation request. No database admission, recovery replay,
//! Git process, provider runtime, project selection or normal ready event.
use crate::{project::ProjectManager, require_exact_params, SidecarError};
use serde_json::Value;
use std::io::{Read, Write};

pub(crate) fn run() -> Result<(), SidecarError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            SidecarError::new(
                "invalidPreparation",
                "could not read Git preparation request",
            )
        })?;
    if bytes.len() > 65_536 {
        return Err(SidecarError::new(
            "invalidPreparation",
            "Git preparation request is too large",
        ));
    }
    let request: Value = serde_json::from_slice(&bytes)
        .map_err(|_| SidecarError::new("invalidPreparation", "invalid Git preparation request"))?;
    let invalid = |detail: String| SidecarError::new("invalidPreparation", detail);
    let action = request
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing Git preparation action".into()))?;
    match action {
        "inspectGitTrust" | "revokeGitTrust" => {
            require_exact_params(&request, &["action"]).map_err(invalid)?;
        }
        "confirmGitTrust" => {
            require_exact_params(&request, &["action", "identity", "confirmed"])
                .map_err(invalid)?;
            require_exact_params(
                &request["identity"],
                &[
                    "installationPath",
                    "volumePath",
                    "directoryId",
                    "commonName",
                    "organization",
                ],
            )
            .map_err(invalid)?;
            if request["confirmed"] != true {
                return Err(invalid("Git trust requires explicit confirmation".into()));
            }
        }
        _ => {
            return Err(invalid(
                "operation is not allowed in Git preparation".into(),
            ))
        }
    }
    // The main process supplies one fixed app-owned cwd, never a renderer path.
    // ProjectManager opens only managed directories here; Storage is not opened.
    let root =
        std::env::current_dir().map_err(|_| invalid("app data directory unavailable".into()))?;
    let manager = ProjectManager::new(&root.join(crate::SQLITE_FILE_V1))?;
    let result = match action {
        "inspectGitTrust" => manager.preview_git_trust().map(|inspection| serde_json::json!({"schemaVersion":1,"action":action,"inspection":inspection})),
        "confirmGitTrust" => {
            let identity = serde_json::from_value(request["identity"].clone()).map_err(|_| invalid("invalid Git trust identity".into()))?;
            manager.confirm_git_trust(identity, true).map(|inspection| serde_json::json!({"schemaVersion":1,"action":action,"inspection":inspection}))
        },
        _ => manager.revoke_git_trust().map(|()| serde_json::json!({"schemaVersion":1,"action":action,"revoked":true})),
    }.map_err(|error| SidecarError::new(error.kind, error.detail))?;
    let encoded = serde_json::to_vec(&result)
        .map_err(|_| invalid("could not encode Git preparation result".into()))?;
    if encoded.len() > 65_536 {
        return Err(invalid("Git preparation result exceeds limit".into()));
    }
    std::io::stdout()
        .write_all(&encoded)
        .map_err(|_| invalid("could not return Git preparation result".into()))
}
