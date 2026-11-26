import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { failStandingRead, finishStandingRead, standingView, startStandingRead, type StandingRead } from "../src/m5/standing-read.ts";
import { TaskDispositionStore } from "../src/m5/task-disposition.ts";
import { countMarkdownNodes, MARKDOWN_MAX_NODES, parseMarkdownWithLimits, tooLargeToDraw } from "../src/m5/markdown.ts";
import { noticesForChat } from "../src/m5/approval-display.ts";
import { taskPhase } from "../src/m5/task-status.ts";

const task = { chatId: "chat_copy", updatedAt: "2026-11-26T03:00:00Z" };
const receipt = { applied: false, pending: 2, total: 2 };

test("copy task: two reads in a row (apply pressed, changes reloaded) never drop its kind, so the apply panel stays mounted", () => {
  // The reviewer's order: a known copy task; "apply" asks for the confirmation
  // and empties then reloads the changes list, so a second read starts while
  // the first is still out.
  const known = finishStandingRead(startStandingRead(null, task, 1), { ...task, generation: 1, merge: { kind: "snapshot" }, apply: receipt }, 1);
  assert.equal(standingView(known, task).kind, "copy");
  const firstOut = startStandingRead(known, task, 2);
  assert.equal(standingView(firstOut, task).kind, "copy");
  const secondOut = startStandingRead(firstOut, task, 3);           // before the first answered
  assert.equal(standingView(secondOut, task).kind, "copy", "was null before: the apply panel unmounted and lost the click");
  assert.equal(standingView(secondOut, task).merge?.kind, "snapshot");
  // The first answer is late (dropped); the second answers.
  const stale = finishStandingRead(secondOut, { ...task, generation: 2, merge: { kind: "snapshot" }, apply: receipt }, 3);
  assert.equal(stale, secondOut);
  // Only the apply receipt fails: the kind read just before still counts.
  const applyFailed = failStandingRead(secondOut, { ...task, generation: 3, kind: "copy" }, 3);
  assert.equal(standingView(applyFailed, task).kind, "copy");
  assert.equal(standingView(applyFailed, task).failed, true);
  // A first read whose apply receipt fails still knows it is a copy task.
  const firstEver = failStandingRead(startStandingRead(null, task, 9), { ...task, generation: 9, kind: "copy" }, 9);
  assert.equal(standingView(firstEver, task).kind, "copy");
  // The apply panel is mounted on the kind, not on a read that is in flight.
  const workbench = readFileSync(new URL("../src/m5/workbench.tsx", import.meta.url), "utf8");
  assert.ok(workbench.includes('!phaseIsWorking(activePhase) && view.kind === "copy" && <ApplyLatest'));
  assert.ok(workbench.includes('!phaseIsWorking(activePhase) && view.kind === "git" && <ChangesMergeStatus'));
  assert.ok(workbench.includes("kind: readKind"), "a read whose apply part failed passes the kind it learnt");
});

test("#4: the open task's failed read reaches the list, and switching away never brings back its old 'merged'", async () => {
  const listed = { chatId: "chat_merged", updatedAt: "v1", state: "reviewReady" };
  const asked: string[] = [];
  const store = new TaskDispositionStore(async chatId => { asked.push(chatId); return { kind: "git", standing: "done" }; }, null);
  // The list read "merged" for this task version.
  store.request([listed]);
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.equal(store.get(listed)?.standing, "done");
  // The person opens it; its latest read fails (the workbench reports it).
  store.reportFailed(listed);
  // They switch to another task: the same version is not asked again...
  store.request([listed]);
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.deepEqual(asked, ["chat_merged"]);
  // ...and the row says "check failed", not the older "merged".
  assert.equal(store.failed(listed), true);
  const workbench = readFileSync(new URL("../src/m5/workbench.tsx", import.meta.url), "utf8");
  assert.ok(workbench.includes("if (chat.activeChat && view.failed) dispositions.reportFailed(chat.activeChat);"));
  assert.ok(workbench.includes('readFailed ? t("m5.task.readFailed")'));
  // Checking again clears it once a read succeeds.
  store.revalidate();
  store.request([listed]);
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.equal(store.failed(listed), false);
  assert.equal(taskPhase({ state: "reviewReady", disposition: store.get(listed) }), "merged");
});

