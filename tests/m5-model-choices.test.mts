import test from "node:test";
import assert from "node:assert/strict";
import { validateModelChoicesRequest, validateModelChoicesResult } from "../src/m5/contracts/model-choices-v1.ts";

const request = validateModelChoicesRequest({ runtime: "codex", refresh: true, chatId: "chat-1", roleId: "planning" });
const result = () => ({ schemaVersion: 1, runtime: "codex", scope: { chatId: "chat-1", roleId: "planning", worktreeId: "work-1", workspaceIdentity: "workspace-1" }, models: [{ modelSelector: "available-model", reasoningEfforts: ["medium"] }] });

test("model choices are scoped display data without execution authority", () => {
  assert.deepEqual(validateModelChoicesResult(result(), request), result());
  assert.throws(() => validateModelChoicesResult({ ...result(), canStart: true }, request));
  assert.throws(() => validateModelChoicesResult({ ...result(), scope: { ...result().scope, chatId: "other" } }, request));
  assert.throws(() => validateModelChoicesRequest({ runtime: "codex", refresh: true }));
  assert.throws(() => validateModelChoicesRequest({ ...request, runtime: "claude-code" }));
});

test("model choices reject duplicate, unbounded and raw vendor fields", () => {
  const value = result();
  assert.throws(() => validateModelChoicesResult({ ...value, models: [value.models[0], value.models[0]] }, request));
  assert.throws(() => validateModelChoicesResult({ ...value, models: [{ ...value.models[0], description: "private" }] }, request));
  assert.throws(() => validateModelChoicesResult({ ...value, models: [{ modelSelector: "x".repeat(129), reasoningEfforts: [] }] }, request));
  assert.throws(() => validateModelChoicesResult({ ...value, models: [{ modelSelector: "model", reasoningEfforts: ["medium", "medium"] }] }, request));
  assert.deepEqual(validateModelChoicesResult({ ...value, models: [] }, request).models, []);
});
