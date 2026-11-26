import test from "node:test";
import assert from "node:assert/strict";
import type { ModelChoicesRequest, ModelChoicesResult } from "../src/m5/contracts/model-choices-v1.ts";

const store = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
  getItem: (key: string) => store.get(key) ?? null,
  setItem: (key: string, value: string) => { store.set(key, value); },
  removeItem: (key: string) => { store.delete(key); },
} });

test("display model list survives a restart, skips re-query while fresh, and never replays usage", async () => {
  let calls = 0;
  const query = async (input: ModelChoicesRequest): Promise<ModelChoicesResult> => {
    calls++;
    return { schemaVersion: 1, runtime: input.runtime, scope: null,
      models: [{ modelSelector: "remembered-default", reasoningEfforts: ["medium"], isDefault: true }] };
  };
  const first = await import(`../src/m5/model-catalog.ts?first=${Date.now()}`);
  await first.loadModels("claude-code", query);
  assert.equal(calls, 1);
  assert.ok(store.has("elsecrew.m5.models.v1:claude-code"));
  // A fresh module instance models an app restart with an empty session cache.
  const restarted = await import(`../src/m5/model-catalog.ts?restart=${Date.now()}`);
  assert.equal(restarted.cachedModels("claude-code")?.[0].modelSelector, "remembered-default");
  await restarted.loadModels("claude-code", query);
  assert.equal(calls, 1, "a fresh remembered list must not query the provider again");
  await restarted.loadModels("claude-code", query, true);
  assert.equal(calls, 2, "explicit refresh still queries");
  // Stale or malformed entries are ignored rather than trusted.
  store.set("elsecrew.m5.models.v1:codex", JSON.stringify({ at: Date.now() - 7 * 60 * 60 * 1000,
    result: { schemaVersion: 1, runtime: "codex", scope: null, models: [{ modelSelector: "old", reasoningEfforts: [], isDefault: true }], usage: null } }));
  const stale = await import(`../src/m5/model-catalog.ts?stale=${Date.now()}`);
  assert.equal(stale.cachedModels("codex")?.[0].modelSelector, "old");
  await stale.loadModels("codex", query);
  assert.equal(calls, 3, "a list older than the freshness window is refreshed");
  store.set("elsecrew.m5.models.v1:claude-code", "{not json");
  const broken = await import(`../src/m5/model-catalog.ts?broken=${Date.now()}`);
  assert.equal(broken.cachedModels("claude-code"), undefined);
  assert.equal(JSON.parse(store.get("elsecrew.m5.models.v1:codex")!).result.usage, undefined);
});
