/** Bounded live view of validated durable pages. Eviction affects only this
 * window, never the ledger. History must be fetched separately by cursor. */
import { validateAgentPublicEventAgainstRunV1, type AgentRunBindingV1 } from "./contracts/agent-adapter-v1.ts";
import type { OutputRow, OutputItemUpdate } from "./contracts/agent-output-pages.ts";

export const OUTPUT_WINDOW_ROWS = 200;
export const OUTPUT_WINDOW_BYTES = 4 * 1024 * 1024;
const ITEM_BYTES = 64 * 1024;
const encoder = new TextEncoder();

export interface OutputDisplayRow {
  key: string;
  lastSequence: number;
  text: string;
  textBytes: number;
  kind: string;
  channel?: OutputItemUpdate["channel"];
  complete: boolean;
  truncated: boolean;
}
export interface OutputWindow {
  binding: AgentRunBindingV1;
  cursor: number;
  rows: readonly OutputDisplayRow[];
  hasEarlier: boolean;
}

function suffix(text: string): { text: string; textBytes: number; truncated: boolean } {
  const length = encoder.encode(text).length;
  if (length <= ITEM_BYTES) return { text, textBytes: length, truncated: false };
  // Input is at most the retained item plus one bounded incoming event.
  const points = Array.from(text);
  let bytes = 0;
  let start = points.length;
  while (start > 0) {
    const size = encoder.encode(points[start - 1]!).length;
    if (bytes + size > ITEM_BYTES) break;
    bytes += size;
    start--;
  }
  return { text: points.slice(start).join(""), textBytes: bytes, truncated: true };
}

/** Caller decodes OutputPage first. Batch acceptance is atomic: on failure the
 * old cursor/window are unchanged, so the reader must not ACK the failed page. */
export function reduceOutputWindow(previous: OutputWindow, incoming: readonly OutputRow[]): OutputWindow {
  const rows = previous.rows.map(row => ({ ...row }));
  let cursor = previous.cursor;
  let hasEarlier = previous.hasEarlier;
  for (const row of incoming) {
    if (!Number.isSafeInteger(row.outputSequence) || row.outputSequence !== cursor + 1) throw new Error("output sequence gap");
    const event = validateAgentPublicEventAgainstRunV1(row.event, previous.binding);
    const update = row.itemUpdate;
    const key = update ? `item:${update.itemId}` : `event:${event.eventId}`;
    const index = rows.findIndex(value => value.key === key);
    const current = index < 0 ? undefined : rows[index];
    if (update && current && (current.channel !== update.channel
      || (current.complete && (update.operation !== "complete" || event.kind !== "final" || current.kind === "final")))) {
      throw new Error("output item transition differs");
    }
    const append = update?.operation === "append";
    const clipped = suffix(append ? (current?.text ?? "") + event.text : event.text);
    if (current?.complete && update && current.text !== clipped.text) throw new Error("final output text differs");
    const next: OutputDisplayRow = {
      key, lastSequence: row.outputSequence, text: clipped.text, textBytes: clipped.textBytes, kind: event.kind,
      ...(update ? { channel: update.channel } : {}),
      complete: !append,
      truncated: clipped.truncated || (append && (current?.truncated ?? cursor > 0)),
    };
    if (index < 0) rows.push(next); else rows[index] = next;
    let bytes = rows.reduce((sum, value) => sum + value.textBytes, 0);
    while (rows.length > OUTPUT_WINDOW_ROWS || bytes > OUTPUT_WINDOW_BYTES) {
      bytes -= rows.shift()!.textBytes;
      hasEarlier = true;
    }
    cursor = row.outputSequence;
  }
  return { binding: previous.binding, cursor, rows, hasEarlier };
}
