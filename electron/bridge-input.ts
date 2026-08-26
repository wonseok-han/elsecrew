import { MAX_SEQUENCE, MAX_TEXT_CHUNK_BYTES } from "../src/m5/contracts/protocol-v1.ts";
import { FILE_MAX_BYTES_V1 } from "../src/m5/contracts/project-v1.ts";
import { validateFileVersionV1, type SaveChunkV1, type SaveCommitV1 } from "../src/m5/contracts/file-v1.ts";
import {
  validateChatCreateRequestV1,
  validateChatGetRequestV1,
  validateChatListRequestV1,
  validateFixtureRunStartRequestV1,
  validatePageRequestV1,
  validateProjectRoleSettingsUpdateRequestV1,
  validatePrepareChatWorkspaceRequestV1,
  validateAbandonPreparedWorkspaceRequestV1,
  validateRecoveryMarkResolvedRequestV1,
  validateRunCancelRequestV1,
} from "../src/m5/contracts/run-recovery-v1.ts";
import {
  validateChatWorkspaceRefV1,
  validateWorkspaceDiffFileRequestV1,
  validateWorkspaceFileReadRequestV1,
  validateWorkspaceTreeRequestV1,
} from "../src/m5/contracts/workspace-addressing-v1.ts";

type ProjectInput = { projectId: string };
type PathInput = ProjectInput & { path: string };

export function validateTreeInput(value: unknown): { projectId: string; parentPath: string; cursor: string | null } {
  const input = exact(value, ["projectId", "parentPath", "cursor"]);
  return { projectId: id(input.projectId, "projectId"), parentPath: path(input.parentPath, true), cursor: input.cursor === null ? null : id(input.cursor, "cursor") };
}

export function validateProjectInput(value: unknown): ProjectInput {
  const input = exact(value, ["projectId"]);
  return { projectId: id(input.projectId, "projectId") };
}

export function validatePathInput(value: unknown): PathInput {
  const input = exact(value, ["projectId", "path"]);
  return { projectId: id(input.projectId, "projectId"), path: path(input.path, false) };
}

export function validateSaveBeginInput(value: unknown) {
  const input = exact(value, ["projectId", "path", "expectedVersion", "totalBytes", "encoding"]);
  return {
    projectId: id(input.projectId, "projectId"), path: path(input.path, false),
    expectedVersion: validateFileVersionV1(input.expectedVersion), totalBytes: bytes(input.totalBytes, "totalBytes"),
    encoding: literal(input.encoding, "utf8", "encoding"),
  };
}

export function validateSaveChunkInput(value: unknown): SaveChunkV1 {
  const input = exact(value, ["saveId", "seq", "text"]);
  const text = string(input.text, "text");
  if (new TextEncoder().encode(text).byteLength > MAX_TEXT_CHUNK_BYTES) throw new Error("text exceeds 256 KiB");
  return { saveId: id(input.saveId, "saveId"), seq: sequence(input.seq, "seq"), text };
}

export function validateSaveCommitInput(value: unknown): SaveCommitV1 {
  const input = exact(value, ["saveId", "expectedVersion", "lastSeq", "totalBytes"]);
  return { saveId: id(input.saveId, "saveId"), expectedVersion: validateFileVersionV1(input.expectedVersion), lastSeq: sequence(input.lastSeq, "lastSeq"), totalBytes: bytes(input.totalBytes, "totalBytes") };
}

