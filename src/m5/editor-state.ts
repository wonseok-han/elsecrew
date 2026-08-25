import type { M5BridgeV2 } from "./contracts/bridge-v2.ts";
import { FILE_MAX_BYTES_V1, TREE_PAGE_MAX_ENTRIES_V1, type TreeEntryV1, type WatchEventV1 } from "./contracts/project-v1.ts";
import type { DiffFileDescriptorV1, DiffSummaryV1 } from "./contracts/diff-v1.ts";
import type { FileVersionV1 } from "./contracts/file-v1.ts";
import type { EventV1 } from "./contracts/protocol-v1.ts";

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
  tree: readonly TreeEntryV1[];
  treeParent: string;
  tabs: readonly EditorTab[];
  activePath: string | null;
  panel: WorkbenchPanel;
  diffSummary: DiffSummaryV1 | null;
  diffFile: DiffFileDescriptorV1 | null;
  diffText: string;
  rescanRequired: boolean;
  selectionBlocked: boolean;
  notice: string | null;
  error: { message: string; technicalDetail: string } | null;
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
  readonly #bridge: ProjectBridge;
  readonly #listeners = new Set<() => void>();
  readonly #streams = new Map<string, StreamAssembly>();
  readonly #pendingStreams = new Map<string, PendingStream>();
  readonly #saveGenerations = new Map<string, number>();
  readonly #snapshotCreationTimeoutMs: number;
  #pendingStreamBytes = 0;
  #pendingOverflowUntil = 0;
  #unsubscribers: (() => void)[] = [];
  #started = false;
  #disposed = false;
  #controllerGeneration = 0;
  #projectEpoch = 0;
  #selectionGeneration = 0;
  #snapshotCreation: SnapshotCreationOperation | null = null;
  #snapshotCreationBlocked: SnapshotCreationOperation | null = null;
  #state: EditorSnapshot = {
    project: null,
    tree: [],
    treeParent: "",
    tabs: [],
    activePath: null,
    panel: "files",
    diffSummary: null,
    diffFile: null,
    diffText: "",
    rescanRequired: false,
    selectionBlocked: false,
    notice: null,
    error: null,
  };

  constructor(bridge: ProjectBridge, options: ProjectEditorControllerOptions = {}) {
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
  }

  snapshot(): EditorSnapshot {
    return this.#state;
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
    }
    this.#streams.clear();
    for (const stream of this.#pendingStreams.values()) clearTimeout(stream.expiry);
    this.#pendingStreams.clear();
    this.#pendingStreamBytes = 0;
    this.#pendingOverflowUntil = 0;
    this.#listeners.clear();
  }

  async chooseProject(): Promise<void> {
    if (this.#disposed) return;
    if (!this.canLeaveProject()) {
      this.#setNotice("저장하지 않은 변경이 있어 프로젝트를 바꿀 수 없어요.");
      return;
    }
    if (this.#snapshotCreationBlocked) return;
    const selection = ++this.#selectionGeneration;
    const controllerGeneration = this.#controllerGeneration;
    try {
      const project = await this.#bridge.openProject();
      if (!this.#isLiveGeneration(controllerGeneration) || selection !== this.#selectionGeneration) return;
      if (this.#snapshotCreationBlocked) return;
      // Picker cancellation must leave the existing project surface untouched. Clear transient
      // messages only after the native picker returned a real project identity.
      this.#set({ error: null, notice: null });
      if (this.#snapshotCreation) {
        this.#setNotice("이전 프로젝트 선택을 안전하게 마무리하는 중이에요. 끝나면 다시 선택해 주세요.");
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
      this.#setError("프로젝트를 열지 못했어요. 다른 Git 프로젝트를 선택해 보세요.", error);
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
          "작업 복사본 준비가 오래 걸려 안전하게 멈췄어요. 앱을 다시 열어 상태를 확인한 뒤 다시 선택해 주세요.",
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
      if (!this.#isLiveOperation(operation) || operation.timedOut || operation.selection !== this.#selectionGeneration) {
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
        tree: [],
        treeParent: "",
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
          "작업 복사본 준비 결과를 확인할 수 없어요. 앱을 다시 열어 복구 상태를 확인해 주세요.",
          error,
        );
      } else if (this.#isLiveOperation(operation) && operation.selection === this.#selectionGeneration) {
        this.#setError("작업 복사본을 준비하지 못했어요. 원본 프로젝트는 건드리지 않았어요.", error);
      } else {
        this.#setError("이전 프로젝트 선택이 안전하게 끝났는지 확인할 수 없어요. 상태를 확인해 주세요.", error);
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
        this.#setNotice("늦게 끝난 작업 복사본을 안전하게 닫았어요. 필요하면 프로젝트를 다시 선택해 주세요.");
      } else if (operation.selection !== this.#selectionGeneration) {
        this.#setNotice("이전 프로젝트 선택을 안전하게 닫았어요. 필요하면 프로젝트를 다시 선택해 주세요.");
      } else {
        this.#setError("작업 복사본 결과를 확인하지 못했어요. 원본 프로젝트는 건드리지 않았어요.", reason);
      }
    } catch (error) {
      if (!this.#isLiveOperation(operation)) return;
      this.#blockSnapshotCreation(
        operation,
        "이전 작업 복사본을 안전하게 닫았는지 확인할 수 없어요. 앱을 다시 열어 복구 상태를 확인해 주세요.",
        error,
      );
    } finally {
      operation.resolve(null);
    }
  }

  #blockSnapshotCreation(operation: SnapshotCreationOperation, message: string, error: unknown): void {
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
    const project = this.#state.project;
    if (!project) return;
    await this.#loadTree(project.id, this.#projectEpoch, parentPath);
  }

  async #loadTree(projectId: string, epoch: number, parentPath: string): Promise<void> {
    try {
      const page = await this.#bridge.getProjectTree({ projectId, parentPath, cursor: null });
      if (!this.#isCurrent(projectId, epoch)) return;
      // The sidecar contract already caps a page at 500. Keep the renderer bounded too.
      const entries = page.entries.slice(0, MAX_RENDERED_TREE_ENTRIES);
      this.#set({ tree: entries, treeParent: page.parentPath, rescanRequired: false });
    } catch (error) {
      if (!this.#isCurrent(projectId, epoch)) return;
      this.#setError("파일 목록을 불러오지 못했어요.", error);
    }
  }

  async openFile(path: string): Promise<void> {
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
      const descriptor = await this.#bridge.readFile({ projectId: project.id, path });
      const text = descriptor.classification === "utf8" && descriptor.stream
        ? await this.#awaitStream(descriptor.stream.streamId, descriptor.byteLength, FILE_MAX_BYTES_V1)
        : "";
      if (!this.#isCurrent(project.id, epoch)) return;
      const tab: EditorTab = {
        path,
        mode: descriptor.classification === "utf8" ? "text" : descriptor.classification,
        saveState: "clean",
        baseVersion: descriptor.version,
        byteLength: descriptor.byteLength,
        text,
      };
      this.#set({ tabs: [...this.#state.tabs, tab], activePath: path, panel: "editor" });
    } catch (error) {
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#setError("파일을 열지 못했어요.", error);
    }
  }

  updateText(path: string, text: string): void {
    const tab = this.#tab(path);
    if (!tab || tab.mode !== "text" || tab.saveState === "conflicted") return;
    this.#replaceTab(path, { ...tab, text, saveState: "dirty" });
  }

  async saveActive(): Promise<void> {
    const project = this.#state.project;
    const epoch = this.#projectEpoch;
    const activePath = this.#state.activePath;
    if (activePath === null) return;
    const path = activePath;
    const tab = this.#tab(path);
    if (!project || !tab || tab.mode !== "text" || tab.saveState !== "dirty") return;
    const totalBytes = new TextEncoder().encode(tab.text).byteLength;
    if (totalBytes > FILE_MAX_BYTES_V1) {
      this.#setError("5MiB보다 큰 파일은 여기서 저장할 수 없어요.", new Error(`draft is ${totalBytes} bytes`));
      return;
    }
    const saveGeneration = (this.#saveGenerations.get(path) ?? 0) + 1;
    this.#saveGenerations.set(path, saveGeneration);
    this.#replaceTab(path, { ...tab, saveState: "saving" });
    try {
      const begin = await this.#bridge.beginFileSave({
        projectId: project.id,
        path,
        expectedVersion: tab.baseVersion,
        totalBytes,
        encoding: "utf8",
      });
      const chunks = splitUtf8(tab.text, begin.chunkBytes);
      for (const [index, text] of chunks.entries()) {
        await this.#bridge.appendFileSaveChunk({ saveId: begin.saveId, seq: index + 1, text });
      }
      const committed = await this.#bridge.commitFileSave({
        saveId: begin.saveId,
        expectedVersion: tab.baseVersion,
        lastSeq: chunks.length,
        totalBytes,
      });
      if (!this.#isCurrent(project.id, epoch)) return;
      const latest = this.#tab(path);
      if (!latest || latest.saveState !== "saving" || this.#saveGenerations.get(path) !== saveGeneration) return;
      if (committed.outcome === "dirtyConflict") {
        this.#replaceTab(path, { ...latest, saveState: "conflicted", technicalDetail: committed.error.detail });
        this.#setNotice("다른 변경이 먼저 들어왔어요. 내 초안은 보존했고 덮어쓰지는 않았어요.");
        return;
      }
      this.#replaceTab(path, { ...latest, baseVersion: committed.version, saveState: "clean", byteLength: totalBytes });
      this.#setNotice("안전하게 저장했어요.");
      await this.refreshChanges();
    } catch (error) {
      if (!this.#isCurrent(project.id, epoch)) return;
      const latest = this.#tab(path);
      if (latest?.saveState === "saving" && this.#saveGenerations.get(path) === saveGeneration) {
        this.#replaceTab(path, { ...latest, saveState: "dirty" });
      }
      this.#setError("저장하지 못했어요. 초안은 그대로 남아 있어요.", error);
    }
  }

  closeTab(path: string): boolean {
    const tab = this.#tab(path);
    if (!tab) return true;
    if (tab.saveState !== "clean") {
      this.#setNotice("저장하거나 충돌을 해결하기 전에는 이 탭을 닫을 수 없어요.");
      return false;
    }
    const tabs = this.#state.tabs.filter((item) => item.path !== path);
    const lastTab = tabs.length === 0 ? undefined : tabs[tabs.length - 1];
    this.#set({ tabs, activePath: this.#state.activePath === path ? lastTab?.path ?? null : this.#state.activePath });
    return true;
  }

  async abandonProject(): Promise<void> {
    const project = this.#state.project;
    if (!project) return;
    const epoch = this.#projectEpoch;
    if (!this.canLeaveProject()) {
      this.#setNotice("저장하지 않은 변경이나 충돌을 먼저 확인해 주세요.");
      return;
    }
    try {
      await this.#bridge.closeProject({ projectId: project.id });
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#projectEpoch += 1;
      this.#set({
        project: null,
        tree: [],
        treeParent: "",
        tabs: [],
        activePath: null,
        diffSummary: null,
        diffFile: null,
        diffText: "",
        rescanRequired: false,
        notice: "원본과 분리된 작업 복사본 연결을 닫았어요. 파일을 삭제하지는 않았어요.",
      });
    } catch (error) {
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#setError("프로젝트 연결을 닫지 못했어요.", error);
    }
  }

  async refreshChanges(): Promise<void> {
    const project = this.#state.project;
    if (!project) return;
    await this.#refreshChanges(project.id, this.#projectEpoch);
  }

  async #refreshChanges(projectId: string, epoch: number): Promise<void> {
    try {
      const diffSummary = await this.#bridge.getDiffSummary({ projectId });
      if (!this.#isCurrent(projectId, epoch)) return;
      this.#set({ diffSummary, rescanRequired: false });
    } catch (error) {
      if (!this.#isCurrent(projectId, epoch)) return;
      this.#setError("변경 목록을 불러오지 못했어요.", error);
    }
  }

  async openDiff(path: string): Promise<void> {
    const project = this.#state.project;
    if (!project) return;
    const epoch = this.#projectEpoch;
    try {
      const diffFile = await this.#bridge.getDiffFile({ projectId: project.id, path });
      const diffText = diffFile.stream
        ? await this.#awaitStream(diffFile.stream.streamId, diffFile.byteLength, MAX_DIFF_STREAM_BYTES)
        : "";
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#set({ diffFile, diffText, panel: "changes", error: null });
    } catch (error) {
      if (!this.#isCurrent(project.id, epoch)) return;
      this.#setError("이 파일의 변경 내용을 불러오지 못했어요.", error);
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
    const project = this.#state.project;
    if (!project || event.projectId !== project.id) return;
    if (event.rescanRequired) {
      this.#set({ rescanRequired: true, notice: "파일 변경이 많아 목록을 다시 확인해야 해요." });
      return;
    }
    for (const path of event.paths) {
      const tab = this.#tab(path);
      if (!tab) continue;
      if (event.event === "file.conflict" || tab.saveState === "dirty" || tab.saveState === "saving") {
        this.#replaceTab(path, { ...tab, saveState: "conflicted", technicalDetail: "external file change" });
        this.#set({ rescanRequired: true, notice: "열어 둔 파일이 밖에서 바뀌었어요. 내 초안은 보존했고 자동으로 덮어쓰지 않았어요." });
      } else {
        this.#setNotice("열어 둔 파일이 밖에서 바뀌었어요. 다시 열어 최신 내용을 확인해 주세요.");
      }
    }
    if (event.event === "diff.changed") void this.refreshChanges();
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

  #setNotice(notice: string): void {
    this.#set({ notice });
  }

  #setError(message: string, error: unknown): void {
    this.#set({ error: errorDetail(message, error) });
  }

  #set(update: Partial<EditorSnapshot>): void {
    if (this.#disposed) return;
    this.#state = { ...this.#state, ...update };
    for (const listener of this.#listeners) listener();
  }
}

export function splitUtf8(text: string, maxBytes: number): string[] {
  if (text.length === 0) return [];
  const encoder = new TextEncoder();
  const chunks: string[] = [];
  let current = "";
  let currentBytes = 0;
  for (const character of text) {
    const bytes = encoder.encode(character).byteLength;
    if (bytes > maxBytes) throw new Error("a Unicode character exceeds the protocol chunk limit");
    if (currentBytes + bytes > maxBytes) {
      chunks.push(current);
      current = "";
      currentBytes = 0;
    }
    current += character;
    currentBytes += bytes;
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}

function basename(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

function isPickerCancelled(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}

function errorDetail(message: string, error: unknown): { message: string; technicalDetail: string } {
  const technicalDetail = error instanceof Error ? error.message : String(error);
  return { message, technicalDetail: technicalDetail.slice(0, 2048) };
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
