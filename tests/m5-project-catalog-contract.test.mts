import assert from "node:assert/strict";
import test from "node:test";
import { validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";

test("chat application status rejects another chat or project response", () => {
  const request = {action:"chatApplicationStatus" as const,projectId:"project_a",chatId:"chat_a"};
  const status = {projectId:"project_a",applicationId:null,state:"none",currentIndex:null,total:0,running:false,failure:null};
  const result = {schemaVersion:1,action:request.action,chatId:request.chatId,status};
  assert.doesNotThrow(() => validateProjectCatalogRequest(request));
  assert.doesNotThrow(() => validateProjectCatalogResult(result,request));
  assert.throws(() => validateProjectCatalogRequest({...request,chatId:undefined}));
  assert.throws(() => validateProjectCatalogResult({...result,chatId:"chat_other"},request));
  assert.throws(() => validateProjectCatalogResult({...result,status:{...status,projectId:"project_other"}},request));
});

test("backup preparation recovery binds project, record and exact digest", () => {
  const entry = {reviewId:`review_${"a".repeat(32)}`,fingerprint:"b".repeat(64)};
  const request = {action:"backupPreparations" as const,projectId:"project_a"};
  const result = {schemaVersion:1,action:request.action,projectId:request.projectId,entries:[entry]};
  assert.doesNotThrow(() => validateProjectCatalogResult(result,request));
  assert.throws(() => validateProjectCatalogResult({...result,entries:[entry,entry]},request));
  assert.throws(() => validateProjectCatalogResult({...result,projectId:"other"},request));
  const abandon = {action:"abandonBackupPreparation" as const,projectId:request.projectId,...entry};
  assert.doesNotThrow(() => validateProjectCatalogRequest(abandon));
  assert.throws(() => validateProjectCatalogRequest({...abandon,fingerprint:null}));
  assert.throws(() => validateProjectCatalogResult({schemaVersion:1,action:abandon.action,projectId:abandon.projectId,reviewId:`review_${"c".repeat(32)}`},abandon));
});

test("application start commands preserve target and reject missing confirmation", () => {
  const request = {action:"startApplication" as const,projectId:"project_a",chatId:"chat_a",confirmationId:`confirmation_${"a".repeat(32)}`};
  const result = {schemaVersion:1,action:request.action,projectId:request.projectId,chatId:request.chatId,applicationId:`application_${"b".repeat(32)}`};
  assert.doesNotThrow(() => validateProjectCatalogResult(result,request));
  assert.throws(() => validateProjectCatalogRequest({...request,confirmationId:""}));
  assert.throws(() => validateProjectCatalogResult({...result,chatId:"chat_other"},request));
  const restore = {action:"restoreApplication" as const,projectId:request.projectId,applicationId:result.applicationId,fingerprint:"c".repeat(64)};
  assert.doesNotThrow(() => validateProjectCatalogResult({schemaVersion:1,action:restore.action,projectId:restore.projectId,applicationId:restore.applicationId},restore));
  assert.throws(() => validateProjectCatalogRequest({...restore,fingerprint:null}));
  assert.throws(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationCapabilities",enabled:"true"},{action:"applicationCapabilities"}));
});

test("restoration pages bind the exact current application and record fingerprint", () => {
  const request = {action:"restorationPreview" as const,projectId:"project_a",applicationId:`application_${"a".repeat(32)}`,offset:0,fingerprint:null};
  const result = {schemaVersion:1,action:request.action,projectId:request.projectId,applicationId:request.applicationId,state:"applying",page:{worktreeId:"worktree_a",total:1,offset:0,fingerprint:"b".repeat(64),changes:[{path:"src/file.ts",kind:"modified"}]}};
  assert.doesNotThrow(() => validateProjectCatalogResult(result, request));
  for (const patch of [{applicationId:`application_${"c".repeat(32)}`},{state:"restored"},{projectId:"other"}]) {
    assert.throws(() => validateProjectCatalogResult({...result,...patch},request));
  }
  assert.throws(() => validateProjectCatalogResult(result,{...request,fingerprint:"c".repeat(64)}));
  assert.throws(() => validateProjectCatalogRequest({...request,offset:100}));
});

test("application status is read-only, bounded, and tied to the selected project", () => {
  const request = {action:"applicationStatus" as const,projectId:"project_a"};
  const status = {projectId:"project_a",applicationId:"application_0123456789abcdef0123456789abcdef",state:"applying",currentIndex:1,total:2,running:true,failure:null};
  assert.doesNotThrow(() => validateProjectCatalogRequest(request));
  assert.doesNotThrow(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status},request));
  for (const change of [{projectId:"other"},{currentIndex:2},{total:100001},{state:"unknown"},{applicationId:null},{path:"/source"},{running:"true"},{running:true,state:"restored"}]) {
    assert.throws(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status:{...status,...change}},request));
  }
  const failure = {code:"E_RECOVERY_REQUIRED",detail:"preserved"};
  assert.doesNotThrow(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status:{...status,running:false,failure}},request));
  assert.throws(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status:{...status,failure}},request));
  assert.throws(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status:{...status,running:false,failure:{...failure,detail:"x".repeat(2049)}}},request));
  assert.doesNotThrow(() => validateProjectCatalogResult({schemaVersion:1,action:"applicationStatus",status:{projectId:"project_a",applicationId:null,state:"none",currentIndex:null,total:0,running:false,failure:null}},request));
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

