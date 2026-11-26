import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import test from "node:test";
import ts from "typescript";
import { renderToStaticMarkup } from "react-dom/server";
import { failStandingRead, finishStandingRead, standingView, startStandingRead, type StandingRead } from "../src/m5/standing-read.ts";
import { resultBarView, taskPhase } from "../src/m5/task-status.ts";
import { TaskDispositionStore } from "../src/m5/task-disposition.ts";
import { MARKDOWN_MAX_NODES, parseInline, parseMarkdownWithLimits, tooLargeToDraw } from "../src/m5/markdown.ts";
import * as contract from "../src/m5/contracts/approval-v1.ts";
import * as display from "../src/m5/approval-display.ts";
import { ko } from "../src/locales/ko.ts";
import type { ChatMergeState } from "../src/m5/contracts/project-catalog-v1.ts";
import { workbenchSource } from "./m5-workbench-source.mts";

const require = createRequire(import.meta.url);
const git = (over: Partial<Extract<ChatMergeState, { kind: "gitWorktree" }>> = {}): ChatMergeState => ({
  kind: "gitWorktree", branch: "elsecrew/task-1", baseRef: "main", head: "a".repeat(40), baseTip: "b".repeat(40),
  ahead: 0, behind: 0, uncommitted: false, baseCheckedOut: true, resolving: false, unresolved: 0, mergedBefore: false, blocked: null, ...over,
});
const task = { chatId: "chat_a", updatedAt: "2026-11-26T01:00:00Z" };

/** One read cycle, as the workbench runs it. */
function read(previous: StandingRead | null, generation: number, outcome: { merge: ChatMergeState } | "fail", latest = generation, target = task) {
  const started = startStandingRead(previous, target, generation);
  return outcome === "fail" ? failStandingRead(started, { ...target, generation }, latest)
    : finishStandingRead(started, { ...target, generation, merge: outcome.merge, apply: null }, latest);
}

test("N2: after 'nothing to merge', a failed re-read shows the failure bar, not 'answered'", () => {
  const nothing = read(null, 1, { merge: git() });
  assert.equal(standingView(nothing, task).disposition?.standing, "none");
  // The person edits files; the next read fails.
  const failed = read(nothing, 2, "fail");
  const view = standingView(failed, task);
  assert.equal(view.failed, true);
  assert.equal(view.disposition, null, "the earlier 'nothing to merge' no longer counts");
  const phase = taskPhase({ state: "waitingForUser", disposition: view.disposition });
  const bar = resultBarView({ phase, kind: null, known: true, failed: view.failed });
  assert.equal(bar?.failed, true, "the bar and 'look again' stay");
});

test("#4: after 'merged', a failed re-read never keeps showing 'merged'", () => {
  const merged = read(null, 1, { merge: git({ mergedBefore: true }) });
  assert.equal(standingView(merged, task).disposition?.standing, "done");
  const failed = read(merged, 2, "fail");
  assert.equal(standingView(failed, task).disposition, null);
  assert.equal(taskPhase({ state: "reviewReady", disposition: standingView(failed, task).disposition }), "review");
  // While a re-read is out, the same task version's last known value shows.
  const reading = startStandingRead(merged, task, 3);
  assert.equal(standingView(reading, task).disposition?.standing, "done");
  assert.equal(standingView(reading, task).reading, true);
});

test("N3: an answer from before the team restarted, a switch or a newer read is dropped", () => {
  // Read 1 is out; the team starts again (the workbench moves the generation on and clears).
  const out = startStandingRead(null, task, 1);
  assert.equal(finishStandingRead(null, { ...task, generation: 1, merge: git({ mergedBefore: true }), apply: null }, 2), null, "cleared state stays clear");
  assert.equal(finishStandingRead(out, { ...task, generation: 1, merge: git({ mergedBefore: true }), apply: null }, 2), out, "a stale generation is ignored");
  // Switched to another task: an answer for the first is not taken.
  const other = startStandingRead(out, { chatId: "chat_b", updatedAt: task.updatedAt }, 3);
  assert.equal(finishStandingRead(other, { ...task, generation: 3, merge: git(), apply: null }, 3), other);
  // Same task, newer version (it ran again): an answer for the older version is not taken.
  const newer = startStandingRead(out, { ...task, updatedAt: "2026-11-26T02:00:00Z" }, 4);
  assert.equal(finishStandingRead(newer, { ...task, generation: 4, merge: git(), apply: null }, 4), newer);
  assert.equal(standingView(newer, task).disposition, null, "a view for another version shows nothing");
  // A failure from an old read does not override a current one either.
  const current = read(null, 5, { merge: git() });
  assert.equal(failStandingRead(current, { ...task, generation: 4 }, 5), current);
  // In the workbench, every way out of the reading effect moves the generation
  // on, and the panels' own reads never write the standing.
  const source = workbenchSource();
  const effect = source.slice(source.indexOf("// Reading the open task's standing runs on its own"), source.indexOf("const dispositionKnown = standingRead"));
  assert.ok(effect.includes("const leave = () => { standingGeneration.current++; };"));
  assert.equal((effect.match(/return leave;/g) ?? []).length, 2, "both the early and the reading branch invalidate on leave");
  assert.ok(effect.includes("if (activeWorking) setStanding(null);"));
  assert.ok(!source.includes("onState={onMergeState}") && !source.includes("onInspected={onApplyInspected}"), "one owner of the standing");
  assert.ok(source.includes("standing?.status === \"known\" && view.disposition"), "only a current, known read is kept for the list");
});

