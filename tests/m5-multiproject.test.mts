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
  assert.match(workbench, /chatController\.canLeaveProjectView\(\) && !chatController\.isTeamWorking\(\);/);
  assert.match(workbench, /<UpdateBanner teamWorking=\{chatController\.isTeamWorking\(\)\}/);
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
