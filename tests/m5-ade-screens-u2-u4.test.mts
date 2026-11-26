// ADE screen unification U2·U3·U4 (DESIGN 4.1): the home, the first run, the
// switcher, the new task window and the settings pages. Each decision the
// screens rest on is held here so a later change cannot quietly undo it.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { DEFAULT_NOTIFICATION_PREFERENCES, NOTIFICATION_PREFERENCES_CHANGED, NotificationCenter, PRUNE_INTERVAL_MS, readNotificationPreferences, RESOLVED_KEEP_MS, saveNotificationPreferences, type NotificationPreferences } from "../src/m5/notifications.ts";
import { byRecency, openedAgo, projectMark, projectName, readProjectOpenedAt, recordProjectOpened } from "../src/m5/project-recency.ts";
import { firstRunStep } from "../src/m5/first-run-step.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

function memory() {
  const store = new Map<string, string>();
  return { store, getItem: (key: string) => store.get(key) ?? null, setItem: (key: string, value: string) => { store.set(key, value); } };
}
const source = (name: string) => readFileSync(`src/m5/${name}`, "utf8");
const item = (id: string, kind: "attention" | "done" | "stopped" = "done") => ({ id, kind, projectId: "p", chatId: "c", goal: "g", text: "t", at: "2026-11-20T00:00:00.000Z" });

test("Q4: notification kinds can be turned off; an off kind is not kept nor bannered", () => {
  const storage = memory();
  let preferences: NotificationPreferences = { ...DEFAULT_NOTIFICATION_PREFERENCES, kinds: { attention: true, done: false, stopped: true } };
  const center = new NotificationCenter(storage, () => preferences);
  assert.equal(center.add(item("a", "done")), false, "done is off");
  assert.equal(center.add(item("b", "attention")), true);
  preferences = DEFAULT_NOTIFICATION_PREFERENCES;
  assert.equal(center.add(item("a", "done")), true, "turned back on, it is heard again");
  assert.deepEqual(center.snapshot().map(entry => entry.id), ["a", "b"]);
});

test("Q4: preferences are read safely; damaged or partial records fall back to on", () => {
  const storage = memory();
  assert.deepEqual(readNotificationPreferences(storage), DEFAULT_NOTIFICATION_PREFERENCES);
  saveNotificationPreferences({ kinds: { attention: true, done: false, stopped: true }, pruneResolved: false }, storage);
  assert.deepEqual(readNotificationPreferences(storage), { kinds: { attention: true, done: false, stopped: true }, pruneResolved: false });
  storage.setItem("elsecrew.m5.notifications.prefs.v1", "{broken");
  assert.deepEqual(readNotificationPreferences(storage), DEFAULT_NOTIFICATION_PREFERENCES);
  storage.setItem("elsecrew.m5.notifications.prefs.v1", JSON.stringify({ kinds: { done: "no" } }));
  assert.deepEqual(readNotificationPreferences(storage), DEFAULT_NOTIFICATION_PREFERENCES);
  assert.deepEqual(readNotificationPreferences(null), DEFAULT_NOTIFICATION_PREFERENCES);
});

test("Q4: settled lines are tidied away a day after they were settled, only when kept on; open ones never", () => {
  const storage = memory();
  let now = Date.parse("2026-11-20T00:00:00.000Z");
  const first = new NotificationCenter(storage, () => DEFAULT_NOTIFICATION_PREFERENCES, () => now);
  first.add({ ...item("approval:1", "attention"), chatId: "c1" });
  first.add(item("open", "attention"));
  first.resolveApprovalsFor("c1");
  assert.equal(first.snapshot().find(entry => entry.id === "approval:1")?.resolvedAt, "2026-11-20T00:00:00.000Z");
  now += RESOLVED_KEEP_MS - 1000;
  assert.equal(new NotificationCenter(storage, () => DEFAULT_NOTIFICATION_PREFERENCES, () => now).snapshot().length, 2, "within a day it stays");
  now += 2000;
  const off = new NotificationCenter(storage, () => ({ ...DEFAULT_NOTIFICATION_PREFERENCES, pruneResolved: false }), () => now);
  assert.equal(off.snapshot().length, 2, "tidying turned off keeps it");
  const on = new NotificationCenter(storage, () => DEFAULT_NOTIFICATION_PREFERENCES, () => now);
  assert.deepEqual(on.snapshot().map(entry => entry.id), ["open"], "an open line is never tidied");
  assert.deepEqual(JSON.parse(storage.getItem("elsecrew.m5.notifications.v1")!).map((entry: { id: string }) => entry.id), ["open"], "and the record follows");
});

