import type { ChatStateV1 } from "./contracts/run-recovery-v1.ts";
import type { ChatMergeState } from "./contracts/project-catalog-v1.ts";

/**
 * One task status for every place that shows it (task list, result bar,
 * changes panel, run cards). Display only: the sidecar stays the source of
 * truth for what may be merged, applied or started.
 *
 * draft → running → attention → review → merged | applied | changedSince,
 * plus stopped (canceled) and discarded.
 */
export type TaskPhase = "draft" | "running" | "attention" | "review" | "merged" | "applied" | "changedSince" | "stopped" | "discarded";

/** How the task reaches the user's folder: a Git task merges, a copy task applies. */
export type TaskKind = "git" | "copy";

/** Where a finished task's changes stand against the user's folder. */
export interface TaskDisposition {
  kind: TaskKind;
  /** none: the task changed nothing to merge or apply. */
  standing: "none" | "notYet" | "done" | "changedSince";
  /** Files still to apply again (copy tasks only). */
  pending?: number;
}

/** A Git task from its merge inspection; a copy task answers `null` (ask apply). */
export function dispositionFromMerge(state: ChatMergeState | null): TaskDisposition | null {
  if (state?.kind !== "gitWorktree") return null;
  const pending = state.ahead > 0 || state.uncommitted;
  if (state.mergedBefore) return { kind: "git", standing: pending ? "changedSince" : "done" };
  return { kind: "git", standing: pending ? "notYet" : "none" };
}

/** A copy task from its apply receipt. */
export function dispositionFromApply(state: { applied: boolean; pending: number; total: number } | null): TaskDisposition | null {
  if (!state) return null;
  if (!state.applied) return { kind: "copy", standing: state.total > 0 ? "notYet" : "none" };
  return { kind: "copy", standing: state.pending === 0 ? "done" : "changedSince", pending: state.pending };
}

const RUNNING: readonly ChatStateV1[] = ["running", "stopping", "applying"];
const ATTENTION: readonly ChatStateV1[] = ["waitingForUser", "needsAttention", "recoveryRequired"];
const FINISHED: readonly ChatStateV1[] = ["reviewReady", "completed"];

/**
 * The single task status. A task that works again after a merge is running,
 * never "merged": the finished-work standing only counts once the team is done.
 */
export function taskPhase(input: { state: ChatStateV1; disposition?: TaskDisposition | null; approvalPending?: boolean }): TaskPhase {
  const { state, disposition = null, approvalPending = false } = input;
  if (state === "discarded") return "discarded";
  if (state === "draft" || state === "preparing") return approvalPending ? "attention" : "draft";
  if (RUNNING.includes(state)) return approvalPending ? "attention" : "running";
  if (ATTENTION.includes(state)) return "attention";
  if (state === "canceled") return "stopped";
  if (FINISHED.includes(state)) {
    if (disposition?.standing === "done") return disposition.kind === "git" ? "merged" : "applied";
    if (disposition?.standing === "changedSince") return "changedSince";
    return "review";
  }
  return "review";
}

/** The team is (or may be) working: nothing is offered to merge or apply. */
export function phaseIsWorking(phase: TaskPhase): boolean {
  return phase === "draft" || phase === "running" || phase === "attention";
}

/** The task's work has reached the user's folder and nothing changed since. */
export function phaseIsSettled(phase: TaskPhase): boolean {
  return phase === "merged" || phase === "applied";
}

/** Shown words for a phase. `changedSince` reads differently for each kind. */
export function taskPhaseLabelKey(phase: TaskPhase, kind: TaskKind | null = null) {
  if (phase === "changedSince") return kind === "copy" ? "m5.task.phase.changedSinceApply" as const : "m5.task.phase.changedSinceMerge" as const;
  return `m5.task.phase.${phase}` as const;
}

/** Colour family (CSS `--status-*`). */
export type TaskTone = "running" | "attention" | "review" | "done" | "danger" | "neutral";
export function taskPhaseTone(phase: TaskPhase): TaskTone {
  switch (phase) {
    case "running": return "running";
    case "attention": return "attention";
    case "review": case "changedSince": return "review";
    case "merged": case "applied": return "done";
    case "stopped": return "danger";
    default: return "neutral";
  }
}

/** Groups for a status-grouped task list (working / needs me / waiting for review / done). */
export type TaskGroup = "attention" | "working" | "review" | "done";
export function taskGroup(phase: TaskPhase): TaskGroup {
  if (phase === "attention") return "attention";
  if (phase === "draft" || phase === "running") return "working";
  if (phase === "review" || phase === "changedSince") return "review";
  return "done";
}

/**
 * What the result bar says and offers. `null` hides it (the team is not done).
 * `loading`: the standing is not read yet, so a placeholder shows instead of a
 * button that could belong to the task shown a moment ago.
 */
export interface ResultBarView {
  phase: TaskPhase;
  loading: boolean;
  detailKey: "m5.task.result.review" | "m5.task.result.merged" | "m5.task.result.applied" | "m5.task.result.changedSinceMerge" | "m5.task.result.changedSinceApply" | null;
}
export function resultBarView(input: { phase: TaskPhase; kind: TaskKind | null; known: boolean; switching?: boolean }): ResultBarView | null {
  const { phase, kind, known, switching = false } = input;
  if (switching) return { phase, loading: true, detailKey: null };
  if (phaseIsWorking(phase) || phase === "stopped" || phase === "discarded") return null;
  if (!known) return { phase, loading: true, detailKey: null };
  const detailKey = phase === "merged" ? "m5.task.result.merged"
    : phase === "applied" ? "m5.task.result.applied"
    : phase === "changedSince" ? (kind === "copy" ? "m5.task.result.changedSinceApply" : "m5.task.result.changedSinceMerge")
    : "m5.task.result.review";
  return { phase, loading: false, detailKey };
}

/** The changes panel's one status line. */
export type ChangesStatus = "working" | "notYet" | "done" | "changedSince" | null;
export function changesStatus(phase: TaskPhase, changeCount: number): ChangesStatus {
  if (changeCount === 0 && !phaseIsSettled(phase)) return null;
  if (phaseIsWorking(phase)) return "working";
  if (phaseIsSettled(phase)) return "done";
  if (phase === "changedSince") return "changedSince";
  if (phase === "review") return "notYet";
  return null;
}

/**
 * A finished run's closing line. Asking to check the changes belongs only to
 * a task waiting for review; once its work is in the user's folder (or the
 * team works on), the run just says it ended well.
 */
export function succeededRunNoticeKey(phase: TaskPhase | null) {
  return phase === null || phase === "review" || phase === "changedSince"
    ? "m5.run.help.succeededClean" as const
    : "m5.run.help.succeededSettled" as const;
}
