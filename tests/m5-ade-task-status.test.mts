import assert from "node:assert/strict";
import test from "node:test";
import {
  changesStatus, dispositionFromApply, dispositionFromMerge, resultBarView, succeededRunNoticeKey,
  taskGroup, taskPhase, taskPhaseLabelKey, taskPhaseTone, type TaskPhase,
} from "../src/m5/task-status.ts";
import { TaskDispositionStore } from "../src/m5/task-disposition.ts";
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
  assert.equal(taskPhase({ state: "reviewReady", disposition: dispositionFromMerge(git()) }), "review");
  assert.equal(dispositionFromMerge(git())?.standing, "none");
  assert.equal(dispositionFromMerge({ kind: "snapshot" }), null);
  for (const phase of ["draft", "running", "attention", "review", "merged", "applied", "stopped", "discarded"] as const) {
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
