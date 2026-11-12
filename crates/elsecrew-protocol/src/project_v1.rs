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
pub const WORKSPACE_KIND_V1: &str = "standaloneSnapshot";
pub const SNAPSHOT_TRANSPORT_V1: &str = "gitBundle";
pub const SOURCE_MUTATION_POLICY_V1: &str = "forbidden";
pub const WORKTREE_LIFECYCLES_V1: &[&str] = &["active", "abandoned", "recoveryRequired"];
pub const SNAPSHOT_BUNDLE_MAX_BYTES_V1: u64 = 2 * 1024 * 1024 * 1024;
pub const SNAPSHOT_MATERIALIZED_TOTAL_MAX_BYTES_V1: u64 = 4 * 1024 * 1024 * 1024;
pub const SNAPSHOT_ENTRIES_MAX_V1: u64 = 200_000;
pub const SNAPSHOT_SINGLE_BLOB_MAX_BYTES_V1: u64 = 1024 * 1024 * 1024;
pub const SNAPSHOT_DISK_RESERVE_BYTES_V1: u64 = 512 * 1024 * 1024;
pub const SNAPSHOT_CREATION_PHASES_V1: &[&str] = &[
    "recoveryMarkerPersisted",
    "sourceDigestBefore",
    "bundleHeaderVerified",
    "bundleExportStreamed",
    "sourceDigestAfter",
    "bundleVerified",
    "materializationStreamed",
    "workspaceVerified",
    "externalManifestPersisted",
    "gitMarkerPersisted",
    "workspaceRenamed",
    "databaseCommitted",
    "activated",
];
pub const SNAPSHOT_FAILURE_KINDS_V1: &[&str] = &[
    "corruptBundle",
    "truncatedBundle",
    "oversizeBundle",
    "shallowMissingObjects",
];
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
        "standaloneSnapshotInvariant" => validate_standalone_snapshot_invariant(value),
        "sourceMutationTrace" => validate_source_mutation_trace(value),
        "snapshotSizing" => validate_snapshot_sizing(value),
        "snapshotMaterialization" => validate_snapshot_materialization(value),
        "snapshotLifecycle" => validate_snapshot_lifecycle(value),
        "snapshotFailure" => validate_snapshot_failure(value),
        "snapshotActivationEvidence" => validate_snapshot_activation_evidence(value),
        "treePage" => validate_tree_page(value),
        "fileClassification" => validate_file_classification(value),
        "fileDigest" => validate_file_digest(value),
        "fileRead" => validate_file_read(value),
        "saveTranscript" => validate_save_transcript(value),
        "diffSummary" => validate_diff_summary(value),
        "diffFile" => validate_diff_file(value),
        "watchEvent" => validate_watch_event(value),
        "projectClose" => validate_project_close(value),
        "safetyError" => validate_safety_error(value),
        "g4Ready" => validate_g4_ready(value),
        "g4Status" => validate_g4_status(value),
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
            "workspaceKind",
            "snapshotTransport",
            "sourceMutationPolicy",
            "objectFormat",
            "baseCommit",
            "baseTree",
            "sourceIdentity",
            "workspaceIdentity",
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
    id(object.get("sourceIdentity"), "sourceIdentity")?;
    id(object.get("workspaceIdentity"), "workspaceIdentity")?;
    let root = absolute_path(object.get("canonicalGitRoot"), "canonicalGitRoot")?;
    let worktree = absolute_path(object.get("worktreePath"), "worktreePath")?;
    if root == worktree || at_or_below(worktree, root) {
        return fail(
            "snapshot compatibility worktreePath cannot be the original Git root or inside it",
        );
    }
    validate_worktree_manifest_policy_fields_v1(&WorktreeManifestPolicyFieldsV1 {
        workspace_kind: string(object.get("workspaceKind"), "workspaceKind")?,
        snapshot_transport: string(object.get("snapshotTransport"), "snapshotTransport")?,
        source_mutation_policy: string(object.get("sourceMutationPolicy"), "sourceMutationPolicy")?,
        object_format: string(object.get("objectFormat"), "objectFormat")?,
        base_commit: string(object.get("baseCommit"), "baseCommit")?,
        base_tree: string(object.get("baseTree"), "baseTree")?,
        head_mode: string(object.get("headMode"), "headMode")?,
        lifecycle: string(object.get("lifecycle"), "lifecycle")?,
    })
}

