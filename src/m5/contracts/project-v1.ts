import { MAX_TEXT_CHUNK_BYTES, METHODS_V1 } from "./protocol-v1.ts";

export const PROJECT_SCHEMA_VERSION_V1 = 1 as const;
export const TREE_PAGE_MAX_ENTRIES_V1 = 500 as const;
export const FILE_MAX_BYTES_V1 = 5 * 1024 * 1024;
export type ProjectWatchResultV1 = { watching: true } | { watching: false; rescanRequired: true };
export const FILE_VERSION_ALGORITHM_V1 = "sha256" as const;
export const FILE_VERSION_PREFIX_V1 = "sha256:" as const;
export const FILE_VERSION_DIGEST_INPUT_V1 = "exactFileBytes" as const;
export const WORKTREE_HEAD_MODE_V1 = "detached" as const;
export const WORKSPACE_KIND_V1 = "standaloneSnapshot" as const;
export const SNAPSHOT_TRANSPORT_V1 = "gitBundle" as const;
export const SOURCE_MUTATION_POLICY_V1 = "forbidden" as const;
export const WORKTREE_LIFECYCLES_V1 = [
  "active",
  "abandoned",
  "recoveryRequired",
] as const;
export const SNAPSHOT_LIMITS_V1 = {
  bundleMaxBytes: 2 * 1024 * 1024 * 1024,
  materializedTotalMaxBytes: 4 * 1024 * 1024 * 1024,
  entriesMax: 200_000,
  singleBlobMaxBytes: 1024 * 1024 * 1024,
  diskReserveBytes: 512 * 1024 * 1024,
  streamingRequired: true,
  fullAllocationForbidden: true,
} as const;
export const SNAPSHOT_CREATION_PHASES_V1 = [
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
] as const;
export const SNAPSHOT_FAILURE_KINDS_V1 = [
  "corruptBundle",
  "truncatedBundle",
  "oversizeBundle",
  "shallowMissingObjects",
] as const;

export const G4_CAPABILITIES_V1 = [
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
] as const;

export const G4_ERROR_KINDS_V1 = [
  "manifestMismatch",
  "symlinkEscape",
  "dirtyConflict",
  "recoveryRequired",
] as const;

export interface ProjectIdentityV1 {
  schemaVersion: typeof PROJECT_SCHEMA_VERSION_V1;
  projectId: string;
  selectedPath: string;
  canonicalGitRoot: string;
}

export interface WorktreeManifestV1 {
  schemaVersion: typeof PROJECT_SCHEMA_VERSION_V1;
  projectId: string;
  canonicalGitRoot: string;
  worktreeId: string;
  worktreePath: string;
  workspaceKind: typeof WORKSPACE_KIND_V1;
  snapshotTransport: typeof SNAPSHOT_TRANSPORT_V1;
  sourceMutationPolicy: typeof SOURCE_MUTATION_POLICY_V1;
  objectFormat: "sha1" | "sha256";
  baseCommit: string;
  baseTree: string;
  sourceIdentity: string;
  workspaceIdentity: string;
  headMode: typeof WORKTREE_HEAD_MODE_V1;
  lifecycle: (typeof WORKTREE_LIFECYCLES_V1)[number];
}

export type TreeEntryKindV1 = "file" | "directory" | "symlink";

export interface TreeEntryV1 {
  path: string;
  name: string;
  kind: TreeEntryKindV1;
}

export interface TreePageV1 {
  projectId: string;
  parentPath: string;
  entries: TreeEntryV1[];
  nextCursor: string | null;
}

export interface ProjectCloseResultV1 {
  projectId: string;
  lifecycle: "abandoned";
  physicallyDeleted: false;
}

export interface WatchEventV1 {
  projectId: string;
  event: "file.changed" | "file.conflict" | "diff.changed";
  paths: string[];
  rescanRequired: boolean;
}

export interface G4SafetyErrorV1 {
  code: string;
  kind: (typeof G4_ERROR_KINDS_V1)[number];
  detail: string;
  retryable: false;
  outcomeUnknown: false;
}

