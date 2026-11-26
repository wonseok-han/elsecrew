// ADE multi-project PR-A: the stage bar shows every stage, a team keeps
// working when the person switches projects, and the project list shows
// which projects work or wait.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { stageCells } from "../src/m5/task-story-model.ts";
import { BETWEEN_STEPS_MS, listedWorking, otherProjectsActivity, projectActivity, ProjectActivityTracker } from "../src/m5/project-activity.ts";
import type { AgentRunRecordV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import type { AppNotification } from "../src/m5/notifications.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

const source = (name: string) => readFileSync(new URL(`../src/m5/${name}`, import.meta.url), "utf8");
const now = Date.parse("2026-11-26T10:00:00Z");
const run = (id: string, role: string, state: string, at: number) => ({
  runId: id, chatId: "chat_a", createdAt: new Date(now - 600_000 + at * 1000).toISOString(),
  updatedAt: new Date(now - 600_000 + at * 1000 + 30_000).toISOString(), state, selectedProfile: { roleId: role },
}) as unknown as AgentRunRecordV1;

test("folded, the bar shows the last stages and how many came before; open, it shows every stage in order", () => {
  const runs = Array.from({ length: 9 }, (_, index) => run(`r${index}`, index % 2 ? "coding" : "coordinator", "succeeded", index * 60));
  const folded = stageCells(runs, { working: true, now });
  assert.equal(folded.hidden, 5);
  assert.equal(folded.cells.length, 5);
  const all = stageCells(runs, { working: true, now, max: Number.POSITIVE_INFINITY });
  assert.equal(all.hidden, 0);
  assert.deepEqual(all.cells.filter(cell => cell.kind === "run").map(cell => cell.kind === "run" ? cell.runId : ""), runs.map(item => item.runId));
  assert.equal(all.cells.at(-1)?.kind, "between", "what happens now still ends the row");
});

test("the stage bar's '+N' opens every stage in one sideways row and folds back", () => {
  const bar = source("stage-bar.tsx");
  assert.match(bar, /className="m5-stage-more" aria-expanded="false"[^>]*onClick=\{\(\) => setExpanded\(true\)\}/);
  assert.match(bar, /aria-expanded="true"[^>]*onClick=\{\(\) => setExpanded\(false\)\}/);
  assert.match(bar, /max: expanded \? Number\.POSITIVE_INFINITY : undefined/);
  assert.match(bar, /row\.scrollLeft = row\.scrollWidth/, "opened, the newest stage is in view");
  assert.match(bar, /data-testid="member-card"\]\[data-run-id=/, "a stage takes the story to its member's card");
  const css = source("styles.css");
  assert.match(css, /\.m5-stage-bar-all \.m5-stage-row \{[^}]*overflow-x: auto/);
  assert.match(css, /\.m5-stage-bar-all \.m5-stage \{[^}]*flex: 0 0 auto/);
  for (const dict of [ko, en]) for (const key of ["m5.stage.showAll", "m5.stage.fold", "m5.stage.jump"]) assert.ok(dict[key as keyof typeof dict], key);
});

test("switching projects leaves the team at work and keeps unsaved text for its task", () => {
  const workbench = source("workbench.tsx");
  const leave = workbench.slice(workbench.indexOf("const leaveProject = async"), workbench.indexOf("// Adding a folder keeps"));
  assert.match(leave, /controller\.canLeaveKeepingDrafts\(\)/);
  assert.match(leave, /controller\.leaveProjectView\(false, true\)/);
  assert.doesNotMatch(leave, /isTeamWorking/);
  const blocked = workbench.slice(workbench.indexOf("const switchBlockedReason"), workbench.indexOf("const addBlockedReason"));
  assert.doesNotMatch(blocked, /teamWorking|isTeamWorking/, "a working team does not hold the switch");
  // The update restart still waits for the open task's team.
  assert.match(workbench, /chatController\.canLeaveProjectView\(\) && !chatController\.isTeamWorking\(\) && !appWorking;/);
  const chat = source("chat-state.ts");
  const can = chat.slice(chat.indexOf("canLeaveProjectView(): boolean"), chat.indexOf("isTeamWorking(): boolean"));
  assert.doesNotMatch(can, /agentRuns|runs\.some/, "runs are not checked for leaving");
  assert.equal(ko["m5.switch.teamWorking" as keyof typeof ko], undefined);
});

test("a project is working while one of its runs is live, for a short wait between steps, and while its list says so", () => {
  let clock = now;
  const tracker = new ProjectActivityTracker(() => clock);
  const live = (id: string, chatId: string, projectId: string, state: string) => tracker.observeRun({ runId: id, chatId, projectId, state });
  live("r1", "chat_a", "project_a", "running");
  assert.equal(tracker.working("project_a"), true);
  assert.equal(tracker.working("project_b"), false);
  live("r1", "chat_a", "project_a", "succeeded");
  assert.equal(tracker.working("project_a"), true, "the coordinator picks the next member");
  clock += BETWEEN_STEPS_MS + 1;
  assert.equal(tracker.working("project_a"), false);
  // A run seen only ending (not started this session) is not a reason to wait.
  live("r9", "chat_z", "project_z", "failed");
  assert.equal(tracker.working("project_z"), false);
  // A project's own list, read when it opened: its task at work counts until a run of it ends.
  tracker.reconcileProject("project_b", ["chat_b"]);
  assert.deepEqual(tracker.workingProjects(), ["project_b"]);
  tracker.observeStatus("chat_b");
  assert.equal(tracker.working("project_b"), false, "the team said it paused or finished");
  tracker.reconcileProject("project_b", ["chat_b"]);
  tracker.reconcileProject("project_b", []);
  assert.equal(tracker.working("project_b"), false, "read again: nothing at work");
  tracker.dispose();
  // A draft that never started is not at work in the list (unlike the task view's own "working").
  assert.equal(listedWorking("draft"), false);
  for (const state of ["preparing", "running", "stopping", "applying"]) assert.equal(listedWorking(state), true, state);
  for (const state of ["reviewReady", "needsAttention", "canceled", null]) assert.equal(listedWorking(state), false, String(state));
  assert.match(source("workbench.tsx"), /chat\.chats\.filter\(item => listedWorking\(item\.state\)\)/);
});

test("the project list counts each project's open requests and the switcher marks other projects", () => {
  const tracker = new ProjectActivityTracker(() => now);
  tracker.observeRun({ runId: "r1", chatId: "chat_b", projectId: "project_b", state: "running" });
  const item = (id: string, projectId: string, kind: AppNotification["kind"], resolved = false) => ({ id, kind, projectId, chatId: "chat", goal: "goal", text: "", at: new Date(now).toISOString(), read: false, ...(resolved ? { resolved: true } : {}) }) as unknown as AppNotification;
  const items = [item("a1", "project_a", "attention"), item("b1", "project_b", "attention"), item("b2", "project_b", "attention", true), item("b3", "project_b", "done")];
  assert.deepEqual(projectActivity(tracker, items, "project_b"), { working: true, attention: 1 });
  assert.deepEqual(projectActivity(tracker, items, "project_c"), { working: false, attention: 0 });
  assert.deepEqual(otherProjectsActivity(tracker, items, "project_a"), { working: true, attention: 1 });
  assert.deepEqual(otherProjectsActivity(tracker, items, "project_b"), { working: false, attention: 1 });
  const catalog = source("project-catalog-view.tsx");
  assert.match(catalog, /activity && <ActivityBadges activity=\{activity\(entry\.projectId\)\} \/>/);
  assert.match(catalog, /m5-badge m5-badge-attention/);
  assert.match(catalog, /m5-badge m5-badge-running/);
  const switcher = source("project-switcher.tsx");
  assert.match(switcher, /m5-switcher-others-\$\{mark\}/);
  for (const dict of [ko, en]) for (const key of ["m5.catalog.attention", "m5.catalog.working", "m5.switch.othersWorking", "m5.switch.othersAttention", "m5.switch.busy", "m5.switch.unsavedHeld"]) assert.ok(dict[key as keyof typeof dict], key);
  tracker.dispose();
});

// PR #18 review I2: a waiting request is announced even when its task could
// not be read the first time (one failed `chat.get` used to drop it for good).
test("an approval whose task read failed once is announced on the next list", async () => {
  const { NotificationCenter } = await import("../src/m5/notifications.ts");
  const { startNotificationFeed } = await import("../src/m5/notification-feed.ts");
  const center = new NotificationCenter(null);
  const listeners: ((event: any) => void)[] = [];
  let reads = 0;
  const bridge = {
    onAgentRunStateChanged: (listener: (event: any) => void) => { listeners.push(listener); return () => {}; },
    onAgentPublicEvent: (listener: (event: any) => void) => { listeners.push(listener); return () => {}; },
    getChat: async () => { reads++; if (reads === 1) throw new Error("synthetic read failure"); return { chat: { chatId: "chat_b", projectId: "project_b", goal: "B", state: "running" } }; },
  };
  const approvals = { request: async () => ({ pending: [{ approvalId: "approval_b" }] }) };
  const realSetInterval = globalThis.setInterval;
  let tick: (() => void) | null = null;
  (globalThis as any).setInterval = (fn: () => void) => { tick = fn; return 1; };
  const stop = startNotificationFeed(bridge, center, approvals, () => {}, 1);
  (globalThis as any).setInterval = realSetInterval;
  listeners[0]!({ run: { runId: "run_b", chatId: "chat_b", taskId: "t", projectId: "project_b", worktreeId: "w", workspaceIdentity: "i", state: "running", selectedProfile: { roleId: "coding" } } });
  tick!();
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(center.snapshot().length, 0, "the first read failed");
  tick!();
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(center.snapshot()[0]?.id, "approval:approval_b");
  assert.equal(center.snapshot()[0]?.projectId, "project_b");
  // A question whose task read fails once is read again.
  reads = 0;
  listeners[1]!({ eventId: "question_b", chatId: "chat_b", kind: "question", text: "계속할까요?" });
  await new Promise(resolve => setTimeout(resolve, 30));
  assert.ok(center.snapshot().some(item => item.id === "question:question_b"));
  stop();
});

// PR #18 review I1·I3·M1 at the source.
test("the switch keeps drafts before it lets go; the update waits for every project; a stage on another page opens it", () => {
  const workbench = source("workbench.tsx");
  const leave = workbench.slice(workbench.indexOf("const leaveProject = async"), workbench.indexOf("// Adding a folder keeps"));
  assert.ok(leave.indexOf("prepareLeaveKeepingDrafts()") < leave.indexOf('action: "releaseView"'), "drafts are kept before the watch is released");
  assert.match(leave, /setDraftsHold\(true\); return false;/);
  assert.match(workbench, /&& !chatController\.isTeamWorking\(\) && !appWorking;/);
  assert.match(workbench, /<UpdateBanner teamWorking=\{chatController\.isTeamWorking\(\) \|\| appWorking\}/);
  assert.match(workbench, /action: "workStatus" as const/);
  assert.match(source("stage-bar.tsx"), /if \(!card\) \{ onReveal\?\.\(runId\); return; \}/);
  assert.match(source("task-story.tsx"), /onReveal=\{revealStage\}/);
  for (const dict of [ko, en]) for (const key of ["m5.switch.notKept", "m5.editorNotice.draftsNotKept"]) assert.ok(dict[key as keyof typeof dict], key);
});

// ADE multi-project PR-B (P3′·P4).
test("the inbox groups every project's tasks: to check, working (or waiting its turn), ready for review", async () => {
  const { groupInbox, inboxGroup, workingByProject } = await import("../src/m5/task-inbox-model.ts");
  const item = (chatId: string, projectId: string, state: string, extra: Record<string, boolean> = {}) => ({ chatId, projectId, goal: chatId, state, updatedAt: "2026-11-27T00:00:00Z", waiting: false, working: false, ...extra }) as never;
  assert.equal(inboxGroup(item("a", "p", "needsAttention")), "attention");
  assert.equal(inboxGroup(item("a", "p", "recoveryRequired")), "attention");
  assert.equal(inboxGroup(item("a", "p", "running")), "working");
  assert.equal(inboxGroup(item("a", "p", "reviewReady")), "review");
  assert.equal(inboxGroup(item("a", "p", "reviewReady", { waiting: true })), "working", "a follow-up waiting its turn works");
  const items = [item("a", "p1", "running"), item("b", "p1", "reviewReady"), item("c", "p2", "running", { waiting: true }), item("d", "p2", "needsAttention")];
  const groups = groupInbox(items);
  assert.deepEqual([groups.attention.length, groups.working.length, groups.review.length], [1, 2, 1]);
  // Badges come from the engine's full list of work (PR #20 review I4): a
  // task at work that the shortened list does not show still counts.
  const working = [{ projectId: "p1", chatId: "a" }, { projectId: "p2", chatId: "c" }, { projectId: "p4", chatId: "not-in-the-list" }];
  assert.deepEqual([...workingByProject(working)], [["p1", ["a"]], ["p2", ["c"]], ["p4", ["not-in-the-list"]]]);
  // The engine's list replaces the project lists' guesses for badges.
  const tracker = new ProjectActivityTracker(() => now);
  tracker.reconcileProject("p3", ["stale"]);
  tracker.reconcileFromEngine(workingByProject(working));
  assert.deepEqual(tracker.workingProjects().sort(), ["p1", "p2", "p4"]);
  tracker.dispose();
  assert.match(source("workbench.tsx"), /reconcileFromEngine\(workingByProject\(result\.workingTasks\)\)/, "badges never read the shortened list");
});

test("the inbox answer is checked: exact fields, known states, at most 200", async () => {
  const { validateProjectCatalogResult } = await import("../src/m5/contracts/project-catalog-v1.ts");
  const input = { action: "taskInbox" as const };
  const good = { projectId: "project_1", chatId: "chat_1", goal: "goal", state: "running", updatedAt: "2026-11-27T00:00:00Z", waiting: false, working: true };
  const work = [{ projectId: "project_1", chatId: "chat_1" }];
  assert.equal((validateProjectCatalogResult({ schemaVersion: 1, action: "taskInbox", items: [good], workingTasks: work }, input) as { items: unknown[] }).items.length, 1);
  for (const bad of [{ ...good, state: "draft" }, { ...good, extra: 1 }, { ...good, working: "yes" }, { ...good, chatId: "../x" }]) {
    assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, action: "taskInbox", items: [bad], workingTasks: work }, input), JSON.stringify(bad));
  }
  assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, action: "taskInbox", items: Array(201).fill(good), workingTasks: work }, input));
  assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, action: "taskInbox", items: [good] }, input), "the work list is required");
  for (const bad of [[{ projectId: "project_1" }], [{ projectId: "project_1", chatId: "../x" }], Array(1001).fill(work[0])]) {
    assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, action: "taskInbox", items: [good], workingTasks: bad }, input));
  }
});

