/** Work-history rows: one tool use per "activity" output row, written by the
 * engine as compact JSON (crates/elsecrew-sidecar/.../preparation/activity.rs).
 * A row names the kind of work and what it worked on: a file inside the
 * workspace, a search, or the command line. When the tool use ends, a second
 * row with the same `id` and `phase: "done"` says how it ended; it is shown
 * in place of the first.
 *
 * The engine masks secrets before a row is stored. Every string is still
 * untrusted here: it is drawn as text only (never markdown or HTML), control
 * and direction marks are neutralised, secret shapes are masked again with
 * the same rules (rows from older builds were stored unmasked), and every
 * field has a length cap. Anything unexpected is not shown as history. */
import { maskSecrets, redactCommand } from "./secret-shape.ts";

export type WorkAction = "read" | "edit" | "write" | "delete" | "search" | "command" | "other";
export type WorkOutcome = "refused" | "failed" | "stopped";
export interface WorkActivity {
  action: WorkAction;
  target?: string;
  tool?: string;
  outcome?: WorkOutcome;
  /** The app's own key linking a tool use's first row with its closing row. */
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
  /** The command could not be read safely; only its program name is kept. */
  argsHidden?: boolean;
}

const ACTIONS: readonly string[] = ["read", "edit", "write", "delete", "search", "command", "other"];
const OUTCOMES: readonly string[] = ["refused", "failed", "stopped"];
const LINE_FIELDS = ["target", "tool", "cwd"] as const;
const BLOCK_FIELDS = ["command", "note"] as const;
const NUMBER_FIELDS = ["exit", "ms", "added", "removed"] as const;
const KEYS: readonly string[] = ["action", "outcome", "phase", "id", "argsHidden", ...LINE_FIELDS, ...BLOCK_FIELDS, ...NUMBER_FIELDS];
/** The engine's keys are short and plain; anything else is not a key. */
const ID_SHAPE = /^[A-Za-z0-9_:-]{1,64}$/;
/** Characters (code points) shown. The engine writes at most 161 for a
 * line and 4001 for a block; older rows stay within these too. */
export const LINE_MAX = 160;
export const BLOCK_MAX = 4000;
/** Rows bigger than this are not read at all. */
const FIELD_LIMIT = 16 * 1024;

/** Controls, line/paragraph separators and invisible format marks that could
 * reorder or hide the text (the engine's `neutralise` uses the same set). */
const MARKS = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u061c\u200b-\u200f\u2028-\u202e\u2060-\u2069\ufeff]/gu;

/** Marks that could reorder or hide text become a visible U+FFFD; a line
 * keeps no line breaks, a block keeps them (and tabs). */
export function neutralise(text: string, block = false): string {
  const clean = text.replace(/\r\n/g, "\n").replace(MARKS, "\ufffd");
  return block ? clean : clean.replace(/\n/g, "\ufffd");
}

/** At most `max` characters, with … when cut. */
export function capped(text: string, max: number): string {
  const points = Array.from(text);
  return points.length > max ? `${points.slice(0, max).join("")}…` : text;
}

/** One line as shown: neutralised, masked, then capped (masking first, so a
 * cut never leaves part of a secret that masking would have caught). The
 * engine's `display_line` gives the same result (shared cases). */
export function displayLine(text: string): string {
  return capped(maskSecrets(neutralise(text, false)), LINE_MAX);
}

