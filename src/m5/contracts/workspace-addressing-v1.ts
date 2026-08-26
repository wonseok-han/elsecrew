import {
  MAX_TEXT_CHUNK_BYTES,
  PROTOCOL_VERSION,
  encodeJsonl,
  type EventV1,
} from "./protocol-v1.ts";
import {
  exactKeys,
  fail,
  requireObject,
  requireRelativePath,
  requireSafeUnsigned,
  validateG4SafetyErrorV1,
  validateProjectCloseResultV1,
  validateTreePageV1,
  validateWatchEventV1,
  validateWorktreeManifestV1,
  type G4SafetyErrorV1,
  type TreeEntryV1,
  type WorktreeManifestV1,
} from "./project-v1.ts";
import {
  validateFileReadDescriptorV1,
  validateFileVersionV1,
  validateSaveTranscriptV1,
  type FileClassificationV1,
  type FileVersionV1,
  type SaveChunkV1,
  type SaveCommitV1,
  type TextStreamDescriptorV1,
} from "./file-v1.ts";
import {
  validateDiffFileDescriptorV1,
  validateDiffSummaryV1,
  type DiffChangeKindV1,
  type DiffEntryV1,
} from "./diff-v1.ts";
import {
  G5_ID_MAX_BYTES_V1,
  validateChatRecordV1,
  type ChatRecordV1,
} from "./run-recovery-v1.ts";

export const WORKSPACE_CAPABILITIES_V1 = [
  "workspace.getTree",
  "workspace.close",
  "workspace.file.read",
  "workspace.file.save",
  "workspace.file.watch",
  "workspace.diff.getSummary",
  "workspace.diff.getFile",
] as const;
export const WORKSPACE_EVENTS_V1 = ["workspace.changed", "workspace.blob.chunk"] as const;

export interface WorkspaceRefV1 {
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
}
export interface WorkspaceTreeRequestV1 extends WorkspaceRefV1 { parentPath: string; cursor: string | null }
export interface WorkspaceTreePageV1 extends WorkspaceRefV1 {
  parentPath: string; entries: TreeEntryV1[]; nextCursor: string | null;
}
export interface WorkspaceCloseResultV1 extends WorkspaceRefV1 {
  lifecycle: "abandoned"; physicallyDeleted: false;
}
export interface WorkspaceFileReadRequestV1 extends WorkspaceRefV1 { path: string }
export interface WorkspaceFileReadDescriptorV1 extends WorkspaceRefV1 {
  path: string; classification: FileClassificationV1; byteLength: number;
  version: FileVersionV1; stream: TextStreamDescriptorV1 | null;
}
export interface WorkspaceSaveBeginRequestV1 extends WorkspaceRefV1 {
  path: string; expectedVersion: FileVersionV1; totalBytes: number; encoding: "utf8";
}
export interface WorkspaceSaveBeginResultV1 extends WorkspaceRefV1 {
  saveId: string; chunkBytes: typeof MAX_TEXT_CHUNK_BYTES;
}
export interface WorkspaceSaveChunkExchangeV1 {
  request: SaveChunkV1;
  response: { saveId: string; acceptedSeq: number };
}
export type WorkspaceSaveCommitResultV1 =
  | (WorkspaceRefV1 & { saveId: string; outcome: "committed"; version: FileVersionV1 })
  | (WorkspaceRefV1 & { saveId: string; outcome: "dirtyConflict"; error: G4SafetyErrorV1 });
