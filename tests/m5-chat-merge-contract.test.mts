import assert from "node:assert/strict";
import test from "node:test";
import { validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";

const address = { chatId: "chat_1", projectId: "project_1", worktreeId: "worktree_1", workspaceIdentity: "workspace_1" };
const head = "a".repeat(40), base = "b".repeat(40);
const state = { kind: "gitWorktree", branch: "elsecrew/task-1", baseRef: "main", head, baseTip: base, ahead: 2, behind: 0, uncommitted: false, baseCheckedOut: true, blocked: null };

test("merge requests carry the reviewed commits and an explicit confirmation", () => {
  validateProjectCatalogRequest({ action: "inspectChatMerge", ...address });
  validateProjectCatalogRequest({ action: "mergeChatWorkspace", ...address, expectedHead: head, expectedBase: base, confirmed: true });
  for (const bad of [
    { action: "mergeChatWorkspace", ...address, expectedHead: head, expectedBase: base },
    { action: "mergeChatWorkspace", ...address, expectedHead: "HEAD", expectedBase: base, confirmed: true },
    { action: "inspectChatMerge", ...address, extra: 1 },
  ]) assert.throws(() => validateProjectCatalogRequest(bad as never));
});

test("merge results are a known outcome over a worktree state", () => {
  const inspect = { action: "inspectChatMerge" as const, ...address };
  validateProjectCatalogResult({ schemaVersion: 1, action: "inspectChatMerge", merge: state }, inspect);
  validateProjectCatalogResult({ schemaVersion: 1, action: "inspectChatMerge", merge: { kind: "snapshot" } }, inspect);
  assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, action: "inspectChatMerge", merge: { ...state, branch: "main" } }, inspect));
  const merge = { action: "mergeChatWorkspace" as const, ...address, expectedHead: head, expectedBase: base, confirmed: true as const };
  validateProjectCatalogResult({ schemaVersion: 1, action: "mergeChatWorkspace", outcome: "merged", merge: { ...state, ahead: 0, blocked: "nothingToMerge" } }, merge);
  validateProjectCatalogResult({ schemaVersion: 1, action: "mergeChatWorkspace", outcome: "conflict", conflictCount: 1, conflicts: ["app.js"], merge: state }, merge);
  validateProjectCatalogResult({ schemaVersion: 1, action: "mergeChatWorkspace", outcome: "blocked", reason: "userChangesOverlap", merge: state }, merge);
  for (const bad of [
    { schemaVersion: 1, action: "mergeChatWorkspace", outcome: "done", merge: state },
    { schemaVersion: 1, action: "mergeChatWorkspace", outcome: "conflict", conflictCount: 0, conflicts: ["a"], merge: state },
    { schemaVersion: 1, action: "mergeChatWorkspace", outcome: "merged", merge: { kind: "snapshot" } },
  ]) assert.throws(() => validateProjectCatalogResult(bad, merge));
});
