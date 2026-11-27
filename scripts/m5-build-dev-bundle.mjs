#!/usr/bin/env node
// Builds main/preload/renderer into .vite/ (no packaging), as the latency and
// real-run probes need: node scripts/m5-build-dev-bundle.mjs
import { builtinModules } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export async function buildDevBundle() {
  const { build } = await import("vite");
  const external = ["electron", "electron/common", ...builtinModules.flatMap(name => [name, `node:${name}`])];
  const define = { MAIN_WINDOW_VITE_DEV_SERVER_URL: "undefined", MAIN_WINDOW_VITE_NAME: JSON.stringify("main_window") };
  await build({ configFile: false, root: repoRoot, mode: "production", logLevel: "warn", define,
    build: { emptyOutDir: false, outDir: ".vite/build", minify: false, lib: { entry: "electron/main.ts", formats: ["cjs"], fileName: () => "main.cjs" }, rollupOptions: { external } } });
  await build({ configFile: false, root: repoRoot, mode: "production", logLevel: "warn", define,
    build: { emptyOutDir: false, outDir: ".vite/build", minify: false, rollupOptions: { external, input: "electron/preload.ts", output: { format: "cjs", inlineDynamicImports: true, entryFileNames: "[name].cjs", chunkFileNames: "[name].cjs" } } } });
  await build({ configFile: join(repoRoot, "electron", "vite.renderer.config.ts"), logLevel: "warn" });
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await buildDevBundle();
