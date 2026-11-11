import test from "node:test";
import assert from "node:assert/strict";
import { validateModelChoicesRequest, validateModelChoicesResult } from "../src/m5/contracts/model-choices-v1.ts";

const request = validateModelChoicesRequest({ runtime: "codex", refresh: true, chatId: "chat-1", roleId: "planning" });
const result = () => ({ schemaVersion: 1, runtime: "codex", scope: { chatId: "chat-1", roleId: "planning", worktreeId: "work-1", workspaceIdentity: "workspace-1" }, models: [{ modelSelector: "available-model", reasoningEfforts: ["medium"] }] });

test("model choices are scoped display data without execution authority", () => {
  assert.deepEqual(validateModelChoicesResult(result(), request), result());
  assert.throws(() => validateModelChoicesResult({ ...result(), canStart: true }, request));
  assert.throws(() => validateModelChoicesResult({ ...result(), scope: { ...result().scope, chatId: "other" } }, request));
  const unscoped = validateModelChoicesRequest({ runtime: "codex", refresh: true });
  assert.equal(validateModelChoicesResult({ ...result(), scope: null }, unscoped).scope, null);
  assert.throws(() => validateModelChoicesResult(result(), unscoped));
  assert.throws(() => validateModelChoicesResult({ ...result(), scope: null }, request));
  const claude = validateModelChoicesRequest({ ...request, runtime: "claude-code" });
  assert.equal(validateModelChoicesResult({ ...result(), runtime: "claude-code" }, claude).runtime, "claude-code");
  assert.throws(() => validateModelChoicesResult(result(), claude));
});

test("model choices reject duplicate, unbounded and raw vendor fields", () => {
  const value = result();
  assert.throws(() => validateModelChoicesResult({ ...value, models: [value.models[0], value.models[0]] }, request));
  assert.throws(() => validateModelChoicesResult({ ...value, models: [{ ...value.models[0], description: "private" }] }, request));
  assert.throws(() => validateModelChoicesResult({ ...value, models: [{ modelSelector: "x".repeat(129), reasoningEfforts: [] }] }, request));
  assert.throws(() => validateModelChoicesResult({ ...value, models: [{ modelSelector: "model", reasoningEfforts: ["medium", "medium"] }] }, request));
  assert.deepEqual(validateModelChoicesResult({ ...value, models: [] }, request).models, []);
});

 test("provider default metadata is bounded and does not fabricate newest ordering", () => {
  const value = result();
  const models = [{ ...value.models[0], isDefault: true }];
  assert.equal(validateModelChoicesResult({ ...value, models }, request).models[0].isDefault, true);
  assert.throws(() => validateModelChoicesResult({ ...value, models: [{ ...models[0], isDefault: "true" }] }, request));
 });
