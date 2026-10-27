import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, isAbsolute, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";

const scriptUrl = new URL("../scripts/m5-stage-sidecar.mjs", import.meta.url);
const root = resolve(dirname(fileURLToPath(scriptUrl)), "..");
// Execute the real script body with only built-in imports/process replaced.
// Neither Cargo nor filesystem mutations are allowed in these focused tests.
const script = readFileSync(scriptUrl, "utf8")
  .replace(/^import .* from "node:[^"]+";\n/gm, "")
  .replace("import.meta.url", "scriptUrl");

function artifact(executable: unknown, overrides: Record<string, unknown> = {}) {
  return {
    reason: "compiler-artifact",
    manifest_path: resolve(root, "crates", "hello-agent-sidecar", "Cargo.toml"),
    target: { name: "hello-agent-sidecar", kind: ["bin"] },
    profile: { test: false, debug_assertions: false },
    executable,
    ...overrides,
  };
}

function stage(options: {
  messages?: unknown[];
  stdout?: string;
  status?: number | null;
  error?: Error;
  platform?: string;
  env?: Record<string, string>;
  resources?: string;
} = {}) {
  const writes: unknown[][] = [];
  const calls: unknown[][] = [];
  const context = {
    dirname, isAbsolute, resolve, fileURLToPath,
    scriptUrl: scriptUrl.href,
    process: {
      argv: ["node", fileURLToPath(scriptUrl), options.resources ?? "/stage resources"],
      platform: options.platform ?? "darwin",
      env: options.env ?? {},
    },
    spawnSync: (...args: unknown[]) => {
      calls.push(args);
      return {
        status: options.status === undefined ? 0 : options.status,
        error: options.error,
        stdout: options.stdout ?? (options.messages ?? []).map((message) => JSON.stringify(message)).join("\n"),
      };
    },
    mkdirSync: (...args: unknown[]) => writes.push(["mkdir", ...args]),
    copyFileSync: (...args: unknown[]) => writes.push(["copy", ...args]),
    chmodSync: (...args: unknown[]) => writes.push(["chmod", ...args]),
  };
  let error: unknown;
  try {
    runInNewContext(script, context, { filename: fileURLToPath(scriptUrl) });
  } catch (caught) {
    error = caught;
  }
  // VM array/object prototypes are deliberately normalized for assertions.
  return { error, writes: JSON.parse(JSON.stringify(writes)), calls: JSON.parse(JSON.stringify(calls)) };
}

for (const [name, executable, env] of [
  ["default target", resolve(root, "target/release/hello-agent-sidecar"), {}],
  ["shared environment target", "/shared build/release/hello-agent-sidecar", { CARGO_TARGET_DIR: "/shared build" }],
  ["configured target triple", "/configured target/aarch64-apple-darwin/release/hello-agent-sidecar", {}],
] as const) {
  test(`sidecar staging uses Cargo's exact executable for ${name}`, () => {
    const result = stage({ messages: [artifact(executable)], env });
    assert.equal(result.error, undefined);
    assert.deepEqual(result.writes, [
      ["mkdir", "/stage resources/sidecar", { recursive: true }],
      ["copy", executable, "/stage resources/sidecar/hello-agent-sidecar"],
      ["chmod", "/stage resources/sidecar/hello-agent-sidecar", 0o755],
    ]);
    assert.deepEqual(result.calls, [["cargo", [
      "build", "--locked", "--release", "-p", "hello-agent-sidecar", "--bin", "hello-agent-sidecar", "--message-format=json-render-diagnostics",
    ], { cwd: root, stdio: ["inherit", "pipe", "inherit"], encoding: "utf8" }]]);
  });
}

test("sidecar staging ignores unrelated artifacts and accepts fresh cached output", () => {
  const executable = "/shared/release/hello-agent-sidecar";
  const result = stage({ messages: [
    { reason: "compiler-message", message: { rendered: "a warning" } },
    artifact("/wrong-package", { manifest_path: "/dependency/Cargo.toml" }),
    artifact("/wrong-name", { target: { name: "other", kind: ["bin"] } }),
    artifact("/wrong-kind", { target: { name: "hello-agent-sidecar", kind: ["lib"] } }),
    artifact("/test-harness", { profile: { test: true } }),
    artifact(executable, { fresh: true }),
    { reason: "build-finished", success: true },
  ] });
  assert.equal(result.error, undefined);
  assert.equal(result.writes.find((write: unknown[]) => write[0] === "copy")[1], executable);
});

test("sidecar staging preserves Windows destination naming without chmod", () => {
  const result = stage({ platform: "win32", messages: [artifact("/cross/release/hello-agent-sidecar.exe")] });
  assert.equal(result.error, undefined);
  assert.deepEqual(result.writes, [
    ["mkdir", "/stage resources/sidecar", { recursive: true }],
    ["copy", "/cross/release/hello-agent-sidecar.exe", "/stage resources/sidecar/hello-agent-sidecar.exe"],
  ]);
});

test("sidecar staging rejects fixture-capable debug assertions and unknown profiles", () => {
  for (const profile of [{ test: false, debug_assertions: true }, { test: false }]) {
    const result = stage({ messages: [artifact("/overridden/release/hello-agent-sidecar", { profile })] });
    assert.ok(result.error);
    assert.equal(result.writes.length, 0);
  }
});

test("sidecar staging never copies an artifact after Cargo failure or spawn error", () => {
  for (const failure of [{ status: 101 }, { status: null }, { error: new Error("spawn cargo ENOENT") }]) {
    const result = stage({ ...failure, messages: [artifact("/stale/hello-agent-sidecar")] });
    assert.ok(result.error);
    assert.deepEqual(result.writes, []);
  }
});

test("sidecar staging rejects absent, ambiguous or invalid executable artifacts before mutation", () => {
  for (const messages of [
    [],
    [{ reason: "build-finished", success: true }],
    [artifact("/first"), artifact("/second")],
    [artifact("/same"), artifact("/same")],
    [artifact(null)],
    [artifact(42)],
    [artifact("")],
    [artifact("relative/hello-agent-sidecar")],
    [artifact("/wrong-kind-type", { target: { name: "hello-agent-sidecar", kind: "bin" } })],
  ]) {
    const result = stage({ messages });
    assert.match(String(result.error), /exactly one executable artifact/);
    assert.deepEqual(result.writes, []);
  }
  const malformed = stage({ stdout: `${JSON.stringify(artifact("/valid"))}\n{truncated` });
  assert.ok(malformed.error);
  assert.deepEqual(malformed.writes, []);
});
