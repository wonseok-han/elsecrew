use crate::{AppendOutcome, SidecarError, Storage};
#[cfg(unix)]
use cap_fs_ext::OsMetadataExt;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::ambient_authority;
#[cfg(unix)]
use cap_std::fs::PermissionsExt;
use cap_std::fs::{Dir, File as CapFile, OpenOptions as CapOpenOptions};
use hello_agent_protocol::project_v1::{
    sha256_file_version_v1, validate_project_payload_v1, FILE_MAX_BYTES_V1,
    PROJECT_SCHEMA_VERSION_V1, SNAPSHOT_BUNDLE_MAX_BYTES_V1, SNAPSHOT_CREATION_PHASES_V1,
    SNAPSHOT_DISK_RESERVE_BYTES_V1, SNAPSHOT_ENTRIES_MAX_V1,
    SNAPSHOT_MATERIALIZED_TOTAL_MAX_BYTES_V1, SNAPSHOT_SINGLE_BLOB_MAX_BYTES_V1,
    SNAPSHOT_TRANSPORT_V1, SOURCE_MUTATION_POLICY_V1, TREE_PAGE_MAX_ENTRIES_V1, WORKSPACE_KIND_V1,
    WORKTREE_HEAD_MODE_V1,
};
use hello_agent_protocol::MAX_TEXT_CHUNK_BYTES;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::{CString, OsStr};
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
#[cfg(not(unix))]
use std::process::{Child, ChildStdin, ChildStdout, ExitStatus};
#[cfg(not(unix))]
use std::thread::JoinHandle;
#[cfg(unix)]
use std::{
    os::fd::AsRawFd,
    os::unix::{ffi::OsStrExt, fs::MetadataExt, process::CommandExt},
};
use uuid::Uuid;
#[cfg(unix)]
mod application;
#[cfg(unix)]
mod git_process;
#[cfg(unix)]
use git_process::{GitStdin, GitStdout, StreamingGitChild};
#[cfg(not(unix))]
type GitStdin = ChildStdin;
#[cfg(not(unix))]
type GitStdout = ChildStdout;
pub(crate) mod runtime_isolation;
mod snapshot_recovery;

const MANIFEST_DIRECTORY: &str = "worktree-manifests-v1";
const WORKTREE_DIRECTORY: &str = "worktrees-v1";
const SAVE_STAGING_DIRECTORY: &str = "save-staging-v1";
const WATCH_MAX_ENTRIES: usize = 10_000;
const WATCH_MAX_CHANGED_PATHS: usize = 1_024;
const DIFF_MAX_BYTES: usize = 10 * 1024 * 1024;
const DIFF_MAX_ENTRIES: usize = 10_000;
const SNAPSHOT_MARKER: &str = "hello-agent-snapshot-v1.json";
const SNAPSHOT_BUNDLE: &str = ".hello-agent-snapshot.bundle";
const GIT_CONTROL_OUTPUT_MAX_BYTES: usize = 64 * 1024;
const GIT_STDERR_TAIL_BYTES: usize = 8 * 1024;
#[cfg(target_os = "macos")]
// macOS SDK sys/stdio.h defines this newer flag; pinned libc exposes renameatx_np but not the
// constant. Older kernels/filesystems return EINVAL/ENOTSUP, which remains a fail-closed save.
const RENAME_NOFOLLOW_ANY: libc::c_uint = 0x0000_0010;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFailure {
    pub code: &'static str,
    pub kind: &'static str,
    pub detail: String,
    pub outcome_unknown: bool,
}

impl ProjectFailure {
    pub(crate) fn manifest(detail: impl Into<String>) -> Self {
        Self {
            code: "E_MANIFEST_MISMATCH",
            kind: "manifestMismatch",
            detail: detail.into(),
            outcome_unknown: false,
        }
    }

    pub(crate) fn symlink(detail: impl Into<String>) -> Self {
        Self {
            code: "E_SYMLINK_ESCAPE",
            kind: "symlinkEscape",
            detail: detail.into(),
            outcome_unknown: false,
        }
    }

    pub(crate) fn dirty(detail: impl Into<String>) -> Self {
        Self {
            code: "E_DIRTY_CONFLICT",
            kind: "dirtyConflict",
            detail: detail.into(),
            outcome_unknown: false,
        }
    }

    pub(crate) fn recovery(detail: impl Into<String>) -> Self {
        Self {
            code: "E_RECOVERY_REQUIRED",
            kind: "recoveryRequired",
            detail: detail.into(),
            outcome_unknown: false,
        }
    }

    pub(crate) fn ambiguous(detail: impl Into<String>) -> Self {
        Self {
            code: "E_RECOVERY_REQUIRED",
            kind: "recoveryRequired",
            detail: detail.into(),
            outcome_unknown: true,
        }
    }
}

impl From<SidecarError> for ProjectFailure {
    fn from(error: SidecarError) -> Self {
        Self::recovery(format!(
            "durable storage rejected the project operation: {error}"
        ))
    }
}

#[derive(Debug)]
pub struct TextStream {
    pub stream_id: String,
    pub chunks: Vec<String>,
}

#[derive(Debug)]
pub struct WatchEmission {
    pub stream_id: String,
    pub seq: u64,
    pub event: &'static str,
    pub payload: Value,
}

