import assert from "node:assert/strict";
import test from "node:test";
import { validateDiffSummaryV1 } from "../src/m5/contracts/diff-v1.ts";

const summary = (entry: Record<string, unknown>) => ({ projectId: "project_1", baseCommit: "a".repeat(40), entries: [entry] });

test("changed line counts come as a pair of safe integers on text entries only", () => {
  assert.equal(validateDiffSummaryV1(summary({ kind: "modified", path: "a.txt", binary: false, added: 3, removed: 1 })).entries[0]!.added, 3);
  assert.equal(validateDiffSummaryV1(summary({ kind: "added", path: "a.txt", binary: false })).entries[0]!.added, undefined);
  for (const bad of [
    { kind: "modified", path: "a.txt", binary: false, added: 3 },
    { kind: "modified", path: "a.txt", binary: false, added: -1, removed: 0 },
    { kind: "modified", path: "a.txt", binary: false, added: 1.5, removed: 0 },
    { kind: "modified", path: "a.txt", binary: false, added: "3", removed: 0 },
    { kind: "binary", path: "a.bin", binary: true, added: 0, removed: 0 },
    { kind: "modified", path: "a.txt", binary: false, added: 1, removed: 0, extra: true },
  ]) assert.throws(() => validateDiffSummaryV1(summary(bad)), JSON.stringify(bad));
});
