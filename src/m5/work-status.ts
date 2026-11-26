import type { SystemStatusG5DV1 } from "./contracts/lifecycle-g5d-v1.ts";
import type { AgentRunRecordV1 } from "./contracts/agent-adapter-v1.ts";
import { succeededRunNoticeKey, type TaskPhase } from "./task-status.ts";

/** Collapse only older confirmed terminal records, never live/recovery work. */
const chronological = (a: AgentRunRecordV1, b: AgentRunRecordV1) =>
  Date.parse(a.createdAt) - Date.parse(b.createdAt) || a.runId.localeCompare(b.runId);
export function groupRunHistory(runs: readonly AgentRunRecordV1[]) {
  const ordered = [...runs].sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt));
  const current: AgentRunRecordV1[] = [];
  const previous: AgentRunRecordV1[] = [];
  ordered.forEach((run, index) => {
    const closed = ["succeeded", "failed", "canceled"].includes(run.state)
      && run.terminationOutcome === "cleanExit";
    (index > 0 && closed ? previous : current).push(run);
  });
  // Finished runs read top to bottom in the order the team worked.
  previous.reverse();
  return { current, previous };
}

/** 1-based position of each run in the chat, oldest first, for "#n" labels. */
export function runOrder(runs: readonly AgentRunRecordV1[]): Map<string, number> {
  return new Map([...runs].sort(chronological).map((run, index) => [run.runId, index + 1]));
}

/** Whole seconds between start and last update; null when unknown. A run
 * still going counts up to `now` (when given): its last update says nothing
 * about how long it has been running. */
export function runDurationSeconds(run: Pick<AgentRunRecordV1, "createdAt" | "updatedAt"> & { state?: string }, now?: number): number | null {
  const going = now !== undefined && ["preparing", "running", "cancelling"].includes(run.state ?? "");
  const start = Date.parse(run.createdAt), end = going ? now : Date.parse(run.updatedAt);
  return Number.isFinite(start) && Number.isFinite(end) && end >= start ? Math.round((end - start) / 1000) : null;
}

/** Display only. A clean exit is not a successful task or permission to apply.
 * A read-only role changes no files, so its card never points at changes. */
/** `phase`: the task's single status (task-status.ts); a finished task's runs
 * ask for a review only while the task waits for one. */
export function runOutcomeNotice(run: Pick<AgentRunRecordV1, "state" | "terminationOutcome"> & { selectedProfile?: { permission: string } }, phase: TaskPhase | null = null) {
  if (run.state === "recoveryRequired" || run.terminationOutcome === "recoveryRequired") return "m5.run.help.recovery" as const;
  if (!["succeeded", "failed", "canceled"].includes(run.state)) return null;
  if (run.terminationOutcome !== "cleanExit") return "m5.run.help.unconfirmed" as const;
  if (run.state === "failed") return "m5.run.help.failedClean" as const;
  const readOnly = run.selectedProfile?.permission === "readOnly";
  if (run.state === "canceled") return readOnly ? "m5.run.help.canceledReadOnly" as const : "m5.run.help.canceledClean" as const;
  return readOnly ? "m5.run.help.succeededReadOnly" as const : succeededRunNoticeKey(phase);
}

export interface WorkStatusSummary { total: number; ids: string[]; omitted: number }

/** System blockers include live runs, not just failed runs. Never infer death here. */
/** `liveRunIds`: runs the screen knows are still working normally. They hold
 * the app too, but a run doing its job is not something to check. */
export function summarizeWorkStatus(status: SystemStatusG5DV1, liveRunIds: readonly string[] = []): WorkStatusSummary | null {
  if (status.storageAdmission.state !== "ready") throw new Error("storage is not ready");
  if (!Array.isArray(status.blockerIds) || status.blockerIds.some(id => typeof id !== "string" || !id)
    || new Set(status.blockerIds).size !== status.blockerIds.length
    || status.recoveryRequired !== (status.blockerIds.length > 0)) throw new Error("invalid blocker summary");
  const live = new Set(liveRunIds.map(id => `run:${id}`));
  const blockers = status.blockerIds.filter(id => !live.has(id));
  if (!blockers.length) return null;
  return {
    total: blockers.length,
    ids: blockers.slice(0, 20).map(id => id.replace(/[\p{Cc}\p{Cf}]/gu, "�").slice(0, 160)),
    omitted: Math.max(0, blockers.length - 20),
  };
}
