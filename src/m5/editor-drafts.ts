/** Unsaved editor text kept across an app restart, the way a code editor's
 * hot exit works: quitting never asks and never loses what was typed. One
 * entry per task workspace, restored only into that exact workspace; saving,
 * reloading or discarding a tab removes it. Nothing here writes a project
 * file, and any storage problem only ever means "nothing was kept". */
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
  /** `null` forgets the workspace's drafts. */
  save(workspace: ChatWorkspaceRefV1, drafts: Omit<WorkspaceDrafts, "workspace"> | null): void;
}

export const EDITOR_DRAFTS_KEY = "elsecrew.editorDrafts.v1";
/** Tasks left with unsaved edits and never reopened do not pile up. */
export const EDITOR_DRAFTS_MAX_WORKSPACES = 8;
/** Browser storage is small; a draft beyond this is simply not kept. */
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
export function createEditorDraftStore(storage: KeyValue | null): EditorDraftStore {
  const readAll = (): WorkspaceDrafts[] => {
    try {
      const raw = storage?.getItem(EDITOR_DRAFTS_KEY);
      if (!raw) return [];
      const value: unknown = JSON.parse(raw);
      if (!value || typeof value !== "object" || (value as { version?: unknown }).version !== 1) return [];
      const entries = (value as { entries?: unknown }).entries;
      if (!Array.isArray(entries)) return [];
      // One unreadable entry never takes the others with it.
      return entries.map(entry).filter((item): item is WorkspaceDrafts => item !== null);
    } catch { return []; }
  };
  const writeAll = (entries: WorkspaceDrafts[]): boolean => {
    if (!storage) return false;
    try {
      if (entries.length) storage.setItem(EDITOR_DRAFTS_KEY, JSON.stringify({ version: 1, entries }));
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
      const others = readAll().filter(item => item.workspace.chatId !== workspace.chatId);
      const entries = others.slice(0, EDITOR_DRAFTS_MAX_WORKSPACES - 1);
      if (drafts?.tabs.length) {
        // Newest first. Tabs that do not fit the budget are left out, whole.
        let room = EDITOR_DRAFTS_MAX_CHARS;
        const tabs = drafts.tabs.filter(tab => { if (tab.text.length > room) return false; room -= tab.text.length; return true; });
        if (tabs.length) {
          entries.unshift({ workspace: { chatId: workspace.chatId, projectId: workspace.projectId, worktreeId: workspace.worktreeId, workspaceIdentity: workspace.workspaceIdentity },
            activePath: drafts.activePath, tabs });
        }
      }
      // Older tasks give way first when the whole set is too large.
      while (entries.length > 1 && entries.reduce((sum, item) => sum + chars(item), 0) > EDITOR_DRAFTS_MAX_CHARS) entries.pop();
      if (writeAll(entries)) return;
      // Storage refused (full or unavailable): keep the newest alone, or nothing
      // rather than something stale.
      if (!writeAll(entries.slice(0, 1))) { try { storage?.removeItem(EDITOR_DRAFTS_KEY); } catch { /* nothing kept */ } }
    },
  };
}