test("a team over its agent's limit shows 'waiting its turn', not a failure", async () => {
  const { stageCells: cells, waitingTurn, TURN_WAIT_PREFIX } = await import("../src/m5/task-story-model.ts");
  const status = (text: string, at: number) => ({ kind: "status", text, createdAt: new Date(now + at * 1000).toISOString() });
  const done = run("r1", "coordinator", "succeeded", 0);
  assert.equal(waitingTurn([status(`${TURN_WAIT_PREFIX}. 다른 작업들이 Claude를 쓰고 있어요.`, 700)], [done]), true);
  assert.equal(waitingTurn([status(`${TURN_WAIT_PREFIX}.`, -900)], [done]), false, "an older wait than the last run");
  assert.equal(waitingTurn([status(`${TURN_WAIT_PREFIX}.`, 700)], [run("r2", "coding", "running", 0)]), false, "a run works");
  assert.equal(waitingTurn([status("팀 작업을 이어가고 있어요.", 700)], [done]), false);
  assert.equal(cells([done], { working: true, now, waiting: true }).cells.at(-1)?.kind, "waiting");
  assert.equal(cells([], { working: true, now, waiting: true }).cells.at(-1)?.kind, "waiting");
  // The engine's notice and the screen's prefix are one sentence.
  const engine = readFileSync(new URL("../crates/elsecrew-sidecar/src/orchestration_dispatch.rs", import.meta.url), "utf8");
  assert.ok(engine.includes(`pub(crate) const TURN_WAIT_NOTICE: &str = "${TURN_WAIT_PREFIX}";`));
  assert.match(source("chat-state.ts"), /\/waits its turn\/\.test\(/);
  assert.match(source("task-story.tsx"), /waiting=\{working && waitingTurn\(chat\.messages, chat\.agentRuns\)\}/);
  for (const dict of [ko, en]) for (const key of ["m5.stage.waiting", "m5.chatNotice.waitingTurn", "m5.inbox.title", "m5.inbox.waiting"]) assert.ok(dict[key as keyof typeof dict], key);
});
