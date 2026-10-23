import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { reduceOutputWindow, OUTPUT_WINDOW_BYTES, type OutputWindow } from "../src/m5/agent-output-window.ts";
import type { OutputPage, OutputRow } from "../src/m5/contracts/agent-output-pages.ts";

const fixture: OutputPage = JSON.parse(readFileSync(new URL("./fixtures/m5/agent-output-page.json", import.meta.url), "utf8"));
const empty = (): OutputWindow => ({ binding: fixture.request.binding, cursor: 0, rows: [], hasEarlier: false });
function row(sequence: number, text: string, operation?: "append" | "complete", kind: OutputRow["event"]["kind"] = "status"): OutputRow {
  return { outputSequence: sequence, event: { ...fixture.rows[0]!.event, eventId: `agent_event_${sequence}`, kind, text },
    ...(operation ? { itemUpdate: { itemId: "answer", channel: "message" as const, operation } } : {}) };
}
test("deltas are replaced by completion and promoted without duplicate final", () => {
  let state = reduceOutputWindow(empty(), [row(1, "hel", "append"), row(2, "lo", "append")]);
  assert.equal(state.rows[0]!.text, "hello");
  state = reduceOutputWindow(state, [row(3, "hello", "complete"), row(4, "hello", "complete", "final")]);
  assert.equal(state.rows.length, 1);
  assert.equal(state.rows[0]!.kind, "final");
  assert.throws(() => reduceOutputWindow(state, [row(5, "late", "append")]));
});
test("three thousand events retain only a bounded window without losing cursor", () => {
  let state = empty();
  for (let seq = 1; seq <= 3000; seq++) state = reduceOutputWindow(state, [row(seq, "message")]);
  assert.equal(state.cursor, 3000);
  assert.equal(state.rows.length, 200);
  assert.equal(state.hasEarlier, true);
  for (let seq = 3001; seq <= 3100; seq++) state = reduceOutputWindow(state, [row(seq, "가".repeat(21000))]);
  assert.ok(state.rows.reduce((sum, row) => sum + row.textBytes, 0) <= OUTPUT_WINDOW_BYTES);
});
test("failed batch leaves original cursor and content intact", () => {
  const state = reduceOutputWindow(empty(), [row(1, "hello")]);
  assert.throws(() => reduceOutputWindow(state, [row(2, "next"), row(4, "gap")]));
  assert.equal(state.cursor, 1);
  assert.equal(state.rows.length, 1);
  const crossed = row(2, "wrong");
  crossed.event.runId = "agent_run_other";
  assert.throws(() => reduceOutputWindow(state, [crossed]));
});
