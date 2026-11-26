#!/usr/bin/env node
// Runs the multi-project native smoke (scripts/m5-multiproject-ui-smoke.cjs):
//   node scripts/m5-multiproject-ui-run.mjs [--build] [captures-folder]
// --build builds main/preload/renderer into .vite/ first (no packaging).
// Needs target/debug/elsecrew-sidecar (`cargo build -p elsecrew-sidecar`).
// Fixture agents and isolated app data only: no account, no provider.
// macOS only (exit 2 elsewhere, "not run").
import { spawnSync } from "node:child_process";
import { builtinModules, createRequire } from "node:module";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "darwin") {
  console.log(JSON.stringify({ status: "NOT_RUN", reason: "macOS only (the fixture agent and /usr/bin/git)" }));
  process.exit(2);
}
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
if (args.includes("--build")) {
  const { build } = await import("vite");
  const external = ["electron", "electron/common", ...builtinModules.flatMap(name => [name, `node:${name}`])];
  const define = { MAIN_WINDOW_VITE_DEV_SERVER_URL: "undefined", MAIN_WINDOW_VITE_NAME: JSON.stringify("main_window") };
  await build({ configFile: false, root: repoRoot, mode: "production", logLevel: "warn", define,
    build: { emptyOutDir: false, outDir: ".vite/build", minify: false, lib: { entry: "electron/main.ts", formats: ["cjs"], fileName: () => "main.cjs" }, rollupOptions: { external } } });
  await build({ configFile: false, root: repoRoot, mode: "production", logLevel: "warn", define,
    build: { emptyOutDir: false, outDir: ".vite/build", minify: false, rollupOptions: { external, input: "electron/preload.ts", output: { format: "cjs", inlineDynamicImports: true, entryFileNames: "[name].cjs", chunkFileNames: "[name].cjs" } } } });
  await build({ configFile: join(repoRoot, "electron", "vite.renderer.config.ts"), logLevel: "warn" });
}
for (const required of [".vite/build/main.cjs", ".vite/build/preload.cjs", ".vite/renderer/main_window/index.html", "target/debug/elsecrew-sidecar"]) {
  if (!existsSync(join(repoRoot, required))) throw new Error(`missing ${required}; run with --build and cargo build -p elsecrew-sidecar`);
}
const electron = createRequire(import.meta.url)("electron");
const env = { ...process.env };
delete env.ELECTRON_RUN_AS_NODE;
const captures = args.find(arg => !arg.startsWith("--"));
const script = join(repoRoot, "scripts", "m5-multiproject-ui-smoke.cjs");
const extra = captures ? [resolve(captures)] : [];
// 1-4: two projects at work; the app then closes with both working.
const first = spawnSync(electron, [script, ...extra], { cwd: repoRoot, env, stdio: ["inherit", "pipe", "inherit"], encoding: "utf8" });
process.stdout.write(first.stdout ?? "");
if (first.status !== 0) process.exit(first.status ?? 1);
const root = (first.stdout ?? "").split("\n").map(line => { try { return JSON.parse(line); } catch { return null; } }).find(value => value?.root)?.root;
if (!root) { console.error("no app data folder from the first run"); process.exit(1); }
// 5: the same app data opened again.
const second = spawnSync(electron, [script, ...extra], { cwd: repoRoot, env: { ...env, MP_STAGE: "reopen", MP_ROOT: root }, stdio: "inherit" });
process.exit(second.status ?? 1);
