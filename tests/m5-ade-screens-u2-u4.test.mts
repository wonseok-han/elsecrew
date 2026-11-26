// ADE screen unification U2·U3·U4 (DESIGN 4.1): the home, the first run, the
// switcher, the new task window and the settings pages. Each decision the
// screens rest on is held here so a later change cannot quietly undo it.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { DEFAULT_NOTIFICATION_PREFERENCES, NotificationCenter, readNotificationPreferences, RESOLVED_KEEP_MS, saveNotificationPreferences, type NotificationPreferences } from "../src/m5/notifications.ts";
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

test("Q7: the folder confirmations are the app's own dialog; main no longer opens a native box for them", () => {
  const main = readFileSync("electron/main.ts", "utf8");
  const open = main.slice(main.indexOf("M5_BRIDGE_V2_CHANNELS.projectOpen, async"), main.indexOf("const invoke = <T>"));
  assert.doesNotMatch(open, /showMessageBox/);
  assert.equal(open.match(/askFolderConfirm\(/g)?.length, 2);
  for (const kind of ["emptyFolder", "plainFolder", "recordMissing"]) {
    for (const part of ["title", "body", "confirm"]) {
      assert.ok(ko[`m5.folderConfirm.${kind}.${part}` as keyof typeof ko], `ko ${kind}.${part}`);
      assert.ok(en[`m5.folderConfirm.${kind}.${part}` as keyof typeof en], `en ${kind}.${part}`);
    }
  }
  const preload = readFileSync("electron/preload.ts", "utf8");
  assert.match(preload, /exposeInMainWorld\("m5FolderConfirm"/);
  assert.doesNotMatch(preload.slice(preload.indexOf("const folderConfirmApi"), preload.indexOf('exposeInMainWorld("m5FolderConfirm"')), /path\s*:\s*unknown|answer: async \([^)]*path/, "the page answers without a folder");
});

test("Q9: a project's first task still needs its own yes before Send; Send never stands in for it", () => {
  const workbench = source("workbench.tsx");
  assert.match(workbench, /className="m5-first-chat-confirm"><input type="checkbox" checked=\{setupConfirmed\}/);
  const send = workbench.slice(workbench.indexOf('<button type="submit" title={t("m5.chat.sendShortcut")}'));
  assert.match(send.slice(0, 800), /\(chat\.chats\.length === 0 && !setupConfirmed\)/);
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
  const prefixes = ["m5.home.", "m5.start.", "m5.switch.", "m5.remove.", "m5.folderConfirm.", "m5.newTask.", "m5.notifications.settings.", "m5.preferences."];
  const pick = (dict: Record<string, string>) => Object.keys(dict).filter(key => prefixes.some(prefix => key.startsWith(prefix))).sort();
  assert.deepEqual(pick(ko), pick(en));
});
