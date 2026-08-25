import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { PassThrough } from "node:stream";
import { resolve } from "node:path";
import test from "node:test";

import { MAX_CONTROL_LINE_BYTES, type EventV1 } from "../src/m5/contracts/protocol-v1.ts";
import { G4_CAPABILITIES_V1 } from "../src/m5/contracts/project-v1.ts";
import { QuitCoordinatorV1 } from "../electron/quit-coordinator.ts";
import { SidecarClientV1, type SidecarProcessV1 } from "../electron/sidecar-client.ts";
import { validatePathInput, validateSaveBeginInput, validateSaveChunkInput, validateSaveCommitInput, validateTreeInput } from "../electron/bridge-input.ts";
import { EventCreditV1, MAX_IN_FLIGHT_EVENTS } from "../electron/event-credit.ts";

class FixtureSidecar extends EventEmitter {
  readonly stdout = new PassThrough();
  readonly stderr = new PassThrough();
  readonly writes: string[] = [];
  terminations = 0;
  writeError: Error | undefined;
  readonly stdin = {
    writable: true,
    write: (chunk: Buffer | string, _encoding: string, callback: (error?: Error | null) => void) => {
      this.writes.push(Buffer.from(chunk).toString("utf8"));
      callback(this.writeError);
      return true;
    },
  } as unknown as SidecarProcessV1["stdin"];
  terminate() {
    this.terminations += 1;
  }
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
      capabilities: [...G4_CAPABILITIES_V1],
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
  assert.equal(timeout.child.terminations, 1);

  const preReady = createClient();
  const starting = preReady.client.start();
  preReady.child.stderr.write("migration failed\n");
  preReady.child.emit("exit", 70, null);
  await assert.rejects(starting, /sidecar exited/);

  const oversized = createClient();
  const oversizedStart = oversized.client.start();
  oversized.child.stdout.write(Buffer.alloc(MAX_CONTROL_LINE_BYTES + 1, 0x61));
  await assert.rejects(oversizedStart, /oversized or unterminated/);
  assert.equal(oversized.child.terminations, 1);

  const malformed = createClient();
  const malformedStart = malformed.client.start();
  malformed.child.stdout.write("not-json\n");
  await assert.rejects(malformedStart, /invalid JSON/);
  assert.equal(malformed.child.terminations, 1);
});

test("G4 rejects a ready handshake without the exact cumulative capabilities", async () => {
  const { child, client } = createClient();
  const starting = client.start();
  const ready = JSON.parse(readyLine()) as { payload: { capabilities: string[] } };
  ready.payload.capabilities.pop();
  child.stdout.write(`${JSON.stringify(ready)}\n`);
  await assert.rejects(starting, /capabilities do not match G4/);
  assert.equal(child.terminations, 1);
});

test("a partial spawned process is terminated before any pipe listener can leak", async () => {
  const child = new FixtureSidecar();
  let spawns = 0;
  const client = new SidecarClientV1(
    { command: "fixture", args: ["--stdio"], cwd: "/private/tmp" },
    {
      spawn: () => {
        spawns += 1;
        return {
          stdin: child.stdin,
          stdout: child.stdout,
          stderr: null,
          once: (event, listener) => child.once(event, listener),
          terminate: () => child.terminate(),
        } as unknown as SidecarProcessV1;
      },
    },
  );
  await assert.rejects(client.start(), /stdio pipes are unavailable/);
  assert.equal(spawns, 1);
  assert.equal(child.terminations, 1);
  assert.equal(child.stdout.listenerCount("data"), 0);
  assert.equal(child.stderr.listenerCount("data"), 0);
  await assert.rejects(client.start(), /stdio pipes are unavailable/);
  assert.equal(spawns, 1, "partial startup is never retried automatically");
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
  assert.equal(child.terminations, 1);
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
  assert.equal(child.terminations, 1);
});