test("#2: the 12,000 allowance holds across blocks, cells and the cells' inline pieces", () => {
  // The reviewer's case: a header of 8,000 cells (8,000 cells + 8,000 inline pieces).
  const wide = "|" + "a|".repeat(8_000) + "\n" + "|" + "-|".repeat(8_000);
  assert.ok(!tooLargeToDraw(wide));
  const parsed = parseMarkdownWithLimits(wide);
  assert.ok(countMarkdownNodes(parsed.blocks) <= MARKDOWN_MAX_NODES, `${countMarkdownNodes(parsed.blocks)}`);
  assert.equal(parsed.blocks.some(block => block.kind === "table"), false, "the table goes to original text");
  assert.equal(parsed.truncated, true);
  // Spread over rows: 70 rows of 100 cells, with formatted cell contents.
  const row = "|" + "**b**|".repeat(100);
  const tall = [row, "|" + "-|".repeat(100), ...Array.from({ length: 70 }, () => row)].join("\n");
  const tallParsed = parseMarkdownWithLimits(tall);
  assert.ok(countMarkdownNodes(tallParsed.blocks) <= MARKDOWN_MAX_NODES, `${countMarkdownNodes(tallParsed.blocks)}`);
  assert.equal(tallParsed.truncated, true);
  // A small table still draws whole.
  const small = parseMarkdownWithLimits("|a|**b**|\n|-|-|\n|1|2|");
  assert.equal(small.blocks[0]!.kind, "table");
  assert.equal(small.truncated, false);
});

test("N5: a budget that runs out while reading the last link still says formatting stopped", () => {
  // No bracket follows the link whose destination used up the budget.
  const lastLink = "보고서 요약입니다.\n\n[전체 보고서](https://example.com/" + "u".repeat(70_000) + ")";
  assert.ok(!tooLargeToDraw(lastLink));
  const parsed = parseMarkdownWithLimits(lastLink);
  assert.equal(parsed.truncated, true);
  // A short document with an ordinary last link is not marked.
  assert.equal(parseMarkdownWithLimits("요약\n\n[전체 보고서](https://example.com/r)").truncated, false);
});

test("a late 'answer not taken' from a task left behind never shows in another conversation", () => {
  const notices = [
    { chatId: "chat_old", approvalId: "approval_1", kind: "command" as const, summary: "npm test", answerFailed: true },
    { chatId: "chat_new", approvalId: "approval_2", kind: "tool" as const, summary: null },
  ];
  assert.deepEqual(noticesForChat(notices, "chat_new").map(item => item.approvalId), ["approval_2"]);
  assert.deepEqual(noticesForChat(notices, "chat_old").map(item => item.approvalId), ["approval_1"]);
  assert.deepEqual(noticesForChat(notices, null), []);
  // Every notice the card sends names its own task, and the conversation filters on it.
  const card = readFileSync(new URL("../src/m5/approval-card.tsx", import.meta.url), "utf8");
  assert.equal((card.match(/chatId: identity\.chatId/g) ?? []).length, 3);
  const workbench = readFileSync(new URL("../src/m5/workbench.tsx", import.meta.url), "utf8");
  assert.ok(workbench.includes("const mine = noticesForChat(items, timelineChat.current);"));
  assert.ok(workbench.includes("noticesForChat(withdrawnAtEnd, chat.activeChat?.chatId ?? null).map(item =>"));
});

test("standing reads keep a task's kind but never carry it to another task", () => {
  const copy = finishStandingRead(startStandingRead(null, task, 1), { ...task, generation: 1, merge: { kind: "snapshot" }, apply: receipt }, 1) as StandingRead;
  const other = startStandingRead(copy, { chatId: "chat_git", updatedAt: "v1" }, 2);
  assert.equal(other.kind, null);
  assert.equal(standingView(copy, { chatId: "chat_git", updatedAt: "v1" }).kind, null);
});