export interface WorkspaceSaveTranscriptV1 {
  beginRequest: WorkspaceSaveBeginRequestV1;
  beginResult: WorkspaceSaveBeginResultV1;
  chunks: WorkspaceSaveChunkExchangeV1[];
  commitRequest: SaveCommitV1;
  commitResult: WorkspaceSaveCommitResultV1;
  currentVersionBefore: FileVersionV1;
}
export interface WorkspaceWatchRequestV1 extends WorkspaceRefV1 {}
export interface WorkspaceWatchResultV1 extends WorkspaceRefV1 { watching: true }
export interface WorkspaceDiffSummaryV1 extends WorkspaceRefV1 {
  baseCommit: string; entries: DiffEntryV1[];
}
export interface WorkspaceDiffFileRequestV1 extends WorkspaceRefV1 { path: string }
export interface WorkspaceDiffFileDescriptorV1 extends WorkspaceRefV1 {
  kind: DiffChangeKindV1; path: string; oldPath: string | null;
  baseVersion: FileVersionV1 | null; worktreeVersion: FileVersionV1 | null;
  byteLength: number; stream: TextStreamDescriptorV1 | null;
}
export interface WorkspaceChangedEventV1 extends WorkspaceRefV1 {
  protocolVersion: typeof PROTOCOL_VERSION; kind: "event"; sidecarInstanceId: string;
  streamId: string; seq: number; event: "workspace.changed";
  payload: { event: "file.changed" | "file.conflict" | "diff.changed"; paths: string[]; rescanRequired: boolean };
}
export interface WorkspaceBlobChunkEventV1 extends WorkspaceRefV1 {
  protocolVersion: typeof PROTOCOL_VERSION; kind: "event"; sidecarInstanceId: string;
  streamId: string; seq: number; event: "workspace.blob.chunk"; payload: { text: string };
}
export type WorkspaceEventV1 = WorkspaceChangedEventV1 | WorkspaceBlobChunkEventV1;
export interface WorkspaceChatBindingTranscriptV1 {
  chat: ChatRecordV1;
  verifiedSnapshot: WorktreeManifestV1;
  workspace: WorkspaceRefV1;
}
export interface WorkspaceEventRouteTranscriptV1 {
  chat: ChatRecordV1;
  event: WorkspaceEventV1;
}
export interface WorkspaceOperationTranscriptV1 {
  operation: "getTree" | "close" | "fileRead" | "fileWatch" | "diffSummary" | "diffFile";
  request: unknown;
  response: unknown;
}

export function validateWorkspaceRefV1(value: unknown): WorkspaceRefV1 {
  const object = requireObject(value, "workspace reference");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity"]);
  for (const field of ["projectId", "worktreeId", "workspaceIdentity"] as const) requireWorkspaceId(object[field], field);
  return object as unknown as WorkspaceRefV1;
}

export function validateWorkspaceTreeRequestV1(value: unknown): WorkspaceTreeRequestV1 {
  const object = requireObject(value, "workspace tree request");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "parentPath", "cursor"]);
  validateWorkspaceRefFields(object);
  requireRelativePath(object.parentPath, "parentPath", true);
  if (object.cursor !== null) requireWorkspaceId(object.cursor, "cursor");
  return object as unknown as WorkspaceTreeRequestV1;
}

export function validateWorkspaceTreePageV1(value: unknown): WorkspaceTreePageV1 {
  const object = requireObject(value, "workspace tree page");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "parentPath", "entries", "nextCursor"]);
  validateWorkspaceRefFields(object);
  validateTreePageV1({ projectId: object.projectId, parentPath: object.parentPath, entries: object.entries, nextCursor: object.nextCursor });
  return object as unknown as WorkspaceTreePageV1;
}

export function validateWorkspaceCloseResultV1(value: unknown): WorkspaceCloseResultV1 {
  const object = requireObject(value, "workspace close result");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "lifecycle", "physicallyDeleted"]);
  validateWorkspaceRefFields(object);
  validateProjectCloseResultV1({ projectId: object.projectId, lifecycle: object.lifecycle, physicallyDeleted: object.physicallyDeleted });
  return object as unknown as WorkspaceCloseResultV1;
}

export function validateWorkspaceFileReadRequestV1(value: unknown): WorkspaceFileReadRequestV1 {
  const object = requireObject(value, "workspace file read request");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "path"]);
  validateWorkspaceRefFields(object); requireRelativePath(object.path, "file path", false);
  return object as unknown as WorkspaceFileReadRequestV1;
}