test("system.status identity mismatch and stderr tails corrupt the session without leaking control bytes", async () => {
  const identity = createClient();
  const started = identity.client.start();
  identity.child.stdout.write(`${readyLine()}\n`);
  await started;
  const pending = identity.client.getSystemStatus();
  const request = JSON.parse(identity.child.writes[0] ?? "{}") as { requestId: string };
  identity.child.stdout.write(`${JSON.stringify({
    protocolVersion: 1,
    kind: "response",
    sidecarInstanceId: "sidecar_test_01",
    requestId: request.requestId,
    ok: true,
    result: {
      state: "ready",
      sidecar: {
        protocolVersion: 1,
        sidecarSchemaVersion: 1,
        appSchemaVersion: 1,
        pid: 99,
        platform: "darwin",
        capabilities: [...G4_CAPABILITIES_V1],
        sidecarInstanceId: "sidecar_test_02",
      },
      database: { health: "healthy", schemaVersion: 1 },
      recoveryRequired: false,
      blockerIds: [],
    },
  })}\n`);
  await assert.rejects(pending, /identity does not match/);
  assert.equal(identity.child.terminations, 1);

  const diagnostics = createClient();
  const starting = diagnostics.client.start();
  diagnostics.child.stderr.write(`\u0001${"x".repeat(33 * 1024)}\r\nmigration tail`);
  diagnostics.child.emit("exit", 70, null);
  await assert.rejects(starting, /sidecar exited/);
  assert.ok(diagnostics.client.technicalDetail.endsWith("\nmigration tail"));
  assert.ok(!diagnostics.client.technicalDetail.includes("\u0001"));
  assert.ok(Buffer.byteLength(diagnostics.client.technicalDetail) <= 32 * 1024 + 2);
});

test("pre-ready startup failure permits one explicitly unclean app close without re-entering quit", () => {
  let sidecarCalls = 0;
  const exits: string[] = [];
  const coordinator = new QuitCoordinatorV1({
    prepareSidecarQuit: async () => {
      sidecarCalls += 1;
      return { safeToQuit: true, blockers: [] };
    },
    failedBeforeReady: () => true,
    allowProcessExit: (kind) => exits.push(kind),
  });
  coordinator.handleBeforeQuit();
  coordinator.handleBeforeQuit();
  assert.equal(sidecarCalls, 0, "failed startup must not falsely request a clean marker");
  assert.deepEqual(exits, ["uncleanStartupFailure"]);
  assert.equal(coordinator.exitAllowed, true);
});

test("G4 file streams correlate descriptors, chunks, hashes, and project watcher events", async () => {
  const { child, client } = createClient();
  const started = client.start();
  child.stdout.write(`${readyLine()}\n`);
  await started;
  const watched: unknown[] = [];
  const unwatch = client.onProjectEvent((event) => watched.push(event));
  const read = client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const request = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(`${JSON.stringify({
    protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId: request.requestId, ok: true,
    result: { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 5,
      version: "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      stream: { streamId: "read_stream_01", chunkBytes: 262144 } },
  })}\n`);
  await read;
  child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "read_stream_01", seq: 1, event: "blob.chunk", payload: { text: "hello" } }));
  child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "watch_stream_01", seq: 1, event: "file.changed", payload: { projectId: "project_01", event: "file.changed", paths: ["src/App.tsx"], rescanRequired: false } }));
  assert.equal(client.failure, undefined);
  assert.equal(watched.length, 1);
  unwatch();
  child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "watch_stream_01", seq: 2, event: "diff.changed", payload: { projectId: "project_01", event: "diff.changed", paths: ["src/App.tsx"], rescanRequired: false } }));
  assert.equal(watched.length, 1, "unsubscribe must remove the exact event wrapper");
});

