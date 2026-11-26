// ADE screen unification U5 (agent connection, F11 login guides) and U6 (the
// notification panel, D10). Each rule the screens rest on is held here.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { LOGIN_GUIDE_KINDS, loginGuide, loginGuideKind } from "../src/m5/login-guide.ts";
import { loginErrorMessage } from "../src/m5/provider-login-message.ts";
import { attentionCount, groupNotifications, needsAttention } from "../src/m5/notification-groups.ts";
import type { AppNotification } from "../src/m5/notifications.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

const source = (name: string) => readFileSync(`src/m5/${name}`, "utf8");
const ERRORS: Record<string, string | null> = {
  recovery: "providerLoginRecoveryRequired", busy: "providerBusy", changed: "providerExecutableChanged", missing: "providerExecutableMissing",
  profile: "providerProfileUnavailable", profileCompatibility: "providerProfileIsolationRequired", failed: "providerLoginFailed",
};

test("U5: every login failure kind is a guide with one main action, its title and alternatives", () => {
  assert.equal(LOGIN_GUIDE_KINDS.length, 7);
  for (const kind of LOGIN_GUIDE_KINDS) {
    assert.equal(loginGuideKind(ERRORS[kind]), kind);
    for (const provider of ["codex", "claude"] as const) {
      const guide = loginGuide(provider, ERRORS[kind]);
      assert.equal(guide.kind, kind);
      assert.ok(guide.primary, `${kind} has a main action`);
      assert.ok(!guide.alternatives.includes(guide.primary), `${kind}: the main action is not repeated`);
      for (const dict of [ko, en]) assert.match(dict[`m5.login.guide.title.${kind}` as keyof typeof ko], /\{name\}/, `${kind} title`);
    }
  }
  // Unknown or missing errors are the general failure.
  assert.equal(loginGuideKind(null), "failed");
  assert.equal(loginGuideKind("somethingNew"), "failed");
});

test("U5: a new sign-in is never the main action where it could hide the real problem", () => {
  for (const kind of ["recovery", "busy", "changed", "missing", "profile", "profileCompatibility"] as const) {
    for (const provider of ["codex", "claude"] as const) {
      const guide = loginGuide(provider, ERRORS[kind]);
      assert.notEqual(guide.primary, "retryLogin", `${provider} ${kind}`);
      assert.ok(!guide.alternatives.includes("retryLogin"), `${provider} ${kind}`);
    }
  }
  assert.equal(loginGuide("codex", ERRORS.missing).primary, "install");
  assert.equal(loginGuide("claude", ERRORS.failed).primary, "retryLogin", "an expired Claude code needs a new sign-in");
  assert.equal(loginGuide("codex", ERRORS.failed).primary, "check", "a saved Codex login is checked first");
  // The cautious text stays the guide's "what happened" (provider-login tests hold its wording).
  assert.match(loginErrorMessage("codex", ERRORS.profileCompatibility), /삭제하지 말고/);
});

