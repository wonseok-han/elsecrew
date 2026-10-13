/** Real sidecar startup on a generated pre-migrated DB; release or explicit debug diagnostic. */
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdtemp, rm, readFile, mkdir, writeFile } from "node:fs/promises";
import { tmpdir, cpus, totalmem, release } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { decodeJsonl, parseReadyPayload, parsePrepareQuitResult } from "../src/m5/contracts/protocol-v1.ts";
import { parseCurrentSystemStatusV1 } from "../src/m5/contracts/lifecycle-g5d-v1.ts";
import { qaEnvironment, lowerQaPriority } from "./m5-qa-resources.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));
const git = (args: string[]) => {
  const result = spawnSync("/usr/bin/git", args, { cwd: root, encoding: "utf8" });
  assert.equal(result.status, 0, "Git metadata unavailable"); return result.stdout.trim();
};

async function trial(binary: string, directory: string) {
  const started = performance.now();
  const child = spawn(binary, ["--stdio"], { cwd: directory, env: qaEnvironment(), stdio: ["pipe", "pipe", "pipe"], shell: false });
  // Listen before any await so a fast exit/error cannot be missed.
  const closed = new Promise<void>((done, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => code === 0 && signal === null ? done() : reject(new Error("sidecar exited unsuccessfully")));
  });
  void closed.catch(() => {});
  child.stdin.on("error", () => {});
  child.stderr.resume(); // Do not persist unbounded raw process output.
  let outputBytes = 0;
  child.stdout.on("data", (chunk: Buffer) => {
    outputBytes += chunk.length;
    if (outputBytes > 1024 * 1024) child.stdout.destroy(new Error("startup output exceeded limit"));
  });
  // The benchmark only sends status/quit; it never starts an agent or descendants.
  let timer: ReturnType<typeof setTimeout>;
  const deadline = new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error("startup trial timed out")), 10000); });
  const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
  const iterator = lines[Symbol.asyncIterator]();
  const next = async () => {
    const line = await Promise.race([iterator.next(), deadline]);
    assert.equal(line.done, false, "sidecar ended before response");
    return decodeJsonl(`${line.value}\n`);
  };
  try {
    const event = await next();
    assert.equal(event.kind, "event");
    if (event.kind !== "event") throw new Error("expected ready event");
    const ready = parseReadyPayload(event);
    assert.equal(ready.pid, child.pid);
    const shellReadyMs = performance.now() - started;
    let sequence = 0;
    const request = async (method: string) => {
      const requestId = `startup_${++sequence}`;
      child.stdin.write(`${JSON.stringify({ protocolVersion: 1, kind: "request", sidecarInstanceId: ready.sidecarInstanceId, requestId, method, params: {} })}\n`);
      const response = await next();
      if (response.kind !== "response") throw new Error("expected response");
      assert.equal(response.sidecarInstanceId, ready.sidecarInstanceId);
      assert.equal(response.requestId, requestId); assert.equal(response.ok, true);
      return response;
    };
    for (;;) {
      const status = parseCurrentSystemStatusV1((await request("system.status")).result, false, true);
      assert.deepEqual(status.sidecar, ready);
      assert.notEqual(status.storageAdmission.state, "failed");
      if (status.storageAdmission.state === "ready") {
        assert.equal(status.recoveryRequired, false); assert.deepEqual(status.blockerIds, []); break;
      }
      await new Promise((done) => setTimeout(done, 10));
    }
    const storageReadyMs = performance.now() - started;
    assert.deepEqual(parsePrepareQuitResult(await request("app.prepareQuit")), { blockers: [], safeToQuit: true });
    child.stdin.end();
    await Promise.race([closed, deadline]);
    const db = new DatabaseSync(resolve(directory, "hello-agent.sqlite3"), { readOnly: true });
    try {
      assert.equal(typeof db.prepare("SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id = ?").get(ready.sidecarInstanceId)?.clean_shutdown_at, "string");
    } finally { db.close(); }
    return { shellReadyMs, storageReadyMs };
  } finally {
    clearTimeout(timer!); lines.close();
    if (child.exitCode === null && child.signalCode === null) {
      child.stdin.end(); child.kill("SIGTERM");
      let cleanupTimer: ReturnType<typeof setTimeout>;
      try {
        await Promise.race([closed.catch(() => {}), new Promise<never>((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("cleanup unconfirmed; temporary DB retained")), 3000); })]);
      } finally { clearTimeout(cleanupTimer!); }
    }
  }
}

