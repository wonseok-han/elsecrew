//! Result preview reads (ADE 1차-나 F12; `docs/proposals/m5-ade-layout-plan.md`
//! 2.3 "sidecar `preview.readAsset` 규칙"). Read-only: nothing here writes,
//! creates or removes anything.
//!
//! - `preview.resolve` takes only the project and the chat. The workspace
//!   address comes from the chat's durable binding (never from the page), and
//!   the full workspace check (including the Git checks) runs once, here, on
//!   the directory handle the binding check verified. The entry is looked up
//!   through that same handle. The root's identity is recorded under a new
//!   random `previewHandle`.
//! - `preview.readAsset` reads one allow-listed file of that exact workspace.
//!   Every call re-runs the binding check (durable binding, manifest, lock,
//!   Git link or snapshot marker) on a fresh handle, requires that handle to
//!   be the very directory recorded at resolve (device and inode), and reads
//!   through that verified handle (PR #6 review B1: never a second open of the
//!   path). A discarded, cleaned or recovery-marked workspace, or a folder
//!   swapped in under the same path, stops serving at once.
//! - Paths are walked one directory handle at a time without following links
//!   (`open_parent_capability`/`open_real_file_at`, the same no-follow walk as
//!   `file.read`). Dot segments (`.git`, `.env`, …) and unknown extensions
//!   never open.
//! - Reading is bounded (review I1): a file declared larger than 5 MiB is not
//!   read at all, and a file that grows after opening is read at most one byte
//!   past the limit. A file is read and hashed once; its later chunks come
//!   from that one bounded read.
//! - A missing, refused or oversized file is an answer (`outcome`), not an
//!   error: the app turns it into a 404 for the page.

use super::*;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use elsecrew_protocol::preview_v1::{
    preview_entry_refusal_v1, preview_path_refusal_v1, PREVIEW_ASSET_CHUNK_BYTES_V1,
    PREVIEW_ASSET_MAX_BYTES_V1, PREVIEW_ENTRY_CANDIDATES_V1,
};
use std::collections::VecDeque;

/// Recorded previews kept at once; the app shows one, so older ones only
/// matter for reads still in flight when it switched.
const PREVIEW_ROOTS_MAX: usize = 8;

#[cfg(unix)]
type RootIdentity = (u64, u64);
#[cfg(windows)]
type RootIdentity = windows_fs::Identity;
#[cfg(not(any(unix, windows)))]
type RootIdentity = ();

fn root_identity(directory: &Dir) -> Result<RootIdentity, ProjectFailure> {
    #[cfg(unix)]
    { capability_directory_identity(directory) }
    #[cfg(windows)]
    { windows_fs::identity(directory, true) }
    #[cfg(not(any(unix, windows)))]
    { let _ = directory; Err(ProjectFailure::recovery("result preview is unavailable on this platform")) }
}

struct PreviewRoot {
    handle: String,
    address: WorkspaceAddress,
    identity: RootIdentity,
}

/// One bounded read of a file larger than a chunk, kept until its last chunk
/// is served (or another file is read).
struct PreviewChunks {
    handle: String,
    path: String,
    version: String,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct PreviewRoots {
    roots: VecDeque<PreviewRoot>,
    chunks: Option<PreviewChunks>,
}

impl PreviewRoots {
    fn insert(&mut self, root: PreviewRoot) {
        // The same chat opened again: its earlier preview is replaced.
        self.roots.retain(|entry| entry.address.chat_id != root.address.chat_id);
        if self.roots.len() == PREVIEW_ROOTS_MAX {
            self.roots.pop_front();
        }
        self.roots.push_back(root);
        if self.chunks.as_ref().is_some_and(|chunks| !self.roots.iter().any(|root| root.handle == chunks.handle)) {
            self.chunks = None;
        }
    }

