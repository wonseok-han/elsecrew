import assert from "node:assert/strict";
import test from "node:test";
import { activeDockTab, defaultDockLayout, focusDockPanel, moveDockPanel, PANEL_IDS, parseDockLayout, resizeDockPanels, toggleDockPanel, visibleDockPanels } from "../src/m5/dock-layout.ts";

test("tabs persist selection and recover when a hidden or unavailable tab disappears", () => {
  const layout = { ...defaultDockLayout(), tabbed: true, activeTab: "conversation" as const };
  assert.deepEqual(parseDockLayout(JSON.stringify(layout)), layout);
  assert.equal(activeDockTab(layout, PANEL_IDS), "conversation");
  assert.equal(activeDockTab(layout, ["files", "editor", "changes"]), "editor");
  assert.equal(activeDockTab(layout, ["files"]), "files");
  assert.equal(activeDockTab(layout, []), undefined);
  const hidden = toggleDockPanel(layout, "conversation", PANEL_IDS);
  assert.equal(activeDockTab(hidden, visibleDockPanels(hidden, PANEL_IDS)), "editor");
  for (const invalid of [{ tabbed: "yes" }, { activeTab: "missing" }, { activeTab: null }]) {
    assert.deepEqual(parseDockLayout(JSON.stringify({ ...layout, ...invalid })), defaultDockLayout());
  }
});

test("panel visibility persists without allowing an empty workspace", () => {
  const initial = defaultDockLayout();
  const focused = focusDockPanel(initial, "editor");
  assert.deepEqual(visibleDockPanels(focused, PANEL_IDS), ["editor"]);
  assert.equal(toggleDockPanel(focused, "editor", PANEL_IDS), focused);
  const reopened = toggleDockPanel(focused, "files", PANEL_IDS);
  assert.deepEqual(visibleDockPanels(reopened, PANEL_IDS), ["files", "editor"]);
  assert.deepEqual(parseDockLayout(JSON.stringify(reopened)), reopened);
  assert.deepEqual(reopened.widths, initial.widths);
  assert.deepEqual(reopened.order, initial.order);
  assert.equal(initial.hidden, undefined);
});
test("leaving a focused conversation retains editor access and can reopen panels", () => {
  const available = ["files", "editor", "changes"] as const;
  const layout = focusDockPanel(defaultDockLayout(), "conversation");
  assert.deepEqual(visibleDockPanels(layout, available), ["editor"]);
  const reopened = toggleDockPanel(layout, "files", available);
  assert.deepEqual(visibleDockPanels(reopened, available), ["files", "editor"]);
  assert.equal(toggleDockPanel(layout, "conversation", available), layout);
});
test("invalid visibility is discarded and older saved layouts remain readable", () => {
  for (const hidden of ["editor", ["unknown"], PANEL_IDS, ["files", "files"], null]) {
    assert.deepEqual(parseDockLayout(JSON.stringify({...defaultDockLayout(),hidden})), defaultDockLayout());
  }
  assert.deepEqual(parseDockLayout(JSON.stringify(defaultDockLayout())), defaultDockLayout());
});

test("layout round-trip and invalid stored data recover to a complete default", () => {
  const initial = defaultDockLayout();
  assert.deepEqual(parseDockLayout(JSON.stringify(initial)), initial);
  for (const raw of [null, "{", "null", JSON.stringify({ ...initial, order: ["files", "files", "editor", "changes"] }), JSON.stringify({ ...initial, widths: { ...initial.widths, editor: -1 } }), JSON.stringify({ ...initial, version: 2 })]) assert.deepEqual(parseDockLayout(raw), initial);
});
test("vertical layout round-trips with resizing, movement and visibility intact", () => {
  const layout = { ...defaultDockLayout(), direction: "vertical" as const };
  const moved = resizeDockPanels(moveDockPanel(layout, "editor", "files"), "editor", "files", .04);
  const focused = focusDockPanel(moved, "editor");
  assert.deepEqual(parseDockLayout(JSON.stringify(focused)), focused);
  for (const direction of [null, "diagonal", 1, {}]) {
    assert.deepEqual(parseDockLayout(JSON.stringify({ ...layout, direction })), defaultDockLayout());
  }
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
