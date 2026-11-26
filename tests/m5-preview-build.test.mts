// Build-then-preview (결과 미리보기 확장 1단계): the runner's policy, its
// process lifecycle with a fake process, the contracts and the tab's model.
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";

import { PreviewBuildManagerV1, type PreviewBuildOptionsV1 } from "../electron/preview-build.ts";
import {
  PreviewLineSplitter, findTool, newestNodeVersions, previewBuildArgs, previewBuildCommandLine, previewBuildConsentBox,
  previewBuildEnvironment, previewBuildLine, previewBuildLogName, previewToolDirs,
} from "../electron/preview-build-policy.ts";
import { parsePreviewRequestUrl } from "../electron/preview-policy.ts";
import {
  PREVIEW_BUILD_LOG_MAX_LINES_V1, PREVIEW_METHODS_V1, isPreviewOutputDirV1, validatePreviewBuildIpcRequestV1, validatePreviewBuildIpcResultV1,
  validatePreviewBuildPlanResultV1, validatePreviewIpcRequestV1, type PreviewBuildJobViewV1, type PreviewBuildPlanResultV1,
} from "../src/m5/contracts/preview-v1.ts";
import { appendBuildLines, previewBuildBar, previewBuildFinished, previewBuildStatus } from "../src/m5/preview-build-model.ts";

const TOKEN = "a".repeat(32);

test("fixed commands per package manager: a lockfile is followed, never rewritten", () => {
  assert.deepEqual(previewBuildArgs("npm", "build", true), ["run", "build"]);
  assert.deepEqual(previewBuildArgs("pnpm", "build", false), ["run", "build"]);
  assert.deepEqual(previewBuildArgs("npm", "install", true), ["ci", "--no-audit", "--no-fund"]);
  assert.deepEqual(previewBuildArgs("npm", "install", false), ["install", "--no-package-lock", "--no-audit", "--no-fund"]);
  assert.deepEqual(previewBuildArgs("pnpm", "install", true), ["install", "--frozen-lockfile"]);
  assert.deepEqual(previewBuildArgs("pnpm", "install", false), ["install", "--lockfile=false"]);
  assert.deepEqual(previewBuildArgs("yarn", "install", true), ["install", "--frozen-lockfile"]);
  assert.equal(previewBuildCommandLine("npm", "build", true), "npm run build");
});

test("the build's environment: an allow-list, no app settings or keys, a PATH of found tools", () => {
  const env = previewBuildEnvironment({
    HOME: "/Users/me", USER: "me", LANG: "ko_KR.UTF-8", PATH: "/evil/bin", TMPDIR: "/tmp/x",
    ANTHROPIC_API_KEY: "sk-ant-x", OPENAI_API_KEY: "sk-x", GITHUB_TOKEN: "ghp_x", ELSECREW_PREVIEW: "1", NODE_OPTIONS: "--require /evil.js",
    npm_config__authToken: "x", AWS_SECRET_ACCESS_KEY: "x", SSH_AUTH_SOCK: "/tmp/ssh",
  }, ["/opt/homebrew/bin", "/usr/bin", "/opt/homebrew/bin"]);
  assert.deepEqual(Object.keys(env).sort(), ["BROWSER", "FORCE_COLOR", "HOME", "LANG", "NO_COLOR", "NO_UPDATE_NOTIFIER", "PATH", "TMPDIR", "USER", "npm_config_fund", "npm_config_update_notifier"]);
  assert.equal(env.PATH, "/opt/homebrew/bin:/usr/bin");
  assert.equal(env.HOME, "/Users/me");
});

test("tools are looked up in the inherited PATH, then the usual places, newest nvm first", () => {
  assert.deepEqual(newestNodeVersions(["v9.11.2", "v24.2.0", "v18.20.1", "junk", "v24.10.0"]), ["v24.10.0", "v24.2.0", "v18.20.1", "v9.11.2"]);
  const dirs = previewToolDirs("/usr/bin:relative/bin:/custom/bin", "/Users/me", ["v24.10.0"]);
  assert.deepEqual(dirs.slice(0, 2), ["/usr/bin", "/custom/bin"]);
  assert.ok(dirs.includes("/opt/homebrew/bin") && dirs.includes("/Users/me/.nvm/versions/node/v24.10.0/bin") && dirs.includes("/Users/me/.volta/bin"));
  assert.ok(!dirs.includes("relative/bin"));
  assert.equal(findTool("npm", ["/a", "/b"], path => path === "/b/npm"), "/b/npm");
  assert.equal(findTool("npm", ["/a"], () => false), null);
});

