// D1 (2026-11-26 integration check): more open text streams than the client
// allows must fail only the extra request. It used to throw inside the stdout
// handler, which marked the sidecar protocolInvalid, restarted the engine and
// stopped the running agent.
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import test from "node:test";

import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";
import { G5_CAPABILITIES_V1 } from "../src/m5/contracts/run-recovery-v1.ts";
import { SidecarClientV1, SidecarStreamCapacityError, type SidecarProcessV1 } from "../electron/sidecar-client.ts";

class FixtureSidecar extends EventEmitter {
  readonly stdout = new PassThrough();
  readonly stderr = new PassThrough();
  readonly writes: string[] = [];
  terminations = 0;
  readonly stdin = {
    writable: true,
    write: (chunk: Buffer | string, _encoding: string, callback: (error?: Error | null) => void) => {
      this.writes.push(Buffer.from(chunk).toString("utf8"));
      callback();
      return true;
    },
  } as unknown as SidecarProcessV1["stdin"];
  terminate() { this.terminations += 1; }
}

const SIDECAR = "sidecar_test_01";
const REF = { chatId: "chat_01", projectId: "project_01", worktreeId: "worktree_01", workspaceIdentity: "workspace_01" };
// sha256("hello")
const HELLO = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

async function startedClient() {
  const child = new FixtureSidecar();
  const client = new SidecarClientV1(
    { command: "fixture", args: ["--stdio"], cwd: "/private/tmp" },
    { readyTimeoutMs: 1_000, spawn: () => child as unknown as SidecarProcessV1 },
  );
  const started = client.start();
  child.stdout.write(`${JSON.stringify({
    protocolVersion: 1, kind: "event", sidecarInstanceId: SIDECAR, streamId: "system", seq: 1, event: "system.ready",
    payload: { protocolVersion: 1, sidecarSchemaVersion: 1, appSchemaVersion: 1, pid: 42, platform: "darwin",
      capabilities: [...G5_CAPABILITIES_V1], sidecarInstanceId: SIDECAR },
  })}\n`);
  await started;
  return { child, client };
}

function lastRequest(child: FixtureSidecar) {
  return JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string; method: string };
}

function respond(child: FixtureSidecar, requestId: string, result: unknown) {
  child.stdout.write(`${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: SIDECAR, requestId, ok: true, result })}\n`);
}

function chunk(child: FixtureSidecar, streamId: string, event: "blob.chunk" | "workspace.blob.chunk", text: string, seq = 1) {
  const payload = event === "blob.chunk" ? { text } : { ...REF, text };
  child.stdout.write(`${JSON.stringify({ protocolVersion: 1, kind: "event", sidecarInstanceId: SIDECAR, streamId, seq, event, payload } satisfies EventV1)}\n`);
}

/** Send a project file read and its descriptor, holding the chunks back. */
async function openProjectRead(child: FixtureSidecar, client: SidecarClientV1, streamId: string) {
  const read = client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  respond(child, lastRequest(child).requestId, {
    projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 5, version: HELLO,
    stream: { streamId, chunkBytes: 262144 },
  });
  return read.then(() => "ok" as const, (error: unknown) => error);
}

test("17 or more open text streams fail only the extra reads and never restart the engine", async () => {
  const { child, client } = await startedClient();
  const outcomes: unknown[] = [];
  for (let index = 0; index < 20; index += 1) outcomes.push(await openProjectRead(child, client, `read_${index}`));
  assert.deepEqual(outcomes.slice(0, 16), Array(16).fill("ok"));
  for (const refused of outcomes.slice(16)) assert.ok(refused instanceof SidecarStreamCapacityError, String(refused));
  assert.equal(client.failure, undefined, "a capacity refusal is not a protocol violation");
  assert.equal(child.terminations, 0, "the engine keeps running");

  // The sidecar still sends every refused stream's chunks: they are dropped quietly.
  for (let index = 0; index < 20; index += 1) chunk(child, `read_${index}`, "blob.chunk", "hello");
  assert.equal(client.failure, undefined);
  assert.equal(child.terminations, 0);

  // Once the streams settle, reads work again.
  assert.equal(await openProjectRead(child, client, "read_after"), "ok");
  chunk(child, "read_after", "blob.chunk", "hello");
  assert.equal(client.failure, undefined);
});

test("a refused stream's chunks are still bounded: oversize and replays remain protocol failures", async () => {
  {
    const { child, client } = await startedClient();
    for (let index = 0; index < 17; index += 1) await openProjectRead(child, client, `read_${index}`);
    chunk(child, "read_16", "blob.chunk", "hello world");
    assert.match(String(client.failure?.message), /exceeds declared stream size/);
    assert.equal(child.terminations, 1);
  }
  {
    const { child, client } = await startedClient();
    for (let index = 0; index < 17; index += 1) await openProjectRead(child, client, `read_${index}`);
    chunk(child, "read_16", "blob.chunk", "hello");
    chunk(child, "read_16", "blob.chunk", "x", 2);
    assert.match(String(client.failure?.message), /unknown or completed stream/, "a completed refused stream is forgotten");
  }
  {
    const { child, client } = await startedClient();
    for (let index = 0; index < 17; index += 1) await openProjectRead(child, client, `read_${index}`);
    // Reusing a refused stream id is a duplicate, as for an accepted one.
    await openProjectRead(child, client, "read_16");
    assert.match(String(client.failure?.message), /duplicate sidecar text stream/);
  }
});

// The leak itself (task-workspace chunks never settling their streams) is
// covered in m5-electron-bridge.test.mts since PR #15.

test("chunks of a refused task-workspace read are not forwarded to the renderer", async () => {
  const { child, client } = await startedClient();
  const seen: unknown[] = [];
  client.onWorkspaceEvent(event => seen.push(event));
  // Hold 16 project streams open, then a workspace read is refused.
  for (let index = 0; index < 16; index += 1) await openProjectRead(child, client, `read_${index}`);
  const read = client.readWorkspaceFile({ ...REF, path: "src/App.tsx" });
  respond(child, lastRequest(child).requestId, {
    ...REF, path: "src/App.tsx", classification: "utf8", byteLength: 5, version: HELLO,
    stream: { streamId: "workspace_refused", chunkBytes: 262144 },
  });
  await assert.rejects(read, SidecarStreamCapacityError);
  chunk(child, "workspace_refused", "workspace.blob.chunk", "hello");
  assert.equal(seen.length, 0);
  assert.equal(client.failure, undefined);
});

test("an empty stream holds no slot, even when the others are all open", async () => {
  const { child, client } = await startedClient();
  for (let index = 0; index < 16; index += 1) await openProjectRead(child, client, `read_${index}`);
  const read = client.readFile({ projectId: "project_01", path: "empty.txt" });
  respond(child, lastRequest(child).requestId, {
    projectId: "project_01", path: "empty.txt", classification: "utf8", byteLength: 0,
    version: "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    stream: { streamId: "read_empty", chunkBytes: 262144 },
  });
  await read;
  assert.equal(client.failure, undefined);
});
