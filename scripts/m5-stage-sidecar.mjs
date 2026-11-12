import { chmodSync, copyFileSync, mkdirSync } from "node:fs";
import { dirname, isAbsolute, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const resourcesDirectory = process.argv[2];
if (!resourcesDirectory) throw new Error("usage: node scripts/m5-stage-sidecar.mjs <resources-directory>");

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const suffix = process.platform === "win32" ? ".exe" : "";
const binary = `elsecrew-sidecar${suffix}`;
const build = spawnSync("cargo", ["build", "--locked", "--release", "-p", "elsecrew-sidecar", "--bin", "elsecrew-sidecar", "--message-format=json-render-diagnostics"], {
  cwd: root,
  stdio: ["inherit", "pipe", "inherit"],
  encoding: "utf8",
});
if (build.status !== 0 || build.error) {
  throw build.error ?? new Error(`sidecar build failed with exit ${String(build.status)}`);
}

// Cargo reports the resolved artifact location, including target-dir config and
// target triples. Never fall back to a possibly stale root/target executable.
const artifacts = (build.stdout ?? "").split(/\r?\n/).filter((line) => line.trim()).map((line) => JSON.parse(line)).filter((message) =>
  message?.reason === "compiler-artifact"
  && message.manifest_path === resolve(root, "crates", "elsecrew-sidecar", "Cargo.toml")
  && message.target?.name === "elsecrew-sidecar"
  && Array.isArray(message.target.kind) && message.target.kind.includes("bin")
  && message.profile?.test === false
  && message.profile?.debug_assertions === false
);
if (artifacts.length !== 1 || typeof artifacts[0].executable !== "string" || !isAbsolute(artifacts[0].executable)) {
  throw new Error("sidecar build did not report exactly one executable artifact");
}
const source = artifacts[0].executable;
if (!Array.isArray(artifacts[0].features) || artifacts[0].features.includes("qa-output-fixtures")) {
  throw new Error("production sidecar staging requires a non-QA artifact");
}

const destinationDirectory = resolve(resourcesDirectory, "sidecar");
mkdirSync(destinationDirectory, { recursive: true });
const destination = resolve(destinationDirectory, binary);
copyFileSync(source, destination);
if (process.platform !== "win32") chmodSync(destination, 0o755);
