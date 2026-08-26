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
const WORKSPACE_REF_FIELDS_V1 = ["projectId", "worktreeId", "workspaceIdentity"] as const;
const CHAT_WORKSPACE_REF_FIELDS_V1 = ["chatId", ...WORKSPACE_REF_FIELDS_V1] as const;

export interface WorkspaceRefV1 {
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
}
export interface ChatWorkspaceRefV1 extends WorkspaceRefV1 { chatId: string }
export interface WorkspaceTreeRequestV1 extends ChatWorkspaceRefV1 { parentPath: string; cursor: string | null }
export interface WorkspaceTreePageV1 extends ChatWorkspaceRefV1 {
  parentPath: string; entries: TreeEntryV1[]; nextCursor: string | null;
}
export interface WorkspaceCloseResultV1 extends ChatWorkspaceRefV1 {
  lifecycle: "abandoned"; physicallyDeleted: false;
}
export interface WorkspaceFileReadRequestV1 extends ChatWorkspaceRefV1 { path: string }
export interface WorkspaceFileReadDescriptorV1 extends ChatWorkspaceRefV1 {
  path: string; classification: FileClassificationV1; byteLength: number;
  version: FileVersionV1; stream: TextStreamDescriptorV1 | null;
}
export interface WorkspaceSaveBeginRequestV1 extends ChatWorkspaceRefV1 {
  path: string; expectedVersion: FileVersionV1; totalBytes: number; encoding: "utf8";
}
export interface WorkspaceSaveBeginResultV1 extends ChatWorkspaceRefV1 {
  saveId: string; chunkBytes: typeof MAX_TEXT_CHUNK_BYTES;
}
export interface WorkspaceSaveSessionV1 extends ChatWorkspaceRefV1 {
  saveId: string; path: string; expectedVersion: FileVersionV1; totalBytes: number;
  encoding: "utf8"; chunkBytes: typeof MAX_TEXT_CHUNK_BYTES;
}
export interface WorkspaceSaveChunkRequestV1 extends ChatWorkspaceRefV1, SaveChunkV1 {}
export interface WorkspaceSaveChunkExchangeV1 {
  request: WorkspaceSaveChunkRequestV1;
  response: ChatWorkspaceRefV1 & { saveId: string; acceptedSeq: number };
}
export interface WorkspaceSaveCommitRequestV1 extends ChatWorkspaceRefV1, SaveCommitV1 {}
export type WorkspaceSaveCommitResultV1 =
  | (ChatWorkspaceRefV1 & { saveId: string; outcome: "committed"; version: FileVersionV1 })
  | (ChatWorkspaceRefV1 & { saveId: string; outcome: "dirtyConflict"; error: G4SafetyErrorV1 });
