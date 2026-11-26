// Color token contrast check (ADE screen unification U0): reads the dark
// `:root` block and the `:root[data-theme="light"]` block of src/m5/styles.css
// and checks text pairs at 4.5:1 and non-text pairs (action face, input edge,
// state dots, focus ring, changed-line colors) at 3:1 (WCAG 1.4.3 / 1.4.11).
//
//   node scripts/m5-contrast-check.mjs [styles.css] [--json out.json]
//
// Also checks component recipes (colors as drawn, including color-mix).
// Exits 1 when a pair fails. `tests/m5-style-tokens.test.mts` runs it in `npm test`.
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

function block(css, selector) {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`missing ${selector} block`);
  return css.slice(start, css.indexOf("\n}", start));
}
const parse = text => Object.fromEntries([...text.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map(m => [m[1], m[2].trim()]));

export function themeTokens(css) {
  const dark = parse(block(css, ":root"));
  const light = { ...dark, ...parse(block(css, ':root[data-theme="light"]')) };
  return { dark, light };
}

function resolveVar(vars, value, depth = 0) {
  const m = /^var\((--[\w-]+)\)$/.exec(value ?? "");
  return m && depth < 8 ? resolveVar(vars, vars[m[1]], depth + 1) : value;
}
const rgb = hex => { const h = hex.replace("#", ""); return [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16) / 255); };
const lum = hex => { const [r, g, b] = rgb(hex).map(c => c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
export const contrast = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
/** `color-mix(in srgb, fg p%, bg)` (and `fg p%, transparent` drawn over bg): channel-wise in gamma sRGB. */
export function mixHex(fg, percent, bg) {
  const a = rgb(fg), b = rgb(bg), p = percent / 100;
  return "#" + a.map((c, i) => Math.round((c * p + b[i] * (1 - p)) * 255).toString(16).padStart(2, "0")).join("");
}

export function contrastPairs() {
  const surfaces = ["--bg-app", "--bg-pane", "--bg-raised", "--bg-sunken", "--bg-hover", "--bg-selected"];
  const pairs = [];
  for (const fg of ["--text-primary", "--text-secondary", "--text-tertiary", "--link", "--status-danger-text"]) for (const bg of surfaces) pairs.push([fg, bg, 4.5]);
  pairs.push(["--action-text", "--action-bg", 4.5], ["--action-text", "--action-bg-hover", 4.5], ["--action-text", "--status-danger", 4.5], ["--action-text", "--status-attention", 4.5]);
  pairs.push(["--bg-raised", "--status-running", 3], ["--bg-raised", "--status-done", 3]);
  for (const s of ["running", "attention", "review", "done", "danger"]) {
    pairs.push(["--text-primary", `--status-${s}-bg`, 4.5]);
    pairs.push([`--status-${s}-text`, `--status-${s}-bg`, 4.5], [`--status-${s}-text`, "--bg-pane", 4.5], [`--status-${s}-text`, "--bg-raised", 4.5]);
    pairs.push([`--status-${s}`, "--bg-pane", 3], [`--status-${s}`, "--bg-raised", 3]);
  }
  pairs.push(["--status-neutral", "--bg-pane", 3]);
  for (let i = 1; i <= 8; i++) pairs.push([`--role-${i}`, "--bg-pane", 4.5], [`--role-${i}`, "--bg-raised", 4.5]);
  pairs.push(["--diff-add", "--bg-sunken", 3], ["--diff-remove", "--bg-sunken", 3]);
  pairs.push(["--action-bg", "--bg-pane", 3], ["--action-bg", "--bg-raised", 3], ["--action-bg", "--bg-app", 3]);
  pairs.push(["--focus-ring", "--bg-pane", 3], ["--focus-ring", "--bg-raised", 3], ["--line-input", "--bg-input", 3], ["--line-input", "--bg-raised", 3]);
  pairs.push(["--editor-line-number", "--bg-app", 4.5], ["--editor-line-number-active", "--bg-pane", 4.5], ["--text-primary", "--editor-selection", 4.5]);
  return pairs;
}

/**
 * Combinations components draw (PR #10 review I4): a recipe names its colors as
 * tokens or `mix(token p bgToken)`; checked in both themes like the pairs.
 */
export function componentRecipes() {
  const recipes = [
    ["button primary, hover", "--action-text", "--action-bg-hover", 4.5],
    ["button danger confirm", "--action-text", "--status-danger", 4.5],
    ["button danger confirm, hover", "--action-text", "--danger-bg-hover", 4.5],
    ["button secondary", "--text-primary", "--bg-raised", 4.5],
    ["button secondary, hover", "--text-primary", "--bg-hover", 4.5],
    ["button quiet / link button", "--text-secondary", "--bg-pane", 4.5],
    ["segmented: chosen text", "--text-primary", "--bg-raised", 4.5],
    ["segmented: other text on track", "--text-secondary", "--bg-track", 4.5],
    ["segmented: chosen edge on chosen face", "--selected-edge", "--bg-raised", 3],
    ["segmented: chosen edge on track", "--selected-edge", "--bg-track", 3],
    ["segmented: focus ring inside chosen", "--focus-ring", "--bg-raised", 3],
    ["segmented: focus ring on track", "--focus-ring", "--bg-track", 3],
    ["seg / preview viewport: chosen edge", "--selected-edge", "--bg-selected", 3],
    ["seg / preview viewport: chosen edge on pane", "--selected-edge", "--bg-pane", 3],
    ["seg / preview viewport: chosen text", "--text-primary", "--bg-selected", 4.5],
    ["first-run current step", "--text-primary", "--bg-selected", 4.5],
    ["first-run current step edge", "--action-bg", "--bg-selected", 3],
    ["input edge on pane", "--line-input", "--bg-pane", 3],
    ["input edge on app face", "--line-input", "--bg-app", 3],
    ["input edge on input face", "--line-input", "--bg-input", 3],
    ["badge running", "--status-running-text", "--status-running-bg", 4.5],
    ["toast icon on raised", "--status-attention-text", "--bg-raised", 3],
    ["notification count", "--action-text", "--status-attention", 4.5],
    ["link on raised", "--link", "--bg-raised", 4.5],
  ];
  // .m5-role-icon: the role's letter in the role color on the role color mixed by --role-tint.
  for (let i = 1; i <= 8; i++) for (const bg of ["--bg-raised", "--bg-pane", "--bg-app", "--bg-selected", "--bg-hover"]) {
    recipes.push([`role icon ${i} on ${bg}`, `--role-${i}`, `mix(--role-${i} --role-tint ${bg})`, 4.5]);
  }
  return recipes;
}

function colorOf(vars, name) {
  const m = /^mix\((--[\w-]+) (--[\w-]+|\d+) (--[\w-]+)\)$/.exec(name);
  if (m) {
    const fg = resolveVar(vars, vars[m[1]]), bg = resolveVar(vars, vars[m[3]]);
    const percent = Number(String(m[2].startsWith("--") ? resolveVar(vars, vars[m[2]]) : m[2]).replace("%", ""));
    return /^#[0-9a-f]{6}$/i.test(fg ?? "") && /^#[0-9a-f]{6}$/i.test(bg ?? "") && percent >= 0 && percent <= 100 ? mixHex(fg, percent, bg) : undefined;
  }
  return resolveVar(vars, vars[name]);
}

function measure(vars, fg, bg, min, label) {
  const a = colorOf(vars, fg), b = colorOf(vars, bg);
  if (!/^#[0-9a-f]{6}$/i.test(a ?? "") || !/^#[0-9a-f]{6}$/i.test(b ?? "")) return { label, fg, bg, fgValue: a, bgValue: b, ratio: 0, min, pass: false };
  const exact = contrast(a, b);
  // Compare the exact ratio; only the reported number is rounded (review I4).
  return { label, fg, bg, fgValue: a, bgValue: b, ratio: Math.floor(exact * 100) / 100, min, pass: exact >= min };
}

export function checkContrast(css) {
  const themes = themeTokens(css);
  const result = {};
  for (const [name, vars] of Object.entries(themes)) {
    result[name] = [
      ...contrastPairs().map(([fg, bg, min]) => measure(vars, fg, bg, min, undefined)),
      ...componentRecipes().map(([label, fg, bg, min]) => measure(vars, fg, bg, min, label)),
    ];
  }
  return result;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const jsonAt = args.indexOf("--json");
  const out = jsonAt >= 0 ? args.splice(jsonAt, 2)[1] : null;
  const file = args[0] ?? resolve(fileURLToPath(new URL("..", import.meta.url)), "src/m5/styles.css");
  const result = checkContrast(readFileSync(file, "utf8"));
  let failed = 0;
  for (const [theme, pairs] of Object.entries(result)) for (const p of pairs.filter(p => !p.pass)) { failed++; console.log(`FAIL ${theme} ${p.label ? p.label + ": " : ""}${p.fg} on ${p.bg}: ${p.ratio} < ${p.min} (${p.fgValue} / ${p.bgValue})`); }
  const summary = Object.fromEntries(Object.entries(result).map(([k, v]) => [k, { pairs: v.length, failed: v.filter(p => !p.pass).length, lowest: Math.min(...v.map(p => p.ratio)) }]));
  if (out) writeFileSync(out, JSON.stringify({ summary, result }, null, 1));
  console.log(JSON.stringify(summary));
  process.exit(failed ? 1 : 0);
}