/// Fixed manifest policy fields. Identity, exact keys and both normalized path
/// predicates are separate and are not implied by this pure policy check.
pub struct WorktreeManifestPolicyFieldsV1<'a> {
    pub workspace_kind: &'a str,
    pub snapshot_transport: &'a str,
    pub source_mutation_policy: &'a str,
    pub object_format: &'a str,
    pub base_commit: &'a str,
    pub base_tree: &'a str,
    pub head_mode: &'a str,
    pub lifecycle: &'a str,
}

pub fn validate_worktree_manifest_policy_fields_v1(
    fields: &WorktreeManifestPolicyFieldsV1<'_>,
) -> Result<(), ProjectContractError> {
    if fields.workspace_kind != WORKSPACE_KIND_V1 {
        return fail("workspace must be a standalone snapshot");
    }
    if fields.snapshot_transport != SNAPSHOT_TRANSPORT_V1 {
        return fail("snapshot transport must be gitBundle");
    }
    if fields.source_mutation_policy != SOURCE_MUTATION_POLICY_V1 {
        return fail("source mutation is forbidden");
    }
    let object_format = fields.object_format;
    if !matches!(object_format, "sha1" | "sha256") {
        return fail("unsupported Git object format");
    }
    validate_git_oid_text(fields.base_commit, object_format, "baseCommit")?;
    validate_git_oid_text(fields.base_tree, object_format, "baseTree")?;
    if fields.head_mode != WORKTREE_HEAD_MODE_V1 {
        return fail("standalone snapshot must use detached HEAD");
    }
    if !WORKTREE_LIFECYCLES_V1.contains(&fields.lifecycle) {
        return fail("unknown snapshot lifecycle");
    }
    Ok(())
}

fn validate_standalone_snapshot_invariant(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "standalone snapshot invariant")?;
    exact_keys(
        object,
        &[
            "workspaceIdentity",
            "objectFormat",
            "headCommit",
            "baseCommit",
            "indexTree",
            "baseTree",
            "remoteCount",
            "alternatesCount",
            "sourceRefCount",
            "sourceAbsolutePathCount",
            "objectConnectivity",
            "externalManifestIdentity",
            "gitMarkerIdentity",
            "databaseIdentity",
        ],
    )?;
    let identity = id(object.get("workspaceIdentity"), "workspaceIdentity")?;
    let object_format = string(object.get("objectFormat"), "objectFormat")?;
    if !matches!(object_format, "sha1" | "sha256") {
        return fail("unsupported Git object format");
    }
    let head = git_oid(object.get("headCommit"), object_format, "headCommit")?;
    let commit = git_oid(object.get("baseCommit"), object_format, "baseCommit")?;
    let index = git_oid(object.get("indexTree"), object_format, "indexTree")?;
    let tree = git_oid(object.get("baseTree"), object_format, "baseTree")?;
    if head != commit || index != tree {
        return fail("detached HEAD and index tree must match the snapshot base");
    }
    for field in [
        "remoteCount",
        "alternatesCount",
        "sourceRefCount",
        "sourceAbsolutePathCount",
    ] {
        if unsigned(object.get(field), field)? != 0 {
            return fail(format!("{field} must be zero in a standalone snapshot"));
        }
    }
    if !boolean(object.get("objectConnectivity"), "objectConnectivity")? {
        return fail("snapshot object connectivity must be verified");
    }
    for field in [
        "externalManifestIdentity",
        "gitMarkerIdentity",
        "databaseIdentity",
    ] {
        if string(object.get(field), field)? != identity {
            return fail(format!("{field} must match workspaceIdentity"));
        }
    }
    Ok(())
}