#[derive(Debug, Default)]
pub struct ProjectAction {
    pub result: Value,
    pub streams: Vec<TextStream>,
    pub watch_events: Vec<WatchEmission>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeFailurePoint {
    AfterRecoveryLedgerBeforeExport,
    AfterBundleBeforeMaterialization,
    AfterWorkspaceRenameBeforeActivation,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapshotCreation {
    ReuseActive,
    FreshChat,
    // Not exposed through product IPC until application admission is ready.
    #[cfg(unix)]
    #[allow(dead_code)]
    ApplicationCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFailurePoint {
    AfterRenameBeforeDirectorySync,
    AfterReopenBeforeLedgerClear,
}

struct PendingSave {
    chat_id: Option<String>,
    worktree_id: String,
    workspace_identity: String,
    project_id: String,
    relative_path: String,
    expected_version: String,
    total_bytes: u64,
    received_bytes: u64,
    last_seq: u64,
    staging_name: String,
    file: CapFile,
}

struct PreparedTarget {
    data_root: Dir,
    temp_parent: Dir,
    temp_name: String,
    temp_relative: PathBuf,
    target_relative: PathBuf,
    #[cfg(unix)]
    target_parent_identity: (u64, u64),
    file: Option<CapFile>,
    renamed: bool,
}

impl Drop for PreparedTarget {
    fn drop(&mut self) {
        if !self.renamed {
            self.file.take();
            remove_capability_temp(&self.temp_parent, &self.temp_name);
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct WatchFingerprint {
    kind: u8,
    length: u64,
    modified_nanos: u128,
}

struct WatchState {
    stream_id: String,
    snapshot: BTreeMap<String, WatchFingerprint>,
    next_seq: u64,
    workspace: Option<WorkspaceAddress>,
}

struct ActiveProject {
    project_id: String,
    canonical_git_root: PathBuf,
    worktree_id: String,
    worktree_path: PathBuf,
    base_commit: String,
    base_tree: String,
    object_format: String,
    source_identity: String,
    workspace_identity: String,
    manifest_path: PathBuf,
    lock_id: String,
}

struct SourceRepository {
    canonical_root: PathBuf,
    _root: Dir,
    git_dir: Dir,
    object_format: String,
    base_commit: String,
    base_tree: String,
    source_identity: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceDigestSet {
    head: String,
    index: String,
    refs: String,
    config: String,
    objects: String,
    packed_refs: String,
    entry_set: String,
    lock_count: u64,
}

impl SourceDigestSet {
    fn contract_value(&self) -> Value {
        json!({
            "head":self.head,
            "index":self.index,
            "refs":self.refs,
            "config":self.config,
            "objects":self.objects,
            "packedRefs":self.packed_refs,
            "entrySet":self.entry_set,
        })
    }
}

struct SnapshotStats {
    bundle_bytes: u64,
    materialized_bytes: u64,
    entry_count: u64,
    largest_blob: u64,
}

#[cfg(not(unix))]
struct StreamingGitChild {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr_reader: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    finished: bool,
}

#[cfg(not(unix))]
impl StreamingGitChild {
    fn spawn(command: &mut Command) -> std::io::Result<Self> {
        command.stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let Some(stderr) = child.stderr.take() else {
            terminate_child(&mut child);
            return Err(std::io::Error::other(
                "Git child did not expose its bounded stderr pipe",
            ));
        };
        Ok(Self {
            stdin: child.stdin.take(),
            stdout: child.stdout.take(),
            child,
            stderr_reader: Some(spawn_bounded_stderr_reader(stderr)),
            finished: false,
        })
    }

    fn finish(&mut self) -> std::io::Result<(ExitStatus, Vec<u8>)> {
        let status = self.child.wait()?;
        self.finished = true;
        let stderr = self
            .stderr_reader
            .take()
            .ok_or_else(|| std::io::Error::other("Git stderr reader was already consumed"))?
            .join()
            .map_err(|_| std::io::Error::other("Git stderr reader panicked"))??;
        Ok((status, stderr))
    }
}

#[cfg(not(unix))]
impl Drop for StreamingGitChild {
    fn drop(&mut self) {
        if !self.finished {
            terminate_child(&mut self.child);
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}

pub struct ProjectManager {
    data_root: PathBuf,
    data_dir: Dir,
    worktrees_root: PathBuf,
    manifests_root: PathBuf,
    worktrees_dir: Dir,
    manifests_dir: Dir,
    save_staging_root: PathBuf,
    save_staging_dir: Dir,
    pending_saves: HashMap<String, PendingSave>,
    watchers: HashMap<String, WatchState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceAddress {
    pub chat_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
}

impl ProjectManager {
    pub(crate) fn application_preview(&self, storage: &Storage, project_id: &str) -> Result<ProjectAction, ProjectFailure> {
        #[cfg(unix)]
        {
            let plan = application::review::prepare(self, storage, project_id)?;
            Ok(ProjectAction {
                result: serde_json::json!({"schemaVersion":1,"action":"applicationPreview","projectId":project_id,"preview":plan.preview()}),
                ..Default::default()
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (storage, project_id);
            Err(ProjectFailure::recovery("application preview unavailable on this platform"))
        }
    }
    /// Historical summaries are not live lock checks or restoration grants.
    pub(crate) fn application_history(
        &self,
        storage: &Storage,
        project_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let project = required_project(storage, project_id)?;
        let mut entries = Vec::new();
        if let Some(history) = project.get("application_history") {
            let invalid = || ProjectFailure::recovery("invalid application history");
            let history = history.as_object().ok_or_else(invalid)?;
            if history.len() > 1_000 { return Err(invalid()); }
            for (id, record) in history {
                if id.len() != 44 || !id.starts_with("application_")
                    || !id[12..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    || record["applicationId"].as_str() != Some(id.as_str())
                    || record["binding"]["projectId"] != project_id || record["version"] != 1
                    || !matches!(record["state"].as_str(), Some("applied" | "restored" | "abandoned"))
                { return Err(invalid()); }
                let total = record["backup"]["header"]["count"].as_u64().ok_or_else(invalid)?;
                if total > 100_000 { return Err(invalid()); }
                entries.push(serde_json::json!({"applicationId":id,"state":record["state"],"total":total}));
            }
        }
        Ok(ProjectAction {
            result: serde_json::json!({"schemaVersion":1,"action":"applicationHistory","projectId":project_id,"entries":entries}),
            ..Default::default()
        })
    }
    pub(crate) fn application_status(
        &self,
        storage: &Storage,
        project_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let project = required_project(storage, project_id)?;
        let status = if project.get("application_intent").is_none() {
            serde_json::json!({"projectId":project_id,"applicationId":null,"state":"none","currentIndex":null,"total":0})
        } else {
            #[cfg(unix)]
            {
                let record = application::review::intent::inspect(storage, project_id)?;
                serde_json::json!({"projectId":project_id,"applicationId":record["applicationId"],"state":record["state"],"currentIndex":record["pendingItem"]["index"],"total":record["backup"]["header"]["count"]})
            }
            #[cfg(not(unix))]
            {
                return Err(ProjectFailure::recovery(
                    "application state inspection unavailable on this platform",
                ));
            }
        };
        Ok(ProjectAction {
            result: serde_json::json!({"schemaVersion":1,"action":"applicationStatus","status":status}),
            ..Default::default()
        })
    }
    pub fn new(database_path: &Path) -> Result<Self, SidecarError> {
        let data_root = database_path.parent().ok_or_else(|| {
            SidecarError::new("dataDirectoryUnavailable", "database has no parent")
        })?;
        let data_root = fs::canonicalize(data_root).map_err(|error| {
            SidecarError::new(
                "dataDirectoryUnavailable",
                format!("could not canonicalize data root: {error}"),
            )
        })?;
        let data_dir = Dir::open_ambient_dir(&data_root, ambient_authority()).map_err(|error| {
            SidecarError::new(
                "dataDirectoryUnavailable",
                format!("could not open data root capability: {error}"),
            )
        })?;
        let (worktrees_root, worktrees_dir) =
            create_managed_directory(&data_root, &data_dir, WORKTREE_DIRECTORY)?;
        let (manifests_root, manifests_dir) =
            create_managed_directory(&data_root, &data_dir, MANIFEST_DIRECTORY)?;
        let (save_staging_root, save_staging_dir) =
            create_managed_directory(&data_root, &data_dir, SAVE_STAGING_DIRECTORY)?;
        Ok(Self {
            data_root,
            data_dir,
            worktrees_root,
            manifests_root,
            worktrees_dir,
            manifests_dir,
            save_staging_root,
            save_staging_dir,
            pending_saves: HashMap::new(),
            watchers: HashMap::new(),
        })
    }

    pub fn open_project(
        &mut self,
        storage: &mut Storage,
        selected_path: &str,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let selected = canonical_existing_directory(Path::new(selected_path))?;
        let source = self.open_source_repository(&selected)?;
        self.open_verified_source(storage, &selected, source, request_id)
    }

    /// Explicit selection of a saved ID, not renderer-supplied filesystem authority.
    pub fn reopen_project(
        &mut self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let path = storage
            .project_catalog_path(project_id)?
            .ok_or_else(|| ProjectFailure::manifest("saved project was not found"))?;
        if project_id_for_root(&path) != project_id {
            return Err(ProjectFailure::manifest("saved project ID and root differ"));
        }
        let selected = canonical_existing_directory(Path::new(&path))?;
        if canonical_protocol_path(&selected)? != path {
            return Err(ProjectFailure::manifest(
                "saved project path now resolves elsewhere",
            ));
        }
        let source = self.open_source_repository(&selected)?;
        if source.canonical_root != selected {
            return Err(ProjectFailure::manifest(
                "saved project is no longer a repository root",
            ));
        }
        // Keep the already validated capability-held repository through admission.
        // A newer HEAD is allowed just as with an explicit folder picker selection.
        self.open_verified_source(storage, &selected, source, request_id)
    }

    pub fn list_saved_projects(
        &self,
        storage: &Storage,
        cursor: Option<&str>,
    ) -> Result<ProjectAction, ProjectFailure> {
        let page = storage.project_catalog_page(cursor)?;
        let projects: Vec<Value> = page
            .entries
            .into_iter()
            .map(|entry| {
                json!({
                    "projectId": entry.project_id, "canonicalPath": entry.canonical_path,
                })
            })
            .collect();
        let result = json!({"schemaVersion":1,"action":"list","projects":projects,"nextCursor":page.next_cursor});
        if serde_json::to_vec(&result)
            .map_err(|error| ProjectFailure::manifest(error.to_string()))?
            .len()
            > 768 * 1024
        {
            return Err(ProjectFailure::manifest(
                "encoded project catalog page exceeds limit",
            ));
        }
        Ok(ProjectAction {
            result,
            ..ProjectAction::default()
        })
    }

    /// Detach watchers only; durable snapshots and chat bindings remain reusable.
    pub fn release_project_view(
        &mut self,
        storage: &Storage,
        project_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        storage
            .project_catalog_path(project_id)?
            .ok_or_else(|| ProjectFailure::manifest("saved project was not found"))?;
        self.watchers.retain(|key, watcher| {
            key != project_id
                && watcher
                    .workspace
                    .as_ref()
                    .is_none_or(|workspace| workspace.project_id != project_id)
        });
        Ok(ProjectAction {
            result: json!({"schemaVersion":1,"action":"releaseView","projectId":project_id}),
            ..ProjectAction::default()
        })
    }

    fn open_verified_source(
        &mut self,
        storage: &mut Storage,
        selected: &Path,
        source: SourceRepository,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        if !selected.starts_with(&source.canonical_root) {
            return Err(ProjectFailure::manifest(
                "picker-approved path is outside the capability-held Git root",
            ));
        }
        let canonical_text = canonical_protocol_path(&source.canonical_root)?;
        let selected_text = canonical_protocol_path(selected)?;
        let project_id = project_id_for_root(&canonical_text);

        if let Some(existing) = storage.projection_record("project", &project_id)? {
            let lifecycle = value_string(&existing, "lifecycle")?;
            if lifecycle == "recoveryRequired" {
                return Err(ProjectFailure::recovery(
                    "project has durable recovery state from an interrupted snapshot operation",
                ));
            }
            if lifecycle == "active" {
                let active = self.validate_active_project(storage, &project_id)?;
                if active.canonical_git_root != source.canonical_root {
                    return Err(ProjectFailure::manifest(
                        "persisted project root differs from the picker-approved root",
                    ));
                }
                return Ok(ProjectAction {
                    result: project_identity(&project_id, &selected_text, &canonical_text),
                    ..ProjectAction::default()
                });
            }
        }

        append_object(
            storage,
            "project",
            &project_id,
            &format!("op_project_open_{request_id}"),
            request_id,
            preserve_project_extensions(
                json!({
                    "project_id": project_id,
                    "canonical_path": canonical_text,
                    "selected_path": selected_text,
                    "base_commit": source.base_commit,
                    "base_tree": source.base_tree,
                    "object_format": source.object_format,
                    "source_identity": source.source_identity,
                    "lifecycle": "opened",
                }),
                &storage.projection_record("project", &project_id)?.unwrap_or_default(),
            )?,
        )?;
        Ok(ProjectAction {
            result: project_identity(&project_id, &selected_text, &canonical_text),
            ..ProjectAction::default()
        })
    }

    pub fn create_worktree(
        &mut self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        self.create_worktree_with_failure(storage, project_id, request_id, None)
    }

    pub fn create_worktree_with_failure(
        &mut self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
        failure: Option<WorktreeFailurePoint>,
    ) -> Result<ProjectAction, ProjectFailure> {
        self.create_worktree_inner(storage, project_id, request_id, failure, None, SnapshotCreation::ReuseActive)
    }

    pub fn prepare_chat_workspace(
        &mut self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        self.create_worktree_inner(storage, project_id, request_id, None, None, SnapshotCreation::FreshChat)
    }

    #[cfg(test)]
    fn create_worktree_with_pre_export_hook<F>(
        &mut self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
        mut hook: F,
    ) -> Result<ProjectAction, ProjectFailure>
    where
        F: FnMut(),
    {
        self.create_worktree_inner(
            storage,
            project_id,
            request_id,
            None,
            Some(&mut hook),
            SnapshotCreation::ReuseActive,
        )
    }

    fn create_worktree_inner(
        &mut self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
        failure: Option<WorktreeFailurePoint>,
        mut pre_export_hook: Option<&mut dyn FnMut()>,
        creation: SnapshotCreation,
    ) -> Result<ProjectAction, ProjectFailure> {
        let project = required_project(storage, project_id)?;
        if !storage.blocker_ids()?.is_empty() {
            return Err(ProjectFailure::recovery(
                "durable recovery blockers must be resolved before another snapshot export",
            ));
        }
        let lifecycle = value_string(&project, "lifecycle")?;
        if lifecycle == "active" && creation == SnapshotCreation::ReuseActive {
            let active = self.validate_active_project(storage, project_id)?;
            return Ok(ProjectAction {
                result: manifest_value(&active, "active")?,
                ..ProjectAction::default()
            });
        }
        if lifecycle == "recoveryRequired" {
            return Err(ProjectFailure::recovery(
                "snapshot creation was interrupted and cannot be retried automatically",
            ));
        }
        if !matches!(lifecycle, "opened" | "active" | "abandoned") {
            return Err(ProjectFailure::manifest(
                "project lifecycle cannot create a standalone snapshot",
            ));
        }

        let source =
            self.open_source_repository(Path::new(value_string(&project, "canonical_path")?))?;
        for (field, actual) in [
            ("base_commit", source.base_commit.as_str()),
            ("base_tree", source.base_tree.as_str()),
            ("object_format", source.object_format.as_str()),
            ("source_identity", source.source_identity.as_str()),
        ] {
            if project.get(field).and_then(Value::as_str) != Some(actual) {
                return Err(ProjectFailure::manifest(
                    "source identity changed after project.open",
                ));
            }
        }

        let worktree_id = format!("worktree_{}", Uuid::new_v4().simple());
        let workspace_identity = format!("workspace_{}", Uuid::new_v4().simple());
        let worktree_path = self.worktrees_root.join(&worktree_id);
        let staging_name = format!(".{worktree_id}.creating");
        let staging_path = self.worktrees_root.join(&staging_name);
        let manifest_path = self.manifests_root.join(format!("{worktree_id}.json"));
        if worktree_path.exists() || staging_path.exists() || manifest_path.exists() {
            return Err(ProjectFailure::recovery(
                "new snapshot identity unexpectedly already exists",
            ));
        }
        let lock_id = format!("lock_{worktree_id}");
        let worktree_text = canonical_protocol_path_without_existing(&worktree_path)?;
        let root_text = canonical_protocol_path(&source.canonical_root)?;
        let manifest_text = canonical_protocol_path_without_existing(&manifest_path)?;

        let preparation_lock = json!({
                "lock_id": lock_id,
                "project_id": project_id,
                "writer_run_id": Value::Null,
                "worktree_path": worktree_text,
                "poisoned": false,
                "recovery_required": true,
                "evidence": {"phase":"beforeSnapshotExport","launchFenceVersion":1,"request_id":request_id,
                    "workspace_identity":workspace_identity},
            });
        let preparation_project = preserve_project_extensions(
                json!({
                    "project_id": project_id,
                    "canonical_path": root_text,
                    "selected_path": value_string(&project, "selected_path")?,
                    "base_commit": source.base_commit,
                    "base_tree": source.base_tree,
                    "object_format": source.object_format,
                    "source_identity": source.source_identity,
                    "workspace_identity": workspace_identity,
                    "lifecycle": "recoveryRequired",
                    "worktree_id": worktree_id,
                    "worktree_path": worktree_text,
                    "manifest_path": manifest_text,
                    "lock_id": lock_id,
                }),
                &project,
            )?;
        snapshot_recovery::write_pair(storage, &format!("snapshot_prepare_{worktree_id}"), request_id, preparation_project, preparation_lock.clone(), &[])?;
        if failure == Some(WorktreeFailurePoint::AfterRecoveryLedgerBeforeExport) {
            return Err(ProjectFailure::recovery(
                "failure injected after durable recovery marker before snapshot export",
            ));
        }

        self.worktrees_dir
            .create_dir(&staging_name)
            .map_err(|error| {
                ProjectFailure::recovery(format!(
                    "could not create managed snapshot staging child: {error}"
                ))
            })?;
        let staging = open_real_child_dir(&self.worktrees_dir, &staging_name)?;
        let before = source_digest(&source.git_dir)?;
        if before.lock_count != 0 {
            return Err(ProjectFailure::recovery(
                "source Git directory contains lock files before snapshot export",
            ));
        }
        if let Some(hook) = &mut pre_export_hook {
            hook();
        }
        let mut launching = preparation_lock;
        launching["evidence"]["phase"] = json!("snapshotExportStarting");
        append_object(storage, "lock", &lock_id, &format!("op_snapshot_launch_{worktree_id}"), request_id, launching)?;
        #[cfg(unix)]
        let baseline = if creation == SnapshotCreation::ApplicationCandidate {
            Some(application::git_baseline::raw_baseline::capture_clean_baseline(self, &source)?)
        } else {
            None
        };
        let bundle_bytes = self.export_bundle(&source.git_dir, &staging)?;
        let after = source_digest(&source.git_dir)?;
        if before != after || after.lock_count != 0 {
            return Err(ProjectFailure::recovery(
                "source Git identity changed during snapshot export",
            ));
        }
        if failure == Some(WorktreeFailurePoint::AfterBundleBeforeMaterialization) {
            return Err(ProjectFailure::recovery(
                "failure injected after bundle export before materialization",
            ));
        }

        #[cfg(unix)]
        if let Some(expected) = &baseline {
            if *expected != application::git_baseline::raw_baseline::capture_clean_baseline(self, &source)? {
                return Err(ProjectFailure::dirty("source baseline changed during snapshot export"));
            }
        }

        self.initialize_snapshot_repository(&staging, &source)?;
        let mut stats = self.materialize_snapshot(&staging, &source)?;
        stats.bundle_bytes = bundle_bytes;
        let disk_bytes_after = available_space(&staging)?;
        if disk_bytes_after < SNAPSHOT_DISK_RESERVE_BYTES_V1 {
            return Err(ProjectFailure::recovery(
                "snapshot materialization would violate the 512 MiB disk reserve",
            ));
        }

        let active = ActiveProject {
            project_id: project_id.to_string(),
            canonical_git_root: source.canonical_root,
            worktree_id,
            worktree_path,
            base_commit: source.base_commit,
            base_tree: source.base_tree,
            object_format: source.object_format,
            source_identity: source.source_identity,
            workspace_identity,
            manifest_path,
            lock_id,
        };
        #[cfg(unix)]
        if let Some(baseline) = &baseline {
            application::git_baseline::raw_baseline::receipt::persist_and_bind(
                self, storage, &active, baseline, request_id,
            )?;
        }
        let manifest = manifest_value(&active, "active")?;
        write_json_atomic(
            &self.manifests_dir,
            &format!("{}.json", active.worktree_id),
            &manifest,
        )?;
        let git_dir = open_real_child_dir(&staging, ".git")?;
        write_json_atomic(&git_dir, SNAPSHOT_MARKER, &snapshot_identity(&active)?)?;
        self.verify_snapshot_repository(&staging, &active)?;
        self.worktrees_dir
            .rename(&staging_name, &self.worktrees_dir, &active.worktree_id)
            .map_err(|error| {
                ProjectFailure::recovery(format!("could not activate snapshot directory: {error}"))
            })?;
        sync_capability_directory(&self.worktrees_dir)?;
        if failure == Some(WorktreeFailurePoint::AfterWorkspaceRenameBeforeActivation) {
            return Err(ProjectFailure::recovery(
                "failure injected after snapshot rename before activation",
            ));
        }

        // Publication may have added a source receipt after the original open record.
        let project = required_project(storage, project_id)?;
        self.persist_pre_activation(storage, &active, &project, request_id)?;
        let final_dir = self.worktree_capability(&active)?;
        let durable = required_project(storage, &active.project_id)?;
        let canonical_root_text =
            canonical_protocol_path_without_existing(&active.canonical_git_root)?;
        for (field, expected) in [
            ("worktree_id", active.worktree_id.as_str()),
            ("workspace_identity", active.workspace_identity.as_str()),
            ("canonical_path", canonical_root_text.as_str()),
            ("source_identity", active.source_identity.as_str()),
            ("object_format", active.object_format.as_str()),
            ("base_commit", active.base_commit.as_str()),
            ("base_tree", active.base_tree.as_str()),
        ] {
            if durable.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "database pre-activation identity differs from the proposed snapshot",
                ));
            }
        }
        if read_bounded_json(&self.manifests_dir, &format!("{}.json", active.worktree_id))?
            != manifest
        {
            return Err(ProjectFailure::manifest(
                "external manifest changed before snapshot activation",
            ));
        }
        if read_bounded_json(&open_real_child_dir(&final_dir, ".git")?, SNAPSHOT_MARKER)?
            != snapshot_identity(&active)?
        {
            return Err(ProjectFailure::manifest(
                "Git marker changed before snapshot activation",
            ));
        }
        let invariant = self.snapshot_invariant(&final_dir, &active)?;
        let evidence = activation_evidence(&active, &before, &after, invariant)?;
        validate_project_payload_v1("snapshotActivationEvidence", &evidence).map_err(|error| {
            ProjectFailure::manifest(format!("composite activation evidence failed: {error}"))
        })?;
        validate_project_payload_v1(
            "snapshotSizing",
            &json!({
                "bundleBytes":stats.bundle_bytes,
                "materializedTotalBytes":stats.materialized_bytes,
                "entryCount":stats.entry_count,
                "singleBlobBytes":stats.largest_blob,
                "diskBytesAfterMaterialization":disk_bytes_after,
                "streaming":true,
                "fullAllocation":false,
            }),
        )
        .map_err(|error| ProjectFailure::manifest(format!("snapshot sizing failed: {error}")))?;

        self.persist_active(storage, &active, &project, request_id)?;
        Ok(ProjectAction {
            result: manifest,
            ..ProjectAction::default()
        })
    }

    pub fn get_tree(
        &mut self,
        storage: &Storage,
        project_id: &str,
        parent_path: &str,
        cursor: Option<&str>,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_active_project(storage, project_id)?;
        self.get_tree_from_active(&active, project_id, parent_path, cursor)
    }

    fn get_tree_from_active(
        &self,
        active: &ActiveProject,
        project_id: &str,
        parent_path: &str,
        cursor: Option<&str>,
    ) -> Result<ProjectAction, ProjectFailure> {
        validate_relative_path(parent_path, true)?;
        let worktree = self.worktree_capability(active)?;
        let directory = open_relative_dir(&worktree, parent_path)?;
        let cursor_key = tree_cursor_key(project_id, parent_path);
        let offset = match cursor {
            None => 0,
            Some(value) => parse_tree_cursor(value, &cursor_key)?,
        };
        let mut entries = Vec::new();
        for entry in directory.entries().map_err(|error| {
            ProjectFailure::manifest(format!("could not read tree capability: {error}"))
        })? {
            let entry = entry.map_err(|error| {
                ProjectFailure::manifest(format!("could not inspect tree entry: {error}"))
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                ProjectFailure::manifest("tree entry is not valid Unicode for the protocol")
            })?;
            if is_internal_temp_name(&name) {
                continue;
            }
            validate_relative_path(&name, false)?;
            let relative = if parent_path.is_empty() {
                name.clone()
            } else {
                format!("{parent_path}/{name}")
            };
            let file_type = entry.file_type().map_err(|error| {
                ProjectFailure::manifest(format!("could not classify tree entry: {error}"))
            })?;
            let kind = if file_type.is_symlink() {
                "symlink"
            } else if file_type.is_dir() {
                "directory"
            } else if file_type.is_file() {
                "file"
            } else {
                return Err(ProjectFailure::manifest(
                    "tree contains an unsupported filesystem entry",
                ));
            };
            entries.push((relative, name, kind));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        if offset > entries.len() {
            return Err(ProjectFailure::manifest(
                "tree cursor is outside the current page set",
            ));
        }
        let end = offset
            .saturating_add(TREE_PAGE_MAX_ENTRIES_V1)
            .min(entries.len());
        let values = entries[offset..end]
            .iter()
            .map(|(path, name, kind)| json!({"path":path,"name":name,"kind":kind}))
            .collect::<Vec<_>>();
        let next = if end < entries.len() {
            Value::String(format!("{cursor_key}:{end}"))
        } else {
            Value::Null
        };
        Ok(ProjectAction {
            result: json!({
                "projectId": project_id,
                "parentPath": parent_path,
                "entries": values,
                "nextCursor": next,
            }),
            ..ProjectAction::default()
        })
    }

    pub fn read_file(
        &mut self,
        storage: &Storage,
        project_id: &str,
        relative_path: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_active_project(storage, project_id)?;
        self.read_file_from_active(&active, project_id, relative_path)
    }

    pub fn workspace_get_tree(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        parent_path: &str,
        cursor: Option<&str>,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        self.get_tree_from_active(&active, &address.project_id, parent_path, cursor)
    }

    pub fn workspace_read_file(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        relative_path: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        self.read_file_from_active(&active, &address.project_id, relative_path)
    }

    #[cfg(test)]
    fn read_file_with_post_validation_hook<F>(
        &mut self,
        storage: &Storage,
        project_id: &str,
        relative_path: &str,
        hook: F,
    ) -> Result<ProjectAction, ProjectFailure>
    where
        F: FnOnce(),
    {
        let active = self.validate_active_project(storage, project_id)?;
        hook();
        self.read_file_from_active(&active, project_id, relative_path)
    }

    fn read_file_from_active(
        &self,
        active: &ActiveProject,
        project_id: &str,
        relative_path: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let worktree = self.worktree_capability(active)?;
        let (parent, name) = open_parent_capability(&worktree, relative_path)?;
        let mut file = open_real_file_at(&parent, &name)?;
        let (bytes, byte_length, version) = read_file_bounded_handle(&mut file, FILE_MAX_BYTES_V1)?;
        let classification = if byte_length > FILE_MAX_BYTES_V1 {
            "tooLarge"
        } else if bytes
            .as_deref()
            .filter(|value| !value.contains(&0))
            .and_then(|value| std::str::from_utf8(value).ok())
            .is_some()
        {
            "utf8"
        } else {
            "binary"
        };
        if classification != "utf8" {
            return Ok(ProjectAction {
                result: json!({
                    "projectId":project_id,"path":relative_path,"classification":classification,
                    "byteLength":byte_length,"version":version,"stream":Value::Null,
                }),
                ..ProjectAction::default()
            });
        }
        let bytes = bytes.ok_or_else(|| {
            ProjectFailure::manifest("bounded UTF-8 read did not retain the classified bytes")
        })?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            ProjectFailure::manifest("file changed while its UTF-8 classification was finalized")
        })?;
        let stream_id = format!("stream_read_{}", Uuid::new_v4().simple());
        Ok(ProjectAction {
            result: json!({
                "projectId":project_id,"path":relative_path,"classification":"utf8",
                "byteLength":byte_length,"version":version,
                "stream":{"streamId":stream_id,"chunkBytes":MAX_TEXT_CHUNK_BYTES},
            }),
            streams: vec![TextStream {
                stream_id,
                chunks: chunk_utf8(text, MAX_TEXT_CHUNK_BYTES),
            }],
            watch_events: Vec::new(),
        })
    }

    pub fn begin_save(
        &mut self,
        storage: &Storage,
        project_id: &str,
        relative_path: &str,
        expected_version: &str,
        total_bytes: u64,
        encoding: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_active_project(storage, project_id)?;
        self.begin_save_from_active(
            None,
            &active,
            relative_path,
            expected_version,
            total_bytes,
            encoding,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn workspace_begin_save(
        &mut self,
        storage: &mut Storage,
        address: &WorkspaceAddress,
        relative_path: &str,
        expected_version: &str,
        total_bytes: u64,
        encoding: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        let action = self.begin_save_from_active(
            Some(address.chat_id.as_str()),
            &active,
            relative_path,
            expected_version,
            total_bytes,
            encoding,
        )?;
        let save_id = action.result["saveId"]
            .as_str()
            .ok_or_else(|| ProjectFailure::recovery("save begin omitted its identity"))?
            .to_string();
        if let Err(error) = self.persist_workspace_save_session(storage, address, &save_id, true) {
            if let Some(pending) = self.pending_saves.remove(&save_id) {
                remove_capability_temp(&self.save_staging_dir, pending.staging_name);
            }
            return Err(error);
        }
        Ok(action)
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_save_from_active(
        &mut self,
        chat_id: Option<&str>,
        active: &ActiveProject,
        relative_path: &str,
        expected_version: &str,
        total_bytes: u64,
        encoding: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let project_id = active.project_id.as_str();
        validate_file_version(expected_version)?;
        validate_relative_path(relative_path, false)?;
        if encoding != "utf8" || total_bytes > FILE_MAX_BYTES_V1 {
            return Err(ProjectFailure::manifest(
                "file.save begin requires UTF-8 content no larger than 5 MiB",
            ));
        }
        let worktree = self.worktree_capability(active)?;
        let (parent, target_name) = open_parent_capability(&worktree, relative_path)?;
        open_real_file_at(&parent, &target_name)?;
        let save_id = format!("save_{}", Uuid::new_v4().simple());
        let staging_name = format!("{save_id}.stage");
        let mut options = CapOpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let file = self
            .save_staging_dir
            .open_with(&staging_name, &options)
            .map_err(|error| {
                ProjectFailure::recovery(format!(
                    "could not create managed save staging file: {error}"
                ))
            })?;
        self.pending_saves.insert(
            save_id.clone(),
            PendingSave {
                chat_id: chat_id.map(str::to_string),
                worktree_id: active.worktree_id.clone(),
                workspace_identity: active.workspace_identity.clone(),
                project_id: project_id.to_string(),
                relative_path: relative_path.to_string(),
                expected_version: expected_version.to_string(),
                total_bytes,
                received_bytes: 0,
                last_seq: 0,
                staging_name,
                file,
            },
        );
        Ok(ProjectAction {
            result: json!({"saveId":save_id,"chunkBytes":MAX_TEXT_CHUNK_BYTES}),
            ..ProjectAction::default()
        })
    }

    pub fn append_save_chunk(
        &mut self,
        save_id: &str,
        seq: u64,
        text: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let pending = self.pending_saves.get_mut(save_id).ok_or_else(|| {
            ProjectFailure::recovery("save identity is missing and cannot be retried automatically")
        })?;
        let bytes = text.as_bytes();
        if seq != pending.last_seq + 1 || bytes.len() > MAX_TEXT_CHUNK_BYTES {
            return Err(ProjectFailure::manifest(
                "save chunks must be contiguous and no larger than 256 KiB",
            ));
        }
        let next = pending
            .received_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| ProjectFailure::manifest("save byte count overflow"))?;
        if next > pending.total_bytes || next > FILE_MAX_BYTES_V1 {
            return Err(ProjectFailure::manifest(
                "save chunks exceed declared total bytes",
            ));
        }
        pending.file.write_all(bytes).map_err(|error| {
            ProjectFailure::recovery(format!("could not write save temp file: {error}"))
        })?;
        pending.received_bytes = next;
        pending.last_seq = seq;
        Ok(ProjectAction {
            result: json!({"acceptedSeq":seq}),
            ..ProjectAction::default()
        })
    }

    pub fn workspace_append_save_chunk(
        &mut self,
        storage: &Storage,
        address: &WorkspaceAddress,
        save_id: &str,
        seq: u64,
        text: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        self.validate_workspace_address(storage, address)?;
        self.verify_pending_save_address(address, save_id)?;
        self.verify_durable_save_session(storage, address, save_id)?;
        self.append_save_chunk(save_id, seq, text)
    }

    fn verify_pending_save_address(
        &self,
        address: &WorkspaceAddress,
        save_id: &str,
    ) -> Result<(), ProjectFailure> {
        let pending = self.pending_saves.get(save_id).ok_or_else(|| {
            ProjectFailure::recovery("save identity is missing and cannot be retried automatically")
        })?;
        if pending.chat_id.as_deref() != Some(address.chat_id.as_str())
            || pending.project_id != address.project_id
            || pending.worktree_id != address.worktree_id
            || pending.workspace_identity != address.workspace_identity
        {
            return Err(ProjectFailure::manifest(
                "save continuation differs from its durable chat workspace binding",
            ));
        }
        Ok(())
    }

    pub fn commit_save(
        &mut self,
        storage: &mut Storage,
        save_id: &str,
        expected_version: &str,
        last_seq: u64,
        total_bytes: u64,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        self.commit_save_with_failure(
            storage,
            save_id,
            expected_version,
            last_seq,
            total_bytes,
            request_id,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn workspace_commit_save(
        &mut self,
        storage: &mut Storage,
        address: &WorkspaceAddress,
        save_id: &str,
        expected_version: &str,
        last_seq: u64,
        total_bytes: u64,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        self.validate_workspace_address(storage, address)?;
        self.verify_pending_save_address(address, save_id)?;
        self.verify_durable_save_session(storage, address, save_id)?;
        let result = self.commit_save(
            storage,
            save_id,
            expected_version,
            last_seq,
            total_bytes,
            request_id,
        );
        let preserve = result
            .as_ref()
            .err()
            .is_some_and(|error| error.outcome_unknown);
        if !preserve {
            if let Err(error) =
                self.persist_workspace_save_session(storage, address, save_id, false)
            {
                if result.is_ok() {
                    return Err(ProjectFailure::ambiguous(format!(
                        "file replacement completed but durable save-session cleanup failed: {}",
                        error.detail
                    )));
                }
            }
        }
        result
    }

    fn verify_durable_save_session(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        save_id: &str,
    ) -> Result<(), ProjectFailure> {
        let chat = storage
            .projection_record("chat", &address.chat_id)?
            .ok_or_else(|| ProjectFailure::manifest("save chat projection is missing"))?;
        let session = chat
            .get("save_sessions")
            .and_then(Value::as_object)
            .and_then(|sessions| sessions.get(save_id))
            .ok_or_else(|| ProjectFailure::recovery("save session is no longer durable"))?;
        for (field, expected) in [
            ("chatId", address.chat_id.as_str()),
            ("projectId", address.project_id.as_str()),
            ("worktreeId", address.worktree_id.as_str()),
            ("workspaceIdentity", address.workspace_identity.as_str()),
            ("saveId", save_id),
        ] {
            if session.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "durable save session differs from its continuation",
                ));
            }
        }
        Ok(())
    }

    fn persist_workspace_save_session(
        &self,
        storage: &mut Storage,
        address: &WorkspaceAddress,
        save_id: &str,
        insert: bool,
    ) -> Result<(), ProjectFailure> {
        let mut chat = storage
            .projection_record("chat", &address.chat_id)?
            .ok_or_else(|| ProjectFailure::manifest("save chat projection is missing"))?;
        let mut sessions = chat
            .remove("save_sessions")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        if insert {
            let pending = self.pending_saves.get(save_id).ok_or_else(|| {
                ProjectFailure::recovery("in-memory save session disappeared before persistence")
            })?;
            if sessions.len() >= 100 {
                return Err(ProjectFailure::recovery(
                    "durable save session limit has been reached",
                ));
            }
            sessions.insert(
                save_id.to_string(),
                json!({
                    "chatId":address.chat_id,"projectId":address.project_id,
                    "worktreeId":address.worktree_id,"workspaceIdentity":address.workspace_identity,
                    "saveId":save_id,"path":pending.relative_path,
                    "expectedVersion":pending.expected_version,"totalBytes":pending.total_bytes,
                    "encoding":"utf8","chunkBytes":MAX_TEXT_CHUNK_BYTES,
                }),
            );
        } else if sessions.remove(save_id).is_none() {
            return Err(ProjectFailure::recovery(
                "durable save session disappeared before completion",
            ));
        }
        chat.insert("save_sessions".to_string(), Value::Object(sessions));
        storage.upsert_projection_record(
            "chat",
            &address.chat_id,
            &format!(
                "op_workspace_save_session_{}_{}",
                if insert { "begin" } else { "finish" },
                save_id
            ),
            None,
            chat,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_save_with_failure(
        &mut self,
        storage: &mut Storage,
        save_id: &str,
        expected_version: &str,
        last_seq: u64,
        total_bytes: u64,
        request_id: &str,
        failure: Option<SaveFailurePoint>,
    ) -> Result<ProjectAction, ProjectFailure> {
        self.commit_save_inner(
            storage,
            save_id,
            expected_version,
            last_seq,
            total_bytes,
            request_id,
            failure,
            None,
            None,
        )
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn commit_save_with_pre_syscall_hook<F>(
        &mut self,
        storage: &mut Storage,
        save_id: &str,
        expected_version: &str,
        last_seq: u64,
        total_bytes: u64,
        request_id: &str,
        mut hook: F,
    ) -> Result<ProjectAction, ProjectFailure>
    where
        F: FnMut(),
    {
        self.commit_save_inner(
            storage,
            save_id,
            expected_version,
            last_seq,
            total_bytes,
            request_id,
            None,
            None,
            Some(&mut hook),
        )
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn commit_save_with_pre_revalidation_hook<F>(
        &mut self,
        storage: &mut Storage,
        save_id: &str,
        expected_version: &str,
        last_seq: u64,
        total_bytes: u64,
        request_id: &str,
        mut hook: F,
    ) -> Result<ProjectAction, ProjectFailure>
    where
        F: FnMut(),
    {
        self.commit_save_inner(
            storage,
            save_id,
            expected_version,
            last_seq,
            total_bytes,
            request_id,
            None,
            Some(&mut hook),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_save_inner(
        &mut self,
        storage: &mut Storage,
        save_id: &str,
        expected_version: &str,
        last_seq: u64,
        total_bytes: u64,
        request_id: &str,
        failure: Option<SaveFailurePoint>,
        pre_revalidation_hook: Option<&mut dyn FnMut()>,
        pre_syscall_hook: Option<&mut dyn FnMut()>,
    ) -> Result<ProjectAction, ProjectFailure> {
        let mut pending = self.pending_saves.remove(save_id).ok_or_else(|| {
            ProjectFailure::recovery("save identity is missing and cannot be retried automatically")
        })?;
        let result = self.commit_pending_save(
            storage,
            save_id,
            expected_version,
            last_seq,
            total_bytes,
            request_id,
            failure,
            pre_revalidation_hook,
            pre_syscall_hook,
            &mut pending,
        );
        let preserve_staging = result
            .as_ref()
            .err()
            .is_some_and(|error| error.outcome_unknown);
        drop(pending.file);
        if !preserve_staging {
            remove_capability_temp(&self.save_staging_dir, &pending.staging_name);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_pending_save(
        &self,
        storage: &mut Storage,
        save_id: &str,
        expected_version: &str,
        last_seq: u64,
        total_bytes: u64,
        request_id: &str,
        failure: Option<SaveFailurePoint>,
        pre_revalidation_hook: Option<&mut dyn FnMut()>,
        pre_syscall_hook: Option<&mut dyn FnMut()>,
        pending: &mut PendingSave,
    ) -> Result<ProjectAction, ProjectFailure> {
        if pending.expected_version != expected_version
            || pending.last_seq != last_seq
            || pending.total_bytes != total_bytes
            || pending.received_bytes != total_bytes
        {
            return Err(ProjectFailure::manifest(
                "save commit does not match its begin and received chunks",
            ));
        }
        pending.file.flush().map_err(|error| {
            ProjectFailure::recovery(format!("could not flush save temp file: {error}"))
        })?;
        pending.file.sync_all().map_err(|error| {
            ProjectFailure::recovery(format!("could not sync managed save staging file: {error}"))
        })?;
        pending.file.seek(SeekFrom::Start(0)).map_err(|error| {
            ProjectFailure::recovery(format!(
                "could not rewind managed save staging file: {error}"
            ))
        })?;

        let active = if let Some(chat_id) = &pending.chat_id {
            self.validate_workspace_address(
                storage,
                &WorkspaceAddress {
                    chat_id: chat_id.clone(),
                    project_id: pending.project_id.clone(),
                    worktree_id: pending.worktree_id.clone(),
                    workspace_identity: pending.workspace_identity.clone(),
                },
            )?
        } else {
            self.validate_active_project(storage, &pending.project_id)?
        };
        let worktree = self.worktree_capability(&active)?;
        let (parent, target_name) = open_parent_capability(&worktree, &pending.relative_path)?;
        let mut target = open_real_file_at(&parent, &target_name)?;
        let permissions = target
            .metadata()
            .map_err(|error| {
                ProjectFailure::manifest(format!("could not inspect save permissions: {error}"))
            })?
            .permissions();
        #[cfg(unix)]
        let target_parent_identity = capability_directory_identity(&parent)?;
        let (_, _, current) = read_file_bounded_handle(&mut target, 0)?;
        if current != pending.expected_version {
            let error = ProjectFailure::dirty(
                "file version changed after the editor read it; target bytes were not written",
            );
            return Ok(ProjectAction {
                result: json!({
                    "outcome":"dirtyConflict",
                    "error":{
                        "code":error.code,"kind":error.kind,"detail":error.detail,
                        "retryable":false,"outcomeUnknown":false,
                    }
                }),
                ..ProjectAction::default()
            });
        }
        drop(target);
        let temp_name = format!(".hello-agent-save-{}.tmp", Uuid::new_v4().simple());
        let mut options = CapOpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let file = worktree.open_with(&temp_name, &options).map_err(|error| {
            ProjectFailure::recovery(format!("could not create target save temp file: {error}"))
        })?;
        let temp_relative = PathBuf::from(WORKTREE_DIRECTORY)
            .join(&active.worktree_id)
            .join(&temp_name);
        let target_relative = PathBuf::from(WORKTREE_DIRECTORY)
            .join(&active.worktree_id)
            .join(&pending.relative_path);
        let mut prepared = PreparedTarget {
            data_root: self.data_dir.try_clone().map_err(|error| {
                ProjectFailure::recovery(format!(
                    "could not clone stable data-root capability for save: {error}"
                ))
            })?,
            temp_parent: worktree.try_clone().map_err(|error| {
                ProjectFailure::recovery(format!(
                    "could not clone stable save-temp parent capability: {error}"
                ))
            })?,
            temp_name,
            temp_relative,
            target_relative,
            #[cfg(unix)]
            target_parent_identity,
            file: Some(file),
            renamed: false,
        };
        prepared
            .file
            .as_ref()
            .ok_or_else(|| ProjectFailure::recovery("target save temp file was lost"))?
            .set_permissions(permissions)
            .map_err(|error| {
                ProjectFailure::recovery(format!("could not preserve save permissions: {error}"))
            })?;
        let mut copied = 0_u64;
        let mut staged_hasher = Sha256::new();
        let mut buffer = vec![0_u8; MAX_TEXT_CHUNK_BYTES];
        loop {
            let read = pending.file.read(&mut buffer).map_err(|error| {
                ProjectFailure::recovery(format!(
                    "could not read managed save staging file: {error}"
                ))
            })?;
            if read == 0 {
                break;
            }
            copied = copied
                .checked_add(read as u64)
                .ok_or_else(|| ProjectFailure::manifest("save staging byte count overflow"))?;
            staged_hasher.update(&buffer[..read]);
            prepared
                .file
                .as_mut()
                .ok_or_else(|| ProjectFailure::recovery("target save temp file was lost"))?
                .write_all(&buffer[..read])
                .map_err(|error| {
                    ProjectFailure::recovery(format!(
                        "could not write target save temp file: {error}"
                    ))
                })?;
        }
        if copied != pending.total_bytes {
            return Err(ProjectFailure::recovery(
                "managed save staging bytes differ from the declared total",
            ));
        }
        let staged_version = format!("sha256:{:x}", staged_hasher.finalize());
        let mut target_temp = prepared.file.take().ok_or_else(|| {
            ProjectFailure::recovery("target save temp file was lost before flush")
        })?;
        target_temp.flush().map_err(|error| {
            ProjectFailure::recovery(format!("could not flush target save temp file: {error}"))
        })?;
        target_temp.sync_all().map_err(|error| {
            ProjectFailure::recovery(format!("could not sync target save temp file: {error}"))
        })?;
        drop(target_temp);

        self.persist_save_lock(storage, &active, save_id, request_id, true, None)?;
        self.rename_prepared_save_from_stable_root(
            &prepared,
            expected_version,
            pre_revalidation_hook,
            pre_syscall_hook,
        )?;
        prepared.renamed = true;
        if failure == Some(SaveFailurePoint::AfterRenameBeforeDirectorySync) {
            return Err(ProjectFailure::ambiguous(
                "failure injected after save rename before directory durability",
            ));
        }
        let target_relative = prepared.target_relative.to_str().ok_or_else(|| {
            ProjectFailure::ambiguous(
                "save rename completed but the target path is not protocol-safe UTF-8",
            )
        })?;
        let (committed_parent, committed_name) =
            open_parent_capability(&prepared.data_root, target_relative).map_err(|error| {
                ProjectFailure::ambiguous(format!(
                    "save rename completed but the target parent could not be reopened: {}",
                    error.detail
                ))
            })?;
        sync_capability_directory(&committed_parent).map_err(|error| {
            ProjectFailure::ambiguous(format!(
                "save rename completed but directory durability is unknown: {}",
                error.detail
            ))
        })?;
        let mut committed =
            open_real_file_at(&committed_parent, &committed_name).map_err(|error| {
                ProjectFailure::ambiguous(format!(
                    "save rename completed but the target could not be reopened: {}",
                    error.detail
                ))
            })?;
        let (_, _, version) = read_file_bounded_handle(&mut committed, 0).map_err(|error| {
            ProjectFailure::ambiguous(format!(
                "save rename completed but committed bytes could not be hashed: {}",
                error.detail
            ))
        })?;
        if version != staged_version {
            return Err(ProjectFailure::ambiguous(
                "save rename completed but committed bytes differ from managed staging",
            ));
        }
        if failure == Some(SaveFailurePoint::AfterReopenBeforeLedgerClear) {
            return Err(ProjectFailure::ambiguous(
                "failure injected after save verification before recovery ledger clear",
            ));
        }
        self.persist_save_lock(storage, &active, save_id, request_id, false, Some(&version))
            .map_err(|error| {
                ProjectFailure::ambiguous(format!(
                    "save bytes were replaced but recovery ledger clear failed: {}",
                    error.detail
                ))
            })?;
        Ok(ProjectAction {
            result: json!({"outcome":"committed","version":version}),
            ..ProjectAction::default()
        })
    }

    #[cfg(target_os = "macos")]
    fn rename_prepared_save_from_stable_root(
        &self,
        prepared: &PreparedTarget,
        expected_version: &str,
        mut pre_revalidation_hook: Option<&mut dyn FnMut()>,
        mut pre_syscall_hook: Option<&mut dyn FnMut()>,
    ) -> Result<(), ProjectFailure> {
        let root_file = prepared
            .data_root
            .try_clone()
            .map_err(|error| {
                ProjectFailure::recovery(format!(
                    "could not clone the stable data-root rename capability: {error}"
                ))
            })?
            .into_std_file();
        let source = root_relative_cstring(&prepared.temp_relative)?;
        let destination = root_relative_cstring(&prepared.target_relative)?;

        self.validate_prepared_save_boundary(prepared, expected_version)?;

        if let Some(hook) = &mut pre_revalidation_hook {
            hook();
        }

        self.validate_prepared_save_boundary(prepared, expected_version)?;

        // This test-only hook is the real syscall boundary: validation, root FD, and arguments
        // are complete. macOS still rejects every symlink component in-kernel.
        if let Some(hook) = &mut pre_syscall_hook {
            hook();
        }

        // SAFETY: both NUL-terminated paths were validated as relative normal components, are
        // resolved from the same held data-root FD, and cannot follow a symlink in-kernel.
        let result = unsafe {
            libc::renameatx_np(
                root_file.as_raw_fd(),
                source.as_ptr(),
                root_file.as_raw_fd(),
                destination.as_ptr(),
                RENAME_NOFOLLOW_ANY,
            )
        };
        if result != 0 {
            return Err(ProjectFailure::recovery(format!(
                "atomic stable-root save replace failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn validate_prepared_save_boundary(
        &self,
        prepared: &PreparedTarget,
        expected_version: &str,
    ) -> Result<(), ProjectFailure> {
        self.verify_stable_data_root(&prepared.data_root)?;
        let _ = verify_real_parent_and_file(
            &prepared.data_root,
            &prepared.temp_relative,
            "managed save temp",
        )?;
        let (current_parent, mut current) = verify_real_parent_and_file(
            &prepared.data_root,
            &prepared.target_relative,
            "save destination",
        )?;
        if capability_directory_identity(&current_parent)? != prepared.target_parent_identity {
            return Err(ProjectFailure::recovery(
                "save destination hierarchy changed at the final rename boundary",
            ));
        }
        let (_, _, current_version) =
            read_file_bounded_handle(&mut current, 0).map_err(|error| {
                ProjectFailure::recovery(format!(
                    "save destination changed at the final rename boundary: {}",
                    error.detail
                ))
            })?;
        if current_version != expected_version {
            return Err(ProjectFailure::recovery(
                "save destination changed at the final rename boundary",
            ));
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn rename_prepared_save_from_stable_root(
        &self,
        _prepared: &PreparedTarget,
        _expected_version: &str,
        _pre_revalidation_hook: Option<&mut dyn FnMut()>,
        _pre_syscall_hook: Option<&mut dyn FnMut()>,
    ) -> Result<(), ProjectFailure> {
        Err(ProjectFailure::recovery(
            "atomic stable-root save replacement is unavailable on this platform",
        ))
    }

    #[cfg(target_os = "macos")]
    fn verify_stable_data_root(&self, held: &Dir) -> Result<(), ProjectFailure> {
        self.verify_managed_paths()?;
        let held_metadata = held.metadata(".").map_err(|error| {
            ProjectFailure::recovery(format!(
                "could not inspect held managed data-root identity: {error}"
            ))
        })?;
        let ambient_metadata = fs::metadata(&self.data_root).map_err(|error| {
            ProjectFailure::recovery(format!(
                "could not inspect current managed data-root identity: {error}"
            ))
        })?;
        if held_metadata.dev() != ambient_metadata.dev()
            || held_metadata.ino() != ambient_metadata.ino()
        {
            return Err(ProjectFailure::recovery(
                "managed data root changed after startup",
            ));
        }
        Ok(())
    }

    pub fn watch_project(
        &mut self,
        storage: &Storage,
        project_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_active_project(storage, project_id)?;
        let worktree = self.worktree_capability(&active)?;
        let snapshot = match snapshot_tree(&worktree) {
            Ok(snapshot) => snapshot,
            Err(_) => {
                let mut suspended = WatchState {
                    stream_id: format!("watch_{}", Uuid::new_v4().simple()),
                    snapshot: BTreeMap::new(),
                    next_seq: 1,
                    workspace: None,
                };
                return Ok(ProjectAction {
                    result: json!({"watching":false,"rescanRequired":true}),
                    watch_events: vec![watch_emission(
                        &mut suspended,
                        project_id,
                        Vec::new(),
                        true,
                    )],
                    ..ProjectAction::default()
                });
            }
        };
        self.watchers.insert(
            project_id.to_string(),
            WatchState {
                stream_id: format!("watch_{}", Uuid::new_v4().simple()),
                snapshot,
                next_seq: 1,
                workspace: None,
            },
        );
        Ok(ProjectAction {
            result: json!({"watching":true}),
            ..ProjectAction::default()
        })
    }

    pub fn workspace_watch(
        &mut self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        let worktree = self.worktree_capability(&active)?;
        let snapshot = snapshot_tree(&worktree)?;
        self.watchers.insert(
            format!("workspace:{}", address.chat_id),
            WatchState {
                stream_id: format!("workspace_watch_{}", Uuid::new_v4().simple()),
                snapshot,
                next_seq: 1,
                workspace: Some(address.clone()),
            },
        );
        Ok(ProjectAction {
            result: json!({"watching":true}),
            ..ProjectAction::default()
        })
    }

    pub fn poll_watchers(&mut self, storage: &Storage) -> Vec<WatchEmission> {
        let project_ids = self.watchers.keys().cloned().collect::<Vec<_>>();
        let mut emitted = Vec::new();
        for project_id in project_ids {
            let workspace = self
                .watchers
                .get(&project_id)
                .and_then(|state| state.workspace.clone());
            let resolved = match workspace.as_ref() {
                Some(address) => self.validate_workspace_address(storage, address),
                None => self.validate_active_project(storage, &project_id),
            };
            let emission_project_id = workspace
                .as_ref()
                .map_or(project_id.as_str(), |address| address.project_id.as_str());
            let active = match resolved {
                Ok(active) => active,
                Err(_) => {
                    if let Some(mut state) = self.watchers.remove(&project_id) {
                        emitted.push(watch_emission(
                            &mut state,
                            emission_project_id,
                            Vec::new(),
                            true,
                        ));
                    }
                    continue;
                }
            };
            let worktree = match self.worktree_capability(&active) {
                Ok(worktree) => worktree,
                Err(_) => {
                    if let Some(mut state) = self.watchers.remove(&project_id) {
                        emitted.push(watch_emission(
                            &mut state,
                            emission_project_id,
                            Vec::new(),
                            true,
                        ));
                    }
                    continue;
                }
            };
            let fresh = match snapshot_tree(&worktree) {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    if let Some(mut state) = self.watchers.remove(&project_id) {
                        emitted.push(watch_emission(
                            &mut state,
                            emission_project_id,
                            Vec::new(),
                            true,
                        ));
                    }
                    continue;
                }
            };
            let Some(state) = self.watchers.get_mut(&project_id) else {
                continue;
            };
            let mut changed = BTreeSet::new();
            for path in state.snapshot.keys().chain(fresh.keys()) {
                if state.snapshot.get(path) != fresh.get(path) {
                    changed.insert(path.clone());
                }
            }
            if changed.is_empty() {
                continue;
            }
            let overflow = changed.len() > WATCH_MAX_CHANGED_PATHS;
            let paths = if overflow {
                Vec::new()
            } else {
                changed.into_iter().collect()
            };
            state.snapshot = fresh;
            emitted.push(watch_emission(state, emission_project_id, paths, overflow));
            if overflow {
                self.watchers.remove(&project_id);
            }
        }
        emitted
    }

    pub fn stop_watchers(&mut self) {
        self.watchers.clear();
    }

    pub fn diff_summary(
        &mut self,
        storage: &Storage,
        project_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_active_project(storage, project_id)?;
        self.diff_summary_from_active(&active, project_id)
    }

    fn diff_summary_from_active(
        &self,
        active: &ActiveProject,
        project_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let entries = self.diff_entries(active)?;
        Ok(ProjectAction {
            result: json!({
                "projectId":project_id,
                "baseCommit":active.base_commit,
                "entries":entries.into_iter().map(|entry| entry.summary_value()).collect::<Vec<_>>(),
            }),
            ..ProjectAction::default()
        })
    }

    pub fn diff_file(
        &mut self,
        storage: &Storage,
        project_id: &str,
        relative_path: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        validate_relative_path(relative_path, false)?;
        let active = self.validate_active_project(storage, project_id)?;
        self.diff_file_from_active(&active, project_id, relative_path)
    }

    pub fn workspace_diff_summary(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        self.diff_summary_from_active(&active, &address.project_id)
    }

    pub fn workspace_diff_file(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        relative_path: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        validate_relative_path(relative_path, false)?;
        let active = self.validate_workspace_address(storage, address)?;
        self.diff_file_from_active(&active, &address.project_id, relative_path)
    }

    fn diff_file_from_active(
        &self,
        active: &ActiveProject,
        project_id: &str,
        relative_path: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let entry = self
            .diff_entries(active)?
            .into_iter()
            .find(|entry| entry.path == relative_path)
            .ok_or_else(|| ProjectFailure::manifest("requested file has no worktree diff"))?;
        let worktree = self.worktree_capability(active)?;
        let base_path = entry.old_path.as_deref().unwrap_or(&entry.path);
        let base = if !entry.base_exists {
            None
        } else {
            Some(self.base_content(active, base_path)?)
        };
        let current = if !entry.current_exists {
            None
        } else {
            Some(self.current_content(&worktree, &entry.path)?)
        };
        let base_version = base.as_ref().and_then(|content| content.version.clone());
        let worktree_version = current.as_ref().and_then(|content| content.version.clone());
        let binary = entry.kind == "binary"
            || base.as_ref().is_some_and(|content| content.binary)
            || current.as_ref().is_some_and(|content| content.binary);
        if binary {
            let length = base
                .as_ref()
                .map(|content| content.length)
                .unwrap_or(0)
                .max(current.as_ref().map(|content| content.length).unwrap_or(0));
            return Ok(ProjectAction {
                result: json!({
                    "projectId":project_id,"kind":"binary","path":entry.path,
                    "oldPath":Value::Null,"baseVersion":base_version,
                    "worktreeVersion":worktree_version,"byteLength":length,"stream":Value::Null,
                }),
                ..ProjectAction::default()
            });
        }
        let base_bytes = base.as_ref().and_then(|content| content.bytes.as_deref());
        let current_bytes = current
            .as_ref()
            .and_then(|content| content.bytes.as_deref());
        let text = build_bounded_text_diff(&entry, base_bytes, current_bytes)?;
        let stream_id = format!("stream_diff_{}", Uuid::new_v4().simple());
        Ok(ProjectAction {
            result: json!({
                "projectId":project_id,"kind":entry.kind,"path":entry.path,
                "oldPath":entry.old_path,"baseVersion":base_version,
                "worktreeVersion":worktree_version,"byteLength":text.len(),
                "stream":{"streamId":stream_id,"chunkBytes":MAX_TEXT_CHUNK_BYTES},
            }),
            streams: vec![TextStream {
                stream_id,
                chunks: chunk_utf8(&text, MAX_TEXT_CHUNK_BYTES),
            }],
            watch_events: Vec::new(),
        })
    }

    pub fn close_project(
        &mut self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_active_project(storage, project_id)?;
        self.watchers.remove(project_id);
        let abandoned_staging = self
            .pending_saves
            .iter()
            .filter(|(_, save)| save.project_id == project_id)
            .map(|(_, save)| save.staging_name.clone())
            .collect::<Vec<_>>();
        self.pending_saves
            .retain(|_, save| save.project_id != project_id);
        for staging_name in abandoned_staging {
            remove_capability_temp(&self.save_staging_dir, &staging_name);
        }
        write_json_atomic(
            &self.manifests_dir,
            &format!("{}.json", active.worktree_id),
            &manifest_value(&active, "abandoned")?,
        )?;
        append_object(
            storage,
            "project",
            project_id,
            &format!("op_project_close_{request_id}"),
            request_id,
            preserve_project_extensions(
                json!({
                "project_id":project_id,
                "canonical_path":canonical_protocol_path_without_existing(&active.canonical_git_root)?,
                "selected_path":canonical_protocol_path_without_existing(&active.canonical_git_root)?,
                "base_commit":active.base_commit,
                "base_tree":active.base_tree,
                "object_format":active.object_format,
                "source_identity":active.source_identity,
                "workspace_identity":active.workspace_identity,
                "lifecycle":"abandoned",
                "worktree_id":active.worktree_id,
                "worktree_path":canonical_protocol_path(&active.worktree_path)?,
                "manifest_path":canonical_protocol_path_without_existing(&active.manifest_path)?,
                "lock_id":active.lock_id,
                }),
                &required_project(storage, project_id)?,
            )?,
        )?;
        Ok(ProjectAction {
            result: json!({
                "projectId":project_id,"lifecycle":"abandoned","physicallyDeleted":false,
            }),
            ..ProjectAction::default()
        })
    }

    pub fn workspace_close(
        &mut self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<ProjectAction, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        self.watchers
            .remove(&format!("workspace:{}", address.chat_id));
        let abandoned = self
            .pending_saves
            .iter()
            .filter(|(_, save)| {
                save.chat_id.as_deref() == Some(address.chat_id.as_str())
                    && save.worktree_id == address.worktree_id
                    && save.workspace_identity == address.workspace_identity
            })
            .map(|(save_id, save)| (save_id.clone(), save.staging_name.clone()))
            .collect::<Vec<_>>();
        for (save_id, staging_name) in abandoned {
            self.pending_saves.remove(&save_id);
            remove_capability_temp(&self.save_staging_dir, staging_name);
        }
        write_json_atomic(
            &self.manifests_dir,
            &format!("{}.json", active.worktree_id),
            &manifest_value(&active, "abandoned")?,
        )?;
        Ok(ProjectAction {
            result: json!({"lifecycle":"abandoned","physicallyDeleted":false}),
            ..ProjectAction::default()
        })
    }

    pub fn workspace_mark_recovery_required(
        &mut self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<(), ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        write_json_atomic(
            &self.manifests_dir,
            &format!("{}.json", active.worktree_id),
            &manifest_value(&active, "recoveryRequired")?,
        )
    }

    pub fn workspace_ensure_recovery_abandoned(
        &mut self,
        address: &WorkspaceAddress,
        expected_recovery_manifest: &Value,
        expected_manifest_digest: &str,
    ) -> Result<(), ProjectFailure> {
        if manifest_identity_digest(expected_recovery_manifest)? != expected_manifest_digest {
            return Err(ProjectFailure::manifest(
                "recovery intent manifest digest differs",
            ));
        }
        let manifest_name = format!("{}.json", address.worktree_id);
        let manifest = read_bounded_json(&self.manifests_dir, &manifest_name)?;
        for (field, expected) in [
            ("projectId", address.project_id.as_str()),
            ("worktreeId", address.worktree_id.as_str()),
            ("workspaceIdentity", address.workspace_identity.as_str()),
        ] {
            if manifest.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "recovery workspace identity or lifecycle differs",
                ));
            }
        }
        let lifecycle = manifest.get("lifecycle").and_then(Value::as_str);
        if !matches!(lifecycle, Some("recoveryRequired" | "abandoned")) {
            return Err(ProjectFailure::manifest(
                "recovery workspace lifecycle is not reconcilable",
            ));
        }
        let mut comparable = manifest.clone();
        comparable["lifecycle"] = Value::String("recoveryRequired".to_string());
        if comparable != *expected_recovery_manifest
            || manifest_identity_digest(&comparable)? != expected_manifest_digest
        {
            return Err(ProjectFailure::manifest(
                "recovery workspace manifest differs from its resolution intent",
            ));
        }
        if lifecycle == Some("abandoned") {
            self.watchers
                .remove(&format!("workspace:{}", address.chat_id));
            return Ok(());
        }
        let mut abandoned = manifest;
        abandoned["lifecycle"] = Value::String("abandoned".to_string());
        write_json_atomic(&self.manifests_dir, &manifest_name, &abandoned)?;
        self.watchers
            .remove(&format!("workspace:{}", address.chat_id));
        Ok(())
    }

    pub fn workspace_verify_recovery_abandoned(
        &self,
        address: &WorkspaceAddress,
        expected_recovery_manifest: &Value,
        expected_manifest_digest: &str,
    ) -> Result<(), ProjectFailure> {
        if manifest_identity_digest(expected_recovery_manifest)? != expected_manifest_digest {
            return Err(ProjectFailure::manifest(
                "recovery intent manifest digest differs",
            ));
        }
        let manifest = read_bounded_json(
            &self.manifests_dir,
            &format!("{}.json", address.worktree_id),
        )?;
        let mut comparable = manifest.clone();
        if comparable.get("lifecycle").and_then(Value::as_str) != Some("abandoned") {
            return Err(ProjectFailure::manifest(
                "recovery workspace is not durably abandoned",
            ));
        }
        comparable["lifecycle"] = Value::String("recoveryRequired".to_string());
        if comparable != *expected_recovery_manifest
            || manifest_identity_digest(&comparable)? != expected_manifest_digest
        {
            return Err(ProjectFailure::manifest(
                "abandoned recovery workspace differs from its resolution intent",
            ));
        }
        Ok(())
    }

    pub fn workspace_abandon_recovery(
        &mut self,
        address: &WorkspaceAddress,
    ) -> Result<(), ProjectFailure> {
        let manifest_name = format!("{}.json", address.worktree_id);
        let manifest = read_bounded_json(&self.manifests_dir, &manifest_name)?;
        for (field, expected) in [
            ("projectId", address.project_id.as_str()),
            ("worktreeId", address.worktree_id.as_str()),
            ("workspaceIdentity", address.workspace_identity.as_str()),
            ("lifecycle", "recoveryRequired"),
        ] {
            if manifest.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "recovery workspace identity or lifecycle differs",
                ));
            }
        }
        let mut abandoned = manifest;
        abandoned["lifecycle"] = Value::String("abandoned".to_string());
        write_json_atomic(&self.manifests_dir, &manifest_name, &abandoned)?;
        self.watchers
            .remove(&format!("workspace:{}", address.chat_id));
        Ok(())
    }

    pub fn workspace_restore_recovery_required(
        &mut self,
        address: &WorkspaceAddress,
    ) -> Result<(), ProjectFailure> {
        let manifest_name = format!("{}.json", address.worktree_id);
        let mut manifest = read_bounded_json(&self.manifests_dir, &manifest_name)?;
        for (field, expected) in [
            ("projectId", address.project_id.as_str()),
            ("worktreeId", address.worktree_id.as_str()),
            ("workspaceIdentity", address.workspace_identity.as_str()),
        ] {
            if manifest.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "unclean recovery workspace identity differs from its manifest",
                ));
            }
        }
        if !matches!(
            manifest.get("lifecycle").and_then(Value::as_str),
            Some("active" | "recoveryRequired")
        ) {
            return Err(ProjectFailure::manifest(
                "unclean recovery workspace is already terminal",
            ));
        }
        manifest["lifecycle"] = Value::String("recoveryRequired".to_string());
        write_json_atomic(&self.manifests_dir, &manifest_name, &manifest)
    }

    fn persist_pre_activation(
        &self,
        storage: &mut Storage,
        active: &ActiveProject,
        previous: &Map<String, Value>,
        request_id: &str,
    ) -> Result<(), ProjectFailure> {
        append_object(
            storage,
            "project",
            &active.project_id,
            &format!("op_snapshot_database_committed_{}", active.worktree_id),
            request_id,
            preserve_project_extensions(
                json!({
                    "project_id":active.project_id,
                    "canonical_path":canonical_protocol_path_without_existing(&active.canonical_git_root)?,
                    "selected_path":canonical_protocol_path_without_existing(&active.canonical_git_root)?,
                    "base_commit":active.base_commit,
                    "base_tree":active.base_tree,
                    "object_format":active.object_format,
                    "source_identity":active.source_identity,
                    "workspace_identity":active.workspace_identity,
                    "lifecycle":"recoveryRequired",
                    "worktree_id":active.worktree_id,
                    "worktree_path":canonical_protocol_path(&active.worktree_path)?,
                    "manifest_path":canonical_protocol_path_without_existing(&active.manifest_path)?,
                    "lock_id":active.lock_id,
                }),
                previous,
            )?,
        )
    }

    fn persist_active(
        &self,
        storage: &mut Storage,
        active: &ActiveProject,
        previous: &Map<String, Value>,
        request_id: &str,
    ) -> Result<(), ProjectFailure> {
        append_object(
            storage,
            "project",
            &active.project_id,
            &format!("op_worktree_active_project_{}", active.worktree_id),
            request_id,
            preserve_project_extensions(
                json!({
                    "project_id":active.project_id,
                    "canonical_path":canonical_protocol_path_without_existing(&active.canonical_git_root)?,
                    "selected_path":canonical_protocol_path_without_existing(&active.canonical_git_root)?,
                    "base_commit":active.base_commit,
                    "base_tree":active.base_tree,
                    "object_format":active.object_format,
                    "source_identity":active.source_identity,
                    "workspace_identity":active.workspace_identity,
                    "lifecycle":"active",
                    "worktree_id":active.worktree_id,
                    "worktree_path":canonical_protocol_path(&active.worktree_path)?,
                    "manifest_path":canonical_protocol_path_without_existing(&active.manifest_path)?,
                    "lock_id":active.lock_id,
                }),
                previous,
            )?,
        )?;
        append_object(
            storage,
            "lock",
            &active.lock_id,
            &format!("op_worktree_active_lock_{}", active.worktree_id),
            request_id,
            json!({
                "lock_id":active.lock_id,
                "project_id":active.project_id,
                "writer_run_id":Value::Null,
                "worktree_path":canonical_protocol_path(&active.worktree_path)?,
                "poisoned":false,"recovery_required":false,
                "evidence":{"phase":"activated","manifest":canonical_protocol_path_without_existing(&active.manifest_path)?,
                    "workspace_identity":active.workspace_identity},
            }),
        )?;
        Ok(())
    }

    fn persist_save_lock(
        &self,
        storage: &mut Storage,
        active: &ActiveProject,
        save_id: &str,
        request_id: &str,
        recovery_required: bool,
        committed_version: Option<&str>,
    ) -> Result<(), ProjectFailure> {
        let phase = if recovery_required {
            "saveBeforeRename"
        } else {
            "saveCommitted"
        };
        append_object(
            storage,
            "lock",
            &active.lock_id,
            &format!(
                "op_save_{}_{}",
                if recovery_required {
                    "prepare"
                } else {
                    "commit"
                },
                save_id
            ),
            request_id,
            json!({
                "lock_id":active.lock_id,
                "project_id":active.project_id,
                "writer_run_id":Value::Null,
                "worktree_path":canonical_protocol_path(&active.worktree_path)?,
                "poisoned":false,
                "recovery_required":recovery_required,
                "evidence":{
                    "phase":phase,
                    "save_id":save_id,
                    "committed_version":committed_version,
                },
            }),
        )
    }

    fn validate_active_project(
        &self,
        storage: &Storage,
        project_id: &str,
    ) -> Result<ActiveProject, ProjectFailure> {
        let record = required_project(storage, project_id)?;
        if value_string(&record, "lifecycle")? != "active" {
            return Err(ProjectFailure::recovery(
                "project snapshot is not active and cannot accept file or diff operations",
            ));
        }
        let worktree_id = value_string(&record, "worktree_id")?.to_string();
        let worktree_path = self.worktrees_root.join(&worktree_id);
        if record.get("worktree_path").and_then(Value::as_str)
            != Some(canonical_protocol_path(&worktree_path)?.as_str())
        {
            return Err(ProjectFailure::manifest(
                "persisted snapshot path is not the exact managed child",
            ));
        }
        let manifest_path = PathBuf::from(value_string(&record, "manifest_path")?);
        let manifest_name = format!("{worktree_id}.json");
        if manifest_path != self.manifests_root.join(&manifest_name) {
            return Err(ProjectFailure::manifest(
                "persisted manifest is not the exact managed snapshot manifest child",
            ));
        }
        let canonical_git_root = PathBuf::from(value_string(&record, "canonical_path")?);
        if worktree_path == canonical_git_root || worktree_path.starts_with(&canonical_git_root) {
            return Err(ProjectFailure::manifest(
                "managed snapshot overlaps the original project",
            ));
        }
        let active = ActiveProject {
            project_id: project_id.to_string(),
            canonical_git_root,
            worktree_id,
            worktree_path,
            base_commit: value_string(&record, "base_commit")?.to_string(),
            base_tree: value_string(&record, "base_tree")?.to_string(),
            object_format: value_string(&record, "object_format")?.to_string(),
            source_identity: value_string(&record, "source_identity")?.to_string(),
            workspace_identity: value_string(&record, "workspace_identity")?.to_string(),
            manifest_path,
            lock_id: value_string(&record, "lock_id")?.to_string(),
        };

        let manifest = read_bounded_json(&self.manifests_dir, &manifest_name)?;
        if manifest != manifest_value(&active, "active")? {
            return Err(ProjectFailure::manifest(
                "persisted manifest differs from the canonical snapshot identity",
            ));
        }
        validate_project_payload_v1("worktreeManifest", &manifest).map_err(|error| {
            ProjectFailure::manifest(format!("persisted snapshot manifest is invalid: {error}"))
        })?;

        let lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::manifest("snapshot lock projection is missing"))?;
        if lock.get("poisoned").and_then(Value::as_bool) != Some(false)
            || lock.get("recovery_required").and_then(Value::as_bool) != Some(false)
            || lock.get("project_id").and_then(Value::as_str) != Some(project_id)
            || lock.get("worktree_path").and_then(Value::as_str)
                != Some(canonical_protocol_path(&active.worktree_path)?.as_str())
        {
            return Err(ProjectFailure::recovery(
                "snapshot lock is poisoned, recovery-required, or mismatched",
            ));
        }

        let worktree = self.worktree_capability(&active)?;
        let marker = read_bounded_json(&open_real_child_dir(&worktree, ".git")?, SNAPSHOT_MARKER)?;
        if marker != snapshot_identity(&active)? {
            return Err(ProjectFailure::manifest(
                "snapshot Git marker differs from the active identity",
            ));
        }
        self.verify_snapshot_repository(&worktree, &active)?;
        Ok(active)
    }

    fn validate_workspace_address(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<ActiveProject, ProjectFailure> {
        let chat = storage
            .projection_record("chat", &address.chat_id)?
            .ok_or_else(|| ProjectFailure::manifest("chat workspace binding is missing"))?;
        for (field, expected) in [
            ("chat_id", address.chat_id.as_str()),
            ("project_id", address.project_id.as_str()),
            ("worktree_id", address.worktree_id.as_str()),
            ("workspace_identity", address.workspace_identity.as_str()),
        ] {
            if chat.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "chat workspace reference differs from the durable chat binding",
                ));
            }
        }
        if chat.get("state").and_then(Value::as_str) == Some("discarded") {
            return Err(ProjectFailure::recovery(
                "discarded chat workspace cannot be reused",
            ));
        }

        let project = required_project(storage, &address.project_id)?;
        let manifest_name = format!("{}.json", address.worktree_id);
        let manifest = read_bounded_json(&self.manifests_dir, &manifest_name)?;
        validate_project_payload_v1("worktreeManifest", &manifest).map_err(|error| {
            ProjectFailure::manifest(format!("persisted snapshot manifest is invalid: {error}"))
        })?;
        for (field, expected) in [
            ("projectId", address.project_id.as_str()),
            ("worktreeId", address.worktree_id.as_str()),
            ("workspaceIdentity", address.workspace_identity.as_str()),
        ] {
            if manifest.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "workspace manifest differs from the durable chat binding",
                ));
            }
        }
        if manifest.get("lifecycle").and_then(Value::as_str) != Some("active") {
            return Err(ProjectFailure::recovery(
                "chat workspace snapshot is not active",
            ));
        }
        let canonical_git_root = PathBuf::from(value_string(&project, "canonical_path")?);
        if manifest.get("canonicalGitRoot").and_then(Value::as_str)
            != Some(canonical_protocol_path_without_existing(&canonical_git_root)?.as_str())
        {
            return Err(ProjectFailure::manifest(
                "workspace canonical root differs from the durable project",
            ));
        }
        let active = ActiveProject {
            project_id: address.project_id.clone(),
            canonical_git_root,
            worktree_id: address.worktree_id.clone(),
            worktree_path: self.worktrees_root.join(&address.worktree_id),
            base_commit: value_string(
                manifest.as_object().ok_or_else(|| {
                    ProjectFailure::manifest("workspace manifest is not an object")
                })?,
                "baseCommit",
            )?
            .to_string(),
            base_tree: manifest["baseTree"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            object_format: manifest["objectFormat"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            source_identity: manifest["sourceIdentity"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            workspace_identity: address.workspace_identity.clone(),
            manifest_path: self.manifests_root.join(&manifest_name),
            lock_id: format!("lock_{}", address.worktree_id),
        };
        if manifest != manifest_value(&active, "active")? {
            return Err(ProjectFailure::manifest(
                "workspace manifest is not the canonical snapshot identity",
            ));
        }
        let lock = storage
            .projection_record("lock", &active.lock_id)?
            .ok_or_else(|| ProjectFailure::manifest("workspace lock projection is missing"))?;
        if lock.get("poisoned").and_then(Value::as_bool) != Some(false)
            || lock.get("recovery_required").and_then(Value::as_bool) != Some(false)
        {
            return Err(ProjectFailure::recovery(
                "workspace is locked, poisoned, or recovery-required",
            ));
        }
        let worktree = self.worktree_capability(&active)?;
        let marker = read_bounded_json(&open_real_child_dir(&worktree, ".git")?, SNAPSHOT_MARKER)?;
        if marker != snapshot_identity(&active)? {
            return Err(ProjectFailure::manifest(
                "workspace Git marker differs from the durable chat binding",
            ));
        }
        self.verify_snapshot_repository(&worktree, &active)?;
        Ok(active)
    }

    pub fn verify_workspace(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<Value, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        manifest_value(&active, "active")
    }

    /// Point-in-time admission. Reservation callers recheck while synchronously
    /// holding the sole mutable Storage owner through commit (EXCLUSIVE SQLite).
    /// Do not cache this result across a scheduler yield as execution authority.
    pub(crate) fn verify_workspace_for_run(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<Value, ProjectFailure> {
        let project = required_project(storage, &address.project_id)?;
        if project
            .get("application_intent")
            .is_some_and(|intent| intent["state"] != "abandoned")
        {
            return Err(ProjectFailure::recovery(
                "project has an unfinished source application",
            ));
        }
        self.verify_workspace(storage, address)
    }

    /// Point-in-time file admission, not a sandbox or protection against a hostile
    /// same-UID writer racing this scan. Scan BEFORE Git verification can run.
    #[cfg(unix)]
    pub(crate) fn verify_runtime_file_isolation(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
    ) -> Result<(), ProjectFailure> {
        self.verify_managed_paths()?;
        let root = open_real_child_dir(&self.worktrees_dir, &address.worktree_id)?;
        runtime_isolation::verify(&root)?;
        self.validate_workspace_address(storage, address)?;
        Ok(())
    }

    pub fn verify_unbound_workspace(
        &self,
        storage: &Storage,
        project_id: &str,
        worktree_id: &str,
        workspace_identity: &str,
    ) -> Result<(), ProjectFailure> {
        let project = required_project(storage, project_id)?;
        let manifest_name = format!("{worktree_id}.json");
        let manifest = read_bounded_json(&self.manifests_dir, &manifest_name)?;
        validate_project_payload_v1("worktreeManifest", &manifest).map_err(|error| {
            ProjectFailure::manifest(format!("persisted snapshot manifest is invalid: {error}"))
        })?;
        for (field, expected) in [
            ("projectId", project_id),
            ("worktreeId", worktree_id),
            ("workspaceIdentity", workspace_identity),
            ("lifecycle", "active"),
        ] {
            if manifest.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "requested chat identity does not name an active standalone snapshot",
                ));
            }
        }
        if project.get("canonical_path").and_then(Value::as_str)
            != manifest.get("canonicalGitRoot").and_then(Value::as_str)
        {
            return Err(ProjectFailure::manifest(
                "snapshot and project canonical roots differ",
            ));
        }
        let lock = storage
            .projection_record("lock", &format!("lock_{worktree_id}"))?
            .ok_or_else(|| ProjectFailure::manifest("snapshot lock projection is missing"))?;
        if lock.get("poisoned").and_then(Value::as_bool) != Some(false)
            || lock.get("recovery_required").and_then(Value::as_bool) != Some(false)
        {
            return Err(ProjectFailure::recovery(
                "snapshot is poisoned or recovery-required",
            ));
        }
        Ok(())
    }

    pub fn abandon_unbound_workspace(
        &mut self,
        storage: &Storage,
        project_id: &str,
        worktree_id: &str,
        workspace_identity: &str,
    ) -> Result<(), ProjectFailure> {
        let manifest_name = format!("{worktree_id}.json");
        let mut manifest = read_bounded_json(&self.manifests_dir, &manifest_name)?;
        for (field, expected) in [
            ("projectId", project_id),
            ("worktreeId", worktree_id),
            ("workspaceIdentity", workspace_identity),
        ] {
            if manifest.get(field).and_then(Value::as_str) != Some(expected) {
                return Err(ProjectFailure::manifest(
                    "prepared workspace identity differs from its manifest",
                ));
            }
        }
        let lifecycle = manifest.get("lifecycle").and_then(Value::as_str);
        if !matches!(lifecycle, Some("active" | "abandoned")) {
            return Err(ProjectFailure::manifest(
                "prepared workspace lifecycle cannot be abandoned",
            ));
        }
        let project = required_project(storage, project_id)?;
        if project.get("canonical_path").and_then(Value::as_str)
            != manifest.get("canonicalGitRoot").and_then(Value::as_str)
        {
            return Err(ProjectFailure::manifest(
                "prepared workspace canonical root differs from its project",
            ));
        }
        let lock_id = format!("lock_{worktree_id}");
        let lock = storage
            .projection_record("lock", &lock_id)?
            .ok_or_else(|| ProjectFailure::manifest("prepared workspace lock is missing"))?;
        if lock.get("project_id").and_then(Value::as_str) != Some(project_id)
            || lock.get("worktree_path").and_then(Value::as_str)
                != Some(canonical_protocol_path(&self.worktrees_root.join(worktree_id))?.as_str())
            || lock.get("poisoned").and_then(Value::as_bool) != Some(false)
            || lock.get("recovery_required").and_then(Value::as_bool) != Some(false)
        {
            return Err(ProjectFailure::recovery(
                "prepared workspace lock is poisoned, recovery-required, or mismatched",
            ));
        }
        if lifecycle == Some("abandoned") {
            return Ok(());
        }
        manifest["lifecycle"] = Value::String("abandoned".to_string());
        write_json_atomic(&self.manifests_dir, &manifest_name, &manifest)
    }

    fn worktree_capability(&self, active: &ActiveProject) -> Result<Dir, ProjectFailure> {
        let expected = self.worktrees_root.join(&active.worktree_id);
        if active.worktree_path != expected {
            return Err(ProjectFailure::manifest(
                "snapshot path is not the exact child identified by its durable worktree id",
            ));
        }
        open_real_child_dir(&self.worktrees_dir, &active.worktree_id)
    }

    fn open_source_repository(&self, selected: &Path) -> Result<SourceRepository, ProjectFailure> {
        let selected = canonical_existing_directory(selected)?;
        let mut candidate = Some(selected.as_path());
        let (canonical_root, root, git_dir) = loop {
            let path = candidate.ok_or_else(|| {
                ProjectFailure::manifest(
                    "selected directory is not inside a supported Git repository",
                )
            })?;
            let root = open_absolute_directory_capability(path)?;
            match root.symlink_metadata(".git") {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    let git_dir = open_real_child_dir(&root, ".git")?;
                    break (path.to_path_buf(), root, git_dir);
                }
                Ok(_) => {
                    return Err(ProjectFailure::manifest(
                        "source repository must own a real .git directory; linked worktrees are unsupported",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    candidate = path.parent();
                }
                Err(error) => {
                    return Err(ProjectFailure::manifest(format!(
                        "could not inspect source .git entry: {error}"
                    )));
                }
            }
        };

        if trim_output(&self.git_at(&git_dir, ["rev-parse", "--is-bare-repository"])?)? != "false" {
            return Err(ProjectFailure::manifest(
                "bare Git repositories are unsupported",
            ));
        }
        let object_format =
            trim_output(&self.git_at(&git_dir, ["rev-parse", "--show-object-format"])?)?
                .to_string();
        if !matches!(object_format.as_str(), "sha1" | "sha256") {
            return Err(ProjectFailure::manifest("unsupported Git object format"));
        }
        let base_commit = validated_oid(
            trim_output(&self.git_at(&git_dir, ["rev-parse", "--verify", "HEAD^{commit}"])?)?,
            &object_format,
        )?;
        let base_tree = validated_oid(
            trim_output(&self.git_at(&git_dir, ["rev-parse", "--verify", "HEAD^{tree}"])?)?,
            &object_format,
        )?;
        let mut identity = Sha256::new();
        identity.update(canonical_protocol_path(&canonical_root)?.as_bytes());
        identity.update([0]);
        identity.update(object_format.as_bytes());
        identity.update([0]);
        identity.update(base_commit.as_bytes());
        identity.update([0]);
        identity.update(base_tree.as_bytes());
        let source_identity = format!("source_{:x}", identity.finalize());
        Ok(SourceRepository {
            canonical_root,
            _root: root,
            git_dir,
            object_format,
            base_commit,
            base_tree,
            source_identity,
        })
    }

    fn export_bundle(&self, source_git: &Dir, staging: &Dir) -> Result<u64, ProjectFailure> {
        let mut options = CapOpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut bundle = staging
            .open_with(SNAPSHOT_BUNDLE, &options)
            .map_err(|error| {
                ProjectFailure::recovery(format!("could not create managed bundle: {error}"))
            })?;
        let mut command = self.git_command_at(source_git)?;
        command
            .args(["bundle", "create", "-", "HEAD"])
            .stdout(Stdio::piped());
        let mut child = StreamingGitChild::spawn(&mut command).map_err(|error| {
            ProjectFailure::recovery(format!("could not start bundle export: {error}"))
        })?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProjectFailure::recovery("bundle export did not expose stdout"))?;
        let mut total = 0_u64;
        let mut buffer = vec![0_u8; MAX_TEXT_CHUNK_BYTES];
        loop {
            let read = match stdout.read(&mut buffer) {
                Ok(read) => read,
                Err(error) => {
                    return Err(ProjectFailure::recovery(format!(
                        "could not stream bundle export: {error}"
                    )));
                }
            };
            if read == 0 {
                break;
            }
            total = checked_bundle_size(total, read as u64)?;
            if let Err(error) = bundle.write_all(&buffer[..read]) {
                return Err(ProjectFailure::recovery(format!(
                    "could not persist bundle stream: {error}"
                )));
            }
        }
        let (status, stderr) = child.finish().map_err(|error| {
            ProjectFailure::recovery(format!("could not reap bundle export: {error}"))
        })?;
        if !status.success() {
            return Err(ProjectFailure::recovery(format!(
                "bundle export failed with exit {}: {}",
                status.code().unwrap_or(-1),
                bounded_stderr(&stderr)
            )));
        }
        bundle.flush().map_err(|error| {
            ProjectFailure::recovery(format!("could not flush bundle: {error}"))
        })?;
        bundle.sync_all().map_err(|error| {
            ProjectFailure::recovery(format!("could not durably store bundle: {error}"))
        })?;
        bundle.seek(SeekFrom::Start(0)).map_err(|error| {
            ProjectFailure::recovery(format!("could not rewind bundle: {error}"))
        })?;
        if total == 0 {
            return Err(ProjectFailure::manifest("bundle export was empty"));
        }
        Ok(total)
    }

    fn initialize_snapshot_repository(
        &self,
        staging: &Dir,
        source: &SourceRepository,
    ) -> Result<(), ProjectFailure> {
        self.git_at(
            staging,
            [
                "init",
                "--quiet",
                &format!("--object-format={}", source.object_format),
                ".",
            ],
        )?;
        self.git_at(staging, ["bundle", "verify", SNAPSHOT_BUNDLE])?;
        let heads = self.git_limited_bytes_at(
            staging,
            ["bundle", "list-heads", SNAPSHOT_BUNDLE],
            &[0],
            256,
        )?;
        if heads != format!("{} HEAD\n", source.base_commit).as_bytes() {
            return Err(ProjectFailure::manifest(
                "import bundle HEAD differs from the source identity",
            ));
        }
        self.git_at(staging, ["bundle", "unbundle", SNAPSHOT_BUNDLE])?;
        staging.remove_file(SNAPSHOT_BUNDLE).map_err(|error| {
            ProjectFailure::recovery(format!("could not remove imported bundle: {error}"))
        })?;
        self.git_at(
            staging,
            [
                "update-ref",
                "--no-deref",
                "HEAD",
                source.base_commit.as_str(),
            ],
        )?;
        self.git_at(staging, ["read-tree", source.base_tree.as_str()])?;
        Ok(())
    }

    fn materialize_snapshot(
        &self,
        staging: &Dir,
        source: &SourceRepository,
    ) -> Result<SnapshotStats, ProjectFailure> {
        let mut ls = self.git_command_at(staging)?;
        ls.args(["ls-tree", "-rz", "--full-tree", source.base_tree.as_str()])
            .stdout(Stdio::piped());
        let mut ls_child = StreamingGitChild::spawn(&mut ls).map_err(|error| {
            ProjectFailure::recovery(format!("could not start tree stream: {error}"))
        })?;
        let stdout = ls_child
            .stdout
            .take()
            .ok_or_else(|| ProjectFailure::recovery("tree stream did not expose stdout"))?;
        let mut tree = BufReader::new(stdout);

        let mut cat = self.git_command_at(staging)?;
        cat.args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut cat_child = StreamingGitChild::spawn(&mut cat).map_err(|error| {
            ProjectFailure::recovery(format!("could not start blob stream: {error}"))
        })?;
        let mut cat_stdin = cat_child
            .stdin
            .take()
            .ok_or_else(|| ProjectFailure::recovery("blob stream did not expose stdin"))?;
        let cat_stdout = cat_child
            .stdout
            .take()
            .ok_or_else(|| ProjectFailure::recovery("blob stream did not expose stdout"))?;
        let mut cat_stdout = BufReader::new(cat_stdout);

        let mut stats = SnapshotStats {
            bundle_bytes: 0,
            materialized_bytes: 0,
            entry_count: 0,
            largest_blob: 0,
        };
        let mut folded_paths = BTreeSet::new();
        let mut record = Vec::new();
        loop {
            record.clear();
            let read = tree.read_until(0, &mut record).map_err(|error| {
                ProjectFailure::recovery(format!("could not read tree stream: {error}"))
            })?;
            if read == 0 {
                break;
            }
            if record.last() != Some(&0) {
                return Err(ProjectFailure::manifest("tree stream is truncated"));
            }
            record.pop();
            let entry = parse_tree_record(&record, &source.object_format)?;
            stats.entry_count = stats
                .entry_count
                .checked_add(1)
                .ok_or_else(|| ProjectFailure::manifest("snapshot entry count overflow"))?;
            if stats.entry_count > SNAPSHOT_ENTRIES_MAX_V1 {
                return Err(ProjectFailure::manifest(
                    "snapshot contains more than 200,000 entries",
                ));
            }
            let folded = entry.path.to_lowercase();
            if !folded_paths.insert(folded) {
                return Err(ProjectFailure::manifest(
                    "snapshot contains a case-colliding path",
                ));
            }
            if entry
                .path
                .split('/')
                .any(|component| component.eq_ignore_ascii_case(".git"))
            {
                return Err(ProjectFailure::manifest(
                    "committed tree contains a forbidden .git entry",
                ));
            }
            validate_relative_path(&entry.path, false)?;
            create_parent_directories(staging, &entry.path)?;

            if entry.mode == "160000" {
                create_directory_path(staging, &entry.path)?;
                continue;
            }
            let size = request_blob_header(
                &mut cat_stdin,
                &mut cat_stdout,
                &entry.oid,
                &source.object_format,
            )?;
            if size > SNAPSHOT_SINGLE_BLOB_MAX_BYTES_V1 {
                return Err(ProjectFailure::manifest(
                    "snapshot contains a blob larger than 1 GiB",
                ));
            }
            stats.materialized_bytes = stats
                .materialized_bytes
                .checked_add(size)
                .ok_or_else(|| ProjectFailure::manifest("materialized byte count overflow"))?;
            if stats.materialized_bytes > SNAPSHOT_MATERIALIZED_TOTAL_MAX_BYTES_V1 {
                return Err(ProjectFailure::manifest(
                    "snapshot materialized bytes exceed 4 GiB",
                ));
            }
            stats.largest_blob = stats.largest_blob.max(size);
            if entry.mode == "120000" {
                if size > 64 * 1024 {
                    return Err(ProjectFailure::manifest(
                        "committed symlink target exceeds 64 KiB",
                    ));
                }
                let mut bytes = vec![0; size as usize];
                cat_stdout.read_exact(&mut bytes).map_err(|error| {
                    ProjectFailure::recovery(format!("could not read symlink blob: {error}"))
                })?;
                consume_batch_newline(&mut cat_stdout)?;
                let target = std::str::from_utf8(&bytes).map_err(|_| {
                    ProjectFailure::manifest("committed symlink target is not UTF-8")
                })?;
                staging
                    .symlink_contents(target, &entry.path)
                    .map_err(|error| {
                        ProjectFailure::recovery(format!(
                            "could not materialize committed symlink: {error}"
                        ))
                    })?;
            } else {
                materialize_regular_blob(
                    staging,
                    &entry.path,
                    entry.mode == "100755",
                    size,
                    &mut cat_stdout,
                )?;
            }
        }

        drop(cat_stdin);
        let (ls_status, ls_stderr) = ls_child.finish().map_err(|error| {
            ProjectFailure::recovery(format!("could not reap tree stream: {error}"))
        })?;
        if !ls_status.success() {
            return Err(ProjectFailure::manifest(format!(
                "tree stream failed: {}",
                bounded_stderr(&ls_stderr)
            )));
        }
        let (cat_status, cat_stderr) = cat_child.finish().map_err(|error| {
            ProjectFailure::recovery(format!("could not reap blob stream: {error}"))
        })?;
        if !cat_status.success() {
            return Err(ProjectFailure::manifest(format!(
                "blob stream failed: {}",
                bounded_stderr(&cat_stderr)
            )));
        }
        sync_capability_directory(staging)?;
        Ok(stats)
    }

    fn verify_snapshot_repository(
        &self,
        workspace: &Dir,
        active: &ActiveProject,
    ) -> Result<(), ProjectFailure> {
        let head_output = self.git_at(workspace, ["rev-parse", "--verify", "HEAD^{commit}"])?;
        let head = trim_output(&head_output)?;
        if head != active.base_commit {
            return Err(ProjectFailure::manifest(
                "snapshot detached HEAD differs from its base commit",
            ));
        }
        let tree_output = self.git_at(workspace, ["write-tree"])?;
        let tree = trim_output(&tree_output)?;
        if tree != active.base_tree {
            return Err(ProjectFailure::manifest(
                "snapshot index differs from its base tree",
            ));
        }
        let symbolic =
            self.git_allow_status_at(workspace, ["symbolic-ref", "-q", "HEAD"], &[0, 1])?;
        if symbolic.status.success() || !symbolic.stdout.is_empty() {
            return Err(ProjectFailure::manifest("snapshot HEAD is not detached"));
        }
        self.git_at(
            workspace,
            [
                "fsck",
                "--strict",
                "--connectivity-only",
                "--no-reflogs",
                active.base_commit.as_str(),
            ],
        )?;
        let remotes = self.git_limited_bytes_at(workspace, ["remote"], &[0], 1024)?;
        if !remotes.is_empty() {
            return Err(ProjectFailure::manifest("snapshot retains a Git remote"));
        }
        let git_dir = open_real_child_dir(workspace, ".git")?;
        if git_dir.symlink_metadata("objects/info/alternates").is_ok() {
            return Err(ProjectFailure::manifest(
                "snapshot retains an object alternate",
            ));
        }
        if count_git_refs(&git_dir)? != 0 {
            return Err(ProjectFailure::manifest("snapshot retains source refs"));
        }
        if git_metadata_contains(
            &git_dir,
            canonical_protocol_path_without_existing(&active.canonical_git_root)?.as_bytes(),
        )? {
            return Err(ProjectFailure::manifest(
                "snapshot Git metadata retains the source absolute path",
            ));
        }
        Ok(())
    }

    fn snapshot_invariant(
        &self,
        workspace: &Dir,
        active: &ActiveProject,
    ) -> Result<Value, ProjectFailure> {
        self.verify_snapshot_repository(workspace, active)?;
        Ok(json!({
            "workspaceIdentity":active.workspace_identity,
            "objectFormat":active.object_format,
            "headCommit":active.base_commit,
            "baseCommit":active.base_commit,
            "indexTree":active.base_tree,
            "baseTree":active.base_tree,
            "remoteCount":0,
            "alternatesCount":0,
            "sourceRefCount":0,
            "sourceAbsolutePathCount":0,
            "objectConnectivity":true,
            "externalManifestIdentity":active.workspace_identity,
            "gitMarkerIdentity":active.workspace_identity,
            "databaseIdentity":active.workspace_identity,
        }))
    }

    fn diff_entries(&self, active: &ActiveProject) -> Result<Vec<DiffEntry>, ProjectFailure> {
        let worktree = self.worktree_capability(active)?;
        let output = self.git_limited_bytes_at(
            &worktree,
            [
                "diff",
                "--name-status",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                active.base_commit.as_str(),
                "--",
            ],
            &[0],
            DIFF_MAX_BYTES,
        )?;
        let fields = output
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty())
            .map(protocol_git_path)
            .collect::<Result<Vec<_>, _>>()?;
        let mut index = 0;
        let mut entries = Vec::new();
        while index < fields.len() {
            let status = &fields[index];
            index += 1;
            if status.starts_with('R') {
                if index + 1 >= fields.len() {
                    return Err(ProjectFailure::manifest("Git rename output is truncated"));
                }
                let old = fields[index].clone();
                let path = fields[index + 1].clone();
                index += 2;
                entries.push(self.classify_diff(active, &worktree, "renamed", path, Some(old))?);
            } else {
                let path = fields
                    .get(index)
                    .ok_or_else(|| ProjectFailure::manifest("Git diff output is truncated"))?
                    .clone();
                index += 1;
                let kind = match status.as_str() {
                    "A" => "added",
                    "M" | "T" => "modified",
                    "D" => "deleted",
                    _ => {
                        return Err(ProjectFailure::manifest(format!(
                            "unsupported Git diff status: {status}"
                        )))
                    }
                };
                entries.push(self.classify_diff(active, &worktree, kind, path, None)?);
            }
            if entries.len() > DIFF_MAX_ENTRIES {
                return Err(ProjectFailure::manifest(
                    "diff summary exceeds its bounded 10,000-entry capacity",
                ));
            }
        }
        let untracked = self.git_limited_bytes_at(
            &worktree,
            ["ls-files", "--others", "--exclude-standard", "-z", "--"],
            &[0],
            DIFF_MAX_BYTES,
        )?;
        for path in untracked
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            let path = protocol_git_path(path)?;
            if !is_internal_relative_path(&path) {
                entries.push(self.classify_diff(active, &worktree, "added", path, None)?);
                if entries.len() > DIFF_MAX_ENTRIES {
                    return Err(ProjectFailure::manifest(
                        "diff summary exceeds its bounded 10,000-entry capacity",
                    ));
                }
            }
        }
        let mut renamed = Vec::new();
        let deleted = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.kind == "deleted")
            .map(|(index, entry)| (index, entry.path.clone()))
            .collect::<Vec<_>>();
        let added = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.kind == "added")
            .map(|(index, entry)| (index, entry.path.clone()))
            .collect::<Vec<_>>();
        let mut added_by_version = HashMap::<String, Vec<(usize, String)>>::new();
        for (added_index, new_path) in &added {
            let current = self.current_content(&worktree, new_path)?;
            if !current.binary {
                let Some(version) = current.version else {
                    continue;
                };
                added_by_version
                    .entry(version)
                    .or_default()
                    .push((*added_index, new_path.clone()));
            }
        }
        let mut consumed = BTreeSet::new();
        for (deleted_index, old_path) in &deleted {
            let base = self.base_content(active, old_path)?;
            if base.binary {
                continue;
            }
            let Some(version) = base.version else {
                continue;
            };
            let Some(candidates) = added_by_version.get_mut(&version) else {
                continue;
            };
            if let Some((added_index, new_path)) = candidates.pop() {
                consumed.insert(*deleted_index);
                consumed.insert(added_index);
                renamed.push(DiffEntry {
                    kind: "renamed",
                    path: new_path,
                    old_path: Some(old_path.clone()),
                    base_exists: true,
                    current_exists: true,
                });
            }
        }
        if !consumed.is_empty() {
            entries = entries
                .into_iter()
                .enumerate()
                .filter_map(|(index, entry)| (!consumed.contains(&index)).then_some(entry))
                .collect();
            entries.extend(renamed);
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }

    fn classify_diff(
        &self,
        active: &ActiveProject,
        worktree: &Dir,
        kind: &'static str,
        path: String,
        old_path: Option<String>,
    ) -> Result<DiffEntry, ProjectFailure> {
        validate_relative_path(&path, false)?;
        if let Some(old) = &old_path {
            validate_relative_path(old, false)?;
        }
        let base_path = old_path.as_deref().unwrap_or(&path);
        let base = if kind == "added" {
            None
        } else {
            Some(self.base_content(active, base_path)?)
        };
        let current = if kind == "deleted" {
            None
        } else {
            Some(self.current_content(worktree, &path)?)
        };
        let binary = base.as_ref().is_some_and(|value| value.binary)
            || current.as_ref().is_some_and(|value| value.binary);
        Ok(DiffEntry {
            kind: if binary { "binary" } else { kind },
            path,
            old_path: if binary { None } else { old_path },
            base_exists: kind != "added",
            current_exists: kind != "deleted",
        })
    }

    fn current_content(&self, worktree: &Dir, path: &str) -> Result<ContentInfo, ProjectFailure> {
        let (parent, name) = open_parent_capability(worktree, path)?;
        let mut file = open_real_file_at(&parent, &name)?;
        bounded_content(&mut file, DIFF_MAX_BYTES as u64)
    }

    fn base_content(
        &self,
        active: &ActiveProject,
        path: &str,
    ) -> Result<ContentInfo, ProjectFailure> {
        validate_relative_path(path, false)?;
        let worktree = self.worktree_capability(active)?;
        let object = format!("{}:{path}", active.base_commit);
        let size_output =
            self.git_limited_bytes_at(&worktree, ["cat-file", "-s", object.as_str()], &[0], 64)?;
        let size = std::str::from_utf8(&size_output)
            .map_err(|_| ProjectFailure::manifest("Git object size is not valid UTF-8"))?
            .trim()
            .parse::<u64>()
            .map_err(|_| ProjectFailure::manifest("Git object size is not a valid integer"))?;
        if size > DIFF_MAX_BYTES as u64 {
            return Ok(ContentInfo::too_large(size));
        }
        let bytes = self.git_limited_bytes_at(
            &worktree,
            ["cat-file", "blob", object.as_str()],
            &[0],
            DIFF_MAX_BYTES,
        )?;
        if bytes.len() as u64 != size {
            return Err(ProjectFailure::recovery(
                "Git object length changed while reading immutable base content",
            ));
        }
        Ok(ContentInfo::from_bytes(bytes))
    }

    fn git_at<I, S>(&self, directory: &Dir, args: I) -> Result<Output, ProjectFailure>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.git_allow_status_at(directory, args, &[0])
    }

    fn git_allow_status_at<I, S>(
        &self,
        directory: &Dir,
        args: I,
        allowed: &[i32],
    ) -> Result<Output, ProjectFailure>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.git_command_at(directory)?;
        command.args(args).stdout(Stdio::piped());
        let mut child = StreamingGitChild::spawn(&mut command).map_err(|error| {
            ProjectFailure::manifest(format!("could not start fixed Git command: {error}"))
        })?;
        let mut stdout =
            child.stdout.take().ok_or_else(|| {
                ProjectFailure::recovery("fixed Git command did not expose stdout")
            })?;
        let mut bytes = Vec::with_capacity(GIT_CONTROL_OUTPUT_MAX_BYTES + 1);
        (&mut stdout)
            .take((GIT_CONTROL_OUTPUT_MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                ProjectFailure::recovery(format!("could not read bounded Git output: {error}"))
            })?;
        if bytes.len() > GIT_CONTROL_OUTPUT_MAX_BYTES {
            return Err(ProjectFailure::manifest(
                "fixed Git control output exceeds 64 KiB",
            ));
        }
        let (status, stderr) = child.finish().map_err(|error| {
            ProjectFailure::recovery(format!("could not reap fixed Git command: {error}"))
        })?;
        let code = status.code().unwrap_or(-1);
        if !allowed.contains(&code) {
            return Err(ProjectFailure::manifest(format!(
                "Git command failed with exit {code}: {}",
                bounded_stderr(&stderr)
            )));
        }
        Ok(Output {
            status,
            stdout: bytes,
            stderr,
        })
    }

    fn git_limited_bytes_at<I, S>(
        &self,
        directory: &Dir,
        args: I,
        allowed: &[i32],
        limit: usize,
    ) -> Result<Vec<u8>, ProjectFailure>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.git_command_at(directory)?;
        command.args(args).stdout(Stdio::piped());
        let mut child = StreamingGitChild::spawn(&mut command).map_err(|error| {
            ProjectFailure::manifest(format!("could not start fixed Git command: {error}"))
        })?;
        let mut stdout =
            child.stdout.take().ok_or_else(|| {
                ProjectFailure::recovery("fixed Git command did not expose stdout")
            })?;
        let mut bytes = Vec::with_capacity(limit.saturating_add(1));
        if let Err(error) = (&mut stdout)
            .take(limit.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
        {
            return Err(ProjectFailure::recovery(format!(
                "could not read bounded Git output: {error}"
            )));
        }
        if bytes.len() > limit {
            return Err(ProjectFailure::manifest(
                "fixed Git output exceeds its bounded operation limit",
            ));
        }
        let (status, stderr) = child.finish().map_err(|error| {
            ProjectFailure::recovery(format!("could not reap fixed Git command: {error}"))
        })?;
        let code = status.code().unwrap_or(-1);
        if !allowed.contains(&code) {
            return Err(ProjectFailure::manifest(format!(
                "Git command failed with exit {code}: {}",
                bounded_stderr(&stderr)
            )));
        }
        Ok(bytes)
    }

    #[cfg(unix)]
    fn fixed_git_command(&self) -> Result<Command, ProjectFailure> {
        self.verify_managed_paths()?;
        let mut command = Command::new("/usr/bin/git");
        command
            .arg("--no-pager")
            .stdin(Stdio::null())
            .env_clear()
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_COUNT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "")
            .env("PAGER", "");
        Ok(command)
    }

    #[cfg(unix)]
    fn git_command_at(&self, directory: &Dir) -> Result<Command, ProjectFailure> {
        let mut command = self.fixed_git_command()?;
        let cwd_file = directory
            .try_clone()
            .map_err(|error| {
                ProjectFailure::recovery(format!("could not clone Git cwd capability: {error}"))
            })?
            .into_std_file();
        let cwd_fd = cwd_file.as_raw_fd();
        // SAFETY: the opened no-follow directory capability is moved into the child pre-exec
        // closure. fchdir is async-signal-safe and happens after fork but before exec.
        unsafe {
            command.pre_exec(move || {
                let _keep_cwd_open = &cwd_file;
                if libc::fchdir(cwd_fd) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        Ok(command)
    }

    #[cfg(not(unix))]
    fn git_command_at(&self, _directory: &Dir) -> Result<Command, ProjectFailure> {
        Err(ProjectFailure::recovery(
            "G4 snapshot Git commands are disabled until Windows handle-bound process creation is verified",
        ))
    }

    fn verify_managed_paths(&self) -> Result<(), ProjectFailure> {
        for (path, name) in [
            (&self.data_root, "managed data root"),
            (&self.worktrees_root, "managed worktree directory"),
            (&self.manifests_root, "managed manifest directory"),
            (&self.save_staging_root, "managed save staging directory"),
        ] {
            let metadata = fs::symlink_metadata(path).map_err(|error| {
                ProjectFailure::recovery(format!("could not inspect {name}: {error}"))
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(ProjectFailure::recovery(format!(
                    "{name} was replaced with a symlink or non-directory"
                )));
            }
            let canonical = fs::canonicalize(path).map_err(|error| {
                ProjectFailure::recovery(format!("could not canonicalize {name}: {error}"))
            })?;
            if canonical != *path {
                return Err(ProjectFailure::recovery(format!(
                    "{name} no longer matches its startup identity"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct DiffEntry {
    kind: &'static str,
    path: String,
    old_path: Option<String>,
    base_exists: bool,
    current_exists: bool,
}

struct ContentInfo {
    bytes: Option<Vec<u8>>,
    length: u64,
    version: Option<String>,
    binary: bool,
}

struct MaterializedTreeEntry {
    mode: String,
    oid: String,
    path: String,
}

fn validated_oid(value: &str, object_format: &str) -> Result<String, ProjectFailure> {
    let length = match object_format {
        "sha1" => 40,
        "sha256" => 64,
        _ => return Err(ProjectFailure::manifest("unsupported Git object format")),
    };
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProjectFailure::manifest(
            "Git object id does not match its declared object format",
        ));
    }
    Ok(value.to_string())
}

fn checked_bundle_size(current: u64, next: u64) -> Result<u64, ProjectFailure> {
    let total = current
        .checked_add(next)
        .ok_or_else(|| ProjectFailure::manifest("bundle size overflow"))?;
    if total > SNAPSHOT_BUNDLE_MAX_BYTES_V1 {
        return Err(ProjectFailure::manifest("snapshot bundle exceeds 2 GiB"));
    }
    Ok(total)
}

fn parse_tree_record(
    record: &[u8],
    object_format: &str,
) -> Result<MaterializedTreeEntry, ProjectFailure> {
    let separator = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| ProjectFailure::manifest("Git tree record is missing its path separator"))?;
    let (metadata, path_with_separator) = record.split_at(separator);
    let path = &path_with_separator[1..];
    let metadata = std::str::from_utf8(metadata)
        .map_err(|_| ProjectFailure::manifest("Git tree metadata is not UTF-8"))?;
    let mut fields = metadata.split(' ');
    let mode = fields.next().unwrap_or_default();
    let kind = fields.next().unwrap_or_default();
    let oid = fields.next().unwrap_or_default();
    if fields.next().is_some()
        || !matches!(
            (mode, kind),
            ("100644", "blob") | ("100755", "blob") | ("120000", "blob") | ("160000", "commit")
        )
    {
        return Err(ProjectFailure::manifest(
            "committed tree contains an abnormal mode or object type",
        ));
    }
    let path = std::str::from_utf8(path)
        .map_err(|_| ProjectFailure::manifest("committed tree path is not UTF-8"))?;
    Ok(MaterializedTreeEntry {
        mode: mode.to_string(),
        oid: validated_oid(oid, object_format)?,
        path: path.to_string(),
    })
}

fn request_blob_header(
    input: &mut GitStdin,
    output: &mut BufReader<GitStdout>,
    oid: &str,
    object_format: &str,
) -> Result<u64, ProjectFailure> {
    input.write_all(oid.as_bytes()).map_err(|error| {
        ProjectFailure::recovery(format!("could not request raw blob: {error}"))
    })?;
    input.write_all(b"\n").map_err(|error| {
        ProjectFailure::recovery(format!("could not terminate raw blob request: {error}"))
    })?;
    input.flush().map_err(|error| {
        ProjectFailure::recovery(format!("could not flush raw blob request: {error}"))
    })?;
    let mut header = String::new();
    output.read_line(&mut header).map_err(|error| {
        ProjectFailure::recovery(format!("could not read raw blob header: {error}"))
    })?;
    if header.len() > 256 || !header.ends_with('\n') {
        return Err(ProjectFailure::manifest(
            "raw blob header is truncated or oversized",
        ));
    }
    let fields = header.trim_end().split(' ').collect::<Vec<_>>();
    if fields.len() != 3
        || validated_oid(fields[0], object_format)?.as_str() != oid
        || fields[1] != "blob"
    {
        return Err(ProjectFailure::manifest(
            "raw blob header does not match the requested committed blob",
        ));
    }
    fields[2]
        .parse::<u64>()
        .map_err(|_| ProjectFailure::manifest("raw blob size is invalid"))
}

fn consume_batch_newline(output: &mut BufReader<GitStdout>) -> Result<(), ProjectFailure> {
    let mut newline = [0_u8; 1];
    output.read_exact(&mut newline).map_err(|error| {
        ProjectFailure::recovery(format!("raw blob stream is truncated: {error}"))
    })?;
    if newline != *b"\n" {
        return Err(ProjectFailure::manifest(
            "raw blob stream is missing its record terminator",
        ));
    }
    Ok(())
}

fn create_parent_directories(root: &Dir, path: &str) -> Result<(), ProjectFailure> {
    let mut current = root.try_clone().map_err(|error| {
        ProjectFailure::recovery(format!("could not clone workspace capability: {error}"))
    })?;
    let mut components = path.split('/').collect::<Vec<_>>();
    components.pop();
    for component in components {
        match current.create_dir(component) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(ProjectFailure::recovery(format!(
                    "could not create materialized parent: {error}"
                )))
            }
        }
        current = open_real_child_dir(&current, component)?;
    }
    Ok(())
}

fn create_directory_path(root: &Dir, path: &str) -> Result<(), ProjectFailure> {
    create_parent_directories(root, &format!("{path}/placeholder"))?;
    match root.create_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(ProjectFailure::recovery(format!(
            "could not represent uninitialized submodule: {error}"
        ))),
    }
}

fn materialize_regular_blob(
    root: &Dir,
    path: &str,
    executable: bool,
    size: u64,
    input: &mut BufReader<GitStdout>,
) -> Result<(), ProjectFailure> {
    let mut options = CapOpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = root.open_with(path, &options).map_err(|error| {
        ProjectFailure::recovery(format!("could not create materialized file: {error}"))
    })?;
    let mut remaining = size;
    let mut buffer = vec![0_u8; MAX_TEXT_CHUNK_BYTES];
    while remaining > 0 {
        let requested = remaining.min(buffer.len() as u64) as usize;
        input
            .read_exact(&mut buffer[..requested])
            .map_err(|error| {
                ProjectFailure::recovery(format!("raw committed blob is truncated: {error}"))
            })?;
        file.write_all(&buffer[..requested]).map_err(|error| {
            ProjectFailure::recovery(format!("could not materialize raw blob bytes: {error}"))
        })?;
        remaining -= requested as u64;
    }
    consume_batch_newline(input)?;
    #[cfg(unix)]
    file.set_permissions(cap_std::fs::Permissions::from_mode(if executable {
        0o755
    } else {
        0o644
    }))
    .map_err(|error| {
        ProjectFailure::recovery(format!("could not preserve committed file mode: {error}"))
    })?;
    file.flush().map_err(|error| {
        ProjectFailure::recovery(format!("could not flush materialized file: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        ProjectFailure::recovery(format!("could not sync materialized file: {error}"))
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(unix))]
fn spawn_bounded_stderr_reader(
    mut stderr: std::process::ChildStderr,
) -> JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut tail = Vec::with_capacity(GIT_STDERR_TAIL_BYTES);
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stderr.read(&mut buffer)?;
            if read == 0 {
                return Ok(tail);
            }
            if read >= GIT_STDERR_TAIL_BYTES {
                tail.clear();
                tail.extend_from_slice(&buffer[read - GIT_STDERR_TAIL_BYTES..read]);
                continue;
            }
            let overflow = tail
                .len()
                .saturating_add(read)
                .saturating_sub(GIT_STDERR_TAIL_BYTES);
            if overflow > 0 {
                tail.drain(..overflow);
            }
            tail.extend_from_slice(&buffer[..read]);
        }
    })
}

fn read_bounded_json(dir: &Dir, name: &str) -> Result<Value, ProjectFailure> {
    let mut file = open_real_file_at(dir, name)?;
    let mut bytes = Vec::with_capacity(4096);
    (&mut file)
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            ProjectFailure::manifest(format!("could not read bounded JSON evidence: {error}"))
        })?;
    if bytes.len() > 64 * 1024 {
        return Err(ProjectFailure::manifest(
            "JSON evidence exceeds its bounded 64 KiB representation",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| ProjectFailure::manifest(format!("JSON evidence is invalid: {error}")))
}

fn source_digest(git_dir: &Dir) -> Result<SourceDigestSet, ProjectFailure> {
    let mut entry_hasher = Sha256::new();
    let mut lock_count = 0_u64;
    digest_entry_set(git_dir, "", &mut entry_hasher, &mut lock_count)?;
    Ok(SourceDigestSet {
        head: digest_optional_file(git_dir, "HEAD")?,
        index: digest_optional_file(git_dir, "index")?,
        refs: digest_optional_tree(git_dir, "refs")?,
        config: digest_optional_file(git_dir, "config")?,
        objects: digest_optional_tree(git_dir, "objects")?,
        packed_refs: digest_optional_file(git_dir, "packed-refs")?,
        entry_set: format!("{:x}", entry_hasher.finalize()),
        lock_count,
    })
}

fn digest_optional_file(root: &Dir, path: &str) -> Result<String, ProjectFailure> {
    match root.symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            let mut file = open_file_relative(root, path)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0_u8; MAX_TEXT_CHUNK_BYTES];
            loop {
                let read = file.read(&mut buffer).map_err(|error| {
                    ProjectFailure::recovery(format!("could not digest source file: {error}"))
                })?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(format!("{:x}", hasher.finalize()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(sha256_label("missing-file"))
        }
        Ok(_) => Err(ProjectFailure::recovery(
            "source Git identity file is a symlink or non-file",
        )),
        Err(error) => Err(ProjectFailure::recovery(format!(
            "could not inspect source Git identity file: {error}"
        ))),
    }
}

fn digest_optional_tree(root: &Dir, path: &str) -> Result<String, ProjectFailure> {
    match root.symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            let dir = open_relative_dir(root, path)?;
            let mut hasher = Sha256::new();
            digest_tree_contents(&dir, "", &mut hasher)?;
            Ok(format!("{:x}", hasher.finalize()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(sha256_label("missing-tree"))
        }
        Ok(_) => Err(ProjectFailure::recovery(
            "source Git identity tree is a symlink or non-directory",
        )),
        Err(error) => Err(ProjectFailure::recovery(format!(
            "could not inspect source Git identity tree: {error}"
        ))),
    }
}

fn digest_tree_contents(
    dir: &Dir,
    prefix: &str,
    hasher: &mut Sha256,
) -> Result<(), ProjectFailure> {
    let mut entries = collect_cap_entries(dir)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, kind) in entries {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        hasher.update(path.as_bytes());
        hasher.update([0, kind]);
        match kind {
            1 => {
                let child = open_real_child_dir(dir, &name)?;
                digest_tree_contents(&child, &path, hasher)?;
            }
            2 => {
                let mut file = open_real_file_at(dir, &name)?;
                let mut buffer = vec![0_u8; MAX_TEXT_CHUNK_BYTES];
                loop {
                    let read = file.read(&mut buffer).map_err(|error| {
                        ProjectFailure::recovery(format!(
                            "could not digest source tree file: {error}"
                        ))
                    })?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..read]);
                }
            }
            _ => {
                return Err(ProjectFailure::recovery(
                    "source Git identity tree contains a symlink or special entry",
                ))
            }
        }
    }
    Ok(())
}

fn digest_entry_set(
    dir: &Dir,
    prefix: &str,
    hasher: &mut Sha256,
    lock_count: &mut u64,
) -> Result<(), ProjectFailure> {
    let mut entries = collect_cap_entries(dir)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, kind) in entries {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if name.ends_with(".lock") {
            *lock_count = lock_count
                .checked_add(1)
                .ok_or_else(|| ProjectFailure::recovery("source lock count overflow"))?;
        }
        let metadata = dir.symlink_metadata(&name).map_err(|error| {
            ProjectFailure::recovery(format!("could not inspect source entry: {error}"))
        })?;
        hasher.update(path.as_bytes());
        hasher.update([0, kind]);
        hasher.update(metadata.len().to_le_bytes());
        if kind == 1 {
            let child = open_real_child_dir(dir, &name)?;
            digest_entry_set(&child, &path, hasher, lock_count)?;
        } else if kind != 2 {
            return Err(ProjectFailure::recovery(
                "source Git directory contains a symlink or special entry",
            ));
        }
    }
    Ok(())
}

fn collect_cap_entries(dir: &Dir) -> Result<Vec<(String, u8)>, ProjectFailure> {
    let mut result = Vec::new();
    for entry in dir.entries().map_err(|error| {
        ProjectFailure::recovery(format!("could not enumerate capability directory: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            ProjectFailure::recovery(format!("could not inspect capability entry: {error}"))
        })?;
        let name = entry.file_name().into_string().map_err(|_| {
            ProjectFailure::recovery("source Git directory contains a non-UTF-8 entry")
        })?;
        let file_type = entry.file_type().map_err(|error| {
            ProjectFailure::recovery(format!("could not classify capability entry: {error}"))
        })?;
        let kind = if file_type.is_dir() {
            1
        } else if file_type.is_file() {
            2
        } else {
            3
        };
        result.push((name, kind));
    }
    Ok(result)
}

fn open_file_relative(root: &Dir, path: &str) -> Result<CapFile, ProjectFailure> {
    let (parent, name) = open_parent_capability(root, path)?;
    open_real_file_at(&parent, &name)
}

fn sha256_label(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn count_git_refs(git_dir: &Dir) -> Result<u64, ProjectFailure> {
    let Ok(refs) = open_relative_dir(git_dir, "refs") else {
        return Ok(0);
    };
    count_files(&refs)
}

fn count_files(dir: &Dir) -> Result<u64, ProjectFailure> {
    let mut count = 0_u64;
    for (name, kind) in collect_cap_entries(dir)? {
        match kind {
            1 => {
                count = count
                    .checked_add(count_files(&open_real_child_dir(dir, &name)?)?)
                    .ok_or_else(|| ProjectFailure::manifest("Git ref count overflow"))?;
            }
            2 => {
                count = count
                    .checked_add(1)
                    .ok_or_else(|| ProjectFailure::manifest("Git ref count overflow"))?;
            }
            _ => return Err(ProjectFailure::manifest("Git refs contain a special entry")),
        }
    }
    Ok(count)
}

fn git_metadata_contains(git_dir: &Dir, needle: &[u8]) -> Result<bool, ProjectFailure> {
    for path in ["config", "logs/HEAD", "packed-refs"] {
        let Ok(mut file) = open_file_relative(git_dir, path) else {
            continue;
        };
        let mut bytes = Vec::new();
        (&mut file)
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                ProjectFailure::manifest(format!("could not inspect Git metadata: {error}"))
            })?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(ProjectFailure::manifest(
                "Git metadata exceeds bounded source-link inspection",
            ));
        }
        if !needle.is_empty() && bytes.windows(needle.len()).any(|window| window == needle) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(unix)]
fn available_space(dir: &Dir) -> Result<u64, ProjectFailure> {
    let file = dir
        .try_clone()
        .map_err(|error| {
            ProjectFailure::recovery(format!("could not clone disk capability: {error}"))
        })?
        .into_std_file();
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: stats points to writable storage and file owns a valid directory fd.
    let result = unsafe { libc::fstatvfs(file.as_raw_fd(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(ProjectFailure::recovery(format!(
            "could not measure snapshot disk reserve: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: successful fstatvfs initialized stats.
    let stats = unsafe { stats.assume_init() };
    u64::from(stats.f_bavail)
        .checked_mul(stats.f_frsize)
        .ok_or_else(|| ProjectFailure::recovery("available disk byte count overflow"))
}

#[cfg(not(unix))]
fn available_space(_dir: &Dir) -> Result<u64, ProjectFailure> {
    Err(ProjectFailure::recovery(
        "snapshot disk reserve is unsupported before the Windows safety gate",
    ))
}

impl ContentInfo {
    fn too_large(length: u64) -> Self {
        Self {
            bytes: None,
            length,
            version: None,
            binary: true,
        }
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        let binary = bytes.contains(&0) || std::str::from_utf8(&bytes).is_err();
        let length = bytes.len() as u64;
        let version = Some(sha256_file_version_v1(&bytes));
        Self {
            bytes: Some(bytes),
            length,
            version,
            binary,
        }
    }
}

impl DiffEntry {
    fn summary_value(&self) -> Value {
        if self.kind == "renamed" {
            json!({
                "kind":self.kind,"path":self.path,"oldPath":self.old_path,"binary":false,
            })
        } else {
            json!({
                "kind":self.kind,"path":self.path,"binary":self.kind == "binary",
            })
        }
    }
}

fn create_managed_directory(
    root: &Path,
    root_dir: &Dir,
    name: &str,
) -> Result<(PathBuf, Dir), SidecarError> {
    let path = root.join(name);
    match root_dir.create_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(SidecarError::new(
                "dataDirectoryUnavailable",
                format!("could not create managed {name}: {error}"),
            ))
        }
    }
    let directory = open_real_child_dir(root_dir, name).map_err(|error| {
        SidecarError::new(
            "dataDirectoryUnavailable",
            format!("could not open managed {name} capability: {}", error.detail),
        )
    })?;
    let canonical = fs::canonicalize(&path).map_err(|error| {
        SidecarError::new(
            "dataDirectoryUnavailable",
            format!("could not canonicalize managed {name}: {error}"),
        )
    })?;
    if canonical != path {
        return Err(SidecarError::new(
            "dataDirectoryUnavailable",
            format!("managed {name} path differs from its capability identity"),
        ));
    }
    Ok((canonical, directory))
}

fn canonical_existing_directory(path: &Path) -> Result<PathBuf, ProjectFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ProjectFailure::manifest(format!("selected directory is unavailable: {error}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ProjectFailure::symlink(
            "project directory must be a real directory, not a symlink",
        ));
    }
    fs::canonicalize(path).map_err(|error| {
        ProjectFailure::manifest(format!("could not canonicalize directory: {error}"))
    })
}

fn canonical_protocol_path(path: &Path) -> Result<String, ProjectFailure> {
    canonical_protocol_path_from_absolute(&fs::canonicalize(path).map_err(|error| {
        ProjectFailure::manifest(format!("could not canonicalize path: {error}"))
    })?)
}

fn canonical_protocol_path_without_existing(path: &Path) -> Result<String, ProjectFailure> {
    let parent = path
        .parent()
        .ok_or_else(|| ProjectFailure::manifest("managed path has no parent"))?;
    let parent = fs::canonicalize(parent).map_err(|error| {
        ProjectFailure::manifest(format!("could not canonicalize managed parent: {error}"))
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| ProjectFailure::manifest("managed path has no file name"))?;
    canonical_protocol_path_from_absolute(&parent.join(name))
}

fn canonical_protocol_path_from_absolute(path: &Path) -> Result<String, ProjectFailure> {
    let value = path
        .to_str()
        .ok_or_else(|| ProjectFailure::manifest("path is not valid Unicode for the protocol"))?
        .replace('\\', "/");
    #[cfg(windows)]
    let value = {
        let mut value = value;
        if value.as_bytes().get(1) == Some(&b':') {
            value.replace_range(0..1, &value[0..1].to_ascii_uppercase());
        }
        value
    };
    if !value.starts_with('/')
        && !(value.len() >= 3
            && value.as_bytes()[0].is_ascii_uppercase()
            && value.as_bytes()[1] == b':'
            && value.as_bytes()[2] == b'/')
    {
        return Err(ProjectFailure::manifest("path is not absolute"));
    }
    Ok(value)
}

fn project_id_for_root(root: &str) -> String {
    sha256_file_version_v1(root.as_bytes()).replacen("sha256:", "project_", 1)
}

fn project_identity(project_id: &str, selected: &str, root: &str) -> Value {
    json!({
        "schemaVersion":PROJECT_SCHEMA_VERSION_V1,
        "projectId":project_id,
        "selectedPath":selected,
        "canonicalGitRoot":root,
    })
}

fn manifest_value(active: &ActiveProject, lifecycle: &str) -> Result<Value, ProjectFailure> {
    Ok(json!({
        "schemaVersion":PROJECT_SCHEMA_VERSION_V1,
        "projectId":active.project_id,
        "canonicalGitRoot":canonical_protocol_path_without_existing(&active.canonical_git_root)?,
        "worktreeId":active.worktree_id,
        "worktreePath":canonical_protocol_path_without_existing(&active.worktree_path)?,
        "workspaceKind":WORKSPACE_KIND_V1,
        "snapshotTransport":SNAPSHOT_TRANSPORT_V1,
        "sourceMutationPolicy":SOURCE_MUTATION_POLICY_V1,
        "objectFormat":active.object_format,
        "baseCommit":active.base_commit,
        "baseTree":active.base_tree,
        "sourceIdentity":active.source_identity,
        "workspaceIdentity":active.workspace_identity,
        "headMode":WORKTREE_HEAD_MODE_V1,
        "lifecycle":lifecycle,
    }))
}

fn manifest_identity_digest(manifest: &Value) -> Result<String, ProjectFailure> {
    let bytes = serde_json::to_vec(manifest).map_err(|error| {
        ProjectFailure::manifest(format!(
            "could not encode recovery manifest identity: {error}"
        ))
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn snapshot_identity(active: &ActiveProject) -> Result<Value, ProjectFailure> {
    Ok(json!({
        "projectId":active.project_id,
        "worktreeId":active.worktree_id,
        "workspaceIdentity":active.workspace_identity,
        "canonicalGitRoot":canonical_protocol_path_without_existing(&active.canonical_git_root)?,
        "sourceIdentity":active.source_identity,
        "objectFormat":active.object_format,
        "baseCommit":active.base_commit,
        "baseTree":active.base_tree,
    }))
}

fn activation_evidence(
    active: &ActiveProject,
    before: &SourceDigestSet,
    after: &SourceDigestSet,
    invariant: Value,
) -> Result<Value, ProjectFailure> {
    let identity = snapshot_identity(active)?;
    let completed = SNAPSHOT_CREATION_PHASES_V1[..SNAPSHOT_CREATION_PHASES_V1.len() - 1].to_vec();
    Ok(json!({
        "manifest":manifest_value(active, "active")?,
        "source":{
            "identity":identity,
            "trace":{
                "sourceCapability":"readOnly",
                "sourceLockCount":before.lock_count,
                "sourceWriteCount":0,
                "before":before.contract_value(),
                "after":after.contract_value(),
            }
        },
        "workspace":{
            "identity":snapshot_identity(active)?,
            "invariant":invariant,
        },
        "activation":{
            "identity":snapshot_identity(active)?,
            "completedPhases":completed,
            "activeWorkspaceCount":0,
            "proposedLifecycle":"active",
            "database":snapshot_identity(active)?,
            "externalManifest":snapshot_identity(active)?,
            "gitMarker":snapshot_identity(active)?,
        }
    }))
}

fn append_object(
    storage: &mut Storage,
    aggregate_type: &str,
    aggregate_id: &str,
    operation_id: &str,
    request_id: &str,
    value: Value,
) -> Result<(), ProjectFailure> {
    let object = value
        .as_object()
        .cloned()
        .ok_or_else(|| ProjectFailure::manifest("projection payload must be an object"))?;
    match storage.upsert_projection_record(
        aggregate_type,
        aggregate_id,
        operation_id,
        Some(request_id),
        object,
    )? {
        AppendOutcome::Committed | AppendOutcome::DuplicateNoOp => Ok(()),
        AppendOutcome::Conflict => Err(ProjectFailure::recovery(
            "durable operation identity conflicted and cannot be retried automatically",
        )),
    }
}

fn preserve_project_extensions(
    value: Value,
    previous: &Map<String, Value>,
) -> Result<Value, ProjectFailure> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| ProjectFailure::manifest("project payload must be an object"))?;
    for field in ["role_settings", "prepared_workspaces", "source_baselines", "application_review", "application_chat_reviews", "application_backups", "application_intent", "application_history"] {
        if let Some(extension) = previous.get(field).cloned() {
            object.insert(field.to_string(), extension);
        }
    }
    Ok(Value::Object(object))
}

fn required_project(
    storage: &Storage,
    project_id: &str,
) -> Result<Map<String, Value>, ProjectFailure> {
    storage
        .projection_record("project", project_id)?
        .ok_or_else(|| ProjectFailure::manifest("project identity is not durably registered"))
}

fn value_string<'a>(record: &'a Map<String, Value>, name: &str) -> Result<&'a str, ProjectFailure> {
    record
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProjectFailure::manifest(format!("persisted {name} is missing")))
}

fn trim_output(output: &Output) -> Result<&str, ProjectFailure> {
    let value = std::str::from_utf8(&output.stdout)
        .map_err(|_| ProjectFailure::manifest("Git output is not valid UTF-8"))?
        .trim();
    if value.is_empty() {
        return Err(ProjectFailure::manifest("Git output is empty"));
    }
    Ok(value)
}

fn validate_relative_path(value: &str, allow_empty: bool) -> Result<(), ProjectFailure> {
    if (!allow_empty && value.is_empty())
        || value.starts_with('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(ProjectFailure::symlink(
            "path must be a normalized relative path",
        ));
    }
    if !(allow_empty && value.is_empty())
        && value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(ProjectFailure::symlink(
            "path traversal and empty path segments are not allowed",
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn root_relative_cstring(path: &Path) -> Result<CString, ProjectFailure> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ProjectFailure::recovery(
            "stable-root rename path is not a normalized relative path",
        ));
    }
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        ProjectFailure::recovery("stable-root rename path contains an interior NUL byte")
    })
}