export function validateWorkspaceFileReadDescriptorV1(value: unknown): WorkspaceFileReadDescriptorV1 {
  const object = requireObject(value, "workspace file read descriptor");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "path", "classification", "byteLength", "version", "stream"]);
  validateWorkspaceRefFields(object);
  validateFileReadDescriptorV1({ projectId: object.projectId, path: object.path, classification: object.classification, byteLength: object.byteLength, version: object.version, stream: object.stream });
  return object as unknown as WorkspaceFileReadDescriptorV1;
}

export function validateWorkspaceSaveTranscriptV1(value: unknown): WorkspaceSaveTranscriptV1 {
  const object = requireObject(value, "workspace save transcript");
  exactKeys(object, ["beginRequest", "beginResult", "chunks", "commitRequest", "commitResult", "currentVersionBefore"]);
  const beginRequest = requireObject(object.beginRequest, "workspace save begin request");
  exactKeys(beginRequest, ["projectId", "worktreeId", "workspaceIdentity", "path", "expectedVersion", "totalBytes", "encoding"]);
  const workspace = validateWorkspaceRefFields(beginRequest);
  requireRelativePath(beginRequest.path, "save path", false); validateFileVersionV1(beginRequest.expectedVersion);
  const totalBytes = requireSafeUnsigned(beginRequest.totalBytes, "save totalBytes");
  if (beginRequest.encoding !== "utf8") fail("workspace save encoding must be utf8");
  const beginResult = requireObject(object.beginResult, "workspace save begin result");
  exactKeys(beginResult, ["projectId", "worktreeId", "workspaceIdentity", "saveId", "chunkBytes"]);
  assertWorkspaceEcho(workspace, validateWorkspaceRefFields(beginResult), "save begin result");
  requireWorkspaceId(beginResult.saveId, "saveId");
  if (beginResult.chunkBytes !== MAX_TEXT_CHUNK_BYTES) fail("workspace save chunkBytes must be 256 KiB");
  if (!Array.isArray(object.chunks)) fail("workspace save chunks must be an array");
  const chunkRequests: SaveChunkV1[] = [];
  for (const [index, exchangeValue] of object.chunks.entries()) {
    const exchange = requireObject(exchangeValue, "workspace save chunk exchange");
    exactKeys(exchange, ["request", "response"]);
    const request = requireObject(exchange.request, "workspace save chunk request");
    exactKeys(request, ["saveId", "seq", "text"]);
    const response = requireObject(exchange.response, "workspace save chunk response");
    exactKeys(response, ["saveId", "acceptedSeq"]);
    if (request.saveId !== beginResult.saveId || response.saveId !== beginResult.saveId
      || request.seq !== index + 1 || response.acceptedSeq !== request.seq) fail("saveId/sequence must stay bound after begin");
    chunkRequests.push(request as unknown as SaveChunkV1);
  }
  const commitRequest = requireObject(object.commitRequest, "workspace save commit request");
  exactKeys(commitRequest, ["saveId", "expectedVersion", "lastSeq", "totalBytes"]);
  if (commitRequest.saveId !== beginResult.saveId) fail("workspace save commit saveId mismatch");
  const commitResult = requireObject(object.commitResult, "workspace save commit result");
  const outcome = commitResult.outcome;
  exactKeys(commitResult, outcome === "committed"
    ? ["projectId", "worktreeId", "workspaceIdentity", "saveId", "outcome", "version"]
    : ["projectId", "worktreeId", "workspaceIdentity", "saveId", "outcome", "error"]);
  assertWorkspaceEcho(workspace, validateWorkspaceRefFields(commitResult), "save commit result");
  if (commitResult.saveId !== beginResult.saveId) fail("workspace save result saveId mismatch");
  if (outcome === "committed") validateFileVersionV1(commitResult.version);
  else if (outcome === "dirtyConflict") validateG4SafetyErrorV1(commitResult.error);
  else fail("unknown workspace save outcome");
  const currentVersionBefore = validateFileVersionV1(object.currentVersionBefore);
  validateSaveTranscriptV1({
    begin: { saveId: beginResult.saveId, projectId: workspace.projectId, path: beginRequest.path, expectedVersion: beginRequest.expectedVersion, totalBytes, encoding: beginRequest.encoding, chunkBytes: beginResult.chunkBytes },
    chunks: chunkRequests, commit: commitRequest, currentVersion: currentVersionBefore, outcome,
  });
  return object as unknown as WorkspaceSaveTranscriptV1;
}

