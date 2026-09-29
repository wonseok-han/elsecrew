import assert from "node:assert/strict";
import test from "node:test";
import { activeDockTab, defaultDockLayout, dockGeometry, focusDockPanel, moveDockPanel, PANEL_IDS, parseDockLayout, placeDockPanel, resizeDockPanels, resizeDockSplit, selectDockTab, toggleDockPanel, visibleDockPanels, type DockPlacement } from "../src/m5/dock-layout.ts";

test("nested split and tab groups preserve all panels and round-trip", () => {
  let layout = placeDockPanel(defaultDockLayout(), "changes", "files", "bottom");
  assert.ok(dockGeometry(layout, PANEL_IDS).separators.some(split => split.axis === "vertical"));
  layout = placeDockPanel(layout, "conversation", "editor", "tab");
  const group = dockGeometry(layout, PANEL_IDS).groups.find(group => group.panels.includes("editor"))!;
  assert.deepEqual(group.panels, ["editor", "conversation"]);
  assert.equal(group.active, "conversation");
  assert.equal(dockGeometry(selectDockTab(layout, "editor"), PANEL_IDS).groups.find(group => group.panels.includes("editor"))?.active, "editor");
  assert.deepEqual(parseDockLayout(JSON.stringify(layout)), layout);
  assert.deepEqual([...layout.order].sort(), [...PANEL_IDS].sort());
});
test("nested hidden groups collapse geometrically without losing saved placement", () => {
  const layout = placeDockPanel(defaultDockLayout(), "files", "editor", "top");
  const focused = focusDockPanel(layout, "editor");
  const geometry = dockGeometry(focused, PANEL_IDS);
  assert.deepEqual(geometry.groups.map(group => group.rect), [{ x: 0, y: 0, width: 1, height: 1 }]);
  assert.equal(geometry.separators.length, 0);
  assert.deepEqual(focused.tree, layout.tree);
  const reopened = toggleDockPanel(focused, "files", PANEL_IDS);
  assert.equal(dockGeometry(reopened, PANEL_IDS).groups.length, 2);
});
test("every placement keeps an exact partition, including moving a tab out of its group", () => {
  let layout = defaultDockLayout();
  for (const placement of ["tab", "left", "right", "top", "bottom"] as DockPlacement[]) {
    for (const source of PANEL_IDS) for (const target of PANEL_IDS) {
      layout = placeDockPanel(layout, source, target, placement);
      assert.deepEqual(parseDockLayout(JSON.stringify(layout)), layout);
      const geometry = dockGeometry(layout, PANEL_IDS);
      assert.deepEqual(geometry.groups.flatMap(group => group.panels).sort(), [...PANEL_IDS].sort());
      assert.ok(Math.abs(geometry.groups.reduce((area, group) => area + group.rect.width * group.rect.height, 0) - 1) < .000001);
    }
  }
});
test("nested split resize is bounded and restores correctly after legacy minimum widths", () => {
  const legacy = resizeDockPanels(defaultDockLayout(), "files", "editor", -100);
  const layout = selectDockTab(legacy, "files");
  assert.deepEqual(parseDockLayout(JSON.stringify(layout)), layout);
  const resized = resizeDockSplit(layout, "", 100);
  assert.equal(dockGeometry(resized, PANEL_IDS).separators[0].ratio, .9);
  assert.deepEqual(parseDockLayout(JSON.stringify(resized)), resized);
  assert.equal(resizeDockSplit(layout, "", NaN), layout);
});
test("malformed, duplicate and excessively deep trees restore defaults", () => {
  const initial = defaultDockLayout();
  let deep: unknown = { type: "tabs", panels: PANEL_IDS, active: "editor" };
  for (let depth = 0; depth < 10; depth++) deep = { type: "split", axis: "horizontal", ratio: .5, first: deep, second: { type: "tabs", panels: ["files"], active: "files" } };
  assert.deepEqual(parseDockLayout(JSON.stringify({ ...initial, tree: deep })), initial);
  for (const tree of [null, {}, { type: "tabs", panels: [...PANEL_IDS, "files"], active: "files" }, { type: "tabs", panels: PANEL_IDS, active: "missing" }, { type: "split", axis: "horizontal", ratio: .5, first: null, second: null }]) {
    assert.deepEqual(parseDockLayout(JSON.stringify({ ...initial, tree })), initial);
  }
});

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
