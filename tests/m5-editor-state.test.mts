import assert from "node:assert/strict";
import test from "node:test";

import {
  MAX_DIFF_STREAM_BYTES,
  MAX_PENDING_STREAMS,
  MAX_RENDERED_TREE_ENTRIES,
  ProjectEditorController,
  STREAM_TIMEOUT_MS,
  splitUtf8,
  type ProjectBridge,
} from "../src/m5/editor-state.ts";
import { sha256FileVersionV1, type FileReadDescriptorV1, type FileVersionV1 } from "../src/m5/contracts/file-v1.ts";
import type { DiffFileDescriptorV1, DiffSummaryV1 } from "../src/m5/contracts/diff-v1.ts";
import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";
import { FILE_MAX_BYTES_V1, type ProjectIdentityV1, type TreePageV1, type WatchEventV1 } from "../src/m5/contracts/project-v1.ts";

const version = (text: string): FileVersionV1 => sha256FileVersionV1(new TextEncoder().encode(text));
const baseCommit = "a".repeat(40);

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(error: Error): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}

async function nextTurn(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

class FakeBridge implements ProjectBridge {
  readonly sidecarListeners = new Set<(event: EventV1) => void>();
  readonly projectListeners = new Set<(event: WatchEventV1) => void>();
  readonly appended: { seq: number; text: string }[] = [];
  openError: Error | null = null;
  classification: "utf8" | "binary" | "tooLarge" = "utf8";
  saveConflict = false;
  treeCount = 1;
  fileText = "export const greeting = '안녕';\n";
  savedText = "";
  autoReadChunks = true;
  autoDiffChunks = true;
  pendingCommit: Deferred<
    | { outcome: "committed"; version: FileVersionV1 }
    | { outcome: "dirtyConflict"; error: { code: string; kind: "dirtyConflict"; detail: string; retryable: false; outcomeUnknown: false } }
  > | null = null;

  async openProject() {
    if (this.openError) throw this.openError;
    return {
      schemaVersion: 1 as const,
      projectId: "project_01",
      selectedPath: "/work/example",
      canonicalGitRoot: "/work/example",
    };
  }

  async getProjectTree() {
    return {
      projectId: "project_01",
      parentPath: "",
      entries: Array.from({ length: this.treeCount }, (_, index) => ({
        path: `src/file-${String(index).padStart(3, "0")}.ts`,
        name: `file-${String(index).padStart(3, "0")}.ts`,
        kind: "file" as const,
      })),
      nextCursor: null,
    };
  }

  async closeProject() {
    return { projectId: "project_01", lifecycle: "abandoned" as const, physicallyDeleted: false as const };
  }

  async readFile({ path }: { projectId: string; path: string }) {
    const current = this.classification === "utf8" ? this.fileText : "";
    const currentVersion = version(current);
    if (this.classification === "utf8" && this.autoReadChunks) this.emitChunk("stream_read", current);
    return {
      projectId: "project_01",
      path,
      classification: this.classification,
      byteLength: this.classification === "tooLarge" ? 5 * 1024 * 1024 + 1 : new TextEncoder().encode(current).byteLength,
      version: currentVersion,
      stream: this.classification === "utf8" ? { streamId: "stream_read", chunkBytes: 262144 as const } : null,
    };
  }

  async beginFileSave() {
    return { saveId: "save_01", chunkBytes: 262144 as const };
  }

  async appendFileSaveChunk(input: { saveId: string; seq: number; text: string }) {
    this.appended.push({ seq: input.seq, text: input.text });
    return { acceptedSeq: input.seq };
  }

  async commitFileSave(input: { saveId: string; expectedVersion: FileVersionV1; lastSeq: number; totalBytes: number }) {
    this.savedText = this.appended.map((chunk) => chunk.text).join("");
    if (this.pendingCommit) return this.pendingCommit.promise;
    if (this.saveConflict) {
      return {
        outcome: "dirtyConflict" as const,
        error: { code: "E_DIRTY_CONFLICT", kind: "dirtyConflict" as const, detail: "file changed elsewhere", retryable: false as const, outcomeUnknown: false as const },
      };
    }
    return { outcome: "committed" as const, version: version(this.savedText) };
  }

  async watchProjectFiles() { return { watching: true as const }; }

  async getDiffSummary(): Promise<DiffSummaryV1> {
    return {
      projectId: "project_01",
      baseCommit,
      entries: [
        { kind: "renamed", path: "src/new.ts", oldPath: "src/old.ts", binary: false },
        { kind: "deleted", path: "src/removed.ts", binary: false },
        { kind: "binary", path: "assets/logo.png", binary: true },
      ],
    };
  }

  async getDiffFile({ path }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
    if (path === "src/removed.ts") {
      this.emitChunk("stream_deleted", "-removed\n");
      return {
        projectId: "project_01", kind: "deleted", path, oldPath: null,
        baseVersion: version("removed"), worktreeVersion: null, byteLength: 9,
        stream: { streamId: "stream_deleted", chunkBytes: 262144 },
      };
    }
    if (path === "assets/logo.png") {
      return {
        projectId: "project_01", kind: "binary", path, oldPath: null,
        baseVersion: version("old binary"), worktreeVersion: version("new binary"), byteLength: 10, stream: null,
      };
    }
    if (this.autoDiffChunks) this.emitChunk("stream_diff", "-old\n+new\n");
    return {
      projectId: "project_01",
      kind: "renamed" as const,
      path: "src/new.ts",
      oldPath: "src/old.ts",
      baseVersion: version("old"),
      worktreeVersion: version("new"),
      byteLength: 10,
      stream: { streamId: "stream_diff", chunkBytes: 262144 as const },
    };
  }

  onSidecarEvent(listener: (event: EventV1) => void) {
    this.sidecarListeners.add(listener);
    return () => this.sidecarListeners.delete(listener);
  }

  onProjectEvent(listener: (event: WatchEventV1) => void) {
    this.projectListeners.add(listener);
    return () => this.projectListeners.delete(listener);
  }

  emitProject(event: WatchEventV1): void {
    for (const listener of this.projectListeners) listener(event);
  }

  emitChunk(streamId: string, text: string, seq = 1): void {
    const event: EventV1 = {
      protocolVersion: 1,
      kind: "event",
      sidecarInstanceId: "sidecar_01",
      streamId,
      seq,
      event: "blob.chunk",
      payload: { text },
    };
    for (const listener of this.sidecarListeners) listener(event);
  }
}

async function openText(bridge = new FakeBridge()) {
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  await controller.openFile("src/file-000.ts");
  return { bridge, controller };
}

test("dirty 저장은 UTF-8 chunk와 새 baseVersion을 사용해 clean으로 끝난다", async () => {
  const { bridge, controller } = await openText();
  assert.equal(controller.snapshot().tabs[0]?.text, bridge.fileText);
  controller.updateText("src/file-000.ts", "const emoji = '🦦';\n");
  await controller.saveActive();

  const tab = controller.snapshot().tabs[0];
  assert.equal(tab?.saveState, "clean");
  assert.equal(bridge.savedText, "const emoji = '🦦';\n");
  assert.equal(tab?.baseVersion, version(bridge.savedText));
  assert.equal(controller.closeTab("src/file-000.ts"), true);
  controller.dispose();
});

test("stale save는 초안을 보존한 conflicted 상태로 닫기·프로젝트 전환을 막는다", async () => {
  const { bridge, controller } = await openText();
  bridge.saveConflict = true;
  controller.updateText("src/file-000.ts", "내 초안\n");
  await controller.saveActive();

  const tab = controller.snapshot().tabs[0];
  assert.equal(tab?.saveState, "conflicted");
  assert.equal(tab?.text, "내 초안\n");
  assert.equal(controller.closeTab("src/file-000.ts"), false);
  assert.equal(controller.canLeaveProject(), false);
  await controller.chooseProject();
  assert.match(controller.snapshot().notice ?? "", /프로젝트를 바꿀 수 없어요/);
  controller.dispose();
});

test("외부 파일 event는 dirty draft를 conflicted로 바꾸되 draft를 보존한다", async () => {
  const { bridge, controller } = await openText();
  controller.updateText("src/file-000.ts", "보존할 초안\n");
  bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });

  const tab = controller.snapshot().tabs[0];
  assert.equal(tab?.saveState, "conflicted");
  assert.equal(tab?.text, "보존할 초안\n");
  controller.dispose();
});

