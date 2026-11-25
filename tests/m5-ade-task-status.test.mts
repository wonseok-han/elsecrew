import assert from "node:assert/strict";
import test from "node:test";
import {
  changesStatus, dispositionFromApply, dispositionFromMerge, resultBarView, standingRead, succeededRunNoticeKey,
  taskGroup, taskPhase, taskPhaseLabelKey, taskPhaseTone, type TaskPhase,
} from "../src/m5/task-status.ts";
import { TaskDispositionStore } from "../src/m5/task-disposition.ts";
import { outcomeOfChatState } from "../src/m5/notifications.ts";
import { runOutcomeNotice } from "../src/m5/work-status.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";
import type { ChatMergeState } from "../src/m5/contracts/project-catalog-v1.ts";

const git = (over: Partial<Extract<ChatMergeState, { kind: "gitWorktree" }>> = {}): ChatMergeState => ({
  kind: "gitWorktree", branch: "elsecrew/task-1", baseRef: "main", head: "a".repeat(40), baseTip: "b".repeat(40),
  ahead: 0, behind: 0, uncommitted: false, baseCheckedOut: true, resolving: false, unresolved: 0, mergedBefore: false, blocked: null, ...over,
});

test("a task that works again after a merge is running, never 'merged' (D3①)", () => {
  const merged = dispositionFromMerge(git({ mergedBefore: true }));
  assert.equal(taskPhase({ state: "reviewReady", disposition: merged }), "merged");
  for (const state of ["running", "stopping", "applying"] as const) {
    const phase = taskPhase({ state, disposition: merged });
    assert.equal(phase, "running");
    // The changes panel never says merged or offers a merge while the team works.
    assert.equal(changesStatus(phase, 4), "working");
    assert.equal(resultBarView({ phase, kind: "git", known: true }), null);
  }
});

test("merged and applied tasks show as merged/applied in the list, not 'review ready' (D3②)", () => {
  const merged = taskPhase({ state: "reviewReady", disposition: dispositionFromMerge(git({ mergedBefore: true })) });
  const applied = taskPhase({ state: "reviewReady", disposition: dispositionFromApply({ applied: true, pending: 0, total: 5 }) });
  assert.equal(merged, "merged");
  assert.equal(applied, "applied");
  assert.equal(ko[taskPhaseLabelKey(merged)], "합침");
  assert.equal(ko[taskPhaseLabelKey(applied)], "반영함");
  assert.equal(taskGroup(merged), "done");
  assert.equal(taskPhaseTone(applied), "done");
});

test("every place reads the same value through the todo scenario (git and copy)", () => {
  type Step = { state: Parameters<typeof taskPhase>[0]["state"]; disposition: ReturnType<typeof dispositionFromMerge>; expect: TaskPhase; changes: ReturnType<typeof changesStatus>; bar: boolean };
  const steps: Step[] = [
    { state: "draft", disposition: null, expect: "draft", changes: "working", bar: false },
    { state: "running", disposition: null, expect: "running", changes: "working", bar: false },
    { state: "reviewReady", disposition: dispositionFromMerge(git({ ahead: 3 })), expect: "review", changes: "notYet", bar: true },
    { state: "reviewReady", disposition: dispositionFromMerge(git({ mergedBefore: true })), expect: "merged", changes: "done", bar: true },
    { state: "reviewReady", disposition: dispositionFromApply({ applied: false, pending: 4, total: 4 }), expect: "review", changes: "notYet", bar: true },
    { state: "reviewReady", disposition: dispositionFromApply({ applied: true, pending: 0, total: 4 }), expect: "applied", changes: "done", bar: true },
    { state: "running", disposition: dispositionFromApply({ applied: true, pending: 2, total: 5 }), expect: "running", changes: "working", bar: false },
    { state: "reviewReady", disposition: dispositionFromApply({ applied: true, pending: 2, total: 5 }), expect: "changedSince", changes: "changedSince", bar: true },
    { state: "reviewReady", disposition: dispositionFromApply({ applied: true, pending: 0, total: 5 }), expect: "applied", changes: "done", bar: true },
  ];
  for (const step of steps) {
    const phase = taskPhase({ state: step.state, disposition: step.disposition });
    assert.equal(phase, step.expect, JSON.stringify(step));
    assert.equal(changesStatus(phase, 4), step.changes, JSON.stringify(step));
    assert.equal(resultBarView({ phase, kind: step.disposition?.kind ?? null, known: true }) !== null, step.bar, JSON.stringify(step));
  }
  const again = taskPhase({ state: "reviewReady", disposition: dispositionFromApply({ applied: true, pending: 2, total: 5 }) });
  assert.equal(ko[taskPhaseLabelKey(again, "copy")], "반영 뒤 또 바뀜");
  assert.equal(resultBarView({ phase: again, kind: "copy", known: true })?.detailKey, "m5.task.result.changedSinceApply");
  assert.equal(ko[taskPhaseLabelKey("changedSince", "git")], "합친 뒤 또 바뀜");
});

