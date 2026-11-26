import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  roleNameProblemsV3, roleSettingsAsV3, validateProjectRoleSettingsUpdateRequestV1,
  type RoleProfileV1,
} from "../src/m5/contracts/run-recovery-v1.ts";
import { addWorker, isValidTeamDraft, removedWorker, removeWorker, uniqueRoleName, updateDefinition, updateProfile, type TeamDraft } from "../src/m5/team-draft.ts";
import { insertTemplate } from "../src/m5/role-templates.ts";
import { roleLabel } from "../src/m5/role-names.ts";
import { appDefaultsLoadStep, saveProjectThenAppDefaults, undoLastRemoval } from "../src/m5/role-editor-actions.ts";
import { orchestratorDelegation, orchestratorMessage, orchestratorMisdirected } from "../src/m5/orchestrator-message.ts";
import { en } from "../src/locales/en.ts";
import { ko } from "../src/locales/ko.ts";

/** Reproductions of the PR #8 review (REVIEW-pr8.md), one per finding. */
const profile = (roleId: string, permission: RoleProfileV1["permission"]): RoleProfileV1 => ({ roleId, runtime: "claude-code", modelSelector: "opus", reasoningEffort: null, permission });
const defaults = (): TeamDraft => roleSettingsAsV3({ roles: ["coordinator", "planning", "design", "coding", "review"].map(id => profile(id, id === "coordinator" ? "readOnly" : "workspaceWrite")) });
const shownEn = (team: TeamDraft) => team.roleDefinitions.map(item => roleLabel(item.roleId, team.roleDefinitions, key => (en as Record<string, string>)[key]!));
const source = (name: string) => readFileSync(new URL(`../src/m5/${name}`, import.meta.url), "utf8");
const saveRequest = (team: TeamDraft) => () => validateProjectRoleSettingsUpdateRequestV1({ projectId: "p", expectedRevision: 1, roles: team.roles, roleDefinitions: team.roleDefinitions });

test("I1: another 'Planning' on an English screen gets a name the screen shows differently", () => {
  // The review's path: the default team, English screen, the Planning template once more.
  const added = insertTemplate(defaults(), 2, "planning", "en", en["m5.roles.planning"], []);
  const shown = shownEn(added.draft);
  assert.equal(new Set(shown).size, shown.length, `shown names repeat: ${shown.join(", ")}`);
  assert.equal(added.draft.roleDefinitions[2]!.name, "Planning 2");
  assert.ok(isValidTeamDraft(added.draft));
  // Typed by hand, the same clash is refused on saving, in either language and spelling.
  const typed = (name: string) => updateDefinition(addWorker(defaults(), "x", "member-a7k2q9"), "member-a7k2q9", { name });
  for (const name of ["Planning", "planning", "Development", "Verification", "기획".normalize("NFD")]) {
    const team = typed(name);
    assert.equal(roleNameProblemsV3(team.roleDefinitions)[5], "duplicate", name);
    assert.throws(saveRequest(team), /role names/, name);
  }
  // A renamed starting role is shown as stored, so its old label is free again.
  const renamed = updateDefinition(defaults(), "planning", { name: "Research" });
  assert.equal(uniqueRoleName(renamed, "Planning"), "Planning");
  assert.equal(uniqueRoleName(defaults(), "Development"), "Development 2");
});

