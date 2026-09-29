import assert from "node:assert/strict";
import test from "node:test";
import { validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";

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
