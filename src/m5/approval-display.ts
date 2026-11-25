import { visibleApprovalText, type ApprovalInspection } from "./contracts/approval-v1.ts";

const LINE_MAX = 240;

function shorten(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  const chars = Array.from(flat);
  return chars.length > LINE_MAX ? `${chars.slice(0, LINE_MAX - 1).join("")}…` : flat;
}

/**
 * The first line of a permission request: what the agent wants to run or
 * touch (a command, a path, a host) and why, in plain words. All text is
 * shown as text after hiding invisible and control characters; the full
 * original stays under "details".
 */
export function approvalHeadline(inspection: ApprovalInspection): { target: string; reason: string | null } {
  let target: string | null = null;
  if (inspection.kind === "tool" && inspection.toolInput) {
    try {
      const input: unknown = JSON.parse(inspection.toolInput);
      if (input && typeof input === "object" && !Array.isArray(input)) {
        const fields = input as Record<string, unknown>;
        const value = fields.command ?? fields.file_path ?? fields.path ?? fields.url ?? fields.pattern;
        if (typeof value === "string" && value.trim()) target = value;
      }
    } catch { /* the original stays under details */ }
  }
  target ??= inspection.command ?? inspection.grantRoot ?? (inspection.host ? `${inspection.protocol ? `${inspection.protocol}://` : ""}${inspection.host}` : null) ?? inspection.cwd ?? "";
  const reason = inspection.reason?.trim() ? shorten(visibleApprovalText(inspection.reason)) : null;
  return { target: shorten(visibleApprovalText(target)), reason };
}

/**
 * Requests that left the list without the person answering: the agent took
 * them back. Errors empty the list too, so they never count.
 */
export function withdrawnRequests<T extends { approvalId: string }>(before: readonly T[], after: readonly T[], answered: ReadonlySet<string>): T[] {
  const still = new Set(after.map(item => item.approvalId));
  return before.filter(item => !still.has(item.approvalId) && !answered.has(item.approvalId));
}
