import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { recoveryChatPage, recoveryHistoryPage, resolveHistoryRecovery, recoveryAddress, type RecoveryHistoryBridge } from "../src/m5/project-recovery-history.ts";
const fixtures = JSON.parse(readFileSync(new URL("./fixtures/m5/agent-adapter-v1/g5c-identityless-recovery-cases.json", import.meta.url), "utf8"));
const fixture = (name: string): any => structuredClone(fixtures.valid.find((item: any) => item.name === name).value);
function setup() {
  const detail = fixture("identityless-recovery-chat-detail");
  const recovery = fixture("normal-unresolved-recovery-still-requires-process-evidence");
  const result = fixture("normal-recovery-still-resolves-abandoned");
  const calls: string[] = [];
  const methods = {
    async listChats() { calls.push("list"); return { projectId: detail.chat.projectId, items: [detail.chat], nextCursor: null }; },
    async getChat() { calls.push("history"); return detail; },
    async inspectAgentRecovery() { calls.push("inspect"); return recovery; },
    async resolveAgentRecovery() { calls.push("resolve"); return result; },
  };
  const bridge = new Proxy(methods, { get(target, key) { if (!(key in target)) throw new Error(`Unexpected authority: ${String(key)}`); return target[key as keyof typeof target]; } }) as RecoveryHistoryBridge;
  return { detail, recovery, result, bridge, calls, methods };
}
test("recovery history reads records without project reopening, snapshot creation or editor binding", async () => {
  const f = setup();
  const page = await recoveryChatPage(f.bridge, f.detail.chat.projectId, null);
  const detail = await recoveryHistoryPage(f.bridge, page.items[0]);
  assert.equal(detail.chat.chatId, f.detail.chat.chatId);
  assert.deepEqual(f.calls, ["list", "history"]);
});
test("history rejects project, workspace and cursor drift", async () => {
  const f = setup();
  await assert.rejects(recoveryChatPage(f.bridge, "project_other", null), /mismatch/);
  await assert.rejects(recoveryHistoryPage(f.bridge, { ...f.detail.chat, workspaceIdentity: "different" }), /mismatch/);
  await assert.rejects(recoveryHistoryPage(f.bridge, f.detail.chat, 10), /cursor mismatch/);
});
test("recovery resolution re-inspects exact binding and uses inspected revision", async () => {
  const f = setup();
  f.methods.resolveAgentRecovery = async function(input?: any) { f.calls.push("resolve"); assert.deepEqual(input, { ...recoveryAddress(f.recovery), expectedRevision: f.recovery.revision, resolution: "abandon" }); return f.result; };
  const result = await resolveHistoryRecovery(f.bridge, f.recovery, () => true);
  assert.equal(result.outcome, "resolvedAbandoned");
  assert.deepEqual(f.calls, ["inspect", "resolve"]);
});
test("identity-unavailable, stale screen and changed identity never resolve", async () => {
  const f = setup();
  await assert.rejects(resolveHistoryRecovery(f.bridge, fixture("identityless-recovery-record"), () => true));
  assert.deepEqual(f.calls, []);
  let current = true;
  f.methods.inspectAgentRecovery = async () => { f.calls.push("inspect"); current = false; return f.recovery; };
  await assert.rejects(resolveHistoryRecovery(f.bridge, f.recovery, () => current));
  assert.deepEqual(f.calls, ["inspect"]);
  f.methods.inspectAgentRecovery = async () => ({ ...f.recovery, runId: "agent_run_other" });
  await assert.rejects(resolveHistoryRecovery(f.bridge, f.recovery, () => true), /identity changed/);
});
