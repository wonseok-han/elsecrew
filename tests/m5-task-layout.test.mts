import assert from "node:assert/strict";
import test from "node:test";
import { bandSizes, bandUsesDrawer, clampRightWidth, defaultRightTab, keyedRightWidth, layoutBand, panelRequestTarget, parseRightWidth, previewObscured, rovingIndex, RIGHT_WIDTH_MIN } from "../src/m5/task-layout-model.ts";

test("window widths fall into the four bands of the layout plan", () => {
  assert.equal(layoutBand(1680), "wide");
  assert.equal(layoutBand(1500), "wide");
  assert.equal(layoutBand(1499), "regular");
  assert.equal(layoutBand(1280), "regular");
  assert.equal(layoutBand(1100), "regular");
  assert.equal(layoutBand(1099), "narrow");
  assert.equal(layoutBand(960), "narrow");
  assert.equal(layoutBand(820), "narrow");
  assert.equal(layoutBand(819), "compact");
  assert.equal(layoutBand(640), "compact");
  assert.equal(layoutBand(Number.NaN), "wide");
  assert.equal(bandUsesDrawer("regular"), false);
  assert.equal(bandUsesDrawer("narrow"), true);
  assert.equal(bandUsesDrawer("compact"), true);
});

test("at 1280×768 the story column keeps half the window", () => {
  const { left, right } = bandSizes(layoutBand(1280));
  assert.equal(1280 - left - right, 640);
  assert.ok((1280 - left - right) / 1280 >= 0.5);
  const wide = bandSizes(layoutBand(1680));
  assert.equal(1680 - wide.left - wide.right, 896);
});

test("the results panel is 320px up to half the window and stored as one number", () => {
  assert.equal(clampRightWidth(100, 1280), RIGHT_WIDTH_MIN);
  assert.equal(clampRightWidth(900, 1280), 640);
  assert.equal(clampRightWidth(450.4, 1280), 450);
  assert.equal(clampRightWidth(Number.NaN, 1280), RIGHT_WIDTH_MIN);
  assert.equal(clampRightWidth(500, 500), RIGHT_WIDTH_MIN, "never below the minimum on a tiny window");
  assert.equal(parseRightWidth("420"), 420);
  assert.equal(parseRightWidth("12"), null);
  assert.equal(parseRightWidth("420px"), null);
  assert.equal(parseRightWidth(null), null);
  assert.equal(keyedRightWidth(400, "ArrowLeft", false, 1280), 416, "left widens the panel");
  assert.equal(keyedRightWidth(400, "ArrowRight", true, 1280), 336);
  assert.equal(keyedRightWidth(400, "End", false, 1280), RIGHT_WIDTH_MIN);
  assert.equal(keyedRightWidth(400, "Home", false, 1280), 640);
  assert.equal(keyedRightWidth(400, "a", false, 1280), null);
});

test("editor panel requests lead to the sheet, a results tab, the story or the list", () => {
  assert.deepEqual(panelRequestTarget("editor", false), { kind: "sheet", mode: "editor" });
  assert.deepEqual(panelRequestTarget("editor", true), { kind: "sheet", mode: "changes" });
  assert.deepEqual(panelRequestTarget("changes", false), { kind: "tab", tab: "changes" });
  assert.deepEqual(panelRequestTarget("files", true), { kind: "tab", tab: "files" });
  assert.deepEqual(panelRequestTarget("conversation", false), { kind: "story" });
  assert.deepEqual(panelRequestTarget("chats", false), { kind: "list" });
});

test("a task opens on the tab its state is about", () => {
  assert.equal(defaultRightTab("review", true), "preview");
  assert.equal(defaultRightTab("review", false), "changes");
  assert.equal(defaultRightTab("merged", false), "changes");
  assert.equal(defaultRightTab("changedSince", false), "changes");
  assert.equal(defaultRightTab("running", false), "changes");
  assert.equal(defaultRightTab("draft", false), "files");
  assert.equal(defaultRightTab(null, false), "files");
  assert.equal(defaultRightTab("stopped", true), "history");
  assert.equal(defaultRightTab("discarded", false), "history");
});

test("results tabs move with arrows, Home and End", () => {
  assert.equal(rovingIndex(4, 0, "ArrowRight"), 1);
  assert.equal(rovingIndex(4, 3, "ArrowRight"), 0);
  assert.equal(rovingIndex(4, 0, "ArrowLeft"), 3);
  assert.equal(rovingIndex(4, 2, "Home"), 0);
  assert.equal(rovingIndex(4, 0, "End"), 3);
  assert.equal(rovingIndex(4, 0, "Enter"), null);
  assert.equal(rovingIndex(0, 0, "ArrowRight"), null);
});

test("the preview counts as covered by the sheet, a closed drawer, dialogs and menus", () => {
  const open = { sheetOpen: false, drawerUsed: false, drawerOpen: false, dialogOpen: false, popoverOpen: false, rightTab: "preview" as const };
  assert.equal(previewObscured(open), false);
  assert.equal(previewObscured({ ...open, sheetOpen: true }), true);
  assert.equal(previewObscured({ ...open, dialogOpen: true }), true);
  assert.equal(previewObscured({ ...open, popoverOpen: true }), true);
  assert.equal(previewObscured({ ...open, rightTab: "changes" }), true);
  assert.equal(previewObscured({ ...open, drawerUsed: true }), true, "a closed drawer");
  assert.equal(previewObscured({ ...open, drawerUsed: true, drawerOpen: true }), false);
});