test("Q1: the home orders by when each project was last opened here; unopened keep the engine's order after", () => {
  const storage = memory();
  recordProjectOpened(storage, "b", new Date("2026-11-20T00:00:00Z"));
  recordProjectOpened(storage, "c", new Date("2026-11-25T00:00:00Z"));
  recordProjectOpened(storage, "../evil", new Date());
  const openedAt = readProjectOpenedAt(storage);
  assert.deepEqual(Object.keys(openedAt).sort(), ["b", "c"], "only project ids are kept");
  const entries = ["a", "b", "c", "d"].map(projectId => ({ projectId }));
  assert.deepEqual(byRecency(entries, openedAt).map(entry => entry.projectId), ["c", "b", "a", "d"]);
  storage.setItem("elsecrew.m5.projectOpenedAt.v1", "[1,2]");
  assert.deepEqual(readProjectOpenedAt(storage), {});
  storage.setItem("elsecrew.m5.projectOpenedAt.v1", JSON.stringify({ a: "not a date", b: "2026-11-20T00:00:00Z" }));
  assert.deepEqual(readProjectOpenedAt(storage), { b: "2026-11-20T00:00:00Z" });
  assert.deepEqual(readProjectOpenedAt(null), {});
});

test("Q1: how long ago reads in the screen's language; a project never opened here says so instead", () => {
  const now = Date.parse("2026-11-26T12:00:00Z");
  assert.equal(openedAgo("2026-11-26T11:57:00Z", "ko", now), "3분 전");
  assert.equal(openedAgo("2026-11-25T12:00:00Z", "ko", now), "어제");
  assert.equal(openedAgo("2026-11-26T09:00:00Z", "en", now), "3 hours ago");
  assert.match(openedAgo("2026-11-01T12:00:00Z", "ko", now)!, /11월 1일/);
  assert.equal(openedAgo(undefined, "ko", now), null);
  assert.equal(openedAgo("garbage", "ko", now), null);
});

test("Q1: a card names the project by its folder and marks it with a steady letter and role color", () => {
  assert.equal(projectName("/Users/me/work/my-bakery-site", "x"), "my-bakery-site");
  assert.equal(projectName("C:\\work\\가계부\\", "x"), "가계부");
  assert.equal(projectName("/", "fallback"), "fallback");
  const mark = projectMark("my-bakery-site");
  assert.deepEqual(projectMark("my-bakery-site"), mark, "the same name gives the same mark");
  assert.equal(mark.letters, "M");
  assert.ok(mark.color >= 1 && mark.color <= 8);
  assert.equal(projectMark("가계부").letters, "가");
  assert.equal(projectMark("...").letters, ".");
});

test("D11: the first run's current step is connecting until one agent is connected, then choosing a project", () => {
  assert.equal(firstRunStep(null), 1);
  assert.equal(firstRunStep({ codex: false, "claude-code": false }), 1);
  assert.equal(firstRunStep({ codex: true, "claude-code": false }), 2);
  assert.equal(firstRunStep({ codex: false, "claude-code": true }), 2);
});

test("Q5·Q6: the home has no left rail; paths are not printed, only kept for hover and checks", () => {
  const workbench = source("workbench.tsx");
  const catalog = source("project-catalog-view.tsx");
  assert.doesNotMatch(workbench, /m5-project-rail|railExpanded|m5-sidebar-reopen/);
  assert.doesNotMatch(source("styles.css"), /\.m5-home[^{]*\.m5-project-rail/);
  // A path is never a text node on the cards: only data-path and title carry it.
  assert.doesNotMatch(catalog, />\{[^}]*canonicalPath[^}]*\}</);
  assert.match(catalog, /data-path=\{[^}]*canonicalPath/);
  // One-step switching: leaving and opening happen in one action.
  assert.match(workbench, /const switchProject = /);
});