test("G4 rejects unknown, oversized, and hash-mismatched blob streams without retry", async () => {
  const unknown = createClient();
  const unknownStart = unknown.client.start();
  unknown.child.stdout.write(`${readyLine()}\n`);
  await unknownStart;
  unknown.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "unknown_stream", seq: 1, event: "blob.chunk", payload: { text: "x" } }));
  assert.match(unknown.client.failure?.message ?? "", /unknown or completed stream/);
  assert.equal(unknown.child.terminations, 1);

  const mismatch = createClient();
  const mismatchStart = mismatch.client.start();
  mismatch.child.stdout.write(`${readyLine()}\n`);
  await mismatchStart;
  const pending = mismatch.client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const request = JSON.parse(mismatch.child.writes[0] ?? "{}") as { requestId: string };
  mismatch.child.stdout.write(`${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId: request.requestId, ok: true, result: { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 5, version: "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824", stream: { streamId: "bad_hash_stream", chunkBytes: 262144 } } })}\n`);
  await pending;
  mismatch.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "bad_hash_stream", seq: 1, event: "blob.chunk", payload: { text: "world" } }));
  assert.match(mismatch.client.failure?.message ?? "", /hash does not match/);
  assert.equal(mismatch.child.terminations, 1);

  const oversized = createClient();
  const oversizedStart = oversized.client.start();
  oversized.child.stdout.write(`${readyLine()}\n`);
  await oversizedStart;
  const oversizedRead = oversized.client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const oversizedRequest = JSON.parse(oversized.child.writes[0] ?? "{}") as { requestId: string };
  oversized.child.stdout.write(`${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId: oversizedRequest.requestId, ok: true, result: { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "too_much_text", chunkBytes: 262144 } } })}\n`);
  await oversizedRead;
  oversized.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "too_much_text", seq: 1, event: "blob.chunk", payload: { text: "xx" } }));
  assert.match(oversized.client.failure?.message ?? "", /exceeds declared stream size/);
  assert.equal(oversized.child.terminations, 1);

  const outOfOrder = createClient();
  const outOfOrderStart = outOfOrder.client.start();
  outOfOrder.child.stdout.write(`${readyLine()}\n`);
  await outOfOrderStart;
  const outOfOrderRead = outOfOrder.client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const outOfOrderRequest = JSON.parse(outOfOrder.child.writes[0] ?? "{}") as { requestId: string };
  outOfOrder.child.stdout.write(`${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId: outOfOrderRequest.requestId, ok: true, result: { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "out_of_order_text", chunkBytes: 262144 } } })}\n`);
  await outOfOrderRead;
  outOfOrder.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "out_of_order_text", seq: 2, event: "blob.chunk", payload: { text: "x" } }));
  assert.match(outOfOrder.client.failure?.message ?? "", /expected seq 1, got 2/);
  assert.equal(outOfOrder.child.terminations, 1);

  const descriptorBound = createClient();
  const descriptorStart = descriptorBound.client.start();
  descriptorBound.child.stdout.write(`${readyLine()}\n`);
  await descriptorStart;
  const diff = descriptorBound.client.getDiffFile({ projectId: "project_01", path: "src/App.tsx" });
  const diffRequest = JSON.parse(descriptorBound.child.writes[0] ?? "{}") as { requestId: string };
  descriptorBound.child.stdout.write(responseLine(diffRequest.requestId, {
    projectId: "project_01", kind: "modified", path: "src/App.tsx", oldPath: null,
    baseVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
    worktreeVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
    byteLength: 10 * 1024 * 1024 + 1, stream: { streamId: "oversized_diff", chunkBytes: 262144 },
  }));
  await assert.rejects(diff, /exceeds 10 MiB client bound/);
  assert.equal(descriptorBound.child.terminations, 1);

  const exactDiff = createClient(); const exactStart = exactDiff.client.start(); exactDiff.child.stdout.write(`${readyLine()}\n`); await exactStart;
  const exact = exactDiff.client.getDiffFile({ projectId: "project_01", path: "src/App.tsx" }); const exactRequest = JSON.parse(exactDiff.child.writes[0] ?? "{}") as { requestId: string };
  exactDiff.child.stdout.write(responseLine(exactRequest.requestId, { projectId: "project_01", kind: "modified", path: "src/App.tsx", oldPath: null, baseVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", worktreeVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", byteLength: 10 * 1024 * 1024, stream: { streamId: "exact_diff", chunkBytes: 262144 } }));
  assert.equal((await exact).byteLength, 10 * 1024 * 1024);

  const tooLargeFile = createClient(); const fileStart = tooLargeFile.client.start(); tooLargeFile.child.stdout.write(`${readyLine()}\n`); await fileStart;
  const file = tooLargeFile.client.readFile({ projectId: "project_01", path: "src/App.tsx" }); const fileRequest = JSON.parse(tooLargeFile.child.writes[0] ?? "{}") as { requestId: string };
  tooLargeFile.child.stdout.write(responseLine(fileRequest.requestId, { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 5 * 1024 * 1024 + 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "too_large_file", chunkBytes: 262144 } }));
  await assert.rejects(file, /file classification must be tooLarge/);
});

