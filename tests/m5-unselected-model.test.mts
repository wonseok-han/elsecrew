import assert from "node:assert/strict";
import test from "node:test";
import {
  G5_DEFAULT_ROLE_DEFINITIONS_V3,
  validateChatRecordV1,
  validateProjectRoleSettingsUpdateRequestV1,
  validateProjectRoleSettingsV1,
  validateRoleProfileV1,
  type RoleProfileV1,
} from "../src/m5/contracts/run-recovery-v1.ts";

// The built-in team names no model. Saved V3 settings may leave a model
// unselected; chats (and so runs) only snapshot complete teams.
const unselected = (roleId: string, permission: RoleProfileV1["permission"] = "readOnly"): RoleProfileV1 =>
  ({ roleId, runtime: "codex", modelSelector: "", reasoningEffort: null, permission });
const roles: RoleProfileV1[] = [unselected("coordinator"), unselected("planning"), unselected("design"), unselected("coding", "workspaceWrite"), unselected("review")];
const definitions = G5_DEFAULT_ROLE_DEFINITIONS_V3.map(definition => ({ ...definition }));
const at = "2026-11-13T00:00:00Z";

test("V3 role settings and updates accept an unselected model without a reasoning effort", () => {
  assert.doesNotThrow(() => validateProjectRoleSettingsV1({ schemaVersion: 3, projectId: "p", revision: 1, roles, roleDefinitions: definitions, updatedAt: at }));
  assert.doesNotThrow(() => validateProjectRoleSettingsUpdateRequestV1({ projectId: "p", expectedRevision: 1, roles, roleDefinitions: definitions }));
  const effort = roles.map((role, index) => index === 1 ? { ...role, reasoningEffort: "medium" } : role);
  assert.throws(() => validateProjectRoleSettingsV1({ schemaVersion: 3, projectId: "p", revision: 1, roles: effort, roleDefinitions: definitions, updatedAt: at }), /unselected model/);
  const blank = roles.map((role, index) => index === 1 ? { ...role, modelSelector: "  " } : role);
  assert.throws(() => validateProjectRoleSettingsV1({ schemaVersion: 3, projectId: "p", revision: 1, roles: blank, roleDefinitions: definitions, updatedAt: at }));
});

test("chats, single profiles and legacy settings still require a model", () => {
  assert.throws(() => validateRoleProfileV1(roles[1]));
  assert.throws(() => validateChatRecordV1({ schemaVersion: 3, chatId: "c", taskId: "t", projectId: "p", worktreeId: "w", workspaceIdentity: "i", goal: "g", settingsRevision: 1, roleSnapshots: roles, roleDefinitions: definitions, state: "draft", createdAt: at, updatedAt: at }));
  const legacy = [unselected("planning"), unselected("coding", "workspaceWrite"), unselected("review")];
  // Legacy validation maps over roles; an array index must never opt in.
  assert.throws(() => validateProjectRoleSettingsV1({ schemaVersion: 1, projectId: "p", revision: 1, roles: legacy, updatedAt: at }));
  assert.throws(() => validateProjectRoleSettingsUpdateRequestV1({ projectId: "p", expectedRevision: 1, roles: legacy }));
});