fn validate_source_mutation_trace(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "source mutation trace")?;
    exact_keys(
        object,
        &[
            "sourceCapability",
            "sourceLockCount",
            "sourceWriteCount",
            "before",
            "after",
        ],
    )?;
    if string(object.get("sourceCapability"), "sourceCapability")? != "readOnly"
        || unsigned(object.get("sourceLockCount"), "sourceLockCount")? != 0
        || unsigned(object.get("sourceWriteCount"), "sourceWriteCount")? != 0
    {
        return fail(
            "snapshot export must hold only a read-only source capability without locks or writes",
        );
    }
    let before = source_digest_set(object.get("before"), "before source digest")?;
    let after = source_digest_set(object.get("after"), "after source digest")?;
    for field in SOURCE_DIGEST_FIELDS_V1 {
        if before.get(*field) != after.get(*field) {
            return fail(format!("source {field} changed during snapshot export"));
        }
    }
    Ok(())
}

fn validate_snapshot_sizing(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "snapshot sizing")?;
    exact_keys(
        object,
        &[
            "bundleBytes",
            "materializedTotalBytes",
            "entryCount",
            "singleBlobBytes",
            "diskBytesAfterMaterialization",
            "streaming",
            "fullAllocation",
        ],
    )?;
    for (field, limit) in [
        ("bundleBytes", SNAPSHOT_BUNDLE_MAX_BYTES_V1),
        (
            "materializedTotalBytes",
            SNAPSHOT_MATERIALIZED_TOTAL_MAX_BYTES_V1,
        ),
        ("entryCount", SNAPSHOT_ENTRIES_MAX_V1),
        ("singleBlobBytes", SNAPSHOT_SINGLE_BLOB_MAX_BYTES_V1),
    ] {
        let value = unsigned(object.get(field), field)?;
        if value > super::MAX_SEQUENCE || value > limit {
            return fail(format!("{field} exceeds the standalone snapshot limit"));
        }
    }
    if unsigned(
        object.get("diskBytesAfterMaterialization"),
        "diskBytesAfterMaterialization",
    )? < SNAPSHOT_DISK_RESERVE_BYTES_V1
    {
        return fail("diskBytesAfterMaterialization must preserve the required disk reserve");
    }
    if !boolean(object.get("streaming"), "streaming")?
        || boolean(object.get("fullAllocation"), "fullAllocation")?
    {
        return fail("snapshot export and materialization must stream without full allocation");
    }
    Ok(())
}

fn validate_snapshot_materialization(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "snapshot materialization policy")?;
    exact_keys(
        object,
        &[
            "rawCommittedBlobBytes",
            "preserveGitMode",
            "checkout",
            "filters",
            "eolTransform",
            "encodingTransform",
            "identTransform",
            "exportAttributes",
            "lfsTransform",
            "symlinkPolicy",
            "submodulePolicy",
            "rejectCaseCollision",
            "rejectNonUtf8Path",
            "rejectDotGitEntry",
            "rejectAbnormalMode",
        ],
    )?;
    if !boolean(object.get("rawCommittedBlobBytes"), "rawCommittedBlobBytes")?
        || !boolean(object.get("preserveGitMode"), "preserveGitMode")?
    {
        return fail("materialization must preserve committed blob bytes and mode");
    }
    for field in [
        "checkout",
        "filters",
        "eolTransform",
        "encodingTransform",
        "identTransform",
        "exportAttributes",
        "lfsTransform",
    ] {
        if boolean(object.get(field), field)? {
            return fail(format!("{field} transformation is forbidden"));
        }
    }
    if string(object.get("symlinkPolicy"), "symlinkPolicy")? != "noFollow"
        || string(object.get("submodulePolicy"), "submodulePolicy")? != "noInit"
    {
        return fail("symlinks cannot be followed and submodules cannot be initialized");
    }
    for field in [
        "rejectCaseCollision",
        "rejectNonUtf8Path",
        "rejectDotGitEntry",
        "rejectAbnormalMode",
    ] {
        if !boolean(object.get(field), field)? {
            return fail(format!("{field} must fail closed"));
        }
    }
    Ok(())
}