test("a finished run asks for a review only while the task waits for one (D3③)", () => {
  const run = { state: "succeeded" as const, terminationOutcome: "cleanExit" as const };
  assert.equal(runOutcomeNotice(run), "m5.run.help.succeededClean");
  assert.equal(runOutcomeNotice(run, "review"), "m5.run.help.succeededClean");
  for (const phase of ["merged", "applied", "running"] as const) {
    assert.equal(runOutcomeNotice(run, phase), "m5.run.help.succeededSettled");
    assert.equal(succeededRunNoticeKey(phase), "m5.run.help.succeededSettled");
  }
  assert.ok(!ko["m5.run.help.succeededSettled"].includes("검토"));
  assert.ok(!ko["m5.run.help.succeededClean"].includes("원본에 적용"));
});

test("switching tasks or an unread standing shows a placeholder, never the previous task's bar (D3④)", () => {
  const view = resultBarView({ phase: "review", kind: "git", known: true, switching: true });
  assert.deepEqual(view, { phase: "review", loading: true, detailKey: null });
  assert.deepEqual(resultBarView({ phase: "review", kind: null, known: false }), { phase: "review", loading: true, detailKey: null });
  assert.equal(resultBarView({ phase: "review", kind: "git", known: true })?.loading, false);
});

test("approvals waiting make a running task 'needs you'; other states map once", () => {
  assert.equal(taskPhase({ state: "running", approvalPending: true }), "attention");
  assert.equal(taskPhase({ state: "needsAttention" }), "attention");
  assert.equal(taskPhase({ state: "canceled" }), "stopped");
  assert.equal(taskPhase({ state: "discarded" }), "discarded");
  assert.equal(taskPhase({ state: "reviewReady", disposition: dispositionFromMerge(git()) }), "answered", "nothing to merge is not a review");
  assert.equal(dispositionFromMerge(git())?.standing, "none");
  assert.equal(dispositionFromMerge({ kind: "snapshot" }), null);
  for (const phase of ["draft", "running", "attention", "review", "answered", "merged", "applied", "stopped", "discarded"] as const) {
    assert.ok(ko[taskPhaseLabelKey(phase)] && en[taskPhaseLabelKey(phase)], phase);
  }
});

test("the task list asks each finished task once per version and forgets a task that ran again", async () => {
  const asked: string[] = [];
  const answers = new Map([["chat_a", { kind: "git" as const, standing: "done" as const }], ["chat_b", { kind: "copy" as const, standing: "changedSince" as const, pending: 2 }]]);
  const saved = new Map<string, string>();
  const storage = { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => { saved.set(key, value); } };
  const store = new TaskDispositionStore(async chatId => { asked.push(chatId); return answers.get(chatId) ?? null; }, storage);
  const a = { chatId: "chat_a", updatedAt: "2026-11-25T01:00:00Z", state: "reviewReady" };
  const b = { chatId: "chat_b", updatedAt: "2026-11-25T01:00:00Z", state: "reviewReady" };
  const running = { chatId: "chat_c", updatedAt: "2026-11-25T01:00:00Z", state: "running" };
  store.request([a, b, running]);
  store.request([a, b, running]);
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.deepEqual(asked, ["chat_a", "chat_b"], "running tasks are not asked, each finished one only once");
  assert.equal(store.get(a)?.standing, "done");
  assert.equal(taskPhase({ state: "reviewReady", disposition: store.get(b) }), "changedSince");
  // A new version of the task (it ran again) is not shown with its old standing.
  assert.equal(store.get({ ...a, updatedAt: "2026-11-25T02:00:00Z" }), null);
  // Restart: the per-viewer record comes back.
  const reopened = new TaskDispositionStore(async () => null, storage);
  assert.equal(reopened.get(a)?.standing, "done");
  reopened.forget("chat_a");
  assert.equal(reopened.get(a), null);
});

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { renderToStaticMarkup } from "react-dom/server";
import * as taskStatus from "../src/m5/task-status.ts";
import * as catalog from "../src/m5/contracts/project-catalog-v1.ts";

