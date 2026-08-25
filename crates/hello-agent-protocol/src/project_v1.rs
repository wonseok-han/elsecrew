use serde_json::{Map, Value};
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display};

pub const PROJECT_SCHEMA_VERSION_V1: u64 = 1;
pub const TREE_PAGE_MAX_ENTRIES_V1: usize = 500;
pub const FILE_MAX_BYTES_V1: u64 = 5 * 1024 * 1024;
pub const FILE_VERSION_ALGORITHM_V1: &str = "sha256";
pub const FILE_VERSION_PREFIX_V1: &str = "sha256:";
pub const FILE_VERSION_DIGEST_INPUT_V1: &str = "exactFileBytes";
pub const WORKTREE_HEAD_MODE_V1: &str = "detached";
pub const WORKTREE_LIFECYCLES_V1: &[&str] = &["active", "abandoned", "recoveryRequired"];
pub const G4_CAPABILITIES_V1: &[&str] = &[
    "system.status",
    "app.prepareQuit",
    "project.open",
    "project.getTree",
    "project.createWorktree",
    "project.close",
    "file.read",
    "file.save",
    "file.watch",
    "diff.getSummary",
    "diff.getFile",
];
pub const G4_ERROR_KINDS_V1: &[&str] = &[
    "manifestMismatch",
    "symlinkEscape",
    "dirtyConflict",
    "recoveryRequired",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContractError {
    detail: String,
}

impl ProjectContractError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for ProjectContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for ProjectContractError {}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPayloadV1 {
    pub payload_type: String,
    pub value: Value,
}

pub fn parse_project_payload_v1(
    payload_type: &str,
    value: &Value,
) -> Result<ProjectPayloadV1, ProjectContractError> {
    validate_project_payload_v1(payload_type, value)?;
    Ok(ProjectPayloadV1 {
        payload_type: payload_type.to_string(),
        value: value.clone(),
    })
}

pub fn validate_project_payload_v1(
    validator: &str,
    value: &Value,
) -> Result<(), ProjectContractError> {
    match validator {
        "projectIdentity" => validate_project_identity(value),
        "worktreeManifest" => validate_worktree_manifest(value),
        "treePage" => validate_tree_page(value),
        "fileClassification" => validate_file_classification(value),
        "fileRead" => validate_file_read(value),
        "saveTranscript" => validate_save_transcript(value),
        "diffSummary" => validate_diff_summary(value),
        "diffFile" => validate_diff_file(value),
        "watchEvent" => validate_watch_event(value),
        "projectClose" => validate_project_close(value),
        "safetyError" => validate_safety_error(value),
        _ => Err(ProjectContractError::new(format!(
            "unknown G4 payload validator: {validator}"
        ))),
    }
}

pub fn assert_g4_capabilities_v1() -> Result<(), ProjectContractError> {
    if G4_CAPABILITIES_V1
        .iter()
        .any(|capability| !super::METHODS_V1.contains(capability))
    {
        return Err(ProjectContractError::new(
            "G4 capability is not reserved by protocol v1",
        ));
    }
    if super::MAX_TEXT_CHUNK_BYTES != 256 * 1024 {
        return Err(ProjectContractError::new(
            "protocol text chunk contract drifted",
        ));
    }
    Ok(())
}

fn validate_project_identity(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "project identity")?;
    exact_keys(
        object,
        &[
            "schemaVersion",
            "projectId",
            "selectedPath",
            "canonicalGitRoot",
        ],
    )?;
    if unsigned(object.get("schemaVersion"), "project schemaVersion")? != PROJECT_SCHEMA_VERSION_V1
    {
        return fail("project schemaVersion must be 1");
    }
    id(object.get("projectId"), "projectId")?;
    let selected = absolute_path(object.get("selectedPath"), "selectedPath")?;
    let root = absolute_path(object.get("canonicalGitRoot"), "canonicalGitRoot")?;
    if !at_or_below(selected, root) {
        return fail("selectedPath must be the canonical Git root or one of its descendants");
    }
    Ok(())
}

