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

import { elapsedParts, groupStartsFolded, groupTaskRows, relativeTime, rowGroup } from "../src/m5/task-list-model.ts";
import { taskPhase, type TaskPhase } from "../src/m5/task-status.ts";

test("every task status lands in its list group, in the plan's order", () => {
  const table: [TaskPhase, string][] = [
    ["attention", "attention"], ["running", "working"], ["draft", "working"],
    ["review", "review"], ["changedSince", "review"],
    ["merged", "done"], ["applied", "done"], ["answered", "done"], ["stopped", "done"], ["discarded", "done"],
  ];
  for (const [phase, group] of table) assert.equal(rowGroup({ phase, readFailed: false }), group, phase);
  assert.equal(rowGroup({ phase: "merged", readFailed: true }), "review", "an unread standing waits for a look");
});

test("the diagnosis' five states sit where the 1차-가 status puts them", () => {
  const git = (standing: "notYet" | "done" | "changedSince") => ({ kind: "git" as const, standing });
  const copy = (standing: "notYet" | "done" | "changedSince") => ({ kind: "copy" as const, standing });
  assert.equal(rowGroup({ phase: taskPhase({ state: "running" }), readFailed: false }), "working");
  assert.equal(rowGroup({ phase: taskPhase({ state: "running", approvalPending: true }), readFailed: false }), "attention");
  assert.equal(rowGroup({ phase: taskPhase({ state: "reviewReady", disposition: git("notYet") }), readFailed: false }), "review");
  assert.equal(rowGroup({ phase: taskPhase({ state: "reviewReady", disposition: git("done") }), readFailed: false }), "done");
  assert.equal(rowGroup({ phase: taskPhase({ state: "completed", disposition: copy("done") }), readFailed: false }), "done");
  assert.equal(rowGroup({ phase: taskPhase({ state: "reviewReady", disposition: copy("changedSince") }), readFailed: false }), "review");
  assert.equal(rowGroup({ phase: taskPhase({ state: "running", disposition: git("done") }), readFailed: false }), "working", "working again after a merge");
});

test("groups come in order, newest first, and empty groups are left out", () => {
  const rows = [
    { chatId: "a", phase: "merged" as const, readFailed: false, updatedAt: "2026-11-25T10:00:00Z" },
    { chatId: "b", phase: "review" as const, readFailed: false, updatedAt: "2026-11-25T09:00:00Z" },
    { chatId: "c", phase: "review" as const, readFailed: false, updatedAt: "2026-11-25T11:00:00Z" },
    { chatId: "d", phase: "attention" as const, readFailed: false, updatedAt: "bad" },
  ];
  const groups = groupTaskRows(rows);
  assert.deepEqual(groups.map(entry => entry.group), ["attention", "review", "done"]);
  assert.deepEqual(groups[1]!.rows.map(row => row.chatId), ["c", "b"]);
  assert.deepEqual(groupTaskRows([]), []);
  assert.equal(groupStartsFolded("done", false), true);
  assert.equal(groupStartsFolded("done", true), false, "the open task's group stays open");
  assert.equal(groupStartsFolded("review", false), false);
});

test("row times read as just now, minutes, hours, yesterday or a date", () => {
  const now = new Date(2026, 10, 26, 15, 0, 0).getTime();
  assert.deepEqual(relativeTime(new Date(now - 20_000).toISOString(), now), { key: "m5.when.now" });
  assert.deepEqual(relativeTime(new Date(now - 5 * 60_000).toISOString(), now), { key: "m5.when.minutes", n: 5 });
  assert.deepEqual(relativeTime(new Date(now - 3 * 3600_000).toISOString(), now), { key: "m5.when.hours", n: 3 });
  assert.deepEqual(relativeTime(new Date(2026, 10, 25, 20, 0).toISOString(), now), { key: "m5.when.yesterday" });
  assert.equal(relativeTime(new Date(2026, 10, 20).toISOString(), now)?.key, "m5.when.date");
  assert.equal(relativeTime("nope", now), null);
  assert.deepEqual(elapsedParts(42), { key: "m5.when.forSeconds", n: 42 });
  assert.deepEqual(elapsedParts(125), { key: "m5.when.forMinutes", n: 2 });
});