test("picker AbortError는 조용한 취소이고 다른 오류는 쉬운 오류와 technical detail을 남긴다", async () => {
  const cancelled = new FakeBridge();
  cancelled.openError = Object.assign(new Error("native picker cancelled"), { name: "AbortError" });
  const cancelledController = new ProjectEditorController(cancelled);
  cancelledController.start();
  await cancelledController.chooseProject();
  assert.equal(cancelledController.snapshot().project, null);
  assert.match(cancelledController.snapshot().notice ?? "", /취소/);

  const failed = new FakeBridge();
  failed.openError = new Error("E_NOT_GIT /tmp/not-a-project");
  const failedController = new ProjectEditorController(failed);
  failedController.start();
  await failedController.chooseProject();
  assert.match(failedController.snapshot().error?.message ?? "", /열지 못했어요/);
  assert.match(failedController.snapshot().error?.technicalDetail ?? "", /E_NOT_GIT/);
  cancelledController.dispose();
  failedController.dispose();
});

test("binary·tooLarge 파일, watcher overflow, bounded tree와 rename diff를 분명히 표시한다", async () => {
  const bridge = new FakeBridge();
  bridge.treeCount = MAX_RENDERED_TREE_ENTRIES + 37;
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  assert.equal(controller.snapshot().tree.length, MAX_RENDERED_TREE_ENTRIES);

  bridge.classification = "binary";
  await controller.openFile("assets/logo.png");
  assert.equal(controller.snapshot().tabs[0]?.mode, "binary");
  bridge.classification = "tooLarge";
  await controller.openFile("logs/large.log");
  assert.equal(controller.snapshot().tabs[1]?.mode, "tooLarge");
  bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: [], rescanRequired: true });
  assert.equal(controller.snapshot().rescanRequired, true);

  await controller.openDiff("src/new.ts");
  assert.equal(controller.snapshot().diffFile?.kind, "renamed");
  assert.equal(controller.snapshot().diffFile?.oldPath, "src/old.ts");
  assert.match(controller.snapshot().diffText, /\+new/);
  await controller.openDiff("src/removed.ts");
  assert.equal(controller.snapshot().diffFile?.kind, "deleted");
  assert.match(controller.snapshot().diffText, /-removed/);
  await controller.openDiff("assets/logo.png");
  assert.equal(controller.snapshot().diffFile?.kind, "binary");
  assert.equal(controller.snapshot().diffText, "");
  controller.dispose();
});