    fn find(&self, handle: &str, address: &WorkspaceAddress) -> Option<&PreviewRoot> {
        self.roots.iter().find(|root| root.handle == handle && &root.address == address)
    }
}

/// What a bounded read found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PreviewFileRead {
    /// Over the limit; `read` is how many bytes were actually read.
    TooLarge { read: u64 },
    Bytes { bytes: Vec<u8>, version: String },
}

/// Never reads more than the limit plus one byte, whatever the file claims
/// or becomes while it is read; nothing at all when it already claims more.
pub(crate) fn read_preview_file<R: Read>(reader: &mut R, declared: u64) -> std::io::Result<PreviewFileRead> {
    if declared > PREVIEW_ASSET_MAX_BYTES_V1 {
        return Ok(PreviewFileRead::TooLarge { read: 0 });
    }
    let mut bytes = Vec::with_capacity(declared as usize);
    reader.take(PREVIEW_ASSET_MAX_BYTES_V1 + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > PREVIEW_ASSET_MAX_BYTES_V1 {
        return Ok(PreviewFileRead::TooLarge { read: bytes.len() as u64 });
    }
    let version = sha256_file_version_v1(&bytes);
    Ok(PreviewFileRead::Bytes { bytes, version })
}

#[cfg(test)]
thread_local! {
    /// Test only: runs once at a named point of a preview call, to swap the
    /// workspace folder at the exact moment a race would.
    static TEST_HOOK: std::cell::RefCell<Option<(&'static str, Box<dyn FnOnce()>)>> = std::cell::RefCell::new(None);
}

#[cfg(test)]
pub(crate) fn set_preview_test_hook(point: &'static str, hook: impl FnOnce() + 'static) {
    TEST_HOOK.with(|slot| *slot.borrow_mut() = Some((point, Box::new(hook))));
}

fn test_point(_point: &'static str) {
    #[cfg(test)]
    TEST_HOOK.with(|slot| {
        let take = slot.borrow().as_ref().is_some_and(|(point, _)| *point == _point);
        if take {
            if let Some((_, hook)) = slot.borrow_mut().take() { hook(); }
        }
    });
}

impl ProjectManager {
    #[cfg(test)]
    pub(crate) fn set_git_worktrees_for_test(&mut self, enabled: bool) {
        self.git_worktrees_enabled = enabled;
    }

    pub fn preview_resolve(
        &mut self,
        storage: &Storage,
        project_id: &str,
        chat_id: &str,
        entry: Option<&str>,
    ) -> Result<ProjectAction, ProjectFailure> {
        let address = preview_chat_address(storage, project_id, chat_id)?;
        let (active, root) = self.validate_workspace_binding_with_lock_handle(
            storage, &address, "active", WorkspaceLockAdmission::Ready,
        )?;
        self.verify_snapshot_repository(&root, &active)?;
        test_point("resolve:verified");
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
        let handle = format!("ph_{}", Uuid::new_v4().simple());
        self.preview_roots.insert(PreviewRoot { handle: handle.clone(), address: address.clone(), identity: root_identity(&root)? });
        Ok(ProjectAction {
            result: json!({
                "chatId": address.chat_id,
                "projectId": address.project_id,
                "worktreeId": address.worktree_id,
                "workspaceIdentity": address.workspace_identity,
                "previewHandle": handle,
                "entry": entry,
                "refusal": refusal,
            }),
            ..ProjectAction::default()
        })
    }

    pub fn preview_read_asset(
        &mut self,
        storage: &Storage,
        handle: &str,
        address: &WorkspaceAddress,
        path: &str,
        offset: u64,
    ) -> Result<ProjectAction, ProjectFailure> {
        let identity = self
            .preview_roots
            .find(handle, address)
            .map(|root| root.identity)
            .ok_or_else(|| ProjectFailure::recovery("preview handle is unknown, expired or for another workspace"))?;
        let (_active, root) = self.validate_workspace_binding_with_lock_handle(
            storage, address, "active", WorkspaceLockAdmission::Ready,
        )?;
        if root_identity(&root)? != identity {
            return Err(ProjectFailure::recovery("workspace folder changed since the preview opened"));
        }
        // Everything below reads through `root`, the handle just verified.
        test_point("read:verified");
        let answer = |outcome: &str| ProjectAction {
            result: json!({"path": path, "outcome": outcome, "byteLength": Value::Null,
                "version": Value::Null, "offset": offset, "bytesBase64": Value::Null}),
            ..ProjectAction::default()
        };
        if preview_path_refusal_v1(path).is_some() {
            return Ok(answer("refused"));
        }
        let chunk = |bytes: &[u8], version: &str| -> Result<ProjectAction, ProjectFailure> {
            let length = bytes.len() as u64;
            if offset > length || (offset == length && length != 0) {
                return Err(ProjectFailure::manifest("preview read offset is past the end of the file"));
            }
            let end = (offset + PREVIEW_ASSET_CHUNK_BYTES_V1).min(length) as usize;
            Ok(ProjectAction {
                result: json!({
                    "path": path, "outcome": "ok", "byteLength": length, "version": version,
                    "offset": offset, "bytesBase64": BASE64.encode(&bytes[offset as usize..end]),
                }),
                ..ProjectAction::default()
            })
        };
        // A later chunk of the file read just before: the same bytes, not a new read.
        if offset > 0 {
            if let Some(cached) = self.preview_roots.chunks.as_ref().filter(|cached| cached.handle == handle && cached.path == path) {
                let action = chunk(&cached.bytes, &cached.version)?;
                if offset + PREVIEW_ASSET_CHUNK_BYTES_V1 >= cached.bytes.len() as u64 {
                    self.preview_roots.chunks = None;
                }
                return Ok(action);
            }
        }
        let Ok((parent, name)) = open_parent_capability(&root, path) else {
            return Ok(answer("notFound"));
        };
        let Ok(mut file) = open_real_file_at(&parent, &name) else {
            return Ok(answer("notFound"));
        };
        let declared = file
            .metadata()
            .map_err(|error| ProjectFailure::manifest(format!("could not inspect preview file: {error}")))?
            .len();
        let read = read_preview_file(&mut file, declared)
            .map_err(|error| ProjectFailure::manifest(format!("could not read preview file: {error}")))?;
        let PreviewFileRead::Bytes { bytes, version } = read else {
            return Ok(answer("tooLarge"));
        };
        let action = chunk(&bytes, &version)?;
        self.preview_roots.chunks = (offset + PREVIEW_ASSET_CHUNK_BYTES_V1 < bytes.len() as u64)
            .then(|| PreviewChunks { handle: handle.to_string(), path: path.to_string(), version, bytes });
        Ok(action)
    }
}

/// The chat's own workspace, from its durable record only.
pub(super) fn preview_chat_address(
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

pub(super) fn regular_file(root: &Dir, path: &str) -> bool {
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
    fn bounded_reads_stop_at_the_limit_whatever_the_file_claims() {
        let max = PREVIEW_ASSET_MAX_BYTES_V1;
        // Claims more than the limit: nothing is read.
        let mut untouched = std::io::repeat(7);
        assert_eq!(read_preview_file(&mut untouched, max + 1).unwrap(), PreviewFileRead::TooLarge { read: 0 });
        // Claims little, keeps growing (an endless reader): one byte past the limit, no more.
        let mut endless = std::io::repeat(7);
        assert_eq!(read_preview_file(&mut endless, 10).unwrap(), PreviewFileRead::TooLarge { read: max + 1 });
        // Exactly the limit is a file.
        let mut exact = std::io::repeat(1).take(max);
        match read_preview_file(&mut exact, max).unwrap() {
            PreviewFileRead::Bytes { bytes, version } => {
                assert_eq!(bytes.len() as u64, max);
                assert_eq!(version, sha256_file_version_v1(&bytes));
            }
            other => panic!("unexpected {other:?}"),
        }
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
