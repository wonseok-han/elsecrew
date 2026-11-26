// Screen brightness (ADE screen unification U0): the renderer keeps the
// choice (system / light / dark) and tells main, which follows it with
// `nativeTheme.themeSource` and the window background, and remembers it so
// the next launch paints the right background before the page loads.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const APPEARANCE_CHANNEL = "m5:appearance:set";
export const APPEARANCE_FILE = "appearance.json";
export type ThemePreference = "system" | "light" | "dark";

/** Window backgrounds before the page paints: `--bg-app` in styles.css (dark / light). */
export const WINDOW_BACKGROUND = { dark: "#111315", light: "#f3f3f1" } as const;

export function isThemePreference(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

/** The saved choice, or "system" when there is none or it cannot be read. */
export function readThemePreference(userData: string): ThemePreference {
  try {
    const saved = JSON.parse(readFileSync(join(userData, APPEARANCE_FILE), "utf8")) as { theme?: unknown };
    return isThemePreference(saved?.theme) ? saved.theme : "system";
  } catch {
    return "system";
  }
}

/** Remembers the choice; a failure only costs the next launch's first paint. */
export function writeThemePreference(userData: string, theme: ThemePreference): boolean {
  try {
    writeFileSync(join(userData, APPEARANCE_FILE), JSON.stringify({ theme }));
    return true;
  } catch {
    return false;
  }
}

export function windowBackground(dark: boolean): string {
  return dark ? WINDOW_BACKGROUND.dark : WINDOW_BACKGROUND.light;
}