/** Renders ApplyLatest with fixed hook state; effects run in order (once). */
function renderApplyLatest(states: unknown[], props: Record<string, unknown>) {
  const require = createRequire(import.meta.url);
  const compiled = ts.transpileModule(readFileSync(new URL("../src/m5/apply-latest.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020 },
  }).outputText;
  let index = 0;
  const effects: (() => void)[] = [];
  const sets: unknown[][] = [];
  const context: any = { exports: {}, window: { m5Projects: { request: async () => { throw new Error("offline"); } } }, require(name: string) {
    if (name === "react") return {
      useState: (initial: unknown) => { const slot = index++; const value = slot < states.length ? states[slot] : initial; sets[slot] = []; return [value, (next: unknown) => sets[slot]!.push(next)]; },
      useRef: (value: unknown) => ({ current: value }), useCallback: (fn: unknown) => fn,
      useEffect: (fn: () => void) => { effects.push(fn); },
    };
    if (name === "../i18n.tsx") return { useI18n: () => ({ t: (key: string, params: Record<string, unknown> = {}) => (ko as Record<string, string>)[key]!.replace(/\{(\w+)\}/g, (_: string, k: string) => String(params[k])) }) };
    if (name === "./task-status.ts") return taskStatus;
    if (name === "./contracts/project-catalog-v1.ts") return catalog;
    return require(name);
  } };
  runInNewContext(compiled, context);
  const html = renderToStaticMarkup(context.exports.ApplyLatest({ projectId: "project_1", chatId: "chat_1", changeCount: 2, disabled: false, ...props }));
  return { html, sets };
}

test("a copy task's apply panel shows a placeholder until its receipt is read, never 'not applied' first", () => {
  const original = (globalThis as any).window;
  (globalThis as any).window = { m5Projects: { request: async () => ({}) } };
  try {
    // useState order: confirming, busy, result, failed, state, unreadable.
    const before = renderApplyLatest([false, false, null, null, null, false], {}).html;
    assert.ok(before.includes("m5-apply-latest-loading"), before);
    assert.ok(!before.includes("<button"), "no apply button before the standing is known");
    const unreadable = renderApplyLatest([false, false, null, null, null, true], {}).html;
    assert.ok(unreadable.includes("<button") && !unreadable.includes("m5-apply-latest-loading"), "an unreadable receipt falls back to the plain panel");
    const known = renderApplyLatest([false, false, null, null, { applied: true, pending: 1, total: 2 }, false], {}).html;
    assert.ok(known.includes("반영 뒤 새로 바뀐 파일 1개") && known.includes(ko["m5.apply.again"]));
  } finally { (globalThis as any).window = original; }
});

test("a team's ordinary answer without file changes is 'answered', not 'needs you' or 'waiting for review' (review #6)", () => {
  // orchestration.rs: completed without changes → waitingForUser; with changes → reviewReady.
  const answered = taskPhase({ state: "waitingForUser" });
  assert.equal(answered, "answered");
  assert.equal(ko[taskPhaseLabelKey(answered)], "답변 완료");
  assert.equal(taskGroup(answered), "done");
  assert.equal(resultBarView({ phase: answered, kind: "git", known: true }), null, "nothing to merge or apply: no result bar");
  assert.equal(changesStatus(answered, 0), null);
  assert.equal(succeededRunNoticeKey(answered), "m5.run.help.succeededSettled");
  // `completed` with nothing to put in the folder never says "not in your folder yet".
  assert.equal(taskPhase({ state: "completed", disposition: { kind: "git", standing: "none" } }), "answered");
  assert.equal(resultBarView({ phase: "answered", kind: "git", known: true }), null);
  // An answer after earlier, not yet merged changes still asks for a review.
  assert.equal(taskPhase({ state: "waitingForUser", disposition: { kind: "git", standing: "notYet" } }), "review");
  assert.equal(taskPhase({ state: "waitingForUser", disposition: { kind: "copy", standing: "done" } }), "applied");
  // Unread standing of an answer: a placeholder until read (it may have changes).
  assert.equal(resultBarView({ phase: "answered", kind: null, known: false })?.loading, true);
  // The bell calls the same state "done".
  assert.equal(outcomeOfChatState("waitingForUser"), "done");
});

test("a failed apply-receipt read never leaves the result bar locked (review #5)", () => {
  const snapshot = { state: { kind: "snapshot" as const } };
  assert.equal(standingRead({ disposition: null, mergeRead: snapshot, applyRead: null }), false, "still reading");
  assert.equal(standingRead({ disposition: null, mergeRead: snapshot, applyRead: { state: null } }), true, "read failed: known, the panel offers to look again");
  assert.equal(standingRead({ disposition: null, mergeRead: { state: null }, applyRead: null }), true);
  assert.equal(standingRead({ disposition: { kind: "copy", standing: "notYet" }, mergeRead: null, applyRead: null }), true);
  const view = resultBarView({ phase: taskPhase({ state: "reviewReady" }), kind: "copy", known: standingRead({ disposition: null, mergeRead: snapshot, applyRead: { state: null } }) });
  assert.equal(view?.loading, false);
});

test("kept standings are asked again every session and after a revalidation; late answers are dropped (review #4)", async () => {
  const saved = new Map<string, string>();
  const storage = { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => { saved.set(key, value); } };
  const task = { chatId: "chat_a", updatedAt: "2026-11-25T01:00:00Z", state: "reviewReady" };
  const first = new TaskDispositionStore(async () => ({ kind: "git", standing: "done" }), storage);
  first.request([task]);
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.equal(first.get(task)?.standing, "done");
  // Next session: the kept answer shows, and the task is still asked again
  // (its folder or base branch can change without its record).
  let answer: { kind: "git"; standing: "done" | "changedSince" } = { kind: "git", standing: "changedSince" };
  const asked: string[] = [];
  const second = new TaskDispositionStore(async chatId => { asked.push(chatId); return answer; }, storage);
  assert.equal(second.get(task)?.standing, "done");
  second.request([task]);
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.deepEqual(asked, ["chat_a"]);
  assert.equal(second.get(task)?.standing, "changedSince");
  // Same session, same version: not asked again until revalidated.
  second.request([task]);
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.equal(asked.length, 1);
  second.revalidate();
  answer = { kind: "git", standing: "done" };
  second.request([task]);
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.equal(asked.length, 2);
  assert.equal(second.get(task)?.standing, "done");
  // An answer that arrives after a revalidation started is out of date.
  let release: (value: { kind: "git"; standing: "changedSince" }) => void = () => {};
  const slow = new TaskDispositionStore(() => new Promise(resolve => { release = resolve; }), null);
  slow.request([task]);
  await new Promise(resolve => setTimeout(resolve, 5));
  slow.revalidate();
  release({ kind: "git", standing: "changedSince" });
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.equal(slow.get(task), null);
});

test("the open task's merge and apply standings are read again when its changes are", () => {
  for (const file of ["../src/m5/chat-workspace-merge.tsx", "../src/m5/apply-latest.tsx"]) {
    const source = readFileSync(new URL(file, import.meta.url), "utf8");
    assert.match(source, /useEffect\(\(\) => \{ void inspect\(\); return \(\) => \{ generation\.current\+\+; \}; \}, \[inspect, [^\]]*revision\]\);/, file);
  }
  const workbench = readFileSync(new URL("../src/m5/workbench.tsx", import.meta.url), "utf8");
  assert.ok(workbench.includes("revision={`${diffRevision}`}"));
  assert.ok(workbench.includes("dispositions.revalidate()"));
});
