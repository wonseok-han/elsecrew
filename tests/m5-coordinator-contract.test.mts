import assert from "node:assert/strict";
import test from "node:test";
import { coordinatorRoles, validateProjectRoleSettingsV1, validateChatRecordV1, validateRoleProfileV1, type RoleProfileV1 } from "../src/m5/contracts/run-recovery-v1.ts";
const roles: RoleProfileV1[] = [
  {roleId:"planning",runtime:"codex",modelSelector:"selected-model",reasoningEffort:"medium",permission:"readOnly"},
  {roleId:"coding",runtime:"claude-code",modelSelector:"coding-model",reasoningEffort:null,permission:"workspaceWrite"},
  {roleId:"review",runtime:"codex",modelSelector:"review-model",reasoningEffort:null,permission:"readOnly"},
];
test("versioned coordinator settings preserve V1 exact three, seed selected model, reject write and mixed schemas", () => {
  const old = {schemaVersion:1,projectId:"p",revision:1,roles,updatedAt:"2026-11-10T00:00:00Z"};
  const seeded = coordinatorRoles(roles);
  assert.doesNotThrow(() => validateProjectRoleSettingsV1(old));
  assert.doesNotThrow(() => validateProjectRoleSettingsV1({...old,schemaVersion:2,roles:seeded}));
  assert.throws(() => validateProjectRoleSettingsV1({...old,roles:seeded}));
  assert.throws(() => validateProjectRoleSettingsV1({...old,schemaVersion:2}));
  assert.equal(seeded[0].modelSelector, roles[0].modelSelector);
  assert.equal(seeded[0].reasoningEffort, roles[0].reasoningEffort);
  assert.deepEqual(seeded.filter(role => !["coordinator","design"].includes(role.roleId)), roles);
  assert.throws(() => validateRoleProfileV1({...seeded[0],permission:"workspaceWrite"}));
  const chat = {schemaVersion:2,chatId:"c",taskId:"t",projectId:"p",worktreeId:"w",workspaceIdentity:"i",goal:"explain",settingsRevision:2,roleSnapshots:seeded,state:"draft",createdAt:old.updatedAt,updatedAt:old.updatedAt};
  assert.doesNotThrow(() => validateChatRecordV1(chat));
  assert.throws(() => validateChatRecordV1({...chat,schemaVersion:1}));
});

test("coordinator is not a legacy fixture role", async () => {
  const { validateFixtureRunStartRequestV1 } = await import("../src/m5/contracts/run-recovery-v1.ts");
  assert.throws(() => validateFixtureRunStartRequestV1({chatId:"c",roleId:"coordinator",scenario:"planning-success"}));
});