export function validateProjectIdentityV1(value: unknown): ProjectIdentityV1 {
  const object = requireObject(value, "project identity");
  exactKeys(object, ["schemaVersion", "projectId", "selectedPath", "canonicalGitRoot"]);
  if (requireSafeUnsigned(object.schemaVersion, "project schemaVersion") !== PROJECT_SCHEMA_VERSION_V1) fail("project schemaVersion must be 1");
  requireId(object.projectId, "projectId");
  const selectedPath = requireAbsolutePath(object.selectedPath, "selectedPath");
  const canonicalGitRoot = requireAbsolutePath(object.canonicalGitRoot, "canonicalGitRoot");
  if (!isAtOrBelow(selectedPath, canonicalGitRoot)) {
    fail("selectedPath must be the canonical Git root or one of its descendants");
  }
  return object as unknown as ProjectIdentityV1;
}

export function validateWorktreeManifestV1(value: unknown): WorktreeManifestV1 {
  const object = requireObject(value, "worktree manifest");
  exactKeys(object, [
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
  ]);
  if (requireSafeUnsigned(object.schemaVersion, "manifest schemaVersion") !== PROJECT_SCHEMA_VERSION_V1) fail("manifest schemaVersion must be 1");
  requireId(object.projectId, "projectId");
  requireId(object.worktreeId, "worktreeId");
  requireId(object.sourceIdentity, "sourceIdentity");
  requireId(object.workspaceIdentity, "workspaceIdentity");
  const root = requireAbsolutePath(object.canonicalGitRoot, "canonicalGitRoot");
  const worktree = requireAbsolutePath(object.worktreePath, "worktreePath");
  if (root === worktree || isAtOrBelow(worktree, root)) {
    fail("snapshot compatibility worktreePath cannot be the original Git root or inside it");
  }
  if (object.workspaceKind !== WORKSPACE_KIND_V1) fail("workspace must be a standalone snapshot");
  if (object.snapshotTransport !== SNAPSHOT_TRANSPORT_V1) fail("snapshot transport must be gitBundle");
  if (object.sourceMutationPolicy !== SOURCE_MUTATION_POLICY_V1) fail("source mutation is forbidden");
  if (object.objectFormat !== "sha1" && object.objectFormat !== "sha256") fail("unsupported Git object format");
  requireGitOid(object.baseCommit, object.objectFormat, "baseCommit");
  requireGitOid(object.baseTree, object.objectFormat, "baseTree");
  if (object.headMode !== WORKTREE_HEAD_MODE_V1) fail("standalone snapshot must use detached HEAD");
  if (!WORKTREE_LIFECYCLES_V1.includes(object.lifecycle as never)) fail("unknown snapshot lifecycle");
  return object as unknown as WorktreeManifestV1;
}

export function validateStandaloneSnapshotInvariantV1(value: unknown): void {
  const object = requireObject(value, "standalone snapshot invariant");
  exactKeys(object, [
    "workspaceIdentity", "objectFormat", "headCommit", "baseCommit", "indexTree", "baseTree",
    "remoteCount", "alternatesCount", "sourceRefCount", "sourceAbsolutePathCount",
    "objectConnectivity", "externalManifestIdentity", "gitMarkerIdentity", "databaseIdentity",
  ]);
  requireId(object.workspaceIdentity, "workspaceIdentity");
  if (object.objectFormat !== "sha1" && object.objectFormat !== "sha256") fail("unsupported Git object format");
  for (const field of ["headCommit", "baseCommit", "indexTree", "baseTree"] as const) {
    requireGitOid(object[field], object.objectFormat, field);
  }
  if (object.headCommit !== object.baseCommit || object.indexTree !== object.baseTree) {
    fail("detached HEAD and index tree must match the snapshot base");
  }
  for (const field of ["remoteCount", "alternatesCount", "sourceRefCount", "sourceAbsolutePathCount"] as const) {
    if (requireSafeUnsigned(object[field], field) !== 0) fail(`${field} must be zero in a standalone snapshot`);
  }
  if (object.objectConnectivity !== true) fail("snapshot object connectivity must be verified");
  for (const field of ["externalManifestIdentity", "gitMarkerIdentity", "databaseIdentity"] as const) {
    if (object[field] !== object.workspaceIdentity) fail(`${field} must match workspaceIdentity`);
  }
}