#[cfg(unix)]
fn open_absolute_directory_capability(path: &Path) -> Result<Dir, ProjectFailure> {
    if !path.is_absolute() {
        return Err(ProjectFailure::symlink(
            "source repository capability requires an absolute path",
        ));
    }
    let mut current = Dir::open_ambient_dir("/", ambient_authority()).map_err(|error| {
        ProjectFailure::recovery(format!(
            "could not open filesystem root capability: {error}"
        ))
    })?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                current = open_real_os_child_dir(&current, name)?;
            }
            _ => {
                return Err(ProjectFailure::symlink(
                    "source repository path contains a non-normal component",
                ));
            }
        }
    }
    Ok(current)
}

fn open_real_os_child_dir(parent: &Dir, name: &OsStr) -> Result<Dir, ProjectFailure> {
    let component = Path::new(name);
    if component.components().count() != 1 {
        return Err(ProjectFailure::symlink(
            "directory capability traversal accepts one component at a time",
        ));
    }
    let mut options = CapOpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .maybe_dir(true);
    let opened = parent.open_with(component, &options).map_err(|error| {
        ProjectFailure::symlink(format!(
            "directory component is unavailable without following links: {error}"
        ))
    })?;
    let metadata = opened.metadata().map_err(|error| {
        ProjectFailure::manifest(format!(
            "could not inspect opened directory handle: {error}"
        ))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ProjectFailure::symlink(
            "directory component is a symlink, reparse point, or non-directory",
        ));
    }
    Dir::reopen_dir(&opened).map_err(|error| {
        ProjectFailure::symlink(format!("could not retain directory capability: {error}"))
    })
}

