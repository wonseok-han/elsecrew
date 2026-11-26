/**
 * When each project was last opened on this computer (ADE U2, decision Q1 (나)):
 * the home cards show it and the most recent one leads. A per-viewer
 * convenience in this page's storage; the engine's list stays the source of
 * which projects exist.
 */
interface StorageLike { getItem(key: string): string | null; setItem(key: string, value: string): void }

const KEY = "elsecrew.m5.projectOpenedAt.v1";
const ID = /^[A-Za-z0-9_-]{1,128}$/;
const LIMIT = 200;

export type OpenedAt = Readonly<Record<string, string>>;

export function readProjectOpenedAt(storage: StorageLike | null): OpenedAt {
  try {
    const raw: unknown = JSON.parse(storage?.getItem(KEY) ?? "{}");
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
    return Object.fromEntries(Object.entries(raw as Record<string, unknown>)
      .filter(([id, at]) => ID.test(id) && typeof at === "string" && Number.isFinite(Date.parse(at))) as [string, string][]);
  } catch { return {}; }
}

export function recordProjectOpened(storage: StorageLike | null, projectId: string, at: Date = new Date()): void {
  if (!ID.test(projectId)) return;
  const next = { ...readProjectOpenedAt(storage), [projectId]: at.toISOString() };
  const kept = Object.entries(next).sort((a, b) => Date.parse(b[1]) - Date.parse(a[1])).slice(0, LIMIT);
  try { storage?.setItem(KEY, JSON.stringify(Object.fromEntries(kept))); } catch { /* a convenience only */ }
}

/** Most recently opened first; projects never opened here keep the engine's order after them. */
export function byRecency<T extends { projectId: string }>(entries: readonly T[], openedAt: OpenedAt): T[] {
  return entries.map((entry, index) => ({ entry, index, at: Date.parse(openedAt[entry.projectId] ?? "") }))
    .sort((a, b) => (Number.isFinite(b.at) ? b.at : -Infinity) - (Number.isFinite(a.at) ? a.at : -Infinity) || a.index - b.index)
    .map(item => item.entry);
}

/** "3분 전", "어제", "11월 20일" — how long ago, in the screen's language. */
export function openedAgo(iso: string | undefined, lang: string, now: number = Date.now()): string | null {
  const at = iso ? Date.parse(iso) : NaN;
  if (!Number.isFinite(at)) return null;
  const seconds = Math.round((at - now) / 1000);
  const format = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  if (Math.abs(seconds) < 60) return format.format(0, "second");
  if (Math.abs(seconds) < 3600) return format.format(Math.round(seconds / 60), "minute");
  if (Math.abs(seconds) < 86400) return format.format(Math.round(seconds / 3600), "hour");
  if (Math.abs(seconds) < 7 * 86400) return format.format(Math.round(seconds / 86400), "day");
  return new Intl.DateTimeFormat(lang, { month: "long", day: "numeric" }).format(at);
}

/** The project's name: the last part of its folder path. */
export function projectName(canonicalPath: string, fallback: string): string {
  return canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? fallback;
}

/** A two-letter mark and a role color slot (1–8) from the name, so cards are told apart. */
export function projectMark(name: string): { letters: string; color: number } {
  const letters = Array.from(name.replace(/[^\p{L}\p{N}]/gu, "") || name).slice(0, 1).join("").toUpperCase() || "?";
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.codePointAt(0)!) >>> 0;
  return { letters, color: (hash % 8) + 1 };
}
