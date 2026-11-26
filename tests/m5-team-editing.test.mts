import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  G5_DEFAULT_ROLE_DEFINITIONS_V3, G5_FORMAT_CHAR_RANGES_V3, G5_LEGACY_ROLE_LABELS_V3, G5_ROLE_ID_PATTERN_V3, G5_ROLE_NAME_MAX_BYTES_V3, G5_ROLE_RESPONSIBILITY_MAX_BYTES_V3,
  roleNameProblemsV3, roleSettingsAsV3, utf8ByteLength, validateChatRecordV1, validateProjectRoleSettingsUpdateRequestV1, validateProjectRoleSettingsV1,
  type RoleProfileV1,
} from "../src/m5/contracts/run-recovery-v1.ts";
import { validateProjectCatalogRequest } from "../src/m5/contracts/project-catalog-v1.ts";
import {
  addWorker, coordinatorDefaults, duplicateWorker, insertWorker, isValidTeamDraft, moveWorker, newMemberId, removedWorker, removeWorker, restoreWorker,
  sameTeam, teamChanges, teamDraftFrom, uniqueRoleName, updateDefinition, type TeamDraft,
} from "../src/m5/team-draft.ts";
import { insertTemplate, ROLE_TEMPLATE_KEYS, templateInTeam, templatePermission, templateText } from "../src/m5/role-templates.ts";
import { roleLabel } from "../src/m5/role-names.ts";
import { looksLikeSecret } from "../src/m5/secret-shape.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

const profile = (roleId: string, permission: RoleProfileV1["permission"] = "readOnly"): RoleProfileV1 => ({ roleId, runtime: "claude-code", modelSelector: "opus", reasoningEffort: null, permission });
const defaults = (): TeamDraft => roleSettingsAsV3({ roles: ["coordinator", "planning", "design", "coding", "review"].map(id => profile(id, id === "coordinator" ? "readOnly" : "workspaceWrite")) });
const ids = (draft: TeamDraft) => draft.roles.map(role => role.roleId);
const seed = (roleId: string, name: string) => ({ roleId, name, responsibility: "", permission: "readOnly" as const });

test("a member can be inserted anywhere below the coordinator, and only while there is room", () => {
  const team = defaults();
  assert.deepEqual(ids(insertWorker(team, 3, seed("member-aaaaaa", "QA"))), ["coordinator", "planning", "design", "member-aaaaaa", "coding", "review"]);
  assert.deepEqual(ids(insertWorker(team, 1, seed("member-aaaaaa", "QA"))).slice(0, 2), ["coordinator", "member-aaaaaa"], "right below the coordinator");
  assert.deepEqual(ids(insertWorker(team, 0, seed("member-aaaaaa", "QA"))).slice(0, 2), ["coordinator", "member-aaaaaa"], "never above it");
  assert.equal(ids(insertWorker(team, 99, seed("member-aaaaaa", "QA"))).at(-1), "member-aaaaaa", "past the end appends");
  const inserted = insertWorker(team, 2, seed("member-aaaaaa", "QA"));
  assert.deepEqual(inserted.roleDefinitions.map(item => item.roleId), ids(inserted), "definitions stay parallel");
  assert.equal(inserted.roles[2]!.runtime, team.roles[0]!.runtime, "agent and model come from the coordinator");
  assert.ok(isValidTeamDraft(inserted));
  assert.equal(insertWorker(team, 2, seed("planning", "Again")), team, "an ID in the team is not added twice");
  assert.equal(insertWorker(team, 2, seed("coordinator", "Second lead")), team);
  let full = team;
  for (let index = 0; index < 5; index++) full = insertWorker(full, 1, seed(`member-${index}aaaaa`, `R${index}`));
  assert.equal(full.roles.length, 8, "the coordinator and seven members (Q3)");
  assert.equal(insertWorker(full, 1, seed("member-zzzzzz", "Z")), full);
  assert.equal(addWorker(full, "Z"), full);
});

