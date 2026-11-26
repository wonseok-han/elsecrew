// Screen brightness (ADE screen unification U0). main keeps the choice
// (system / light / dark) in `userData/appearance.json`: it is the record the
// settings screen starts from, and the next launch paints its first frame and
// sets `nativeTheme.themeSource` from it. The page keeps a copy in its own
// storage only to paint before main answers (PR #10 review I1).
import { closeSync, fsyncSync, openSync, readFileSync, renameSync, rmSync, writeSync } from "node:fs";
import { join } from "node:path";

export const APPEARANCE_CHANNEL = "m5:appearance:set";
export const APPEARANCE_GET_CHANNEL = "m5:appearance:get";
export const APPEARANCE_FILE = "appearance.json";
export type ThemePreference = "system" | "light" | "dark";

/** Window backgrounds before the page paints: `--bg-app` in styles.css (dark / light). */
export const WINDOW_BACKGROUND = { dark: "#111315", light: "#f3f3f1" } as const;

export function isThemePreference(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

/** The saved choice; `saved` is false when there is no file or it cannot be read as one. */
export function readThemePreference(userData: string): { theme: ThemePreference; saved: boolean } {
  try {
    const saved = JSON.parse(readFileSync(join(userData, APPEARANCE_FILE), "utf8")) as { theme?: unknown };
    if (isThemePreference(saved?.theme)) return { theme: saved.theme, saved: true };
  } catch { /* Missing or damaged: treated as no saved choice. */ }
  return { theme: "system", saved: false };
}

/**
 * Remembers the choice. The new content goes to a file next to the record
 * and replaces it in one rename, so a failed write leaves the previous record
 * whole. Returns false when the choice was not remembered.
 */
export function writeThemePreference(userData: string, theme: ThemePreference): boolean {
  const target = join(userData, APPEARANCE_FILE);
  const temporary = `${target}.tmp`;
  let descriptor: number | undefined;
  try {
    descriptor = openSync(temporary, "w", 0o600);
    writeSync(descriptor, JSON.stringify({ theme }));
    fsyncSync(descriptor);
    closeSync(descriptor);
    descriptor = undefined;
    renameSync(temporary, target);
    return true;
  } catch {
    if (descriptor !== undefined) { try { closeSync(descriptor); } catch { /* already closed */ } }
    try { rmSync(temporary, { force: true }); } catch { /* nothing to clean, or not ours to remove */ }
    return false;
  }
}

export function windowBackground(dark: boolean): string {
  return dark ? WINDOW_BACKGROUND.dark : WINDOW_BACKGROUND.light;
}