const diagnostic = process.argv.length === 3 && process.argv[2] === "--diagnostic";
const report: any = { schemaVersion: 1, evidence: "m5-sidecar-startup-only", status: "NOT_READY", diagnostic, gateEligible: false, buildProfile: diagnostic ? "debug" : "release", startedAt: new Date().toISOString(), samples: [], reasons: [],
  limitations: ["Sidecar-only warm startup on a generated empty DB. Does not measure packaged UI interactivity, loaded databases, Windows or overall G6 readiness."] };
let directory: string | undefined;
try {
  assert.ok(process.argv.length === 2 || diagnostic, "only --diagnostic is supported");
  assert.equal(process.platform, "darwin", "macOS measurement only");
  assert.equal(process.arch, "arm64", "macOS arm64 measurement only");
  report.head = git(["rev-parse", "HEAD"]); report.gitStatusBefore = git(["status", "--porcelain"]); report.dirtyBefore = report.gitStatusBefore !== "";
  assert.ok(diagnostic || !report.dirtyBefore, "clean checkout required");
  lowerQaPriority();
  const build = spawnSync("cargo", ["build", ...(diagnostic ? [] : ["--release"]), "--locked", "-p", "hello-agent-sidecar", "--bin", "hello-agent-sidecar", "--message-format=json"], { cwd: root, env: { ...qaEnvironment(), CARGO_INCREMENTAL: "0" }, encoding: "utf8", timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
  assert.equal(build.status, 0, "sidecar build failed");
  const binaries = build.stdout.trim().split("\n").map((line) => JSON.parse(line)).filter((value) => value.reason === "compiler-artifact" && value.target?.name === "hello-agent-sidecar" && value.target.kind.includes("bin") && value.executable).map((value) => value.executable);
  assert.equal(binaries.length, 1, "expected exact Cargo executable");
  const binary = binaries[0];
  report.binarySha256 = createHash("sha256").update(await readFile(binary)).digest("hex");
  report.environment = { cpu: cpus()[0]?.model, ramBytes: totalmem(), os: release(), platform: process.platform, arch: process.arch, node: process.version };
  const machine = spawnSync("/usr/sbin/sysctl", ["-n", "hw.model"], { encoding: "utf8" });
  assert.equal(machine.status, 0); report.environment.machine = machine.stdout.trim();
  const rust = spawnSync("rustc", ["--version"], { encoding: "utf8" });
  assert.equal(rust.status, 0); report.environment.rust = rust.stdout.trim();
  directory = await mkdtemp(resolve(tmpdir(), "hello-agent-startup-metrics-"));
  report.temporaryDirectory = directory;
  await trial(binary, directory); // Bootstrap/migration excluded from warm samples.
  report.warmup = await trial(binary, directory);
  for (let index = 0; index < 10; index++) report.samples.push(await trial(binary, directory));
  report.shellReadyP95Ms = Math.max(...report.samples.map((sample: any) => sample.shellReadyMs));
  report.storageReadyP95Ms = Math.max(...report.samples.map((sample: any) => sample.storageReadyMs));
  report.shellHardTimeoutPassed = report.samples.every((sample: any) => sample.shellReadyMs <= 3000);
  report.headAfter = git(["rev-parse", "HEAD"]); report.gitStatusAfter = git(["status", "--porcelain"]); report.dirtyAfter = report.gitStatusAfter !== "";
  assert.equal(report.headAfter, report.head); assert.equal(report.gitStatusAfter, report.gitStatusBefore);
  report.gateEligible = !diagnostic;
  report.status = report.shellReadyP95Ms <= 1000 && report.storageReadyP95Ms <= 1000 ? (diagnostic ? "NOT_READY" : "PASS") : "FAIL";
  if (diagnostic) report.reasons.push("debug diagnostic only; clean release measurements still required");
  if (report.status === "FAIL") report.reasons.push("sidecar startup exceeded 1000ms p95");
  await rm(directory, { recursive: true }); report.temporaryDirectory = null;
} catch (error) {
  report.status = report.head && (diagnostic || report.dirtyBefore === false) ? "FAIL" : "NOT_READY";
  report.reasons.push(error instanceof Error ? error.message : "measurement failed");
  // Preserve generated DB evidence on any failed/uncertain trial.
}
report.finishedAt = new Date().toISOString();
const evidenceDirectory = resolve(root, ".artifacts", "m5-performance");
try {
  await mkdir(evidenceDirectory, { recursive: true });
  const path = resolve(evidenceDirectory, `sidecar-startup-${report.startedAt.replaceAll(":", "-")}-${process.pid}.json`);
  await writeFile(path, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  report.evidencePath = path;
} catch { report.status = "FAIL"; report.reasons.push("could not persist measurement report"); }
console.log(JSON.stringify(report));
process.exitCode = report.status === "PASS" ? 0 : report.status === "FAIL" ? 1 : 2;