function shown(text: string, block: boolean): string {
  return block ? capped(maskSecrets(neutralise(text, true)), BLOCK_MAX) : displayLine(text);
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
  if ("id" in row && (typeof row.id !== "string" || !ID_SHAPE.test(row.id))) return null;
  const out: WorkActivity = { action: row.action as WorkAction };
  if (row.outcome) out.outcome = row.outcome as WorkOutcome;
  if (row.phase) out.phase = "done";
  if (typeof row.id === "string") out.id = row.id;
  if ("argsHidden" in row) { if (row.argsHidden !== true) return null; out.argsHidden = true; }
  for (const key of [...LINE_FIELDS, ...BLOCK_FIELDS]) {
    if (!(key in row)) continue;
    const field = row[key];
    if (typeof field !== "string" || !field.trim() || field.length > FIELD_LIMIT) return null;
    // A command line (and a command row's target) is redacted fail-closed
    // again, so rows from older builds show no more than new ones.
    const command = key === "command" || (key === "target" && row.action === "command");
    const value = command ? redactCommand(neutralise(field, true)).text : field;
    if (command && !value.trim()) continue;
    out[key] = shown(value, (BLOCK_FIELDS as readonly string[]).includes(key));
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
  stopped: "m5.output.activity.stopped",
} as const;

type LineLabel = typeof WORK_ACTION_KEYS[WorkAction] | "m5.output.activity.searchFiles" | "m5.output.activity.searchText" | "m5.output.activity.webFetch" | "m5.output.activity.webSearch";

/**
 * One activity line as shown (D6): a Korean verb for the kind of work and
 * what it worked on — a command's first line, a file's place in the
 * workspace, a search and where, a web page's domain. `file` is the line's
 * file only when it changed something and nothing went wrong; whether it can
 * be opened is up to the task's real change list (`openableFile`).
 */
export function activityLine(activity: WorkActivity): { label: LineLabel; target: string | null; file: string | null } {
  const label: LineLabel = activity.action === "search" && activity.tool === "Glob" ? "m5.output.activity.searchFiles"
    : activity.action === "search" && activity.tool === "Grep" ? "m5.output.activity.searchText"
    : activity.action === "other" && activity.tool === "WebFetch" ? "m5.output.activity.webFetch"
    : activity.action === "other" && activity.tool === "WebSearch" ? "m5.output.activity.webSearch"
    : WORK_ACTION_KEYS[activity.action];
  const target = activity.target ?? null;
  const changes = activity.action === "edit" || activity.action === "write" || activity.action === "delete";
  return { label, target, file: changes && !activity.outcome ? target : null };
}

/**
 * The change-list path a line's file opens, or null. Only a path that is in
 * the task's current change list exactly (`changed`, the sheet's own keys)
 * opens: a masked, cut or neutralised name, a file outside the workspace or
 * one the task did not change has no button.
 */
export function openableFile(file: string | null, changed: ReadonlySet<string> | null | undefined): string | null {
  return file !== null && changed?.has(file) ? file : null;
}

/** What the folded detail of a line holds; null when there is nothing more to show. */
export function activityDetail(activity: WorkActivity): {
  command: string | null; cwd: string | null; note: string | null; exit: number | null;
  ms: number | null; lines: { added: number; removed: number } | null;
} | null {
  const detail = {
    command: activity.command ?? null,
    cwd: activity.cwd ?? null,
    note: activity.note ?? null,
    exit: activity.exit ?? null,
    ms: activity.ms ?? null,
    lines: activity.added !== undefined || activity.removed !== undefined ? { added: activity.added ?? 0, removed: activity.removed ?? 0 } : null,
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
 * Rows as lines: a tool use's closing row takes the place of its first row
 * (each closing row is used once, by the first earlier row with its id,
 * kind and target); consecutive changes of the same file read as one line
 * with how many times and the lines changed in all; identical rows read once
 * with a count. Rows that are not work history are kept as they are.
 */
export function activityItems<T>(rows: readonly T[], text: (row: T) => string | null): ({ row: T; activity: null; count: 1 } | ActivityItem<T>)[] {
  const parsed = rows.map(row => { const value = text(row); return value === null ? null : parseWorkActivity(value); });
  const used = new Set<number>();
  const out: ({ row: T; activity: null; count: 1 } | ActivityItem<T>)[] = [];
  parsed.forEach((activity, index) => {
    const row = rows[index]!;
    if (text(row) === null) { out.push({ row, activity: null, count: 1 }); return; }
    if (!activity || used.has(index)) return;
    let shownActivity = activity;
    if (activity.id && activity.phase !== "done") {
      const end = parsed.findIndex((other, at) => at > index && !used.has(at) && other?.phase === "done"
        && other.id === activity.id && other.action === activity.action && other.target === activity.target);
      if (end >= 0) { shownActivity = parsed[end]!; used.add(end); }
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
  const changes = (activity: WorkActivity) => activity.action === "edit" || activity.action === "write";
  return changes(a) && changes(b) && !a.outcome && !b.outcome
    && a.target !== undefined && a.target === b.target && Boolean(a.id) && Boolean(b.id);
}

/** Two changes of one file as one line: lines changed summed, latest state kept. */
function combine(a: WorkActivity, b: WorkActivity): WorkActivity {
  const lines = a.added !== undefined || b.added !== undefined
    ? { added: (a.added ?? 0) + (b.added ?? 0), removed: (a.removed ?? 0) + (b.removed ?? 0) } : {};
  const ms = a.ms !== undefined || b.ms !== undefined ? { ms: (a.ms ?? 0) + (b.ms ?? 0) } : {};
  return { ...b, action: a.action === "write" && b.action === "write" ? "write" : "edit", ...lines, ...ms };
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
