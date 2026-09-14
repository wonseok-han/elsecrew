import assert from "node:assert/strict";
import test from "node:test";
import { defaultDockLayout, moveDockPanel, parseDockLayout, resizeDockPanels } from "../src/m5/dock-layout.ts";

test("layout round-trip and invalid stored data recover to a complete default", () => {
  const initial = defaultDockLayout();
  assert.deepEqual(parseDockLayout(JSON.stringify(initial)), initial);
  for (const raw of [null, "{", "null", JSON.stringify({ ...initial, order: ["files", "files", "editor", "changes"] }), JSON.stringify({ ...initial, widths: { ...initial.widths, editor: -1 } }), JSON.stringify({ ...initial, version: 2 })]) assert.deepEqual(parseDockLayout(raw), initial);
});
test("panel movement in either direction preserves widths and all identities", () => {
  const initial = defaultDockLayout();
  const moved = moveDockPanel(initial, "files", "changes");
  assert.deepEqual(moved.order, ["editor", "changes", "files", "conversation"]);
  assert.deepEqual(moveDockPanel(moved, "files", "editor").order, initial.order);
  assert.deepEqual(moved.widths, initial.widths);
  assert.deepEqual(initial.order, ["files", "editor", "changes", "conversation"]);
});
test("resize clamps both neighbours without changing total or other panels", () => {
  const initial = defaultDockLayout();
  for (const delta of [-100, -.04, .04, 100]) {
    const resized = resizeDockPanels(initial, "files", "editor", delta);
    assert.ok(resized.widths.files >= .08 && resized.widths.editor >= .08 - Number.EPSILON);
    assert.ok(Math.abs(Object.values(resized.widths).reduce((a,b) => a+b,0)-1) < .000001);
    assert.equal(resized.widths.changes, initial.widths.changes);
    assert.deepEqual(parseDockLayout(JSON.stringify(resized)), resized);
  }
  assert.equal(resizeDockPanels(initial, "files", "editor", NaN), initial);
});
