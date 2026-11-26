/** Work-history rows: one tool use per "activity" output row, written by the
 * engine as compact JSON (crates/elsecrew-sidecar/.../preparation/activity.rs).
 * A row names the kind of work and what it worked on: a file inside the
 * workspace, a search, or the command line. When the tool use ends, a second
 * row with the same `id` and `phase: "done"` says how it ended; it is shown
 * in place of the first.
 *
 * Every string is untrusted. It is drawn as text only (never markdown or
 * HTML), control and direction marks are neutralised, anything that looks
 * like a secret is masked again here (rows from older builds were stored
 * unmasked), and every field has a length cap. Anything unexpected is not
 * shown as history. */
import { maskSecrets } from "./secret-shape.ts";

export type WorkAction = "read" | "edit" | "write" | "delete" | "search" | "command" | "other";
export type WorkOutcome = "refused" | "failed";
export interface WorkActivity {
  action: WorkAction;
  target?: string;
  tool?: string;
  outcome?: WorkOutcome;
  /** Links a tool use's first row with the row that says how it ended. */
  id?: string;
  phase?: "done";
  /** The whole command line (masked by the engine), and where it ran. */
  command?: string;
  cwd?: string;
  /** The agent's own one-line summary of a command. */
  note?: string;
  exit?: number;
  ms?: number;
  added?: number;
  removed?: number;
  /** The first lines of a command's output. */
  output?: string;
}

const ACTIONS: readonly string[] = ["read", "edit", "write", "delete", "search", "command", "other"];
const OUTCOMES: readonly string[] = ["refused", "failed"];
const LINE_FIELDS = ["target", "tool", "id", "cwd"] as const;
const BLOCK_FIELDS = ["command", "note", "output"] as const;
const NUMBER_FIELDS = ["exit", "ms", "added", "removed"] as const;
const KEYS: readonly string[] = ["action", "outcome", "phase", ...LINE_FIELDS, ...BLOCK_FIELDS, ...NUMBER_FIELDS];
/** Characters (code points) shown. The engine writes at most 161 for a
 * line and 4001 for a block; older rows stay within these too. */
export const LINE_MAX = 160;
export const BLOCK_MAX = 4000;
/** Rows bigger than this are not read at all. */
const FIELD_LIMIT = 16 * 1024;

/** Controls and invisible format marks that could reorder or hide the text. */
const MARKS = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f؜​-‏‪-‮⁠-⁩﻿]/gu;

/** Marks that could reorder or hide text become a visible �; a line keeps
 * no line breaks, a block keeps them (and tabs). */
export function neutralise(text: string, block = false): string {
  const clean = text.replace(/\r\n?/g, "\n").replace(MARKS, "�");
  return block ? clean : clean.replace(/\n/g, "�");
}

/** At most `max` characters, with … when cut. */
export function capped(text: string, max: number): string {
  const points = Array.from(text);
  return points.length > max ? `${points.slice(0, max).join("")}…` : text;
}

/** Display form of one untrusted field: neutralised, masked, capped. */
function shown(text: string, block: boolean): string {
  return capped(maskSecrets(neutralise(text, block)), block ? BLOCK_MAX : LINE_MAX);
}

export function parseWorkActivity(text: string): WorkActivity | null {
  let value: unknown;
  try { value = JSON.parse(text); } catch { return null; }
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const row = value as Record<string, unknown>;
  if (Object.keys(row).some(key => !KEYS.includes(key))) return null;
  if (typeof row.action !== "string" || !ACTIONS.includes(row.action)) return null;
  if ("outcome" in row && (typeof row.outcome !== "string" || !OUTCOMES.includes(row.outcome))) return null;
  if ("phase" in row && row.phase !== "done") return null;
  const out: WorkActivity = { action: row.action as WorkAction };
  if (row.outcome) out.outcome = row.outcome as WorkOutcome;
  if (row.phase) out.phase = "done";
  for (const key of [...LINE_FIELDS, ...BLOCK_FIELDS]) {
    if (!(key in row)) continue;
    const field = row[key];
    if (typeof field !== "string" || !field.trim() || field.length > FIELD_LIMIT) return null;
    out[key] = shown(field, (BLOCK_FIELDS as readonly string[]).includes(key));
  }
  for (const key of NUMBER_FIELDS) {
    if (!(key in row)) continue;
    const field = row[key];
    if (typeof field !== "number" || !Number.isSafeInteger(field) || (key !== "exit" && field < 0)) return null;
    out[key] = field;
  }
  return out;
}

export const WORK_ACTION_KEYS = {
  read: "m5.output.activity.read",
  edit: "m5.output.activity.edit",
  write: "m5.output.activity.write",
  delete: "m5.output.activity.delete",
  search: "m5.output.activity.search",
  command: "m5.output.activity.command",
  other: "m5.output.activity.other",
} as const;

export const WORK_OUTCOME_KEYS = {
  refused: "m5.output.activity.refused",
  failed: "m5.output.activity.failed",
} as const;

type LineLabel = typeof WORK_ACTION_KEYS[WorkAction] | "m5.output.activity.searchFiles" | "m5.output.activity.searchText" | "m5.output.activity.webFetch" | "m5.output.activity.webSearch";

/** A path inside the workspace (not `~/…`, absolute, or climbing out). */
export function workspacePath(target: string | undefined): string | null {
  if (!target || target === "." || target.startsWith("/") || target.startsWith("~") || /^[A-Za-z]:[\\/]/.test(target)) return null;
  if (target.split(/[\\/]/).includes("..") || target.includes("�") || target.endsWith("…")) return null;
  return target;
}