test("G4 save responses are phase-specific and duplicate responses corrupt the session", async () => {
  const valid = createClient();
  const validStart = valid.client.start();
  valid.child.stdout.write(`${readyLine()}\n`);
  await validStart;
  const begin = valid.client.beginFileSave(saveBeginInput());
  const beginRequest = JSON.parse(valid.child.writes[0] ?? "{}") as { requestId: string };
  valid.child.stdout.write(responseLine(beginRequest.requestId, { saveId: "save_01", chunkBytes: 262144 }));
  assert.deepEqual(await begin, { saveId: "save_01", chunkBytes: 262144 });
  const chunk = valid.client.appendFileSaveChunk({ saveId: "save_01", seq: 1, text: "x" });
  const chunkRequest = JSON.parse(valid.child.writes[1] ?? "{}") as { requestId: string };
  valid.child.stdout.write(responseLine(chunkRequest.requestId, { acceptedSeq: 1 }));
  assert.deepEqual(await chunk, { acceptedSeq: 1 });
  const commit = valid.client.commitFileSave({ saveId: "save_01", expectedVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", lastSeq: 1, totalBytes: 1 });
  const commitRequest = JSON.parse(valid.child.writes[2] ?? "{}") as { requestId: string };
  valid.child.stdout.write(responseLine(commitRequest.requestId, { outcome: "dirtyConflict", error: { code: "E_DIRTY", kind: "dirtyConflict", detail: "file changed", retryable: false, outcomeUnknown: false } }));
  assert.equal((await commit).outcome, "dirtyConflict");

  const invalidPhase = createClient();
  const invalidStart = invalidPhase.client.start();
  invalidPhase.child.stdout.write(`${readyLine()}\n`);
  await invalidStart;
  const invalidBegin = invalidPhase.client.beginFileSave(saveBeginInput());
  const invalidRequest = JSON.parse(invalidPhase.child.writes[0] ?? "{}") as { requestId: string };
  invalidPhase.child.stdout.write(responseLine(invalidRequest.requestId, { acceptedSeq: 1 }));
  await assert.rejects(invalidBegin, /file\.save result is invalid/);
  assert.equal(invalidPhase.child.terminations, 1);

  for (const saveId of ["", "x".repeat(513)]) {
    const invalidId = createClient(); const invalidIdStart = invalidId.client.start(); invalidId.child.stdout.write(`${readyLine()}\n`); await invalidIdStart;
    const beginWithBadId = invalidId.client.beginFileSave(saveBeginInput()); const badIdRequest = JSON.parse(invalidId.child.writes[0] ?? "{}") as { requestId: string };
    invalidId.child.stdout.write(responseLine(badIdRequest.requestId, { saveId, chunkBytes: 262144 }));
    await assert.rejects(beginWithBadId, /file\.save result is invalid/); assert.equal(invalidId.child.terminations, 1);
  }

  const duplicate = createClient();
  const duplicateStart = duplicate.client.start();
  duplicate.child.stdout.write(`${readyLine()}\n`);
  await duplicateStart;
  const watch = duplicate.client.watchProjectFiles({ projectId: "project_01" });
  const duplicateRequest = JSON.parse(duplicate.child.writes[0] ?? "{}") as { requestId: string };
  const response = responseLine(duplicateRequest.requestId, { watching: true });
  duplicate.child.stdout.write(response);
  await watch;
  duplicate.child.stdout.write(response);
  assert.match(duplicate.client.failure?.message ?? "", /more than one response/);
  assert.equal(duplicate.child.terminations, 1);
});

test("G4 rejects a valid-shaped response for a different project or path", async () => {
  const { child, client } = createClient();
  const started = client.start();
  child.stdout.write(`${readyLine()}\n`);
  await started;
  const read = client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const request = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(request.requestId, {
    projectId: "project_02", path: "src/Other.tsx", classification: "utf8", byteLength: 1,
    version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
    stream: { streamId: "wrong_project_text", chunkBytes: 262144 },
  }));
  await assert.rejects(read, /projectId does not match request/);
  assert.equal(child.terminations, 1);
});

test("a stdin write EPIPE poisons every pending request without retry", async () => {
  const { child, client, spawns } = createClient();
  const started = client.start(); child.stdout.write(`${readyLine()}\n`); await started;
  const first = client.watchProjectFiles({ projectId: "project_01" });
  const firstRejected = assert.rejects(first, /EPIPE/);
  child.writeError = new Error("EPIPE");
  const second = client.getDiffSummary({ projectId: "project_01" });
  await firstRejected;
  await assert.rejects(second, /EPIPE/);
  assert.equal(child.terminations, 1); assert.equal(spawns(), 1);
  await assert.rejects(client.watchProjectFiles({ projectId: "project_01" }), /EPIPE/);
});

test("G4 bounds empty stream events and releases completed stream state", async () => {
  const bounded = createClient(); const start = bounded.client.start(); bounded.child.stdout.write(`${readyLine()}\n`); await start;
  const read = bounded.client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const request = JSON.parse(bounded.child.writes[0] ?? "{}") as { requestId: string };
  bounded.child.stdout.write(responseLine(request.requestId, { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "bounded", chunkBytes: 262144 } })); await read;
  for (let seq = 1; seq <= 257; seq += 1) bounded.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "bounded", seq, event: "blob.chunk", payload: { text: "" } }));
  assert.match(bounded.client.failure?.message ?? "", /exceeds 256 events/); assert.equal(bounded.child.terminations, 1);

  const complete = createClient(); const completeStart = complete.client.start(); complete.child.stdout.write(`${readyLine()}\n`); await completeStart;
  const completeRead = complete.client.readFile({ projectId: "project_01", path: "src/App.tsx" }); const completeRequest = JSON.parse(complete.child.writes[0] ?? "{}") as { requestId: string };
  complete.child.stdout.write(responseLine(completeRequest.requestId, { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "completed", chunkBytes: 262144 } })); await completeRead;
  complete.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "completed", seq: 1, event: "blob.chunk", payload: { text: "x" } }));
  complete.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "completed", seq: 2, event: "blob.chunk", payload: { text: "" } }));
  assert.match(complete.client.failure?.message ?? "", /unknown or completed/); assert.equal(complete.child.terminations, 1);
});