export function validateSourceMutationTraceV1(value: unknown): void {
  const object = requireObject(value, "source mutation trace");
  exactKeys(object, ["sourceCapability", "sourceLockCount", "sourceWriteCount", "before", "after"]);
  if (
    object.sourceCapability !== "readOnly" ||
    requireSafeUnsigned(object.sourceLockCount, "sourceLockCount") !== 0 ||
    requireSafeUnsigned(object.sourceWriteCount, "sourceWriteCount") !== 0
  ) {
    fail("snapshot export must hold only a read-only source capability without locks or writes");
  }
  const before = sourceDigestSet(object.before, "before source digest");
  const after = sourceDigestSet(object.after, "after source digest");
  for (const field of SOURCE_DIGEST_FIELDS_V1) {
    if (before[field] !== after[field]) fail(`source ${field} changed during snapshot export`);
  }
}

export function validateSnapshotSizingV1(value: unknown): void {
  const object = requireObject(value, "snapshot sizing");
  exactKeys(object, [
    "bundleBytes", "materializedTotalBytes", "entryCount", "singleBlobBytes",
    "diskBytesAfterMaterialization", "streaming", "fullAllocation",
  ]);
  const checks = [
    ["bundleBytes", SNAPSHOT_LIMITS_V1.bundleMaxBytes],
    ["materializedTotalBytes", SNAPSHOT_LIMITS_V1.materializedTotalMaxBytes],
    ["entryCount", SNAPSHOT_LIMITS_V1.entriesMax],
    ["singleBlobBytes", SNAPSHOT_LIMITS_V1.singleBlobMaxBytes],
  ] as const;
  for (const [field, limit] of checks) {
    if (requireSafeUnsigned(object[field], field) > limit) {
      fail(`${field} exceeds the standalone snapshot limit`);
    }
  }
  if (requireSafeUnsigned(object.diskBytesAfterMaterialization, "diskBytesAfterMaterialization") < SNAPSHOT_LIMITS_V1.diskReserveBytes) {
    fail("diskBytesAfterMaterialization must preserve the required disk reserve");
  }
  if (object.streaming !== true || object.fullAllocation !== false) {
    fail("snapshot export and materialization must stream without full allocation");
  }
}

export function validateSnapshotMaterializationV1(value: unknown): void {
  const object = requireObject(value, "snapshot materialization policy");
  exactKeys(object, [
    "rawCommittedBlobBytes", "preserveGitMode", "checkout", "filters", "eolTransform",
    "encodingTransform", "identTransform", "exportAttributes", "lfsTransform",
    "symlinkPolicy", "submodulePolicy", "rejectCaseCollision", "rejectNonUtf8Path",
    "rejectDotGitEntry", "rejectAbnormalMode",
  ]);
  if (object.rawCommittedBlobBytes !== true || object.preserveGitMode !== true) fail("materialization must preserve committed blob bytes and mode");
  for (const field of ["checkout", "filters", "eolTransform", "encodingTransform", "identTransform", "exportAttributes", "lfsTransform"] as const) {
    if (object[field] !== false) fail(`${field} transformation is forbidden`);
  }
  if (object.symlinkPolicy !== "noFollow" || object.submodulePolicy !== "noInit") fail("symlinks cannot be followed and submodules cannot be initialized");
  for (const field of ["rejectCaseCollision", "rejectNonUtf8Path", "rejectDotGitEntry", "rejectAbnormalMode"] as const) {
    if (object[field] !== true) fail(`${field} must fail closed`);
  }
}

export function validateSnapshotLifecycleV1(value: unknown): void {
  const object = requireObject(value, "snapshot lifecycle");
  exactKeys(object, [
    "phases", "bundleHeader", "workspaceIdentity", "externalManifestIdentity",
    "gitMarkerIdentity", "databaseIdentity", "activatedWorkspaceCount",
  ]);
  if (!Array.isArray(object.phases) || object.phases.length !== SNAPSHOT_CREATION_PHASES_V1.length || object.phases.some((phase, index) => phase !== SNAPSHOT_CREATION_PHASES_V1[index])) {
    fail("snapshot creation phases must be complete and ordered");
  }
  const header = requireObject(object.bundleHeader, "bundle header");
  exactKeys(header, ["transport", "references", "prerequisites", "selfContainedHead"]);
  if (
    header.transport !== SNAPSHOT_TRANSPORT_V1 ||
    !Array.isArray(header.references) ||
    header.references.length !== 1 ||
    header.references[0] !== "HEAD" ||
    !Array.isArray(header.prerequisites) ||
    header.prerequisites.length !== 0 ||
    header.selfContainedHead !== true
  ) {
    fail("bundle header must prove a self-contained HEAD snapshot");
  }
  requireId(object.workspaceIdentity, "workspaceIdentity");
  for (const field of ["externalManifestIdentity", "gitMarkerIdentity", "databaseIdentity"] as const) {
    if (object[field] !== object.workspaceIdentity) fail(`${field} must match before activation`);
  }
  if (requireSafeUnsigned(object.activatedWorkspaceCount, "activatedWorkspaceCount") !== 1) fail("exactly one workspace may activate after all durable identity records");
}