export interface WorkspaceSaveTranscriptV1 {
  binding: WorkspaceResolvedBindingV1;
  beginRequest: WorkspaceSaveBeginRequestV1;
  beginResult: WorkspaceSaveBeginResultV1;
  activeSessions: WorkspaceSaveSessionV1[];
  chunks: WorkspaceSaveChunkExchangeV1[];
  commitRequest: WorkspaceSaveCommitRequestV1;
  commitResult: WorkspaceSaveCommitResultV1;
  currentVersionBefore: FileVersionV1;
}
export interface WorkspaceWatchResultV1 extends ChatWorkspaceRefV1 { watching: true }
export interface WorkspaceDiffSummaryV1 extends ChatWorkspaceRefV1 {
  baseCommit: string; entries: DiffEntryV1[];
}
export interface WorkspaceDiffFileRequestV1 extends ChatWorkspaceRefV1 { path: string }
export interface WorkspaceDiffFileDescriptorV1 extends ChatWorkspaceRefV1 {
  kind: DiffChangeKindV1; path: string; oldPath: string | null;
  baseVersion: FileVersionV1 | null; worktreeVersion: FileVersionV1 | null;
  byteLength: number; stream: TextStreamDescriptorV1 | null;
}
export interface WorkspaceChangedEventV1 extends ChatWorkspaceRefV1 {
  protocolVersion: typeof PROTOCOL_VERSION; kind: "event"; sidecarInstanceId: string;
  streamId: string; seq: number; event: "workspace.changed";
  payload: { event: "file.changed" | "file.conflict" | "diff.changed"; paths: string[]; rescanRequired: boolean };
}
export interface WorkspaceBlobChunkEventV1 extends ChatWorkspaceRefV1 {
  protocolVersion: typeof PROTOCOL_VERSION; kind: "event"; sidecarInstanceId: string;
  streamId: string; seq: number; event: "workspace.blob.chunk"; payload: { text: string };
}
export type WorkspaceEventV1 = WorkspaceChangedEventV1 | WorkspaceBlobChunkEventV1;
export interface WorkspaceChatBindingTranscriptV1 {
  chat: ChatRecordV1;
  verifiedSnapshot: WorktreeManifestV1;
  workspace: ChatWorkspaceRefV1;
}
export interface WorkspaceResolvedBindingV1 {
  storedBinding: ChatWorkspaceRefV1;
  verifiedSnapshot: WorktreeManifestV1;
}
export interface WorkspaceEventRouteTranscriptV1 {
  binding: WorkspaceResolvedBindingV1;
  event: WorkspaceEventV1;
}
export interface WorkspaceOperationTranscriptV1 {
  binding: WorkspaceResolvedBindingV1;
  operation: "getTree" | "close" | "fileRead" | "fileWatch" | "diffSummary" | "diffFile";
  request: unknown;
  response: unknown;
}

export function validateWorkspaceRefV1(value: unknown): WorkspaceRefV1 {
  const object = requireObject(value, "workspace reference");
  exactKeys(object, WORKSPACE_REF_FIELDS_V1);
  validateWorkspaceRefFields(object);
  return object as unknown as WorkspaceRefV1;
}

export function validateChatWorkspaceRefV1(value: unknown): ChatWorkspaceRefV1 {
  const object = requireObject(value, "chat workspace reference");
  exactKeys(object, CHAT_WORKSPACE_REF_FIELDS_V1);
  return validateChatWorkspaceRefFields(object);
}

export function validateWorkspaceTreeRequestV1(value: unknown): WorkspaceTreeRequestV1 {
  const object = requireObject(value, "workspace tree request");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "parentPath", "cursor"]);
  validateChatWorkspaceRefFields(object);
  requireRelativePath(object.parentPath, "parentPath", true);
  if (object.cursor !== null) requireWorkspaceId(object.cursor, "cursor");
  return object as unknown as WorkspaceTreeRequestV1;
}

export function validateWorkspaceTreePageV1(value: unknown): WorkspaceTreePageV1 {
  const object = requireObject(value, "workspace tree page");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "parentPath", "entries", "nextCursor"]);
  validateChatWorkspaceRefFields(object);
  validateTreePageV1({ projectId: object.projectId, parentPath: object.parentPath, entries: object.entries, nextCursor: object.nextCursor });
  return object as unknown as WorkspaceTreePageV1;
}

export function validateWorkspaceCloseResultV1(value: unknown): WorkspaceCloseResultV1 {
  const object = requireObject(value, "workspace close result");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "lifecycle", "physicallyDeleted"]);
  validateChatWorkspaceRefFields(object);
  validateProjectCloseResultV1({ projectId: object.projectId, lifecycle: object.lifecycle, physicallyDeleted: object.physicallyDeleted });
  return object as unknown as WorkspaceCloseResultV1;
}

export function validateWorkspaceFileReadRequestV1(value: unknown): WorkspaceFileReadRequestV1 {
  const object = requireObject(value, "workspace file read request");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "path"]);
  validateChatWorkspaceRefFields(object); requireRelativePath(object.path, "file path", false);
  return object as unknown as WorkspaceFileReadRequestV1;
}

