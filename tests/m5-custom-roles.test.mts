import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  G5_DEFAULT_ROLE_DEFINITIONS_V3, isRoleIdV3, roleSettingsAsV3, stageForProfileV3,
  validateChatRecordV1, validateProjectRoleSettingsUpdateRequestV1, validateProjectRoleSettingsV1,
  type RoleProfileV1,
} from "../src/m5/contracts/run-recovery-v1.ts";
import { validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";
import { roleLabel } from "../src/m5/role-names.ts";
import { addWorker, canRemoveWorker, isValidTeamDraft, nextWorkerId, removeWorker, sameTeam, teamDraftFrom, updateDefinition, updateProfile } from "../src/m5/team-draft.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

const profile = (roleId: string, permission: RoleProfileV1["permission"] = "readOnly"): RoleProfileV1 => ({ roleId, runtime: "codex", modelSelector: "sol", reasoningEffort: "medium", permission });
const legacyRoles = ["coordinator", "planning", "design", "coding", "review"].map(id => profile(id, id === "coding" ? "workspaceWrite" : "readOnly"));

test("default V3 definitions match the Rust seed exactly", () => {
  const rust = readFileSync(new URL("../crates/elsecrew-protocol/src/run_recovery_v1.rs", import.meta.url), "utf8");
  for (const { roleId, name, responsibility } of G5_DEFAULT_ROLE_DEFINITIONS_V3) {
    assert.ok(rust.includes(`("${roleId}", "${name}", "${responsibility}")`), roleId);
  }
  assert.deepEqual(G5_DEFAULT_ROLE_DEFINITIONS_V3.map(item => item.roleId), ["coordinator", "planning", "design", "coding", "review"]);
});

test("V3 role IDs, stage and legacy permissions are permission-based but legacy-exact", () => {
  for (const id of ["qa", "ui-design", "coordinator", "a1"]) assert.ok(isRoleIdV3(id), id);
  for (const id of ["", "a", "1a", "Qa", "q a", "q_a", "한글", `a${"b".repeat(32)}`]) assert.ok(!isRoleIdV3(id), id);
  assert.equal(stageForProfileV3({ roleId: "planning", permission: "workspaceWrite" }), "implementing");
  assert.equal(stageForProfileV3({ roleId: "review", permission: "readOnly" }), "verifying");
  assert.equal(stageForProfileV3({ roleId: "qa", permission: "readOnly" }), "planning");
  const legacy = { schemaVersion: 2, projectId: "p", revision: 1, roles: legacyRoles, updatedAt: "2026-11-12T00:00:00Z" };
  validateProjectRoleSettingsV1(legacy);
  assert.throws(() => validateProjectRoleSettingsV1({ ...legacy, roles: legacyRoles.map(role => role.roleId === "planning" ? { ...role, permission: "workspaceWrite" } : role) }));
});

test("V3 settings, update requests and chat records require parallel bounded definitions", () => {
  const { roles, roleDefinitions } = roleSettingsAsV3({ roles: legacyRoles });
  const settings = { schemaVersion: 3, projectId: "p", revision: 2, roles, roleDefinitions, updatedAt: "2026-11-12T00:00:00Z" };
  validateProjectRoleSettingsV1(settings);
  assert.throws(() => validateProjectRoleSettingsV1({ ...settings, roleDefinitions: undefined }));
  assert.throws(() => validateProjectRoleSettingsV1({ ...settings, roleDefinitions: [...roleDefinitions].reverse() }));
  assert.throws(() => validateProjectRoleSettingsV1({ ...settings, roles: [profile("coordinator", "workspaceWrite"), ...roles.slice(1)] }));
  assert.throws(() => validateProjectRoleSettingsV1({ ...settings, roles: roles.slice(0, 1), roleDefinitions: roleDefinitions.slice(0, 1) }));
  assert.throws(() => validateProjectRoleSettingsV1({ ...settings, roleDefinitions: roleDefinitions.map((item, index) => index === 1 ? { ...item, name: "n".repeat(65) } : item) }));
  assert.throws(() => validateProjectRoleSettingsV1({ ...settings, roleDefinitions: roleDefinitions.map((item, index) => index === 1 ? { ...item, responsibility: "r".repeat(2001) } : item) }));
  assert.throws(() => validateProjectRoleSettingsV1({ ...settings, roleDefinitions: roleDefinitions.map((item, index) => index === 1 ? { ...item, name: "a\u0007b" } : item) }));
  validateProjectRoleSettingsV1({ ...settings, roleDefinitions: roleDefinitions.map((item, index) => index === 1 ? { ...item, responsibility: "line one\nline two\tok" } : item) });
  const nine = [profile("coordinator"), ...Array.from({ length: 8 }, (_, index) => profile(`w${index}`))];
  assert.throws(() => validateProjectRoleSettingsV1({ ...settings, roles: nine, roleDefinitions: nine.map(role => ({ roleId: role.roleId, name: "W", responsibility: "" })) }));
  validateProjectRoleSettingsUpdateRequestV1({ projectId: "p", expectedRevision: 2, roles, roleDefinitions });
  assert.throws(() => validateProjectRoleSettingsUpdateRequestV1({ projectId: "p", expectedRevision: 2, roles: [profile("coordinator"), profile("qa")] }));
  const chat = { schemaVersion: 3, chatId: "c", taskId: "t", projectId: "p", worktreeId: "w", workspaceIdentity: "i", goal: "g", settingsRevision: 2, roleSnapshots: roles, roleDefinitions, state: "draft", createdAt: "2026-11-12T00:00:00Z", updatedAt: "2026-11-12T00:00:00Z" };
  validateChatRecordV1(chat);
  assert.throws(() => validateChatRecordV1({ ...chat, schemaVersion: 2 }));
});

test("app defaults catalog requests carry V3 definitions and require an exact echo", () => {
  const { roles, roleDefinitions } = roleSettingsAsV3({ roles: legacyRoles });
  const request = { action: "updateAppRoleDefaults", expectedRevision: 1, roles, roleDefinitions } as const;
  assert.deepEqual(validateProjectCatalogRequest(request), request);
  const result = { schemaVersion: 1, action: "updateAppRoleDefaults", settings: { schemaVersion: 3, revision: 2, roles, roleDefinitions, updatedAt: "2026-11-12T00:00:00Z" } };
  assert.deepEqual(validateProjectCatalogResult(result, request), result);
  const renamed = roleDefinitions.map((item, index) => index === 1 ? { ...item, name: "Other" } : item);
  assert.throws(() => validateProjectCatalogResult({ ...result, settings: { ...result.settings, roleDefinitions: renamed } }, request));
});

test("role labels localize untouched template names and show stored custom names", () => {
  const t = (key: string) => (en as Record<string, string>)[key];
  const defaults = roleSettingsAsV3({ roles: legacyRoles }).roleDefinitions;
  assert.equal(roleLabel("planning", defaults, t), "Planning");
  assert.equal(roleLabel("planning", undefined, t), "Planning");
  assert.equal(roleLabel("planning", [{ roleId: "planning", name: "Research", responsibility: "" }], t), "Research");
  assert.equal(roleLabel("qa", [{ roleId: "qa", name: "품질", responsibility: "" }], t), "품질");
  assert.equal(roleLabel("qa", undefined, t), "qa");
  for (const key of ["m5.roles.add", "m5.roles.remove", "m5.roles.name", "m5.roles.responsibility", "m5.roles.permissionWrite", "m5.roles.newRoleName", "m5.roles.invalidTeam"] as const) {
    assert.ok(ko[key] && en[key], key);
  }
});

test("team drafts add and remove workers but keep the coordinator and at least one worker", () => {
  let draft = teamDraftFrom({ roles: [legacyRoles[1], legacyRoles[3], legacyRoles[4]] });
  assert.deepEqual(draft.roles.map(role => role.roleId), ["coordinator", "planning", "design", "coding", "review"], "V1 input normalizes to the default coordinator team");
  assert.ok(isValidTeamDraft(draft));
  draft = teamDraftFrom({ roles: [profile("coordinator"), profile("planning")], roleDefinitions: [{ roleId: "coordinator", name: "총괄", responsibility: "" }, { roleId: "planning", name: "기획", responsibility: "" }] });
  assert.ok(!canRemoveWorker(draft, "planning"));
  assert.ok(!canRemoveWorker(draft, "coordinator"));
  assert.equal(nextWorkerId(draft), "member-1");
  const added = addWorker(draft, "New role 2");
  assert.equal(added.roles[2].roleId, "member-1");
  assert.equal(added.roles[2].permission, "readOnly");
  assert.ok(isValidTeamDraft(added));
  assert.ok(!sameTeam(added, draft));
  const writer = updateProfile(added, "member-1", { permission: "workspaceWrite" });
  assert.equal(writer.roles[2].permission, "workspaceWrite");
  assert.equal(updateProfile(writer, "coordinator", { permission: "workspaceWrite" }).roles[0].permission, "readOnly");
  assert.ok(!isValidTeamDraft(updateDefinition(writer, "member-1", { name: " " })));
  assert.ok(sameTeam(removeWorker(added, "member-1"), draft));
  let full = draft;
  for (let index = 0; index < 10; index += 1) full = addWorker(full, `R${index}`);
  assert.equal(full.roles.length, 8);
  assert.ok(isValidTeamDraft(full));
});
