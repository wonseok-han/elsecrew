#!/usr/bin/env node
// Screen captures for before/after comparison (replaces the old tests/m5-ui-capture.cjs,
// whose first-screen expectation went stale). It runs the renderer smoke, whose capture
// points walk every screen with a synthetic bridge (no sidecar, no provider), once per
// brightness, saving each point as a PNG and the rendered contrast measurement beside it.
// Needs a renderer build: `vite build --config electron/vite.renderer.config.ts`.
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

const require = createRequire(import.meta.url);
const electron = require("electron");
const out = resolve(process.argv[2] ?? `.artifacts/m5-visual/${new Date().toISOString().replace(/[:.]/g, "-")}`);
let failed = false;
for (const theme of ["dark", "light"]) {
  const dir = resolve(out, theme);
  mkdirSync(dir, { recursive: true });
  const temp = mkdtempSync(resolve(tmpdir(), "elsecrew-ui-capture-"));
  const result = spawnSync(electron, ["tests/m5-renderer-smoke-runner.cjs"], {
    cwd: resolve("."), encoding: "utf8",
    env: { ...process.env, M5_SMOKE_TEMP_ROOT: temp, M5_MEASURE_PARALLEL_PAINT: "0", M5_MEASURE_MESSAGE_PAINT: "0",
      M5_RENDERER_DIR: resolve(".vite/renderer/main_window"), M5_SMOKE_THEME: theme, M5_SMOKE_CAPTURE_DIR: dir, ELECTRON_DISABLE_SECURITY_WARNINGS: "true" },
  });
  if (result.status === 0) rmSync(temp, { recursive: true });
  const contrast = result.stdout.match(/\{"renderedContrast":[^\n]*\}/)?.[0];
  console.log(`${theme}: ${result.status === 0 ? "ok" : `failed (${result.status})`} ${contrast ?? ""} → ${dir}`);
  if (result.status !== 0) { failed = true; console.error(result.stdout.slice(-4000), result.stderr.slice(-4000)); }
}
process.exit(failed ? 1 : 0);