fn validate_snapshot_lifecycle(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "snapshot lifecycle")?;
    exact_keys(
        object,
        &[
            "phases",
            "bundleHeader",
            "workspaceIdentity",
            "externalManifestIdentity",
            "gitMarkerIdentity",
            "databaseIdentity",
            "activatedWorkspaceCount",
        ],
    )?;
    let phases = array(object.get("phases"), "snapshot phases")?;
    if phases.len() != SNAPSHOT_CREATION_PHASES_V1.len()
        || phases
            .iter()
            .zip(SNAPSHOT_CREATION_PHASES_V1)
            .any(|(actual, expected)| actual.as_str() != Some(expected))
    {
        return fail("snapshot creation phases must be complete and ordered");
    }
    let header = self::object(
        object
            .get("bundleHeader")
            .ok_or_else(|| missing("bundleHeader"))?,
        "bundle header",
    )?;
    exact_keys(
        header,
        &[
            "transport",
            "references",
            "prerequisites",
            "selfContainedHead",
        ],
    )?;
    let references = array(header.get("references"), "bundle references")?;
    let prerequisites = array(header.get("prerequisites"), "bundle prerequisites")?;
    if string(header.get("transport"), "bundle transport")? != SNAPSHOT_TRANSPORT_V1
        || references.len() != 1
        || references[0].as_str() != Some("HEAD")
        || !prerequisites.is_empty()
        || !boolean(header.get("selfContainedHead"), "selfContainedHead")?
    {
        return fail("bundle header must prove a self-contained HEAD snapshot");
    }
    let identity = id(object.get("workspaceIdentity"), "workspaceIdentity")?;
    for field in [
        "externalManifestIdentity",
        "gitMarkerIdentity",
        "databaseIdentity",
    ] {
        if string(object.get(field), field)? != identity {
            return fail(format!("{field} must match before activation"));
        }
    }
    if unsigned(
        object.get("activatedWorkspaceCount"),
        "activatedWorkspaceCount",
    )? != 1
    {
        return fail("exactly one workspace may activate after all durable identity records");
    }
    Ok(())
}