export function validateSnapshotFailureV1(value: unknown): void {
  const object = requireObject(value, "snapshot failure");
  exactKeys(object, ["failureKind", "activeWorkspaceCount", "recoveryMarkerPresent"]);
  if (!SNAPSHOT_FAILURE_KINDS_V1.includes(object.failureKind as never)) fail("unknown snapshot failure kind");
  if (requireSafeUnsigned(object.activeWorkspaceCount, "activeWorkspaceCount") !== 0 || object.recoveryMarkerPresent !== true) {
    fail("failed snapshot creation must activate nothing and retain its recovery marker");
  }
}

export function validateSnapshotActivationEvidenceV1(value: unknown): void {
  const object = requireObject(value, "snapshot activation evidence");
  exactKeys(object, ["manifest", "source", "workspace", "activation"]);

  const manifest = validateWorktreeManifestV1(object.manifest);
  if (manifest.lifecycle !== "active") fail("activation evidence must describe the proposed active manifest");
  const canonical = identityFromManifest(manifest);

  const source = requireObject(object.source, "source activation evidence");
  exactKeys(source, ["identity", "trace"]);
  assertSameSnapshotIdentity(canonical, validateSnapshotEvidenceIdentity(source.identity, "source evidence identity"), "source evidence");
  validateSourceMutationTraceV1(source.trace);

  const workspace = requireObject(object.workspace, "workspace activation evidence");
  exactKeys(workspace, ["identity", "invariant"]);
  assertSameSnapshotIdentity(canonical, validateSnapshotEvidenceIdentity(workspace.identity, "workspace evidence identity"), "workspace evidence");
  validateStandaloneSnapshotInvariantV1(workspace.invariant);
  const invariant = requireObject(workspace.invariant, "standalone snapshot invariant");
  for (const field of ["workspaceIdentity", "objectFormat", "baseCommit", "baseTree"] as const) {
    if (invariant[field] !== canonical[field]) fail(`workspace invariant ${field} must match the proposed manifest`);
  }

  const activation = requireObject(object.activation, "pre-activation trace");
  exactKeys(activation, [
    "identity", "completedPhases", "activeWorkspaceCount", "proposedLifecycle",
    "database", "externalManifest", "gitMarker",
  ]);
  assertSameSnapshotIdentity(canonical, validateSnapshotEvidenceIdentity(activation.identity, "activation trace identity"), "activation trace");
  const preActivationPhases = SNAPSHOT_CREATION_PHASES_V1.slice(0, -1);
  if (
    !Array.isArray(activation.completedPhases) ||
    activation.completedPhases.length !== preActivationPhases.length ||
    activation.completedPhases.some((phase, index) => phase !== preActivationPhases[index])
  ) {
    fail("pre-activation phases must be complete and ordered without activated");
  }
  if (
    requireSafeUnsigned(activation.activeWorkspaceCount, "activeWorkspaceCount") !== 0 ||
    activation.proposedLifecycle !== "active"
  ) {
    fail("activation evidence must prove zero active workspaces before the active transition");
  }
  for (const field of ["database", "externalManifest", "gitMarker"] as const) {
    assertSameSnapshotIdentity(
      canonical,
      validateSnapshotEvidenceIdentity(activation[field], `${field} identity`),
      field,
    );
  }
}