fn validate_worktree_manifest(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "worktree manifest")?;
    exact_keys(
        object,
        &[
            "schemaVersion",
            "projectId",
            "canonicalGitRoot",
            "worktreeId",
            "worktreePath",
            "baseCommit",
            "headMode",
            "lifecycle",
        ],
    )?;
    if unsigned(object.get("schemaVersion"), "manifest schemaVersion")? != PROJECT_SCHEMA_VERSION_V1
    {
        return fail("manifest schemaVersion must be 1");
    }
    id(object.get("projectId"), "projectId")?;
    id(object.get("worktreeId"), "worktreeId")?;
    let root = absolute_path(object.get("canonicalGitRoot"), "canonicalGitRoot")?;
    let worktree = absolute_path(object.get("worktreePath"), "worktreePath")?;
    if root == worktree || at_or_below(worktree, root) {
        return fail("isolated worktreePath cannot be the original Git root or inside it");
    }
    let commit = string(object.get("baseCommit"), "baseCommit")?;
    if !matches!(commit.len(), 40 | 64)
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return fail("baseCommit must be a full lowercase Git object id");
    }
    if string(object.get("headMode"), "headMode")? != WORKTREE_HEAD_MODE_V1 {
        return fail("worktree must use detached HEAD");
    }
    if !WORKTREE_LIFECYCLES_V1.contains(&string(object.get("lifecycle"), "lifecycle")?) {
        return fail("unknown worktree lifecycle");
    }
    Ok(())
}

fn validate_tree_page(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "tree page")?;
    exact_keys(
        object,
        &["projectId", "parentPath", "entries", "nextCursor"],
    )?;
    id(object.get("projectId"), "projectId")?;
    relative_path(object.get("parentPath"), "parentPath", true)?;
    let entries = array(object.get("entries"), "tree entries")?;
    if entries.len() > TREE_PAGE_MAX_ENTRIES_V1 {
        return fail("tree page exceeds 500 entries");
    }
    let mut previous = "";
    for entry in entries {
        let entry = self::object(entry, "tree entry")?;
        exact_keys(entry, &["path", "name", "kind"])?;
        let path = relative_path(entry.get("path"), "tree entry path", false)?;
        let name = id(entry.get("name"), "tree entry name")?;
        if path.rsplit('/').next() != Some(name) {
            return fail("tree entry name must match its path");
        }
        if !["file", "directory", "symlink"].contains(&string(entry.get("kind"), "kind")?) {
            return fail("unknown tree entry kind");
        }
        if !previous.is_empty() && path <= previous {
            return fail("tree entries must be sorted and unique");
        }
        previous = path;
    }
    if !matches!(object.get("nextCursor"), Some(Value::Null)) {
        id(object.get("nextCursor"), "opaque nextCursor")?;
    }
    Ok(())
}

fn validate_file_classification(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "file classification")?;
    exact_keys(object, &["byteLength", "utf8Valid", "classification"])?;
    let bytes = unsigned(object.get("byteLength"), "file byteLength")?;
    if bytes > super::MAX_SEQUENCE {
        return fail("file byteLength must be a non-negative safe integer");
    }
    let utf8 = boolean(object.get("utf8Valid"), "utf8Valid")?;
    let expected = if bytes > FILE_MAX_BYTES_V1 {
        "tooLarge"
    } else if utf8 {
        "utf8"
    } else {
        "binary"
    };
    if string(object.get("classification"), "classification")? != expected {
        return fail(format!("file classification must be {expected}"));
    }
    Ok(())
}

fn validate_file_read(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "file read descriptor")?;
    exact_keys(
        object,
        &[
            "projectId",
            "path",
            "classification",
            "byteLength",
            "version",
            "stream",
        ],
    )?;
    id(object.get("projectId"), "projectId")?;
    relative_path(object.get("path"), "file path", false)?;
    let bytes = unsigned(object.get("byteLength"), "file byteLength")?;
    let classification = string(object.get("classification"), "classification")?;
    let expected = if bytes > FILE_MAX_BYTES_V1 {
        "tooLarge"
    } else if classification == "utf8" {
        "utf8"
    } else {
        "binary"
    };
    if classification != expected || !["utf8", "binary", "tooLarge"].contains(&classification) {
        return fail("file classification is inconsistent with its byte length");
    }
    file_version(object.get("version"))?;
    if classification == "utf8" {
        stream_descriptor(object.get("stream"), "text stream")?;
    } else if !matches!(object.get("stream"), Some(Value::Null)) {
        return fail("binary and too-large files cannot expose a text stream");
    }
    Ok(())
}