fn validate_snapshot_failure(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "snapshot failure")?;
    exact_keys(
        object,
        &[
            "failureKind",
            "activeWorkspaceCount",
            "recoveryMarkerPresent",
        ],
    )?;
    if !SNAPSHOT_FAILURE_KINDS_V1.contains(&string(object.get("failureKind"), "failureKind")?) {
        return fail("unknown snapshot failure kind");
    }
    if unsigned(object.get("activeWorkspaceCount"), "activeWorkspaceCount")? != 0
        || !boolean(object.get("recoveryMarkerPresent"), "recoveryMarkerPresent")?
    {
        return fail(
            "failed snapshot creation must activate nothing and retain its recovery marker",
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SnapshotEvidenceIdentity<'a> {
    project_id: &'a str,
    worktree_id: &'a str,
    workspace_identity: &'a str,
    canonical_git_root: &'a str,
    source_identity: &'a str,
    object_format: &'a str,
    base_commit: &'a str,
    base_tree: &'a str,
}

fn validate_snapshot_activation_evidence(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "snapshot activation evidence")?;
    exact_keys(object, &["manifest", "source", "workspace", "activation"])?;

    let manifest_value = object.get("manifest").ok_or_else(|| missing("manifest"))?;
    validate_worktree_manifest(manifest_value)?;
    let manifest = self::object(manifest_value, "worktree manifest")?;
    if string(manifest.get("lifecycle"), "manifest lifecycle")? != "active" {
        return fail("activation evidence must describe the proposed active manifest");
    }
    let canonical = snapshot_identity_from_manifest(manifest)?;

    let source = self::object(
        object.get("source").ok_or_else(|| missing("source"))?,
        "source activation evidence",
    )?;
    exact_keys(source, &["identity", "trace"])?;
    assert_same_snapshot_identity(
        canonical,
        validate_snapshot_evidence_identity(source.get("identity"), "source evidence identity")?,
        "source evidence",
    )?;
    validate_source_mutation_trace(source.get("trace").ok_or_else(|| missing("source trace"))?)?;

    let workspace = self::object(
        object
            .get("workspace")
            .ok_or_else(|| missing("workspace"))?,
        "workspace activation evidence",
    )?;
    exact_keys(workspace, &["identity", "invariant"])?;
    assert_same_snapshot_identity(
        canonical,
        validate_snapshot_evidence_identity(
            workspace.get("identity"),
            "workspace evidence identity",
        )?,
        "workspace evidence",
    )?;
    let invariant_value = workspace
        .get("invariant")
        .ok_or_else(|| missing("workspace invariant"))?;
    validate_standalone_snapshot_invariant(invariant_value)?;
    let invariant = self::object(invariant_value, "standalone snapshot invariant")?;
    for (field, expected) in [
        ("workspaceIdentity", canonical.workspace_identity),
        ("objectFormat", canonical.object_format),
        ("baseCommit", canonical.base_commit),
        ("baseTree", canonical.base_tree),
    ] {
        if string(invariant.get(field), field)? != expected {
            return fail(format!(
                "workspace invariant {field} must match the proposed manifest"
            ));
        }
    }

    let activation = self::object(
        object
            .get("activation")
            .ok_or_else(|| missing("activation"))?,
        "pre-activation trace",
    )?;
    exact_keys(
        activation,
        &[
            "identity",
            "completedPhases",
            "activeWorkspaceCount",
            "proposedLifecycle",
            "database",
            "externalManifest",
            "gitMarker",
        ],
    )?;
    assert_same_snapshot_identity(
        canonical,
        validate_snapshot_evidence_identity(
            activation.get("identity"),
            "activation trace identity",
        )?,
        "activation trace",
    )?;
    let completed_phases = array(activation.get("completedPhases"), "completedPhases")?;
    let pre_activation_phases =
        &SNAPSHOT_CREATION_PHASES_V1[..SNAPSHOT_CREATION_PHASES_V1.len() - 1];
    if completed_phases.len() != pre_activation_phases.len()
        || completed_phases
            .iter()
            .zip(pre_activation_phases)
            .any(|(actual, expected)| actual.as_str() != Some(expected))
    {
        return fail("pre-activation phases must be complete and ordered without activated");
    }
    if unsigned(
        activation.get("activeWorkspaceCount"),
        "activeWorkspaceCount",
    )? != 0
        || string(activation.get("proposedLifecycle"), "proposedLifecycle")? != "active"
    {
        return fail(
            "activation evidence must prove zero active workspaces before the active transition",
        );
    }
    for field in ["database", "externalManifest", "gitMarker"] {
        assert_same_snapshot_identity(
            canonical,
            validate_snapshot_evidence_identity(activation.get(field), field)?,
            field,
        )?;
    }
    Ok(())
}

fn snapshot_identity_from_manifest<'a>(
    manifest: &'a Map<String, Value>,
) -> Result<SnapshotEvidenceIdentity<'a>, ProjectContractError> {
    let object_format = string(manifest.get("objectFormat"), "manifest objectFormat")?;
    Ok(SnapshotEvidenceIdentity {
        project_id: id(manifest.get("projectId"), "manifest projectId")?,
        worktree_id: id(manifest.get("worktreeId"), "manifest worktreeId")?,
        workspace_identity: id(
            manifest.get("workspaceIdentity"),
            "manifest workspaceIdentity",
        )?,
        canonical_git_root: absolute_path(
            manifest.get("canonicalGitRoot"),
            "manifest canonicalGitRoot",
        )?,
        source_identity: id(manifest.get("sourceIdentity"), "manifest sourceIdentity")?,
        object_format,
        base_commit: git_oid(
            manifest.get("baseCommit"),
            object_format,
            "manifest baseCommit",
        )?,
        base_tree: git_oid(manifest.get("baseTree"), object_format, "manifest baseTree")?,
    })
}

