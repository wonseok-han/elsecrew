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
  if (inspection.kind === "tool" && inspection.toolInput && inspection.toolInputPages !== undefined) {
    // The first page of a large input: name its target from the page's start.
    target = pagedTarget(inspection.toolInput);
  } else if (inspection.kind === "tool" && inspection.toolInput) {
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
export function canAcceptApproval(input: { approvalId: string; inspectedId: string | null; openedIds: ReadonlySet<string>; sending: boolean; error: boolean;
  /** For a large tool input read in pages: every page was read, joined and
   * checked against the engine's size and SHA-256 (PR #19 review B1). A
   * request read whole passes true. */
  fullyRead: boolean }): boolean {
  return !input.sending && !input.error && input.fullyRead && input.inspectedId === input.approvalId && input.openedIds.has(input.approvalId);
}

/** The target of a large tool input from its first page (a JSON object's
 * start): the first file path, command or path field, or null. */
export function pagedTarget(firstPage: string): string | null {
  const match = /"(file_path|command|path|notebook_path)":("(?:[^"\\]|\\.)*")/.exec(firstPage);
  if (!match) return null;
  try { const value: unknown = JSON.parse(match[2]); return typeof value === "string" && value.trim() ? value : null; } catch { return null; }
}

const hex = (bytes: ArrayBuffer) => [...new Uint8Array(bytes)].map(byte => byte.toString(16).padStart(2, "0")).join("");

/**
 * Join every page of a large tool input and check the result is exactly what
 * the engine holds: the same size in bytes and the same SHA-256. Only then is
 * it shown as the full content and may be allowed (PR #19 review B1).
 */
export async function assembleToolInput(pages: readonly string[], inspection: Pick<ApprovalInspection, "toolInputBytes" | "toolInputSha256" | "toolInputPages">): Promise<string> {
  if (inspection.toolInputPages === undefined || pages.length !== inspection.toolInputPages) throw new Error("tool input pages missing");
  const text = pages.join("");
  const bytes = new TextEncoder().encode(text);
  if (bytes.length !== inspection.toolInputBytes) throw new Error("tool input size differs");
  if (hex(await crypto.subtle.digest("SHA-256", bytes)) !== inspection.toolInputSha256) throw new Error("tool input hash differs");
  return text;
}

/** Notices about requests of this conversation only: a late notice from a
 * task left behind never shows in another one. */
export function noticesForChat<T extends { chatId: string }>(items: readonly T[], chatId: string | null): T[] {
  return chatId === null ? [] : items.filter(item => item.chatId === chatId);
}
