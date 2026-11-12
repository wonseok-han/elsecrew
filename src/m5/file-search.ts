import { TREE_PAGE_MAX_ENTRIES_V1, type TreeEntryV1, type TreePageV1 } from "./contracts/project-v1.ts";

export interface FileSearchResult { matches: TreeEntryV1[]; pages: number; limited: boolean }
export class FileSearchBusyError extends Error {}
export const FILE_SEARCH_LIMITS = { pages: 100, matches: 100, directories: 1000, milliseconds: 10_000 };

/** Read names only, one bounded page at a time. Never follow symlinks. */
export async function searchFileNames(
  read: (parentPath: string, cursor: string | null) => Promise<Pick<TreePageV1, "parentPath" | "entries" | "nextCursor">>,
  query: string,
  signal: AbortSignal,
): Promise<FileSearchResult> {
  const needle = query.trim().normalize("NFC").toLowerCase();
  if (!needle || needle.length > 256) throw new Error("file search needs 1–256 characters");
  const folders = [""];
  const seenFolders = new Set(folders);
  const matches: TreeEntryV1[] = [];
  const seenFiles = new Set<string>();
  const started = Date.now();
  let pages = 0;
  const check = () => { if (signal.aborted) throw new DOMException("Search canceled", "AbortError"); };
  for (let index = 0; index < folders.length; index++) {
    const parent = folders[index];
    let cursor: string | null = null;
    const cursors = new Set<string>();
    do {
      check();
      if (pages >= FILE_SEARCH_LIMITS.pages || Date.now() - started >= FILE_SEARCH_LIMITS.milliseconds) return { matches, pages, limited: true };
      const page = await read(parent, cursor);
      check();
      pages++;
      if (page.parentPath !== parent || page.entries.length > TREE_PAGE_MAX_ENTRIES_V1) throw new Error("file search page mismatch");
      for (const entry of page.entries) {
        if (!entry.name || entry.name === "." || entry.name === ".." || entry.name.includes("/") || entry.name.includes("\\") || entry.path !== `${parent ? `${parent}/` : ""}${entry.name}`) throw new Error("file search path mismatch");
        if (entry.kind === "directory" && !seenFolders.has(entry.path)) {
          if (folders.length >= FILE_SEARCH_LIMITS.directories) return { matches, pages, limited: true };
          seenFolders.add(entry.path); folders.push(entry.path);
        } else if (entry.kind === "file" && !seenFiles.has(entry.path) && entry.path.normalize("NFC").toLowerCase().includes(needle)) {
          seenFiles.add(entry.path); matches.push(entry);
          if (matches.length >= FILE_SEARCH_LIMITS.matches) return { matches, pages, limited: true };
        }
      }
      cursor = page.nextCursor;
      if (cursor) { if (cursors.has(cursor)) throw new Error("file search cursor repeated"); cursors.add(cursor); }
    } while (cursor);
  }
  return { matches, pages, limited: false };
}