export const validateWorkspaceTreeInput = validateWorkspaceTreeRequestV1;
export const validateWorkspaceRefInput = validateChatWorkspaceRefV1;
export const validateWorkspaceFileReadInput = validateWorkspaceFileReadRequestV1;
export function validateWorkspaceSaveBeginInput(value: unknown) {
  const input = exact(value, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "path", "expectedVersion", "totalBytes", "encoding"]);
  return {
    ...validateChatWorkspaceRefV1({ chatId: input.chatId, projectId: input.projectId, worktreeId: input.worktreeId, workspaceIdentity: input.workspaceIdentity }),
    path: path(input.path, false), expectedVersion: validateFileVersionV1(input.expectedVersion), totalBytes: bytes(input.totalBytes, "totalBytes"), encoding: literal(input.encoding, "utf8", "encoding"),
  };
}
export const validateWorkspaceDiffFileInput = validateWorkspaceDiffFileRequestV1;
export function validateWorkspaceSaveChunkInput(value: unknown) {
  const input = exact(value, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "saveId", "seq", "text"]);
  return {
    ...validateChatWorkspaceRefV1({ chatId: input.chatId, projectId: input.projectId, worktreeId: input.worktreeId, workspaceIdentity: input.workspaceIdentity }),
    ...validateSaveChunkInput({ saveId: input.saveId, seq: input.seq, text: input.text }),
  };
}
export function validateWorkspaceSaveCommitInput(value: unknown) {
  const input = exact(value, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "saveId", "expectedVersion", "lastSeq", "totalBytes"]);
  return {
    ...validateChatWorkspaceRefV1({ chatId: input.chatId, projectId: input.projectId, worktreeId: input.worktreeId, workspaceIdentity: input.workspaceIdentity }),
    ...validateSaveCommitInput({ saveId: input.saveId, expectedVersion: input.expectedVersion, lastSeq: input.lastSeq, totalBytes: input.totalBytes }),
  };
}
export const validateRoleSettingsUpdateInput = validateProjectRoleSettingsUpdateRequestV1;
export const validatePrepareChatWorkspaceInput = validatePrepareChatWorkspaceRequestV1;
export const validateAbandonPreparedWorkspaceInput = validateAbandonPreparedWorkspaceRequestV1;
export function validateRoleSettingsGetInput(value: unknown): { projectId: string } { return validateProjectInput(value); }
export const validateChatCreateInput = validateChatCreateRequestV1;
export const validateChatListInput = validateChatListRequestV1;
export const validateChatGetInput = validateChatGetRequestV1;
export const validateFixtureRunStartInput = validateFixtureRunStartRequestV1;
export const validateRunCancelInput = validateRunCancelRequestV1;
export const validateRecoveryListInput = validatePageRequestV1;
export function validateRecoveryInspectInput(value: unknown): { recoveryId: string } {
  const input = exact(value, ["recoveryId"]);
  return { recoveryId: id(input.recoveryId, "recoveryId") };
}
export const validateRecoveryMarkResolvedInput = validateRecoveryMarkResolvedRequestV1;

function exact(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("input must be an object");
  const actual = Object.keys(value).sort(); const expected = [...keys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) throw new Error("input fields are invalid");
  return value as Record<string, unknown>;
}
function id(value: unknown, name: string): string { return string(value, name, 512); }
function string(value: unknown, name: string, max = Number.MAX_SAFE_INTEGER): string { if (typeof value !== "string" || value.length === 0 || new TextEncoder().encode(value).byteLength > max) throw new Error(`${name} is invalid`); return value; }
function path(value: unknown, empty: boolean): string { const valueString = empty && value === "" ? "" : string(value, "path"); if ((!empty && valueString === "") || valueString.startsWith("/") || valueString.includes("\\") || valueString.split("/").some((part) => part === "" || part === "." || part === "..")) throw new Error("path is not normalized"); return valueString; }
function bytes(value: unknown, name: string): number { if (!Number.isSafeInteger(value) || (value as number) < 0 || (value as number) > FILE_MAX_BYTES_V1) throw new Error(`${name} is invalid`); return value as number; }
function sequence(value: unknown, name: string): number { if (!Number.isSafeInteger(value) || (value as number) < 1 || (value as number) > MAX_SEQUENCE) throw new Error(`${name} is invalid`); return value as number; }
function literal(value: unknown, expected: string, name: string): "utf8" { if (value !== expected) throw new Error(`${name} is invalid`); return "utf8"; }