fn validate_save_transcript(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "save transcript")?;
    exact_keys(
        object,
        &["begin", "chunks", "commit", "currentVersion", "outcome"],
    )?;
    let begin = self::object(
        object.get("begin").ok_or_else(|| missing("begin"))?,
        "save begin",
    )?;
    exact_keys(
        begin,
        &[
            "saveId",
            "projectId",
            "path",
            "expectedVersion",
            "totalBytes",
            "encoding",
            "chunkBytes",
        ],
    )?;
    let save_id = id(begin.get("saveId"), "saveId")?;
    id(begin.get("projectId"), "projectId")?;
    relative_path(begin.get("path"), "save path", false)?;
    let expected_version = file_version(begin.get("expectedVersion"))?;
    let declared_bytes = unsigned(begin.get("totalBytes"), "save totalBytes")?;
    if declared_bytes > FILE_MAX_BYTES_V1 {
        return fail("save cannot exceed 5 MiB");
    }
    if string(begin.get("encoding"), "save encoding")? != "utf8"
        || unsigned(begin.get("chunkBytes"), "save chunkBytes")?
            != super::MAX_TEXT_CHUNK_BYTES as u64
    {
        return fail("save begin must use UTF-8 and 256 KiB chunks");
    }
    let chunks = array(object.get("chunks"), "save chunks")?;
    let mut received_bytes = 0_u64;
    for (index, chunk) in chunks.iter().enumerate() {
        let chunk = self::object(chunk, "save chunk")?;
        exact_keys(chunk, &["saveId", "seq", "text"])?;
        if string(chunk.get("saveId"), "save chunk saveId")? != save_id {
            return fail("save chunk saveId mismatch");
        }
        let seq = unsigned(chunk.get("seq"), "save chunk seq")?;
        if seq != (index + 1) as u64 || seq > super::MAX_SEQUENCE {
            return fail("save chunk seq must start at 1 and be contiguous");
        }
        let text = string(chunk.get("text"), "save chunk text")?;
        if text.len() > super::MAX_TEXT_CHUNK_BYTES {
            return fail("save chunk exceeds 256 KiB");
        }
        received_bytes += text.len() as u64;
    }
    let commit = self::object(
        object.get("commit").ok_or_else(|| missing("commit"))?,
        "save commit",
    )?;
    exact_keys(
        commit,
        &["saveId", "expectedVersion", "lastSeq", "totalBytes"],
    )?;
    if string(commit.get("saveId"), "commit saveId")? != save_id
        || file_version(commit.get("expectedVersion"))? != expected_version
    {
        return fail("save commit must match begin identity and expectedVersion");
    }
    if unsigned(commit.get("lastSeq"), "commit lastSeq")? != chunks.len() as u64
        || unsigned(commit.get("totalBytes"), "commit totalBytes")? != declared_bytes
        || received_bytes != declared_bytes
    {
        return fail("save commit byte or sequence totals do not match received chunks");
    }
    let current_version = file_version(object.get("currentVersion"))?;
    let expected_outcome = if current_version == expected_version {
        "committed"
    } else {
        "dirtyConflict"
    };
    if string(object.get("outcome"), "save outcome")? != expected_outcome {
        return fail(format!("save outcome must be {expected_outcome}"));
    }
    Ok(())
}

fn validate_diff_summary(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "diff summary")?;
    exact_keys(object, &["projectId", "baseCommit", "entries"])?;
    id(object.get("projectId"), "projectId")?;
    let commit = string(object.get("baseCommit"), "baseCommit")?;
    if !matches!(commit.len(), 40 | 64)
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return fail("diff baseCommit must be a full lowercase Git object id");
    }
    for entry in array(object.get("entries"), "diff entries")? {
        validate_diff_entry(entry)?;
    }
    Ok(())
}

