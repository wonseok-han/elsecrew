/** Unsaved editor text kept across an app restart, the way a code editor's
 * hot exit works: quitting never asks and never loses what was typed. One
 * entry per task workspace, restored only into that exact workspace; saving,
 * reloading or discarding a tab removes it. Nothing here writes a project
 * file.
 *
 * What is kept stays kept (PR #18 review B2): a save never drops another
 * workspace's drafts to make room, never rewrites an entry it cannot read
 * (an older or damaged one stays as it was), and when storage refuses a
 * write nothing already kept is removed. A draft that does not fit is simply
 * not kept, and `save` says so. */
import type { FileVersionV1 } from "./contracts/file-v1.ts";
import type { ChatWorkspaceRefV1 } from "./contracts/workspace-addressing-v1.ts";

export interface DraftTab {
  path: string;
  text: string;
  /** The saved version the draft was typed over: a later save still refuses
   * to overwrite a file that changed on disk in the meantime. */
  baseVersion: FileVersionV1;
  byteLength: number;
  baseTextLength?: number;
}
export interface WorkspaceDrafts {
  workspace: ChatWorkspaceRefV1;
  activePath: string | null;
  tabs: DraftTab[];
}
export interface EditorDraftStore {
  load(workspace: ChatWorkspaceRefV1): WorkspaceDrafts | null;
  /** `null` forgets the workspace's drafts. True when the store now holds
   * exactly what was given; false leaves every kept draft as it was. */
  save(workspace: ChatWorkspaceRefV1, drafts: Omit<WorkspaceDrafts, "workspace"> | null): boolean;
}

export const EDITOR_DRAFTS_KEY = "elsecrew.editorDrafts.v1";
/** At most this many workspaces have kept drafts; a ninth is not kept. */
export const EDITOR_DRAFTS_MAX_WORKSPACES = 8;
/** Browser storage is small; drafts beyond this in all are not kept. */
export const EDITOR_DRAFTS_MAX_CHARS = 2_000_000;

type KeyValue = Pick<Storage, "getItem" | "setItem" | "removeItem">;

const ID = /^[A-Za-z0-9_-]{1,128}$/u;
const VERSION = /^sha256:[0-9a-f]{64}$/u;

function workspaceRef(value: unknown): ChatWorkspaceRefV1 | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const { chatId, projectId, worktreeId, workspaceIdentity } = value as Record<string, unknown>;
  for (const id of [chatId, projectId, worktreeId, workspaceIdentity]) if (typeof id !== "string" || !ID.test(id)) return null;
  return { chatId, projectId, worktreeId, workspaceIdentity } as ChatWorkspaceRefV1;
}

function draftTab(value: unknown): DraftTab | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const { path, text, baseVersion, byteLength, baseTextLength } = value as Record<string, unknown>;
  if (typeof path !== "string" || !path.length || path.length > 4096 || typeof text !== "string"
    || typeof baseVersion !== "string" || !VERSION.test(baseVersion)
    || !Number.isSafeInteger(byteLength) || (byteLength as number) < 0
    || (baseTextLength !== undefined && (!Number.isSafeInteger(baseTextLength) || (baseTextLength as number) < 0))) return null;
  return { path, text, baseVersion: baseVersion as FileVersionV1, byteLength: byteLength as number,
    ...(baseTextLength === undefined ? {} : { baseTextLength: baseTextLength as number }) };
}

function entry(value: unknown): WorkspaceDrafts | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const { workspace, activePath, tabs } = value as Record<string, unknown>;
  const ref = workspaceRef(workspace);
  if (!ref || !(activePath === null || typeof activePath === "string") || !Array.isArray(tabs) || !tabs.length) return null;
  const parsed: DraftTab[] = [];
  for (const tab of tabs) {
    const draft = draftTab(tab);
    if (!draft || parsed.some(item => item.path === draft.path)) return null;
    parsed.push(draft);
  }
  return { workspace: ref, activePath: activePath as string | null, tabs: parsed };
}

