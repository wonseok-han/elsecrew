// Starts the build-then-preview check in the real app (결과 미리보기 확장 1단계).
//   node scripts/m5-preview-build-run.mjs [--build] [--real] [report-folder]
// --build builds main/preload/renderer into .vite/ first (no packaging).
// --real uses the development app's data and sign-ins: the team writes the
// app (paid model calls). Needs target/debug/elsecrew-sidecar.
import { spawnSync } from "node:child_process";
import { builtinModules, createRequire } from "node:module";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "darwin") {
  console.log(JSON.stringify({ status: "NOT_RUN", reason: "macOS only" }));
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
const report = args.find(arg => !arg.startsWith("--"));
const env = { ...process.env, ...(args.includes("--real") ? { ELSECREW_PREVIEW_BUILD_REAL: "1" } : {}) };
const run = spawnSync(electron, [join(repoRoot, "scripts", "m5-preview-build-run.cjs"), ...(report ? [report] : [])], { cwd: repoRoot, env, stdio: "inherit" });
process.exit(run.status ?? 1);