fn validate_snapshot_evidence_identity<'a>(
    value: Option<&'a Value>,
    name: &str,
) -> Result<SnapshotEvidenceIdentity<'a>, ProjectContractError> {
    let object = self::object(value.ok_or_else(|| missing(name))?, name)?;
    exact_keys(
        object,
        &[
            "projectId",
            "worktreeId",
            "workspaceIdentity",
            "canonicalGitRoot",
            "sourceIdentity",
            "objectFormat",
            "baseCommit",
            "baseTree",
        ],
    )?;
    let object_format = string(object.get("objectFormat"), "objectFormat")?;
    if !matches!(object_format, "sha1" | "sha256") {
        return fail(format!("{name} has an unsupported Git object format"));
    }
    Ok(SnapshotEvidenceIdentity {
        project_id: id(object.get("projectId"), "projectId")?,
        worktree_id: id(object.get("worktreeId"), "worktreeId")?,
        workspace_identity: id(object.get("workspaceIdentity"), "workspaceIdentity")?,
        canonical_git_root: absolute_path(object.get("canonicalGitRoot"), "canonicalGitRoot")?,
        source_identity: id(object.get("sourceIdentity"), "sourceIdentity")?,
        object_format,
        base_commit: git_oid(object.get("baseCommit"), object_format, "baseCommit")?,
        base_tree: git_oid(object.get("baseTree"), object_format, "baseTree")?,
    })
}

fn assert_same_snapshot_identity(
    expected: SnapshotEvidenceIdentity<'_>,
    actual: SnapshotEvidenceIdentity<'_>,
    name: &str,
) -> Result<(), ProjectContractError> {
    for (field, expected_value, actual_value) in [
        ("projectId", expected.project_id, actual.project_id),
        ("worktreeId", expected.worktree_id, actual.worktree_id),
        (
            "workspaceIdentity",
            expected.workspace_identity,
            actual.workspace_identity,
        ),
        (
            "canonicalGitRoot",
            expected.canonical_git_root,
            actual.canonical_git_root,
        ),
        (
            "sourceIdentity",
            expected.source_identity,
            actual.source_identity,
        ),
        ("objectFormat", expected.object_format, actual.object_format),
        ("baseCommit", expected.base_commit, actual.base_commit),
        ("baseTree", expected.base_tree, actual.base_tree),
    ] {
        if actual_value != expected_value {
            return fail(format!("{name} {field} must match the proposed manifest"));
        }
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
    let parent_path = relative_path(object.get("parentPath"), "parentPath", true)?;
    let entries = array(object.get("entries"), "tree entries")?;
    if entries.len() > TREE_PAGE_MAX_ENTRIES_V1 {
        return fail("tree page exceeds 500 entries");
    }
    let mut previous: Option<(bool, &str)> = None;
    let mut paths = std::collections::HashSet::new();
    for entry in entries {
        let entry = self::object(entry, "tree entry")?;
        exact_keys(entry, &["path", "name", "kind"])?;
        let path = relative_path(entry.get("path"), "tree entry path", false)?;
        let child_name = if parent_path.is_empty() {
            path
        } else {
            path.strip_prefix(parent_path)
                .and_then(|suffix| suffix.strip_prefix('/'))
                .unwrap_or_default()
        };
        if child_name.is_empty() || child_name.contains('/') {
            return fail("tree entry must be a direct child of parentPath");
        }
        let name = id(entry.get("name"), "tree entry name")?;
        if path.rsplit('/').next() != Some(name) {
            return fail("tree entry name must match its path");
        }
        if !["file", "directory", "symlink"].contains(&string(entry.get("kind"), "kind")?) {
            return fail("unknown tree entry kind");
        }
        let current = (string(entry.get("kind"), "kind")? != "directory", path);
        if !paths.insert(path) || previous.is_some_and(|previous| current <= previous) {
            return fail("tree entries must be directory-first, sorted and unique");
        }
        previous = Some(current);
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

fn validate_file_digest(value: &Value) -> Result<(), ProjectContractError> {
    let object = object(value, "file digest vector")?;
    exact_keys(object, &["inputHex", "version"])?;
    let input_hex = string(object.get("inputHex"), "digest inputHex")?;
    if input_hex.len() % 2 != 0
        || !input_hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return fail("digest inputHex must encode exact bytes as lowercase hex");
    }
    let bytes = input_hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("validated ASCII hex");
            u8::from_str_radix(pair, 16).expect("validated lowercase hex")
        })
        .collect::<Vec<_>>();
    let expected = file_version(object.get("version"))?;
    if sha256_file_version_v1(&bytes) != expected {
        return fail("file version digest does not match exact bytes");
    }
    Ok(())
}