export function validateWorkspaceWatchTranscriptV1(value: unknown): { request: WorkspaceRefV1; response: WorkspaceWatchResultV1 } {
  const object = requireObject(value, "workspace watch transcript"); exactKeys(object, ["request", "response"]);
  const request = validateWorkspaceRefV1(object.request);
  const responseObject = requireObject(object.response, "workspace watch response");
  exactKeys(responseObject, ["projectId", "worktreeId", "workspaceIdentity", "watching"]);
  const response = validateWorkspaceRefFields(responseObject); assertWorkspaceEcho(request, response, "watch response");
  if (responseObject.watching !== true) fail("workspace watch response must confirm watching");
  return { request, response: responseObject as unknown as WorkspaceWatchResultV1 };
}

export function validateWorkspaceDiffSummaryV1(value: unknown): WorkspaceDiffSummaryV1 {
  const object = requireObject(value, "workspace diff summary");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "baseCommit", "entries"]);
  validateWorkspaceRefFields(object); validateDiffSummaryV1({ projectId: object.projectId, baseCommit: object.baseCommit, entries: object.entries });
  return object as unknown as WorkspaceDiffSummaryV1;
}

export function validateWorkspaceDiffFileRequestV1(value: unknown): WorkspaceDiffFileRequestV1 {
  const object = requireObject(value, "workspace diff file request");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "path"]);
  validateWorkspaceRefFields(object); requireRelativePath(object.path, "diff path", false);
  return object as unknown as WorkspaceDiffFileRequestV1;
}

export function validateWorkspaceDiffFileDescriptorV1(value: unknown): WorkspaceDiffFileDescriptorV1 {
  const object = requireObject(value, "workspace diff file descriptor");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "kind", "path", "oldPath", "baseVersion", "worktreeVersion", "byteLength", "stream"]);
  validateWorkspaceRefFields(object);
  validateDiffFileDescriptorV1({ projectId: object.projectId, kind: object.kind, path: object.path, oldPath: object.oldPath, baseVersion: object.baseVersion, worktreeVersion: object.worktreeVersion, byteLength: object.byteLength, stream: object.stream });
  return object as unknown as WorkspaceDiffFileDescriptorV1;
}

export function validateWorkspaceEventV1(value: unknown): WorkspaceEventV1 {
  const object = requireObject(value, "workspace event");
  exactKeys(object, ["protocolVersion", "kind", "sidecarInstanceId", "streamId", "seq", "event", "projectId", "worktreeId", "workspaceIdentity", "payload"]);
  validateWorkspaceRefFields(object); encodeJsonl(object as unknown as EventV1);
  if (object.event === "workspace.changed") {
    const payload = requireObject(object.payload, "workspace changed payload");
    exactKeys(payload, ["event", "paths", "rescanRequired"]);
    validateWatchEventV1({ projectId: object.projectId, event: payload.event, paths: payload.paths, rescanRequired: payload.rescanRequired });
  } else if (object.event !== "workspace.blob.chunk") fail("unknown workspace event");
  return object as unknown as WorkspaceEventV1;
}

export function validateWorkspaceChatBindingTranscriptV1(value: unknown): WorkspaceChatBindingTranscriptV1 {
  const object = requireObject(value, "workspace chat binding transcript");
  exactKeys(object, ["chat", "verifiedSnapshot", "workspace"]);
  const chat = validateChatRecordV1(object.chat);
  const snapshot = validateWorktreeManifestV1(object.verifiedSnapshot);
  const workspace = validateWorkspaceRefV1(object.workspace);
  if (snapshot.lifecycle !== "active") fail("chat workspace addressing requires an active snapshot");
  assertWorkspaceEcho(chat, snapshot, "chat verified snapshot");
  assertWorkspaceEcho(chat, workspace, "chat workspace request");
  return { chat, verifiedSnapshot: snapshot, workspace };
}

