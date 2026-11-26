import assert from "node:assert/strict";
import test from "node:test";
import { groupRunHistory, runDurationSeconds, runOrder, runOutcomeNotice, summarizeWorkStatus } from "../src/m5/work-status.ts";
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

test("finished runs read oldest first with a stable chat-wide order and duration", () => {
  const run = (id: string, minute: number) => ({runId:id,state:"succeeded",terminationOutcome:"cleanExit",
    createdAt:`2026-11-12T05:0${minute}:00Z`,updatedAt:`2026-11-12T05:0${minute}:45Z`}) as AgentRunRecordV1;
  const runs = [run("coordinator-2", 3), run("coordinator-1", 1), run("planning", 2), run("coding", 4)];
  const grouped = groupRunHistory(runs);
  assert.deepEqual(grouped.current.map(r => r.runId), ["coding"]);
  assert.deepEqual(grouped.previous.map(r => r.runId), ["coordinator-1", "planning", "coordinator-2"]);
  const order = runOrder(runs);
  assert.deepEqual(["coordinator-1", "planning", "coordinator-2", "coding"].map(id => order.get(id)), [1, 2, 3, 4]);
  assert.equal(runDurationSeconds(runs[0]), 45);
  assert.equal(runDurationSeconds({createdAt:"bad", updatedAt:"2026-11-12T05:00:00Z"}), null);
  // A run still going counts up to now; a finished one keeps its record.
  const now = Date.parse("2026-11-12T05:03:00Z");
  const going = {createdAt:"2026-11-12T05:01:00Z", updatedAt:"2026-11-12T05:01:17Z", state:"running"};
  assert.equal(runDurationSeconds(going, now), 120);
  assert.equal(runDurationSeconds(going), 17);
  assert.equal(runDurationSeconds({...going, state:"succeeded"}, now), 17);
  // A read-only role's card never points at changes it cannot have made.
  const readOnly = {state:"succeeded", terminationOutcome:"cleanExit", selectedProfile:{permission:"readOnly"}} as const;
  assert.equal(runOutcomeNotice(readOnly), "m5.run.help.succeededReadOnly");
  assert.equal(runOutcomeNotice({...readOnly, state:"canceled"}), "m5.run.help.canceledReadOnly");
  assert.equal(runOutcomeNotice({...readOnly, selectedProfile:{permission:"workspaceWrite"}}), "m5.run.help.succeededClean");
});

test("work that is running normally is not something to check", () => {
  assert.equal(summarizeWorkStatus(status(["run:agent_run_a"]), ["agent_run_a"]), null);
  const summary = summarizeWorkStatus(status(["run:agent_run_a", "run:agent_run_b", "lock:lock_x"]), ["agent_run_a"]);
  assert.deepEqual(summary?.ids, ["run:agent_run_b", "lock:lock_x"]);
  assert.equal(summary?.total, 2);
  assert.equal(summarizeWorkStatus(status(["run:agent_run_b"]), [])?.total, 1);
});