pub fn sha256_file_version_v1(input: &[u8]) -> String {
    let bit_length = (input.len() as u64) * 8;
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());

    let mut hash = [
        0x6a09e667_u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    for block in padded.chunks_exact(64) {
        let mut words = [0_u32; 64];
        for (index, bytes) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes(bytes.try_into().expect("four-byte SHA word"));
        }
        for index in 16..64 {
            let left = words[index - 15];
            let right = words[index - 2];
            let sigma0 = left.rotate_right(7) ^ left.rotate_right(18) ^ (left >> 3);
            let sigma1 = right.rotate_right(17) ^ right.rotate_right(19) ^ (right >> 10);
            words[index] = words[index - 16]
                .wrapping_add(sigma0)
                .wrapping_add(words[index - 7])
                .wrapping_add(sigma1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;
        for index in 0..64 {
            let choice = (e & f) ^ (!e & g);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let first = h
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(SHA256_ROUND_CONSTANTS[index])
                .wrapping_add(words[index]);
            let second = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(first);
            d = c;
            c = b;
            b = a;
            a = first.wrapping_add(second);
        }
        for (value, addition) in hash.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *value = value.wrapping_add(addition);
        }
    }
    format!(
        "{FILE_VERSION_PREFIX_V1}{}",
        hash.iter()
            .map(|word| format!("{word:08x}"))
            .collect::<String>()
    )
}

const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn validate_g4_ready(value: &Value) -> Result<(), ProjectContractError> {
    let ready = serde_json::from_value::<super::ReadyPayloadV1>(value.clone())
        .map_err(|error| ProjectContractError::new(format!("invalid G4 ready payload: {error}")))?;
    super::lifecycle_v1::validate_g4_ready_identity_v1(&ready)
        .map_err(|error| ProjectContractError::new(error.to_string()))
}

fn validate_g4_status(value: &Value) -> Result<(), ProjectContractError> {
    let status = serde_json::from_value::<super::lifecycle_v1::SystemStatusV1>(value.clone())
        .map_err(|error| {
            ProjectContractError::new(format!("invalid G4 status payload: {error}"))
        })?;
    super::lifecycle_v1::validate_g4_system_status_v1(&status)
        .map_err(|error| ProjectContractError::new(error.to_string()))
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
        return fail(
            "project.close must abandon without physically deleting the standalone snapshot",
        );
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

const SOURCE_DIGEST_FIELDS_V1: &[&str] = &[
    "head",
    "index",
    "refs",
    "config",
    "objects",
    "packedRefs",
    "entrySet",
];

fn source_digest_set<'a>(
    value: Option<&'a Value>,
    name: &str,
) -> Result<&'a Map<String, Value>, ProjectContractError> {
    let object = object(value.ok_or_else(|| missing(name))?, name)?;
    exact_keys(object, SOURCE_DIGEST_FIELDS_V1)?;
    for field in SOURCE_DIGEST_FIELDS_V1 {
        let digest = string(object.get(*field), field)?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return fail(format!("{name} {field} must be a lowercase SHA-256 digest"));
        }
    }
    Ok(object)
}

fn git_oid<'a>(
    value: Option<&'a Value>,
    object_format: &str,
    name: &str,
) -> Result<&'a str, ProjectContractError> {
    let value = string(value, name)?;
    validate_git_oid_text(value, object_format, name)?;
    Ok(value)
}

