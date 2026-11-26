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
  unreachable: "providerLoginTransportUnavailable",
};

test("U5: every login failure kind is a guide with one main action, its title and alternatives", () => {
  assert.equal(LOGIN_GUIDE_KINDS.length, 8);
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
  for (const kind of ["recovery", "busy", "changed", "missing", "profile", "profileCompatibility", "unreachable"] as const) {
    for (const provider of ["codex", "claude"] as const) {
      const guide = loginGuide(provider, ERRORS[kind]);
      assert.notEqual(guide.primary, "retryLogin", `${provider} ${kind}`);
      assert.ok(!guide.alternatives.includes("retryLogin"), `${provider} ${kind}`);
    }
  }
  assert.equal(loginGuide("codex", ERRORS.missing).primary, "install");
  // A general failure is not proof of an expired code: both agents check first, a new sign-in is the alternative (PR #14 review I1).
  for (const provider of ["codex", "claude"] as const) {
    assert.equal(loginGuide(provider, ERRORS.failed).primary, "check");
    assert.ok(loginGuide(provider, ERRORS.failed).alternatives.includes("retryLogin"));
  }
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
    for (const key of ["m5.connection.connect", "m5.connection.disconnect", "m5.login.guide.action.retryLogin", "m5.login.guide.action.install", "m5.login.guide.action.switchRoles"] as const) {
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
  assert.match(source("workbench.tsx"), /onSwitchRoles=\{snapshot\.project && !projectDataReady \? undefined : \(\) => navigatePreferences\(/);
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

test("PR #14 I1: a request that never reached the agent is 'unreachable' and never asks for a new sign-in", () => {
  for (const code of ["providerLoginCapabilityUnavailable", "providerLoginRequestRejected", "providerLoginRequestBlocked", "providerLoginResponseInvalid", "providerLoginTransportUnavailable", "providerLoginTransportFailed"]) {
    for (const provider of ["codex", "claude"] as const) {
      const guide = loginGuide(provider, code);
      assert.equal(guide.kind, "unreachable", code);
      assert.equal(guide.primary, "check");
      assert.ok(!guide.alternatives.includes("retryLogin"), code);
    }
  }
  // The card hands the fixed diagnostic code of a failed request to the guide, not null.
  assert.match(source("provider-login.tsx"), /loginGuide\(provider, error \? diagnostic \?\? "providerLoginTransportUnavailable" : status\.error\)/);
});

test("PR #14 I2: an answered question stops counting, and keeping the newest lines never drops an open request", async () => {
  const { NotificationCenter } = await import("../src/m5/notifications.ts");
  const { startNotificationFeed } = await import("../src/m5/notification-feed.ts");
  const store = new Map<string, string>();
  const center = new NotificationCenter({ getItem: key => store.get(key) ?? null, setItem: (key, value) => { store.set(key, value); } });
  const listeners: Record<string, (event: unknown) => void> = {};
  const stop = startNotificationFeed({
    onAgentRunStateChanged: listener => { listeners.run = listener as (event: unknown) => void; return () => undefined; },
    onAgentPublicEvent: listener => { listeners.public = listener as (event: unknown) => void; return () => undefined; },
    getChat: async () => ({ chat: { chatId: "c", projectId: "p", goal: "g", state: "waitingForUser" } }),
  }, center, undefined, () => undefined);
  listeners.public({ eventId: "q1", chatId: "c", kind: "question", text: "어떤 색이 좋을까요?" });
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(attentionCount(center.snapshot()), 1);
  // Many newer lines do not push the open question out.
  for (let index = 0; index < 60; index++) center.add({ id: `done:${index}`, kind: "done", projectId: "p", chatId: "c", goal: "g", text: "", at: new Date().toISOString() });
  assert.equal(attentionCount(center.snapshot()), 1);
  assert.ok(center.snapshot().length <= 51);
  // The person answered: the task runs again, and the question is settled.
  listeners.run({ run: { runId: "r2", chatId: "c", taskId: "t", projectId: "p", worktreeId: "w", workspaceIdentity: "i", state: "running", selectedProfile: { roleId: "coordinator" } } });
  assert.equal(attentionCount(center.snapshot()), 0);
  assert.equal(center.snapshot().find(item => item.id === "question:q1")?.resolved, true);
  stop();
});

test("PR #14 I3·minor 1: names are kept page by page, reading stops once all are named, and stored data cannot break the panel", async () => {
  const bell = source("notification-bell.tsx");
  assert.match(bell, /for \(let page = 0; page < 20 && needed\.size; page\+\+\)/);
  assert.match(bell, /setNames\(current => new Map\(\[\.\.\.current, \.\.\.found\]\)\)/, "each page's names are kept as they come");
  assert.match(bell, /names\.get\(project\.projectId\)/, "a Map: no inherited property can be read as a name");
  assert.match(bell, /if \(!Number\.isFinite\(Date\.parse\(at\)\)\) return "";/);
  assert.match(bell, /<bdi>\{name\}<\/bdi>/);
  const { NotificationCenter } = await import("../src/m5/notifications.ts");
  const bad = { id: "x", kind: "done", projectId: "constructor", chatId: "c", goal: "g", text: "", at: "not a time", read: false };
  const center = new NotificationCenter({ getItem: () => JSON.stringify([bad, { ...bad, id: "y", at: "2026-11-26T00:00:00Z" }]), setItem: () => undefined });
  assert.deepEqual(center.snapshot().map(item => item.id), ["y"], "a damaged time is not restored");
});

test("PR #14 I4: a notification is read only once its task opened; a blocked switch says why and leaves nothing pending", () => {
  const bell = source("notification-bell.tsx");
  assert.match(bell, /if \(await onOpen\(item\) !== false\) center\.markRead\(item\.id\);/);
  const workbench = source("workbench.tsx");
  const open = workbench.slice(workbench.indexOf("const openNotification = async"), workbench.indexOf("const openNotificationRef"));
  assert.match(open, /if \(controller\.snapshot\(\)\.project && switchBlockedReason\) \{ setOpenBlocked\(switchBlockedReason\); return false; \}/);
  assert.match(open, /await switchProject\(/, "the same one-step switch as the switcher");
  assert.match(open, /setPendingOpen\(null\);\n    return false;/);
});

test("PR #14 minor 2·3: the roles shortcut says where it goes and waits for the project; the guide's check is the card's one", () => {
  assert.equal(ko["m5.login.guide.action.switchRoles"], "팀 역할 열기");
  for (const dict of [ko, en]) assert.ok(dict["m5.login.guide.action.switchRolesNamed"].includes(dict["m5.login.guide.action.switchRoles"]), "the read name holds the visible words");
  assert.match(source("workbench.tsx"), /onSwitchRoles=\{snapshot\.project && !projectDataReady \? undefined :/);
  const login = source("provider-login.tsx");
  assert.match(login, /\{!guide && <button type="button" className="m5-connection-check"/);
  assert.match(login, /case "check": return \{ label: t\("m5\.login\.check"\)/);
  assert.match(source("notification-bell.tsx"), /\{item\.text && <><span className="m5-notification-kind">/, "an empty line shows its kind once");
});