test("log lines: colours and progress frames off, secrets masked, bounded", () => {
  assert.equal(previewBuildLine("\u001b[32m✓\u001b[39m built in 1.2s"), "✓ built in 1.2s");
  assert.equal(previewBuildLine("50%\r75%\r100% done"), "100% done");
  const masked = previewBuildLine("TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123 and sk-ant-api03-abcdefghijklmnopqrstuvwxyz");
  assert.ok(!masked.includes("abcdefghijklmnopqrstuvwxyz"), masked);
  assert.equal(previewBuildLine("x".repeat(5000)).length, 2000);
  const split = new PreviewLineSplitter();
  assert.deepEqual(split.push("a\nb"), ["a"]);
  assert.deepEqual(split.push("c\n"), ["bc"]);
  assert.deepEqual(split.push("tail"), []);
  assert.deepEqual(split.flush(), ["tail"]);
  assert.match(previewBuildLogName("project_1", "chat_1"), /^[0-9a-f]{32}\.log$/);
  assert.ok(!previewBuildLogName("project_1", "chat_1").includes("chat"));
});

test("main's consent box shows the exact command, program and folder; the network for installing", () => {
  const consent = { step: "install" as const, command: "npm ci --no-audit --no-fund", program: "/opt/homebrew/bin/npm", folder: "/Users/me/elsecrew/workspaces/app/t1", buildScript: null };
  const ko = previewBuildConsentBox(consent, true);
  assert.match(ko.message, /인터넷/);
  for (const shown of ["실행할 명령: npm ci --no-audit --no-fund", "프로그램: /opt/homebrew/bin/npm", "위치: /Users/me/elsecrew/workspaces/app/t1"]) assert.ok(ko.detail.includes(shown), shown);
  assert.deepEqual(ko.buttons, ["취소", "설치"]);
  const build = previewBuildConsentBox({ ...consent, step: "build", command: "npm run build", buildScript: "vite build" }, false);
  assert.ok(build.detail.includes("Build script: vite build") && build.detail.includes("outside the preview's protection"));
  assert.deepEqual(build.buttons, ["Cancel", "Build"]);
});

test("a build output's paths and a single-page app's routes", () => {
  const url = (path: string) => `elsecrew-preview://${TOKEN}${path}`;
  const parse = (path: string, root: string | null = "dist") => parsePreviewRequestUrl(url(path), TOKEN, root ? `${root}/index.html` : "index.html", root);
  assert.deepEqual(parse("/"), { ok: true, path: "dist/index.html" });
  assert.deepEqual(parse("/assets/index-abc.js"), { ok: true, path: "dist/assets/index-abc.js" });
  assert.deepEqual(parse("/about"), { ok: true, path: "dist/index.html" });
  assert.deepEqual(parse("/users/42/"), { ok: true, path: "dist/index.html" });
  assert.deepEqual(parse("/favicon.ico"), { ok: true, path: "dist/favicon.ico" });
  for (const refused of ["/.env", "/assets/.hidden.js", "/%2e%2e/x.js", "/a//b.js", "/x.exe"]) assert.equal(parse(refused).ok, false, refused);
  // Without a root nothing changes: no fallback, workspace paths.
  assert.deepEqual(parse("/about", null), { ok: false, reason: "extension" });
  assert.deepEqual(parse("/assets/a.js", null), { ok: true, path: "assets/a.js" });
});

