import { ko, type MessageKey } from "../locales/ko.ts";
type EditorNoticeKey = Extract<MessageKey, `m5.editorNotice.${string}`>;
function editorNotice(key: EditorNoticeKey) { return { notice: ko[key], noticeKey: key }; }
type EditorErrorKey = Extract<MessageKey, `m5.editorError.${string}`>;
import type { M5BridgeV2 } from "./contracts/bridge-v2.ts";
import type { M5BridgeV3 } from "./contracts/bridge-v3.ts";
import type { ChatWorkspaceRefV1, WorkspaceEventV1 } from "./contracts/workspace-addressing-v1.ts";
import { FILE_MAX_BYTES_V1, TREE_PAGE_MAX_ENTRIES_V1, type TreeEntryV1, type WatchEventV1 } from "./contracts/project-v1.ts";
import type { DiffFileDescriptorV1, DiffSummaryV1 } from "./contracts/diff-v1.ts";
import type { FileVersionV1 } from "./contracts/file-v1.ts";
import type { EventV1 } from "./contracts/protocol-v1.ts";
import { FileSearchBusyError, searchFileNames, type FileSearchResult } from "./file-search.ts";
import { validateProjectCatalogResult, type ProjectCatalogRequest, type ProjectCatalogResult } from "./contracts/project-catalog-v1.ts";

export const MAX_RENDERED_TREE_ENTRIES = TREE_PAGE_MAX_ENTRIES_V1;
export const MAX_DIFF_STREAM_BYTES = 10 * 1024 * 1024;
export const STREAM_TIMEOUT_MS = 3_000;
export const MAX_PENDING_STREAMS = 16;
export const MAX_PENDING_STREAM_BYTES = 20 * 1024 * 1024;
export const DEFAULT_SNAPSHOT_CREATION_TIMEOUT_MS = 60_000;
export const WORKBENCH_PANELS = ["files", "editor", "changes"] as const;

export type WorkbenchPanel = (typeof WORKBENCH_PANELS)[number];
export type TabMode = "text" | "binary" | "tooLarge";
export type SaveState = "clean" | "dirty" | "saving" | "conflicted";

export interface EditorTab {
  path: string;
  mode: TabMode;
  saveState: SaveState;
  baseVersion: FileVersionV1;
  byteLength: number;
  text: string;
  technicalDetail?: string;
}

export interface EditorSnapshot {
  project: { id: string; name: string; root: string } | null;
  workspace: ChatWorkspaceRefV1 | null;
  tree: readonly TreeEntryV1[];
  treeParent: string;
  treeNextCursor: string | null;
  treePage: number;
  treeLoading: boolean;
  tabs: readonly EditorTab[];
  activePath: string | null;
  panel: WorkbenchPanel;
  panelRequest: { panel: WorkbenchPanel } | null;
  diffSummary: DiffSummaryV1 | null;
  diffFile: DiffFileDescriptorV1 | null;
  diffText: string;
  rescanRequired: boolean;
  selectionBlocked: boolean;
  actionBlocked: boolean;
  notice: string | null;
  noticeKey?: EditorNoticeKey;
  error: { message: string; messageKey: EditorErrorKey; technicalDetail: string } | null;
}

export type ProjectBridge = Pick<
  M5BridgeV2,
  | "openProject"
  | "getProjectTree"
  | "createProjectWorktree"
  | "closeProject"
  | "readFile"
  | "beginFileSave"
  | "appendFileSaveChunk"
  | "commitFileSave"
  | "watchProjectFiles"
  | "getDiffSummary"
  | "getDiffFile"
  | "onSidecarEvent"
  | "onProjectEvent"
>;
export type ChatProjectBridge = ProjectBridge & Partial<Pick<M5BridgeV3,
  "getWorkspaceTree" | "closeWorkspace" | "readWorkspaceFile" | "beginWorkspaceFileSave"
  | "appendWorkspaceFileSaveChunk" | "commitWorkspaceFileSave" | "watchWorkspaceFiles"
  | "getWorkspaceDiffSummary" | "getWorkspaceDiffFile" | "onWorkspaceEvent"
>>;

interface StreamAssembly {
  chunks: string[];
  chunkCount: number;
  byteLength: number;
  nextSeq: number;
  expectedBytes: number | null;
  maxBytes: number | null;
  failure: Error | null;
  resolve: ((text: string) => void) | null;
  reject: ((error: Error) => void) | null;
  timeout: ReturnType<typeof setTimeout> | null;
}

interface PendingStream {
  chunks: string[];
  chunkCount: number;
  byteLength: number;
  nextSeq: number;
  failure: Error | null;
  expiry: ReturnType<typeof setTimeout>;
}

interface SnapshotCreationOperation {
  project: { projectId: string; canonicalGitRoot: string };
  selection: number;
  controllerGeneration: number;
  timedOut: boolean;
  resolve: (value: { epoch: number } | null) => void;
}

export interface ProjectEditorControllerOptions {
  snapshotCreationTimeoutMs?: number;
}

