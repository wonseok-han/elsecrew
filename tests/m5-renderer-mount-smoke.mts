import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { runInNewContext } from "node:vm";

import { M5_BRIDGE_V6_METHODS } from "../src/m5/contracts/bridge-v6.ts";
import { parseG5DProductSystemStatusV1 } from "../src/m5/contracts/lifecycle-g5d-v1.ts";

const require = createRequire(import.meta.url);
const electronPath = require("electron") as string;

test("renderer smoke preload exposes the complete V6 bridge with ready admission and enforces snapshot-before-tree", async () => {
  const window: { m5?: Record<string, unknown> } = {};
  runInNewContext(readFileSync(resolve("tests/m5-renderer-smoke-preload.cjs"), "utf8"), { window, Set, Error });
  const bridge = window.m5;
  assert.ok(bridge);
  assert.equal(bridge.version, 6);
  assert.deepEqual(Object.keys(bridge).sort(), ["version", ...M5_BRIDGE_V6_METHODS].sort());
  const getSystemStatus = bridge.getSystemStatus as () => Promise<unknown>;
  const getProjectTree = bridge.getProjectTree as (input: unknown) => Promise<unknown>;
  const createProjectWorktree = bridge.createProjectWorktree as (input: unknown) => Promise<unknown>;
  const status = await getSystemStatus();
  assert.doesNotThrow(() => parseG5DProductSystemStatusV1(status));
  await assert.rejects(getProjectTree({}), /before creating its standalone snapshot/);
  await createProjectWorktree({ projectId: "smoke_project" });
  assert.deepEqual(JSON.parse(JSON.stringify(await getProjectTree({}))), {
    projectId: "smoke_project",
    parentPath: "",
    entries: [{ path: "src/example.ts", name: "example.ts", kind: "file" }],
    nextCursor: null,
  });
});

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
