import type { SystemStatusG5DV1 } from "./contracts/lifecycle-g5d-v1.ts";

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
