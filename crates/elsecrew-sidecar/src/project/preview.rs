//! Result preview reads (ADE 1차-나 F12; `docs/proposals/m5-ade-layout-plan.md`
//! 2.3 "sidecar `preview.readAsset` 규칙"). Read-only: nothing here writes,
//! creates or removes anything.
//!
//! - `preview.resolve` takes only the project and the chat. The workspace
//!   address comes from the chat's durable binding (never from the page), and
//!   the full workspace check (including the Git checks) runs once, here.
//! - `preview.readAsset` reads one allow-listed file of that exact workspace.
//!   It re-checks the durable binding, the manifest and the lock on every call
//!   (a discarded, merged-and-cleaned or recovery-marked workspace stops
//!   serving at once), but not the Git repository: the preview never touches
//!   Git and a page may load hundreds of files.
//! - Paths are walked one directory handle at a time without following links
//!   (`open_parent_capability`/`open_real_file_at`, the same no-follow walk as
//!   `file.read`), so a link swapped in after a check still cannot lead out.
//!   Dot segments (`.git`, `.env`, …) and unknown extensions never open.
//! - A missing, refused or oversized file is an answer (`outcome`), not an
//!   error: the app turns it into a 404 for the page.

use super::*;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use elsecrew_protocol::preview_v1::{
    preview_entry_refusal_v1, preview_path_refusal_v1, PREVIEW_ASSET_CHUNK_BYTES_V1,
    PREVIEW_ASSET_MAX_BYTES_V1, PREVIEW_ENTRY_CANDIDATES_V1,
};

impl ProjectManager {
    pub fn preview_resolve(
        &self,
        storage: &Storage,
        project_id: &str,
        chat_id: &str,
        entry: Option<&str>,
    ) -> Result<ProjectAction, ProjectFailure> {
        let address = preview_chat_address(storage, project_id, chat_id)?;
        let active = self.validate_workspace_address(storage, &address)?;
        let root = self.worktree_capability(&active)?;
        let (entry, refusal) = match entry {
            Some(path) => match preview_entry_refusal_v1(path) {
                Some(reason) => (None, Some(reason)),
                None if regular_file(&root, path) => (Some(path.to_string()), None),
                None => (None, Some("notFound")),
            },
            None => (
                PREVIEW_ENTRY_CANDIDATES_V1
                    .iter()
                    .find(|candidate| regular_file(&root, candidate))
                    .map(|candidate| (*candidate).to_string()),
                None,
            ),
        };
        Ok(ProjectAction {
            result: json!({
                "chatId": address.chat_id,
                "projectId": address.project_id,
                "worktreeId": address.worktree_id,
                "workspaceIdentity": address.workspace_identity,
                "entry": entry,
                "refusal": refusal,
            }),
            ..ProjectAction::default()
        })
    }

    pub fn preview_read_asset(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        path: &str,
        offset: u64,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_workspace_address_binding(storage, address)?;
        let answer = |outcome: &str| ProjectAction {
            result: json!({"path": path, "outcome": outcome, "byteLength": Value::Null,
                "version": Value::Null, "offset": offset, "bytesBase64": Value::Null}),
            ..ProjectAction::default()
        };
        if preview_path_refusal_v1(path).is_some() {
            return Ok(answer("refused"));
        }
        let root = self.worktree_capability(&active)?;
        let Ok((parent, name)) = open_parent_capability(&root, path) else {
            return Ok(answer("notFound"));
        };
        let Ok(mut file) = open_real_file_at(&parent, &name) else {
            return Ok(answer("notFound"));
        };
        let (bytes, length, version) = read_file_bounded_handle(&mut file, PREVIEW_ASSET_MAX_BYTES_V1)?;
        let Some(bytes) = bytes else {
            return Ok(answer("tooLarge"));
        };
        if offset > length || (offset == length && length != 0) {
            return Err(ProjectFailure::manifest("preview read offset is past the end of the file"));
        }
        let start = offset as usize;
        let end = (offset + PREVIEW_ASSET_CHUNK_BYTES_V1).min(length) as usize;
        Ok(ProjectAction {
            result: json!({
                "path": path, "outcome": "ok", "byteLength": length, "version": version,
                "offset": offset, "bytesBase64": BASE64.encode(&bytes[start..end]),
            }),
            ..ProjectAction::default()
        })
    }
}

/// The chat's own workspace, from its durable record only.
fn preview_chat_address(
    storage: &Storage,
    project_id: &str,
    chat_id: &str,
) -> Result<WorkspaceAddress, ProjectFailure> {
    let chat = storage
        .projection_record("chat", chat_id)?
        .ok_or_else(|| ProjectFailure::manifest("preview chat is missing"))?;
    if chat.get("project_id").and_then(Value::as_str) != Some(project_id) {
        return Err(ProjectFailure::manifest("preview chat belongs to another project"));
    }
    let field = |name: &str| {
        chat.get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| ProjectFailure::manifest("preview chat has no workspace"))
    };
    Ok(WorkspaceAddress {
        chat_id: chat_id.to_string(),
        project_id: project_id.to_string(),
        worktree_id: field("worktree_id")?,
        workspace_identity: field("workspace_identity")?,
    })
}

fn regular_file(root: &Dir, path: &str) -> bool {
    open_parent_capability(root, path)
        .and_then(|(parent, name)| open_real_file_at(&parent, &name))
        .is_ok()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn manager_root() -> (tempfile::TempDir, Dir) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ws");
        fs::create_dir_all(root.join("css")).unwrap();
        fs::write(root.join("index.html"), "<p>hi</p>").unwrap();
        fs::write(root.join("css/app.css"), "p{}").unwrap();
        let dir = Dir::open_ambient_dir(&root, ambient_authority()).unwrap();
        (temp, dir)
    }

    #[test]
    fn entry_lookup_uses_no_follow_regular_files() {
        let (temp, root) = manager_root();
        let base = temp.path().join("ws");
        assert!(regular_file(&root, "index.html"));
        assert!(regular_file(&root, "css/app.css"));
        assert!(!regular_file(&root, "missing.html"));
        assert!(!regular_file(&root, "css"));
        // A link to a file outside, a link to a folder outside, a chain.
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "outside").unwrap();
        symlink(outside.join("secret.txt"), base.join("leak.txt")).unwrap();
        symlink(&outside, base.join("out")).unwrap();
        symlink(base.join("leak.txt"), base.join("chain.txt")).unwrap();
        symlink(base.join("index.html"), base.join("inside-link.html")).unwrap();
        assert!(!regular_file(&root, "leak.txt"));
        assert!(!regular_file(&root, "out/secret.txt"));
        assert!(!regular_file(&root, "chain.txt"));
        assert!(!regular_file(&root, "inside-link.html"));
        // A FIFO never blocks and is not a file.
        let fifo = base.join("pipe.txt");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(!regular_file(&root, "pipe.txt"));
    }
}