test("UTF-8 splitter는 문자 경계를 깨지 않고 protocol byte limit을 지킨다", () => {
  const chunks = splitUtf8("a🦦나", 4);
  assert.deepEqual(chunks, ["a", "🦦", "나"]);
  assert.ok(chunks.every((chunk) => new TextEncoder().encode(chunk).byteLength <= 4));
});

test("cross-IPC에서 descriptor 전에 온 single·multi chunk를 원자적으로 채택하고 정상 post-descriptor stream도 계속 받는다", async () => {
  class PreResponseBridge extends FakeBridge {
    override async readFile({ path }: { projectId: string; path: string }): Promise<FileReadDescriptorV1> {
      const text = "첫 파일\n";
      this.emitChunk("stream_pre_file", "첫 ", 1);
      this.emitChunk("stream_pre_file", "파일\n", 2);
      return {
        projectId: "project_01", path, classification: "utf8", byteLength: new TextEncoder().encode(text).byteLength,
        version: version(text), stream: { streamId: "stream_pre_file", chunkBytes: 262144 },
      };
    }

    override async getDiffFile({ path }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
      const text = "-before\n+after\n";
      this.emitChunk("stream_pre_diff", "-before\n", 1);
      this.emitChunk("stream_pre_diff", "+after\n", 2);
      return {
        projectId: "project_01", kind: "modified", path, oldPath: null,
        baseVersion: version("before"), worktreeVersion: version("after"), byteLength: new TextEncoder().encode(text).byteLength,
        stream: { streamId: "stream_pre_diff", chunkBytes: 262144 },
      };
    }
  }

  const bridge = new PreResponseBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  await controller.openFile("src/pre-response.ts");
  assert.equal(controller.snapshot().tabs[0]?.text, "첫 파일\n");
  await controller.openDiff("src/pre-response.ts");
  assert.equal(controller.snapshot().diffText, "-before\n+after\n");

  const postBridge = new FakeBridge();
  postBridge.autoReadChunks = false;
  postBridge.fileText = "after descriptor\n";
  const postController = new ProjectEditorController(postBridge);
  postController.start();
  await postController.chooseProject();
  const opening = postController.openFile("src/post-response.ts");
  await nextTurn();
  postBridge.emitChunk("stream_read", postBridge.fileText, 1);
  await opening;
  assert.equal(postController.snapshot().tabs[0]?.text, postBridge.fileText);
  postController.dispose();
  controller.dispose();
});