export class ProjectEditorController {
  readonly #bridge: ChatProjectBridge;
  readonly #listeners = new Set<() => void>();
  readonly #streams = new Map<string, StreamAssembly>();
  readonly #pendingStreams = new Map<string, PendingStream>();
  readonly #saveGenerations = new Map<string, number>();
  readonly #saveWatchPending = new Set<string>();
  readonly #snapshotCreationTimeoutMs: number;
  #pendingStreamBytes = 0;
  #pendingOverflowUntil = 0;
  #unsubscribers: (() => void)[] = [];
  #started = false;
  #disposed = false;
  #controllerGeneration = 0;
  #projectEpoch = 0;
  #selectionGeneration = 0;
  #treeGeneration = 0;
  #snapshotCreation: SnapshotCreationOperation | null = null;
  #snapshotCreationBlocked: SnapshotCreationOperation | null = null;
  #actionBlocked = false;
  #fileSearchActive = false;
  #state: EditorSnapshot = {
    project: null,
    workspace: null,
    tree: [],
    treeParent: "",
    treeNextCursor: null,
    treePage: 1,
    treeLoading: false,
    tabs: [],
    activePath: null,
    panel: "files",
    panelRequest: null,
    diffSummary: null,
    diffFile: null,
    diffText: "",
    rescanRequired: false,
    selectionBlocked: false,
    actionBlocked: false,
    notice: null,
    error: null,
  };

  constructor(bridge: ChatProjectBridge, options: ProjectEditorControllerOptions = {}) {
    this.#bridge = bridge;
    const timeout = options.snapshotCreationTimeoutMs ?? DEFAULT_SNAPSHOT_CREATION_TIMEOUT_MS;
    if (!Number.isSafeInteger(timeout) || timeout <= 0) throw new Error("snapshot creation timeout must be a positive safe integer");
    this.#snapshotCreationTimeoutMs = timeout;
  }

  start(): void {
    if (this.#started) return;
    this.#disposed = false;
    this.#started = true;
    this.#unsubscribers = [
      this.#bridge.onSidecarEvent((event) => this.#observeSidecarEvent(event)),
      this.#bridge.onProjectEvent((event) => this.#observeProjectEvent(event)),
    ];
    if (this.#bridge.onWorkspaceEvent) this.#unsubscribers.push(this.#bridge.onWorkspaceEvent((event) => this.#observeWorkspaceEvent(event)));
  }

  snapshot(): EditorSnapshot {
    return this.#state;
  }
  setActionBlocked(blocked: boolean, notice?: string | { key: EditorNoticeKey }): void {
    const clearBlockingNotice = !blocked && this.#actionBlocked && this.#state.noticeKey !== undefined
      && ["m5.editorNotice.preparing", "m5.editorNotice.settings", "m5.editorNotice.switching", "m5.editorNotice.protected"].includes(this.#state.noticeKey);
    if (blocked && !this.#actionBlocked) this.#selectionGeneration += 1;
    this.#actionBlocked = blocked;
    this.#set({ actionBlocked: blocked, ...(clearBlockingNotice ? { notice: null, noticeKey: undefined } : {}), ...(blocked && notice ? typeof notice === "string" ? { notice } : editorNotice(notice.key) : {}) });
  }

  subscribe(listener: () => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  dispose(): void {
    this.#disposed = true;
    this.#controllerGeneration += 1;
    this.#selectionGeneration += 1;
    this.#projectEpoch += 1;
    this.#snapshotCreationBlocked = null;
    this.#snapshotCreation?.resolve(null);
    for (const unsubscribe of this.#unsubscribers) unsubscribe();
    this.#unsubscribers = [];
    this.#started = false;
    for (const stream of this.#streams.values()) {
      if (stream.timeout) clearTimeout(stream.timeout);
      stream.reject?.(new Error("editor disposed before stream completed"));
    }
    this.#streams.clear();
    for (const stream of this.#pendingStreams.values()) clearTimeout(stream.expiry);
    this.#pendingStreams.clear();
    this.#pendingStreamBytes = 0;
    this.#pendingOverflowUntil = 0;
    this.#listeners.clear();
  }

  async chooseProject(): Promise<void> {
    return this.#selectProject(() => this.#bridge.openProject());
  }

  async reopenProject(projectId: string, request: (input: ProjectCatalogRequest) => Promise<ProjectCatalogResult>): Promise<void> {
    return this.#selectProject(async () => {
      const input = { action: "reopen" as const, projectId };
      const result = validateProjectCatalogResult(await request(input), input);
      if (result.action !== "reopen") throw new Error("unexpected project catalog response");
      return result.project;
    });
  }

  async #selectProject(open: ProjectBridge["openProject"]): Promise<void> {
    if (this.#disposed) return;
    if (this.#actionBlocked) return;
    if (this.#state.workspace) {
      this.#setNotice("m5.editorNotice.activeChat");
      return;
    }
    if (!this.canLeaveProject()) {
      this.#setNotice("m5.editorNotice.dirtyProject");
      return;
    }
    if (this.#snapshotCreationBlocked) return;
    const selection = ++this.#selectionGeneration;
    const controllerGeneration = this.#controllerGeneration;
    try {
      const project = await open();
      if (!this.#isLiveGeneration(controllerGeneration) || selection !== this.#selectionGeneration) return;
      if (this.#snapshotCreationBlocked) return;
      if (this.#actionBlocked) return;
      if (this.#state.workspace || !this.canLeaveProject()) {
        this.#setNotice("m5.editorNotice.changed");
        return;
      }
      // Picker cancellation must leave the existing project surface untouched. Clear transient
      // messages only after the native picker returned a real project identity.
      this.#set({ error: null, notice: null });
      if (this.#snapshotCreation) {
        this.#setNotice("m5.editorNotice.previous");
        return;
      }
      const active = await this.#createSnapshot(project, selection, controllerGeneration);
      if (!active || !this.#isLiveGeneration(controllerGeneration) || selection !== this.#selectionGeneration) return;
      await Promise.all([
        this.#loadTree(project.projectId, active.epoch, ""),
        this.#refreshChanges(project.projectId, active.epoch),
        this.#bridge.watchProjectFiles({ projectId: project.projectId }),
      ]);
    } catch (error) {
      if (!this.#isLiveGeneration(controllerGeneration) || selection !== this.#selectionGeneration) return;
      if (isPickerCancelled(error)) {
        return;
      }
      this.#setError("m5.editorError.openProject", error);
    }
  }

  /** Binds the editor only after chat.create persisted its immutable workspace triple. */
  async bindChat(input: ChatWorkspaceRefV1): Promise<boolean> {
    // Chat records are structurally assignable to this type but contain fields
    // forbidden by the exact workspace IPC contract. Retain only the address.
    const workspace: ChatWorkspaceRefV1 = { projectId: input.projectId, chatId: input.chatId, worktreeId: input.worktreeId, workspaceIdentity: input.workspaceIdentity };
    if (this.#disposed) return false;
    if (!this.canLeaveProject()) { this.#setNotice("m5.editorNotice.dirtyChat"); return false; }
    if (!this.#workspaceMethods()) { this.#setError("m5.editorError.workspaceUnavailable", new Error("M5BridgeV3 workspace methods are unavailable")); return false; }
    const project = this.#state.project;
    if (!project || project.id !== workspace.projectId) { this.#setError("m5.editorError.identity", new Error("chat project identity mismatch")); return false; }
    const previous = this.#state;
    const epoch = ++this.#projectEpoch;
    this.#set({ workspace, tree: [], treeParent: "", treeNextCursor: null, treePage: 1, treeLoading: false, tabs: [], activePath: null, diffSummary: null, diffFile: null, diffText: "", rescanRequired: false, error: null, ...editorNotice("m5.editorNotice.opened") });
    try {
      // Bind in order. A failed tree must not start later diff/watch operations for B.
      await this.#loadTree(project.id, epoch, "", true);
      await this.#refreshChanges(project.id, epoch, true);
      await this.#bridge.watchWorkspaceFiles!(workspace);
      return this.#isCurrent(project.id, epoch) && sameWorkspace(this.#state.workspace, workspace);
    } catch (error) {
      if (this.#isCurrent(project.id, epoch) && sameWorkspace(this.#state.workspace, workspace)) {
        this.#projectEpoch += 1;
        this.#set({ ...previous, actionBlocked: true, ...editorNotice("m5.editorNotice.failedOpen"), error: errorDetail("m5.editorError.openWorkspace", error) });
      }
      return false;
    }
  }

  async #createSnapshot(
    project: { projectId: string; canonicalGitRoot: string },
    selection: number,
    controllerGeneration: number,
  ): Promise<{ epoch: number } | null> {
    let resolve!: (value: { epoch: number } | null) => void;
    const completion = new Promise<{ epoch: number } | null>((nextResolve) => { resolve = nextResolve; });
    const operation: SnapshotCreationOperation = { project, selection, controllerGeneration, timedOut: false, resolve };
    this.#snapshotCreation = operation;
    void this.#observeSnapshotCreation(operation);

    let timeout: ReturnType<typeof setTimeout> | null = null;
    const timedOut = new Promise<null>((nextResolve) => {
      timeout = setTimeout(() => {
        if (!this.#isLiveOperation(operation)) {
          operation.resolve(null);
          nextResolve(null);
          return;
        }
        operation.timedOut = true;
        this.#blockSnapshotCreation(
          operation,
          "m5.editorError.timeout",
          new Error(`snapshot creation did not settle within ${this.#snapshotCreationTimeoutMs}ms`),
        );
        nextResolve(null);
      }, this.#snapshotCreationTimeoutMs);
    });
    const result = await Promise.race([completion, timedOut]);
    if (timeout) clearTimeout(timeout);
    return result;
  }

  async #observeSnapshotCreation(operation: SnapshotCreationOperation): Promise<void> {
    try {
      const manifest = await this.#bridge.createProjectWorktree({ projectId: operation.project.projectId });
      const valid = manifest.projectId === operation.project.projectId
        && manifest.workspaceKind === "standaloneSnapshot"
        && manifest.lifecycle === "active";
      if (!valid) {
        await this.#abandonCreatedSnapshot(operation, new Error("snapshot manifest does not match the selected project"));
        return;
      }
      if (!this.#isLiveOperation(operation) || operation.timedOut || operation.selection !== this.#selectionGeneration || this.#actionBlocked || this.#state.workspace || !this.canLeaveProject()) {
        await this.#abandonCreatedSnapshot(operation, new Error("snapshot creation completed after its selection was no longer current"));
        return;
      }
      const epoch = ++this.#projectEpoch;
      this.#set({
        project: {
          id: operation.project.projectId,
          // Keep the picker-approved source root as the visible project identity. The private
          // snapshot path is never rendered; all workspace operations use this project ID.
          name: basename(operation.project.canonicalGitRoot),
          root: operation.project.canonicalGitRoot,
        },
        workspace: null,
        tree: [],
        treeParent: "",
        treeNextCursor: null,
        treePage: 1,
        treeLoading: false,
        tabs: [],
        activePath: null,
        diffSummary: null,
        diffFile: null,
        diffText: "",
        rescanRequired: false,
      });
      operation.resolve({ epoch });
    } catch (error) {
      if (operation.timedOut) {
        this.#blockSnapshotCreation(
          operation,
          "m5.editorError.unknownPreparation",
          error,
        );
      } else if (this.#isLiveOperation(operation) && operation.selection === this.#selectionGeneration) {
        this.#setError("m5.editorError.prepare", error);
      } else {
        this.#setError("m5.editorError.previousSelection", error);
      }
      operation.resolve(null);
    } finally {
      if (this.#snapshotCreation === operation) this.#snapshotCreation = null;
    }
  }

  async #abandonCreatedSnapshot(operation: SnapshotCreationOperation, reason: Error): Promise<void> {
    try {
      // project.close is an evidence-preserving abandon operation, not deletion.
      await this.#bridge.closeProject({ projectId: operation.project.projectId });
      if (!this.#isLiveOperation(operation)) {
        return;
      }
      if (operation.timedOut) {
        this.#clearSnapshotCreationBlock(operation);
        this.#setNotice("m5.editorNotice.lateClosed");
      } else if (operation.selection !== this.#selectionGeneration) {
        this.#setNotice("m5.editorNotice.previousClosed");
      } else {
        this.#setError("m5.editorError.unknownWorkspace", reason);
      }
    } catch (error) {
      if (!this.#isLiveOperation(operation)) return;
      this.#blockSnapshotCreation(
        operation,
        "m5.editorError.previousClose",
        error,
      );
    } finally {
      operation.resolve(null);
    }
  }

  #blockSnapshotCreation(operation: SnapshotCreationOperation, message: EditorErrorKey, error: unknown): void {
    if (!this.#isLiveOperation(operation)) return;
    this.#snapshotCreationBlocked = operation;
    this.#set({ selectionBlocked: true, error: errorDetail(message, error) });
  }

  #clearSnapshotCreationBlock(operation: SnapshotCreationOperation): void {
    if (!this.#isLiveOperation(operation)) return;
    if (this.#snapshotCreationBlocked !== operation) return;
    this.#snapshotCreationBlocked = null;
    this.#set({ selectionBlocked: false });
  }

  async loadTree(parentPath = ""): Promise<void> {
    if (this.#actionBlocked) return;
    const project = this.#state.project;
    if (!project) return;
    await this.#loadTree(project.id, this.#projectEpoch, parentPath);
  }

  async searchFiles(query: string, signal: AbortSignal): Promise<FileSearchResult> {
    const project = this.#state.project, workspace = this.#state.workspace, epoch = this.#projectEpoch;
    const check = () => {
      if (!project || signal.aborted || this.#actionBlocked || !this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) throw new DOMException("Search context changed", "AbortError");
    };
    check();
    if (this.#fileSearchActive) throw new FileSearchBusyError("previous search read is still pending");
    this.#fileSearchActive = true;
    try { return await searchFileNames(async (parentPath, cursor) => {
      check();
      const page = workspace
        ? await this.#workspaceMethods()!.getWorkspaceTree({ ...workspace, parentPath, cursor })
        : await this.#bridge.getProjectTree({ projectId: project!.id, parentPath, cursor });
      check();
      return page;
    }, query, signal); } finally { this.#fileSearchActive = false; }
  }

  async loadNextTreePage(): Promise<void> {
    const { project, treeNextCursor, treeParent, treePage, treeLoading } = this.#state;
    if (this.#actionBlocked || !project || !treeNextCursor || treeLoading) return;
    await this.#loadTree(project.id, this.#projectEpoch, treeParent, false, treeNextCursor, treePage + 1);
  }

  async #loadTree(projectId: string, epoch: number, parentPath: string, propagate = false, cursor: string | null = null, pageNumber = 1): Promise<void> {
    const generation = ++this.#treeGeneration;
    this.#set({ treeLoading: true });
    try {
      const workspace = this.#state.workspace;
      const page = workspace ? await this.#workspaceMethods()!.getWorkspaceTree({ ...workspace, parentPath, cursor }) : await this.#bridge.getProjectTree({ projectId, parentPath, cursor });
      if (!this.#isCurrent(projectId, epoch) || !sameWorkspace(this.#state.workspace, workspace) || generation !== this.#treeGeneration) return;
      // The sidecar contract already caps a page at 500. Keep the renderer bounded too.
      const entries = page.entries.slice(0, MAX_RENDERED_TREE_ENTRIES);
      this.#set({ tree: entries, treeParent: page.parentPath, treeNextCursor: page.nextCursor, treePage: pageNumber, treeLoading: false, rescanRequired: false });
    } catch (error) {
      if (!this.#isCurrent(projectId, epoch) || generation !== this.#treeGeneration) return;
      this.#set({ treeLoading: false });
      this.#setError("m5.editorError.tree", error);
      if (propagate) throw error;
    }
  }

  async openFile(path: string): Promise<void> {
    if (this.#actionBlocked) return;
    const project = this.#state.project;
    if (!project) return;
    const epoch = this.#projectEpoch;
    const alreadyOpen = this.#state.tabs.find((tab) => tab.path === path);
    if (alreadyOpen) {
      this.#set({ activePath: path, panel: "editor", notice: null });
      return;
    }
    this.#set({ error: null, notice: null });
    try {
      const workspace = this.#state.workspace;
      const descriptor = workspace ? await this.#workspaceMethods()!.readWorkspaceFile({ ...workspace, path }) : await this.#bridge.readFile({ projectId: project.id, path });
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
      const text = descriptor.classification === "utf8" && descriptor.stream
        ? await this.#awaitStream(descriptor.stream.streamId, descriptor.byteLength, FILE_MAX_BYTES_V1)
        : "";
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
      const tab: EditorTab = {
        path,
        mode: descriptor.classification === "utf8" ? "text" : descriptor.classification,
        saveState: "clean",
        baseVersion: descriptor.version,
        byteLength: descriptor.byteLength,
        text,
      };
      // Another read may have opened this path while this stream was pending.
      // Keep its current draft/version rather than appending a duplicate tab.
      if (this.#tab(path)) {
        this.#set({ activePath: path, panel: "editor" });
        return;
      }
      this.#set({ tabs: [...this.#state.tabs, tab], activePath: path, panel: "editor" });
    } catch (error) {
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#setError("m5.editorError.file", error);
    }
  }

  updateText(path: string, text: string): void {
    const tab = this.#tab(path);
    if (!tab || tab.mode !== "text" || tab.saveState === "conflicted") return;
    if (text === tab.text) return;
    this.#replaceTab(path, { ...tab, text, saveState: tab.saveState === "saving" ? "saving" : "dirty" });
  }

  /** Requires explicit UI confirmation of this exact draft; does not write any file. */
  async reloadConfirmedDraft(confirmed: EditorTab): Promise<boolean> {
    const project = this.#state.project;
    const workspace = this.#state.workspace;
    const epoch = this.#projectEpoch;
    if (!project || this.#actionBlocked || confirmed.saveState === "saving" || this.#tab(confirmed.path) !== confirmed) return false;
    try {
      const descriptor = workspace ? await this.#workspaceMethods()!.readWorkspaceFile({ ...workspace, path: confirmed.path }) : await this.#bridge.readFile({ projectId: project.id, path: confirmed.path });
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return false;
      if (descriptor.classification !== "utf8" || !descriptor.stream) throw new Error("saved file is not an editable text file");
      const text = await this.#awaitStream(descriptor.stream.streamId, descriptor.byteLength, FILE_MAX_BYTES_V1);
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace) || this.#actionBlocked || this.#tab(confirmed.path) !== confirmed) return false;
      this.#replaceTab(confirmed.path, { ...confirmed, text, baseVersion: descriptor.version, byteLength: descriptor.byteLength, saveState: "clean", technicalDetail: undefined });
      this.#set({ error: null, ...editorNotice("m5.editorNotice.reloaded") });
      return true;
    } catch (error) {
      if (this.#isCurrent(project.id, epoch) && sameWorkspace(this.#state.workspace, workspace) && this.#tab(confirmed.path) === confirmed) this.#setError("m5.editorError.reload", error);
      return false;
    }
  }

  async saveActive(): Promise<void> {
    if (this.#actionBlocked) { this.#setNotice("m5.editorNotice.blockedSave"); return; }
    const project = this.#state.project;
    const epoch = this.#projectEpoch;
    const activePath = this.#state.activePath;
    if (activePath === null) return;
    const path = activePath;
    const tab = this.#tab(path);
    if (!project || !tab || tab.mode !== "text" || tab.saveState !== "dirty") return;
    const totalBytes = new TextEncoder().encode(tab.text).byteLength;
    if (totalBytes > FILE_MAX_BYTES_V1) {
      this.#setError("m5.editorError.large", new Error(`draft is ${totalBytes} bytes`));
      return;
    }
    const saveGeneration = (this.#saveGenerations.get(path) ?? 0) + 1;
    this.#saveGenerations.set(path, saveGeneration);
    this.#replaceTab(path, { ...tab, saveState: "saving" });
    try {
      const workspace = this.#state.workspace;
      const begin = workspace ? await this.#workspaceMethods()!.beginWorkspaceFileSave({ ...workspace, path, expectedVersion: tab.baseVersion, totalBytes, encoding: "utf8" }) : await this.#bridge.beginFileSave({ projectId: project.id, path, expectedVersion: tab.baseVersion, totalBytes, encoding: "utf8" });
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
      const chunks = splitUtf8(tab.text, begin.chunkBytes);
      for (const [index, text] of chunks.entries()) {
        if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
        if (workspace) await this.#workspaceMethods()!.appendWorkspaceFileSaveChunk({ ...workspace, saveId: begin.saveId, seq: index + 1, text });
        else await this.#bridge.appendFileSaveChunk({ saveId: begin.saveId, seq: index + 1, text });
      }
      const committed = workspace ? await this.#workspaceMethods()!.commitWorkspaceFileSave({ ...workspace, saveId: begin.saveId, expectedVersion: tab.baseVersion, lastSeq: chunks.length, totalBytes }) : await this.#bridge.commitFileSave({ saveId: begin.saveId, expectedVersion: tab.baseVersion, lastSeq: chunks.length, totalBytes });
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
      const latest = this.#tab(path);
      if (!latest || latest.saveState !== "saving" || this.#saveGenerations.get(path) !== saveGeneration) return;
      if (committed.outcome === "dirtyConflict") {
        this.#replaceTab(path, { ...latest, saveState: "conflicted", technicalDetail: committed.error.detail });
        this.#setNotice("m5.editorNotice.newerChange");
        return;
      }
      const hasNewerDraft = latest.text !== tab.text;
      this.#replaceTab(path, { ...latest, baseVersion: committed.version, saveState: hasNewerDraft ? "dirty" : "clean", byteLength: totalBytes });
      this.#setNotice(hasNewerDraft ? "m5.editorNotice.newerDraft" : "m5.editorNotice.saved");
      await this.refreshChanges();
    } catch (error) {
      if (!this.#isCurrent(project.id, epoch)) return;
      const latest = this.#tab(path);
      if (latest?.saveState === "saving" && this.#saveGenerations.get(path) === saveGeneration) {
        this.#replaceTab(path, { ...latest, saveState: "dirty" });
      }
      this.#setError("m5.editorError.save", error);
    } finally {
      if (this.#saveWatchPending.delete(path) && this.#isCurrent(project.id, epoch)) {
        const latest = this.#tab(path);
        if (latest && latest.saveState !== "conflicted") void this.#checkCleanFileChange(latest);
      }
    }
  }

  closeTab(path: string): boolean {
    if (this.#actionBlocked) { this.#setNotice("m5.editorNotice.blockedTab"); return false; }
    const tab = this.#tab(path);
    if (!tab) return true;
    if (tab.saveState !== "clean") {
      this.#setNotice("m5.editorNotice.dirtyTab");
      return false;
    }
    const tabs = this.#state.tabs.filter((item) => item.path !== path);
    const lastTab = tabs.length === 0 ? undefined : tabs[tabs.length - 1];
    this.#set({ tabs, activePath: this.#state.activePath === path ? lastTab?.path ?? null : this.#state.activePath });
    return true;
  }

  async abandonProject(): Promise<void> {
    if (this.#actionBlocked) { this.#setNotice("m5.editorNotice.blockedClose"); return; }
    const project = this.#state.project;
    if (!project) return;
    const epoch = this.#projectEpoch;
    if (!this.canLeaveProject()) {
      this.#setNotice("m5.editorNotice.dirtyClose");
      return;
    }
    try {
      const workspace = this.#state.workspace;
      if (workspace) await this.#workspaceMethods()!.closeWorkspace(workspace);
      else await this.#bridge.closeProject({ projectId: project.id });
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#projectEpoch += 1;
      this.#set({
        project: null,
        workspace: null,
        tree: [],
        treeParent: "",
        treeNextCursor: null,
        treePage: 1,
        treeLoading: false,
        tabs: [],
        activePath: null,
        diffSummary: null,
        diffFile: null,
        diffText: "",
        rescanRequired: false,
        ...editorNotice("m5.editorNotice.closed"),
      });
    } catch (error) {
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#setError("m5.editorError.close", error);
    }
  }

  /** Leave the view without abandoning the durable snapshot or its chat. */
  leaveProjectView(): boolean {
    if (this.#disposed || this.#actionBlocked || this.#snapshotCreation || this.#snapshotCreationBlocked || !this.canLeaveProject()) return false;
    this.#selectionGeneration++;
    this.#projectEpoch++;
    this.#set({ project: null, workspace: null, tree: [], treeParent: "", treeNextCursor: null,
      treePage: 1, treeLoading: false, tabs: [], activePath: null, diffSummary: null,
      diffFile: null, diffText: "", rescanRequired: false, error: null,
      ...editorNotice("m5.editorNotice.left") });
    return true;
  }

  async refreshChanges(): Promise<void> {
    if (this.#actionBlocked) return;
    const project = this.#state.project;
    if (!project) return;
    await this.#refreshChanges(project.id, this.#projectEpoch);
  }

  async #refreshChanges(projectId: string, epoch: number, propagate = false): Promise<void> {
    try {
      const workspace = this.#state.workspace;
      const diffSummary = workspace ? await this.#workspaceMethods()!.getWorkspaceDiffSummary(workspace) : await this.#bridge.getDiffSummary({ projectId });
      if (!this.#isCurrent(projectId, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
      this.#set({ diffSummary, rescanRequired: false });
    } catch (error) {
      if (!this.#isCurrent(projectId, epoch)) return;
      this.#setError("m5.editorError.changes", error);
      if (propagate) throw error;
    }
  }

  async openDiff(path: string): Promise<void> {
    if (this.#actionBlocked) return;
    const project = this.#state.project;
    if (!project) return;
    const epoch = this.#projectEpoch;
    try {
      const workspace = this.#state.workspace;
      const diffFile = workspace ? await this.#workspaceMethods()!.getWorkspaceDiffFile({ ...workspace, path }) : await this.#bridge.getDiffFile({ projectId: project.id, path });
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
      const diffText = diffFile.stream
        ? await this.#awaitStream(diffFile.stream.streamId, diffFile.byteLength, MAX_DIFF_STREAM_BYTES)
        : "";
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
      this.#set({ diffFile, diffText, panel: "changes", error: null });
    } catch (error) {
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#setError("m5.editorError.diff", error);
    }
  }

  selectPanel(panel: WorkbenchPanel): void {
    this.#set({ panel });
  }

  canLeaveProject(): boolean {
    return this.#state.tabs.every((tab) => tab.saveState === "clean");
  }

  #observeSidecarEvent(event: EventV1): void {
    if (!isBlobChunkEvent(event)) return;
    const stream = this.#streams.get(event.streamId);
    if (!stream) {
      this.#bufferPendingChunk(event.streamId, event.seq, event.payload.text);
      return;
    }
    this.#appendChunk(stream, event.streamId, event.seq, event.payload.text, stream.maxBytes ?? FILE_MAX_BYTES_V1);
    this.#settleStream(event.streamId);
  }

  #appendChunk(stream: Pick<StreamAssembly, "chunks" | "chunkCount" | "byteLength" | "nextSeq" | "failure">, streamId: string, seq: number, text: string, maxBytes: number): void {
    if (stream.failure) return;
    if (seq !== stream.nextSeq) {
      stream.failure = new Error(`stream ${streamId} expected seq ${stream.nextSeq}, got ${seq}`);
      return;
    }
    const chunkBytes = new TextEncoder().encode(text).byteLength;
    if (chunkBytes > 256 * 1024) {
      stream.failure = new Error(`stream ${streamId} chunk exceeds 256 KiB`);
      return;
    }
    if (stream.chunkCount >= 256) {
      stream.failure = new Error(`stream ${streamId} exceeds 256 chunks`);
      return;
    }
    if (stream.byteLength + chunkBytes > maxBytes) {
      stream.failure = new Error(`stream ${streamId} exceeds renderer byte limit`);
      return;
    }
    stream.nextSeq += 1;
    stream.chunkCount += 1;
    stream.byteLength += chunkBytes;
    stream.chunks.push(text);
  }

  #bufferPendingChunk(streamId: string, seq: number, text: string): void {
    let stream = this.#pendingStreams.get(streamId);
    if (!stream) {
      if (this.#pendingStreams.size >= MAX_PENDING_STREAMS) {
        // Reject an imminent descriptor instead of accepting a stream whose prefix was dropped.
        this.#pendingOverflowUntil = Date.now() + STREAM_TIMEOUT_MS;
        return;
      }
      const expiry = setTimeout(() => this.#discardPendingStream(streamId), STREAM_TIMEOUT_MS);
      stream = { chunks: [], chunkCount: 0, byteLength: 0, nextSeq: 1, failure: null, expiry };
      this.#pendingStreams.set(streamId, stream);
    }
    const beforeBytes = stream.byteLength;
    this.#appendChunk(stream, streamId, seq, text, MAX_DIFF_STREAM_BYTES);
    const addedBytes = stream.byteLength - beforeBytes;
    if (this.#pendingStreamBytes + addedBytes > MAX_PENDING_STREAM_BYTES) {
      stream.byteLength = beforeBytes;
      stream.chunks.pop();
      stream.nextSeq -= 1;
      stream.chunkCount -= 1;
      stream.failure = new Error(`stream ${streamId} exceeds pending renderer byte limit`);
      this.#pendingOverflowUntil = Date.now() + STREAM_TIMEOUT_MS;
      return;
    }
    this.#pendingStreamBytes += addedBytes;
  }

  #discardPendingStream(streamId: string): PendingStream | undefined {
    const stream = this.#pendingStreams.get(streamId);
    if (!stream) return undefined;
    clearTimeout(stream.expiry);
    this.#pendingStreams.delete(streamId);
    this.#pendingStreamBytes -= stream.byteLength;
    return stream;
  }

  #adoptPendingStream(streamId: string): StreamAssembly {
    const existing = this.#streams.get(streamId);
    if (existing) return existing;
    const pending = this.#discardPendingStream(streamId);
    const adopted: StreamAssembly = pending
      ? { chunks: pending.chunks, chunkCount: pending.chunkCount, byteLength: pending.byteLength, nextSeq: pending.nextSeq, expectedBytes: null, maxBytes: null, failure: pending.failure, resolve: null, reject: null, timeout: null }
      : { chunks: [], chunkCount: 0, byteLength: 0, nextSeq: 1, expectedBytes: null, maxBytes: null, failure: null, resolve: null, reject: null, timeout: null };
    this.#streams.set(streamId, adopted);
    return adopted;
  }

  #observeProjectEvent(event: WatchEventV1): void {
    if (this.#state.workspace) return;
    this.#applyWatchEvent(event);
  }

  #applyWatchEvent(event: WatchEventV1): void {
    const project = this.#state.project;
    if (!project || event.projectId !== project.id) return;
    if (event.rescanRequired) {
      this.#set({ rescanRequired: true, ...editorNotice("m5.editorNotice.rescan") });
      return;
    }
    for (const path of event.paths) {
      const tab = this.#tab(path);
      if (!tab) continue;
      if (event.event === "file.changed" && tab.saveState === "saving") {
        this.#saveWatchPending.add(path);
        continue;
      }
      if (event.event === "file.conflict" || tab.saveState === "dirty" || tab.saveState === "saving") {
        this.#replaceTab(path, { ...tab, saveState: "conflicted", technicalDetail: "external file change" });
        this.#set({ rescanRequired: true, ...editorNotice("m5.editorNotice.externalDraft") });
      } else {
        void this.#checkCleanFileChange(tab);
      }
    }
    if (event.event === "diff.changed") void this.refreshChanges();
  }

  async #checkCleanFileChange(tab: EditorTab): Promise<void> {
    const project = this.#state.project;
    const workspace = this.#state.workspace;
    const epoch = this.#projectEpoch;
    if (!project) return;
    if (this.#actionBlocked) return;
    let unchanged = false;
    try {
      const descriptor = workspace
        ? await this.#workspaceMethods()!.readWorkspaceFile({ ...workspace, path: tab.path })
        : await this.#bridge.readFile({ projectId: project.id, path: tab.path });
      if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace)) return;
      // Drain the bounded read stream even when only its version is needed.
      if (descriptor.stream) await this.#awaitStream(descriptor.stream.streamId, descriptor.byteLength, FILE_MAX_BYTES_V1);
      unchanged = descriptor.version === tab.baseVersion;
    } catch {
      // An unreadable/deleted file is not evidence that the editor is current.
    }
    if (!this.#isCurrent(project.id, epoch) || !sameWorkspace(this.#state.workspace, workspace) || this.#actionBlocked) return;
    const latest = this.#tab(tab.path);
    if (!latest || latest.baseVersion !== tab.baseVersion || unchanged) return;
    if (latest.saveState !== "clean") {
      this.#replaceTab(tab.path, { ...latest, saveState: "conflicted", technicalDetail: "external file change" });
      this.#set({ rescanRequired: true, ...editorNotice("m5.editorNotice.externalDraft") });
    } else {
      this.#setNotice("m5.editorNotice.external");
    }
  }

  #observeWorkspaceEvent(event: WorkspaceEventV1): void {
    const workspace = this.#state.workspace;
    if (!workspace || !sameWorkspace(event.payload, workspace)) return;
    if (event.event === "workspace.blob.chunk") {
      const stream = this.#streams.get(event.streamId);
      if (!stream) this.#bufferPendingChunk(event.streamId, event.seq, event.payload.text);
      else { this.#appendChunk(stream, event.streamId, event.seq, event.payload.text, stream.maxBytes ?? FILE_MAX_BYTES_V1); this.#settleStream(event.streamId); }
      return;
    }
    this.#applyWatchEvent({
      projectId: event.payload.projectId,
      event: event.payload.event,
      paths: event.payload.paths,
      rescanRequired: event.payload.rescanRequired,
    });
  }

  #workspaceMethods(): Required<Pick<M5BridgeV3,
    "getWorkspaceTree" | "closeWorkspace" | "readWorkspaceFile" | "beginWorkspaceFileSave"
    | "appendWorkspaceFileSaveChunk" | "commitWorkspaceFileSave" | "watchWorkspaceFiles"
    | "getWorkspaceDiffSummary" | "getWorkspaceDiffFile"
  >> | null {
    const bridge = this.#bridge;
    if (!bridge.getWorkspaceTree || !bridge.closeWorkspace || !bridge.readWorkspaceFile || !bridge.beginWorkspaceFileSave
      || !bridge.appendWorkspaceFileSaveChunk || !bridge.commitWorkspaceFileSave || !bridge.watchWorkspaceFiles
      || !bridge.getWorkspaceDiffSummary || !bridge.getWorkspaceDiffFile) return null;
    return bridge as Required<Pick<M5BridgeV3,
      "getWorkspaceTree" | "closeWorkspace" | "readWorkspaceFile" | "beginWorkspaceFileSave"
      | "appendWorkspaceFileSaveChunk" | "commitWorkspaceFileSave" | "watchWorkspaceFiles"
      | "getWorkspaceDiffSummary" | "getWorkspaceDiffFile"
    >>;
  }

  #awaitStream(streamId: string, expectedBytes: number, maxBytes: number): Promise<string> {
    if (Date.now() < this.#pendingOverflowUntil) {
      this.#discardPendingStream(streamId);
      return Promise.reject(new Error("pre-descriptor stream buffer overflow"));
    }
    if (expectedBytes < 0 || expectedBytes > maxBytes) {
      this.#discardPendingStream(streamId);
      return Promise.reject(new Error(`stream ${streamId} has an unsafe byteLength`));
    }
    const stream = this.#adoptPendingStream(streamId);
    if (stream.expectedBytes !== null && stream.expectedBytes !== expectedBytes) {
      this.#failStream(streamId, new Error(`stream ${streamId} descriptor byteLength changed`));
    }
    if (stream.maxBytes !== null && stream.maxBytes !== maxBytes) {
      this.#failStream(streamId, new Error(`stream ${streamId} stream limit changed`));
    }
    stream.expectedBytes = expectedBytes;
    stream.maxBytes = maxBytes;
    return new Promise<string>((resolve, reject) => {
      stream.resolve = resolve;
      stream.reject = reject;
      stream.timeout = setTimeout(() => {
        this.#failStream(streamId, new Error(`stream ${streamId} did not reach its declared byteLength`));
      }, STREAM_TIMEOUT_MS);
      this.#settleStream(streamId);
    });
  }

  #settleStream(streamId: string): void {
    const stream = this.#streams.get(streamId);
    if (!stream || stream.expectedBytes === null) return;
    if (stream.failure) {
      if (stream.timeout) clearTimeout(stream.timeout);
      stream.reject?.(stream.failure);
      this.#streams.delete(streamId);
      return;
    }
    if (stream.byteLength > stream.expectedBytes) {
      this.#failStream(streamId, new Error(`stream ${streamId} exceeds descriptor byteLength`));
      return;
    }
    if (stream.byteLength !== stream.expectedBytes) return;
    if (stream.timeout) clearTimeout(stream.timeout);
    stream.resolve?.(stream.chunks.join(""));
    if (stream.resolve) this.#streams.delete(streamId);
  }

  #failStream(streamId: string, error: Error): void {
    const stream = this.#streams.get(streamId);
    if (!stream) return;
    stream.failure = error;
    this.#settleStream(streamId);
  }

  #isCurrent(projectId: string, epoch: number): boolean {
    return !this.#disposed && this.#state.project?.id === projectId && this.#projectEpoch === epoch;
  }

  #isLiveGeneration(generation: number): boolean {
    return !this.#disposed && this.#controllerGeneration === generation;
  }

  #isLiveOperation(operation: SnapshotCreationOperation): boolean {
    return this.#isLiveGeneration(operation.controllerGeneration);
  }

  #tab(path: string): EditorTab | undefined {
    return this.#state.tabs.find((tab) => tab.path === path);
  }

  #replaceTab(path: string, replacement: EditorTab): void {
    this.#set({ tabs: this.#state.tabs.map((tab) => tab.path === path ? replacement : tab) });
  }

  #setNotice(notice: EditorNoticeKey): void {
    this.#set(editorNotice(notice));
  }

  #setError(message: EditorErrorKey, error: unknown): void {
    this.#set({ error: errorDetail(message, error) });
  }

  #set(update: Partial<EditorSnapshot>): void {
    if (this.#disposed) return;
    this.#state = { ...this.#state, ...update, ...("notice" in update ? { noticeKey: update.notice ? update.noticeKey : undefined } : {}), ...(update.panel ? { panelRequest: { panel: update.panel } } : {}) };
    for (const listener of this.#listeners) listener();
  }
}

export function splitUtf8(text: string, maxBytes: number): string[] {
  if (text.length === 0) return [];
  const chunks: string[] = [];
  let start = 0;
  let offset = 0;
  let currentBytes = 0;
  for (const character of text) {
    // for..of keeps valid surrogate pairs together. Lone surrogates cost
    // three bytes (TextEncoder's U+FFFD replacement), just like other BMP
    // characters here. Avoid allocating a Uint8Array for every character.
    const point = character.codePointAt(0)!;
    const bytes = point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
    if (bytes > maxBytes) throw new Error("a Unicode character exceeds the protocol chunk limit");
    if (currentBytes + bytes > maxBytes) {
      chunks.push(text.slice(start, offset));
      start = offset;
      currentBytes = 0;
    }
    offset += character.length;
    currentBytes += bytes;
  }
  if (offset > start) chunks.push(text.slice(start));
  return chunks;
}

function basename(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

function isPickerCancelled(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}

function sameWorkspace(left: ChatWorkspaceRefV1 | null | undefined, right: ChatWorkspaceRefV1 | null | undefined): boolean {
  if (left === null || left === undefined || right === null || right === undefined) return left === right;
  return left.chatId === right.chatId && left.projectId === right.projectId && left.worktreeId === right.worktreeId && left.workspaceIdentity === right.workspaceIdentity;
}

function errorDetail(message: EditorErrorKey, error: unknown): { message: string; messageKey: EditorErrorKey; technicalDetail: string } {
  const technicalDetail = error instanceof Error ? error.message : String(error);
  return { message: ko[message], messageKey: message, technicalDetail: technicalDetail.slice(0, 2048) };
}

function isBlobChunkEvent(event: EventV1): event is EventV1 & { event: "blob.chunk"; payload: { text: string } } {
  return event.protocolVersion === 1
    && event.kind === "event"
    && event.event === "blob.chunk"
    && typeof event.sidecarInstanceId === "string"
    && event.sidecarInstanceId.length > 0
    && typeof event.streamId === "string"
    && event.streamId.length > 0
    && Number.isSafeInteger(event.seq)
    && event.seq >= 1
    && typeof event.payload?.text === "string";
}