/**
 * One activity line as shown (D6): a Korean verb for the kind of work and
 * what it worked on — a command's first line, a file's place in the
 * workspace, a search and where, a web page's domain. `file` is set when the
 * line names a file inside the workspace whose change can be opened.
 */
export function activityLine(activity: WorkActivity): { label: LineLabel; target: string | null; file: string | null } {
  const label: LineLabel = activity.action === "search" && activity.tool === "Glob" ? "m5.output.activity.searchFiles"
    : activity.action === "search" && activity.tool === "Grep" ? "m5.output.activity.searchText"
    : activity.action === "other" && activity.tool === "WebFetch" ? "m5.output.activity.webFetch"
    : activity.action === "other" && activity.tool === "WebSearch" ? "m5.output.activity.webSearch"
    : WORK_ACTION_KEYS[activity.action];
  const target = activity.target ?? null;
  const changes = activity.action === "edit" || activity.action === "write" || activity.action === "delete";
  return { label, target, file: changes ? workspacePath(activity.target) : null };
}

/** What the folded detail of a line holds; null when there is nothing more to show. */
export function activityDetail(activity: WorkActivity): {
  command: string | null; cwd: string | null; note: string | null; exit: number | null;
  ms: number | null; lines: { added: number; removed: number } | null; output: string | null;
} | null {
  const detail = {
    command: activity.command ?? null,
    cwd: activity.cwd ?? null,
    note: activity.note ?? null,
    exit: activity.exit ?? null,
    ms: activity.ms ?? null,
    lines: activity.added !== undefined || activity.removed !== undefined ? { added: activity.added ?? 0, removed: activity.removed ?? 0 } : null,
    output: activity.output ?? null,
  };
  // A command whose whole line is already the target, and nothing else, adds nothing folded.
  if (detail.command === activity.target && Object.entries(detail).every(([key, value]) => key === "command" || value === null)) return null;
  return Object.values(detail).some(value => value !== null) ? detail : null;
}

/** Seconds or minutes for a tool use's time: "0.1초 미만", "0.4초", "12초", "2분 5초". */
export function durationParts(ms: number): { key: "m5.output.activity.instant" | "m5.output.activity.ms" | "m5.output.activity.seconds" | "m5.output.activity.minutes"; values: Record<string, number> } {
  if (ms < 100) return { key: "m5.output.activity.instant", values: {} };
  if (ms < 1000) return { key: "m5.output.activity.ms", values: { s: Math.round(ms / 100) / 10 } };
  const seconds = Math.round(ms / 1000);
  return seconds < 60 ? { key: "m5.output.activity.seconds", values: { s: seconds } } : { key: "m5.output.activity.minutes", values: { m: Math.floor(seconds / 60), s: seconds % 60 } };
}

export interface ActivityItem<T> {
  row: T;
  activity: WorkActivity;
  /** How many tool uses this line stands for (same file edited in a row, or identical rows). */
  count: number;
}

/**
 * Rows as lines: a tool use's closing row takes the place of its first row;
 * consecutive edits of the same file read as one line with how many times
 * and the lines changed in all; identical rows read once with a count.
 * Rows that are not work history are kept as they are (activity null).
 */
export function activityItems<T>(rows: readonly T[], text: (row: T) => string | null): ({ row: T; activity: null; count: 1 } | ActivityItem<T>)[] {
  const parsed = rows.map(row => { const value = text(row); return value === null ? null : parseWorkActivity(value); });
  const closing = new Map<string, number>();
  parsed.forEach((activity, index) => { if (activity?.id && activity.phase === "done") closing.set(activity.id, index); });
  const used = new Set<number>();
  const out: ({ row: T; activity: null; count: 1 } | ActivityItem<T>)[] = [];
  parsed.forEach((activity, index) => {
    const row = rows[index]!;
    if (text(row) === null) { out.push({ row, activity: null, count: 1 }); return; }
    if (!activity || used.has(index)) return;
    let shownActivity = activity;
    if (activity.id && activity.phase !== "done") {
      const end = closing.get(activity.id);
      if (end !== undefined && end > index) { shownActivity = parsed[end]!; used.add(end); }
    }
    const last = out[out.length - 1];
    if (last?.activity && sameFile(last.activity, shownActivity)) {
      last.count++;
      last.activity = combine(last.activity, shownActivity);
      return;
    }
    if (last?.activity && !shownActivity.id && !last.activity.id && JSON.stringify(last.activity) === JSON.stringify(shownActivity)) { last.count++; return; }
    out.push({ row, activity: shownActivity, count: 1 });
  });
  return out;
}

function sameFile(a: WorkActivity, b: WorkActivity): boolean {
  return a.action === "edit" && (b.action === "edit" || b.action === "write") && !a.outcome && !b.outcome
    && a.target !== undefined && a.target === b.target && Boolean(a.id) && Boolean(b.id);
}

/** Two edits of one file as one line: lines changed summed, latest state kept. */
function combine(a: WorkActivity, b: WorkActivity): WorkActivity {
  const lines = a.added !== undefined || b.added !== undefined
    ? { added: (a.added ?? 0) + (b.added ?? 0), removed: (a.removed ?? 0) + (b.removed ?? 0) } : {};
  const ms = a.ms !== undefined || b.ms !== undefined ? { ms: (a.ms ?? 0) + (b.ms ?? 0) } : {};
  return { ...b, action: "edit", ...lines, ...ms };
}

/** Consecutive identical rows read once, with how many times they happened. */
export function collapseRepeats<T>(rows: readonly T[], same: (a: T, b: T) => boolean): { row: T; count: number }[] {
  const out: { row: T; count: number }[] = [];
  for (const row of rows) {
    const last = out[out.length - 1];
    if (last && same(last.row, row)) last.count++;
    else out.push({ row, count: 1 });
  }
  return out;
}