fn validate_diff_file(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "diff file descriptor")?;
    exact_keys(
        object,
        &[
            "projectId",
            "kind",
            "path",
            "oldPath",
            "baseVersion",
            "worktreeVersion",
            "byteLength",
            "stream",
        ],
    )?;
    id(object.get("projectId"), "projectId")?;
    relative_path(object.get("path"), "diff path", false)?;
    let kind = diff_kind(object.get("kind"))?;
    if kind == "renamed" {
        let old = relative_path(object.get("oldPath"), "renamed oldPath", false)?;
        if Some(old) == object.get("path").and_then(Value::as_str) {
            return fail("rename oldPath and path must differ");
        }
    } else if !matches!(object.get("oldPath"), Some(Value::Null)) {
        return fail("only renamed diff may carry oldPath");
    }
    optional_file_version(object.get("baseVersion"))?;
    optional_file_version(object.get("worktreeVersion"))?;
    let byte_length = unsigned(object.get("byteLength"), "diff byteLength")?;
    if byte_length > super::MAX_SEQUENCE {
        return fail("diff byteLength must be a non-negative safe integer");
    }
    if kind == "added" && !matches!(object.get("baseVersion"), Some(Value::Null)) {
        return fail("added diff cannot have baseVersion");
    }
    if kind == "deleted" && !matches!(object.get("worktreeVersion"), Some(Value::Null)) {
        return fail("deleted diff cannot have worktreeVersion");
    }
    if kind == "binary" {
        if !matches!(object.get("stream"), Some(Value::Null)) {
            return fail("binary diff cannot expose a text stream");
        }
    } else {
        stream_descriptor(object.get("stream"), "diff text stream")?;
    }
    Ok(())
}

fn validate_diff_entry(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "diff entry")?;
    let kind = diff_kind(object.get("kind"))?;
    if kind == "renamed" {
        exact_keys(object, &["kind", "path", "oldPath", "binary"])?;
        let old = relative_path(object.get("oldPath"), "rename oldPath", false)?;
        if Some(old) == object.get("path").and_then(Value::as_str) {
            return fail("rename paths must differ");
        }
    } else {
        exact_keys(object, &["kind", "path", "binary"])?;
    }
    relative_path(object.get("path"), "diff entry path", false)?;
    if boolean(object.get("binary"), "diff binary")? != (kind == "binary") {
        return fail("diff binary flag must match binary kind");
    }
    Ok(())
}

fn validate_watch_event(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "watch event")?;
    exact_keys(object, &["projectId", "event", "paths", "rescanRequired"])?;
    id(object.get("projectId"), "projectId")?;
    if !["file.changed", "file.conflict", "diff.changed"]
        .contains(&string(object.get("event"), "watch event")?)
    {
        return fail("unknown watch event");
    }
    let paths = array(object.get("paths"), "watch paths")?;
    for path in paths {
        relative_path(Some(path), "watch path", false)?;
    }
    let rescan = boolean(object.get("rescanRequired"), "rescanRequired")?;
    if rescan && !paths.is_empty() {
        return fail("watch overflow cannot claim a complete path list");
    }
    if !rescan && paths.is_empty() {
        return fail("non-overflow watch event requires changed paths");
    }
    Ok(())
}

fn validate_project_close(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "project close result")?;
    exact_keys(object, &["projectId", "lifecycle", "physicallyDeleted"])?;
    id(object.get("projectId"), "projectId")?;
    if string(object.get("lifecycle"), "close lifecycle")? != "abandoned"
        || boolean(object.get("physicallyDeleted"), "physicallyDeleted")?
    {
        return fail("project.close must abandon without physically deleting the worktree");
    }
    Ok(())
}

fn validate_safety_error(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "G4 safety error")?;
    exact_keys(
        object,
        &["code", "kind", "detail", "retryable", "outcomeUnknown"],
    )?;
    id(object.get("code"), "error code")?;
    id(object.get("detail"), "error detail")?;
    if !G4_ERROR_KINDS_V1.contains(&string(object.get("kind"), "error kind")?) {
        return fail("unknown G4 safety error kind");
    }
    if boolean(object.get("retryable"), "retryable")?
        || boolean(object.get("outcomeUnknown"), "outcomeUnknown")?
    {
        return fail("G4 safety boundary failures cannot be retried automatically");
    }
    Ok(())
}

fn stream_descriptor(value: Option<&Value>, name: &str) -> Result<(), ProjectContractError> {
    let object = object(value.ok_or_else(|| missing(name))?, name)?;
    exact_keys(object, &["streamId", "chunkBytes"])?;
    id(object.get("streamId"), "streamId")?;
    if unsigned(object.get("chunkBytes"), "chunkBytes")? != super::MAX_TEXT_CHUNK_BYTES as u64 {
        return fail("text stream chunkBytes must be 256 KiB");
    }
    Ok(())
}