test("confirmation is bound to target, fixed lifetime and exact cancel token", () => {
  const request = { action: "openApplicationConfirmation" as const, projectId: "project_a", chatId: "chat_a", fingerprint: "a".repeat(64) };
  const confirmationId = `confirmation_${"b".repeat(32)}`;
  const result = { schemaVersion: 1, action: request.action, projectId: request.projectId, chatId: request.chatId, confirmationId, worktreeId: "worktree_a", total: 3, expiresInSeconds: 300 };
  assert.doesNotThrow(() => validateProjectCatalogResult(result, request));
  for (const change of [{ chatId: "chat_b" }, { expiresInSeconds: 301 }, { confirmationId: "forged" }]) assert.throws(() => validateProjectCatalogResult({ ...result, ...change }, request));
  assert.throws(() => validateProjectCatalogResult({ schemaVersion:1, action:"cancelApplicationConfirmation", confirmationId:"other" }, {action:"cancelApplicationConfirmation",confirmationId}));
});

test("preview pages bind the cursor, target and change fingerprint", () => {
  const fingerprint = "a".repeat(64);
  const request = { action: "applicationPreviewPage" as const, projectId: "project_a", chatId: "chat_a", offset: 100, fingerprint };
  const result = { schemaVersion: 1, action: request.action, projectId: request.projectId, chatId: request.chatId, page: { worktreeId: "worktree_a", offset: 100, fingerprint, total: 101, changes: [{path:"last.txt",kind:"added"}] } };
  assert.doesNotThrow(() => validateProjectCatalogResult(result, request));
  for (const change of [{ offset: 0 }, { fingerprint: "b".repeat(64) }, { total: 102 }, { changes: [] }]) assert.throws(() => validateProjectCatalogResult({ ...result, page: { ...result.page, ...change } }, request));
  assert.throws(() => validateProjectCatalogRequest({ ...request, fingerprint: null }));
});

test("chat preview cannot return another chat's result", () => {
  const request = { action: "chatApplicationPreview" as const, projectId: "project_a", chatId: "chat_a" };
  const result = { schemaVersion: 1, ...request, preview: { worktreeId: "worktree_a", total: 0, changes: [] } };
  assert.doesNotThrow(() => validateProjectCatalogResult(result, request));
  assert.throws(() => validateProjectCatalogResult({ ...result, chatId: "chat_b" }, request));
  assert.throws(() => validateProjectCatalogResult({ ...result, projectId: "project_b" }, request));
  assert.throws(() => validateProjectCatalogRequest({ ...request, worktreePath: "/private" }));
});

test("application workspace preparation binds explicit project without paths", () => {
  const request = { action: "prepareApplicationWorkspace" as const, projectId: "project_a" };
  assert.doesNotThrow(() => validateProjectCatalogResult({ schemaVersion: 1, ...request, worktreeId: "worktree_a" }, request));
  assert.throws(() => validateProjectCatalogResult({ schemaVersion: 1, ...request, projectId: "other", worktreeId: "worktree_a" }, request));
  assert.throws(() => validateProjectCatalogRequest({ ...request, path: "/tmp/project" }));
});

test("application preview binds project, relative paths and explicit truncation", () => {
  const request = { action: "applicationPreview" as const, projectId: "project_a" };
  const preview = { worktreeId: "worktree_a", total: 1, changes: [{ path: "src/file.ts", kind: "modified" }] };
  const result = { schemaVersion: 1, ...request, preview };
  assert.doesNotThrow(() => validateProjectCatalogResult(result, request));
  assert.throws(() => validateProjectCatalogResult({ ...result, projectId: "other" }, request));
  for (const change of [
    { total: 2 }, { total: -1 }, { changes: [{ path: "../secret", kind: "added" }] },
    { changes: [{ path: "/absolute", kind: "added" }] },
    { changes: [{ path: "src/file.ts", kind: "execute" }] },
    { changes: [{ path: "src/file.ts", kind: "added", content: "private" }] },
  ]) assert.throws(() => validateProjectCatalogResult({ ...result, preview: { ...preview, ...change } }, request));
  assert.doesNotThrow(() => validateProjectCatalogResult({ ...result, preview: { ...preview, total: 101, changes: Array.from({ length: 100 }, (_, index) => ({path: `file-${index}`, kind: "added"})) } }, request));
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
