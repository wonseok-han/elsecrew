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

/**
 * Everything the agent asks to run or touch, exactly as the engine reported
 * it: no shortening, no whitespace changes (only invisible and control
 * characters are shown as escapes). This is what the person must have opened
 * before allowing.
 */
export function approvalFields(inspection: ApprovalInspection): { field: "command" | "cwd" | "reason" | "toolInput" | "grantRoot" | "host" | "protocol"; text: string }[] {
  const fields = [
    ["command", inspection.command], ["toolInput", inspection.toolInput ?? null], ["cwd", inspection.cwd],
    ["grantRoot", inspection.grantRoot], ["host", inspection.host], ["protocol", inspection.protocol], ["reason", inspection.reason],
  ] as const;
  return fields.filter(([, value]) => value !== null).map(([field, value]) => ({ field, text: visibleApprovalText(value!) }));
}

/**
 * Allowing a request needs its full content read from the engine for this
 * exact request and opened by the person on this card. A summary line read
 * automatically is never enough, and a new request starts unopened.
 */
export function canAcceptApproval(input: { approvalId: string; inspectedId: string | null; openedIds: ReadonlySet<string>; sending: boolean; error: boolean }): boolean {
  return !input.sending && !input.error && input.inspectedId === input.approvalId && input.openedIds.has(input.approvalId);
}