export function validateWorkspaceFileReadDescriptorV1(value: unknown): WorkspaceFileReadDescriptorV1 {
  const object = requireObject(value, "workspace file read descriptor");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "path", "classification", "byteLength", "version", "stream"]);
  validateChatWorkspaceRefFields(object);
  validateFileReadDescriptorV1({ projectId: object.projectId, path: object.path, classification: object.classification, byteLength: object.byteLength, version: object.version, stream: object.stream });
  return object as unknown as WorkspaceFileReadDescriptorV1;
}

export function validateWorkspaceSaveTranscriptV1(value: unknown): WorkspaceSaveTranscriptV1 {
  const object = requireObject(value, "workspace save transcript");
  exactKeys(object, ["binding", "beginRequest", "beginResult", "activeSessions", "chunks", "commitRequest", "commitResult", "currentVersionBefore"]);
  const binding = validateWorkspaceResolvedBindingV1(object.binding);
  const beginRequest = requireObject(object.beginRequest, "workspace save begin request");
  exactKeys(beginRequest, [...CHAT_WORKSPACE_REF_FIELDS_V1, "path", "expectedVersion", "totalBytes", "encoding"]);
  const workspace = validateChatWorkspaceRefFields(beginRequest);
  assertChatWorkspaceEcho(binding.storedBinding, workspace, "save binding");
  requireRelativePath(beginRequest.path, "save path", false); validateFileVersionV1(beginRequest.expectedVersion);
  const totalBytes = requireSafeUnsigned(beginRequest.totalBytes, "save totalBytes");
  if (beginRequest.encoding !== "utf8") fail("workspace save encoding must be utf8");
  const beginResult = requireObject(object.beginResult, "workspace save begin result");
  exactKeys(beginResult, [...CHAT_WORKSPACE_REF_FIELDS_V1, "saveId", "chunkBytes"]);
  assertChatWorkspaceEcho(workspace, validateChatWorkspaceRefFields(beginResult), "save begin result");
  requireWorkspaceId(beginResult.saveId, "saveId");
  if (beginResult.chunkBytes !== MAX_TEXT_CHUNK_BYTES) fail("workspace save chunkBytes must be 256 KiB");
  if (!Array.isArray(object.activeSessions) || object.activeSessions.length === 0 || object.activeSessions.length > 100) {
    fail("workspace save activeSessions must be a nonempty bounded array");
  }
  const sessions = object.activeSessions.map(validateWorkspaceSaveSessionV1);
  if (new Set(sessions.map((session) => session.saveId)).size !== sessions.length) fail("workspace save sessions require unique saveId values");
  const session = sessions.find((candidate) => candidate.saveId === beginResult.saveId);
  if (session === undefined) fail("workspace save begin result must have a persisted session");
  assertChatWorkspaceEcho(workspace, session, "persisted save session");
  if (session.path !== beginRequest.path || session.expectedVersion !== beginRequest.expectedVersion
    || session.totalBytes !== totalBytes || session.encoding !== beginRequest.encoding
    || session.chunkBytes !== beginResult.chunkBytes) fail("persisted save session must exactly match begin");
  if (!Array.isArray(object.chunks)) fail("workspace save chunks must be an array");
  const chunkRequests: SaveChunkV1[] = [];
  for (const [index, exchangeValue] of object.chunks.entries()) {
    const exchange = requireObject(exchangeValue, "workspace save chunk exchange");
    exactKeys(exchange, ["request", "response"]);
    const request = requireObject(exchange.request, "workspace save chunk request");
    exactKeys(request, [...CHAT_WORKSPACE_REF_FIELDS_V1, "saveId", "seq", "text"]);
    const requestRef = validateChatWorkspaceRefFields(request);
    const response = requireObject(exchange.response, "workspace save chunk response");
    exactKeys(response, [...CHAT_WORKSPACE_REF_FIELDS_V1, "saveId", "acceptedSeq"]);
    const responseRef = validateChatWorkspaceRefFields(response);
    const requestSession = sessions.find((candidate) => candidate.saveId === request.saveId);
    if (requestSession === undefined) fail("workspace save chunk references an unknown or late save session");
    assertChatWorkspaceEcho(requestSession, requestRef, "save chunk request session");
    assertChatWorkspaceEcho(requestSession, responseRef, "save chunk response session");
    if (request.saveId !== beginResult.saveId || response.saveId !== beginResult.saveId
      || request.seq !== index + 1 || response.acceptedSeq !== request.seq) fail("saveId/sequence must stay bound after begin");
    chunkRequests.push({ saveId: request.saveId, seq: request.seq, text: request.text } as SaveChunkV1);
  }
  const commitRequest = requireObject(object.commitRequest, "workspace save commit request");
  exactKeys(commitRequest, [...CHAT_WORKSPACE_REF_FIELDS_V1, "saveId", "expectedVersion", "lastSeq", "totalBytes"]);
  const commitRequestRef = validateChatWorkspaceRefFields(commitRequest);
  const commitSession = sessions.find((candidate) => candidate.saveId === commitRequest.saveId);
  if (commitSession === undefined) fail("workspace save commit references an unknown or late save session");
  assertChatWorkspaceEcho(commitSession, commitRequestRef, "save commit request session");
  if (commitRequest.saveId !== beginResult.saveId) fail("workspace save commit saveId mismatch");
  const commitResult = requireObject(object.commitResult, "workspace save commit result");
  const outcome = commitResult.outcome;
  exactKeys(commitResult, outcome === "committed"
    ? [...CHAT_WORKSPACE_REF_FIELDS_V1, "saveId", "outcome", "version"]
    : [...CHAT_WORKSPACE_REF_FIELDS_V1, "saveId", "outcome", "error"]);
  assertChatWorkspaceEcho(commitSession, validateChatWorkspaceRefFields(commitResult), "save commit result");
  if (commitResult.saveId !== beginResult.saveId) fail("workspace save result saveId mismatch");
  if (outcome === "committed") validateFileVersionV1(commitResult.version);
  else if (outcome === "dirtyConflict") validateG4SafetyErrorV1(commitResult.error);
  else fail("unknown workspace save outcome");
  const currentVersionBefore = validateFileVersionV1(object.currentVersionBefore);
  validateSaveTranscriptV1({
    begin: { saveId: beginResult.saveId, projectId: workspace.projectId, path: beginRequest.path, expectedVersion: beginRequest.expectedVersion, totalBytes, encoding: beginRequest.encoding, chunkBytes: beginResult.chunkBytes },
    chunks: chunkRequests,
    commit: { saveId: commitRequest.saveId, expectedVersion: commitRequest.expectedVersion, lastSeq: commitRequest.lastSeq, totalBytes: commitRequest.totalBytes },
    currentVersion: currentVersionBefore, outcome,
  });
  return object as unknown as WorkspaceSaveTranscriptV1;
}