test("BridgeV2 runtime validators reject malformed and extra input before IPC", () => {
  assert.throws(() => validateTreeInput({ projectId: "project_01", parentPath: "../escape", cursor: null }));
  assert.throws(() => validatePathInput({ projectId: "project_01", path: "a", extra: true }));
  assert.throws(() => validateSaveBeginInput({ ...saveBeginInput(), totalBytes: 5 * 1024 * 1024 + 1 }));
  assert.throws(() => validateSaveChunkInput({ saveId: "save_01", seq: 0, text: "x" }));
  assert.throws(() => validateSaveCommitInput({ saveId: "save_01", expectedVersion: "sha256:bad", lastSeq: 1, totalBytes: 1 }));
  assert.deepEqual(validatePathInput({ projectId: "project_01", path: "src/App.tsx" }), { projectId: "project_01", path: "src/App.tsx" });
});

test("renderer event credit bounds stalled delivery and drains only exact acknowledgements", () => {
  let failure: Error | undefined; const credit = new EventCreditV1((error) => { failure = error; }); let sent = 0;
  const event = (seq: number) => ({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "credit", seq, event: "blob.chunk", payload: { text: "" } } as EventV1);
  for (let seq = 1; seq <= MAX_IN_FLIGHT_EVENTS; seq += 1) credit.deliver(event(seq), () => { sent += 1; });
  credit.deliver(event(MAX_IN_FLIGHT_EVENTS + 1), () => { sent += 1; });
  assert.equal(sent, MAX_IN_FLIGHT_EVENTS); assert.match(failure?.message ?? "", /consumerTooSlow/);
  let normalFailure: Error | undefined; const normal = new EventCreditV1((error) => { normalFailure = error; }); normal.deliver(event(1), () => {}); normal.acknowledge({ streamId: "credit", seq: 1 }); normal.deliver(event(2), () => {}); assert.equal(normalFailure, undefined);
  normal.acknowledge({ streamId: "credit", seq: 1 }); assert.match(normalFailure?.message ?? "", /consumerTooSlow/);
});