fn file_version(value: Option<&Value>) -> Result<&str, ProjectContractError> {
    let value = string(value, "file version")?;
    let Some(digest) = value.strip_prefix(FILE_VERSION_PREFIX_V1) else {
        return fail("file version must use sha256 prefix");
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return fail("file version must contain a lowercase 256-bit digest");
    }
    Ok(value)
}

fn optional_file_version(value: Option<&Value>) -> Result<(), ProjectContractError> {
    if !matches!(value, Some(Value::Null)) {
        file_version(value)?;
    }
    Ok(())
}

fn diff_kind(value: Option<&Value>) -> Result<&str, ProjectContractError> {
    let kind = string(value, "diff kind")?;
    if !["added", "modified", "deleted", "renamed", "binary"].contains(&kind) {
        return fail("unknown diff change kind");
    }
    Ok(kind)
}

fn object<'a>(
    value: &'a Value,
    name: &str,
) -> Result<&'a Map<String, Value>, ProjectContractError> {
    value
        .as_object()
        .ok_or_else(|| ProjectContractError::new(format!("{name} must be an object")))
}

fn array<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a [Value], ProjectContractError> {
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| ProjectContractError::new(format!("{name} must be an array")))
}

fn exact_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), ProjectContractError> {
    let actual: HashSet<&str> = object.keys().map(String::as_str).collect();
    let expected: HashSet<&str> = expected.iter().copied().collect();
    if actual != expected {
        return fail("payload contains missing or unknown fields");
    }
    Ok(())
}

fn id<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str, ProjectContractError> {
    let value = string(value, name)?;
    if value.is_empty() || value.len() > 512 {
        return fail(format!("{name} is required"));
    }
    Ok(value)
}

fn string<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str, ProjectContractError> {
    value
        .and_then(Value::as_str)
        .ok_or_else(|| ProjectContractError::new(format!("{name} must be a string")))
}

fn unsigned(value: Option<&Value>, name: &str) -> Result<u64, ProjectContractError> {
    value
        .and_then(Value::as_u64)
        .ok_or_else(|| ProjectContractError::new(format!("{name} must be a non-negative integer")))
}

fn boolean(value: Option<&Value>, name: &str) -> Result<bool, ProjectContractError> {
    value
        .and_then(Value::as_bool)
        .ok_or_else(|| ProjectContractError::new(format!("{name} must be boolean")))
}

fn relative_path<'a>(
    value: Option<&'a Value>,
    name: &str,
    allow_empty: bool,
) -> Result<&'a str, ProjectContractError> {
    let value = string(value, name)?;
    if (!allow_empty && value.is_empty()) || value.starts_with('/') || value.contains('\\') {
        return fail(format!("{name} must be a normalized relative path"));
    }
    if !(allow_empty && value.is_empty())
        && value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return fail(format!(
            "{name} must not traverse or contain empty segments"
        ));
    }
    Ok(value)
}

fn absolute_path<'a>(
    value: Option<&'a Value>,
    name: &str,
) -> Result<&'a str, ProjectContractError> {
    let value = string(value, name)?;
    let bytes = value.as_bytes();
    let windows =
        bytes.len() >= 3 && bytes[0].is_ascii_uppercase() && bytes[1] == b':' && bytes[2] == b'/';
    if (!value.starts_with('/') && !windows)
        || value == "/"
        || value.contains('\\')
        || value.ends_with('/')
    {
        return fail(format!("{name} must be a canonical absolute path"));
    }
    if value.split('/').enumerate().any(|(index, segment)| {
        (index > 0 && segment.is_empty())
            || matches!(segment, "." | "..")
            || (windows
                && index == 0
                && !(segment.len() == 2
                    && segment.as_bytes()[0].is_ascii_uppercase()
                    && segment.as_bytes()[1] == b':'))
    }) {
        return fail(format!("{name} must be normalized"));
    }
    Ok(value)
}

fn at_or_below(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn missing(name: &str) -> ProjectContractError {
    ProjectContractError::new(format!("{name} is required"))
}

fn fail<T>(detail: impl Into<String>) -> Result<T, ProjectContractError> {
    Err(ProjectContractError::new(detail))
}

#[cfg(test)]
mod tests;