export function validateTreePageV1(value: unknown): TreePageV1 {
  const object = requireObject(value, "tree page");
  exactKeys(object, ["projectId", "parentPath", "entries", "nextCursor"]);
  requireId(object.projectId, "projectId");
  const parentPath = requireRelativePath(object.parentPath, "parentPath", true);
  if (!Array.isArray(object.entries) || object.entries.length > TREE_PAGE_MAX_ENTRIES_V1) {
    fail("tree page exceeds 500 entries");
  }
  let previous: [number, string] | null = null;
  const paths = new Set<string>();
  for (const entryValue of object.entries) {
    const entry = requireObject(entryValue, "tree entry");
    exactKeys(entry, ["path", "name", "kind"]);
    const path = requireRelativePath(entry.path, "tree entry path", false);
    const childName = parentPath === "" ? path : path.startsWith(`${parentPath}/`) ? path.slice(parentPath.length + 1) : "";
    if (childName.length === 0 || childName.includes("/")) {
      fail("tree entry must be a direct child of parentPath");
    }
    requireId(entry.name, "tree entry name");
    const segments = path.split("/");
    if (segments[segments.length - 1] !== entry.name) fail("tree entry name must match its path");
    if (!["file", "directory", "symlink"].includes(String(entry.kind))) fail("unknown tree entry kind");
    const rank = entry.kind === "directory" ? 0 : 1;
    if (paths.has(path) || (previous && (rank < previous[0] || (rank === previous[0] && path <= previous[1])))) {
      fail("tree entries must be directory-first, sorted and unique");
    }
    paths.add(path);
    previous = [rank, path];
  }
  if (object.nextCursor !== null) requireId(object.nextCursor, "opaque nextCursor");
  return object as unknown as TreePageV1;
}

export function validateProjectCloseResultV1(value: unknown): ProjectCloseResultV1 {
  const object = requireObject(value, "project close result");
  exactKeys(object, ["projectId", "lifecycle", "physicallyDeleted"]);
  requireId(object.projectId, "projectId");
  if (object.lifecycle !== "abandoned" || object.physicallyDeleted !== false) {
    fail("project.close must abandon without physically deleting the standalone snapshot");
  }
  return object as unknown as ProjectCloseResultV1;
}

export function validateWatchEventV1(value: unknown): WatchEventV1 {
  const object = requireObject(value, "watch event");
  exactKeys(object, ["projectId", "event", "paths", "rescanRequired"]);
  requireId(object.projectId, "projectId");
  if (!["file.changed", "file.conflict", "diff.changed"].includes(String(object.event))) {
    fail("unknown watch event");
  }
  if (!Array.isArray(object.paths)) fail("watch paths must be an array");
  for (const path of object.paths) requireRelativePath(path, "watch path", false);
  if (typeof object.rescanRequired !== "boolean") fail("rescanRequired must be boolean");
  if (object.rescanRequired && object.paths.length !== 0) {
    fail("watch overflow cannot claim a complete path list");
  }
  if (!object.rescanRequired && object.paths.length === 0) {
    fail("non-overflow watch event requires changed paths");
  }
  return object as unknown as WatchEventV1;
}

export function validateG4SafetyErrorV1(value: unknown): G4SafetyErrorV1 {
  const object = requireObject(value, "G4 safety error");
  exactKeys(object, ["code", "kind", "detail", "retryable", "outcomeUnknown"]);
  requireId(object.code, "error code");
  requireId(object.detail, "error detail");
  if (!G4_ERROR_KINDS_V1.includes(object.kind as never)) fail("unknown G4 safety error kind");
  if (object.retryable !== false || object.outcomeUnknown !== false) {
    fail("G4 safety boundary failures cannot be retried automatically");
  }
  return object as unknown as G4SafetyErrorV1;
}

export function assertG4CapabilitiesV1(): void {
  if (G4_CAPABILITIES_V1.some((capability) => !METHODS_V1.includes(capability))) {
    fail("G4 capability is not reserved by protocol v1");
  }
  if (MAX_TEXT_CHUNK_BYTES !== 256 * 1024) fail("protocol text chunk contract drifted");
}

export function requireObject(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) fail(`${name} must be an object`);
  return value as Record<string, unknown>;
}

export function exactKeys(object: Record<string, unknown>, expected: readonly string[]): void {
  const actual = Object.keys(object).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail(`unexpected fields: expected ${wanted.join(",")}, got ${actual.join(",")}`);
  }
}

export function requireId(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || value.length === 0 || value.length > 512) fail(`${name} is required`);
}

export function requireSafeUnsigned(value: unknown, name: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    fail(`${name} must be a non-negative JavaScript safe integer`);
  }
  return value;
}

const SNAPSHOT_EVIDENCE_IDENTITY_FIELDS_V1 = [
  "projectId", "worktreeId", "workspaceIdentity", "canonicalGitRoot", "sourceIdentity",
  "objectFormat", "baseCommit", "baseTree",
] as const;

type SnapshotEvidenceIdentityV1 = {
  [Field in (typeof SNAPSHOT_EVIDENCE_IDENTITY_FIELDS_V1)[number]]: string;
};

