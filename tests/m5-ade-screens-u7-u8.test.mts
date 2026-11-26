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

/** Every TS/TSX file under a folder, its comments removed; paths with "/" on every system (Windows CI). */
function codeFiles(dir: string): { path: string; text: string }[] {
  const found: { path: string; text: string }[] = [];
  const walk = (at: string) => { for (const name of readdirSync(at)) { const path = join(at, name); if (statSync(path).isDirectory()) walk(path); else if (/\.tsx?$/.test(name)) found.push({ path: path.split("\\").join("/"), text: readFileSync(path, "utf8").replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`\\])\/\/[^\n]*/g, "$1") }); } };
  walk(dir);
  return found;
}
const FORBIDDEN_EN = /\bcommits?\b|\bbranch(es)?\b|worktree|snapshot|recovery point|identifier/i;

test("U7: no on-screen string uses developer words outside the details (rule 4) — both languages, code text and main's boxes", () => {
  const allowed = (key: string) => (DETAILS_ONLY as readonly string[]).includes(key);
  assert.deepEqual(Object.entries(ko).filter(([key, value]) => FORBIDDEN.test(value) && !allowed(key)).map(([key]) => key), []);
  // English too (PR #16 review I3), and the app's message to the team that the person also sees.
  assert.deepEqual(Object.entries(en).filter(([key, value]) => FORBIDDEN_EN.test(value) && !allowed(key)).map(([key]) => key), []);
  // Korean words written straight into screen code (JSX text, strings) and every main/preload file, folders included.
  const hangul = (text: string) => [...text.matchAll(/[^\n]*(커밋|브랜치|복구점|식별자|준비 기록|워크트리|스냅샷)[^\n]*/g)].map(match => match[0].trim().slice(0, 80));
  for (const { path, text } of [...codeFiles("src"), ...codeFiles("electron")].filter(file => !file.path.includes("locales"))) assert.deepEqual(hangul(text), [], path);
  // A raw failure code is help information: folded in the run details, not beside the sentence.
  const story = source("task-story.tsx");
  assert.ok(story.indexOf("{run.failure.stage}/{run.failure.code}") > story.indexOf("<details><summary>{t(\"m5.run.details\")}</summary>"), "the failure code sits in the details");
});

test("U7: details-only strings are used only inside details (any quote, any folder)", () => {
  const files = codeFiles("src/m5").filter(file => file.path.endsWith(".tsx"));
  for (const key of DETAILS_ONLY) {
    const uses: string[] = [];
    for (const { text } of files) for (const match of text.matchAll(new RegExp(`["'\`]${key.replace(/\./g, "\\.")}["'\`]`, "g"))) uses.push(text.slice(0, match.index));
    assert.ok(uses.length > 0, `${key} is used`);
    for (const before of uses) assert.ok(before.lastIndexOf("<details") > before.lastIndexOf("</details>"), `${key} sits inside a <details>`);
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
  const api = new Set(["m5-btn-secondary", "m5-refresh", "m5-badge-review", "m5-badge-danger", "m5-banner-running", "m5-banner-done", "m5-md-h1", "m5-md-h2", "m5-message", "m5-agent-output",
    // Left by the work-history detail (PR #11) in the run output area another worker is changing now; they tidy it there.
    "m5-activity-original", "m5-activity-code"]);
  // This checks that each styled class still appears in the code, not that every rule's whole selector matches a live
  // screen (PR #16 review M2): the renderer smoke compares computed styles of the main components for that.
  const dead = [...classes].filter(name => !api.has(name) && !new RegExp(`(?<![\\w-])${name}(?![\\w-])`).test(code) && !dynamic.some(prefix => name.startsWith(prefix)));
  assert.deepEqual(dead, []);
});

test("U8: no color literal in any TS/TSX of src and electron; main's native colors are the tokens' values (PR #16 review M3)", () => {
  const literals: string[] = [];
  for (const { path, text } of [...codeFiles("src"), ...codeFiles("electron")]) {
    // Inside any string (not only a whole one), and six- or eight-digit hex anywhere else (JSX text).
    for (const match of text.matchAll(/(["'`])((?:(?!\1)[^\n\\]|\\.)*?)\1/g)) for (const hex of match[2].matchAll(/#[0-9a-f]{3}(?:[0-9a-f]{3})?(?:[0-9a-f]{2})?(?![0-9a-z_-])/gi)) literals.push(`${path}:${hex[0].toLowerCase()}`);
    for (const hex of text.replace(/(["'`])(?:(?!\1)[^\n\\]|\\.)*?\1/g, "").matchAll(/#[0-9a-f]{6}(?:[0-9a-f]{2})?\b/gi)) literals.push(`${path}:${hex[0].toLowerCase()}`);
  }
  // The window background before the page paints (it cannot read CSS), and a preview page's default white.
  assert.deepEqual(literals.sort(), ["electron/appearance.ts:#111315", "electron/appearance.ts:#f3f3f1", "electron/preview-manager.ts:#ffffff"].sort());
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
  for (const state of ["unavailable", "idle"] as const) assert.equal(updateBannerKind(state), null);
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

test("PR #16 I2: closing the banner hides only what shows now; a later state (a new failure) shows again", () => {
  const banner = source("update-banner.tsx");
  assert.match(banner, /useEffect\(\(\) => \{ setDismissed\(false\); \}, \[state\]\);/);
  assert.match(banner, /setBusy\(true\); setDismissed\(false\);/);
  assert.match(banner, /api\.onState\(value => \{ setState\(value\); setDismissed\(false\); \}\)/, "even a repeated state from main shows again");
  assert.doesNotMatch(banner, /dismissed === state/);
});

test("PR #16 I4: the menu and the settings check answer in the app; only the restart keeps main's native box", () => {
  const menu = readFileSync("electron/app-update-menu.ts", "utf8");
  assert.equal((menu.match(/dialog\.showMessageBox\(/g) ?? []).length, 1, "one native box: the restart confirmation");
  assert.match(menu, /controller\.check\(\);\n\s*reveal\(\);/);
  assert.match(source("app-settings.tsx"), /api\.request\("check"\)/);
  assert.equal(updateBannerKind("checking"), "checking");
  assert.equal(updateBannerKind("current"), "current");
  assert.match(readFileSync("electron/preload.ts", "utf8"), /onReveal: \(listener: \(\) => void\) =>/);
});

test("PR #16 M1: the discard dialog can be left while it inspects, and focus comes back to Cancel afterwards", () => {
  const discard = source("chat-workspace-discard.tsx");
  assert.match(discard, /if \(pending\.current && !inspecting\.current\) return;/);
  assert.match(discard, /data-first-focus disabled=\{busy && inspection !== null\}/);
  assert.match(discard, /element\.querySelector<HTMLButtonElement>\("\[data-first-focus\]"\)\?\.focus\(\);\n  \}, \[busy\]\);/);
});
