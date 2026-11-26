import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import type { AgentRunRecordV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import { failStandingRead, standingView, startStandingRead } from "../src/m5/standing-read.ts";
import { resultBarView, standingRead, taskPhase } from "../src/m5/task-status.ts";
import { stageCells } from "../src/m5/task-story-model.ts";
import { approvalCovered, changesLine, escapeTarget, resultBoxMode, revealResults, teamWorking } from "../src/m5/task-view-model.ts";
import { isLeadRole, roleColorIndex } from "../src/m5/role-colors.ts";
import { workbenchSource } from "./m5-workbench-source.mts";

/** PR #5 independent review (REVIEW-pr5.md): each finding's reproduction, kept as a test. */
const source = (name: string) => readFileSync(new URL(`../src/m5/${name}`, import.meta.url), "utf8");
const now = Date.UTC(2026, 10, 26, 10, 10, 0);
const run = (state: string, terminationOutcome: AgentRunRecordV1["terminationOutcome"] = null) => ({
  runId: "run_1", state, terminationOutcome, selectedProfile: { roleId: "coding" },
  createdAt: new Date(now - 120_000).toISOString(), updatedAt: new Date(now - 60_000).toISOString(),
}) as unknown as AgentRunRecordV1;

test("review #1: a waiting permission request is announced over the sheet and the drawer, and opening it uncovers the card", () => {
  assert.equal(approvalCovered({ waiting: true, sheetOpen: true, drawerOpen: false }), true, "sheet open over the card");
  assert.equal(approvalCovered({ waiting: true, sheetOpen: false, drawerOpen: true }), true, "drawer and its scrim over the card");
  assert.equal(approvalCovered({ waiting: true, sheetOpen: false, drawerOpen: false }), false, "the card itself is reachable");
  assert.equal(approvalCovered({ waiting: false, sheetOpen: true, drawerOpen: true }), false);
  const layout = source("task-layout.tsx");
  assert.ok(layout.includes("{drawerActive && approvalNotice}"), "the drawer shows the notice");
  assert.ok(layout.includes("{sheetOpen && approvalNotice}"), "the sheet shows the notice");
  const workbench = source("workbench.tsx");
  assert.ok(workbench.includes("approvalCovered({ waiting: approvalWaitingNow, sheetOpen, drawerOpen: bandUsesDrawer(layoutBandNow) && drawerOpen })"));
  assert.match(workbench, /const revealStory = \(\) => \{\n    setSheetOpen\(false\); setAllChanges\(null\); setDrawerOpen\(false\);/, "the notice closes the sheet and the drawer");
  assert.ok(workbench.includes("[data-testid=pinned-approval] button:not(:disabled)"), "and moves focus to the request");
  // The open task's permission notification no longer returns without doing anything.
  assert.match(workbench, /if \(chat\.activeChat\?\.chatId !== item\.chatId\) void switchChat\(item\.chatId\);\n      else revealStoryRef\.current\(\);/);
});

test("review #2: the guide's apply path opens the result box, and changes or files reopen a hidden panel or the drawer", () => {
  assert.deepEqual(revealResults(true), { drawerOpen: true, rightCollapsed: false }, "below 1100px: open the drawer");
  assert.deepEqual(revealResults(false), { drawerOpen: false, rightCollapsed: false }, "wide: undo ⌘\\ hiding");
  const workbench = source("workbench.tsx");
  assert.equal((workbench.match(/onClick=\{onDecision\}>\{t\("m5\.guide\.openDecision"\)\}/g) ?? []).length, 2, "both guide buttons for applying or recovering");
  assert.ok(!workbench.includes('onClick={onConversation}>{t("m5.guide.openDecision")}'), "never the conversation");
  assert.ok(workbench.includes("onDecision={() => focusResultBox()}"));
  assert.match(workbench, /const focusResultBox = \(\) => \{\n    setSheetOpen\(false\); setAllChanges\(null\);\n    showResults\(\);/);
  assert.match(workbench, /if \(target\.kind === "tab"\) \{\n      setRightTab\(target\.tab\);\n      showResults\(\);/, "a changes or files request opens the panel first");
});

test("review #3: a task waiting for recovery is not shown as the team working", () => {
  // The review's case: one run left needing recovery, the task needs a check.
  const runs = [run("recoveryRequired", "recoveryRequired")];
  const phase = taskPhase({ state: "recoveryRequired" });
  assert.equal(phase, "attention");
  const working = teamWorking("recoveryRequired");
  assert.equal(working, false);
  const cells = stageCells(runs, { working, now }).cells;
  assert.deepEqual(cells.map(cell => cell.kind === "run" ? cell.status : cell.kind), ["recovery"], "no 'picking who goes next' cell");
  assert.equal(resultBoxMode({ phase, working, switching: false, hasView: resultBarView({ phase, kind: "git", known: false }) !== null }), "attention", "not 'put it in once the team is done'");
  assert.equal(changesLine({ phase, working, kind: "git", known: false, failed: false, changeCount: 2 }), "attention", "not 'the team is working'");
  assert.equal(teamWorking("needsAttention"), false);
  // A permission request while the team runs is still work in progress.
  const asking = taskPhase({ state: "running", approvalPending: true });
  assert.equal(asking, "attention");
  assert.equal(teamWorking("running"), true);
  assert.deepEqual(stageCells([run("running")], { working: true, now }).cells.map(cell => cell.kind === "run" ? cell.status : cell.kind), ["now", "next"]);
  assert.equal(resultBoxMode({ phase: asking, working: true, switching: false, hasView: false }), "waiting");
  assert.equal(changesLine({ phase: asking, working: true, kind: null, known: false, failed: false, changeCount: 2 }), "working");
  assert.equal(teamWorking("reviewReady", true), true, "a run being started counts");
  const story = source("task-story.tsx");
  assert.ok(story.includes("const working = teamWorking(chat.activeChat?.state, startingActual);"));
  assert.ok(!story.includes('phase === "attention" || chat.activeChat?.state === "running"'));
});

test("review #4: an unread apply receipt never reads as 'not in your folder yet'", () => {
  // The review's case: a copy task waiting for review whose receipt read fails.
  const task = { chatId: "chat_copy", updatedAt: "2026-11-26T03:00:00Z" };
  const failed = failStandingRead(startStandingRead(null, task, 1), { ...task, generation: 1, kind: "copy" }, 1);
  const view = standingView(failed, task);
  assert.equal(view.failed, true);
  assert.equal(view.kind, "copy");
  const phase = taskPhase({ state: "reviewReady", disposition: view.disposition });
  assert.equal(phase, "review");
  assert.equal(resultBarView({ phase, kind: view.kind, known: standingRead({ disposition: null, failed: true }), failed: true })?.detailKey, "m5.task.result.readFailed");
  assert.equal(changesLine({ phase, working: false, kind: "copy", known: false, failed: true, changeCount: 3 }), "failed", "was notYet before the fix");
  assert.equal(changesLine({ phase, working: false, kind: "copy", known: false, failed: false, changeCount: 3 }), "loading", "still reading: nothing claimed");
  assert.equal(changesLine({ phase: "review", working: false, kind: "copy", known: true, failed: false, changeCount: 3 }), "notYet");
  assert.equal(changesLine({ phase: "applied", working: false, kind: "copy", known: true, failed: false, changeCount: 3 }), "done");
  const workbench = source("workbench.tsx");
  assert.ok(workbench.includes("const readFailed = selected ? activeReadFailed : dispositions.failed(item);"), "the selected row says so too");
  assert.ok(workbench.includes("failed: activeReadFailed, changeCount"), "the changes tab line reads the failure");
  assert.ok(workbench.includes("changesLineSettled && !phaseIsWorking(activePhase) && view.kind === \"copy\" && <ChangesStandingLine"));
  // The 1차-가 guards stay.
  const all = workbenchSource();
  assert.ok(all.includes('!phaseIsWorking(activePhase) && view.kind === "copy" && <ApplyLatest'));
  assert.ok(all.includes('!phaseIsWorking(activePhase) && view.kind === "git" && <ChangesMergeStatus'));
});

test("review #5: one Escape closes only the innermost thing, and the drawer and sheet manage focus", () => {
  // The review's case: a comparison menu open inside the sheet.
  const first = escapeTarget({ handled: false, menuOpen: true, dialogOpen: false, sheetOpen: true, drawerOpen: false });
  assert.equal(first, "menu", "the first Escape is the menu's");
  assert.equal(escapeTarget({ handled: true, menuOpen: false, dialogOpen: false, sheetOpen: true, drawerOpen: false }), "none", "the menu handler used it up");
  assert.equal(escapeTarget({ handled: false, menuOpen: false, dialogOpen: false, sheetOpen: true, drawerOpen: true }), "sheet", "the next Escape closes the sheet");
  assert.equal(escapeTarget({ handled: false, menuOpen: false, dialogOpen: false, sheetOpen: false, drawerOpen: true }), "drawer");
  assert.equal(escapeTarget({ handled: false, menuOpen: false, dialogOpen: true, sheetOpen: true, drawerOpen: false }), "none", "an open dialog keeps its own Escape");
  const workbench = source("workbench.tsx");
  assert.ok(workbench.includes("if (menu.open) { menu.open = false; event.preventDefault();"), "closing a menu marks the key handled");
  const layout = source("task-layout.tsx");
  assert.ok(layout.includes("const target = escapeTarget({"));
  assert.ok(layout.includes('pick("[data-sheet-focus]") ?? pick(".m5-sheet-file")'), "the picked file before the mode buttons");
  assert.match(layout, /if \(drawerActive\) \{\n      if \(!drawerOpener\.current\) drawerOpener\.current = activeElement\(\);/, "the drawer remembers its opener");
  assert.ok(layout.includes('root.querySelector<HTMLElement>("[role=tab][aria-selected=true]")?.focus();'), "and focuses its open tab");
  assert.match(layout, /const back = drawerOpener\.current;\n      drawerOpener\.current = null;\n      if \(back\.isConnected && !sheetOpen\) back\.focus\(\);/, "and gives focus back on close");
});

test("review minor: roles named by the person never share the coordinator's colour or mark; rail and narrow sheet", () => {
  for (let index = 0; index < 2000; index++) assert.notEqual(roleColorIndex(`member-${index.toString(36)}`), 1);
  assert.equal(roleColorIndex("coordinator"), 1);
  assert.equal(isLeadRole("coordinator"), true);
  assert.equal(isLeadRole("member-coordinator"), false);
  assert.ok(source("member-card.tsx").includes("<bdi>{name}</bdi>"), "names are isolated");
  const list = source("task-list.tsx");
  assert.ok(list.includes("aria-expanded={railOpen === group}"), "each rail dot has its own open state");
  assert.ok(list.includes("shownGroups"), "and opens its own group");
  assert.ok(source("review-sheet.tsx").includes('data-testid="sheet-file-select"'), "a narrow sheet keeps a file picker");
  for (const name of ["workbench.tsx", "task-layout.tsx", "task-list.tsx", "review-sheet.tsx", "result-box.tsx", "styles.css"]) assert.ok(!source(name).endsWith("\n\n"), `${name} ends with a blank line`);
});