test("the sidecar's plan and the IPC are strict: the page names no folder or command", () => {
  assert.deepEqual([...PREVIEW_METHODS_V1], ["preview.resolve", "preview.readAsset", "preview.buildPlan"]);
  const request = { projectId: "p", chatId: "c" };
  const plan = { chatId: "c", projectId: "p", worktreeId: "w", workspaceIdentity: "i", refusal: null, workspacePath: "/Users/me/ws", packageManager: "npm", lockfile: true, buildScript: "vite build", outputDir: "dist", needsInstall: false };
  assert.equal(validatePreviewBuildPlanResultV1(plan, request).outputDir, "dist");
  for (const bad of [
    { ...plan, chatId: "other" }, { ...plan, workspacePath: "relative" }, { ...plan, outputDir: "../out" }, { ...plan, outputDir: ".git" },
    { ...plan, outputDir: "src" }, { ...plan, packageManager: "bun" }, { ...plan, refusal: "noPackage" }, { ...plan, extra: 1 }, { ...plan, refusal: "nope" },
  ]) assert.throws(() => validatePreviewBuildPlanResultV1(bad, request), JSON.stringify(bad));
  const refused = { ...plan, refusal: "outputNotIgnored", workspacePath: null, packageManager: null, lockfile: false, buildScript: null, outputDir: null, needsInstall: false };
  assert.equal(validatePreviewBuildPlanResultV1(refused, request).refusal, "outputNotIgnored");
  assert.ok(isPreviewOutputDirV1("out/web") && !isPreviewOutputDirV1("node_modules/x") && !isPreviewOutputDirV1("a/../b"));

  assert.deepEqual(validatePreviewBuildIpcRequestV1({ action: "start", projectId: "p", chatId: "c", step: "build" }), { action: "start", projectId: "p", chatId: "c", step: "build" });
  for (const bad of [
    { action: "start", projectId: "p", chatId: "c", step: "dev" }, { action: "start", projectId: "p", chatId: "c", step: "build", cwd: "/" },
    { action: "start", projectId: "p", chatId: "c", step: "build", command: "rm" }, { action: "status", projectId: "p", chatId: "c", cursor: -1 }, { action: "run" },
  ]) assert.throws(() => validatePreviewBuildIpcRequestV1(bad), JSON.stringify(bad));
  const job = { step: "build", state: "running", cursor: 2, lines: ["a", "b"], exitCode: null, outputReady: false, startedAt: 1, endedAt: null };
  const status = { action: "status", projectId: "p", chatId: "c", cursor: 0 } as const;
  assert.equal(validatePreviewBuildIpcResultV1({ action: "status", job }, status).action, "status");
  assert.throws(() => validatePreviewBuildIpcResultV1({ action: "status", job: { ...job, lines: Array(PREVIEW_BUILD_LOG_MAX_LINES_V1 + 1).fill("x") } }, status));
  assert.throws(() => validatePreviewBuildIpcResultV1({ action: "plan", plan: { refusal: null, installCommand: null, buildCommand: null, needsInstall: false, outputDir: null, workspacePath: "/x" }, job }, { action: "plan", projectId: "p", chatId: "c" }));
  // The preview's open: a build source names no entry.
  const open = { action: "open", projectId: "p", chatId: "c", entry: null, source: "build", bounds: { x: 0, y: 0, width: 300, height: 300 }, viewport: "wide", pixelRatio: 1, page: { width: 800, height: 600 } };
  assert.equal((validatePreviewIpcRequestV1(open) as { source: string }).source, "build");
  assert.throws(() => validatePreviewIpcRequestV1({ ...open, entry: "dist/index.html" }));
  assert.throws(() => validatePreviewIpcRequestV1({ ...open, source: "server" }));
  assert.throws(() => validatePreviewIpcRequestV1({ ...open, source: undefined }));
});

// ---------------------------------------------------------------------------
// The runner with a fake process.

type FakeChild = EventEmitter & { stdout: EventEmitter & { setEncoding(): void }; stderr: EventEmitter & { setEncoding(): void }; pid: number };

function runner(overrides: Partial<PreviewBuildOptionsV1> = {}, planOverrides: Partial<PreviewBuildPlanResultV1> = {}) {
  const spawns: { program: string; args: string[]; cwd: unknown }[] = [];
  const children: FakeChild[] = [];
  const kills: [number, string][] = [];
  const logs: Record<string, string> = {};
  const confirms: unknown[] = [];
  let answer = true;
  let plan: PreviewBuildPlanResultV1 = { chatId: "c1", projectId: "p", worktreeId: "w", workspaceIdentity: "i1", refusal: null, workspacePath: "/ws/one", packageManager: "npm", lockfile: true, buildScript: "vite build", outputDir: "dist", needsInstall: false, ...planOverrides };
  const manager = new PreviewBuildManagerV1({
    enabled: true,
    sidecar: () => ({ previewBuildPlan: async ({ chatId }) => ({ ...plan, chatId }) }),
    confirm: async (consent) => { confirms.push(consent); return answer; },
    findTools: () => ({ program: "/bin/npm", pathDirs: ["/bin"] }),
    isWorkspaceFolder: () => true,
    spawn: (program, args, options) => {
      spawns.push({ program, args, cwd: options.cwd });
      const stream = () => Object.assign(new EventEmitter(), { setEncoding() {} });
      const child = Object.assign(new EventEmitter(), { stdout: stream(), stderr: stream(), pid: 100 + children.length }) as FakeChild;
      children.push(child);
      return child as never;
    },
    killGroup: (pid, signal) => { kills.push([pid, signal]); },
    environment: { HOME: "/Users/me" },
    writeLog: (name, text) => { logs[name] = text; },
    log: () => undefined,
    timeoutMs: { install: 60, build: 40 },
    killGraceMs: 20,
    ...overrides,
  });
  const ask = (action: "start" | "cancel" | "status" | "plan", chatId = "c1", extra: Record<string, unknown> = {}) =>
    manager.request({ action, projectId: "p", chatId, ...extra } as never) as Promise<{ outcome?: string; job: PreviewBuildJobViewV1; plan?: unknown }>;
  return {
    manager, spawns, children, kills, logs, confirms, ask,
    setAnswer(value: boolean) { answer = value; },
    setPlan(next: Partial<PreviewBuildPlanResultV1>) { plan = { ...plan, ...next }; },
  };
}
const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

