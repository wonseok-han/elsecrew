// PR #10 review (REVIEW-pr10.md): each finding is reproduced here.
// I1 brightness record (main's file and the page's copy), I2 system change,
// I3 segmented choice and focus ring, I4 contrast of drawn combinations,
// minor: solid danger only on confirm buttons, OS wording.
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { readThemePreference, writeThemePreference, APPEARANCE_FILE } from "../electron/appearance.ts";
import { checkContrast } from "../scripts/m5-contrast-check.mjs";
import { createThemeController, THEME_STORAGE_KEY, type ThemeEnvironment, type ThemePreference } from "../src/m5/theme.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

function fakePage(options: { copy?: string | null; storageThrows?: boolean; systemDark?: boolean; main?: { theme: ThemePreference; saved: boolean }; setSaves?: boolean; getHangs?: boolean }) {
  const store = new Map<string, string>();
  if (options.copy !== undefined && options.copy !== null) store.set(THEME_STORAGE_KEY, options.copy);
  const sets: ThemePreference[] = [];
  const painted: string[] = [];
  let systemDark = options.systemDark ?? false;
  let systemListener: (() => void) | undefined;
  let releaseGet: (() => void) | undefined;
  const env: ThemeEnvironment = {
    storage: {
      getItem: key => { if (options.storageThrows) throw new Error("storage unavailable"); return store.get(key) ?? null; },
      setItem: (key, value) => { if (options.storageThrows) throw new Error("storage unavailable"); store.set(key, value); },
    },
    systemDark: () => systemDark,
    onSystemChange: listener => { systemListener = listener; },
    setTheme: resolved => { painted.push(resolved); },
    appearance: options.main ? {
      get: () => {
        const answer = { ...options.main!, dark: systemDark };
        return options.getHangs ? new Promise(done => { releaseGet = () => done(answer); }) : Promise.resolve(answer);
      },
      set: async theme => { sets.push(theme); return { dark: systemDark, saved: options.setSaves ?? true }; },
    } : undefined,
  };
  return {
    controller: createThemeController(env), store, sets, painted,
    flipSystem(dark: boolean) { systemDark = dark; systemListener?.(); },
    releaseGet: () => releaseGet?.(),
  };
}

test("I1: main's record wins when the page's copy is missing, and is not overwritten", async () => {
  const page = fakePage({ copy: null, main: { theme: "dark", saved: true } });
  await page.controller.start();
  assert.equal(page.controller.snapshot().preference, "dark");
  assert.equal(page.controller.snapshot().resolved, "dark");
  assert.deepEqual(page.sets, [], "no default is sent over main's record");
  assert.equal(page.store.get(THEME_STORAGE_KEY), "dark", "the copy is refreshed from main");
});

test("I1: a damaged copy is never sent and main's record wins", async () => {
  const page = fakePage({ copy: "sepia", main: { theme: "dark", saved: true } });
  await page.controller.start();
  assert.equal(page.controller.snapshot().preference, "dark");
  assert.deepEqual(page.sets, []);
});

test("I1: storage that cannot be read falls back to main's record", async () => {
  const page = fakePage({ storageThrows: true, main: { theme: "light", saved: true }, systemDark: true });
  await page.controller.start();
  assert.equal(page.controller.snapshot().resolved, "light");
  assert.deepEqual(page.sets, []);
});

test("I1: when main has no readable record, a valid copy repairs it; nothing valid sends nothing", async () => {
  const repair = fakePage({ copy: "light", main: { theme: "system", saved: false } });
  await repair.controller.start();
  await new Promise(done => setImmediate(done));
  assert.deepEqual(repair.sets, ["light"]);
  assert.equal(repair.controller.snapshot().preference, "light");
  for (const copy of [null, "sepia"]) {
    const none = fakePage({ copy, main: { theme: "system", saved: false } });
    await none.controller.start();
    assert.deepEqual(none.sets, []);
    assert.equal(none.controller.snapshot().preference, "system");
  }
});

test("I1: a choice main could not remember is reported, and a later one clears it", async () => {
  const failing = fakePage({ main: { theme: "system", saved: false }, setSaves: false });
  await failing.controller.start();
  await failing.controller.set("dark");
  assert.equal(failing.controller.snapshot().saveFailed, true);
  const page = fakePage({ main: { theme: "system", saved: true } });
  await page.controller.start();
  await page.controller.set("light");
  assert.equal(page.controller.snapshot().saveFailed, false);
  assert.equal(page.store.get(THEME_STORAGE_KEY), "light");
});

test("I1: a choice made while main's record is on its way is not undone by it", async () => {
  const page = fakePage({ main: { theme: "dark", saved: true }, getHangs: true });
  const started = page.controller.start();
  await page.controller.set("light");
  page.releaseGet();
  await started;
  assert.equal(page.controller.snapshot().preference, "light");
});

test("I1: main's record is replaced in one step; a failed write keeps the previous record", () => {
  const dir = mkdtempSync(join(tmpdir(), "elsecrew-appearance-"));
  assert.deepEqual(readThemePreference(dir), { theme: "system", saved: false });
  assert.equal(writeThemePreference(dir, "dark"), true);
  assert.deepEqual(readThemePreference(dir), { theme: "dark", saved: true });
  writeFileSync(join(dir, APPEARANCE_FILE), "{not json");
  assert.deepEqual(readThemePreference(dir), { theme: "system", saved: false });
  writeFileSync(join(dir, APPEARANCE_FILE), JSON.stringify({ theme: "sepia" }));
  assert.deepEqual(readThemePreference(dir), { theme: "system", saved: false });
  assert.equal(writeThemePreference(dir, "light"), true);
  // The staging name is taken (a directory): the write fails and the record stays whole.
  mkdirSync(join(dir, `${APPEARANCE_FILE}.tmp`));
  assert.equal(writeThemePreference(dir, "dark"), false);
  assert.deepEqual(readThemePreference(dir), { theme: "light", saved: true });
  assert.ok(existsSync(join(dir, `${APPEARANCE_FILE}.tmp`)), "a directory that is not ours is not removed");
  assert.equal(writeThemePreference(join(dir, "missing"), "dark"), false);
});

