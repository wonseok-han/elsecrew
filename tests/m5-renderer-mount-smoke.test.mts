import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const require = createRequire(import.meta.url);
const electronPath = require("electron") as string;

test("packaged renderer build mounts lazy Monaco and creates a local worker in Chromium", { timeout: 20_000 }, () => {
  const result = spawnSync(electronPath, ["tests/m5-renderer-smoke-runner.cjs"], {
    cwd: resolve("."),
    encoding: "utf8",
    env: {
      ...process.env,
      M5_RENDERER_DIR: resolve(".vite/renderer/main_window"),
      ELECTRON_DISABLE_SECURITY_WARNINGS: "true",
    },
  });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /"workerCreated":true/);
});
