export interface DiffRow {
  text: string;
  kind: "metadata" | "context" | "added" | "deleted";
  before: number | null;
  after: number | null;
}

export const DIFF_PAGE_ROWS = 200;

/** Presentation only. Never use these rows to apply a patch or authorize a write. */
export function diffRows(text: string): DiffRow[] {
  const lines = text.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  const raw = () => lines.map((text): DiffRow => ({ text, kind: "metadata", before: null, after: null }));
  const rows: DiffRow[] = [];
  let before = 0, after = 0, oldLeft = 0, newLeft = 0;
  for (const text of lines) {
    const row: DiffRow = { text, kind: "metadata", before: null, after: null };
    if (text.startsWith("@@")) {
      const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?: .*)?$/.exec(text);
      if (!match || oldLeft || newLeft) return raw();
      [before, oldLeft, after, newLeft] = [Number(match[1]), Number(match[2] ?? 1), Number(match[3]), Number(match[4] ?? 1)];
      if (![before, oldLeft, after, newLeft, before + oldLeft, after + newLeft].every(Number.isSafeInteger)) return raw();
    } else if (text === "\\ No newline at end of file") {
      // This marker consumes no source line and remains visible as metadata.
    } else if (oldLeft || newLeft) {
      if (text.startsWith(" ") && oldLeft > 0 && newLeft > 0) {
        row.kind = "context"; row.before = before++; row.after = after++; oldLeft--; newLeft--;
      } else if (text.startsWith("-") && oldLeft > 0) {
        row.kind = "deleted"; row.before = before++; oldLeft--;
      } else if (text.startsWith("+") && newLeft > 0) {
        row.kind = "added"; row.after = after++; newLeft--;
      } else return raw();
    }
    rows.push(row);
  }
  return oldLeft || newLeft ? raw() : rows;
}
