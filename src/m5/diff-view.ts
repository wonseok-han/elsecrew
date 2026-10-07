export interface DiffRow {
  text: string;
  kind: "metadata" | "context" | "added" | "deleted";
  before: number | null;
  after: number | null;
}

export const DIFF_PAGE_ROWS = 200;

interface Cursor {
  offset: number;
  before: number;
  after: number;
  oldLeft: number;
  newLeft: number;
  valid: boolean;
}

function nextRow(source: string, cursor: Cursor): DiffRow {
    const end = source.indexOf("\n", cursor.offset);
    const text = source.slice(cursor.offset, end < 0 ? source.length : end);
    cursor.offset = end < 0 ? source.length : end + 1;
    let { before, after, oldLeft, newLeft } = cursor;
    const row: DiffRow = { text, kind: "metadata", before: null, after: null };
    if (!cursor.valid) return row;
    if (text.startsWith("@@")) {
      const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?: .*)?$/.exec(text);
      if (!match || oldLeft || newLeft) { cursor.valid = false; return row; }
      [before, oldLeft, after, newLeft] = [Number(match[1]), Number(match[2] ?? 1), Number(match[3]), Number(match[4] ?? 1)];
      if (![before, oldLeft, after, newLeft, before + oldLeft, after + newLeft].every(Number.isSafeInteger)) { cursor.valid = false; return row; }
    } else if (text === "\\ No newline at end of file") {
      // This marker consumes no source line and remains visible as metadata.
    } else if (oldLeft || newLeft) {
      if (text.startsWith(" ") && oldLeft > 0 && newLeft > 0) {
        row.kind = "context"; row.before = before++; row.after = after++; oldLeft--; newLeft--;
      } else if (text.startsWith("-") && oldLeft > 0) {
        row.kind = "deleted"; row.before = before++; oldLeft--;
      } else if (text.startsWith("+") && newLeft > 0) {
        row.kind = "added"; row.after = after++; newLeft--;
      } else { cursor.valid = false; return row; }
    }
    Object.assign(cursor, { before, after, oldLeft, newLeft });
    return row;
}

/** Presentation only. Index page boundaries, not every line or row object. */
export function diffPages(text: string): { rowCount: number; pageCount: number; page(index: number): DiffRow[] } {
  const cursor: Cursor = { offset: 0, before: 0, after: 0, oldLeft: 0, newLeft: 0, valid: true };
  const boundaries: Cursor[] = [];
  let rowCount = 0;
  while (cursor.offset < text.length) {
    if (rowCount % DIFF_PAGE_ROWS === 0) boundaries.push({ ...cursor });
    nextRow(text, cursor);
    rowCount++;
  }
  const valid = cursor.valid && cursor.oldLeft === 0 && cursor.newLeft === 0;
  return {
    rowCount, pageCount: Math.max(1, boundaries.length),
    page(index) {
      if (!Number.isSafeInteger(index) || index < 0 || index >= boundaries.length) return [];
      const position = { ...boundaries[index]!, valid };
      const rows: DiffRow[] = [];
      while (position.offset < text.length && rows.length < DIFF_PAGE_ROWS) rows.push(nextRow(text, position));
      return rows;
    },
  };
}

/** Convenience for consumers needing all rows; the UI uses bounded diffPages. */
export function diffRows(text: string): DiffRow[] {
  const pages = diffPages(text);
  return Array.from({ length: pages.pageCount }, (_, index) => pages.page(index)).flat();
}
