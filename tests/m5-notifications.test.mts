import assert from "node:assert/strict";
import test from "node:test";
import { NotificationCenter, osNotificationsEnabled, outcomeOfChatState, setOsNotificationsEnabled } from "../src/m5/notifications.ts";
import { startNotificationFeed, type NotificationFeedBridge } from "../src/m5/notification-feed.ts";

function memory() {
  const values = new Map<string, string>();
  return { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value); } };
}
const base = { kind: "done" as const, projectId: "p", chatId: "c", goal: "todo app", text: "", at: "2026-11-13T00:00:00Z" };

test("the center adds each id once, keeps the newest 50, and remembers read state", () => {
  const storage = memory();
  const center = new NotificationCenter(storage);
  assert.equal(center.add({ ...base, id: "a" }), true);
  assert.equal(center.add({ ...base, id: "a" }), false);
  for (let index = 0; index < 60; index++) center.add({ ...base, id: `n${index}` });
  assert.equal(center.snapshot().length, 50);
  assert.equal(center.snapshot()[0]!.id, "n59");
  center.markRead("n59");
  assert.equal(center.unread(), 49);
  center.markAllRead();
  assert.equal(center.unread(), 0);
  const restored = new NotificationCenter(storage);
  assert.equal(restored.snapshot().length, 50);
  assert.equal(restored.unread(), 0);
  assert.equal(new NotificationCenter({ getItem: () => "{broken", setItem: () => undefined }).snapshot().length, 0);
});

test("removing a task or project drops its notifications", () => {
  const center = new NotificationCenter(memory());
  center.add({ ...base, id: "a", chatId: "c1" });
  center.add({ ...base, id: "b", chatId: "c2" });
  center.add({ ...base, id: "c", projectId: "other", chatId: "c3" });
  center.removeFor("p", "c1");
  assert.deepEqual(center.snapshot().map(item => item.id), ["c", "b"]);
  center.removeFor("p");
  assert.deepEqual(center.snapshot().map(item => item.id), ["c"]);
});

test("chat states map to finished or stopped; banners default on", () => {
  assert.equal(outcomeOfChatState("waitingForUser"), "done");
  assert.equal(outcomeOfChatState("completed"), "done");
  assert.equal(outcomeOfChatState("needsAttention"), "stopped");
  assert.equal(outcomeOfChatState("running"), null);
  assert.equal(outcomeOfChatState("canceled"), null);
  const storage = memory();
  assert.equal(osNotificationsEnabled(storage), true);
  setOsNotificationsEnabled(false, storage);
  assert.equal(osNotificationsEnabled(storage), false);
});

test("the feed reports a finished turn, a team pause, a question and an approval once", async () => {
  const listeners: Record<string, (event: never) => void> = {};
  let chatState = "waitingForUser";
  const bridge: NotificationFeedBridge = {
    onAgentRunStateChanged: listener => { listeners.run = listener as never; return () => undefined; },
    onChatEvent: listener => { listeners.chat = listener as never; return () => undefined; },
    onAgentPublicEvent: listener => { listeners.public = listener as never; return () => undefined; },
    getChat: async ({ chatId }) => ({ chat: { chatId, projectId: "p", goal: "todo app", state: chatState } }),
  };
  let approvalCalls = 0;
  const approvals = { request: async () => { approvalCalls++; return { pending: [{ approvalId: "ap1" }] }; } };
  const center = new NotificationCenter(memory());
  const added: string[] = [];
  const stop = startNotificationFeed(bridge, center, approvals, id => added.push(id));
  const run = (state: string, roleId = "coordinator") => ({ run: { runId: `r-${roleId}`, chatId: "c", taskId: "t", projectId: "p", worktreeId: "w", workspaceIdentity: "i", state, selectedProfile: { roleId } } });
  const settle = () => new Promise(resolve => setTimeout(resolve, 10));
  // A delegating coordinator turn leaves the team running: no notification.
  chatState = "running";
  (listeners.run as (event: unknown) => void)(run("succeeded"));
  await settle();
  assert.equal(added.length, 0);
  chatState = "waitingForUser";
  (listeners.run as (event: unknown) => void)({ run: { ...run("succeeded").run, runId: "r-final" } });
  await settle();
  assert.deepEqual(added, ["done:r-final"]);
  chatState = "needsAttention";
  (listeners.chat as (event: unknown) => void)({ messageId: "m1", chatId: "c", kind: "status", text: "팀 작업을 멈췄어요. (원인 코드: x)" });
  await settle();
  assert.equal(center.snapshot()[0]!.kind, "stopped");
  assert.match(center.snapshot()[0]!.text, /원인 코드/);
  (listeners.public as (event: unknown) => void)({ eventId: "e1", chatId: "c", kind: "question", text: "어떤 색이 좋을까요?" });
  await settle();
  assert.equal(center.snapshot()[0]!.kind, "attention");
  // Approvals are polled only for runs seen running, and reported once.
  (listeners.run as (event: unknown) => void)(run("running", "coding"));
  await new Promise(resolve => setTimeout(resolve, 4300));
  assert.ok(approvalCalls >= 2);
  assert.equal(added.filter(id => id === "approval:ap1").length, 1);
  stop();
});