export function validateWorkspaceEventRouteTranscriptV1(value: unknown): WorkspaceEventRouteTranscriptV1 {
  const object = requireObject(value, "workspace event route transcript");
  exactKeys(object, ["chat", "event"]);
  const chat = validateChatRecordV1(object.chat);
  const event = validateWorkspaceEventV1(object.event);
  assertWorkspaceEcho(chat, event, "workspace event route");
  return { chat, event };
}

export function validateWorkspaceEchoTranscriptV1(value: unknown): { request: WorkspaceRefV1; response: WorkspaceRefV1 } {
  const object = requireObject(value, "workspace echo transcript"); exactKeys(object, ["request", "response"]);
  const request = validateWorkspaceRefV1(object.request); const response = validateWorkspaceRefV1(object.response);
  assertWorkspaceEcho(request, response, "workspace response"); return { request, response };
}

export function validateWorkspaceOperationTranscriptV1(value: unknown): WorkspaceOperationTranscriptV1 {
  const object = requireObject(value, "workspace operation transcript");
  exactKeys(object, ["operation", "request", "response"]);
  let request: WorkspaceRefV1;
  let response: WorkspaceRefV1;
  switch (object.operation) {
    case "getTree": {
      const treeRequest = validateWorkspaceTreeRequestV1(object.request);
      const treeResponse = validateWorkspaceTreePageV1(object.response);
      if (treeRequest.parentPath !== treeResponse.parentPath) fail("tree response parentPath must match the request");
      request = treeRequest; response = treeResponse; break;
    }
    case "close": request = validateWorkspaceRefV1(object.request); response = validateWorkspaceCloseResultV1(object.response); break;
    case "fileRead": {
      const fileRequest = validateWorkspaceFileReadRequestV1(object.request);
      const fileResponse = validateWorkspaceFileReadDescriptorV1(object.response);
      if (fileRequest.path !== fileResponse.path) fail("file read response path must match the request");
      request = fileRequest; response = fileResponse; break;
    }
    case "fileWatch": {
      const transcript = validateWorkspaceWatchTranscriptV1({ request: object.request, response: object.response });
      request = transcript.request; response = transcript.response; break;
    }
    case "diffSummary": request = validateWorkspaceRefV1(object.request); response = validateWorkspaceDiffSummaryV1(object.response); break;
    case "diffFile": {
      const diffRequest = validateWorkspaceDiffFileRequestV1(object.request);
      const diffResponse = validateWorkspaceDiffFileDescriptorV1(object.response);
      if (diffRequest.path !== diffResponse.path) fail("diff file response path must match the request");
      request = diffRequest; response = diffResponse; break;
    }
    default: fail("unknown workspace operation");
  }
  assertWorkspaceEcho(request, response, `${String(object.operation)} response`);
  return object as unknown as WorkspaceOperationTranscriptV1;
}

function validateWorkspaceRefFields(object: Record<string, unknown>): WorkspaceRefV1 {
  for (const field of ["projectId", "worktreeId", "workspaceIdentity"] as const) requireWorkspaceId(object[field], field);
  return { projectId: object.projectId as string, worktreeId: object.worktreeId as string, workspaceIdentity: object.workspaceIdentity as string };
}
function assertWorkspaceEcho(expected: WorkspaceRefV1, actual: WorkspaceRefV1, name: string): void {
  if (expected.projectId !== actual.projectId || expected.worktreeId !== actual.worktreeId
    || expected.workspaceIdentity !== actual.workspaceIdentity) fail(`${name} must exactly echo projectId/worktreeId/workspaceIdentity`);
}
function requireWorkspaceId(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || value.length === 0
    || new TextEncoder().encode(value).length > G5_ID_MAX_BYTES_V1) {
    fail(`${name} is required and must fit the UTF-8 byte limit`);
  }
}
