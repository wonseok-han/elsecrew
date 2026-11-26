#!/usr/bin/env node
// Starts the real-app preview UI smoke (scripts/m5-preview-ui-smoke.cjs) with
// snapshot workspaces, without shell-specific environment syntax.
// macOS only (PR #9 review I5): elsewhere the preview is off by default and
// this check exits 2 ("not run", never a pass).
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "darwin") {
  console.log(JSON.stringify({ status: "NOT_RUN", reason: `macOS only: the result preview is off by default on ${process.platform} and its real-app check has not been made for it` }));
  process.exit(2);
}
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const electron = createRequire(import.meta.url)("electron");
const env = { ...process.env, ELSECREW_GIT_WORKTREES: "0" };
delete env.ELECTRON_RUN_AS_NODE;
const result = spawnSync(electron, [join(repoRoot, "scripts", "m5-preview-ui-smoke.cjs")], { cwd: repoRoot, env, stdio: "inherit" });
process.exit(result.status ?? 1);