export function validateWorkspaceSaveSessionV1(value: unknown): WorkspaceSaveSessionV1 {
  const object = requireObject(value, "workspace save session");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "saveId", "path", "expectedVersion", "totalBytes", "encoding", "chunkBytes"]);
  validateChatWorkspaceRefFields(object); requireWorkspaceId(object.saveId, "saveId");
  requireRelativePath(object.path, "save session path", false); validateFileVersionV1(object.expectedVersion);
  requireSafeUnsigned(object.totalBytes, "save session totalBytes");
  if (object.encoding !== "utf8" || object.chunkBytes !== MAX_TEXT_CHUNK_BYTES) fail("save session must use UTF-8 and 256 KiB chunks");
  return object as unknown as WorkspaceSaveSessionV1;
}

export function validateWorkspaceWatchTranscriptV1(value: unknown): { request: ChatWorkspaceRefV1; response: WorkspaceWatchResultV1 } {
  const object = requireObject(value, "workspace watch transcript"); exactKeys(object, ["request", "response"]);
  const request = validateChatWorkspaceRefV1(object.request);
  const responseObject = requireObject(object.response, "workspace watch response");
  exactKeys(responseObject, [...CHAT_WORKSPACE_REF_FIELDS_V1, "watching"]);
  const response = validateChatWorkspaceRefFields(responseObject); assertChatWorkspaceEcho(request, response, "watch response");
  if (responseObject.watching !== true) fail("workspace watch response must confirm watching");
  return { request, response: responseObject as unknown as WorkspaceWatchResultV1 };
}

