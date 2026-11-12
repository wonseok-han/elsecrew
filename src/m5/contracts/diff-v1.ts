import { MAX_TEXT_CHUNK_BYTES } from "./protocol-v1.ts";
import { exactKeys, fail, requireId, requireObject, requireRelativePath } from "./project-v1.ts";
import { validateFileVersionV1, type FileVersionV1, type TextStreamDescriptorV1 } from "./file-v1.ts";

export type DiffChangeKindV1 = "added" | "modified" | "deleted" | "renamed" | "binary";

export interface DiffEntryV1 {
  kind: DiffChangeKindV1;
  path: string;
  oldPath?: string;
  binary: boolean;
}

export interface DiffSummaryV1 {
  projectId: string;
  baseCommit: string;
  entries: DiffEntryV1[];
}

export interface DiffFileDescriptorV1 {
  projectId: string;
  kind: DiffChangeKindV1;
  path: string;
  oldPath: string | null;
  baseVersion: FileVersionV1 | null;
  worktreeVersion: FileVersionV1 | null;
  byteLength: number;
  stream: TextStreamDescriptorV1 | null;
}

export function validateDiffSummaryV1(value: unknown): DiffSummaryV1 {
  const object = requireObject(value, "diff summary");
  exactKeys(object, ["projectId", "baseCommit", "entries"]);
  requireId(object.projectId, "projectId");
  if (typeof object.baseCommit !== "string" || !/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(object.baseCommit)) {
    fail("diff baseCommit must be a full lowercase Git object id");
  }
  if (!Array.isArray(object.entries)) fail("diff entries must be an array");
  for (const value of object.entries) validateDiffEntry(value);
  return object as unknown as DiffSummaryV1;
}

export function validateDiffFileDescriptorV1(value: unknown): DiffFileDescriptorV1 {
  const object = requireObject(value, "diff file descriptor");
  exactKeys(object, ["projectId", "kind", "path", "oldPath", "baseVersion", "worktreeVersion", "byteLength", "stream"]);
  requireId(object.projectId, "projectId");
  requireRelativePath(object.path, "diff path", false);
  const kind = requireKind(object.kind);
  if (kind === "renamed") {
    requireRelativePath(object.oldPath, "renamed oldPath", false);
    if (object.oldPath === object.path) fail("rename oldPath and path must differ");
  } else if (object.oldPath !== null) {
    fail("only renamed diff may carry oldPath");
  }
  if (object.baseVersion !== null) validateFileVersionV1(object.baseVersion);
  if (object.worktreeVersion !== null) validateFileVersionV1(object.worktreeVersion);
  if (typeof object.byteLength !== "number" || !Number.isSafeInteger(object.byteLength) || object.byteLength < 0) {
    fail("diff byteLength must be a non-negative safe integer");
  }
  if (kind === "added" && object.baseVersion !== null) fail("added diff cannot have baseVersion");
  if (kind === "deleted" && object.worktreeVersion !== null) fail("deleted diff cannot have worktreeVersion");
  if (kind === "binary") {
    if (object.stream !== null) fail("binary diff cannot expose a text stream");
  } else {
    const stream = requireObject(object.stream, "diff text stream");
    exactKeys(stream, ["streamId", "chunkBytes"]);
    requireId(stream.streamId, "streamId");
    if (stream.chunkBytes !== MAX_TEXT_CHUNK_BYTES) fail("diff stream chunkBytes must be 256 KiB");
  }
  return object as unknown as DiffFileDescriptorV1;
}

function validateDiffEntry(value: unknown): void {
  const object = requireObject(value, "diff entry");
  const kind = requireKind(object.kind);
  if (kind === "renamed") {
    exactKeys(object, ["kind", "path", "oldPath", "binary"]);
    requireRelativePath(object.oldPath, "rename oldPath", false);
    if (object.oldPath === object.path) fail("rename paths must differ");
  } else {
    exactKeys(object, ["kind", "path", "binary"]);
  }
  requireRelativePath(object.path, "diff entry path", false);
  if (typeof object.binary !== "boolean" || object.binary !== (kind === "binary")) {
    fail("diff binary flag must match binary kind");
  }
}

function requireKind(value: unknown): DiffChangeKindV1 {
  if (!["added", "modified", "deleted", "renamed", "binary"].includes(String(value))) {
    fail("unknown diff change kind");
  }
  return value as DiffChangeKindV1;
}
