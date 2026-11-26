// Color tokens and brightness (ADE screen unification U0·U1): both themes meet
// the contrast pairs, colors live only in the two token blocks, every token a
// rule reads is defined, and the brightness choice resolves as designed.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";
import { checkContrast } from "../scripts/m5-contrast-check.mjs";
import { monacoThemeData, parseThemePreference, resolveTheme } from "../src/m5/theme.ts";

const css = readFileSync("src/m5/styles.css", "utf8");
const tokenBlocks = (() => {
  const dark = css.indexOf(":root {");
  const light = css.indexOf(':root[data-theme="light"] {');
  return [[dark, css.indexOf("\n}", dark)], [light, css.indexOf("\n}", light)]] as const;
})();
const outsideTokens = (() => {
  let text = css;
  for (const [start, end] of [...tokenBlocks].sort((a, b) => b[0] - a[0])) text = text.slice(0, start) + text.slice(end);
  return text;
})();

test("both themes meet the contrast pairs (text 4.5:1, non-text 3:1)", () => {
  const result = checkContrast(css);
  for (const [theme, pairs] of Object.entries(result)) {
    const failed = pairs.filter(pair => !pair.pass).map(pair => `${pair.fg} on ${pair.bg} = ${pair.ratio}`);
    assert.deepEqual(failed, [], `${theme} contrast`);
    assert.ok(pairs.length >= 90, `${theme} checks the full pair list`);
  }
});

test("colors live only in the dark and light token blocks", () => {
  assert.ok(tokenBlocks.every(([start, end]) => start >= 0 && end > start));
  assert.deepEqual(outsideTokens.match(/#[0-9a-f]{3,8}\b/gi) ?? [], []);
  assert.deepEqual(outsideTokens.match(/\brgba?\(/gi) ?? [], []);
});

test("every token a rule reads is defined", () => {
  const defined = new Set([...css.matchAll(/(--[\w-]+)\s*:/g)].map(m => m[1]));
  for (const file of readdirSync("src/m5").filter(name => /\.tsx?$/.test(name))) {
    for (const m of readFileSync(`src/m5/${file}`, "utf8").matchAll(/["'](--[\w-]+)["']\s*:/g)) defined.add(m[1]);
  }
  const missing = [...new Set([...css.matchAll(/var\((--[\w-]+)/g)].map(m => m[1]))].filter(name => !defined.has(name));
  assert.deepEqual(missing, []);
});

test("screen code has no literal colors (Monaco reads the tokens)", () => {
  const offenders = readdirSync("src/m5").filter(name => /\.tsx?$/.test(name))
    .filter(name => /["'`]#[0-9a-f]{3,8}["'`]/i.test(readFileSync(`src/m5/${name}`, "utf8")));
  assert.deepEqual(offenders, []);
});

test("brightness choice defaults to the system and resolves to one theme", () => {
  assert.equal(parseThemePreference(null), "system");
  assert.equal(parseThemePreference("sepia"), "system");
  assert.equal(parseThemePreference("light"), "light");
  assert.equal(resolveTheme("system", true), "dark");
  assert.equal(resolveTheme("system", false), "light");
  assert.equal(resolveTheme("dark", false), "dark");
  assert.equal(resolveTheme("light", true), "light");
});

test("Monaco theme comes from the tokens and skips unreadable ones", () => {
  const values: Record<string, string> = { "--bg-app": " #111315", "--text-primary": "#eceeed", "--diff-add": "#5fc48a", "--editor-selection": "var(--x)" };
  const dark = monacoThemeData("dark", token => values[token] ?? "");
  assert.equal(dark.base, "vs-dark");
  assert.equal(dark.colors["editor.background"], "#111315");
  assert.equal(dark.colors["diffEditor.insertedLineBackground"], "#5fc48a1f");
  assert.equal("editor.selectionBackground" in dark.colors, false);
  assert.equal(monacoThemeData("light", () => "").base, "vs");
});
