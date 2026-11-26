import assert from "node:assert/strict";
import test from "node:test";
import type { AgentRunRecordV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import { runStageStatus, stageCells, stageNow } from "../src/m5/task-story-model.ts";
import { roleColorIndex, roleInitial } from "../src/m5/role-colors.ts";

const run = (id: string, roleId: string, state: string, start: number, end = start + 60, terminationOutcome: AgentRunRecordV1["terminationOutcome"] = "cleanExit") => ({
  runId: id, state, terminationOutcome, selectedProfile: { roleId },
  createdAt: new Date(Date.UTC(2026, 10, 26, 10, 0, start)).toISOString(),
  updatedAt: new Date(Date.UTC(2026, 10, 26, 10, 0, end)).toISOString(),
}) as unknown as AgentRunRecordV1;
const now = Date.UTC(2026, 10, 26, 10, 10, 0);

test("run states read as done, now, failed, canceled or needing a check", () => {
  assert.equal(runStageStatus({ state: "succeeded", terminationOutcome: "cleanExit" } as AgentRunRecordV1), "done");
  for (const state of ["preparing", "running", "cancelling"]) assert.equal(runStageStatus({ state, terminationOutcome: null } as unknown as AgentRunRecordV1), "now");
  assert.equal(runStageStatus({ state: "failed", terminationOutcome: "cleanExit" } as AgentRunRecordV1), "failed");
  assert.equal(runStageStatus({ state: "canceled", terminationOutcome: "cleanExit" } as AgentRunRecordV1), "canceled");
  assert.equal(runStageStatus({ state: "recoveryRequired", terminationOutcome: null } as unknown as AgentRunRecordV1), "recovery");
  assert.equal(runStageStatus({ state: "failed", terminationOutcome: "recoveryRequired" } as AgentRunRecordV1), "recovery");
});

test("while the team works, some cell always says what happens now (no silent gap)", () => {
  // Waiting: nothing has run yet.
  assert.deepEqual(stageCells([], { working: true, now }).cells, [{ kind: "planning" }]);
  // A run works: it is now, and a dashed next cell follows.
  const working = stageCells([run("a", "coordinator", "succeeded", 0), run("b", "coding", "running", 70)], { working: true, now });
  assert.deepEqual(working.cells.map(cell => cell.kind === "run" ? cell.status : cell.kind), ["done", "now", "next"]);
  assert.equal(stageNow(working.cells)?.kind, "run");
  // Between runs: the coordinator picks who goes next.
  const between = stageCells([run("a", "coordinator", "succeeded", 0), run("b", "coding", "succeeded", 70)], { working: true, now });
  assert.deepEqual(between.cells.map(cell => cell.kind === "run" ? cell.status : cell.kind), ["done", "done", "between"]);
  assert.equal(stageNow(between.cells)?.kind, "between");
  // A failed or canceled run while the team still works is also followed by "between".
  assert.equal(stageCells([run("a", "coding", "failed", 0)], { working: true, now }).cells.at(-1)?.kind, "between");
  assert.equal(stageCells([run("a", "coding", "canceled", 0)], { working: true, now }).cells.at(-1)?.kind, "between");
});

test("a finished task shows only the runs that happened, in order", () => {
  const cells = stageCells([run("b", "review", "succeeded", 100), run("a", "coding", "succeeded", 0)], { working: false, now }).cells;
  assert.deepEqual(cells.map(cell => cell.kind === "run" ? cell.roleId : cell.kind), ["coding", "review"]);
  assert.equal(stageNow(cells), null);
  assert.deepEqual(stageCells([], { working: false, now }).cells, []);
});

test("durations count a live run up to now and older runs fold into +N", () => {
  const live = stageCells([run("a", "coding", "running", 0, 5)], { working: true, now }).cells[0]!;
  assert.equal(live.kind === "run" ? live.seconds : null, 600);
  const many = Array.from({ length: 8 }, (_, index) => run(`r${index}`, "coding", "succeeded", index * 60));
  const folded = stageCells(many, { working: false, now });
  assert.equal(folded.hidden, 3);
  assert.deepEqual(folded.cells.map(cell => cell.kind === "run" ? cell.runId : ""), ["r3", "r4", "r5", "r6", "r7"]);
  const workingFold = stageCells(many, { working: true, now });
  assert.equal(workingFold.hidden, 4, "the trailing cell takes one place");
  assert.equal(workingFold.cells.length, 5);
});

test("a role keeps one colour everywhere; named roles get a stable hashed colour", () => {
  assert.equal(roleColorIndex("coordinator"), 1);
  assert.equal(roleColorIndex("coding"), 3);
  const custom = roleColorIndex("member-a1b2c3");
  assert.ok(custom >= 1 && custom <= 8);
  assert.equal(roleColorIndex("member-a1b2c3"), custom);
  assert.equal(roleColorIndex("constructor") >= 1, true, "object-like IDs are plain strings");
  assert.equal(roleInitial(" 보안 점검"), "보");
  assert.equal(roleInitial("qa"), "Q");
  assert.equal(roleInitial(""), "?");
});

import { storyItems } from "../src/m5/task-story-model.ts";

test("a run and the report it posted become one card; nothing shows twice", () => {
  const at = (s: number) => new Date(Date.UTC(2026, 10, 26, 10, 0, s)).toISOString();
  const messages = [
    { messageId: "m1", kind: "user", runId: null, createdAt: at(1) },
    { messageId: "m2", kind: "role", runId: "r1", createdAt: at(50) },
    { messageId: "m3", kind: "status", runId: null, createdAt: at(55) },
    { messageId: "m4", kind: "role", runId: "r2", createdAt: at(120) },
    { messageId: "m5", kind: "role", runId: "orphan", createdAt: at(130) },
    { messageId: "m6", kind: "user", runId: "r2", createdAt: at(140) },
  ];
  const runs = [{ runId: "r2", createdAt: at(60) }, { runId: "r1", createdAt: at(2) }];
  const items = storyItems(messages, runs);
  assert.deepEqual(items.map(item => item.key), ["message:m1", "run:r1", "message:m3", "run:r2", "message:m5", "message:m6"]);
  const r1 = items[1]!;
  assert.ok(r1.kind === "run" && r1.reports.map(report => report.messageId).join() === "m2");
  const r2 = items[3]!;
  assert.ok(r2.kind === "run" && r2.reports.map(report => report.messageId).join() === "m4");
  assert.equal(items.find(item => item.key === "message:m5")?.kind, "message", "a report of a run not in this task stays on its own");
  assert.equal(items.find(item => item.key === "message:m6")?.kind, "user", "the person's message is never folded into a run");
  const shown = items.flatMap(item => item.kind === "run" ? item.reports.map(report => report.messageId) : [item.message.messageId]);
  assert.equal(new Set(shown).size, shown.length);
  assert.equal(shown.length, messages.length);
  // A run with no report yet is still a card.
  assert.deepEqual(storyItems([], [{ runId: "r9", createdAt: at(1) }]).map(item => item.kind), ["run"]);
});