test("pending과 active stream은 256 chunk까지 허용하고 257번째는 pre/post descriptor 모두 fail-closed한다", async () => {
  class ChunkCountBridge extends FakeBridge {
    count = 256;
    preDescriptor = true;

    override async readFile({ path }: { projectId: string; path: string }): Promise<FileReadDescriptorV1> {
      const streamId = this.preDescriptor ? "stream_count_pre" : "stream_count_post";
      if (this.preDescriptor) {
        for (let index = 0; index < this.count; index += 1) this.emitChunk(streamId, "x", index + 1);
      }
      return {
        projectId: "project_01", path, classification: "utf8", byteLength: this.count,
        version: version("x".repeat(this.count)), stream: { streamId, chunkBytes: 262144 },
      };
    }
  }

  const bridge = new ChunkCountBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();

  await controller.openFile("src/pre-256.ts");
  assert.equal(controller.snapshot().tabs[0]?.text.length, 256);
  assert.equal(controller.closeTab("src/pre-256.ts"), true);

  bridge.count = 257;
  await controller.openFile("src/pre-257.ts");
  assert.equal(controller.snapshot().tabs.length, 0);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /exceeds 256 chunks/);

  bridge.preDescriptor = false;
  const opening = controller.openFile("src/post-257.ts");
  await nextTurn();
  for (let index = 0; index < bridge.count; index += 1) bridge.emitChunk("stream_count_post", "x", index + 1);
  await opening;
  assert.equal(controller.snapshot().project?.id, "project_01");
  assert.equal(controller.snapshot().tabs.length, 0);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /exceeds 256 chunks/);

  bridge.preDescriptor = true;
  bridge.count = 1;
  await controller.openFile("src/recovered.ts");
  assert.equal(controller.snapshot().tabs[0]?.text, "x");
  controller.dispose();
});

test("unknown pre-descriptor stream은 만료되고 pending stream overflow는 프로젝트·draft를 손상시키지 않고 fail-closed한다", { timeout: 10_000 }, async () => {
  const bridge = new FakeBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  for (let index = 0; index < MAX_PENDING_STREAMS; index += 1) bridge.emitChunk(`orphan_${index}`, "x");
  bridge.emitChunk("orphan_overflow", "x");

  await controller.openFile("src/blocked.ts");
  assert.equal(controller.snapshot().project?.id, "project_01");
  assert.equal(controller.snapshot().tabs.length, 0);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /pre-descriptor stream buffer overflow/);

  await new Promise<void>((resolve) => setTimeout(resolve, STREAM_TIMEOUT_MS + 30));
  await controller.openFile("src/after-expiry.ts");
  assert.equal(controller.snapshot().tabs[0]?.text, bridge.fileText);
  controller.dispose();
});

test("descriptor 뒤의 late multi-chunk stream은 byteLength와 seq를 확인한 뒤 파일과 diff에만 노출한다", async () => {
  const bridge = new FakeBridge();
  bridge.autoReadChunks = false;
  bridge.autoDiffChunks = false;
  bridge.fileText = "첫 줄\n둘째 줄\n";
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();

  const opening = controller.openFile("src/file-000.ts");
  await nextTurn();
  bridge.emitChunk("stream_read", "첫 줄\n", 1);
  bridge.emitChunk("stream_read", "둘째 줄\n", 2);
  await opening;
  assert.equal(controller.snapshot().tabs[0]?.text, bridge.fileText);

  const openingDiff = controller.openDiff("src/new.ts");
  await nextTurn();
  bridge.emitChunk("stream_diff", "-old\n", 1);
  bridge.emitChunk("stream_diff", "+new\n", 2);
  await openingDiff;
  assert.equal(controller.snapshot().diffText, "-old\n+new\n");
  controller.dispose();
});

