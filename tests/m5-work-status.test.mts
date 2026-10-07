import assert from "node:assert/strict";
import test from "node:test";
import { groupRunHistory, runOutcomeNotice, summarizeWorkStatus } from "../src/m5/work-status.ts";
import type { AgentRunRecordV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import type { SystemStatusG5DV1 } from "../src/m5/contracts/lifecycle-g5d-v1.ts";

const status = (ids: string[], recoveryRequired = ids.length > 0) => ({
  storageAdmission: { state: "ready" }, blockerIds: ids, recoveryRequired,
}) as SystemStatusG5DV1;

test("run history collapses only older confirmed terminal records without mutating input", () => {
  const run = (id: string, state: AgentRunRecordV1["state"], terminationOutcome: AgentRunRecordV1["terminationOutcome"], day: number) =>
    ({runId:id,state,terminationOutcome,createdAt:`2026-10-0${day}T00:00:00Z`}) as AgentRunRecordV1;
  const runs = [run("old", "failed", "cleanExit", 1), run("latest", "succeeded", "cleanExit", 4),
    run("live", "running", null, 2), run("unknown", "failed", null, 2), run("recovery", "recoveryRequired", "recoveryRequired", 3)];
  const before = [...runs];
  const grouped = groupRunHistory(runs);
  assert.deepEqual(grouped.current.map(r => r.runId), ["latest", "recovery", "live", "unknown"]);
  assert.deepEqual(grouped.previous.map(r => r.runId), ["old"]);
  assert.deepEqual(runs, before);
  assert.deepEqual(groupRunHistory([]), {current:[], previous:[]});
});

test("run outcome guidance distinguishes failure from unconfirmed termination", () => {
  assert.equal(runOutcomeNotice({state:"failed", terminationOutcome:"cleanExit"}), "m5.run.help.failedClean");
  assert.equal(runOutcomeNotice({state:"succeeded", terminationOutcome:null}), "m5.run.help.unconfirmed");
  assert.equal(runOutcomeNotice({state:"canceled", terminationOutcome:"cleanExit"}), "m5.run.help.canceledClean");
  assert.equal(runOutcomeNotice({state:"succeeded", terminationOutcome:"cleanExit"}), "m5.run.help.succeededClean");
  for (const state of ["preparing", "running", "cancelling"] as const) {
    assert.equal(runOutcomeNotice({state, terminationOutcome:null}), null);
  }
  assert.equal(runOutcomeNotice({state:"failed", terminationOutcome:"recoveryRequired"}), "m5.run.help.recovery");
  assert.equal(runOutcomeNotice({state:"recoveryRequired", terminationOutcome:null}), "m5.run.help.recovery");
});

test("work status exposes both live run and pre-chat lock without inferring termination", () => {
  assert.equal(summarizeWorkStatus(status([])), null);
  assert.deepEqual(summarizeWorkStatus(status(["lock:before_chat", "run:live"])), {
    total: 2, ids: ["lock:before_chat", "run:live"], omitted: 0,
  });
});

test("work status rejects contradictory state and bounds technical rendering", () => {
  for (const value of [status(["lock:x"], false), status([], true), status(["a", "a"]), status([""])]) {
    assert.throws(() => summarizeWorkStatus(value));
  }
  assert.throws(() => summarizeWorkStatus({ ...status([]), storageAdmission: { state: "verifying" } } as SystemStatusG5DV1));
  const summary = summarizeWorkStatus(status(Array.from({ length: 25 }, (_, i) => `${i}\u202e${"x".repeat(200)}`)))!;
  assert.equal(summary.ids.length, 20);
  assert.equal(summary.omitted, 5);
  assert(summary.ids.every(id => id.length <= 160 && !id.includes("\u202e")));
});