export function validateWorkspaceDiffSummaryV1(value: unknown): WorkspaceDiffSummaryV1 {
  const object = requireObject(value, "workspace diff summary");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "baseCommit", "entries"]);
  validateChatWorkspaceRefFields(object); validateDiffSummaryV1({ projectId: object.projectId, baseCommit: object.baseCommit, entries: object.entries });
  return object as unknown as WorkspaceDiffSummaryV1;
}

export function validateWorkspaceDiffFileRequestV1(value: unknown): WorkspaceDiffFileRequestV1 {
  const object = requireObject(value, "workspace diff file request");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "path"]);
  validateChatWorkspaceRefFields(object); requireRelativePath(object.path, "diff path", false);
  return object as unknown as WorkspaceDiffFileRequestV1;
}

export function validateWorkspaceDiffFileDescriptorV1(value: unknown): WorkspaceDiffFileDescriptorV1 {
  const object = requireObject(value, "workspace diff file descriptor");
  exactKeys(object, [...CHAT_WORKSPACE_REF_FIELDS_V1, "kind", "path", "oldPath", "baseVersion", "worktreeVersion", "byteLength", "stream"]);
  validateChatWorkspaceRefFields(object);
  validateDiffFileDescriptorV1({ projectId: object.projectId, kind: object.kind, path: object.path, oldPath: object.oldPath, baseVersion: object.baseVersion, worktreeVersion: object.worktreeVersion, byteLength: object.byteLength, stream: object.stream });
  return object as unknown as WorkspaceDiffFileDescriptorV1;
}

export function validateWorkspaceEventV1(value: unknown): WorkspaceEventV1 {
  const object = requireObject(value, "workspace event");
  exactKeys(object, ["protocolVersion", "kind", "sidecarInstanceId", "streamId", "seq", "event", ...CHAT_WORKSPACE_REF_FIELDS_V1, "payload"]);
  validateChatWorkspaceRefFields(object); encodeJsonl(object as unknown as EventV1);
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
  const workspace = validateChatWorkspaceRefV1(object.workspace);
  if (snapshot.lifecycle !== "active") fail("chat workspace addressing requires an active snapshot");
  assertWorkspaceEcho(chat, snapshot, "chat verified snapshot");
  assertChatWorkspaceEcho(chat, workspace, "chat workspace request");
  return { chat, verifiedSnapshot: snapshot, workspace };
}

export function validateWorkspaceResolvedBindingV1(value: unknown): WorkspaceResolvedBindingV1 {
  const object = requireObject(value, "resolved chat workspace binding");
  exactKeys(object, ["storedBinding", "verifiedSnapshot"]);
  const storedBinding = validateChatWorkspaceRefV1(object.storedBinding);
  const verifiedSnapshot = validateWorktreeManifestV1(object.verifiedSnapshot);
  if (verifiedSnapshot.lifecycle !== "active") fail("workspace operation requires an active snapshot");
  assertWorkspaceEcho(storedBinding, verifiedSnapshot, "resolved active snapshot");
  return { storedBinding, verifiedSnapshot };
}