test("I2: with 'system', a system change repaints the page and tells every listener (Monaco)", async () => {
  const page = fakePage({ main: { theme: "system", saved: true }, systemDark: false });
  await page.controller.start();
  let notified = 0;
  page.controller.subscribe(() => { notified++; });
  page.flipSystem(true);
  assert.equal(page.controller.snapshot().resolved, "dark");
  assert.equal(page.painted.at(-1), "dark");
  assert.equal(notified, 1);
  await page.controller.set("light");
  page.flipSystem(false);
  page.flipSystem(true);
  assert.equal(page.controller.snapshot().resolved, "light", "an explicit choice ignores the system");
});

const css = readFileSync("src/m5/styles.css", "utf8");
const rule = (selector: string) => {
  const at = css.indexOf(`${selector} {`);
  assert.ok(at >= 0, `missing ${selector}`);
  return css.slice(at, css.indexOf("}", at));
};

test("I3: the chosen segment carries an edge and the focus ring is drawn inside, unclipped", () => {
  assert.doesNotMatch(rule(".m5-segmented"), /overflow:\s*hidden/);
  assert.match(rule('.m5-segmented button[aria-pressed="true"]'), /inset 0 0 0 1px var\(--selected-edge\)/);
  assert.match(rule('.m5-seg > button[aria-pressed="true"]'), /var\(--selected-edge\)/);
  assert.match(rule('.m5-preview-viewport button[aria-pressed="true"]'), /var\(--selected-edge\)/);
  assert.match(rule(":is(.m5-segmented, .m5-seg, .m5-preview-viewport) button:focus-visible"), /outline-offset:\s*-2px/);
  const result = checkContrast(css);
  for (const theme of ["dark", "light"] as const) {
    for (const label of ["segmented: chosen edge on chosen face", "segmented: chosen edge on track", "segmented: focus ring inside chosen", "seg / preview viewport: chosen edge"]) {
      const found = result[theme].find(entry => entry.label === label);
      assert.ok(found?.pass, `${theme} ${label}`);
    }
  }
});

test("I4: drawn combinations pass in both themes, compared before rounding", () => {
  const result = checkContrast(css);
  for (const theme of ["dark", "light"] as const) {
    const failed = result[theme].filter(entry => !entry.pass).map(entry => `${entry.label ?? `${entry.fg} on ${entry.bg}`} = ${entry.ratio}`);
    assert.deepEqual(failed, [], theme);
    for (const label of ["button danger confirm, hover", "first-run current step", "input edge on pane", "input edge on app face", "role icon 1 on --bg-selected", "role icon 7 on --bg-hover"]) {
      assert.ok(result[theme].some(entry => entry.label === label), `${theme} checks ${label}`);
    }
  }
  // No filter is used to make a hover face: the hover is a token the check can read.
  assert.doesNotMatch(css, /filter:\s*brightness/);
  assert.match(rule(".m5-setup-navigation button[aria-current=\"step\"]"), /color:\s*var\(--text-primary\)/);
  assert.match(css, /--role-color, var\(--text-tertiary\)\) var\(--role-tint\), transparent\)/);
});

test("I4: every input, textarea and select rule draws its edge with --line-input (3:1)", () => {
  // A more specific old rule once outranked the shared input edge (CI, light, Claude connection code).
  const offenders: string[] = [];
  for (const match of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const selector = match[1].trim();
    const last = selector.split(",").map(part => part.trim());
    if (!last.some(part => /(^|[\s>+~])(input|textarea|select)(?![\w-])(?!\[type="(checkbox|radio)"\])/.test(part) && !/type="(checkbox|radio|range)"/.test(part))) continue;
    for (const decl of match[2].matchAll(/border(?:-color)?\s*:\s*([^;]+)/g)) {
      if (/var\(--(line-strong|line-default|line-subtle|m5-border|m5-border-strong)\)/.test(decl[1])) offenders.push(selector.slice(0, 80));
    }
  }
  assert.deepEqual(offenders, []);
});

test("minor: solid danger only on the confirm buttons that ask for it, not every dialog button", () => {
  assert.doesNotMatch(css, /dialog \.m5-danger/);
  assert.doesNotMatch(css, /\.m5-discard-dialog \.m5-danger/);
  for (const [file, marker] of [["workbench.tsx", "m5-danger m5-btn-danger-solid\" disabled={!canRemove}"], ["chat-erasure.tsx", "m5-danger m5-btn-danger-solid"], ["workspace-cleanup.tsx", "m5-danger m5-btn-danger-solid"], ["chat-workspace-discard.tsx", "m5-btn-danger-solid"]]) {
    assert.ok(readFileSync(`src/m5/${file}`, "utf8").includes(marker), file);
  }
  assert.ok(!readFileSync("src/m5/role-settings.tsx", "utf8").includes("m5-btn-danger-solid"), "the role menu's delete stays a menu item");
});

test("minor: the brightness help names the operating system, not macOS", () => {
  assert.doesNotMatch(ko["m5.preferences.themeHelp"], /macOS/);
  assert.doesNotMatch(en["m5.preferences.themeHelp"], /macOS/);
  assert.ok(ko["m5.preferences.themeSaveFailed"] && en["m5.preferences.themeSaveFailed"]);
});