test("I1: the run picker marks the coordinator by ID and isolates names", () => {
  const panel = source("agent-output-panel.tsx");
  const option = panel.slice(panel.indexOf("orderedRuns.map(value => <option"), panel.indexOf("</option>", panel.indexOf("orderedRuns.map(value => <option")));
  assert.match(option, /isLeadRole\(value\.selectedProfile\.roleId\) \? "★ "/);
  assert.match(option, /\\u2068\$\{roleLabel\(/);
});

test("I2: undoing a removal that a template replaced keeps the record and changes nothing", () => {
  // Planning edited to read-only with its own duties, removed, then the Planning template added (same ID).
  let team = updateDefinition(updateProfile(defaults(), "planning", { permission: "readOnly" }), "planning", { responsibility: "내가 고친 설명" });
  const entry = removedWorker(team, "planning")!;
  team = removeWorker(team, "planning");
  team = insertTemplate(team, 1, "planning", "ko", ko["m5.roles.planning"], []).draft;
  const withTemplate = team;
  const blocked = undoLastRemoval(team, [entry]);
  assert.equal(blocked.conflict, "sameId");
  assert.equal(blocked.team, withTemplate, "the template's role stays as it is");
  assert.deepEqual(blocked.removed, [entry], "the removal can still be undone");
  assert.equal(blocked.team.roles.find(role => role.roleId === "planning")!.permission, "workspaceWrite");
  // After removing the template's copy, the undo brings back the edited role.
  const restored = undoLastRemoval(removeWorker(withTemplate, "planning"), blocked.removed);
  assert.equal(restored.conflict, null);
  assert.deepEqual(restored.removed, []);
  const planning = restored.team.roles.find(role => role.roleId === "planning")!;
  assert.equal(planning.permission, "readOnly");
  assert.equal(restored.team.roleDefinitions.find(item => item.roleId === "planning")!.responsibility, "내가 고친 설명");
  // A full team also keeps the record.
  let full = removeWorker(defaults(), "design");
  const design = removedWorker(defaults(), "design")!;
  for (const id of ["member-aaaaa1", "member-aaaaa2", "member-aaaaa3", "member-aaaaa4"]) full = addWorker(full, id, id);
  const fullUndo = undoLastRemoval(full, [design]);
  assert.equal(fullUndo.conflict, "full");
  assert.deepEqual(fullUndo.removed, [design]);
  // The screen uses this and no longer drops the record before trying.
  const editor = source("role-settings.tsx");
  assert.ok(editor.includes("undoLastRemoval(team, removed)"));
  assert.ok(!editor.includes("setRemoved(old => old.slice(0, -1))"));
});

test("I3: loading the app defaults over unsaved changes asks first", () => {
  assert.equal(appDefaultsLoadStep(true), "confirm");
  assert.equal(appDefaultsLoadStep(false), "load");
  const editor = source("role-settings.tsx");
  assert.ok(editor.includes("onClick={requestAppDefaults}"), "the button goes through the confirmation");
  assert.ok(editor.includes('setDiscardAction("appDefaults")'));
  assert.ok(editor.includes('action === "appDefaults") void run(loadAppDefaults)'));
  assert.ok(ko["m5.roles.discardQuestionApp"] && en["m5.roles.discardQuestionApp"] && ko["m5.roles.discardLoadApp"] && en["m5.roles.discardLoadApp"]);
});

test("I4: 'also save as app defaults' reports what was really saved", async () => {
  const calls: string[] = [];
  const app = (fail = false) => async () => { calls.push("app"); if (fail) throw new Error("revision"); };
  // The project's save fails (revision mismatch): the app defaults are left alone.
  assert.equal(await saveProjectThenAppDefaults(async () => false, app()), "none");
  assert.deepEqual(calls, []);
  assert.equal(await saveProjectThenAppDefaults(async () => true, app(true)), "projectOnly");
  assert.equal(await saveProjectThenAppDefaults(async () => true, app()), "both");
  assert.deepEqual(calls, ["app", "app"]);
  // The screen uses it, and only "both" shows the saved notice.
  const editor = source("role-settings.tsx");
  assert.ok(editor.includes("saveProjectThenAppDefaults(() => onSave(saved.roles, saved.roleDefinitions)"));
  const workbench = source("workbench.tsx");
  assert.ok(workbench.includes('appDefaultsOutcome === "both" && <p role="status">{t("m5.roles.appDefaultsAlsoSaved")}'));
  assert.ok(workbench.includes('appDefaultsOutcome === "projectOnly" && <p role="alert">{t("m5.roles.appDefaultsNotSaved")}'));
});

test("m1: a new member avoids the role IDs of the project's tasks", () => {
  const editor = source("role-settings.tsx");
  assert.ok(editor.includes("for (const roleId of knownRoleIds) taken.current.add(roleId);"));
  assert.ok(source("workbench.tsx").includes("knownRoleIds={chat.chats.flatMap(item => item.roleSnapshots.map(role => role.roleId))}"));
});

test("m2: Escape returns the focus to the element that opened the menu", () => {
  const editor = source("role-settings.tsx");
  assert.ok(editor.includes("menuOpener.current?.isConnected ? menuOpener.current : returnTo()"));
  // The insertion line, the ⋯ button and '역할 추가' each record themselves as the opener.
  assert.equal(editor.split("event.currentTarget)}").length - 1, 3);
});

test("a delegation to someone outside the team is not shown as raw JSON", () => {
  const team = ["coordinator", "member-a7k2q9", "coding"];
  const decision = (role: string) => JSON.stringify({ action: "delegate", role, instruction: "조사해 주세요", summary: "자료 조사에게 맡길게요" });
  for (const role of ["자료 조사", "planning", "coordinator"]) {
    assert.equal(orchestratorMisdirected(decision(role), "coordinator", team), role);
    assert.equal(orchestratorDelegation(decision(role), "coordinator", team), null);
  }
  assert.equal(orchestratorMisdirected(decision("member-a7k2q9"), "coordinator", team), null, "a member is an ordinary delegation");
  assert.equal(orchestratorMessage(decision("member-a7k2q9"), "coordinator", team), "자료 조사에게 맡길게요");
  assert.equal(orchestratorMisdirected(decision("QA"), "member-a7k2q9", team), null, "only the coordinator decides");
  assert.equal(orchestratorMisdirected("not json", "coordinator", team), null);
  assert.equal(orchestratorMisdirected(decision("x".repeat(200)), "coordinator", team)!.length, 32);
  assert.ok(source("orchestration-message-body.tsx").includes('t("m5.chat.misdirected"'));
  assert.ok(ko["m5.chat.misdirected"].includes("{role}") && en["m5.chat.misdirected"].includes("{role}"));
});