fn validate_git_oid_text(
    value: &str,
    object_format: &str,
    name: &str,
) -> Result<(), ProjectContractError> {
    let length = match object_format {
        "sha1" => 40,
        "sha256" => 64,
        _ => return fail("unsupported Git object format"),
    };
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return fail(format!("{name} length must match {object_format}"));
    }
    Ok(())
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
    let value = value.and_then(Value::as_u64).ok_or_else(|| {
        ProjectContractError::new(format!("{name} must be a non-negative integer"))
    })?;
    if value > super::MAX_SEQUENCE {
        return fail(format!(
            "{name} must be a non-negative JavaScript safe integer"
        ));
    }
    Ok(value)
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
    let mut scan = AbsolutePathScanV1::default();
    scan.push(value.as_bytes())?;
    scan.finish(name)?;
    Ok(value)
}

/// The existing manifest path predicate, evaluated over decoded UTF-8 fragments.
/// No path, component or source document is retained and no filesystem is used.
#[derive(Default)]
pub struct AbsolutePathScanV1 {
    prefix: [u8; 3],
    length: u64,
    last: u8,
    segment_length: u64,
    segment_dots: bool,
    backslash: bool,
    bad_segment: bool,
    utf8: [u8; 3],
    utf8_len: usize,
}

impl AbsolutePathScanV1 {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), ProjectContractError> {
        let mut remaining = bytes;
        if self.utf8_len > 0 {
            let width = match self.utf8[0] {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return fail("manifest path must be UTF-8"),
            };
            let take = (width - self.utf8_len).min(remaining.len());
            let mut scalar = [0_u8; 4];
            scalar[..self.utf8_len].copy_from_slice(&self.utf8[..self.utf8_len]);
            scalar[self.utf8_len..self.utf8_len + take].copy_from_slice(&remaining[..take]);
            let size = self.utf8_len + take;
            match std::str::from_utf8(&scalar[..size]) {
                Ok(_) => self.utf8_len = 0,
                Err(error) if error.error_len().is_none() && size < width => {
                    self.utf8[..size].copy_from_slice(&scalar[..size]);
                    self.utf8_len = size;
                }
                Err(_) => return fail("manifest path must be UTF-8"),
            }
            remaining = &remaining[take..];
        }
        if self.utf8_len == 0 {
            match std::str::from_utf8(remaining) {
                Ok(_) => {}
                Err(error) if error.error_len().is_none() => {
                    let tail = &remaining[error.valid_up_to()..];
                    if tail.len() > 3 {
                        return fail("manifest path must be UTF-8");
                    }
                    self.utf8[..tail.len()].copy_from_slice(tail);
                    self.utf8_len = tail.len();
                }
                Err(_) => return fail("manifest path must be UTF-8"),
            }
        }
        for byte in bytes {
            if self.length < 3 {
                self.prefix[self.length as usize] = *byte;
            }
            self.backslash |= *byte == b'\\';
            if *byte == b'/' {
                if self.length != 0
                    && (self.segment_length == 0 || (self.segment_length <= 2 && self.segment_dots))
                {
                    self.bad_segment = true;
                }
                self.segment_length = 0;
                self.segment_dots = true;
            } else {
                self.segment_dots =
                    (self.segment_length == 0 || self.segment_dots) && *byte == b'.';
                self.segment_length = self
                    .segment_length
                    .checked_add(1)
                    .ok_or_else(|| ProjectContractError::new("manifest path length overflow"))?;
            }
            self.last = *byte;
            self.length = self
                .length
                .checked_add(1)
                .ok_or_else(|| ProjectContractError::new("manifest path length overflow"))?;
        }
        Ok(())
    }

    pub fn finish(&self, name: &str) -> Result<(), ProjectContractError> {
        let windows = self.length >= 3
            && self.prefix[0].is_ascii_uppercase()
            && self.prefix[1] == b':'
            && self.prefix[2] == b'/';
        if self.utf8_len != 0 {
            return fail("manifest path must be UTF-8");
        }
        if self.length == 0
            || (self.prefix[0] != b'/' && !windows)
            || self.last == b'/'
            || self.backslash
        {
            return fail(format!("{name} must be a canonical absolute path"));
        }
        if self.bad_segment || (self.segment_length <= 2 && self.segment_dots) {
            return fail(format!("{name} must be normalized"));
        }
        Ok(())
    }
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
