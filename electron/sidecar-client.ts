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
  G3_CAPABILITIES_V1,
  parseSystemStatusV1,
  type SystemStatusV1,
} from "../src/m5/contracts/lifecycle-v1.ts";

export const SIDECAR_READY_TIMEOUT_MS = 3_000;

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
}

export type SidecarSpawnV1 = (
  command: string,
  args: readonly string[],
  options: SpawnOptionsWithoutStdio,
) => SidecarProcessV1;

interface PendingRequestV1 {
  method: "system.status" | "app.prepareQuit";
  resolve: (value: SystemStatusV1 | PrepareQuitResultV1) => void;
  reject: (error: Error) => void;
}

export class SidecarClientV1 {
  readonly #launch: SidecarLaunchV1;
  readonly #spawn: SidecarSpawnV1;
  readonly #readyTimeoutMs: number;
  readonly #session = new ProtocolSessionV1();
  readonly #pending = new Map<string, PendingRequestV1>();
  readonly #listeners = new Set<(event: EventV1) => void>();
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

  constructor(
    launch: SidecarLaunchV1,
    options: { spawn?: SidecarSpawnV1; readyTimeoutMs?: number } = {},
  ) {
    this.#launch = launch;
    this.#spawn = options.spawn ?? ((command, args, spawnOptions) =>
      spawn(command, args, spawnOptions) as SidecarProcessV1);
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
      if (!process.stdin || !process.stdout || !process.stderr) {
        this.#fail(new Error("sidecar stdio pipes are unavailable"));
        return this.#readyPromise;
      }
      this.#process = process;
      process.stdout.on("data", (chunk: Buffer) => this.#onStdout(Buffer.from(chunk)));
      process.stderr.on("data", () => {
        // stderr is diagnostic evidence only. It must never become a protocol channel.
      });
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

  onEvent(listener: (event: EventV1) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  async getSystemStatus(): Promise<SystemStatusV1> {
    return this.#request("system.status") as Promise<SystemStatusV1>;
  }

  async prepareQuit(): Promise<PrepareQuitResultV1> {
    return this.#request("app.prepareQuit") as Promise<PrepareQuitResultV1>;
  }

  #request(method: "system.status" | "app.prepareQuit"): Promise<SystemStatusV1 | PrepareQuitResultV1> {
    if (!this.#ready) return Promise.reject(this.#failure ?? new Error("sidecar is not ready"));
    if (this.#failure) return Promise.reject(this.#failure);
    const process = this.#process;
    if (!process?.stdin?.writable) return Promise.reject(new Error("sidecar stdin is unavailable"));

    const requestId = `m5_${randomUUID()}`;
    const request: RequestV1 = {
      protocolVersion: 1,
      kind: "request",
      sidecarInstanceId: this.#ready.sidecarInstanceId,
      requestId,
      method: method as MethodV1,
      params: {},
    };

    try {
      this.#session.observe(request);
      const encoded = encodeJsonl(request);
      return new Promise<SystemStatusV1 | PrepareQuitResultV1>((resolve, reject) => {
        this.#pending.set(requestId, { method, resolve, reject });
        process.stdin?.write(encoded, "utf8", (error) => {
          if (error) {
            this.#pending.delete(requestId);
            reject(asError(error, "sidecar request write failed"));
          }
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

  #onEnvelope(envelope: EnvelopeV1): void {
    this.#session.observe(envelope);
    if (envelope.kind === "event") {
      if (envelope.event === "system.ready") {
        const ready = parseReadyPayload(envelope);
        if (!sameStrings(ready.capabilities, G3_CAPABILITIES_V1)) {
          throw new Error("sidecar ready capabilities do not match G3");
        }
        this.#ready = ready;
        this.#lastReady = ready;
        clearTimeout(this.#readyTimer);
        this.#resolveReady?.(ready);
        this.#resolveReady = undefined;
        this.#rejectReady = undefined;
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
      const status = parseSystemStatusV1(response.result);
      this.#pending.delete(response.requestId);
      pending.resolve(status);
      return;
    }
    const result = parsePrepareQuitResult(response);
    this.#pending.delete(response.requestId);
    pending.resolve(result);
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
  }
}

function sameStrings(actual: readonly string[], expected: readonly string[]): boolean {
  return actual.length === expected.length && actual.every((value, index) => value === expected[index]);
}

function asError(value: unknown, fallback: string): Error {
  return value instanceof Error ? value : new Error(`${fallback}: ${String(value)}`);
}