fn open_real_child_dir(parent: &Dir, name: &str) -> Result<Dir, ProjectFailure> {
    validate_relative_path(name, false)?;
    if name.contains('/') {
        return Err(ProjectFailure::symlink(
            "directory capability traversal accepts one component at a time",
        ));
    }
    open_real_os_child_dir(parent, OsStr::new(name))
}

fn open_relative_dir(root: &Dir, relative: &str) -> Result<Dir, ProjectFailure> {
    validate_relative_path(relative, true)?;
    let mut current = root.try_clone().map_err(|error| {
        ProjectFailure::recovery(format!("could not clone worktree root capability: {error}"))
    })?;
    for component in relative
        .split('/')
        .filter(|component| !component.is_empty())
    {
        current = open_real_child_dir(&current, component)?;
    }
    Ok(current)
}

fn open_parent_capability(root: &Dir, relative: &str) -> Result<(Dir, String), ProjectFailure> {
    validate_relative_path(relative, false)?;
    let (parent, name) = relative
        .rsplit_once('/')
        .map_or(("", relative), |(parent, name)| (parent, name));
    Ok((open_relative_dir(root, parent)?, name.to_string()))
}

fn open_real_file_at(parent: &Dir, name: &str) -> Result<CapFile, ProjectFailure> {
    if name.is_empty() || name.contains('/') {
        return Err(ProjectFailure::symlink(
            "file capability requires one normalized component",
        ));
    }
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let opened = parent.open_with(name, &options).map_err(|error| {
        ProjectFailure::symlink(format!(
            "file is unavailable without following links or reparse points: {error}"
        ))
    })?;
    let metadata = opened.metadata().map_err(|error| {
        ProjectFailure::manifest(format!("could not inspect opened file handle: {error}"))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ProjectFailure::symlink(
            "file is a symlink, reparse point, or non-regular object",
        ));
    }
    Ok(opened)
}

