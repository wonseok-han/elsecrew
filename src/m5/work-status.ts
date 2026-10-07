import type { SystemStatusG5DV1 } from "./contracts/lifecycle-g5d-v1.ts";
import type { AgentRunRecordV1 } from "./contracts/agent-adapter-v1.ts";

/** Collapse only older confirmed terminal records, never live/recovery work. */
export function groupRunHistory(runs: readonly AgentRunRecordV1[]) {
  const ordered = [...runs].sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt));
  const current: AgentRunRecordV1[] = [];
  const previous: AgentRunRecordV1[] = [];
  ordered.forEach((run, index) => {
    const closed = ["succeeded", "failed", "canceled"].includes(run.state)
      && run.terminationOutcome === "cleanExit";
    (index > 0 && closed ? previous : current).push(run);
  });
  return { current, previous };
}

/** Display only. A clean exit is not a successful task or permission to apply. */
export function runOutcomeNotice(run: Pick<AgentRunRecordV1, "state" | "terminationOutcome">) {
  if (run.state === "recoveryRequired" || run.terminationOutcome === "recoveryRequired") return "m5.run.help.recovery" as const;
  if (!["succeeded", "failed", "canceled"].includes(run.state)) return null;
  if (run.terminationOutcome !== "cleanExit") return "m5.run.help.unconfirmed" as const;
  if (run.state === "failed") return "m5.run.help.failedClean" as const;
  if (run.state === "canceled") return "m5.run.help.canceledClean" as const;
  return "m5.run.help.succeededClean" as const;
}

export interface WorkStatusSummary { total: number; ids: string[]; omitted: number }

/** System blockers include live runs, not just failed runs. Never infer death here. */
export function summarizeWorkStatus(status: SystemStatusG5DV1): WorkStatusSummary | null {
  if (status.storageAdmission.state !== "ready") throw new Error("storage is not ready");
  if (!Array.isArray(status.blockerIds) || status.blockerIds.some(id => typeof id !== "string" || !id)
    || new Set(status.blockerIds).size !== status.blockerIds.length
    || status.recoveryRequired !== (status.blockerIds.length > 0)) throw new Error("invalid blocker summary");
  if (!status.blockerIds.length) return null;
  return {
    total: status.blockerIds.length,
    ids: status.blockerIds.slice(0, 20).map(id => id.replace(/[\p{Cc}\p{Cf}]/gu, "�").slice(0, 160)),
    omitted: Math.max(0, status.blockerIds.length - 20),
  };
}
