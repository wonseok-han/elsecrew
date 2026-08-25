import { chmodSync, copyFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const resourcesDirectory = process.argv[2];
if (!resourcesDirectory) throw new Error("usage: node scripts/m5-stage-sidecar.mjs <resources-directory>");

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const suffix = process.platform === "win32" ? ".exe" : "";
const binary = `hello-agent-sidecar${suffix}`;
const build = spawnSync("cargo", ["build", "--locked", "--release", "-p", "hello-agent-sidecar"], {
  cwd: root,
  stdio: "inherit",
});
if (build.status !== 0 || build.error) {
  throw build.error ?? new Error(`sidecar build failed with exit ${String(build.status)}`);
}

const destinationDirectory = resolve(resourcesDirectory, "sidecar");
mkdirSync(destinationDirectory, { recursive: true });
const source = resolve(root, "target", "release", binary);
const destination = resolve(destinationDirectory, binary);
copyFileSync(source, destination);
if (process.platform !== "win32") chmodSync(destination, 0o755);