test("members move anywhere below the coordinator, which never moves", () => {
  const team = defaults();
  assert.deepEqual(ids(moveWorker(team, 3, 1)), ["coordinator", "coding", "planning", "design", "review"]);
  assert.deepEqual(ids(moveWorker(team, 1, 4)), ["coordinator", "design", "coding", "review", "planning"]);
  assert.deepEqual(ids(moveWorker(team, 2, 0)), ["coordinator", "design", "planning", "coding", "review"], "the top spot stays the coordinator's");
  assert.deepEqual(ids(moveWorker(team, 2, 99)).at(-1), "design");
  assert.equal(moveWorker(team, 0, 2), team, "the coordinator cannot be moved");
  assert.equal(moveWorker(team, 2, 2), team);
  assert.equal(moveWorker(team, 9, 1), team);
  const moved = moveWorker(team, 4, 1);
  assert.deepEqual(moved.roleDefinitions.map(item => item.roleId), ids(moved));
  assert.ok(isValidTeamDraft(moved));
});

test("a duplicate lands right below with the same settings, a new ID and its own name", () => {
  const team = updateDefinition(defaults(), "design", { responsibility: "화면 흐름" });
  const copy = duplicateWorker(team, "design", "디자인 사본", "member-copy01");
  assert.deepEqual(ids(copy), ["coordinator", "planning", "design", "member-copy01", "coding", "review"]);
  assert.deepEqual({ ...copy.roles[3], roleId: "design" }, team.roles[2]);
  assert.deepEqual(copy.roleDefinitions[3], { roleId: "member-copy01", name: "디자인 사본", responsibility: "화면 흐름" });
  assert.equal(duplicateWorker(team, "coordinator", "총괄 2", "member-copy02"), team, "the coordinator has no copy");
  assert.ok(isValidTeamDraft(copy));
});

test("a removal can be undone in place before saving; the last member stays", () => {
  const team = defaults();
  const entry = removedWorker(team, "design")!;
  const without = removeWorker(team, "design");
  assert.deepEqual(ids(without), ["coordinator", "planning", "coding", "review"]);
  assert.ok(sameTeam(restoreWorker(without, entry), team));
  let small = team;
  for (const id of ["planning", "design", "coding"]) small = removeWorker(small, id);
  assert.equal(removeWorker(small, "review"), small, "one member stays (Q5)");
  assert.equal(removedWorker(team, "coordinator"), null);
});

