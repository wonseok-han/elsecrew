import type { AgentRunRecordV1 } from "./contracts/agent-adapter-v1.ts";
import { runDurationSeconds } from "./work-status.ts";

/**
 * The task story's pure rules (ADE 1차-나 L4): the stage bar's cells and
 * one card per run. The bar shows the runs that actually happened, in
 * order (no planned stages until 2차 F3), plus what the team does now.
 */

export type StageRunStatus = "done" | "now" | "failed" | "canceled" | "recovery";
export type StageCell =
  | { kind: "run"; runId: string; roleId: string; status: StageRunStatus; seconds: number | null }
  /** Nothing has run yet: the coordinator is planning. */
  | { kind: "planning" }
  /** Between runs: the coordinator reads the result and picks who goes next. */
  | { kind: "between" }
  /** The dashed last cell while a run works: "next · the coordinator decides". */
  | { kind: "next" };

const LIVE: readonly string[] = ["preparing", "running", "cancelling"];

export function runStageStatus(run: Pick<AgentRunRecordV1, "state" | "terminationOutcome">): StageRunStatus {
  if (LIVE.includes(run.state)) return "now";
  if (run.state === "recoveryRequired" || run.terminationOutcome === "recoveryRequired") return "recovery";
  if (run.state === "failed") return "failed";
  if (run.state === "canceled") return "canceled";
  return "done";
}

/**
 * Cells for the stage bar. While the team works there is always a cell saying
 * what happens now (a run, "planning" or "between"), so no stretch of the
 * work goes without a sentence (F1). At most `max` cells show; older runs
 * fold into `hidden` ("+N").
 */
export function stageCells(runs: readonly AgentRunRecordV1[], input: { working: boolean; now: number; max?: number }): { hidden: number; cells: StageCell[] } {
  const max = Math.max(2, input.max ?? 5);
  const ordered = [...runs].sort((a, b) => Date.parse(a.createdAt) - Date.parse(b.createdAt) || a.runId.localeCompare(b.runId));
  const runCells: StageCell[] = ordered.map(run => ({ kind: "run", runId: run.runId, roleId: run.selectedProfile.roleId, status: runStageStatus(run), seconds: runDurationSeconds(run, input.now) }));
  const trailing: StageCell[] = [];
  if (input.working) {
    const live = ordered.some(run => LIVE.includes(run.state));
    if (live) trailing.push({ kind: "next" });
    else trailing.push({ kind: ordered.length === 0 ? "planning" : "between" });
  }
  const room = max - trailing.length;
  const hidden = Math.max(0, runCells.length - room);
  return { hidden, cells: [...runCells.slice(hidden), ...trailing] };
}

/** The one sentence the bar announces (aria-live) for what happens now. */
export function stageNow(cells: readonly StageCell[]): StageCell | null {
  return cells.find(cell => (cell.kind === "run" && cell.status === "now") || cell.kind === "planning" || cell.kind === "between") ?? null;
}

/**
 * One card per run (ADE 1차-나 L4b): a run's posted reports (same `runId`)
 * join its card instead of showing again as separate messages. The
 * person's own messages become bubbles; messages with no run (status lines,
 * a report whose run is not in this task's run list) stay single items.
 * Everything is ordered by time, the runs by their start.
 */
export interface StoryMessage { messageId: string; kind: string; runId: string | null; createdAt: string }
export interface StoryRun { runId: string; createdAt: string }
export type StoryItem<M extends StoryMessage, R extends StoryRun> =
  | { kind: "run"; key: string; run: R; reports: M[] }
  | { kind: "user"; key: string; message: M }
  | { kind: "message"; key: string; message: M };

export function storyItems<M extends StoryMessage, R extends StoryRun>(messages: readonly M[], runs: readonly R[]): StoryItem<M, R>[] {
  const reports = new Map<string, M[]>();
  const known = new Set(runs.map(run => run.runId));
  const items: { at: number; order: number; item: StoryItem<M, R> }[] = [];
  let order = 0;
  for (const message of messages) {
    if (message.runId && known.has(message.runId) && message.kind !== "user") {
      const list = reports.get(message.runId) ?? [];
      list.push(message);
      reports.set(message.runId, list);
      continue;
    }
    const item: StoryItem<M, R> = message.kind === "user"
      ? { kind: "user", key: `message:${message.messageId}`, message }
      : { kind: "message", key: `message:${message.messageId}`, message };
    items.push({ at: Date.parse(message.createdAt), order: order++, item });
  }
  for (const run of runs) items.push({ at: Date.parse(run.createdAt), order: order++, item: { kind: "run", key: `run:${run.runId}`, run, reports: reports.get(run.runId) ?? [] } });
  const time = (value: number) => Number.isFinite(value) ? value : Number.MAX_SAFE_INTEGER;
  return items.sort((a, b) => time(a.at) - time(b.at) || a.order - b.order).map(entry => entry.item);
}
