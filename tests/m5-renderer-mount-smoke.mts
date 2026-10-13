import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { runInNewContext } from "node:vm";

import { M5_BRIDGE_V7_METHODS } from "../src/m5/contracts/bridge-v7.ts";
import { parseG5DProductSystemStatusV1 } from "../src/m5/contracts/lifecycle-g5d-v1.ts";

const require = createRequire(import.meta.url);
const electronPath = require("electron") as string;

test("renderer smoke preload exposes the complete V7 bridge with ready admission and enforces snapshot-before-tree", async () => {
  const window: { m5?: Record<string, unknown> } = {};
  runInNewContext(readFileSync(resolve("tests/m5-renderer-smoke-preload.cjs"), "utf8"), { window, Set, Error });
  const bridge = window.m5;
  assert.ok(bridge);
  assert.equal(bridge.version, 7);
  assert.deepEqual(Object.keys(bridge).sort(), ["version", ...M5_BRIDGE_V7_METHODS].sort());
  const getSystemStatus = bridge.getSystemStatus as () => Promise<unknown>;
  const getProjectTree = bridge.getProjectTree as (input: unknown) => Promise<unknown>;
  const createProjectWorktree = bridge.createProjectWorktree as (input: unknown) => Promise<unknown>;
  const status = await getSystemStatus.call(bridge);
  assert.doesNotThrow(() => parseG5DProductSystemStatusV1(status));
  await assert.rejects(getProjectTree({}), /before creating its standalone snapshot/);
  await createProjectWorktree({ projectId: "smoke_project" });
  assert.deepEqual(JSON.parse(JSON.stringify(await getProjectTree({}))), {
    projectId: "smoke_project",
    parentPath: "",
    entries: [{ path: "src", name: "src", kind: "directory" }],
    nextCursor: null,
  });
});

test("packaged renderer build mounts lazy Monaco and creates a local worker in Chromium", { timeout: 20_000 }, () => {
  const temporaryRoot = mkdtempSync(resolve(tmpdir(), "hello-agent-renderer-test-"));
  const result = spawnSync(electronPath, ["tests/m5-renderer-smoke-runner.cjs"], {
    cwd: resolve("."),
    encoding: "utf8",
    env: {
      ...process.env,
      M5_SMOKE_TEMP_ROOT: temporaryRoot,
      M5_MEASURE_PARALLEL_PAINT: "0",
      M5_MEASURE_MESSAGE_PAINT: "0",
      M5_RENDERER_DIR: resolve(".vite/renderer/main_window"),
      ELECTRON_DISABLE_SECURITY_WARNINGS: "true",
    },
  });
  // spawnSync has reaped this child. On abnormal termination leave its data
  // for diagnosis instead; never remove a path supplied by child output/env.
  if (!result.error && result.signal === null && [0, 1].includes(result.status ?? -1)) {
    rmSync(temporaryRoot, { recursive: true });
  }
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /"workerCreated":true/);
});
