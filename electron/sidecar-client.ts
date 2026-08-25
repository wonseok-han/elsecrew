import { randomUUID } from "node:crypto";
import type { ChildProcess, SpawnOptionsWithoutStdio } from "node:child_process";
import { spawn } from "node:child_process";
import { TextDecoder } from "node:util";

import {
  MAX_CONTROL_LINE_BYTES,
  ProtocolSessionV1,
  decodeJsonl,
  encodeJsonl,
  parsePrepareQuitResult,
  parseReadyPayload,
  type EnvelopeV1,
  type EventV1,
  type MethodV1,
  type PrepareQuitResultV1,
  type ReadyPayloadV1,
  type RequestV1,
  type ResponseV1,
} from "../src/m5/contracts/protocol-v1.ts";
import {
  parseG4SystemStatusV1,
  type SystemStatusV1,
} from "../src/m5/contracts/lifecycle-v1.ts";
import {
  G4_CAPABILITIES_V1,
  validateProjectCloseResultV1,
  validateG4SafetyErrorV1,
  validateProjectIdentityV1,
  validateTreePageV1,
  validateWatchEventV1,
  validateWorktreeManifestV1,
  type ProjectCloseResultV1,
  type ProjectIdentityV1,
  type TreePageV1,
  type WatchEventV1,
  type WorktreeManifestV1,
} from "../src/m5/contracts/project-v1.ts";
import {
  sha256FileVersionV1,
  validateFileReadDescriptorV1,
  validateFileVersionV1,
  type FileReadDescriptorV1,
  type FileVersionV1,
  type SaveChunkV1,
  type SaveBeginV1,
  type SaveCommitV1,
} from "../src/m5/contracts/file-v1.ts";
import {
  validateDiffFileDescriptorV1,
  validateDiffSummaryV1,
  type DiffFileDescriptorV1,
  type DiffSummaryV1,
} from "../src/m5/contracts/diff-v1.ts";

export const SIDECAR_READY_TIMEOUT_MS = 3_000;
export const MAX_SIDECAR_STDERR_TAIL_BYTES = 32 * 1024;
const MAX_FILE_STREAM_BYTES = 5 * 1024 * 1024;
const MAX_DIFF_STREAM_BYTES = 10 * 1024 * 1024;
const MAX_ACTIVE_STREAMS = 16;
const MAX_ACTIVE_STREAM_BYTES = 160 * 1024 * 1024;
const MAX_STREAM_EVENTS = 256;

export interface SidecarLaunchV1 {
  command: string;
  args: readonly string[];
  cwd: string;
}

export interface SidecarProcessV1 {
  readonly stdin: ChildProcess["stdin"];
  readonly stdout: ChildProcess["stdout"];
  readonly stderr: ChildProcess["stderr"];
  once(event: "error" | "exit", listener: (...args: unknown[]) => void): unknown;
  /** Must be idempotent: the client calls it at most once on protocol failure. */
  terminate(): void;
}

export type SidecarSpawnV1 = (
  command: string,
  args: readonly string[],
  options: SpawnOptionsWithoutStdio,
) => SidecarProcessV1;

interface PendingRequestV1 {
  method: MethodV1;
  saveKind: "begin" | "chunk" | "commit" | undefined;
  params: Record<string, unknown>;
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
}

interface TextStreamV1 {
  byteLength: number;
  expectedVersion: FileVersionV1 | null;
  text: string;
  events: number;
}

