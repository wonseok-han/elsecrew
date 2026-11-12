/** Durable-output page validation shared by transport boundaries. */
import {
  validateAgentPublicEventAgainstRunV1,
  validateAgentRunBindingV1,
  type AgentPublicEventV1,
  type AgentRunBindingV1,
} from "./agent-adapter-v1.ts";

export const OUTPUT_FORMAT_VERSION = 1;
export const OUTPUT_PAGE_MAX_ITEMS = 100;
export const OUTPUT_PAGE_MAX_BYTES = 512 * 1024;

export interface OutputPageRequest {
  formatVersion: 1;
  binding: AgentRunBindingV1;
  afterSequence: number;
  throughSequence: number;
  limit: number;
}
export interface OutputItemUpdate {
  itemId: string;
  operation: "append" | "complete";
  channel: "message" | "plan";
}
export interface OutputRow { outputSequence: number; event: AgentPublicEventV1; itemUpdate?: OutputItemUpdate }
/** sealed closes output only; it does not prove process termination/success. */
export interface OutputHead {
  formatVersion: 1;
  binding: AgentRunBindingV1;
  committedSequence: number;
  sealed: boolean;
}
export interface OutputPage {
  request: OutputPageRequest;
  rows: OutputRow[];
  nextAfterSequence: number;
}
function fail(): never { throw new Error("invalid durable output page"); }
function object(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return fail();
  const result = value as Record<string, unknown>;
  if (Object.keys(result).length !== keys.length || keys.some(key => !Object.prototype.hasOwnProperty.call(result, key))) return fail();
  return result;
}
function sequence(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) return fail();
  return value;
}
export function validateOutputHead(value: unknown, expectedValue: AgentRunBindingV1): OutputHead {
  const head = object(value, ["formatVersion", "binding", "committedSequence", "sealed"]);
  const expected = validateAgentRunBindingV1(expectedValue);
  const binding = validateAgentRunBindingV1(head.binding);
  if (head.formatVersion !== OUTPUT_FORMAT_VERSION || typeof head.sealed !== "boolean"
    || (Object.keys(expected) as (keyof AgentRunBindingV1)[]).some(key => binding[key] !== expected[key])) return fail();
  return { formatVersion: OUTPUT_FORMAT_VERSION, binding, committedSequence: sequence(head.committedSequence), sealed: head.sealed };
}
export function validateOutputPageRequest(value: unknown): OutputPageRequest {
  const request = object(value, ["formatVersion", "binding", "afterSequence", "throughSequence", "limit"]);
  const binding = validateAgentRunBindingV1(request.binding);
  const afterSequence = sequence(request.afterSequence);
  const throughSequence = sequence(request.throughSequence);
  const limit = sequence(request.limit);
  if (request.formatVersion !== OUTPUT_FORMAT_VERSION || afterSequence > throughSequence
    || limit < 1 || limit > OUTPUT_PAGE_MAX_ITEMS) return fail();
  return { formatVersion: OUTPUT_FORMAT_VERSION, binding, afterSequence, throughSequence, limit };
}

/** Validate bytes before decoding; storage must separately prove the committed
 * watermark and cross-page event-ID uniqueness. Transport seq is not used. */
export function decodeOutputPage(bytes: Uint8Array, expectedValue: OutputPageRequest): OutputPage {
  if (bytes.byteLength > OUTPUT_PAGE_MAX_BYTES) return fail();
  const value: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  return validatePage(value, expectedValue);
}

/** For already JSON-decoded transport values. Preserve the same serialized byte
 * budget without decoding and parsing a second full copy of the page. */
export function validateDecodedOutputPage(value: unknown, expectedValue: OutputPageRequest): OutputPage {
  const serialized = JSON.stringify(value);
  if (typeof serialized !== "string" || new TextEncoder().encode(serialized).byteLength > OUTPUT_PAGE_MAX_BYTES) return fail();
  return validatePage(value, expectedValue);
}

function validatePage(value: unknown, expectedValue: OutputPageRequest): OutputPage {
  const expected = validateOutputPageRequest(expectedValue);
  const page = object(value, ["request", "rows", "nextAfterSequence"]);
  const request = validateOutputPageRequest(page.request);
  if (request.afterSequence !== expected.afterSequence || request.throughSequence !== expected.throughSequence
    || request.limit !== expected.limit
    || (Object.keys(expected.binding) as (keyof AgentRunBindingV1)[]).some(key => request.binding[key] !== expected.binding[key])) return fail();
  if (!Array.isArray(page.rows) || page.rows.length > expected.limit) return fail();
  let next = expected.afterSequence;
  const ids = new Set<string>();
  const rows: OutputRow[] = [];
  for (const raw of page.rows) {
    const hasUpdate = !!raw && typeof raw === "object" && Object.prototype.hasOwnProperty.call(raw, "itemUpdate");
    const row = object(raw, hasUpdate ? ["outputSequence", "event", "itemUpdate"] : ["outputSequence", "event"]);
    const outputSequence = sequence(row.outputSequence);
    if (next === expected.throughSequence || outputSequence !== next + 1) return fail();
    const event = validateAgentPublicEventAgainstRunV1(row.event, expected.binding);
    if (ids.has(event.eventId)) return fail();
    ids.add(event.eventId);
    let itemUpdate: OutputItemUpdate | undefined;
    if (hasUpdate && row.itemUpdate !== null) {
      const update = object(row.itemUpdate, ["itemId", "operation", "channel"]);
      if (typeof update.itemId !== "string" || !update.itemId.length
        || new TextEncoder().encode(update.itemId).length > 128 || /[\u0000-\u001f\u007f-\u009f]/u.test(update.itemId)
        || (update.operation !== "append" && update.operation !== "complete")
        || (update.channel !== "message" && update.channel !== "plan")
        || (event.kind !== "status" && event.kind !== "final")
        || (event.kind === "final" && update.operation !== "complete")) return fail();
      itemUpdate = update as unknown as OutputItemUpdate;
    }
    rows.push({ outputSequence, event, ...(itemUpdate ? { itemUpdate } : {}) });
    next = outputSequence;
  }
  if (sequence(page.nextAfterSequence) !== next || (!rows.length && next !== expected.throughSequence)) return fail();
  return { request, rows, nextAfterSequence: next };
}
