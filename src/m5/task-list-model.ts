import { taskGroup, type TaskGroup, type TaskPhase } from "./task-status.ts";

/**
 * The status-grouped task list (ADE 1차-나 L3, B). Groups and their order
 * come from the single task status (task-status.ts `taskGroup`); this file
 * only orders, hides and folds them.
 */
export const TASK_GROUPS: readonly TaskGroup[] = ["attention", "working", "review", "done"];

export interface TaskListRow {
  chatId: string;
  phase: TaskPhase;
  /** The latest standing read failed: the row says so (review #4). */
  readFailed: boolean;
  /** ISO time of the task's latest activity. */
  updatedAt: string;
}

/** The group a row sits in. A row whose standing could not be read waits for review. */
export function rowGroup(row: Pick<TaskListRow, "phase" | "readFailed">): TaskGroup {
  if (row.readFailed) return "review";
  return taskGroup(row.phase);
}

/**
 * Rows by group, newest activity first; empty groups left out, so a person
 * with one or two tasks sees no empty headings.
 */
export function groupTaskRows<T extends TaskListRow>(rows: readonly T[]): { group: TaskGroup; rows: T[] }[] {
  const byGroup = new Map<TaskGroup, T[]>();
  for (const row of rows) {
    const group = rowGroup(row);
    const list = byGroup.get(group) ?? [];
    list.push(row);
    byGroup.set(group, list);
  }
  const time = (row: T) => { const value = Date.parse(row.updatedAt); return Number.isFinite(value) ? value : 0; };
  return TASK_GROUPS.filter(group => byGroup.has(group)).map(group => ({
    group,
    rows: [...byGroup.get(group)!].sort((a, b) => time(b) - time(a)),
  }));
}

/** Finished work folds by default; the others always show. The open task's group stays open. */
export function groupStartsFolded(group: TaskGroup, holdsOpenTask: boolean): boolean {
  return group === "done" && !holdsOpenTask;
}

/** A short "when" for the row: just now, minutes, hours, yesterday or the date. */
export type RelativeTime = { key: "m5.when.now" } | { key: "m5.when.minutes"; n: number } | { key: "m5.when.hours"; n: number } | { key: "m5.when.yesterday" } | { key: "m5.when.date"; date: Date };
export function relativeTime(iso: string, now: number): RelativeTime | null {
  const at = Date.parse(iso);
  if (!Number.isFinite(at)) return null;
  const seconds = Math.max(0, Math.round((now - at) / 1000));
  if (seconds < 60) return { key: "m5.when.now" };
  if (seconds < 3600) return { key: "m5.when.minutes", n: Math.floor(seconds / 60) };
  const today = new Date(now); today.setHours(0, 0, 0, 0);
  const startOfToday = today.getTime();
  if (at >= startOfToday) return { key: "m5.when.hours", n: Math.floor(seconds / 3600) };
  if (at >= startOfToday - 86_400_000) return { key: "m5.when.yesterday" };
  return { key: "m5.when.date", date: new Date(at) };
}

/** Elapsed seconds as the shown "N분째"/"N초째" parts. */
export function elapsedParts(seconds: number): { key: "m5.when.forSeconds"; n: number } | { key: "m5.when.forMinutes"; n: number } {
  return seconds < 60 ? { key: "m5.when.forSeconds", n: Math.max(0, Math.floor(seconds)) } : { key: "m5.when.forMinutes", n: Math.floor(seconds / 60) };
}