test("U5: the connection card says the failure as a guide; the raw key only in the folded help details", () => {
  const login = source("provider-login.tsx");
  assert.match(login, /<RecoveryGuide tone=\{guide\.tone\}/);
  assert.match(login, /details=\{diagnostic \?\? status\.error \?\? "providerLoginUnavailable"\}/);
  assert.doesNotMatch(login, /<code>\{diagnostic \?\? status\.error/, "no raw error key outside the guide's details");
  assert.doesNotMatch(login, />↻</, "the check button says what it does");
  const guide = source("recovery-guide.tsx");
  assert.match(guide, /<details className="m5-recover-details">/);
  assert.equal(guide.match(/className="m5-btn-primary"/g)?.length, 1, "one main action");
});

test("U5: connect buttons say only the action; the agent's name goes to the screen reader label", () => {
  for (const [key, value] of [["m5.connection.connect", "연결하기"], ["m5.connection.disconnect", "연결 끊기"], ["m5.login.start", "로그인"]] as const) assert.equal(ko[key], value);
  for (const dict of [ko, en]) {
    for (const key of ["m5.connection.connect", "m5.connection.disconnect", "m5.login.guide.action.retryLogin", "m5.login.guide.action.check", "m5.login.guide.action.install"] as const) {
      assert.doesNotMatch(dict[key], /Codex|Claude/, key);
    }
    assert.match(dict["m5.connection.connectNamed"], /\{name\}/);
    assert.match(dict["m5.connection.disconnectNamed"], /\{name\}/);
  }
  assert.match(source("provider-login.tsx"), /aria-label=\{t\(appConnected \? "m5\.connection\.disconnectNamed" : "m5\.connection\.connectNamed", \{ name \}\)\}/);
});

test("U5: the install consent is a confirm dialog: Cancel takes the first focus, Start is the one main action", () => {
  const install = source("provider-install.tsx");
  assert.match(install, /className="m5-install-confirmation m5-confirm-dialog"/);
  assert.match(install, /<button type="button" data-first-focus disabled=\{busy\}/);
  assert.match(install, /<button type="submit" className="m5-btn-primary" disabled=\{disabled \|\| busy\}>\{t\("m5\.install\.start"\)\}/);
  // A guide only asks to look at installing; the dialog still decides (no start without the consent).
  assert.match(install, /void request\(\{ action: "prepare", provider \}\)/);
  assert.doesNotMatch(install.slice(install.indexOf("const prepare = (event: Event)"), install.indexOf("window.addEventListener(INSTALL_PREPARE")), /action: "start"/);
});

test("U5: a failing agent offers moving its roles to another agent", () => {
  assert.match(source("provider-login.tsx"), /case "switchRoles": return onSwitchRoles \?/);
  assert.match(source("workbench.tsx"), /onSwitchRoles=\{\(\) => navigatePreferences\(/);
});

const item = (id: string, kind: AppNotification["kind"], projectId: string, chatId: string, minutes: number, resolved = false): AppNotification => ({
  id, kind, projectId, chatId, goal: `goal of ${chatId}`, text: id, at: new Date(Date.UTC(2026, 10, 26, 12, 60 - minutes)).toISOString(), read: false, ...(resolved ? { resolved: true } : {}),
});

test("U6: the badge counts only open requests", () => {
  const items = [item("a", "attention", "p", "c", 1), item("b", "attention", "p", "c", 2, true), item("c", "done", "p", "c", 3), item("d", "stopped", "p", "c", 4)];
  assert.equal(attentionCount(items), 1);
  assert.deepEqual(items.filter(needsAttention).map(entry => entry.id), ["a"]);
});

test("U6: lines group by project, then task (goal once); settled lines fold; the newest leads", () => {
  const items = [
    item("old-done", "done", "p2", "c3", 50),
    ...Array.from({ length: 8 }, (_, index) => item(`req-${index}`, "attention", "p1", "c1", 10 + index, index >= 2)),
    item("other-task", "stopped", "p1", "c2", 30),
  ];
  const all = groupNotifications(items, "all");
  assert.deepEqual(all.map(group => group.projectId), ["p1", "p2"]);
  assert.deepEqual(all[0].tasks.map(task => task.chatId), ["c1", "c2"]);
  const task = all[0].tasks[0];
  assert.equal(task.goal, "goal of c1");
  assert.deepEqual(task.open.map(entry => entry.id), ["req-0", "req-1"], "eight lines of one task are one group, open first");
  assert.equal(task.settled.length, 6);
  const attention = groupNotifications(items, "attention");
  assert.deepEqual(attention.map(group => group.projectId), ["p1"]);
  assert.deepEqual(attention[0].tasks.map(task => [task.chatId, task.open.length, task.settled.length]), [["c1", 2, 0]]);
  assert.deepEqual(groupNotifications([], "all"), []);
});

test("U6: the panel names projects, has the attention tab and folds settled lines", () => {
  const bell = source("notification-bell.tsx");
  assert.match(bell, /const names = useProjectNames\(open,/);
  assert.match(bell, /<ProjectMark name=\{name\} \/>/);
  assert.match(bell, /aria-pressed=\{tab === "attention"\}/);
  assert.match(bell, /<details className="m5-notification-settled">/);
  assert.match(bell, /\{waiting > 0 && <span className=\{`m5-notification-count/);
  for (const dict of [ko, en]) for (const key of ["m5.notifications.all", "m5.notifications.emptyAttention", "m5.notifications.settledCount", "m5.notifications.waiting"] as const) assert.ok(dict[key], key);
});

test("U5·U6: ko and en carry the same new keys", () => {
  const prefixes = ["m5.login.guide.", "m5.login.badge.", "m5.guide.", "m5.connection.", "m5.notifications."];
  const pick = (dict: Record<string, string>) => Object.keys(dict).filter(key => prefixes.some(prefix => key.startsWith(prefix))).sort();
  assert.deepEqual(pick(ko), pick(en));
});
