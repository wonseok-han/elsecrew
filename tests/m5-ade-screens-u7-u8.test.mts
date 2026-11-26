// ADE screen unification U7 (errors, recovery, confirmations and empty states
// in plain words and one frame) and U8 (dead styles gone, colors only in the
// tokens, the app update as an in-app banner that main decides).
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";
import { updateBannerKind } from "../src/m5/update-banner-kind.ts";

const source = (name: string) => readFileSync(`src/m5/${name}`, "utf8");
const css = readFileSync("src/m5/styles.css", "utf8");

/** Developer words that stay out of the screen (DESIGN 2.1 rule 4). */
const FORBIDDEN = /커밋|브랜치|복구점|식별자|준비 기록|워크트리|worktree|스냅샷/i;
/**
 * Strings shown only inside "자세히"/help details, where the technical words
 * are the point. Each one is checked to be used inside a <details>.
 */
const DETAILS_ONLY = ["m5.merge.branch", "m5.merge.ahead", "m5.newTask.worktreeDetail"] as const;

test("U7: no on-screen Korean string uses developer words outside the details (rule 4)", () => {
  const offenders = Object.entries(ko).filter(([key, value]) => FORBIDDEN.test(value) && !(DETAILS_ONLY as readonly string[]).includes(key)).map(([key, value]) => `${key}: ${value}`);
  assert.deepEqual(offenders, []);
  // Native boxes main shows are screen strings too.
  for (const file of readdirSync("electron").filter(name => name.endsWith(".ts"))) {
    assert.doesNotMatch(readFileSync(join("electron", file), "utf8"), /커밋|브랜치|복구점|식별자|준비 기록/, file);
  }
});

test("U7: details-only strings are used only inside details", () => {
  const files = readdirSync("src/m5").filter(name => /\.tsx$/.test(name)).map(name => source(name));
  for (const key of DETAILS_ONLY) {
    const uses = files.flatMap(text => {
      const found: string[] = [];
      for (let at = text.indexOf(`"${key}"`); at >= 0; at = text.indexOf(`"${key}"`, at + 1)) found.push(text.slice(0, at));
      return found;
    });
    assert.ok(uses.length > 0, `${key} is used`);
    for (const before of uses) {
      const open = before.lastIndexOf("<details"), close = before.lastIndexOf("</details>");
      assert.ok(open > close, `${key} sits inside a <details>`);
    }
  }
});

test("U7: the stuck-work status in the bell is the F11 guide, its run list only in the help details", () => {
  const status = source("work-status-view.tsx");
  assert.match(status, /<RecoveryGuide tone="attention"/);
  assert.match(status, /details=\{details\}/);
  assert.doesNotMatch(status, /<code>\{id\}<\/code>/);
  assert.doesNotMatch(ko["m5.work.error"], /판단하지 마세요/, "plain words, not an internal warning");
});

test("U7: empty states share one frame (mark · what · why · one action) — rule 7", () => {
  const empty = source("empty-state.tsx");
  assert.match(empty, /className="m5-empty-mark"/);
  assert.match(source("changes-view.tsx"), /<EmptyState icon="file" title=\{t\("m5\.editor\.noChanges"\)\} help=\{t\("m5\.editor\.noChangesHelp"\)\} \/>/);
  assert.match(source("history-tab.tsx"), /<EmptyState icon="clock"/);
  const workbench = source("workbench.tsx");
  assert.match(workbench, /<EmptyState className="m5-first-chat"/);
  assert.equal((workbench.match(/m5-btn-primary m5-first-chat-write/g) ?? []).length, 1, "one main action in the empty project");
  assert.match(source("task-list.tsx"), /className="m5-task-list-empty"/);
  for (const dict of [ko, en]) for (const key of ["m5.editor.noChangesHelp", "m5.diff.noMatchesHelp", "m5.history.emptyHelp", "m5.taskList.empty", "m5.firstChat.write"] as const) assert.ok(dict[key], key);
});