test("diff는 별도 10MiB 상한까지 정확한 chunk 조립을 허용하지만 파일 읽기는 5MiB를 넘기지 않는다", async () => {
  class BoundedStreamBridge extends FakeBridge {
    diffBytes = MAX_DIFF_STREAM_BYTES;
    oversizedFile = false;

    override async readFile({ path }: { projectId: string; path: string }): Promise<FileReadDescriptorV1> {
      if (!this.oversizedFile) return super.readFile({ projectId: "project_01", path });
      return {
        projectId: "project_01",
        path,
        classification: "utf8",
        byteLength: FILE_MAX_BYTES_V1 + 1,
        version: version("oversized"),
        stream: { streamId: "stream_oversized_file", chunkBytes: 262144 },
      };
    }

    override async getDiffFile({ path }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
      return {
        projectId: "project_01",
        kind: "modified",
        path,
        oldPath: null,
        baseVersion: version("old"),
        worktreeVersion: version("new"),
        byteLength: this.diffBytes,
        stream: { streamId: "stream_large_diff", chunkBytes: 262144 },
      };
    }
  }

  const bridge = new BoundedStreamBridge();
  bridge.autoReadChunks = false;
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();

  const openingDiff = controller.openDiff("src/large.ts");
  await nextTurn();
  const chunk = "a".repeat(256 * 1024);
  for (let index = 0; index < MAX_DIFF_STREAM_BYTES / (256 * 1024); index += 1) {
    bridge.emitChunk("stream_large_diff", chunk, index + 1);
  }
  await openingDiff;
  assert.equal(controller.snapshot().diffText.length, MAX_DIFF_STREAM_BYTES);

  bridge.diffBytes = MAX_DIFF_STREAM_BYTES + 1;
  await controller.openDiff("src/too-large-diff.ts");
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /unsafe byteLength/);
  assert.equal(controller.snapshot().diffFile?.path, "src/large.ts");

  bridge.oversizedFile = true;
  await controller.openFile("src/too-large-file.ts");
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /unsafe byteLength/);
  assert.equal(controller.snapshot().tabs.length, 0);
  controller.dispose();
});

test("empty stream, sequence/byteLength failure는 지연 대기 또는 부분 텍스트 노출 없이 판정한다", async () => {
  const empty = new FakeBridge();
  empty.autoReadChunks = false;
  empty.fileText = "";
  const emptyController = new ProjectEditorController(empty);
  emptyController.start();
  await emptyController.chooseProject();
  await emptyController.openFile("src/file-000.ts");
  assert.equal(emptyController.snapshot().tabs[0]?.text, "");

  const malformed = new FakeBridge();
  malformed.autoReadChunks = false;
  const malformedController = new ProjectEditorController(malformed);
  malformedController.start();
  await malformedController.chooseProject();
  const opening = malformedController.openFile("src/file-000.ts");
  await nextTurn();
  malformed.emitChunk("stream_read", malformed.fileText, 2);
  await opening;
  assert.equal(malformedController.snapshot().tabs.length, 0);
  assert.match(malformedController.snapshot().error?.technicalDetail ?? "", /expected seq 1/);

  const oversized = new FakeBridge();
  oversized.autoReadChunks = false;
  const oversizedController = new ProjectEditorController(oversized);
  oversizedController.start();
  await oversizedController.chooseProject();
  const tooLong = oversizedController.openFile("src/file-000.ts");
  await nextTurn();
  oversized.emitChunk("stream_read", `${oversized.fileText}unexpected`, 1);
  await tooLong;
  assert.match(oversizedController.snapshot().error?.technicalDetail ?? "", /descriptor byteLength/);
  emptyController.dispose();
  malformedController.dispose();
  oversizedController.dispose();
});

test("saving 중 watcher conflict가 늦은 commit 성공보다 우선하며 draft와 rescan 요구를 보존한다", async () => {
  const { bridge, controller } = await openText();
  bridge.pendingCommit = deferred();
  controller.updateText("src/file-000.ts", "보존할 저장 초안\n");
  const saving = controller.saveActive();
  await nextTurn();
  assert.equal(controller.snapshot().tabs[0]?.saveState, "saving");
  bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });
  bridge.pendingCommit.resolve({ outcome: "committed", version: version("보존할 저장 초안\n") });
  await saving;
  assert.equal(controller.snapshot().tabs[0]?.saveState, "conflicted");
  assert.equal(controller.snapshot().tabs[0]?.text, "보존할 저장 초안\n");
  assert.equal(controller.snapshot().rescanRequired, true);
  controller.dispose();
});

test("controller 구독은 start/cleanup commit phase에만 생겨 StrictMode 재마운트 뒤 listener를 남기지 않는다", () => {
  const bridge = new FakeBridge();
  const controller = new ProjectEditorController(bridge);
  assert.equal(bridge.sidecarListeners.size, 0);
  assert.equal(bridge.projectListeners.size, 0);
  controller.start();
  assert.equal(bridge.sidecarListeners.size, 1);
  assert.equal(bridge.projectListeners.size, 1);
  controller.dispose();
  assert.equal(bridge.sidecarListeners.size, 0);
  assert.equal(bridge.projectListeners.size, 0);
  controller.start();
  controller.dispose();
  assert.equal(bridge.sidecarListeners.size, 0);
  assert.equal(bridge.projectListeners.size, 0);
});

