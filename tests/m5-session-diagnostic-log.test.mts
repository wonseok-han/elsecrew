import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createSessionDiagnosticLog } from "../electron/session-diagnostic-log.ts";

test("checkpoint snapshot survives sessions, contains only fixed labels, and stays bounded", async () => {
  const directory = mkdtempSync(join(tmpdir(), "m5-checkpoints-"));
  try {
    const log = createSessionDiagnosticLog(directory);
    const capture = log.capture;
    const file = join(directory, "last-provider-checkpoints.json");
    capture("token-secret\nnative Codex transport failed at turn start state");
    assert.deepEqual(readdirSync(directory), [], "partial lines are not accepted");
    capture("native Codex transport failed at turn start state token-secret\n");
    assert.deepEqual(readdirSync(directory), []);
    capture("native Codex transport failed at turn start state\n".repeat(100));
    await log.flush();
    const first = readFileSync(file, "utf8");
    const record = JSON.parse(first);
    assert.equal(record.codexCheckpoints.length, 32);
    assert.ok(!first.includes("token-secret"));
    assert.ok(Buffer.byteLength(first) < 8192);
    capture("native Codex transport failed at turn start state\n".repeat(100));
    assert.equal(readFileSync(file, "utf8"), first, "unchanged snapshots do not rewrite timestamps");
    const nextSession = createSessionDiagnosticLog(directory);
    nextSession.capture("unrelated stderr\n");
    assert.equal(readFileSync(file, "utf8"), first, "empty startup preserves last failure");
    nextSession.capture("native Claude transport failed at init model\n");
    await nextSession.flush();
    const next = JSON.parse(readFileSync(file, "utf8"));
    assert.notEqual(next.sessionId, record.sessionId);
    assert.deepEqual(next.claudeCheckpoints, ["init model"]);
    assert.deepEqual(readdirSync(directory), ["last-provider-checkpoints.json"]);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("checkpoint replacement does not follow a destination file symlink", { skip: process.platform === "win32" }, async () => {
  const directory = mkdtempSync(join(tmpdir(), "m5-checkpoints-"));
  try {
    const original = join(directory, "preserved.txt");
    writeFileSync(original, "preserved");
    symlinkSync(original, join(directory, "last-provider-checkpoints.json"));
    const log = createSessionDiagnosticLog(directory);
    log.capture("native Codex transport failed at turn start state\n");
    await log.flush();
    assert.equal(readFileSync(original, "utf8"), "preserved");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});


test("a directory symlink is rejected without modifying its target", { skip: process.platform === "win32" }, async () => {
  const root = mkdtempSync(join(tmpdir(), "m5-checkpoints-"));
  const external = mkdtempSync(join(tmpdir(), "m5-checkpoints-external-"));
  try {
    const target = join(external, "last-provider-checkpoints.json");
    writeFileSync(target, "preserved");
    const directory = join(root, "logs");
    symlinkSync(external, directory);
    const log = createSessionDiagnosticLog(directory);
    log.capture("native Codex transport failed at turn start state\n");
    await log.flush();
    assert.equal(readFileSync(target, "utf8"), "preserved");
    assert.deepEqual(readdirSync(external), ["last-provider-checkpoints.json"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(external, { recursive: true, force: true });
  }
});

test("a burst retains the latest snapshot and cleans temporary files", async () => {
  const directory = mkdtempSync(join(tmpdir(), "m5-checkpoints-"));
  try {
    const log = createSessionDiagnosticLog(directory);
    for (let i = 0; i < 100; i++) {
      log.capture(`native Codex transport failed at ${i % 2 ? "turn start state" : "turn stream completed"}\n`);
    }
    log.capture("native Codex transport failed at turn stream other frame\n");
    await log.flush();
    const record = JSON.parse(readFileSync(join(directory, "last-provider-checkpoints.json"), "utf8"));
    assert.deepEqual(record.codexCheckpoints, ["turn stream other frame"]);
    assert.deepEqual(readdirSync(directory), ["last-provider-checkpoints.json"]);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("identical checkpoint can persist after a transient storage failure", async () => {
  const root = mkdtempSync(join(tmpdir(), "m5-checkpoints-"));
  try {
    const directory = join(root, "logs");
    writeFileSync(directory, "temporarily unavailable directory");
    const log = createSessionDiagnosticLog(directory);
    const tail = "native Codex transport failed at turn start state\n";
    log.capture(tail);
    await log.flush();
    rmSync(directory);
    log.capture(tail);
    await log.flush();
    const record = JSON.parse(readFileSync(join(directory, "last-provider-checkpoints.json"), "utf8"));
    assert.deepEqual(record.codexCheckpoints, ["turn start state"]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("transport diagnostics retain bounded correlation labels without arbitrary content", async () => {
  const directory = mkdtempSync(join(tmpdir(), "m5-checkpoints-"));
  try {
    writeFileSync(join(directory, "last-provider-checkpoints.json"), "previous failure");
    const log = createSessionDiagnosticLog(directory, "transport");
    const id = "m5_11111111-1111-4111-8111-111111111111";
    log.capture("native Codex transport failed at turn start state\n");
    for (let i = 0; i < 80; i++) log.captureTransport("chat.get", id, "queued");
    log.captureTransport("run.startAgent", id, "received");
    log.captureTransport("run.startAgent", id, "resolved");
    log.captureTransport("agent.runStateChanged", null, "forwarded");
    log.captureTransport("secret-method", id, "queued");
    log.captureTransport("chat.get", "secret-id", "queued");
    log.captureTransport("chat.get", id, "secret-phase");
    await log.flush();
    const text = readFileSync(join(directory, "last-transport-progress.json"), "utf8");
    const record = JSON.parse(text);
    assert.equal(record.transport.length, 64);
    assert.deepEqual(record.transport.slice(-3).map(({method, phase}: {method: string; phase: string}) => [method, phase]), [
      ["run.startAgent", "received"], ["run.startAgent", "resolved"], ["agent.runStateChanged", "forwarded"],
    ]);
    assert.deepEqual(record.codexCheckpoints, ["turn start state"]);
    assert.ok(!text.includes("secret"));
    assert.ok(Buffer.byteLength(text) < 16384);
    assert.equal(readFileSync(join(directory, "last-provider-checkpoints.json"), "utf8"), "previous failure");
    assert.deepEqual(readdirSync(directory).sort(), ["last-provider-checkpoints.json", "last-transport-progress.json"]);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