test("PR #12 B1: a folder becomes a project only after main's own box; the page cannot answer for it", () => {
  const main = readFileSync("electron/main.ts", "utf8");
  const open = main.slice(main.indexOf("M5_BRIDGE_V2_CHANNELS.projectOpen, async"), main.indexOf("const invoke = <T>"));
  // Empty folder, plain folder and lost record each go through dialog.showMessageBox in main.
  assert.equal(open.match(/dialog\.showMessageBox\(/g)?.length, 3);
  assert.doesNotMatch(main, /folder-confirm|askFolderConfirm/);
  assert.doesNotMatch(readFileSync("electron/preload.ts", "utf8"), /m5FolderConfirm|folder-confirm/);
  assert.doesNotMatch(source("workbench.tsx"), /m5FolderConfirm|folder-confirm/);
  assert.equal(Object.keys(ko).some(key => key.startsWith("m5.folderConfirm.")), false);
});

test("Q9: a project's first task still needs its own yes before Send; Send never stands in for it", () => {
  const workbench = source("workbench.tsx");
  assert.match(workbench, /className="m5-first-chat-confirm"><input type="checkbox" checked=\{setupConfirmed\}/);
  const send = workbench.slice(workbench.indexOf('<button type="submit" title={t("m5.chat.sendShortcut")}'));
  assert.match(send.slice(0, 800), /\(chat\.chats\.length === 0 && !firstSetupAgreed\)/);
});

test("U4: settings pages are built from groups and setting rows only", () => {
  const settings = source("app-settings.tsx");
  for (const [name, text] of [["app-settings.tsx", settings], ["workspace-folder-setting.tsx", source("workspace-folder-setting.tsx")], ["discarded-work-cleanup.tsx", source("discarded-work-cleanup.tsx")], ["backup-storage-usage.tsx", source("backup-storage-usage.tsx")]] as const) {
    assert.match(text, /m5-setting\b|<Setting\b|<SettingsGroup\b/, name);
  }
  // Group labels in the navigation: the app's pages, then this project (Q8).
  assert.match(settings, /m5\.preferences\.groupApp/);
  assert.match(settings, /m5\.preferences\.groupThisProject/);
  assert.doesNotMatch(settings, /m5-preferences-section-card|<fieldset/);
});

test("rule: a button says only the action, never the name already on its row", () => {
  for (const [key, value] of [["m5.start.connectAction", "연결하기"], ["m5.newTask.changeTeam", "바꾸기"], ["m5.home.continueAction", "이어서 하기"]] as const) {
    assert.equal(ko[key], value, key);
  }
  for (const dict of [ko, en]) {
    assert.doesNotMatch(dict["m5.start.connectAction"], /Codex|Claude/);
    assert.match(dict["m5.start.connectNamed"], /\{name\}/, "the name goes to the screen reader label instead");
  }
});

test("ko and en carry the same new keys", () => {
  const prefixes = ["m5.home.", "m5.start.", "m5.switch.", "m5.remove.", "m5.newTask.", "m5.notifications.settings.", "m5.preferences."];
  const pick = (dict: Record<string, string>) => Object.keys(dict).filter(key => prefixes.some(prefix => key.startsWith(prefix))).sort();
  assert.deepEqual(pick(ko), pick(en));
});

test("PR #12 I1: the first task's window saves nothing before its separate yes, and a changed permission asks again", () => {
  const permissions = source("project-permissions.tsx");
  // proposeAll (the first task) draws the meaning of allowing, not a save of its own.
  assert.match(permissions, /\{proposeAll \? <p className="m5-settings-intro m5-permission-boundary">/);
  const workbench = source("workbench.tsx");
  const create = workbench.slice(workbench.indexOf("const createChat = async () => {"), workbench.indexOf("const activeTab ="));
  // The yes is checked before anything (team or permissions) is stored.
  const guard = create.indexOf("if (chat.chats.length === 0 && !firstSetupAgreed) return;");
  assert.ok(guard > 0 && guard < create.indexOf("fillRecommendedRoles") && guard < create.indexOf("setAutomationPermissions"));
  assert.match(workbench, /const firstSetupAgreed = setupConfirmed && firstPermissions !== null && firstPermissions\.projectId === snapshot\.project\?\.id;/);
  assert.match(workbench, /useEffect\(\(\) => \{ setSetupConfirmed\(false\); \}, \[firstPermissions\?\.projectId, firstPermissions\?\.permissions\.files, firstPermissions\?\.permissions\.commands\]\);/);
  assert.doesNotMatch(source("styles.css"), /m5-new-chat-dialog[^{]*(m5-permission-save|m5-project-permissions > \.m5-settings-intro)[^{]*\{[^}]*display:\s*none/);
});

test("PR #12 I2: Add in the switcher is off with its reason while a task is open or edits are unsaved; a failed open says why on the home", () => {
  const workbench = source("workbench.tsx");
  assert.match(workbench, /const addBlockedReason = [^;]*snapshot\.workspace \? t\("m5\.switch\.addTaskOpen"\) : !controller\.canLeaveProject\(\) \? t\("m5\.switch\.unsaved"\)/);
  assert.match(source("project-catalog-view.tsx"), /disabled=\{disabled \|\| busy \|\| Boolean\(addBlockedReason\)\}/);
  assert.match(workbench, /\{!snapshot\.project && snapshot\.error && !resumingProject && <section className="m5-banner m5-banner-danger m5-home-error" role="alert"/);
});

test("PR #12 I3: a list that cannot be read is not 'no projects'; the home keeps its error and retry", () => {
  assert.match(source("project-catalog-view.tsx"), /onListState\?\.\(error \? "failed" : entries\.length > 0 \? "projects" : "empty"\)/);
  assert.match(source("workbench.tsx"), /setHasSavedProjects\(state !== "empty"\)/);
});

test("PR #12 I5: the delete confirmation reads the whole folder, not only its name", () => {
  assert.match(source("workbench.tsx"), /removal\?\.path && !removal\.chatId && <p className="m5-remove-target" data-testid="remove-target"><span>\{t\("m5\.remove\.folder"\)\}<\/span><code>\{removal\.path\}<\/code>/);
  assert.doesNotMatch(source("workbench.tsx"), /<h2 id="m5-remove-title" title=/);
});

test("PR #12 minor 1: settings and the team roles window give focus back to what opened them", () => {
  const workbench = source("workbench.tsx");
  assert.match(workbench, /useLayoutEffect\(\(\) => \{ if \(preferencesOpen\) rememberFocus\(\); \}, \[preferencesOpen\]\);/);
  assert.match(workbench, /if \(!preferencesOpen && !settingsOpen && !newChatOpen && !removal && focusReturn\.current\) focusBack\(\);/);
  assert.doesNotMatch(workbench, /settingsButtonRef/);
});

test("PR #12 minor 2: buttons say the action; the screen reader name carries the target", () => {
  assert.equal(ko["m5.permissions.save"], "저장");
  assert.equal(ko["m5.workspaceFolder.choose"], "바꾸기");
  for (const dict of [ko, en]) {
    assert.match(dict["m5.home.continueNamed"], /\{name\}/);
    assert.match(dict["m5.catalog.manageNamed"], /\{name\}/);
    assert.ok(dict["m5.permissions.saveNamed"] && dict["m5.workspaceFolder.chooseNamed"]);
  }
  const catalog = source("project-catalog-view.tsx");
  assert.match(catalog, /aria-label=\{t\("m5\.home\.continueNamed", \{ name \}\)\}/);
  assert.match(catalog, /<ProjectMenu label=\{t\("m5\.catalog\.manageNamed", \{ name \}\)\}>/);
});

test("PR #12 minor 3: the home and the switcher read the engine's further pages before ordering by recency", () => {
  const catalog = source("project-catalog-view.tsx");
  assert.match(catalog, /const RECENCY_PAGES = 10;/);
  assert.match(catalog, /page < \(home \|\| switcher \? RECENCY_PAGES : 1\)/);
});

test("PR #12 minor 4: settled notifications are tidied while the app runs and when the setting changes", () => {
  const previous = (globalThis as { window?: unknown }).window;
  const events: string[] = [];
  (globalThis as { window?: unknown }).window = { dispatchEvent: (event: Event) => { events.push(event.type); return true; } };
  try {
    saveNotificationPreferences(DEFAULT_NOTIFICATION_PREFERENCES, memory());
    assert.deepEqual(events, [NOTIFICATION_PREFERENCES_CHANGED]);
  } finally { (globalThis as { window?: unknown }).window = previous; }
  const workbench = source("workbench.tsx");
  assert.match(workbench, /window\.addEventListener\(NOTIFICATION_PREFERENCES_CHANGED, prune\)/);
  assert.match(workbench, /window\.setInterval\(prune, PRUNE_INTERVAL_MS\)/);
  assert.ok(PRUNE_INTERVAL_MS <= RESOLVED_KEEP_MS);
});