export class SidecarClientV1 {
  readonly #launch: SidecarLaunchV1;
  readonly #spawn: SidecarSpawnV1;
  readonly #readyTimeoutMs: number;
  readonly #session = new ProtocolSessionV1();
  readonly #pending = new Map<string, PendingRequestV1>();
  readonly #listeners = new Set<(event: EventV1) => void>();
  readonly #projectListeners = new Set<(event: WatchEventV1, protocol: EventV1) => void>();
  readonly #streams = new Map<string, TextStreamV1>();
  #streamBytes = 0;
  readonly #decoder = new TextDecoder("utf-8", { fatal: true });
  #process: SidecarProcessV1 | undefined;
  #stdout = Buffer.alloc(0);
  #ready: ReadyPayloadV1 | undefined;
  #lastReady: ReadyPayloadV1 | null = null;
  #readyPromise: Promise<ReadyPayloadV1> | undefined;
  #resolveReady: ((ready: ReadyPayloadV1) => void) | undefined;
  #rejectReady: ((error: Error) => void) | undefined;
  #readyTimer: NodeJS.Timeout | undefined;
  #failure: Error | undefined;
  #stderrTail = Buffer.alloc(0);
  #terminationAttempted = false;

  constructor(
    launch: SidecarLaunchV1,
    options: { spawn?: SidecarSpawnV1; readyTimeoutMs?: number } = {},
  ) {
    this.#launch = launch;
    this.#spawn = options.spawn ?? ((command, args, spawnOptions) => {
      const child = spawn(command, args, spawnOptions);
      return {
        stdin: child.stdin,
        stdout: child.stdout,
        stderr: child.stderr,
        once: (event, listener) => child.once(event, listener),
        terminate: () => {
          if (!child.killed) child.kill();
        },
      };
    });
    this.#readyTimeoutMs = options.readyTimeoutMs ?? SIDECAR_READY_TIMEOUT_MS;
  }

  async start(): Promise<ReadyPayloadV1> {
    if (this.#ready) return this.#ready;
    if (this.#failure) throw this.#failure;
    if (this.#readyPromise) return this.#readyPromise;

    this.#readyPromise = new Promise<ReadyPayloadV1>((resolve, reject) => {
      this.#resolveReady = resolve;
      this.#rejectReady = reject;
      this.#readyTimer = setTimeout(() => {
        this.#fail(new Error("sidecar did not emit system.ready within 3 seconds"));
      }, this.#readyTimeoutMs);
    });

    try {
      const process = this.#spawn(this.#launch.command, this.#launch.args, {
        cwd: this.#launch.cwd,
        shell: false,
        stdio: ["pipe", "pipe", "pipe"],
        windowsHide: true,
      });
      // Own the child before validating its pipes so partial spawn failures can stop it.
      this.#process = process;
      if (!process.stdin || !process.stdout || !process.stderr) {
        this.#fail(new Error("sidecar stdio pipes are unavailable"));
        return this.#readyPromise;
      }
      process.stdout.on("data", (chunk: Buffer) => this.#onStdout(Buffer.from(chunk)));
      process.stderr.on("data", (chunk: Buffer) => this.#onStderr(Buffer.from(chunk)));
      process.once("error", (error: unknown) => this.#fail(asError(error, "sidecar failed to start")));
      process.once("exit", (code: unknown, signal: unknown) => {
        this.#fail(new Error(`sidecar exited (${String(code ?? "null")}, ${String(signal ?? "none")})`));
      });
    } catch (error) {
      this.#fail(asError(error, "sidecar failed to start"));
    }
    return this.#readyPromise;
  }

  get lastReady(): ReadyPayloadV1 | null {
    return this.#lastReady;
  }

  get failure(): Error | undefined {
    return this.#failure;
  }

  failConsumerTooSlow(): void { this.#fail(new Error("consumerTooSlow: renderer event credit exhausted")); }
  failRendererDisconnected(): void { this.#fail(new Error("rendererDisconnected: renderer delivery ended before acknowledgement")); }

  get failedBeforeReady(): boolean {
    return this.#failure !== undefined && this.#lastReady === null;
  }

  /**
   * This bounded diagnostic is for a technical detail view, never a basic UI
   * message. Control characters are normalized, but this is not token or
   * secret redaction and callers must not claim that it is.
   */
  get technicalDetail(): string {
    return normalizeDiagnostic(this.#stderrTail);
  }

  onEvent(listener: (event: EventV1) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  onProjectEvent(listener: (event: WatchEventV1, protocol: EventV1) => void): () => void {
    this.#projectListeners.add(listener);
    return () => this.#projectListeners.delete(listener);
  }

  async getSystemStatus(): Promise<SystemStatusV1> {
    return this.#request("system.status", {}) as Promise<SystemStatusV1>;
  }

  async prepareQuit(): Promise<PrepareQuitResultV1> {
    return this.#request("app.prepareQuit", {}) as Promise<PrepareQuitResultV1>;
  }

  openProject(selectedPath: string): Promise<ProjectIdentityV1> { return this.#request("project.open", { selectedPath }) as Promise<ProjectIdentityV1>; }
  getProjectTree(input: { projectId: string; parentPath: string; cursor: string | null }): Promise<TreePageV1> { return this.#request("project.getTree", input) as Promise<TreePageV1>; }
  createProjectWorktree(input: { projectId: string }): Promise<WorktreeManifestV1> { return this.#request("project.createWorktree", input) as Promise<WorktreeManifestV1>; }
  closeProject(input: { projectId: string }): Promise<ProjectCloseResultV1> { return this.#request("project.close", input) as Promise<ProjectCloseResultV1>; }
  readFile(input: { projectId: string; path: string }): Promise<FileReadDescriptorV1> { return this.#request("file.read", input) as Promise<FileReadDescriptorV1>; }
  beginFileSave(input: Omit<SaveBeginV1, "saveId" | "chunkBytes">): Promise<{ saveId: string; chunkBytes: SaveBeginV1["chunkBytes"] }> { return this.#request("file.save", input) as Promise<{ saveId: string; chunkBytes: SaveBeginV1["chunkBytes"] }>; }
  appendFileSaveChunk(input: SaveChunkV1): Promise<{ acceptedSeq: number }> { return this.#request("file.save", input) as Promise<{ acceptedSeq: number }>; }
  commitFileSave(input: SaveCommitV1): Promise<{ outcome: "committed"; version: FileVersionV1 } | { outcome: "dirtyConflict"; error: ReturnType<typeof validateG4SafetyErrorV1> }> { return this.#request("file.save", input) as Promise<{ outcome: "committed"; version: FileVersionV1 } | { outcome: "dirtyConflict"; error: ReturnType<typeof validateG4SafetyErrorV1> }>; }
  watchProjectFiles(input: { projectId: string }): Promise<{ watching: true }> { return this.#request("file.watch", input) as Promise<{ watching: true }>; }
  getDiffSummary(input: { projectId: string }): Promise<DiffSummaryV1> { return this.#request("diff.getSummary", input) as Promise<DiffSummaryV1>; }
  getDiffFile(input: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> { return this.#request("diff.getFile", input) as Promise<DiffFileDescriptorV1>; }

  #request(method: MethodV1, params: object): Promise<unknown> {
    if (!this.#ready) return Promise.reject(this.#failure ?? new Error("sidecar is not ready"));
    if (this.#failure) return Promise.reject(this.#failure);
    const process = this.#process;
    if (!process?.stdin?.writable) return Promise.reject(new Error("sidecar stdin is unavailable"));

    let requestParams: Record<string, unknown>;
    try {
      requestParams = asRequestParams(params);
    } catch (error) {
      return Promise.reject(asError(error, "invalid sidecar request params"));
    }

    const requestId = `m5_${randomUUID()}`;
    const request: RequestV1 = {
      protocolVersion: 1,
      kind: "request",
      sidecarInstanceId: this.#ready.sidecarInstanceId,
      requestId,
      method,
      params: requestParams,
    };

    try {
      this.#session.observe(request);
      const encoded = encodeJsonl(request);
      return new Promise<unknown>((resolve, reject) => {
        this.#pending.set(requestId, {
          method,
          saveKind: saveKindForRequest(method, requestParams),
          params: requestParams,
          resolve,
          reject,
        });
        process.stdin?.write(encoded, "utf8", (error) => {
          if (error) this.#fail(asError(error, "sidecar request write failed"));
        });
      });
    } catch (error) {
      this.#fail(asError(error, "invalid sidecar request"));
      return Promise.reject(this.#failure);
    }
  }

  #onStdout(chunk: Buffer): void {
    if (this.#failure) return;
    this.#stdout = Buffer.concat([this.#stdout, chunk]);

    let lineEnd = this.#stdout.indexOf(0x0a);
    while (lineEnd !== -1) {
      const rawLine = this.#stdout.subarray(0, lineEnd);
      this.#stdout = this.#stdout.subarray(lineEnd + 1);
      if (rawLine.byteLength > MAX_CONTROL_LINE_BYTES) {
        this.#fail(new Error("sidecar stdout line exceeds 1 MiB"));
        return;
      }
      try {
        const line = this.#decoder.decode(rawLine, { stream: false });
        const envelope = decodeJsonl(line);
        this.#onEnvelope(envelope);
      } catch (error) {
        this.#fail(asError(error, "invalid sidecar stdout protocol"));
        return;
      }
      lineEnd = this.#stdout.indexOf(0x0a);
    }
    if (this.#stdout.byteLength > MAX_CONTROL_LINE_BYTES) {
      this.#fail(new Error("sidecar stdout has an oversized or unterminated line"));
    }
  }

  #onStderr(chunk: Buffer): void {
    this.#stderrTail = Buffer.concat([this.#stderrTail, chunk]);
    if (this.#stderrTail.byteLength > MAX_SIDECAR_STDERR_TAIL_BYTES) {
      this.#stderrTail = this.#stderrTail.subarray(
        this.#stderrTail.byteLength - MAX_SIDECAR_STDERR_TAIL_BYTES,
      );
    }
  }

  #onEnvelope(envelope: EnvelopeV1): void {
    this.#session.observe(envelope);
    if (envelope.kind === "event") {
      if (envelope.event === "system.ready") {
        const ready = parseReadyPayload(envelope);
        if (!sameStrings(ready.capabilities, G4_CAPABILITIES_V1)) {
          throw new Error("sidecar ready capabilities do not match G4");
        }
        this.#ready = ready;
        this.#lastReady = ready;
        clearTimeout(this.#readyTimer);
        this.#resolveReady?.(ready);
        this.#resolveReady = undefined;
        this.#rejectReady = undefined;
      }
      if (envelope.event === "blob.chunk") this.#acceptChunk(envelope);
      if (["file.changed", "file.conflict", "diff.changed"].includes(envelope.event)) {
        const watch = validateWatchEventV1(envelope.payload);
        for (const listener of this.#projectListeners) listener(watch, envelope);
      }
      for (const listener of this.#listeners) listener(envelope);
      return;
    }
    if (envelope.kind !== "response") {
      throw new Error("sidecar emitted an unexpected request");
    }
    this.#resolveResponse(envelope);
  }

  #resolveResponse(response: ResponseV1): void {
    const pending = this.#pending.get(response.requestId);
    if (!pending) throw new Error("sidecar response has no pending request");
    if (!response.ok) {
      this.#pending.delete(response.requestId);
      pending.reject(new Error(`${response.error?.kind ?? "sidecarError"}: ${response.error?.detail ?? "unknown error"}`));
      return;
    }
    if (pending.method === "system.status") {
      const status = parseG4SystemStatusV1(response.result);
      if (status.state === "ready" && !sameReady(status.sidecar, this.#ready)) {
        throw new Error("system.status identity does not match system.ready");
      }
      this.#pending.delete(response.requestId);
      pending.resolve(status);
      return;
    }
    const result = this.#validateResult(pending, response);
    this.#pending.delete(response.requestId);
    pending.resolve(result);
  }

  #validateResult(pending: PendingRequestV1, response: ResponseV1): unknown {
    switch (pending.method) {
      case "app.prepareQuit": return parsePrepareQuitResult(response);
      case "project.open": return validateProjectIdentityV1(response.result);
      case "project.getTree": {
        const value = validateTreePageV1(response.result);
        assertResponseField(value, pending.params, "projectId");
        assertResponseField(value, pending.params, "parentPath");
        return value;
      }
      case "project.createWorktree": {
        const value = validateWorktreeManifestV1(response.result);
        assertResponseField(value, pending.params, "projectId");
        return value;
      }
      case "project.close": {
        const value = validateProjectCloseResultV1(response.result);
        assertResponseField(value, pending.params, "projectId");
        return value;
      }
      case "file.read": {
        const value = validateFileReadDescriptorV1(response.result);
        assertResponseField(value, pending.params, "projectId");
        assertResponseField(value, pending.params, "path");
        this.#registerStream(value.byteLength, value.stream, value.version, MAX_FILE_STREAM_BYTES);
        return value;
      }
      case "diff.getSummary": {
        const value = validateDiffSummaryV1(response.result);
        assertResponseField(value, pending.params, "projectId");
        return value;
      }
      case "diff.getFile": {
        const value = validateDiffFileDescriptorV1(response.result);
        assertResponseField(value, pending.params, "projectId");
        assertResponseField(value, pending.params, "path");
        // The immutable G4 diff descriptor has no digest for its rendered patch
        // stream. Its size and protocol sequence are still bounded and checked.
        this.#registerStream(value.byteLength, value.stream, null, MAX_DIFF_STREAM_BYTES);
        return value;
      }
      case "file.watch": return isWatchResult(response.result) ? { watching: true } : failResult("file.watch result is invalid");
      case "file.save": return this.#validateSaveResult(pending, response.result);
      default: return failResult(`unsupported G4 response method: ${pending.method}`);
    }
  }

  #validateSaveResult(pending: PendingRequestV1, value: unknown): unknown {
    if (typeof value !== "object" || value === null || Array.isArray(value)) return failResult("file.save result is invalid");
    const result = value as Record<string, unknown>;
    if (pending.saveKind === "begin" && validId(result.saveId) && result.chunkBytes === 256 * 1024 && Object.keys(result).length === 2) return result;
    if (pending.saveKind === "chunk" && Number.isSafeInteger(result.acceptedSeq) && Object.keys(result).length === 1) {
      if (result.acceptedSeq !== pending.params.seq) return failResult("file.save acceptedSeq does not match request");
      return result;
    }
    if (pending.saveKind === "commit" && result.outcome === "committed" && Object.keys(result).length === 2) return { outcome: "committed", version: validateFileVersionV1(result.version) };
    if (pending.saveKind === "commit" && result.outcome === "dirtyConflict" && Object.keys(result).length === 2) return { outcome: "dirtyConflict", error: validateG4SafetyErrorV1(result.error) };
    return failResult("file.save result is invalid");
  }

  #registerStream(byteLength: number, stream: { streamId: string } | null, expectedVersion: FileVersionV1 | null, limit: number): void {
    if (!stream) return;
    if (byteLength > limit) throw new Error(`sidecar text stream exceeds ${limit / 1024 / 1024} MiB client bound`);
    if (this.#streams.size >= MAX_ACTIVE_STREAMS || this.#streamBytes + byteLength > MAX_ACTIVE_STREAM_BYTES) {
      throw new Error("sidecar active stream bound exceeded");
    }
    if (this.#streams.has(stream.streamId)) throw new Error("duplicate sidecar text stream");
    const empty = new Uint8Array();
    if (byteLength === 0 && expectedVersion && sha256FileVersionV1(empty) !== expectedVersion) {
      throw new Error("empty text stream hash does not match declared version");
    }
    if (byteLength === 0) return;
    this.#streams.set(stream.streamId, {
      byteLength,
      expectedVersion,
      text: "",
      events: 0,
    });
    this.#streamBytes += byteLength;
  }

  #acceptChunk(event: EventV1): void {
    const stream = this.#streams.get(event.streamId);
    if (!stream) throw new Error("blob.chunk references an unknown or completed stream");
    const text = event.payload.text;
    if (typeof text !== "string") throw new Error("blob.chunk text is invalid");
    if (++stream.events > MAX_STREAM_EVENTS) throw new Error("blob.chunk exceeds 256 events per stream");
    stream.text += text;
    const bytes = new TextEncoder().encode(stream.text);
    if (bytes.byteLength > stream.byteLength) throw new Error("blob.chunk exceeds declared stream size");
    if (bytes.byteLength === stream.byteLength) {
      if (stream.expectedVersion && sha256FileVersionV1(bytes) !== stream.expectedVersion) throw new Error("blob.chunk hash does not match declared version");
      this.#streams.delete(event.streamId);
      this.#streamBytes -= stream.byteLength;
    }
  }

  #fail(error: Error): void {
    if (this.#failure) return;
    this.#failure = error;
    clearTimeout(this.#readyTimer);
    this.#rejectReady?.(error);
    this.#resolveReady = undefined;
    this.#rejectReady = undefined;
    for (const pending of this.#pending.values()) pending.reject(error);
    this.#pending.clear();
    this.#streams.clear();
    this.#streamBytes = 0;
    this.#terminateOnce();
  }

  #terminateOnce(): void {
    if (this.#terminationAttempted) return;
    this.#terminationAttempted = true;
    try {
      this.#process?.terminate();
    } catch {
      // The original protocol failure remains the error reported to callers.
    }
  }
}

function sameStrings(actual: readonly string[], expected: readonly string[]): boolean {
  return actual.length === expected.length && actual.every((value, index) => value === expected[index]);
}

function sameReady(left: ReadyPayloadV1, right: ReadyPayloadV1 | undefined): boolean {
  return right !== undefined &&
    left.protocolVersion === right.protocolVersion &&
    left.sidecarSchemaVersion === right.sidecarSchemaVersion &&
    left.appSchemaVersion === right.appSchemaVersion &&
    left.pid === right.pid &&
    left.platform === right.platform &&
    left.sidecarInstanceId === right.sidecarInstanceId &&
    sameStrings(left.capabilities, right.capabilities);
}

function normalizeDiagnostic(value: Buffer): string {
  return value.toString("utf8")
    .replace(/\r\n?/g, "\n")
    .replace(/[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]/g, "�");
}

function asError(value: unknown, fallback: string): Error {
  return value instanceof Error ? value : new Error(`${fallback}: ${String(value)}`);
}

function failResult(detail: string): never {
  throw new Error(detail);
}

function saveKindForRequest(
  method: MethodV1,
  params: Record<string, unknown>,
): PendingRequestV1["saveKind"] {
  if (method !== "file.save") return undefined;
  if (typeof params.saveId !== "string") return "begin";
  return typeof params.text === "string" ? "chunk" : "commit";
}

function asRequestParams(value: object): Record<string, unknown> {
  if (value === null || Array.isArray(value)) throw new Error("sidecar request params must be an object");
  return value as Record<string, unknown>;
}

function isWatchResult(value: unknown): boolean {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const result = value as Record<string, unknown>;
  return Object.keys(result).length === 1 && result.watching === true;
}

function validId(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 512;
}

function assertResponseField(
  result: object,
  request: Record<string, unknown>,
  field: string,
): void {
  if ((result as Record<string, unknown>)[field] !== request[field]) {
    throw new Error(`sidecar response ${field} does not match request`);
  }
}
