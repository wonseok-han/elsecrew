// Screen brightness (ADE screen unification U0, PR #10 review I1·I2).
//
// main keeps the record (`userData/appearance.json`): the settings screen
// starts from it and the next launch paints its first frame from it. This
// page keeps a copy in its own storage only to paint before main answers.
// Recovery: main's record wins; when main has none it can read and the copy
// is a valid choice, the copy is sent back to main; a damaged copy is never
// sent. `data-theme` on <html> picks the token block in styles.css, and
// Monaco reads its colors from the same tokens.
import { useSyncExternalStore } from "react";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export const THEME_STORAGE_KEY = "elsecrew.theme.v1";
export const THEME_PREFERENCES: readonly ThemePreference[] = ["system", "light", "dark"];

export function isThemePreference(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

export function parseThemePreference(value: unknown): ThemePreference {
  return isThemePreference(value) ? value : "system";
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

export interface ThemeSnapshot {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  /** main could not remember the last choice for the next launch. */
  saveFailed: boolean;
}

/** What the controller touches; the page passes the real ones, tests pass fakes. */
export interface ThemeEnvironment {
  storage: { getItem(key: string): string | null; setItem(key: string, value: string): void } | null;
  systemDark(): boolean;
  onSystemChange(listener: () => void): void;
  setTheme(resolved: ResolvedTheme): void;
  appearance?: {
    get(): Promise<{ theme: ThemePreference; saved: boolean; dark: boolean }>;
    set(theme: ThemePreference): Promise<{ dark: boolean; saved: boolean }>;
  };
}

export function createThemeController(env: ThemeEnvironment) {
  let snapshot: ThemeSnapshot = { preference: "system", resolved: "dark", saveFailed: false };
  let started = false;
  /** Bumped by each choice made here, so an older answer from main never overrides it. */
  let choice = 0;
  const listeners = new Set<() => void>();

  const publish = (next: ThemeSnapshot) => {
    if (next.preference === snapshot.preference && next.resolved === snapshot.resolved && next.saveFailed === snapshot.saveFailed) return;
    snapshot = next;
    for (const listener of [...listeners]) listener();
  };
  const apply = (preference = snapshot.preference, saveFailed = snapshot.saveFailed) => {
    const resolved = resolveTheme(preference, env.systemDark());
    env.setTheme(resolved);
    publish({ preference, resolved, saveFailed });
  };
  const keepCopy = (preference: ThemePreference) => { try { env.storage?.setItem(THEME_STORAGE_KEY, preference); } catch { /* the copy is only for the first paint */ } };
  const send = (preference: ThemePreference, mine: number) => {
    if (!env.appearance) return Promise.resolve();
    return env.appearance.set(preference).then(answer => {
      // main's choice now drives the system signal this page reads.
      if (mine === choice) apply(snapshot.preference, !answer.saved);
    }, () => { if (mine === choice) apply(snapshot.preference, true); });
  };

  return {
    /** Paints from the copy, then takes main's record (or repairs it). Call once before rendering. */
    start(): Promise<void> {
      if (started) return Promise.resolve();
      started = true;
      let copy: string | null = null;
      try { copy = env.storage?.getItem(THEME_STORAGE_KEY) ?? null; } catch { copy = null; }
      const validCopy = isThemePreference(copy) ? copy : null;
      apply(validCopy ?? "system");
      env.onSystemChange(() => apply());
      if (!env.appearance) return Promise.resolve();
      const mine = choice;
      return env.appearance.get().then(record => {
        if (mine !== choice) return;
        if (record.saved) {
          if (record.theme !== validCopy) keepCopy(record.theme);
          apply(record.theme, false);
        } else if (validCopy !== null) {
          void send(validCopy, mine);
        } else {
          apply("system", false);
        }
      }, () => undefined);
    },
    set(next: ThemePreference): Promise<void> {
      const preference = parseThemePreference(next);
      const mine = ++choice;
      keepCopy(preference);
      apply(preference, false);
      return send(preference, mine);
    },
    subscribe(listener: () => void): () => void {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    snapshot(): ThemeSnapshot {
      return snapshot;
    },
  };
}

/* ── The page's controller ─────────────────────────────────────────── */

function pageEnvironment(): ThemeEnvironment {
  const query = () => { try { return window.matchMedia("(prefers-color-scheme: dark)"); } catch { return null; } };
  let storage: ThemeEnvironment["storage"] = null;
  try { storage = window.localStorage; } catch { storage = null; }
  return {
    storage,
    systemDark: () => query()?.matches ?? true,
    onSystemChange: listener => { try { query()?.addEventListener("change", listener); } catch { /* no system signal */ } },
    setTheme: resolved => { document.documentElement.dataset.theme = resolved; },
    appearance: window.m5Appearance,
  };
}

let page: ReturnType<typeof createThemeController> | null = null;
const controller = () => (page ??= createThemeController(pageEnvironment()));

/** Reads the saved choice, applies it and follows system changes. Call once before rendering. */
export function startTheme(): void {
  void controller().start();
}

export function setThemePreference(next: ThemePreference): void {
  void controller().set(next);
}

export function subscribeTheme(listener: () => void): () => void {
  return controller().subscribe(listener);
}

export function currentTheme(): ThemeSnapshot {
  return controller().snapshot();
}

export function useTheme(): ThemeSnapshot {
  return useSyncExternalStore(subscribeTheme, currentTheme, currentTheme);
}

/** Monaco theme data from the live tokens on <html>. */
export function currentMonacoTheme() {
  const style = getComputedStyle(document.documentElement);
  return monacoThemeData(currentTheme().resolved, token => style.getPropertyValue(token));
}