#[cfg(target_os = "macos")]
fn verify_real_parent_and_file(
    root: &Dir,
    relative: &Path,
    name: &str,
) -> Result<(Dir, CapFile), ProjectFailure> {
    let relative = relative.to_str().ok_or_else(|| {
        ProjectFailure::recovery(format!("{name} path is not protocol-safe UTF-8"))
    })?;
    let (parent, child) = open_parent_capability(root, relative).map_err(|error| {
        ProjectFailure::recovery(format!(
            "{name} hierarchy changed before atomic replace: {}",
            error.detail
        ))
    })?;
    let file = open_real_file_at(&parent, &child).map_err(|error| {
        ProjectFailure::recovery(format!(
            "{name} changed before atomic replace: {}",
            error.detail
        ))
    })?;
    Ok((parent, file))
}

#[cfg(unix)]
fn capability_directory_identity(directory: &Dir) -> Result<(u64, u64), ProjectFailure> {
    let metadata = directory.metadata(".").map_err(|error| {
        ProjectFailure::recovery(format!(
            "could not inspect held directory identity: {error}"
        ))
    })?;
    Ok((metadata.dev(), metadata.ino()))
}

fn read_file_bounded_handle(
    file: &mut CapFile,
    retain_limit: u64,
) -> Result<(Option<Vec<u8>>, u64, String), ProjectFailure> {
    let length = file
        .metadata()
        .map_err(|error| ProjectFailure::manifest(format!("could not inspect file: {error}")))?
        .len();
    let allocation = usize::try_from(length.min(retain_limit))
        .map_err(|_| ProjectFailure::manifest("file length exceeds addressable memory"))?;
    let mut retained = (length <= retain_limit).then(|| Vec::with_capacity(allocation));
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; MAX_TEXT_CHUNK_BYTES];
    let mut actual_length = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| ProjectFailure::manifest(format!("could not read file: {error}")))?;
        if read == 0 {
            break;
        }
        actual_length = actual_length
            .checked_add(read as u64)
            .ok_or_else(|| ProjectFailure::manifest("file length overflow while reading"))?;
        hasher.update(&buffer[..read]);
        if let Some(bytes) = &mut retained {
            if actual_length <= retain_limit {
                bytes.extend_from_slice(&buffer[..read]);
            } else {
                retained = None;
            }
        }
    }
    let version = format!("sha256:{:x}", hasher.finalize());
    Ok((retained, actual_length, version))
}

