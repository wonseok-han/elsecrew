/**
 * Picking up where the person left off: the project open when the app last
 * closed opens again (its last task is reopened by the project itself). Only
 * the choice of project is remembered; opening it goes through the same
 * engine checks as a click, and a project the person closed stays closed.
 */
interface StorageLike { getItem(key: string): string | null; setItem(key: string, value: string): void; removeItem(key: string): void }

const KEY = "elsecrew.m5.lastProject.v1";
const ID = /^[A-Za-z0-9_-]{1,128}$/;

export function rememberLastProject(storage: StorageLike | null, projectId: string): void {
  if (!ID.test(projectId)) return;
  try { storage?.setItem(KEY, projectId); } catch { /* a convenience only */ }
}

/** The person closed the project: next start shows the home screen. */
export function forgetLastProject(storage: StorageLike | null): void {
  try { storage?.removeItem(KEY); } catch { /* a convenience only */ }
}

export function recallLastProject(storage: StorageLike | null): string | null {
  try {
    const value = storage?.getItem(KEY) ?? null;
    return value && ID.test(value) ? value : null;
  } catch { return null; }
}

/**
 * Whether to reopen the remembered project now: once per start, only when the
 * app is ready, nothing is open or being opened, and the project is still in
 * the person's list.
 */
export function shouldResumeProject(input: {
  remembered: string | null; attempted: boolean; ready: boolean; projectOpen: boolean; busy: boolean;
  known: readonly string[] | null;
}): string | null {
  const { remembered, attempted, ready, projectOpen, busy, known } = input;
  if (!remembered || attempted || !ready || projectOpen || busy || !known) return null;
  return known.includes(remembered) ? remembered : null;
}

/**
 * Whether the remembered project is still in the saved list, reading page by
 * page (the list comes 50 at a time). "gone" only after the whole list was
 * read; a list that could not be read to the end says "unknown" (keep it).
 */
export async function findRememberedProject(remembered: string, listPage: (cursor: string | null) => Promise<{ ids: readonly string[]; nextCursor: string | null }>, maxPages = 200): Promise<"found" | "gone" | "unknown"> {
  let cursor: string | null = null;
  for (let page = 0; page < maxPages; page++) {
    const result = await listPage(cursor);
    if (result.ids.includes(remembered)) return "found";
    if (result.nextCursor === null) return "gone";
    cursor = result.nextCursor;
  }
  return "unknown";
}