function identityFromManifest(manifest: WorktreeManifestV1): SnapshotEvidenceIdentityV1 {
  return {
    projectId: manifest.projectId,
    worktreeId: manifest.worktreeId,
    workspaceIdentity: manifest.workspaceIdentity,
    canonicalGitRoot: manifest.canonicalGitRoot,
    sourceIdentity: manifest.sourceIdentity,
    objectFormat: manifest.objectFormat,
    baseCommit: manifest.baseCommit,
    baseTree: manifest.baseTree,
  };
}

function validateSnapshotEvidenceIdentity(value: unknown, name: string): SnapshotEvidenceIdentityV1 {
  const object = requireObject(value, name);
  exactKeys(object, SNAPSHOT_EVIDENCE_IDENTITY_FIELDS_V1);
  requireId(object.projectId, `${name} projectId`);
  requireId(object.worktreeId, `${name} worktreeId`);
  requireId(object.workspaceIdentity, `${name} workspaceIdentity`);
  requireAbsolutePath(object.canonicalGitRoot, `${name} canonicalGitRoot`);
  requireId(object.sourceIdentity, `${name} sourceIdentity`);
  if (object.objectFormat !== "sha1" && object.objectFormat !== "sha256") fail(`${name} has an unsupported Git object format`);
  requireGitOid(object.baseCommit, object.objectFormat, `${name} baseCommit`);
  requireGitOid(object.baseTree, object.objectFormat, `${name} baseTree`);
  return object as SnapshotEvidenceIdentityV1;
}

function assertSameSnapshotIdentity(
  expected: SnapshotEvidenceIdentityV1,
  actual: SnapshotEvidenceIdentityV1,
  name: string,
): void {
  for (const field of SNAPSHOT_EVIDENCE_IDENTITY_FIELDS_V1) {
    if (actual[field] !== expected[field]) fail(`${name} ${field} must match the proposed manifest`);
  }
}

export function requireRelativePath(value: unknown, name: string, allowEmpty: boolean): string {
  if (typeof value !== "string") fail(`${name} must be a string`);
  if ((!allowEmpty && value.length === 0) || value.startsWith("/") || value.includes("\\")) {
    fail(`${name} must be a normalized relative path`);
  }
  if (value.split("/").some((segment) => segment === "." || segment === ".." || segment === "")) {
    if (!(allowEmpty && value === "")) fail(`${name} must not traverse or contain empty segments`);
  }
  return value;
}

const SOURCE_DIGEST_FIELDS_V1 = [
  "head", "index", "refs", "config", "objects", "packedRefs", "entrySet",
] as const;

function sourceDigestSet(value: unknown, name: string): Record<(typeof SOURCE_DIGEST_FIELDS_V1)[number], string> {
  const object = requireObject(value, name);
  exactKeys(object, SOURCE_DIGEST_FIELDS_V1);
  for (const field of SOURCE_DIGEST_FIELDS_V1) {
    if (typeof object[field] !== "string" || !/^[0-9a-f]{64}$/.test(object[field])) {
      fail(`${name} ${field} must be a lowercase SHA-256 digest`);
    }
  }
  return object as Record<(typeof SOURCE_DIGEST_FIELDS_V1)[number], string>;
}

function requireGitOid(value: unknown, objectFormat: "sha1" | "sha256", name: string): string {
  const length = objectFormat === "sha1" ? 40 : 64;
  if (typeof value !== "string" || value.length !== length || !/^[0-9a-f]+$/.test(value)) {
    fail(`${name} length must match ${objectFormat}`);
  }
  return value;
}

function requireAbsolutePath(value: unknown, name: string): string {
  if (
    typeof value !== "string" ||
    (!value.startsWith("/") && !/^[A-Z]:\//.test(value)) ||
    value === "/" ||
    value.includes("\\")
  ) {
    fail(`${name} must be a canonical absolute path`);
  }
  const windows = /^[A-Z]:\//.test(value);
  if (
    value.endsWith("/") ||
    value.split("/").some((segment, index) =>
      index > 0 && segment === "" ||
      segment === "." ||
      segment === ".." ||
      windows && index === 0 && !/^[A-Z]:$/.test(segment),
    )
  ) {
    fail(`${name} must be normalized`);
  }
  return value;
}

function isAtOrBelow(path: string, root: string): boolean {
  return path === root || path.startsWith(`${root}/`);
}

export function fail(detail: string): never {
  throw new Error(detail);
}
