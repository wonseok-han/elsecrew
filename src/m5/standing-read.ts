import type { ChatMergeState } from "./contracts/project-catalog-v1.ts";
import { dispositionFromApply, dispositionFromMerge, type TaskDisposition } from "./task-status.ts";

/**
 * The open task's merge/apply standing, read in one place (the workbench).
 *
 * Every read belongs to a generation. A newer read, a switch of task, the
 * team starting again, or leaving the reading effect for any reason moves
 * the generation on, and an answer from an older generation is dropped. A
 * read also names the task and its version (`updatedAt`) it was made for,
 * so an answer is only ever taken for that exact task version.
 *
 * The latest outcome wins: a failed read hides what an earlier read said
 * (it can no longer be confirmed), so an old "merged" or "nothing to merge"
 * never covers a read that failed.
 */
export interface StandingRead {
  chatId: string;
  updatedAt: string;
  generation: number;
  status: "reading" | "known" | "failed";
  /** What kind of task this is, once known. A task never changes kind, so
   * this survives later reads and failures of the same task: the panels it
   * decides (merge or apply) never come and go while the task is re-read. */
  kind: "git" | "copy" | null;
  merge: ChatMergeState | null;
  apply: { applied: boolean; pending: number; total: number } | null;
}

/** A read starts: earlier values of the same task version show meanwhile. */
export function startStandingRead(previous: StandingRead | null, task: { chatId: string; updatedAt: string }, generation: number): StandingRead {
  const sameTask = previous !== null && previous.chatId === task.chatId;
  // Reads in a row keep what the last one showed (a read still out included),
  // never a failure's empty values.
  const same = sameTask && previous.updatedAt === task.updatedAt && previous.status !== "failed";
  return { chatId: task.chatId, updatedAt: task.updatedAt, generation, status: "reading", kind: sameTask ? previous.kind : null,
    merge: same ? previous.merge : null, apply: same ? previous.apply : null };
}

/** True when an answer still belongs to the current read. */
function current(read: StandingRead | null, answer: { chatId: string; updatedAt: string; generation: number }, latestGeneration: number): read is StandingRead {
  return read !== null && answer.generation === latestGeneration && read.generation === answer.generation
    && read.chatId === answer.chatId && read.updatedAt === answer.updatedAt;
}

/** An answer arrived; taken only if it is the current read's. */
export function finishStandingRead(read: StandingRead | null, answer: { chatId: string; updatedAt: string; generation: number; merge: ChatMergeState; apply: StandingRead["apply"] }, latestGeneration: number): StandingRead | null {
  if (!current(read, answer, latestGeneration)) return read;
  return { ...read, status: "known", kind: answer.merge.kind === "snapshot" ? "copy" : "git", merge: answer.merge, apply: answer.apply };
}

/** The read failed or got no answer in time: earlier values are not shown. */
export function failStandingRead(read: StandingRead | null, answer: { chatId: string; updatedAt: string; generation: number; kind?: StandingRead["kind"] }, latestGeneration: number): StandingRead | null {
  if (!current(read, answer, latestGeneration)) return read;
  // Only the apply receipt failed: the kind read just before still counts.
  return { ...read, status: "failed", kind: answer.kind ?? read.kind, merge: null, apply: null };
}

/** What the screen may use for this task right now. */
export function standingView(read: StandingRead | null, task: { chatId: string; updatedAt: string } | null): {
  disposition: TaskDisposition | null; failed: boolean; reading: boolean; kind: StandingRead["kind"]; merge: ChatMergeState | null; apply: StandingRead["apply"];
} {
  const kind = read && task && read.chatId === task.chatId ? read.kind : null;
  if (!read || !task || read.chatId !== task.chatId || read.updatedAt !== task.updatedAt) return { disposition: null, failed: false, reading: false, kind, merge: null, apply: null };
  if (read.status === "failed") return { disposition: null, failed: true, reading: false, kind, merge: null, apply: null };
  const disposition = read.merge?.kind === "gitWorktree" ? dispositionFromMerge(read.merge)
    : read.merge?.kind === "snapshot" && read.apply ? dispositionFromApply(read.apply) : null;
  return { disposition, failed: false, reading: read.status === "reading", kind, merge: read.merge, apply: read.apply };
}