fn bounded_content(file: &mut CapFile, limit: u64) -> Result<ContentInfo, ProjectFailure> {
    let length = file
        .metadata()
        .map_err(|error| ProjectFailure::manifest(format!("could not inspect file: {error}")))?
        .len();
    if length > limit {
        return Ok(ContentInfo::too_large(length));
    }
    let (bytes, actual_length, version) = read_file_bounded_handle(file, limit)?;
    let Some(bytes) = bytes else {
        return Ok(ContentInfo::too_large(actual_length));
    };
    let mut content = ContentInfo::from_bytes(bytes);
    content.length = actual_length;
    content.version = Some(version);
    Ok(content)
}

fn validate_file_version(value: &str) -> Result<(), ProjectFailure> {
    let digest = value
        .strip_prefix("sha256:")
        .ok_or_else(|| ProjectFailure::manifest("expectedVersion must use the sha256 prefix"))?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProjectFailure::manifest(
            "expectedVersion must contain a lowercase 256-bit digest",
        ));
    }
    Ok(())
}

fn chunk_utf8(text: &str, limit: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = start.saturating_add(limit).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = text[start..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| start + offset)
                .unwrap_or(text.len());
        }
        chunks.push(text[start..end].to_string());
        start = end;
    }
    chunks
}

#[cfg(test)]
fn bounded_diff_text(bytes: Vec<u8>) -> Result<Vec<u8>, ProjectFailure> {
    if bytes.len() > DIFF_MAX_BYTES {
        return Err(ProjectFailure::manifest(
            "text diff exceeds the bounded 10 MiB review limit",
        ));
    }
    Ok(bytes)
}

