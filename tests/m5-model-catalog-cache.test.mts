import test from "node:test";
import assert from "node:assert/strict";
import { cachedModels, loadModels, withRecommendedModels } from "../src/m5/model-catalog.ts";
import type { ModelChoicesRequest, ModelChoicesResult } from "../src/m5/contracts/model-choices-v1.ts";
test("screen catalog requests deduplicate, serialize providers, and reuse display cache", async () => {
  const calls: string[] = []; let active = 0;
  const query = async (input: ModelChoicesRequest): Promise<ModelChoicesResult> => {
    assert.equal(input.chatId, undefined); assert.equal(input.roleId, undefined);
    assert.equal(active++, 0); calls.push(input.runtime);
    await new Promise(resolve => setTimeout(resolve, 5)); active--;
    return { schemaVersion: 1, runtime: input.runtime, scope: null, models: [{ modelSelector: "provider-default", reasoningEfforts: ["medium"], isDefault: true }] };
  };
  await Promise.all([loadModels("codex", query, true), loadModels("codex", query, true), loadModels("claude-code", query, true)]);
  assert.deepEqual(calls, ["codex", "claude-code"]);
  await loadModels("codex", query);
  assert.equal(calls.length, 2);
  assert.equal(cachedModels("codex")?.[0].modelSelector, "provider-default");
  await assert.rejects(loadModels("codex", async () => { throw new Error("offline"); }, true));
  assert.equal(cachedModels("codex")?.[0].modelSelector, "provider-default");
  await loadModels("codex", query, true); assert.equal(calls.length, 3);
});

test("roles without a model start on their provider's single recommendation", () => {
  const catalogs = new Map([
    ["claude", [{ modelSelector: "default", reasoningEfforts: [], isDefault: true }, { modelSelector: "opus", reasoningEfforts: [] }]],
    ["codex", [{ modelSelector: "gpt-x", reasoningEfforts: ["low", "medium", "high"], isDefault: true }]],
  ] as const);
  const roles = [
    { roleId: "coordinator", runtime: "claude" as const, modelSelector: "", reasoningEffort: null },
    { roleId: "coding", runtime: "claude" as const, modelSelector: "opus", reasoningEffort: null },
    { roleId: "planning", runtime: "codex" as const, modelSelector: "", reasoningEffort: null },
  ];
  assert.deepEqual(withRecommendedModels(roles, catalogs), [
    { roleId: "coordinator", runtime: "claude", modelSelector: "default", reasoningEffort: null },
    { roleId: "coding", runtime: "claude", modelSelector: "opus", reasoningEffort: null },
    { roleId: "planning", runtime: "codex", modelSelector: "gpt-x", reasoningEffort: "medium" },
  ]);
  // No single recommendation: the user picks by hand.
  const unclear = new Map([["claude", [{ modelSelector: "a", reasoningEfforts: [], isDefault: true }, { modelSelector: "b", reasoningEfforts: [], isDefault: true }]]] as const);
  assert.equal(withRecommendedModels([roles[0]], unclear), null);
  assert.equal(withRecommendedModels([roles[0]], new Map()), null);
});
