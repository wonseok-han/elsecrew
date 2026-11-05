import assert from "node:assert/strict";
import test from "node:test";
import { validateSaveRecoveryRequest, validateSaveRecoveryResult } from "../src/m5/contracts/save-recovery-v1.ts";
const request = { chatId: "chat_one", projectId: "project_one", worktreeId: "worktree_one", workspaceIdentity: "workspace_one" };
const recovery = { saveId: `save_${"a".repeat(32)}`, path: "src/app.ts", phase: "replaceIntent", lockRevision: 12, observation: "replacementObserved", actionsAvailable: false };

test("save recovery accepts only a read-only inspection of the requested workspace", () => {
  assert.deepEqual(validateSaveRecoveryRequest(request), request);
  assert.doesNotThrow(() => validateSaveRecoveryResult({ ...request, recovery }, request));
  assert.doesNotThrow(() => validateSaveRecoveryResult({ ...request, recovery: null }, request));
  for (const field of ["chatId", "projectId", "worktreeId", "workspaceIdentity"]) {
    assert.throws(() => validateSaveRecoveryResult({ ...request, [field]: "another", recovery }, request));
  }
  assert.throws(() => validateSaveRecoveryRequest({ ...request, action: "abort" }));
  for (const patch of [{ actionsAvailable: true }, { phase: "done" }, { lockRevision: 0 }, { lockRevision: 1.5 }, { path: "../secret" }, { saveId: "foreign" }, { observation: "success" }, { rawIntent: {} }]) {
    assert.throws(() => validateSaveRecoveryResult({ ...request, recovery: { ...recovery, ...patch } }, request));
  }
});
