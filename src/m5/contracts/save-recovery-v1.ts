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
  actionsAvailable: false;
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
    if (recovery.actionsAvailable !== false) fail("save recovery mutations are not admitted");
  }
  return object as unknown as SaveRecoveryResult;
}