test("a build runs once agreed, keeps a masked log, and its output opens in the preview", async () => {
  const r = runner();
  r.setAnswer(false);
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "declined");
  assert.equal(r.spawns.length, 0);
  r.setAnswer(true);
  const started = await r.ask("start", "c1", { step: "build" });
  assert.equal(started.outcome, "started");
  assert.deepEqual(r.spawns, [{ program: "/bin/npm", args: ["run", "build"], cwd: "/ws/one" }]);
  assert.equal(r.manager.buildOutput("p", "c1"), null, "nothing to show while it runs");
  const child = r.children[0]!;
  child.stdout.emit("data", "vite v6\nsecret=hunter2hunter2\n");
  child.stderr.emit("data", "warn");
  const status = await r.ask("status", "c1", { cursor: 0 });
  assert.deepEqual(status.job.lines, ["$ npm run build", "vite v6", "secret=••••"]);
  child.emit("close", 0);
  const done = await r.ask("status", "c1", { cursor: status.job.cursor });
  assert.deepEqual(done.job.lines, ["warn"]);
  assert.equal(done.job.state, "succeeded");
  assert.equal(done.job.outputReady, true);
  assert.deepEqual(r.manager.buildOutput("p", "c1"), { dir: "dist", workspaceIdentity: "i1" });
  // What the leader left in its own group goes with it; the log is kept masked.
  assert.deepEqual(r.kills, [[100, "SIGKILL"]]);
  const log = Object.values(r.logs)[0]!;
  assert.ok(log.includes("vite v6") && !log.includes("hunter2"));
  // A failed build shows no output.
  await r.ask("start", "c1", { step: "build" });
  r.children[1]!.emit("close", 1);
  assert.equal((await r.ask("status", "c1", { cursor: 0 })).job.state, "failed");
  assert.equal(r.manager.buildOutput("p", "c1"), null);
});

test("one job per task, two over all; building waits for the packages", async () => {
  const r = runner();
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "started");
  assert.equal((await r.ask("start", "c1", { step: "install" })).outcome, "busy");
  assert.equal((await r.ask("start", "c2", { step: "build" })).outcome, "started");
  assert.equal((await r.ask("start", "c3", { step: "build" })).outcome, "busy");
  assert.equal(r.confirms.length, 2, "busy never asks");
  r.children[0]!.emit("close", 0);
  r.setPlan({ needsInstall: true });
  assert.equal((await r.ask("start", "c3", { step: "build" })).outcome, "refused");
  assert.equal((await r.ask("start", "c3", { step: "install" })).outcome, "started");
  assert.deepEqual(r.spawns.at(-1)!.args, ["ci", "--no-audit", "--no-fund"]);
  r.children[1]!.emit("close", 0);
  r.setPlan({ refusal: "outputNotIgnored", workspacePath: null, packageManager: null, buildScript: null, outputDir: null, needsInstall: false, lockfile: false });
  assert.equal((await r.ask("start", "c4", { step: "build" })).outcome, "refused");
  const plan = await r.ask("plan", "c4");
  assert.deepEqual(plan.plan, { refusal: "outputNotIgnored", installCommand: null, buildCommand: null, needsInstall: false, outputDir: null });
});

