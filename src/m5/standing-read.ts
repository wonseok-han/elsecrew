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
  merge: ChatMergeState | null;
  apply: { applied: boolean; pending: number; total: number } | null;
}

/** A read starts: earlier values of the same task version show meanwhile. */
export function startStandingRead(previous: StandingRead | null, task: { chatId: string; updatedAt: string }, generation: number): StandingRead {
  const same = previous && previous.chatId === task.chatId && previous.updatedAt === task.updatedAt && previous.status === "known";
  return { chatId: task.chatId, updatedAt: task.updatedAt, generation, status: "reading", merge: same ? previous.merge : null, apply: same ? previous.apply : null };
}

/** True when an answer still belongs to the current read. */
function current(read: StandingRead | null, answer: { chatId: string; updatedAt: string; generation: number }, latestGeneration: number): read is StandingRead {
  return read !== null && answer.generation === latestGeneration && read.generation === answer.generation
    && read.chatId === answer.chatId && read.updatedAt === answer.updatedAt;
}

/** An answer arrived; taken only if it is the current read's. */
export function finishStandingRead(read: StandingRead | null, answer: { chatId: string; updatedAt: string; generation: number; merge: ChatMergeState; apply: StandingRead["apply"] }, latestGeneration: number): StandingRead | null {
  if (!current(read, answer, latestGeneration)) return read;
  return { ...read, status: "known", merge: answer.merge, apply: answer.apply };
}

/** The read failed or got no answer in time: earlier values are not shown. */
export function failStandingRead(read: StandingRead | null, answer: { chatId: string; updatedAt: string; generation: number }, latestGeneration: number): StandingRead | null {
  if (!current(read, answer, latestGeneration)) return read;
  return { ...read, status: "failed", merge: null, apply: null };
}

/** What the screen may use for this task right now. */
export function standingView(read: StandingRead | null, task: { chatId: string; updatedAt: string } | null): {
  disposition: TaskDisposition | null; failed: boolean; reading: boolean; merge: ChatMergeState | null; apply: StandingRead["apply"];
} {
  if (!read || !task || read.chatId !== task.chatId || read.updatedAt !== task.updatedAt) return { disposition: null, failed: false, reading: false, merge: null, apply: null };
  if (read.status === "failed") return { disposition: null, failed: true, reading: false, merge: null, apply: null };
  const disposition = read.merge?.kind === "gitWorktree" ? dispositionFromMerge(read.merge)
    : read.merge?.kind === "snapshot" && read.apply ? dispositionFromApply(read.apply) : null;
  return { disposition, failed: false, reading: read.status === "reading", merge: read.merge, apply: read.apply };
}
