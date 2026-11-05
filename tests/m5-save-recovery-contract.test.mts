import assert from "node:assert/strict";
import test from "node:test";
import { validateSaveRecoveryRequest, validateSaveRecoveryResult, validateSaveRecoveryResolveRequest, validateSaveRecoveryResolveResult, saveRecoveryDecision } from "../src/m5/contracts/save-recovery-v1.ts";
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
  for (const patch of [{ actionsAvailable: "true" }, { phase: "done" }, { lockRevision: 0 }, { lockRevision: 1.5 }, { path: "../secret" }, { saveId: "foreign" }, { observation: "success" }, { rawIntent: {} }]) {
    assert.throws(() => validateSaveRecoveryResult({ ...request, recovery: { ...recovery, ...patch } }, request));
  }
});


test("resolution requires reviewed identity, revision, decision and explicit confirmation", () => {
  const confirmed = { ...request, saveId: recovery.saveId, expectedLockRevision: 12, decision: "complete" as const, confirmed: true as const };
  assert.deepEqual(validateSaveRecoveryResolveRequest(confirmed), confirmed);
  for (const patch of [{ confirmed: false }, { confirmed: undefined }, { expectedLockRevision: 0 }, { expectedLockRevision: 1.5 }, { decision: "retry" }, { path: "other" }]) {
    assert.throws(() => validateSaveRecoveryResolveRequest({ ...confirmed, ...patch }));
  }
  const response = { ...request, saveId: recovery.saveId, outcome: "committed", version: `sha256:${"a".repeat(64)}` };
  assert.doesNotThrow(() => validateSaveRecoveryResolveResult(response, confirmed));
  assert.throws(() => validateSaveRecoveryResolveResult({ ...response, outcome: "aborted" }, confirmed));
  assert.throws(() => validateSaveRecoveryResolveResult({ ...response, chatId: "other" }, confirmed));
  assert.throws(() => validateSaveRecoveryResolveResult({ ...response, saveId: `save_${"b".repeat(32)}` }, confirmed));
  assert.doesNotThrow(() => validateSaveRecoveryResolveResult({ ...response, outcome: "aborted" }, { ...confirmed, decision: "abort" }));
});

test("available choices respect admission, observed identity and journal phase", () => {
  const inspected = validateSaveRecoveryResult({ ...request, recovery }, request).recovery!;
  assert.equal(saveRecoveryDecision(inspected), null);
  assert.equal(saveRecoveryDecision({ ...inspected, actionsAvailable: true }), "complete");
  assert.equal(saveRecoveryDecision({ ...inspected, actionsAvailable: true, observation: "originalObserved" }), "abort");
  assert.equal(saveRecoveryDecision({ ...inspected, actionsAvailable: true, observation: "conflictOrMissing" }), null);
  assert.equal(saveRecoveryDecision({ ...inspected, actionsAvailable: true, observation: "expectedContentOnly" }), null);
  assert.equal(saveRecoveryDecision({ ...inspected, actionsAvailable: true, phase: "abortComplete" }), null);
  assert.equal(saveRecoveryDecision({ ...inspected, actionsAvailable: true, phase: "replaced", observation: "originalObserved" }), null);
});
