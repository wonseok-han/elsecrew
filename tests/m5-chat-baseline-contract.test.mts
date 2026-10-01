import test from "node:test";
import assert from "node:assert/strict";
import { validatePrepareChatWorkspaceRequestV1 } from "../src/m5/contracts/run-recovery-v1.ts";

test("chat baseline choice preserves legacy requests and rejects coercion", () => {
  for (const request of [{ projectId: "project_a" }, { projectId: "project_a", applicationBaseline: true }, { projectId: "project_a", applicationBaseline: false }]) {
    assert.deepEqual(validatePrepareChatWorkspaceRequestV1(request), request);
  }
  for (const request of [{ projectId: "project_a", applicationBaseline: null }, { projectId: "project_a", applicationBaseline: "true" }, { projectId: "project_a", applicationBaseline: true, path: "/tmp" }]) {
    assert.throws(() => validatePrepareChatWorkspaceRequestV1(request));
  }
});