fn build_bounded_text_diff(
    entry: &DiffEntry,
    base: Option<&[u8]>,
    current: Option<&[u8]>,
) -> Result<String, ProjectFailure> {
    let old_path = entry.old_path.as_deref().unwrap_or(&entry.path);
    let mut output = Vec::new();
    push_bounded(
        &mut output,
        format!("diff --git a/{old_path} b/{}\n", entry.path).as_bytes(),
    )?;
    if entry.kind == "renamed" {
        push_bounded(&mut output, format!("rename from {old_path}\n").as_bytes())?;
        push_bounded(
            &mut output,
            format!("rename to {}\n", entry.path).as_bytes(),
        )?;
    }
    let old_header = if base.is_some() {
        format!("a/{old_path}")
    } else {
        "/dev/null".to_string()
    };
    let new_header = if current.is_some() {
        format!("b/{}", entry.path)
    } else {
        "/dev/null".to_string()
    };
    push_bounded(&mut output, format!("--- {old_header}\n").as_bytes())?;
    push_bounded(&mut output, format!("+++ {new_header}\n").as_bytes())?;
    let old_count = line_count(base.unwrap_or_default());
    let new_count = line_count(current.unwrap_or_default());
    let old_start = usize::from(old_count > 0);
    let new_start = usize::from(new_count > 0);
    push_bounded(
        &mut output,
        format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@\n").as_bytes(),
    )?;
    push_prefixed_lines(&mut output, b'-', base.unwrap_or_default())?;
    push_prefixed_lines(&mut output, b'+', current.unwrap_or_default())?;
    String::from_utf8(output)
        .map_err(|_| ProjectFailure::manifest("bounded text diff contains invalid UTF-8"))
}

