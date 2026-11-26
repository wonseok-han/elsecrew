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

/**
 * What the person has reviewed of each waiting request on this card (PR #19
 * re-review B1). Allowing needs, at the moment of the click: the request's
 * details open right now, and for a large tool input every page received and
 * checked (size and SHA-256). Closing the details, a changed request or a new
 * read starts that request's review over; a load that finishes after that is
 * dropped (its generation no longer matches).
 */
export interface ApprovalReview {
  readonly open: ReadonlySet<string>;
  readonly full: ReadonlyMap<string, string>;
  readonly progress: ReadonlyMap<string, number | "failed">;
  readonly generation: ReadonlyMap<string, number>;
}

export const EMPTY_REVIEW: ApprovalReview = { open: new Set(), full: new Map(), progress: new Map(), generation: new Map() };

function without<K, V>(map: ReadonlyMap<K, V>, key: K): ReadonlyMap<K, V> {
  if (!map.has(key)) return map;
  const next = new Map(map); next.delete(key); return next;
}

/** The current generation of a request's review: a load carries it and only
 * lands if it still matches. */
export function reviewGeneration(review: ApprovalReview, id: string): number { return review.generation.get(id) ?? 0; }

/** The details were opened. A failed load may be tried again. */
export function reviewOpened(review: ApprovalReview, id: string): ApprovalReview {
  const progress = review.progress.get(id) === "failed" ? without(review.progress, id) : review.progress;
  return review.open.has(id) && progress === review.progress ? review : { ...review, open: new Set(review.open).add(id), progress };
}

/** The details were closed, the request changed, or it was read again: its
 * review starts over and any load still running for it is dropped. */
export function reviewReset(review: ApprovalReview, id: string): ApprovalReview {
  const open = new Set(review.open); open.delete(id);
  return { open, full: without(review.full, id), progress: without(review.progress, id), generation: new Map(review.generation).set(id, reviewGeneration(review, id) + 1) };
}

/** Pages read so far (or a failed check) for the load of this generation. */
export function reviewProgress(review: ApprovalReview, id: string, generation: number, value: number | "failed"): ApprovalReview {
  if (generation !== reviewGeneration(review, id) || !review.open.has(id)) return review;
  return { ...review, progress: new Map(review.progress).set(id, value) };
}

/** Every page received and checked: it counts only if this load is still the
 * current one and the details are still open. */
export function reviewLoaded(review: ApprovalReview, id: string, generation: number, text: string): ApprovalReview {
  if (generation !== reviewGeneration(review, id) || !review.open.has(id)) return review;
  return { ...review, full: new Map(review.full).set(id, text), progress: without(review.progress, id) };
}

/** Forget requests no longer waiting. */
export function reviewRetain(review: ApprovalReview, still: ReadonlySet<string>): ApprovalReview {
  const keep = <V>(map: ReadonlyMap<string, V>) => [...map.keys()].every(id => still.has(id)) ? map : new Map([...map].filter(([id]) => still.has(id)));
  const open = [...review.open].every(id => still.has(id)) ? review.open : new Set([...review.open].filter(id => still.has(id)));
  return { open, full: keep(review.full), progress: keep(review.progress), generation: keep(review.generation) };
}

/** The one check the allow button and its click both use. */
export function reviewAllows(input: { approvalId: string; inspection: ApprovalInspection | null; review: ApprovalReview; sending: boolean; error: boolean }): boolean {
  const paged = input.inspection?.toolInputPages !== undefined;
  return canAcceptApproval({ approvalId: input.approvalId, inspectedId: input.inspection?.approvalId ?? null, openedIds: input.review.open,
    sending: input.sending, error: input.error, fullyRead: !paged || input.review.full.has(input.approvalId) });
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
