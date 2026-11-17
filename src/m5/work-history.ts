/** Work-history rows: one tool use per "activity" output row, written by the
 * engine as compact JSON. Shows the kind of work and where in the workspace,
 * never the tool input. Anything unexpected is not shown as history. */
export type WorkAction = "read" | "edit" | "write" | "search" | "command" | "other";
export type WorkOutcome = "refused" | "failed";
export interface WorkActivity {
  action: WorkAction;
  target?: string;
  tool?: string;
  outcome?: WorkOutcome;
}

const ACTIONS: readonly string[] = ["read", "edit", "write", "search", "command", "other"];
const OUTCOMES: readonly string[] = ["refused", "failed"];
/** Characters (code points); the engine writes at most 161. */
const TEXT_MAX = 256;

function short(value: unknown): value is string {
  if (typeof value !== "string" || !value.length || Array.from(value).length > TEXT_MAX) return false;
  // Controls and invisible format marks that could reorder or hide the text.
  return !/[\u0000-\u001f\u007f-\u009f\u061c\u200b-\u200f\u202a-\u202e\u2060-\u2069\ufeff]/u.test(value);
}

export function parseWorkActivity(text: string): WorkActivity | null {
  let value: unknown;
  try { value = JSON.parse(text); } catch { return null; }
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const row = value as Record<string, unknown>;
  if (Object.keys(row).some(key => !["action", "target", "tool", "outcome"].includes(key))) return null;
  if (typeof row.action !== "string" || !ACTIONS.includes(row.action)) return null;
  if ("target" in row && !short(row.target)) return null;
  if ("tool" in row && !short(row.tool)) return null;
  if ("outcome" in row && (typeof row.outcome !== "string" || !OUTCOMES.includes(row.outcome))) return null;
  return row as unknown as WorkActivity;
}

export const WORK_ACTION_KEYS = {
  read: "m5.output.activity.read",
  edit: "m5.output.activity.edit",
  write: "m5.output.activity.write",
  search: "m5.output.activity.search",
  command: "m5.output.activity.command",
  other: "m5.output.activity.other",
} as const;

export const WORK_OUTCOME_KEYS = {
  refused: "m5.output.activity.refused",
  failed: "m5.output.activity.failed",
} as const;