test("N4: a queued re-check from an older round is skipped before anything is sent", async () => {
  const calls: string[] = [];
  let releaseFirst: () => void = () => {};
  const store = new TaskDispositionStore(chatId => {
    calls.push(chatId);
    if (calls.length === 1) return new Promise(resolve => { releaseFirst = () => resolve({ kind: "git", standing: "done" }); });
    return Promise.resolve({ kind: "git", standing: "done" });
  }, null);
  const a = { chatId: "chat_a", updatedAt: "v1", state: "reviewReady" };
  const b = { chatId: "chat_b", updatedAt: "v1", state: "reviewReady" };
  store.request([a, b]);                       // A is asked, B waits behind it
  await new Promise(resolve => setTimeout(resolve, 5));
  store.revalidate();                          // "check again" before A answers
  store.request([a, b]);
  releaseFirst();
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.deepEqual(calls, ["chat_a", "chat_a", "chat_b"], "the older round's B is never sent");
});

test("#2: a header too wide for the allowance turns the whole table into original text", () => {
  // The reviewer's case: 78,003 characters on 2 lines, 13,000 header cells.
  const header = "|" + "a|".repeat(13_000);
  const table = header + "\n" + "|" + "-|".repeat(13_000);
  assert.ok(!tooLargeToDraw(table));
  const { blocks, truncated } = parseMarkdownWithLimits(table);
  assert.equal(blocks.some(block => block.kind === "table"), false, "no table is drawn");
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0]!.kind, "paragraph");
  assert.equal(truncated, true);
  // A header that fits draws, and its cells count against the allowance.
  const small = parseMarkdownWithLimits("|a|b|\n|-|-|\n|1|2|");
  assert.equal(small.blocks[0]!.kind, "table");
  assert.ok(MARKDOWN_MAX_NODES < 13_000);
});

test("N5: plain bracketed words do not use up the link budget; a spent budget says so", () => {
  // The reviewer's case: a long log of "[passed]" lines with a real link at the end.
  const log = "결과 [통과] 확인함\n".repeat(1_600) + "[전체 보고서](https://example.com/report)";
  assert.ok(log.length > 18_000);
  const inline = parseInline(log);
  const last = inline[inline.length - 1]!;
  assert.equal(last.kind, "link");
  assert.equal((last as { href: string | null }).href, "https://example.com/report");
  assert.deepEqual((last as { children: unknown }).children, [{ kind: "text", text: "전체 보고서" }]);
  // When the budget really runs out, the document is marked as stopped.
  const spent = parseMarkdownWithLimits("[" + "x".repeat(70_000) + "\n\n[a](https://example.com)");
  assert.equal(spent.truncated, true);
});

test("#8: an answer still on its way when the run ends is said by the conversation if it fails", () => {
  const compiled = ts.transpileModule(readFileSync(new URL("../src/m5/approval-card.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020 },
  }).outputText;
  const context: any = { exports: {}, require(name: string) {
    if (name === "react") return { useEffect() {}, useRef: (value: unknown) => ({ current: value }), useState: (initial: unknown) => [initial, () => {}] };
    if (name === "../i18n.tsx") return { useI18n: () => ({ t(key: keyof typeof ko) { return ko[key]; } }) };
    if (name === "./contracts/approval-v1.ts") return contract;
    if (name === "./approval-display.ts") return display;
    return require(name);
  } };
  runInNewContext(compiled, context);
  const failed = renderToStaticMarkup(context.exports.ApprovalWithdrawnNote({ item: { chatId: "chat_a", approvalId: "approval_a", kind: "command", summary: "npm test", answerFailed: true } }));
  assert.ok(failed.includes(renderToStaticMarkup(ko["m5.approval.answerFailed"])));
  assert.ok(!failed.includes(ko["m5.approval.withdrawn"]), "told apart from a request taken back");
  const source = readFileSync(new URL("../src/m5/approval-card.tsx", import.meta.url), "utf8");
  const decide = source.slice(source.indexOf("async function decide("), source.indexOf("async function inspect("));
  assert.match(decide, /else if \(!live\.current && asked\) withdrawnCallback\.current\?\.\(\[\{ chatId: identity\.chatId, approvalId, kind: asked\.kind, summary: shown \? approvalHeadline\(shown\)\.target : null, answerFailed: true \}\]\);/);
  // On unmount, answers still on their way are left to that path (not counted twice).
  assert.ok(source.includes("!decided.current.has(item.approvalId) && !deciding.current.has(item.approvalId)"));
});