test("new member IDs are random, fit the pattern and never reuse one the team had", () => {
  const taken = new Set(["member-1", "planning"]);
  for (let index = 0; index < 10_000; index++) {
    const id = newMemberId(taken);
    assert.match(id, G5_ROLE_ID_PATTERN_V3);
    assert.match(id, /^member-[a-z0-9]{6}$/);
    assert.ok(!taken.has(id), id);
    taken.add(id);
  }
  // A collision draws again.
  const draws = [0, 0, 0, 0, 0, 0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
  assert.equal(newMemberId(["member-000000"], () => draws.shift()!), "member-iiiiii");
});

test("names made by the editor are unique, ignore spaces and case, and stay within 64 bytes", () => {
  const team = updateDefinition(defaults(), "review", { name: "Q A" });
  assert.equal(uniqueRoleName(team, "기획"), "기획 2");
  assert.equal(uniqueRoleName(team, "qa"), "qa 2");
  assert.equal(uniqueRoleName(team, "새 역할"), "새 역할");
  const long = uniqueRoleName(updateDefinition(team, "design", { name: "가".repeat(21) }), "가".repeat(21));
  assert.ok(utf8ByteLength(long) <= G5_ROLE_NAME_MAX_BYTES_V3, long);
  assert.ok(long.endsWith(" 2"));
});

test("templates: a removed starting role returns under its own ID; others get a new one", () => {
  const t = (key: string) => (ko as Record<string, string>)[key]!;
  const without = removeWorker(defaults(), "planning");
  assert.ok(!templateInTeam(without, "planning"));
  const back = insertTemplate(without, 1, "planning", "ko", t("m5.roles.planning"), ids(defaults()));
  assert.equal(back.roleId, "planning");
  assert.deepEqual(back.draft.roleDefinitions[1], G5_DEFAULT_ROLE_DEFINITIONS_V3[1], "the seed name keeps the translated label");
  assert.equal(roleLabel("planning", back.draft.roleDefinitions, key => (en as Record<string, string>)[key]!), "Planning");
  assert.equal(back.draft.roles[1]!.permission, "workspaceWrite", "starting roles keep their permission");
  // Already in the team: one more, under a new ID and a new name.
  const again = insertTemplate(back.draft, 2, "planning", "ko", t("m5.roles.planning"), ids(back.draft));
  assert.match(again.roleId, /^member-/);
  assert.equal(again.draft.roleDefinitions[2]!.name, "기획 2");
  assert.ok(isValidTeamDraft(again.draft));
  // The four added templates and their permissions (Q4, Q8).
  assert.deepEqual(ROLE_TEMPLATE_KEYS.map(key => [key, templatePermission(key)]), [
    ["planning", "workspaceWrite"], ["design", "workspaceWrite"], ["coding", "workspaceWrite"], ["review", "workspaceWrite"],
    ["tests", "workspaceWrite"], ["security", "readOnly"], ["docs", "workspaceWrite"], ["research", "readOnly"],
  ]);
  const research = insertTemplate(defaults(), 5, "research", "en", templateText("research", "en").name, []);
  assert.equal(research.draft.roleDefinitions[5]!.name, "Research");
  assert.equal(research.draft.roles[5]!.permission, "readOnly");
  assert.ok(isValidTeamDraft(research.draft));
  // A blank role starts read-only (Q4).
  assert.equal(addWorker(defaults(), "새 역할").roles[5]!.permission, "readOnly");
});

test("template text is bounded, plain and written in both languages", () => {
  for (const key of ROLE_TEMPLATE_KEYS) for (const lang of ["ko", "en"] as const) {
    const { name, responsibility } = templateText(key, lang);
    assert.ok(utf8ByteLength(name) <= G5_ROLE_NAME_MAX_BYTES_V3 && utf8ByteLength(responsibility) <= G5_ROLE_RESPONSIBILITY_MAX_BYTES_V3, `${key}/${lang}`);
    assert.ok(responsibility.length > 100, `${key}/${lang}`);
    assert.equal(roleNameProblemsV3([{ roleId: "coordinator", name: "총괄" }, { roleId: "member-aaaaaa", name }])[1], null, `${key}/${lang}`);
    assert.ok(!looksLikeSecret(responsibility));
    assert.ok(ko[`m5.roles.template.${key}`] && en[`m5.roles.template.${key}`], key);
  }
  // Read-only templates never ask for file changes they cannot make.
  for (const key of ["security", "research"] as const) assert.match(templateText(key, "ko").responsibility, /파일은 고치지 않고 보고만 합니다/);
});

test("the unsaved summary counts additions, removals, edits and a changed order", () => {
  const team = defaults();
  assert.deepEqual(teamChanges(team, team), { added: 0, removed: 0, moved: false, edited: 0 });
  let next = insertWorker(team, 2, seed("member-aaaaaa", "QA"));
  next = removeWorker(next, "design");
  next = moveWorker(next, 4, 1);
  next = updateDefinition(next, "coding", { responsibility: "다르게" });
  assert.deepEqual(teamChanges(team, next), { added: 1, removed: 1, moved: true, edited: 1 });
  assert.equal(teamChanges(team, insertWorker(team, 2, seed("member-aaaaaa", "QA"))).moved, false, "an insertion alone keeps the others' order");
});

test("the coordinator's defaults are the seed", () => {
  assert.deepEqual(coordinatorDefaults(), { name: "총괄", responsibility: G5_DEFAULT_ROLE_DEFINITIONS_V3[0]!.responsibility });
});

test("name rules follow the table Rust reads; saving refuses, reading still opens", () => {
  const table = JSON.parse(readFileSync(new URL("./fixtures/m5/role-names-v3.json", import.meta.url), "utf8")) as { formatRanges: [number, number][]; legacyLabels: [string, string[]][]; cases: { note: string; roleIds?: string[]; names: string[]; problems: (string | null)[] }[] };
  assert.deepEqual(G5_FORMAT_CHAR_RANGES_V3, table.formatRanges);
  assert.deepEqual(G5_LEGACY_ROLE_LABELS_V3, table.legacyLabels);
  // The labels are what the screen shows (role-names.ts translates these IDs).
  for (const [roleId, [koLabel, enLabel]] of table.legacyLabels) {
    assert.equal((ko as Record<string, string>)[`m5.roles.${roleId}`], koLabel);
    assert.equal((en as Record<string, string>)[`m5.roles.${roleId}`], enLabel);
  }
  for (const { note, roleIds: listed, names, problems } of table.cases) {
    const roleIds = listed ?? names.map((_, index) => index === 0 ? "coordinator" : `member-${index}`);
    assert.deepEqual(roleNameProblemsV3(roleIds.map((roleId, index) => ({ roleId, name: names[index]! }))), problems, note);
    const roles = roleIds.map(id => profile(id));
    const roleDefinitions = roleIds.map((roleId, index) => ({ roleId, name: names[index]!, responsibility: "" }));
    const ok = problems.every(problem => problem === null);
    assert.equal(isValidTeamDraft({ roles, roleDefinitions }), ok, note);
    const update = () => validateProjectRoleSettingsUpdateRequestV1({ projectId: "p", expectedRevision: 1, roles, roleDefinitions });
    if (ok) update(); else assert.throws(update, /role names/, note);
    const appDefaults = () => validateProjectCatalogRequest({ action: "updateAppRoleDefaults", expectedRevision: 1, roles, roleDefinitions });
    if (ok) appDefaults(); else assert.throws(appDefaults, note);
    // Stored settings and task records with the same names still read (only Cc is refused there).
    validateProjectRoleSettingsV1({ schemaVersion: 3, projectId: "p", revision: 1, roles, roleDefinitions, updatedAt: "2026-11-26T00:00:00Z" });
    validateChatRecordV1({ schemaVersion: 3, chatId: "c", taskId: "t", projectId: "p", worktreeId: "w", workspaceIdentity: "i", goal: "g", settingsRevision: 1, roleSnapshots: roles, roleDefinitions, state: "draft", createdAt: "2026-11-26T00:00:00Z", updatedAt: "2026-11-26T00:00:00Z" });
  }
});

test("the format table is Unicode's Cf category", () => {
  const listed = (code: number) => G5_FORMAT_CHAR_RANGES_V3.some(([low, high]) => code >= low && code <= high);
  for (let code = 0; code <= 0x10ffff; code++) {
    if (code >= 0xd800 && code <= 0xdfff) continue;
    assert.equal(listed(code), /\p{Cf}/u.test(String.fromCodePoint(code)), code.toString(16));
  }
});

test("secret-looking text is noticed; ordinary duties are not", () => {
  for (const text of ["key: sk-ant-api03-abcdefghijklmnopqrstuv", "ghp_0123456789abcdefghijABCDEFGHIJ", "AKIAIOSFODNN7EXAMPLE", "-----BEGIN OPENSSH PRIVATE KEY-----", "AIzaSyA1234567890abcdefghijklmnopqrstu"]) {
    assert.ok(looksLikeSecret(text), text);
  }
  for (const text of [G5_DEFAULT_ROLE_DEFINITIONS_V3[3]!.responsibility, "sk-로 시작하는 키는 적지 마세요", "task-runner를 써서 확인"]) assert.ok(!looksLikeSecret(text), text);
});

test("the menu is called team roles in both languages", () => {
  assert.equal(ko["m5.preferences.roles"], "팀 역할");
  assert.equal(ko["m5.shell.roles"], "팀 역할");
  assert.equal(en["m5.preferences.roles"], "Team roles");
});
