import { MAX_TEXT_CHUNK_BYTES, METHODS_V1 } from "./protocol-v1.ts";

export const PROJECT_SCHEMA_VERSION_V1 = 1 as const;
export const TREE_PAGE_MAX_ENTRIES_V1 = 500 as const;
export const FILE_MAX_BYTES_V1 = 5 * 1024 * 1024;
export const FILE_VERSION_ALGORITHM_V1 = "sha256" as const;
export const FILE_VERSION_PREFIX_V1 = "sha256:" as const;
export const FILE_VERSION_DIGEST_INPUT_V1 = "exactFileBytes" as const;
export const WORKTREE_HEAD_MODE_V1 = "detached" as const;
export const WORKTREE_LIFECYCLES_V1 = [
  "active",
  "abandoned",
  "recoveryRequired",
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
  baseCommit: string;
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
  if (object.schemaVersion !== PROJECT_SCHEMA_VERSION_V1) fail("project schemaVersion must be 1");
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
    "baseCommit",
    "headMode",
    "lifecycle",
  ]);
  if (object.schemaVersion !== PROJECT_SCHEMA_VERSION_V1) fail("manifest schemaVersion must be 1");
  requireId(object.projectId, "projectId");
  requireId(object.worktreeId, "worktreeId");
  const root = requireAbsolutePath(object.canonicalGitRoot, "canonicalGitRoot");
  const worktree = requireAbsolutePath(object.worktreePath, "worktreePath");
  if (root === worktree || isAtOrBelow(worktree, root)) {
    fail("isolated worktreePath cannot be the original Git root or inside it");
  }
  if (typeof object.baseCommit !== "string" || !/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(object.baseCommit)) {
    fail("baseCommit must be a full lowercase Git object id");
  }
  if (object.headMode !== WORKTREE_HEAD_MODE_V1) fail("worktree must use detached HEAD");
  if (!WORKTREE_LIFECYCLES_V1.includes(object.lifecycle as never)) fail("unknown worktree lifecycle");
  return object as unknown as WorktreeManifestV1;
}

export function validateTreePageV1(value: unknown): TreePageV1 {
  const object = requireObject(value, "tree page");
  exactKeys(object, ["projectId", "parentPath", "entries", "nextCursor"]);
  requireId(object.projectId, "projectId");
  const parentPath = requireRelativePath(object.parentPath, "parentPath", true);
  if (!Array.isArray(object.entries) || object.entries.length > TREE_PAGE_MAX_ENTRIES_V1) {
    fail("tree page exceeds 500 entries");
  }
  let previous = "";
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
    if (previous !== "" && path <= previous) fail("tree entries must be sorted and unique");
    previous = path;
  }
  if (object.nextCursor !== null) requireId(object.nextCursor, "opaque nextCursor");
  return object as unknown as TreePageV1;
}

export function validateProjectCloseResultV1(value: unknown): ProjectCloseResultV1 {
  const object = requireObject(value, "project close result");
  exactKeys(object, ["projectId", "lifecycle", "physicallyDeleted"]);
  requireId(object.projectId, "projectId");
  if (object.lifecycle !== "abandoned" || object.physicallyDeleted !== false) {
    fail("project.close must abandon without physically deleting the worktree");
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
