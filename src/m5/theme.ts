// Screen brightness (ADE screen unification U0). The choice lives in this
// page's localStorage and defaults to following the system. `data-theme` on
// <html> picks the token block in styles.css; main is told so native parts
// (window background, dialogs) follow, and Monaco reads its colors from the
// same tokens.
import { useSyncExternalStore } from "react";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export const THEME_STORAGE_KEY = "elsecrew.theme.v1";
export const THEME_PREFERENCES: readonly ThemePreference[] = ["system", "light", "dark"];

export function parseThemePreference(value: unknown): ThemePreference {
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}

export function resolveTheme(preference: ThemePreference, systemDark: boolean): ResolvedTheme {
  return preference === "system" ? (systemDark ? "dark" : "light") : preference;
}

/** Monaco wants concrete colors: these come from the semantic tokens. */
export const MONACO_TOKENS = {
  "editor.background": "--bg-app",
  "editor.foreground": "--text-primary",
  "editorGutter.background": "--bg-app",
  "editor.lineHighlightBackground": "--bg-pane",
  "editorLineNumber.foreground": "--editor-line-number",
  "editorLineNumber.activeForeground": "--editor-line-number-active",
  "editor.selectionBackground": "--editor-selection",
  "editor.inactiveSelectionBackground": "--editor-selection-inactive",
} as const;

const HEX = /^#[0-9a-f]{6}$/i;

/** Colors for `monaco.editor.defineTheme` from a token reader; unreadable tokens are left to Monaco's base. */
export function monacoThemeData(resolved: ResolvedTheme, read: (token: string) => string) {
  const colors: Record<string, string> = {};
  for (const [key, token] of Object.entries(MONACO_TOKENS)) {
    const value = read(token).trim();
    if (HEX.test(value)) colors[key] = value;
  }
  // Changed lines: the diff tokens, faint behind the line and a little stronger behind the text.
  for (const [prefix, token] of [["inserted", "--diff-add"], ["removed", "--diff-remove"]] as const) {
    const value = read(token).trim();
    if (!HEX.test(value)) continue;
    colors[`diffEditor.${prefix}LineBackground`] = `${value}1f`;
    colors[`diffEditor.${prefix}TextBackground`] = `${value}38`;
  }
  return { base: resolved === "dark" ? "vs-dark" as const : "vs" as const, inherit: true, rules: [], colors };
}

/* ── Page glue ─────────────────────────────────────────────────────── */

let preference: ThemePreference = "system";
let resolved: ResolvedTheme = "dark";
let snapshot: { preference: ThemePreference; resolved: ResolvedTheme } = { preference, resolved };
let started = false;
const listeners = new Set<() => void>();

function systemDark(): boolean {
  try { return window.matchMedia("(prefers-color-scheme: dark)").matches; } catch { return true; }
}

function apply(): void {
  resolved = resolveTheme(preference, systemDark());
  document.documentElement.dataset.theme = resolved;
  if (snapshot.preference !== preference || snapshot.resolved !== resolved) {
    snapshot = { preference, resolved };
    for (const listener of [...listeners]) listener();
  }
}

function tellMain(): void {
  try { void window.m5Appearance?.set(preference).then(apply, () => undefined); } catch { /* Native parts keep their last choice. */ }
}

/** Reads the saved choice, applies it and follows system changes. Call once before rendering. */
export function startTheme(): void {
  if (started) return;
  started = true;
  let saved: string | null = null;
  try { saved = localStorage.getItem(THEME_STORAGE_KEY); } catch { /* Treated as no saved choice. */ }
  preference = parseThemePreference(saved);
  apply();
  try { window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", apply); } catch { /* No system signal: keep the choice. */ }
  // Without a saved choice here, main's remembered one already drives the
  // system signal this page reads; sending the default would overwrite it.
  if (saved !== null) tellMain();
}

export function setThemePreference(next: ThemePreference): void {
  preference = parseThemePreference(next);
  try { localStorage.setItem(THEME_STORAGE_KEY, preference); } catch { /* Applies for this session only. */ }
  apply();
  tellMain();
}

export function subscribeTheme(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function currentTheme(): { preference: ThemePreference; resolved: ResolvedTheme } {
  return snapshot;
}

export function useTheme(): { preference: ThemePreference; resolved: ResolvedTheme } {
  return useSyncExternalStore(subscribeTheme, currentTheme, currentTheme);
}

/** Monaco theme data from the live tokens on <html>. */
export function currentMonacoTheme() {
  const style = getComputedStyle(document.documentElement);
  return monacoThemeData(snapshot.resolved, token => style.getPropertyValue(token));
}