test("stopping, the time limit and quitting end the build's own group", async () => {
  const r = runner();
  await r.ask("start", "c1", { step: "build" });
  await r.ask("cancel", "c1");
  assert.deepEqual(r.kills, [[100, "SIGTERM"]]);
  await wait(40);
  assert.deepEqual(r.kills, [[100, "SIGTERM"], [100, "SIGKILL"]], "a build that ignores the request is killed after the grace");
  r.children[0]!.emit("close", null);
  assert.equal((await r.ask("status", "c1", { cursor: 0 })).job.state, "cancelled");

  r.kills.length = 0;
  await r.ask("start", "c2", { step: "build" });
  await wait(60);
  assert.deepEqual(r.kills[0], [101, "SIGTERM"]);
  r.children[1]!.emit("close", null);
  assert.equal((await r.ask("status", "c2", { cursor: 0 })).job.state, "timedOut");

  r.kills.length = 0;
  await r.ask("start", "c3", { step: "build" });
  r.manager.stopAll();
  assert.deepEqual(r.kills, [[102, "SIGTERM"]]);
  r.children[2]!.emit("close", null);
  await r.ask("start", "c4", { step: "build" });
  r.kills.length = 0;
  r.manager.shutdown();
  assert.deepEqual(r.kills, [[103, "SIGKILL"]]);
  await assert.rejects(r.ask("status", "c4", { cursor: 0 }), /quits/);
});

test("the log keeps the newest lines and a cursor that never goes back", async () => {
  const r = runner();
  await r.ask("start", "c1", { step: "build" });
  const child = r.children[0]!;
  child.stdout.emit("data", Array.from({ length: PREVIEW_BUILD_LOG_MAX_LINES_V1 + 10 }, (_, index) => `line ${index}`).join("\n") + "\n");
  const all = await r.ask("status", "c1", { cursor: 0 });
  assert.equal(all.job.lines.length, PREVIEW_BUILD_LOG_MAX_LINES_V1);
  assert.equal(all.job.lines.at(-1), `line ${PREVIEW_BUILD_LOG_MAX_LINES_V1 + 9}`);
  assert.equal(all.job.cursor, PREVIEW_BUILD_LOG_MAX_LINES_V1 + 11);
  const next = await r.ask("status", "c1", { cursor: all.job.cursor });
  assert.deepEqual(next.job.lines, []);
});

test("the tab's build bar and its words", () => {
  const plan = { refusal: null, installCommand: "npm ci", buildCommand: "npm run build", needsInstall: false, outputDir: "dist" } as const;
  const job = (state: PreviewBuildJobViewV1["state"], step: PreviewBuildJobViewV1["step"] = "build", outputReady = false): PreviewBuildJobViewV1 => ({ step, state, cursor: 0, lines: [], exitCode: null, outputReady, startedAt: 1, endedAt: null });
  assert.deepEqual(previewBuildBar(null, null, false), { kind: "hidden" });
  for (const refusal of ["noPackage", "noBuildScript", "unsupportedWorkspace", "unavailable"] as const) assert.deepEqual(previewBuildBar({ ...plan, refusal }, null, false), { kind: "hidden" });
  assert.deepEqual(previewBuildBar({ ...plan, refusal: "outputNotIgnored" }, null, false), { kind: "note", note: "notIgnored" });
  assert.deepEqual(previewBuildBar({ ...plan, refusal: "toolMissing" }, null, false), { kind: "note", note: "toolMissing" });
  const ready = previewBuildBar(plan, null, false);
  assert.ok(ready.kind === "ready" && ready.canBuild && ready.canInstall);
  const needs = previewBuildBar({ ...plan, needsInstall: true }, null, false);
  assert.ok(needs.kind === "ready" && !needs.canBuild && needs.canInstall);
  const working = previewBuildBar(plan, null, true);
  assert.ok(working.kind === "ready" && !working.canBuild && !working.canInstall);
  const running = previewBuildBar(plan, job("running", "install"), false);
  assert.ok(running.kind === "ready" && running.running === "install" && !running.canBuild);
  assert.equal(previewBuildStatus(job("succeeded", "install"), null), "installed");
  assert.equal(previewBuildStatus(job("failed"), null), "buildFailed");
  assert.equal(previewBuildStatus(job("running"), null), null);
  assert.equal(previewBuildStatus(null, "declined"), "declined");
  assert.equal(previewBuildFinished(job("running"), job("succeeded", "build", true)), "built");
  assert.equal(previewBuildFinished(job("running", "install"), job("succeeded", "install")), "installed");
  assert.equal(previewBuildFinished(job("running"), job("failed")), "ended");
  assert.equal(previewBuildFinished(job("succeeded"), job("succeeded", "build", true)), null);
  assert.equal(appendBuildLines(Array(PREVIEW_BUILD_LOG_MAX_LINES_V1).fill("a"), ["b"]).at(-1), "b");
});
