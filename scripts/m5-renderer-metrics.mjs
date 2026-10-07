import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { qaEnvironment, lowerQaPriority } from "./m5-qa-resources.mjs";
const root = fileURLToPath(new URL("..", import.meta.url));
const diagnostic = process.argv.includes("--diagnostic");
const domCounters = process.argv.includes("--dom-counters");
const heapSampling = process.argv.includes("--heap-sampling");
const run = (command, args, options = {}) => spawnSync(command, args, { cwd: root, env: qaEnvironment(), encoding: "utf8", timeout: 150000, maxBuffer: 2 * 1024 * 1024, ...options });
try {
  if (domCounters && (!diagnostic || !process.argv.includes("--memory"))) throw new Error("DOM counters require --diagnostic --memory");
  if (heapSampling && (!diagnostic || !process.argv.includes("--memory"))) throw new Error("Heap sampling requires --diagnostic --memory");
  const head = run("/usr/bin/git", ["rev-parse", "HEAD"]);
  const dirty = run("/usr/bin/git", ["status", "--porcelain"]);
  if (head.status !== 0 || dirty.status !== 0 || (!diagnostic && dirty.stdout.trim())) throw new Error("clean checkout required");
  if (process.platform !== "darwin" || process.arch !== "arm64") throw new Error("macOS arm64 measurement only");
  lowerQaPriority();
  const build = run("pnpm", ["exec", "vite", "build", "--config", "electron/vite.renderer.config.ts"]);
  if (build.status !== 0) throw new Error(`renderer build failed: ${build.stderr}`);
  const electron = createRequire(import.meta.url)("electron");
  const result = run(electron, ["tests/m5-renderer-performance-runner.cjs"], { maxBuffer: 8 * 1024 * 1024, env: { ...qaEnvironment(), M5_MEASUREMENT_HEAD: head.stdout.trim(), M5_MEASURE_MEMORY: process.argv.includes("--memory") ? "1" : "0", M5_MEASURE_DIAGNOSTIC: diagnostic ? "1" : "0", M5_MEASURE_DOM_COUNTERS: domCounters ? "1" : "0", M5_MEASURE_HEAP_SAMPLING: heapSampling ? "1" : "0" } });
  if (result.stdout) process.stdout.write(result.stdout);
  if (result.stderr) process.stderr.write(result.stderr);
  if (result.error || result.signal || ![0, 1, 2].includes(result.status)) throw new Error("renderer measurement process failed");
  if (diagnostic && result.status === 0) throw new Error("diagnostic measurement cannot pass the gate");
  process.exitCode = result.status;
} catch (error) {
  console.log(JSON.stringify({ schemaVersion: 1, evidence: "m5-renderer-fixture-performance", status: "NOT_READY", reason: String(error) }));
  process.exitCode = 2;
}