test("U8: every class the stylesheet styles is used by the app (no dead rules)", () => {
  const files: string[] = [];
  const walk = (dir: string) => { for (const name of readdirSync(dir)) { const path = join(dir, name); if (statSync(path).isDirectory()) walk(path); else if (/\.(tsx?|cjs|mjs|html)$/.test(name)) files.push(path); } };
  walk("src"); walk("electron");
  const code = files.map(path => readFileSync(path, "utf8")).join("\n");
  const classes = new Set([...css.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/\.(m5-[a-zA-Z0-9_-]+)/g)].map(match => match[1]));
  const dynamic = [...code.matchAll(/(m5-[a-zA-Z0-9-]*-)\$\{/g)].map(match => match[1]);
  // The shared component layer (DESIGN 2.3) is styled ahead of use; markdown headings are built by level.
  const api = new Set(["m5-btn-secondary", "m5-refresh", "m5-badge-review", "m5-badge-danger", "m5-banner-running", "m5-banner-done", "m5-md-h1", "m5-md-h2", "m5-message", "m5-agent-output"]);
  const dead = [...classes].filter(name => !api.has(name) && !new RegExp(`(?<![\\w-])${name}(?![\\w-])`).test(code) && !dynamic.some(prefix => name.startsWith(prefix)));
  assert.deepEqual(dead, []);
});

test("U8: no color literal in screen code; main's two native colors are the tokens' values", () => {
  for (const name of readdirSync("src/m5").filter(file => /\.tsx?$/.test(file))) assert.doesNotMatch(source(name), /["'`]#[0-9a-f]{3,8}["'`]/i, name);
  const literals = readdirSync("electron").filter(name => name.endsWith(".ts"))
    .flatMap(name => [...readFileSync(join("electron", name), "utf8").matchAll(/["'`](#[0-9a-f]{3,8})["'`]/gi)].map(match => `${name}:${match[1].toLowerCase()}`));
  // The window background before the page paints (it cannot read CSS), and a preview page's default white.
  assert.deepEqual(literals.sort(), ["appearance.ts:#111315", "appearance.ts:#f3f3f1", "preview-manager.ts:#ffffff"].sort());
  const dark = css.slice(css.indexOf(":root {"), css.indexOf("\n}", css.indexOf(":root {")));
  const light = css.slice(css.indexOf(':root[data-theme="light"] {'));
  assert.match(dark, /--p-gray-950: #111315;/);
  assert.match(light.slice(0, light.indexOf("\n}")), /--p-gray-950: #f3f3f1;/);
  assert.match(dark, /--bg-app: var\(--p-gray-950\);/, "the window background is the app background");
});

test("U8: the app update is an in-app banner that follows main; the page only asks", () => {
  assert.equal(updateBannerKind("ready"), "ready");
  assert.equal(updateBannerKind("downloading"), "downloading");
  assert.equal(updateBannerKind("error"), "error");
  for (const state of ["unavailable", "idle", "current", "checking"] as const) assert.equal(updateBannerKind(state), null);
  const banner = source("update-banner.tsx");
  assert.match(banner, /api\.request!\(action\)/);
  assert.doesNotMatch(banner, /quitAndInstall|onRestartCheck/);
  assert.match(banner, /disabled=\{busy \|\| teamWorking\}/, "no restart while the team works");
  const main = readFileSync("electron/main.ts", "utf8");
  const handler = main.slice(main.indexOf(`ipcMain.handle("m5:app-update:request"`), main.indexOf(`ipcMain.handle("m5:app-update:open"`));
  assert.ok(handler.indexOf("dialog.showMessageBox") > 0 && handler.indexOf("dialog.showMessageBox") < handler.indexOf("appUpdate.restart()"), "main's own box comes before any restart");
  assert.doesNotMatch(readFileSync("electron/app-update-menu.ts", "utf8"), /저장했어요|Edits saved/);
  for (const dict of [ko, en]) for (const kind of ["downloading", "ready", "restarting", "error"]) assert.ok(dict[`m5.update.${kind}.title` as keyof typeof ko], kind);
});

test("U7·U8: ko and en carry the same new keys", () => {
  const prefixes = ["m5.update.", "m5.history.", "m5.firstChat.", "m5.taskList.", "m5.editor.noChanges", "m5.diff.noMatches"];
  const pick = (dict: Record<string, string>) => Object.keys(dict).filter(key => prefixes.some(prefix => key.startsWith(prefix))).sort();
  assert.deepEqual(pick(ko), pick(en));
});