export function validateWorkspaceEventRouteTranscriptV1(value: unknown): WorkspaceEventRouteTranscriptV1 {
  const object = requireObject(value, "workspace event route transcript");
  exactKeys(object, ["binding", "event"]);
  const binding = validateWorkspaceResolvedBindingV1(object.binding);
  const event = validateWorkspaceEventV1(object.event);
  assertChatWorkspaceEcho(binding.storedBinding, event, "workspace event route");
  return { binding, event };
}

export function validateWorkspaceEchoTranscriptV1(value: unknown): { request: ChatWorkspaceRefV1; response: ChatWorkspaceRefV1 } {
  const object = requireObject(value, "workspace echo transcript"); exactKeys(object, ["request", "response"]);
  const request = validateChatWorkspaceRefV1(object.request); const response = validateChatWorkspaceRefV1(object.response);
  assertChatWorkspaceEcho(request, response, "workspace response"); return { request, response };
}

export function validateWorkspaceOperationTranscriptV1(value: unknown): WorkspaceOperationTranscriptV1 {
  const object = requireObject(value, "workspace operation transcript");
  exactKeys(object, ["binding", "operation", "request", "response"]);
  const binding = validateWorkspaceResolvedBindingV1(object.binding);
  let request: ChatWorkspaceRefV1;
  let response: ChatWorkspaceRefV1;
  switch (object.operation) {
    case "getTree": {
      const treeRequest = validateWorkspaceTreeRequestV1(object.request);
      const treeResponse = validateWorkspaceTreePageV1(object.response);
      if (treeRequest.parentPath !== treeResponse.parentPath) fail("tree response parentPath must match the request");
      request = treeRequest; response = treeResponse; break;
    }
    case "close": request = validateChatWorkspaceRefV1(object.request); response = validateWorkspaceCloseResultV1(object.response); break;
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
    case "diffSummary": request = validateChatWorkspaceRefV1(object.request); response = validateWorkspaceDiffSummaryV1(object.response); break;
    case "diffFile": {
      const diffRequest = validateWorkspaceDiffFileRequestV1(object.request);
      const diffResponse = validateWorkspaceDiffFileDescriptorV1(object.response);
      if (diffRequest.path !== diffResponse.path) fail("diff file response path must match the request");
      request = diffRequest; response = diffResponse; break;
    }
    default: fail("unknown workspace operation");
  }
  assertChatWorkspaceEcho(binding.storedBinding, request, `${String(object.operation)} active chat binding`);
  assertChatWorkspaceEcho(request, response, `${String(object.operation)} response`);
  return object as unknown as WorkspaceOperationTranscriptV1;
}

function validateWorkspaceRefFields(object: Record<string, unknown>): WorkspaceRefV1 {
  for (const field of WORKSPACE_REF_FIELDS_V1) requireWorkspaceId(object[field], field);
  return { projectId: object.projectId as string, worktreeId: object.worktreeId as string, workspaceIdentity: object.workspaceIdentity as string };
}
function validateChatWorkspaceRefFields(object: Record<string, unknown>): ChatWorkspaceRefV1 {
  requireWorkspaceId(object.chatId, "chatId");
  return { chatId: object.chatId, ...validateWorkspaceRefFields(object) };
}
function assertWorkspaceEcho(expected: WorkspaceRefV1, actual: WorkspaceRefV1, name: string): void {
  if (expected.projectId !== actual.projectId || expected.worktreeId !== actual.worktreeId
    || expected.workspaceIdentity !== actual.workspaceIdentity) fail(`${name} must exactly echo projectId/worktreeId/workspaceIdentity`);
}
function assertChatWorkspaceEcho(expected: ChatWorkspaceRefV1, actual: ChatWorkspaceRefV1, name: string): void {
  if (expected.chatId !== actual.chatId) fail(`${name} must exactly echo chatId`);
  assertWorkspaceEcho(expected, actual, name);
}
function requireWorkspaceId(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || value.length === 0
    || new TextEncoder().encode(value).length > G5_ID_MAX_BYTES_V1) {
    fail(`${name} is required and must fit the UTF-8 byte limit`);
  }
}
