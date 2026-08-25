import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { PassThrough, Writable } from "node:stream";
import { resolve } from "node:path";
import test from "node:test";

import { MAX_CONTROL_LINE_BYTES, type EventV1 } from "../src/m5/contracts/protocol-v1.ts";
import { SidecarClientV1, type SidecarProcessV1 } from "../electron/sidecar-client.ts";

class FixtureSidecar extends EventEmitter {
  readonly stdout = new PassThrough();
  readonly stderr = new PassThrough();
  readonly writes: string[] = [];
  readonly stdin = new Writable({
    write: (chunk, _encoding, callback) => {
      this.writes.push(Buffer.from(chunk).toString("utf8"));
      callback();
    },
  });
}

function createClient(timeout = 1_000) {
  const child = new FixtureSidecar();
  let spawns = 0;
  const client = new SidecarClientV1(
    { command: "fixture", args: ["--stdio"], cwd: "/private/tmp" },
    {
      readyTimeoutMs: timeout,
      spawn: () => {
        spawns += 1;
        return child as unknown as SidecarProcessV1;
      },
    },
  );
  return { child, client, spawns: () => spawns };
}

function readyLine() {
  return JSON.stringify({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId: "system",
    seq: 1,
    event: "system.ready",
    payload: {
      protocolVersion: 1,
      sidecarSchemaVersion: 1,
      appSchemaVersion: 1,
      pid: 42,
      platform: "darwin",
      capabilities: ["system.status", "app.prepareQuit"],
      sidecarInstanceId: "sidecar_test_01",
    },
  });
}

function eventLine(event: EventV1) {
  return `${JSON.stringify(event)}\n`;
}

test("sidecar waits for ready, accepts chunked JSONL, and exposes only ordered events", async () => {
  const { child, client } = createClient();
  const start = client.start();
  const ready = `${readyLine()}\n`;
  child.stdout.write(ready.slice(0, 20));
  child.stdout.write(ready.slice(20));
  await start;

  const received: EventV1[] = [];
  const unsubscribe = client.onEvent((event) => received.push(event));
  child.stdout.write(eventLine({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId: "run_stream_01",
    seq: 1,
    event: "run.event",
    chatId: "chat_01",
    taskId: "task_01",
    runId: "run_01",
    payload: { text: "first" },
  }));
  unsubscribe();
  child.stdout.write(eventLine({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId: "run_stream_01",
    seq: 2,
    event: "run.finished",
    chatId: "chat_01",
    taskId: "task_01",
    runId: "run_01",
    payload: { status: "completed" },
  }));
  assert.equal(received.length, 1);
  assert.equal(received[0]?.event, "run.event");
});

test("ready timeout, pre-ready stderr exit, and oversized partial output fail closed", async () => {
  const timeout = createClient(10);
  await assert.rejects(timeout.client.start(), /within 3 seconds/);

  const preReady = createClient();
  const starting = preReady.client.start();
  preReady.child.stderr.write("migration failed\n");
  preReady.child.emit("exit", 70, null);
  await assert.rejects(starting, /sidecar exited/);

  const oversized = createClient();
  const oversizedStart = oversized.client.start();
  oversized.child.stdout.write(Buffer.alloc(MAX_CONTROL_LINE_BYTES + 1, 0x61));
  await assert.rejects(oversizedStart, /oversized or unterminated/);
});

test("unknown response and abrupt exit reject pending work without restart or retry", async () => {
  const { child, client, spawns } = createClient();
  const start = client.start();
  child.stdout.write(`${readyLine()}\n`);
  await start;

  const pending = client.getSystemStatus();
  const request = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(`${JSON.stringify({
    protocolVersion: 1,
    kind: "response",
    sidecarInstanceId: "sidecar_test_01",
    requestId: `${request.requestId}_wrong`,
    ok: true,
    result: {},
  })}\n`);
  await assert.rejects(pending, /unknown requestId|no pending request/);
  assert.equal(spawns(), 1);
  await assert.rejects(client.getSystemStatus(), /unknown requestId|no pending request/);
  assert.equal(child.writes.length, 1, "the failed request must not be retried automatically");
});

test("abrupt sidecar exit rejects every pending request", async () => {
  const { child, client } = createClient();
  const start = client.start();
  child.stdout.write(`${readyLine()}\n`);
  await start;
  const pending = client.prepareQuit();
  child.emit("exit", 9, null);
  await assert.rejects(pending, /sidecar exited/);
});

test("main, preload, and package boundary retain fixed channels, origin checks, and an external sidecar", () => {
  const main = source("electron/main.ts");
  const sidecarClient = source("electron/sidecar-client.ts");
  const preload = source("electron/preload.ts");
  const forge = source("electron/forge.config.ts");
  const smoke = source("scripts/m5-electron-package-smoke.mjs");

  for (const required of [
    "event.sender !== window.webContents",
    "event.senderFrame !== event.sender.mainFrame",
    "isAllowedNavigation(event.senderFrame.url)",
    "process.resourcesPath",
    "hello-agent-sidecar",
    "args: [\"--stdio\"]",
    "app.getPath(\"userData\")",
    "com.wonseokhan.helloagent",
    "app.on(\"before-quit\"",
    "if (quitInFlight) return quitInFlight",
  ]) assert.ok(main.includes(required), `missing ${required}`);
  assert.match(sidecarClient, /shell: false/);
  assert.doesNotMatch(main, /process\.env/);
  assert.match(preload, /ipcRenderer\.invoke\(M5_INTERNAL_CHANNELS_V1\.systemGetStatus\)/);
  assert.match(preload, /ipcRenderer\.invoke\(M5_INTERNAL_CHANNELS_V1\.appPrepareQuit\)/);
  assert.match(preload, /ipcRenderer\.on\(M5_INTERNAL_CHANNELS_V1\.sidecarEvent, wrapped\)/);
  assert.match(preload, /removeListener\(M5_INTERNAL_CHANNELS_V1\.sidecarEvent, wrapped\)/);
  assert.doesNotMatch(preload, /exposeInMainWorld\([^\n]+ipcRenderer|sendSync|sendTo|postMessage/);
  assert.match(forge, /packageAfterCopy/);
  assert.match(forge, /m5-stage-sidecar\.mjs/);
  assert.match(smoke, /sidecar/);
  assert.match(smoke, /must remain outside app\.asar/);
});

function source(path: string) {
  return readFileSync(resolve(path), "utf8");
}
