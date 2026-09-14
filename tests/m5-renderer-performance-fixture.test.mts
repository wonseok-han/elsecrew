import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
const require = createRequire(import.meta.url);
const { fileText, patch, tree } = require("./m5-renderer-performance-fixture.cjs");

test("renderer performance fixture has exact UTF-8 file size, line count and aggregate diff size", () => {
  assert.equal(Buffer.byteLength(fileText), 5 * 1024 * 1024);
  assert.equal(fileText.split("\n").length - 1, 100000);
  assert.ok(fileText.includes("가"));
  let bytes = 0, changed = 0;
  for (let i = 0; i < 500; i++) {
    const value = patch(i); bytes += Buffer.byteLength(value);
    changed += value.split("\n").filter((line: string) => /^[+-]/.test(line) && !/^---|^\+\+\+/.test(line)).length;
  }
  assert.equal(bytes, 10 * 1024 * 1024); assert.equal(changed, 20000);
});

test("1000 folders and 10000 files are reachable across bounded tree pages", () => {
  const first = tree("", null), second = tree("", first.nextCursor);
  assert.equal(first.entries.length, 500); assert.equal(second.entries.length, 500);
  assert.equal(second.nextCursor, null);
  const paths = new Set<string>();
  for (const folder of [...first.entries, ...second.entries]) {
    const page = tree(folder.path); assert.equal(page.entries.length, 10);
    for (const entry of page.entries) paths.add(entry.path);
  }
  assert.equal(paths.size, 10000);
  assert.throws(() => tree("", "invalid"));
});