test("renderer close/reopen credit lifecycle fails only with unacknowledged delivery", () => {
  let failure: Error | undefined; const stalled = new EventCreditV1((error) => { failure = error; });
  stalled.deliver({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "close", seq: 1, event: "blob.chunk", payload: { text: "" } }, () => {});
  const outstanding = stalled.outstanding; assert.equal(outstanding, 1); if (Number(outstanding) !== 0) failure = new Error("rendererDisconnected");
  assert.match(failure?.message ?? "", /rendererDisconnected/);
  const clean = new EventCreditV1(() => assert.fail("clean close must not fail")); assert.equal(clean.outstanding, 0); assert.equal(clean.resetIfEmpty(), true);
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
    "quitCoordinator.handleBeforeQuit()",
  ]) assert.ok(main.includes(required), `missing ${required}`);
  assert.match(sidecarClient, /shell: false/);
  assert.match(sidecarClient, /MAX_SIDECAR_STDERR_TAIL_BYTES = 32 \* 1024/);
  assert.match(sidecarClient, /terminate\(\): void/);
  assert.match(main, /kind: "sidecarUnavailable"/);
  assert.doesNotMatch(main, /process\.env/);
  assert.match(preload, /ipcRenderer\.invoke\(M5_INTERNAL_CHANNELS_V1\.systemGetStatus\)/);
  assert.match(preload, /ipcRenderer\.invoke\(M5_INTERNAL_CHANNELS_V1\.appPrepareQuit\)/);
  assert.match(preload, /ipcRenderer\.on\(M5_INTERNAL_CHANNELS_V1\.sidecarEvent/);
  assert.match(preload, /ipcRenderer\.send\(M5_BRIDGE_V2_CHANNELS\.eventAck/);
  assert.match(main, /ipcMain\.on\(M5_BRIDGE_V2_CHANNELS\.eventAck/);
  assert.match(main, /sidecar\?\.failRendererDisconnected\(\)/);
  assert.match(main, /quitCoordinator\.exitAllowed/);
  assert.match(main, /dialog\.showOpenDialog/);
  assert.match(main, /properties: \["openDirectory"\]/);
  assert.match(main, /return \{ outcome: "cancelled" \}/);
  assert.match(preload, /new DOMException\("Project selection cancelled", "AbortError"\)/);
  assert.match(preload, /validateProjectIdentityV1\(result\)/);
  assert.doesNotMatch(preload, /openProject:\s*\([^)]*input/);
  assert.doesNotMatch(preload, /selectedPath/);
  assert.doesNotMatch(preload, /exposeInMainWorld\([^\n]+ipcRenderer|sendSync|sendTo|postMessage/);
  assert.match(forge, /packageAfterCopy/);
  assert.match(forge, /m5-stage-sidecar\.mjs/);
  assert.match(smoke, /sidecar/);
  assert.match(smoke, /must remain outside app\.asar/);
  assert.match(smoke, /assert\.deepEqual\(status\.result\.sidecar, ready\.payload\)/);
  assert.match(smoke, /assertCleanMarker\(dataDirectory, ready\.sidecarInstanceId\)/);
  assert.match(smoke, /await stopChild\(child\)/);
  assert.match(smoke, /await withDeadline\(exited, 3_000, "failed packaged sidecar cleanup"\)/);
});

function source(path: string) {
  return readFileSync(resolve(path), "utf8");
}

function responseLine(requestId: string, result: Record<string, unknown>) {
  return `${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId, ok: true, result })}\n`;
}

function saveBeginInput() {
  return {
    projectId: "project_01",
    path: "src/App.tsx",
    expectedVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881" as const,
    totalBytes: 1,
    encoding: "utf8" as const,
  };
}