fn line_count(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    bytes.iter().filter(|byte| **byte == b'\n').count() + usize::from(!bytes.ends_with(b"\n"))
}

fn push_prefixed_lines(
    output: &mut Vec<u8>,
    prefix: u8,
    bytes: &[u8],
) -> Result<(), ProjectFailure> {
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        push_bounded(output, &[prefix])?;
        push_bounded(output, line)?;
    }
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        push_bounded(output, b"\n\\ No newline at end of file\n")?;
    }
    Ok(())
}

fn push_bounded(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ProjectFailure> {
    if output.len().saturating_add(bytes.len()) > DIFF_MAX_BYTES {
        return Err(ProjectFailure::manifest(
            "text diff exceeds the bounded 10 MiB review limit",
        ));
    }
    output.extend_from_slice(bytes);
    Ok(())
}

fn write_json_atomic(dir: &Dir, target: &str, value: &Value) -> Result<(), ProjectFailure> {
    validate_relative_path(target, false)?;
    if target.contains('/') {
        return Err(ProjectFailure::manifest(
            "manifest target must be one managed child component",
        ));
    }
    let temp = format!(".manifest-{}.tmp", Uuid::new_v4().simple());
    let bytes = serde_json::to_vec(value).map_err(|error| {
        ProjectFailure::manifest(format!("could not encode worktree manifest: {error}"))
    })?;
    let mut options = CapOpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = dir.open_with(&temp, &options).map_err(|error| {
        ProjectFailure::recovery(format!("could not create manifest temp file: {error}"))
    })?;
    file.write_all(&bytes).map_err(|error| {
        ProjectFailure::recovery(format!("could not write manifest temp file: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        ProjectFailure::recovery(format!("could not sync manifest temp file: {error}"))
    })?;
    drop(file);
    dir.rename(&temp, dir, target).map_err(|error| {
        ProjectFailure::recovery(format!(
            "could not atomically replace worktree manifest: {error}"
        ))
    })?;
    sync_capability_directory(dir)
}

fn sync_capability_directory(dir: &Dir) -> Result<(), ProjectFailure> {
    #[cfg(unix)]
    dir.try_clone()
        .map_err(|error| {
            ProjectFailure::recovery(format!("could not clone written directory: {error}"))
        })?
        .into_std_file()
        .sync_all()
        .map_err(|error| {
            ProjectFailure::recovery(format!("could not sync written directory: {error}"))
        })?;
    Ok(())
}

fn remove_capability_temp(parent: &Dir, name: impl AsRef<Path>) {
    let _ = parent.remove_file(name);
}

fn snapshot_tree(root: &Dir) -> Result<BTreeMap<String, WatchFingerprint>, ProjectFailure> {
    let mut pending = vec![(
        root.try_clone().map_err(|error| {
            ProjectFailure::recovery(format!("could not clone watcher root capability: {error}"))
        })?,
        String::new(),
    )];
    let mut snapshot = BTreeMap::new();
    while let Some((directory, prefix)) = pending.pop() {
        for entry in directory.entries().map_err(|error| {
            ProjectFailure::manifest(format!("could not scan watched directory: {error}"))
        })? {
            let entry = entry.map_err(|error| {
                ProjectFailure::manifest(format!("could not scan watched entry: {error}"))
            })?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ProjectFailure::manifest("watched path is not valid Unicode"))?;
            if is_internal_temp_name(&name) || (prefix.is_empty() && name == ".git") {
                continue;
            }
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            validate_relative_path(&relative, false)?;
            let entry_name = relative.rsplit('/').next().ok_or_else(|| {
                ProjectFailure::manifest("watched entry is missing its final component")
            })?;
            let metadata = directory.symlink_metadata(entry_name).map_err(|error| {
                ProjectFailure::manifest(format!("could not inspect watched entry: {error}"))
            })?;
            let kind = if metadata.file_type().is_symlink() {
                3
            } else if metadata.is_dir() {
                2
            } else if metadata.is_file() {
                1
            } else {
                4
            };
            let modified_nanos = metadata
                .modified()
                .ok()
                .map(|time| time.into_std())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            snapshot.insert(
                relative.clone(),
                WatchFingerprint {
                    kind,
                    length: metadata.len(),
                    modified_nanos,
                },
            );
            if snapshot.len() > WATCH_MAX_ENTRIES {
                return Err(ProjectFailure::recovery(
                    "watch snapshot exceeds its bounded 10,000-entry capacity",
                ));
            }
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                pending.push((open_real_child_dir(&directory, entry_name)?, relative));
            }
        }
    }
    Ok(snapshot)
}

fn watch_emission(
    state: &mut WatchState,
    project_id: &str,
    paths: Vec<String>,
    overflow: bool,
) -> WatchEmission {
    let seq = state.next_seq;
    state.next_seq += 1;
    let mut payload = json!({
        "projectId":project_id,
        "event":"file.changed",
        "paths":paths,
        "rescanRequired":overflow,
    });
    if let Some(workspace) = &state.workspace {
        let object = payload.as_object_mut().expect("watch payload object");
        object.insert(
            "chatId".to_string(),
            Value::String(workspace.chat_id.clone()),
        );
        object.insert(
            "worktreeId".to_string(),
            Value::String(workspace.worktree_id.clone()),
        );
        object.insert(
            "workspaceIdentity".to_string(),
            Value::String(workspace.workspace_identity.clone()),
        );
    }
    WatchEmission {
        stream_id: state.stream_id.clone(),
        seq,
        event: "file.changed",
        payload,
    }
}

fn tree_cursor_key(project_id: &str, parent_path: &str) -> String {
    let input = format!("{project_id}\0{parent_path}");
    sha256_file_version_v1(input.as_bytes())
        .trim_start_matches("sha256:")
        .chars()
        .take(16)
        .collect()
}

fn parse_tree_cursor(value: &str, expected_key: &str) -> Result<usize, ProjectFailure> {
    let (key, offset) = value
        .split_once(':')
        .ok_or_else(|| ProjectFailure::manifest("tree cursor is malformed"))?;
    if key != expected_key {
        return Err(ProjectFailure::manifest(
            "tree cursor belongs to another project or parent path",
        ));
    }
    offset
        .parse::<usize>()
        .map_err(|_| ProjectFailure::manifest("tree cursor offset is malformed"))
}

fn protocol_git_path(bytes: &[u8]) -> Result<String, ProjectFailure> {
    let path = std::str::from_utf8(bytes)
        .map_err(|_| ProjectFailure::manifest("Git path is not valid UTF-8"))?
        .replace('\\', "/");
    validate_relative_path(&path, false)?;
    Ok(path)
}

fn is_internal_temp_name(name: &str) -> bool {
    name.starts_with(".hello-agent-save-") || name.starts_with(".manifest-")
}

fn is_internal_relative_path(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(is_internal_temp_name)
}

fn bounded_stderr(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(8 * 1024);
    String::from_utf8_lossy(&bytes[start..])
        .chars()
        .map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
                '�'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(unix)]
fn null_device() -> &'static str {
    "/dev/null"
}

#[cfg(all(test, not(windows)))]
mod tests;

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn fake_path_git_is_never_started() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let fake_path = root.path().join("fake-path");
        let sentinel = root.path().join("git-started");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&fake_path).unwrap();
        fs::write(fake_path.join("git.exe"), b"must-not-run").unwrap();
        let manager = ProjectManager::new(&data.join("hello-agent.sqlite3")).unwrap();
        let old_path = std::env::var_os("PATH");
        std::env::set_var("PATH", &fake_path);
        let error = manager.git(root.path(), ["status"]).unwrap_err();
        if let Some(old_path) = old_path {
            std::env::set_var("PATH", old_path);
        } else {
            std::env::remove_var("PATH");
        }
        assert_eq!(error.kind, "recoveryRequired");
        assert!(!sentinel.exists());
        assert_eq!(
            fs::read(fake_path.join("git.exe")).unwrap(),
            b"must-not-run"
        );
    }
}
