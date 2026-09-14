import assert from "node:assert/strict";
import test from "node:test";
import { diffRows, DIFF_PAGE_ROWS } from "../src/m5/diff-view.ts";

test("unified diff distinguishes file headers, context, changes and multiple hunks", () => {
  const rows = diffRows("--- a/a.ts\n+++ b/a.ts\n@@ -2,2 +2,2 @@ function\n same\n-old\n+new\n@@ -10 +20 @@\n-x\n+y\n");
  assert.deepEqual(rows.map(row => [row.kind, row.before, row.after]), [
    ["metadata", null, null], ["metadata", null, null], ["metadata", null, null],
    ["context", 2, 2], ["deleted", 3, null], ["added", null, 3],
    ["metadata", null, null], ["deleted", 10, null], ["added", null, 20],
  ]);
});

test("zero length ranges, no-newline markers and literal code remain intact", () => {
  const text = "@@ -0,0 +1,2 @@\n+<script>alert(1)</script>\n+한글\n\\ No newline at end of file";
  const rows = diffRows(text);
  assert.equal(rows[1]?.after, 1);
  assert.equal(rows[2]?.after, 2);
  assert.equal(rows[3]?.kind, "metadata");
  assert.equal(rows.map(row => row.text).join("\n"), text);
  assert.equal(diffRows("@@ -1,1 +0,0 @@\n-old")[1]?.before, 1);
});

test("incomplete or unsupported hunks fall back to unchanged unnumbered text", () => {
  for (const text of ["@@ -1,2 +1 @@\n-old\n+new", "@@@ -1 -1 +1 @@@\n++merge", "@@ -9007199254740991,2 +1 @@\n-x\n-y\n+z", "@@ -1 +1 @@\n?unknown"]) {
    const rows = diffRows(text);
    assert.ok(rows.every(row => row.kind === "metadata" && row.before === null && row.after === null));
    assert.equal(rows.map(row => row.text).join("\n"), text);
  }
});

test("large changes retain all lines and numbering across bounded page boundaries", () => {
  const lines = Array.from({ length: 1000 }, (_, index) => `+line ${index + 1}`);
  const rows = diffRows(`@@ -0,0 +1,1000 @@\n${lines.join("\n")}\n`);
  assert.equal(rows.length, 1001);
  const pages = Array.from({ length: Math.ceil(rows.length / DIFF_PAGE_ROWS) }, (_, page) => rows.slice(page * DIFF_PAGE_ROWS, (page + 1) * DIFF_PAGE_ROWS));
  assert.ok(pages.every(page => page.length <= 200));
  assert.deepEqual(pages.flat(), rows);
  assert.equal(rows.at(-1)?.after, 1000);
});
