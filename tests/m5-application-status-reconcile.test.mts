import assert from "node:assert/strict";
import test from "node:test";
import { reconcileApplicationStatus } from "../src/m5/application-status-reconcile.ts";

const oldId = `application_${"a".repeat(32)}`;
const newId = `application_${"b".repeat(32)}`;
const query = { action: "chatApplicationStatus", projectId: "project_a", chatId: "chat_a" } as const;
function result(applicationId: string | null, running = false) {
  return { schemaVersion: 1, action: query.action, chatId: query.chatId,
    status: { projectId: query.projectId, applicationId, state: applicationId ? (running ? "applying" : "applied") : "none", currentIndex: null, total: applicationId ? 1 : 0, running, failure: null } };
}

test("lost start response adopts only a new record scoped to the submitted chat", () => {
  for (const baseline of [null, oldId]) {
    for (const running of [true, false]) {
      assert.equal(reconcileApplicationStatus(result(newId, running), query, null, baseline).applicationId, newId);
    }
  }
  assert.throws(() => reconcileApplicationStatus(result(oldId), query, null, oldId));
  assert.throws(() => reconcileApplicationStatus(result(null), query, null, null));
  assert.throws(() => reconcileApplicationStatus(result(newId), query, null, undefined));
  assert.throws(() => reconcileApplicationStatus({ ...result(newId), chatId: "chat_b" }, query, null, null));
  const crossed = result(newId); crossed.status.projectId = "project_b";
  assert.throws(() => reconcileApplicationStatus(crossed, query, null, null));
});

test("known application identities stay exact and project-only reads cannot adopt unknown runs", () => {
  assert.equal(reconcileApplicationStatus(result(newId), query, newId, oldId).applicationId, newId);
  assert.throws(() => reconcileApplicationStatus(result(oldId), query, newId, oldId));
  const projectQuery = {action: "applicationStatus", projectId: query.projectId} as const;
  const projectResult = {schemaVersion: 1, action: projectQuery.action, status: result(newId).status};
  assert.equal(reconcileApplicationStatus(projectResult, projectQuery, newId, undefined).applicationId, newId);
  assert.throws(() => reconcileApplicationStatus(projectResult, projectQuery, null, null));
});
