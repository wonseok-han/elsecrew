import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { qaEnvironment, lowerQaPriority } from "./m5-qa-resources.mjs";
const root = fileURLToPath(new URL("..", import.meta.url));
const run = (command, args, options = {}) => spawnSync(command, args, { cwd: root, env: qaEnvironment(), encoding: "utf8", timeout: 150000, maxBuffer: 2 * 1024 * 1024, ...options });
try {
  const head = run("/usr/bin/git", ["rev-parse", "HEAD"]);
  const dirty = run("/usr/bin/git", ["status", "--porcelain"]);
  if (head.status !== 0 || dirty.status !== 0 || dirty.stdout.trim()) throw new Error("clean checkout required");
  if (process.platform !== "darwin" || process.arch !== "arm64") throw new Error("macOS arm64 measurement only");
  lowerQaPriority();
  const build = run("pnpm", ["exec", "vite", "build", "--config", "electron/vite.renderer.config.ts"]);
  if (build.status !== 0) throw new Error(`renderer build failed: ${build.stderr}`);
  const electron = createRequire(import.meta.url)("electron");
  const result = run(electron, ["tests/m5-renderer-performance-runner.cjs"], { env: { ...qaEnvironment(), M5_MEASUREMENT_HEAD: head.stdout.trim() } });
  if (result.stdout) process.stdout.write(result.stdout);
  if (result.stderr) process.stderr.write(result.stderr);
  if (result.error || result.signal || ![0, 1].includes(result.status)) throw new Error("renderer measurement process failed");
  process.exitCode = result.status;
} catch (error) {
  console.log(JSON.stringify({ schemaVersion: 1, evidence: "m5-renderer-fixture-performance", status: "NOT_READY", reason: String(error) }));
  process.exitCode = 2;
}
