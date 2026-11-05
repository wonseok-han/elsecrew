import { validateFileVersionV1 } from "./file-v1.ts";
import { exactKeys, fail, requireObject, requireRelativePath, requireSafeUnsigned } from "./project-v1.ts";
import { validateChatWorkspaceRefV1, type ChatWorkspaceRefV1 } from "./workspace-addressing-v1.ts";

export const SAVE_RECOVERY_CHANNEL = "m5:workspace:save-recovery";
export const SAVE_RECOVERY_PHASES = ["preparing", "ready", "sourceReady", "replaceIntent", "replaced", "cleanupIntent", "guardRemoved", "stagingRemoved", "cleanupComplete", "abortIntent", "abortSourceRemoved", "abortGuardRemoved", "abortStagingRemoved", "abortComplete"] as const;
export const SAVE_RECOVERY_OBSERVATIONS = ["replacementObserved", "originalObserved", "expectedContentOnly", "conflictOrMissing"] as const;
export interface SaveRecoveryInspection {
  saveId: string;
  path: string;
  phase: typeof SAVE_RECOVERY_PHASES[number];
  lockRevision: number;
  observation: typeof SAVE_RECOVERY_OBSERVATIONS[number];
  actionsAvailable: boolean;
}
export type SaveRecoveryResult = ChatWorkspaceRefV1 & { recovery: SaveRecoveryInspection | null };
export const validateSaveRecoveryRequest = validateChatWorkspaceRefV1;

export function validateSaveRecoveryResult(value: unknown, request: ChatWorkspaceRefV1): SaveRecoveryResult {
  const object = requireObject(value, "save recovery result");
  exactKeys(object, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "recovery"]);
  const expected = validateSaveRecoveryRequest(request);
  for (const key of ["chatId", "projectId", "worktreeId", "workspaceIdentity"] as const) {
    if (object[key] !== expected[key]) fail("save recovery workspace differs");
  }
  if (object.recovery !== null) {
    const recovery = requireObject(object.recovery, "save recovery");
    exactKeys(recovery, ["saveId", "path", "phase", "lockRevision", "observation", "actionsAvailable"]);
    if (typeof recovery.saveId !== "string" || !/^save_[a-f0-9]{32}$/.test(recovery.saveId)) fail("invalid save identity");
    requireRelativePath(recovery.path, "path", false);
    if (!(SAVE_RECOVERY_PHASES as readonly unknown[]).includes(recovery.phase)) fail("invalid save recovery phase");
    if (!(SAVE_RECOVERY_OBSERVATIONS as readonly unknown[]).includes(recovery.observation)) fail("invalid save observation");
    if (requireSafeUnsigned(recovery.lockRevision, "lockRevision") === 0) fail("save revision must be positive");
    if (typeof recovery.actionsAvailable !== "boolean") fail("invalid save recovery admission");
  }
  return object as unknown as SaveRecoveryResult;
}

export const SAVE_RECOVERY_RESOLVE_CHANNEL = "m5:workspace:resolve-save-recovery";
export type SaveRecoveryDecision = "complete" | "abort";
export type SaveRecoveryResolveRequest = ChatWorkspaceRefV1 & {
  saveId: string; expectedLockRevision: number; decision: SaveRecoveryDecision; confirmed: true;
};
export type SaveRecoveryResolveResult = ChatWorkspaceRefV1 & { saveId: string; outcome: "committed" | "aborted"; version: string };
export function validateSaveRecoveryResolveRequest(value: unknown): SaveRecoveryResolveRequest {
  const v = requireObject(value, "save recovery confirmation");
  exactKeys(v, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "saveId", "expectedLockRevision", "decision", "confirmed"]);
  validateChatWorkspaceRefV1({ chatId: v.chatId, projectId: v.projectId, worktreeId: v.worktreeId, workspaceIdentity: v.workspaceIdentity });
  if (typeof v.saveId !== "string" || !/^save_[a-f0-9]{32}$/.test(v.saveId)) fail("invalid save identity");
  if (requireSafeUnsigned(v.expectedLockRevision, "expectedLockRevision") === 0 || v.confirmed !== true || !["complete", "abort"].includes(String(v.decision))) fail("invalid save recovery confirmation");
  return v as unknown as SaveRecoveryResolveRequest;
}
export function validateSaveRecoveryResolveResult(value: unknown, request: SaveRecoveryResolveRequest): SaveRecoveryResolveResult {
  const v = requireObject(value, "save recovery completion");
  exactKeys(v, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "saveId", "outcome", "version"]);
  const expected = validateSaveRecoveryResolveRequest(request);
  for (const key of ["chatId", "projectId", "worktreeId", "workspaceIdentity", "saveId"] as const) if (v[key] !== expected[key]) fail("save recovery completion differs");
  if (v.outcome !== (expected.decision === "complete" ? "committed" : "aborted")) fail("save recovery outcome differs");
  validateFileVersionV1(v.version);
  return v as unknown as SaveRecoveryResolveResult;
}
export function saveRecoveryDecision(recovery: SaveRecoveryInspection): SaveRecoveryDecision | null {
  if (!recovery.actionsAvailable) return null;
  if (recovery.observation === "replacementObserved" && ["replaceIntent", "replaced", "cleanupIntent", "guardRemoved", "stagingRemoved", "cleanupComplete"].includes(recovery.phase)) return "complete";
  if (recovery.observation === "originalObserved" && ["preparing", "ready", "sourceReady", "replaceIntent", "abortIntent", "abortSourceRemoved", "abortGuardRemoved", "abortStagingRemoved", "abortComplete"].includes(recovery.phase)) return "abort";
  return null;
}
