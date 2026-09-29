import assert from "node:assert/strict";
import test from "node:test";
import { FILE_SEARCH_LIMITS, searchFileNames } from "../src/m5/file-search.ts";
import type { TreeEntryV1 } from "../src/m5/contracts/project-v1.ts";

test("search traverses paginated subfolders, matches paths case-insensitively and skips symlinks", async () => {
  const requests: string[] = [];
  const result = await searchFileNames(async (parentPath, cursor) => {
    requests.push(`${parentPath}:${cursor}`);
    const entries: TreeEntryV1[] = parentPath === "src" ? [{ name: "한글.TS", path: "src/한글.TS", kind: "file" }]
      : cursor ? [{ name: "ignored", path: "ignored", kind: "symlink" }]
      : [{ name: "src", path: "src", kind: "directory" }];
    return { parentPath, entries, nextCursor: parentPath || cursor ? null : "next" };
  }, "SRC/한글", new AbortController().signal);
  assert.deepEqual(result.matches.map(entry => entry.path), ["src/한글.TS"]);
  assert.deepEqual(requests, [":null", ":next", "src:null"]);
  assert.equal(result.limited, false);
});

test("cancel during a page read discards late matches and does not visit another directory", async () => {
  const request = new AbortController();
  let calls = 0;
  await assert.rejects(searchFileNames(async parentPath => {
    calls++; request.abort();
    return { parentPath, entries: [{ name: "a", path: "a", kind: "file" }], nextCursor: "next" };
  }, "a", request.signal), { name: "AbortError" });
  assert.equal(calls, 1);
});

test("search bounds matches and page reads, reporting incomplete coverage", async () => {
  const matches = await searchFileNames(async parentPath => ({ parentPath, entries: Array.from({ length: 500 }, (_, n) => ({ name: `file${n}`, path: `file${n}`, kind: "file" as const })), nextCursor: null }), "file", new AbortController().signal);
  assert.equal(matches.matches.length, FILE_SEARCH_LIMITS.matches);
  assert.equal(matches.limited, true);
  let calls = 0;
  const pages = await searchFileNames(async parentPath => ({ parentPath, entries: [], nextCursor: String(++calls) }), "missing", new AbortController().signal);
  assert.equal(calls, FILE_SEARCH_LIMITS.pages);
  assert.equal(pages.limited, true);
});

test("malformed child paths and repeated cursors fail rather than escape or loop", async () => {
  await assert.rejects(searchFileNames(async parentPath => ({ parentPath, entries: [{ name: "outside", path: "../outside", kind: "directory" }], nextCursor: null }), "file", new AbortController().signal), /path mismatch/);
  await assert.rejects(searchFileNames(async parentPath => ({ parentPath, entries: [], nextCursor: "repeat" }), "file", new AbortController().signal), /cursor repeated/);
});

test("directory and elapsed budgets stop scheduling more pages", async t => {
  let calls = 0;
  const directories = await searchFileNames(async parentPath => {
    const offset = calls++ * 500;
    return { parentPath, entries: Array.from({ length: 500 }, (_, n) => ({ name: `folder${offset+n}`, path: `folder${offset+n}`, kind: "directory" as const })), nextCursor: "more" };
  }, "missing", new AbortController().signal);
  assert.equal(directories.limited, true);
  assert.equal(calls, 2);
  let now = 0;
  t.mock.method(Date, "now", () => now);
  calls = 0;
  const elapsed = await searchFileNames(async parentPath => {
    calls++; now = FILE_SEARCH_LIMITS.milliseconds;
    return { parentPath, entries: [], nextCursor: "more" };
  }, "missing", new AbortController().signal);
  assert.equal(elapsed.limited, true);
  assert.equal(calls, 1);
});