test("A의 늦은 tree/read/diff 결과와 오류는 B 프로젝트 상태를 덮지 않는다", async () => {
  class EpochBridge extends FakeBridge {
    readonly projects: ProjectIdentityV1[] = [
      { schemaVersion: 1, projectId: "project_a", selectedPath: "/work/a", canonicalGitRoot: "/work/a" },
      { schemaVersion: 1, projectId: "project_b", selectedPath: "/work/b", canonicalGitRoot: "/work/b" },
    ];
    readonly trees: { projectId: string; deferred: Deferred<TreePageV1> }[] = [];
    readonly summaries: { projectId: string; deferred: Deferred<DiffSummaryV1> }[] = [];
    readonly reads: { projectId: string; deferred: Deferred<FileReadDescriptorV1> }[] = [];
    readonly diffs: { projectId: string; deferred: Deferred<DiffFileDescriptorV1> }[] = [];
    projectIndex = 0;

    async openProject(): Promise<ProjectIdentityV1> {
      return this.projects[this.projectIndex++]!;
    }

    async getProjectTree({ projectId }: { projectId: string; parentPath: string; cursor: string | null }): Promise<TreePageV1> {
      const request = { projectId, deferred: deferred<TreePageV1>() };
      this.trees.push(request);
      return request.deferred.promise;
    }

    async getDiffSummary({ projectId }: { projectId: string }): Promise<DiffSummaryV1> {
      const request = { projectId, deferred: deferred<DiffSummaryV1>() };
      this.summaries.push(request);
      return request.deferred.promise;
    }

    async readFile({ projectId }: { projectId: string; path: string }): Promise<FileReadDescriptorV1> {
      const request = { projectId, deferred: deferred<FileReadDescriptorV1>() };
      this.reads.push(request);
      return request.deferred.promise;
    }

    async getDiffFile({ projectId }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
      const request = { projectId, deferred: deferred<DiffFileDescriptorV1>() };
      this.diffs.push(request);
      return request.deferred.promise;
    }
  }

  const bridge = new EpochBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  const choosingA = controller.chooseProject();
  await nextTurn();
  const readA = controller.openFile("src/a.ts");
  const diffA = controller.openDiff("src/a.ts");
  await nextTurn();
  const choosingB = controller.chooseProject();
  await nextTurn();

  resolveTree(bridge, "project_b");
  resolveSummary(bridge, "project_b");
  await choosingB;
  assert.equal(controller.snapshot().project?.id, "project_b");

  resolveTree(bridge, "project_a");
  resolveSummary(bridge, "project_a");
  const text = "A가 늦게 도착\n";
  bridge.reads.find((request) => request.projectId === "project_a")?.deferred.resolve({
    projectId: "project_a", path: "src/a.ts", classification: "utf8", byteLength: new TextEncoder().encode(text).byteLength,
    version: version(text), stream: { streamId: "stream_a_read", chunkBytes: 262144 },
  });
  bridge.diffs.find((request) => request.projectId === "project_a")?.deferred.resolve({
    projectId: "project_a", kind: "modified", path: "src/a.ts", oldPath: null,
    baseVersion: version("old"), worktreeVersion: version(text), byteLength: new TextEncoder().encode("-old\n+new\n").byteLength,
    stream: { streamId: "stream_a_diff", chunkBytes: 262144 },
  });
  await nextTurn();
  bridge.emitChunk("stream_a_read", text);
  bridge.emitChunk("stream_a_diff", "-old\n+new\n");
  await Promise.all([choosingA, readA, diffA]);

  assert.equal(controller.snapshot().project?.id, "project_b");
  assert.equal(controller.snapshot().tabs.length, 0);
  assert.equal(controller.snapshot().diffFile, null);
  assert.equal(controller.snapshot().error, null);
  controller.dispose();
});

function resolveTree(bridge: { trees: { projectId: string; deferred: Deferred<TreePageV1> }[] }, projectId: string): void {
  const request = bridge.trees.find((item) => item.projectId === projectId);
  assert.ok(request, `missing ${projectId} tree request`);
  request.deferred.resolve({ projectId, parentPath: "", entries: [], nextCursor: null });
}

function resolveSummary(bridge: { summaries: { projectId: string; deferred: Deferred<DiffSummaryV1> }[] }, projectId: string): void {
  const request = bridge.summaries.find((item) => item.projectId === projectId);
  assert.ok(request, `missing ${projectId} summary request`);
  request.deferred.resolve({ projectId, baseCommit, entries: [] });
}
