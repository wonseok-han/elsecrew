import assert from "node:assert/strict";
import test from "node:test";
import { validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";

test("application status is read-only, bounded, and tied to the selected project", () => {
  const request = {action:"applicationStatus" as const,projectId:"project_a"};
  const status = {projectId:"project_a",applicationId:"application_0123456789abcdef0123456789abcdef",state:"applying",currentIndex:1,total:2};
  assert.doesNotThrow(() => validateProjectCatalogRequest(request));
  assert.doesNotThrow(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status},request));
  for (const change of [{projectId:"other"},{currentIndex:2},{total:100001},{state:"unknown"},{applicationId:null},{path:"/source"}]) {
    assert.throws(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status:{...status,...change}},request));
  }
  assert.doesNotThrow(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status:{projectId:"project_a",applicationId:null,state:"none",currentIndex:null,total:0}},request));
});

test("preparation recovery binds exact identity and safe revisions", () => {
  const inspection = { projectId: "project_a", worktreeId: "worktree_a", workspaceIdentity: "workspace_a", lockId: "lock_worktree_a", projectRevision: 8, lockRevision: 9 };
  const request = { action: "abandonPreparation" as const, inspection };
  assert.doesNotThrow(() => validateProjectCatalogRequest(request));
  assert.doesNotThrow(() => validateProjectCatalogResult({ schemaVersion: 1, ...request }, request));
  assert.doesNotThrow(() => validateProjectCatalogResult({ schemaVersion: 1, action: "inspectPreparation", inspection }, { action: "inspectPreparation", projectId: "project_a" }));
  for (const patch of [{ lockId: "other" }, { projectRevision: 0 }, { lockRevision: 1.5 }, { lockRevision: Number.MAX_SAFE_INTEGER + 1 }, { path: "/other" }, { workspaceIdentity: "../escape" }]) {
    assert.throws(() => validateProjectCatalogRequest({ ...request, inspection: { ...inspection, ...patch } }));
  }
  assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, ...request, inspection: { ...inspection, projectRevision: 10 } }, request));
  assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, action: "inspectPreparation", inspection }, { action: "inspectPreparation", projectId: "other" }));
});

test("application history is bounded, project-bound and terminal-only", () => {
  const request = { action: "applicationHistory" as const, projectId: "project_a" };
  const entry = { applicationId: `application_${"a".repeat(32)}`, state: "restored", total: 2 };
  const result = { schemaVersion: 1, ...request, entries: [entry] };
  assert.doesNotThrow(() => validateProjectCatalogResult(result, request));
  assert.doesNotThrow(() => validateProjectCatalogResult({ ...result, entries: [] }, request));
  for (const change of [
    { projectId: "project_b" }, { entries: [entry, entry] },
    { entries: [{ ...entry, state: "applying" }] },
    { entries: [{ ...entry, total: -1 }] },
    { entries: [{ ...entry, backupPath: "/private" }] },
    { entries: Array(1001).fill(entry) },
  ]) assert.throws(() => validateProjectCatalogResult({ ...result, ...change }, request));
  assert.throws(() => validateProjectCatalogRequest({ ...request, path: "/private" }));
});

test("view release is ID-only and binds the acknowledgment", () => {
  const input = { action: "releaseView" as const, projectId: "project_a" };
  assert.doesNotThrow(() => validateProjectCatalogRequest(input));
  assert.doesNotThrow(() => validateProjectCatalogResult({ schemaVersion: 1, ...input }, input));
  assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, action: "releaseView", projectId: "other" }, input));
  assert.throws(() => validateProjectCatalogRequest({ ...input, path: "/other" }));
});

test("catalog requests carry explicit stored IDs, never renderer filesystem paths", () => {
  assert.deepEqual(validateProjectCatalogRequest({ action: "list", cursor: null }), { action: "list", cursor: null });
  assert.deepEqual(validateProjectCatalogRequest({ action: "reopen", projectId: "project_a" }), { action: "reopen", projectId: "project_a" });
  for (const input of [{ action: "list" }, { action: "reopen", projectId: "project_a", path: "/other" }, { action: "reopen", projectId: "../a" }, { action: "open", projectId: "project_a" }]) assert.throws(() => validateProjectCatalogRequest(input));
});
test("catalog pages bind order, cursor, absolute paths and bounds", () => {
  const request = { action: "list" as const, cursor: "project_a" };
  const page = { schemaVersion: 1, action: "list", projects: [{ projectId: "project_b", canonicalPath: "/work/b" }], nextCursor: "project_b" };
  assert.doesNotThrow(() => validateProjectCatalogResult(page, request));
  for (const change of [
    { nextCursor: "project_c" },
    { projects: [] },
    { projects: [{ projectId: "project_a", canonicalPath: "/work/a" }] },
    { projects: [{ projectId: "project_b", canonicalPath: "relative" }] },
    { projects: [page.projects[0], page.projects[0]] },
    { projects: Array(51).fill(page.projects[0]) },
  ]) assert.throws(() => validateProjectCatalogResult({ ...page, ...change }, request));
});
test("reopen response must identify the selected project", () => {
  const project = { schemaVersion: 1, projectId: "project_a", selectedPath: "/work/a", canonicalGitRoot: "/work/a" };
  assert.doesNotThrow(() => validateProjectCatalogResult({ schemaVersion: 1, action: "reopen", project }, { action: "reopen", projectId: "project_a" }));
  assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, action: "reopen", project }, { action: "reopen", projectId: "project_b" }));
});