function same(left: ChatWorkspaceRefV1, right: ChatWorkspaceRefV1): boolean {
  return left.chatId === right.chatId && left.projectId === right.projectId
    && left.worktreeId === right.worktreeId && left.workspaceIdentity === right.workspaceIdentity;
}

const chars = (drafts: WorkspaceDrafts) => drafts.tabs.reduce((sum, tab) => sum + tab.text.length, 0);

/** `storage` is the browser's own (null when unavailable: nothing is kept). */
/** What the store holds: readable entries, and the raw entries it cannot
 * read (kept as they are). `null` when the stored value itself is not one this
 * store wrote: then nothing is rewritten over it. */
interface Stored { entries: WorkspaceDrafts[]; unread: unknown[] }

export function createEditorDraftStore(storage: KeyValue | null): EditorDraftStore {
  const readStored = (): Stored | null => {
    let raw: string | null | undefined;
    try { raw = storage?.getItem(EDITOR_DRAFTS_KEY); } catch { return null; }
    if (!raw) return { entries: [], unread: [] };
    try {
      const value: unknown = JSON.parse(raw);
      if (!value || typeof value !== "object" || (value as { version?: unknown }).version !== 1) return null;
      const list = (value as { entries?: unknown }).entries;
      if (!Array.isArray(list)) return null;
      // One unreadable entry never takes the others with it, and is kept.
      const entries: WorkspaceDrafts[] = [], unread: unknown[] = [];
      for (const item of list) { const parsed = entry(item); if (parsed) entries.push(parsed); else unread.push(item); }
      return { entries, unread };
    } catch { return null; }
  };
  const readAll = (): WorkspaceDrafts[] => readStored()?.entries ?? [];
  const writeAll = (entries: WorkspaceDrafts[], unread: unknown[]): boolean => {
    if (!storage) return false;
    try {
      if (entries.length || unread.length) storage.setItem(EDITOR_DRAFTS_KEY, JSON.stringify({ version: 1, entries: [...entries, ...unread] }));
      else storage.removeItem(EDITOR_DRAFTS_KEY);
      return true;
    } catch { return false; }
  };
  return {
    load(workspace) {
      // Only the same task in the same workspace copy: a draft never lands on other files.
      return readAll().find(item => same(item.workspace, workspace)) ?? null;
    },
    save(workspace, drafts) {
      const stored = readStored();
      // A value this store cannot read is never written over.
      if (!stored) return false;
      // Only this exact workspace's entry is replaced; every other stays,
      // including another workspace of the same task.
      const others = stored.entries.filter(item => !same(item.workspace, workspace));
      const entries = [...others];
      let whole = true;
      if (drafts?.tabs.length) {
        // No room: nothing kept is given up for it. Tabs that do not fit
        // what is left are not kept, whole; the rest is.
        if (others.length + stored.unread.length + 1 > EDITOR_DRAFTS_MAX_WORKSPACES) return false;
        const rawChars = stored.unread.reduce<number>((sum, item) => sum + (JSON.stringify(item)?.length ?? 0), 0);
        let room = EDITOR_DRAFTS_MAX_CHARS - others.reduce((sum, item) => sum + chars(item), 0) - rawChars;
        const tabs = drafts.tabs.filter(tab => { if (tab.text.length > room) return false; room -= tab.text.length; return true; });
        whole = tabs.length === drafts.tabs.length;
        if (tabs.length) entries.unshift({ workspace: { chatId: workspace.chatId, projectId: workspace.projectId, worktreeId: workspace.worktreeId, workspaceIdentity: workspace.workspaceIdentity },
          activePath: drafts.activePath, tabs });
      } else if (others.length === stored.entries.length) return true;
      // Storage refused: what was kept stays as it was.
      return writeAll(entries, stored.unread) && whole;
    },
  };
}
