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
  // Installing never runs the packages' install scripts (PR #22 review B1·B2).
  assert.deepEqual(previewBuildArgs("npm", "install", true), ["ci", "--ignore-scripts", "--no-audit", "--no-fund"]);
  assert.deepEqual(previewBuildArgs("npm", "install", false), ["install", "--ignore-scripts", "--no-package-lock", "--no-audit", "--no-fund"]);
  assert.deepEqual(previewBuildArgs("pnpm", "install", true), ["install", "--frozen-lockfile", "--ignore-scripts"]);
  assert.deepEqual(previewBuildArgs("pnpm", "install", false), ["install", "--lockfile=false", "--ignore-scripts"]);
  assert.deepEqual(previewBuildArgs("yarn", "install", true), ["install", "--frozen-lockfile", "--ignore-scripts"]);
  assert.deepEqual(previewBuildArgs("yarn", "install", true, true), ["install", "--immutable", "--mode=skip-build"]);
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
  // Too long to keep whole: hidden, never cut (a key could straddle the cut).
  assert.equal(previewBuildLine("x ".repeat(2500)), "(가림)");
  const split = new PreviewLineSplitter();
  assert.deepEqual(split.push("a\nb"), ["a"]);
  assert.deepEqual(split.push("c\n"), ["bc"]);
  assert.deepEqual(split.push("tail"), []);
  assert.deepEqual(split.flush(), ["tail"]);
  assert.match(previewBuildLogName("project_1", "chat_1"), /^[0-9a-f]{32}\.log$/);
  assert.ok(!previewBuildLogName("project_1", "chat_1").includes("chat"));
});

test("main's consent box shows the exact command, program and folder; the network for installing", () => {
  const consent = { step: "install" as const, command: "npm ci --ignore-scripts --no-audit --no-fund", program: "/opt/homebrew/bin/npm", folder: "/Users/me/elsecrew/workspaces/app/t1",
    buildScript: null, hooks: [], dependencies: ["react", "vite"], dependencyCount: 12, changed: false, folderIdentity: "16777232:4242",
    inputs: [{ name: "package.json", sha256: "ab".repeat(32) }, { name: "package-lock.json", sha256: "cd".repeat(32) }], userConfigs: ["~/.npmrc"],
    caches: ["/Users/me/.npm/_cacache"], hidden: ["/Users/me/.ssh", "/Users/me/Library/Keychains"] };
  const ko = previewBuildConsentBox(consent, true);
  assert.match(ko.message, /인터넷/);
  for (const shown of ["실행할 명령: npm ci --ignore-scripts --no-audit --no-fund", "프로그램: /opt/homebrew/bin/npm", "위치: /Users/me/elsecrew/workspaces/app/t1",
    "꾸러미 12개: react, vite 외 10개", "설치 스크립트는 실행하지 않아요", "그 밖의 내 파일은 읽을 수 있어요",
    // PR #22 re-review B1·B2: every input and its hash, the folder's identity, the person's settings, and the sandbox's own lists.
    `확인한 파일: package.json (${"ab".repeat(8)}), package-lock.json (${"cd".repeat(8)})`, "(폴더 16777232:4242)", "적용되는 내 설정: ~/.npmrc",
    "쓸 수 있는 곳: 이 작업 공간, 이 설치 전용 임시 폴더, /Users/me/.npm/_cacache.", "읽지도 쓰지도 못하는 곳: /Users/me/.ssh, /Users/me/Library/Keychains."]) assert.ok(ko.detail.includes(shown), shown);
  assert.deepEqual(ko.buttons, ["취소", "설치"]);
  // PR #22 review B2: no claim that the output stays inside; what the sandbox allows is said plainly.
  const build = previewBuildConsentBox({ ...consent, step: "build", command: "npm run build", buildScript: "vite build", hooks: [{ name: "postbuild", script: "node copy.js" }] }, false);
  assert.ok(build.detail.includes("Build script: vite build") && build.detail.includes("Also runs: postbuild — node copy.js"));
  assert.ok(build.detail.includes("cannot use the internet") && build.detail.includes("It can read your other files"));
  assert.ok(!/stays inside/.test(build.detail));
  assert.ok(!previewBuildConsentBox({ ...consent, step: "build" }, true).detail.includes("결과는 작업 공간 안에만"));
  assert.match(previewBuildConsentBox({ ...consent, changed: true }, true).message, /^내용이 바뀌어 다시 확인해요/);
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
  assert.deepEqual([...PREVIEW_METHODS_V1], ["preview.resolve", "preview.readAsset", "preview.buildPlan", "preview.buildReserve", "preview.buildRelease", "preview.buildHolds"]);
  const request = { projectId: "p", chatId: "c" };
  const plan = { chatId: "c", projectId: "p", worktreeId: "w", workspaceIdentity: "i", refusal: null, workspacePath: "/Users/me/ws", packageManager: "npm", lockfile: true, buildScript: "vite build",
    buildHooks: [{ name: "prebuild", script: "x" }], dependencies: ["react"], dependencyCount: 1, yarnBerry: false, outputDir: "dist", needsInstall: false, fingerprint: "f".repeat(64),
    folderIdentity: "1:2", inputs: [{ name: "package.json", sha256: "e".repeat(64) }] };
  assert.equal(validatePreviewBuildPlanResultV1(plan, request).outputDir, "dist");
  for (const bad of [
    { ...plan, chatId: "other" }, { ...plan, workspacePath: "relative" }, { ...plan, outputDir: "../out" }, { ...plan, outputDir: ".git" },
    { ...plan, outputDir: "src" }, { ...plan, packageManager: "bun" }, { ...plan, refusal: "noPackage" }, { ...plan, extra: 1 }, { ...plan, refusal: "nope" },
    { ...plan, fingerprint: "short" }, { ...plan, buildHooks: [{ name: "preinstall", script: "x" }] }, { ...plan, dependencyCount: 0 },
    { ...plan, inputs: [] }, { ...plan, inputs: [{ name: "../x", sha256: "e".repeat(64) }] }, { ...plan, folderIdentity: "x" },
  ]) assert.throws(() => validatePreviewBuildPlanResultV1(bad, request), JSON.stringify(bad));
  const refused = { ...plan, refusal: "outputNotIgnored", workspacePath: null, packageManager: null, lockfile: false, buildScript: null, outputDir: null, needsInstall: false,
    buildHooks: null, dependencies: null, dependencyCount: null, fingerprint: null, folderIdentity: null, inputs: null };
  assert.equal(validatePreviewBuildPlanResultV1(refused, request).refusal, "outputNotIgnored");
  assert.ok(isPreviewOutputDirV1("out/web") && !isPreviewOutputDirV1("node_modules/x") && !isPreviewOutputDirV1("a/../b"));

  assert.deepEqual(validatePreviewBuildIpcRequestV1({ action: "start", projectId: "p", chatId: "c", step: "build" }), { action: "start", projectId: "p", chatId: "c", step: "build" });
  for (const bad of [
    { action: "start", projectId: "p", chatId: "c", step: "dev" }, { action: "start", projectId: "p", chatId: "c", step: "build", cwd: "/" },
    { action: "start", projectId: "p", chatId: "c", step: "build", command: "rm" }, { action: "status", projectId: "p", chatId: "c", cursor: -1 }, { action: "run" },
  ]) assert.throws(() => validatePreviewBuildIpcRequestV1(bad), JSON.stringify(bad));
  const job = { step: "build", state: "running", cursor: 2, lines: ["a", "b"], exitCode: null, outputReady: false, startedAt: 1, endedAt: null, leftovers: 0 };
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
  let plan: PreviewBuildPlanResultV1 = { chatId: "c1", projectId: "p", worktreeId: "w", workspaceIdentity: "i1", refusal: null, workspacePath: "/ws/one", packageManager: "npm", lockfile: true,
    buildScript: "vite build", buildHooks: [], dependencies: ["react", "vite"], dependencyCount: 2, yarnBerry: false, outputDir: "dist", needsInstall: false, fingerprint: "a".repeat(64),
    folderIdentity: "1:2", inputs: [{ name: "package.json", sha256: "b".repeat(64) }], ...planOverrides };
  let tools = { program: "/bin/npm", pathDirs: ["/bin"], identity: "npm-1", userConfigs: [] as string[] };
  const holds: string[] = [];
  const released: string[] = [];
  const temps: string[] = [];
  let reserve: "ok" | "busy" | "lost" = "ok";
  const syncs: { holds: { holdId: string; chatId: string }[]; leftoversSettled: boolean }[] = [];
  let engineBlocks = false;
  let records: unknown = null;
  /** Processes "alive" now, by pid → start time (the fake process table). */
  const table = new Map<number, { ppid: number; pgid: number; started: string }>();
  const killed: [number, string][] = [];
  const running: unknown[] = [];
  let written = true;
  /** The engine: answering, stopped (no client), or restarting (requests fail). */
  let engine: "up" | "down" | "failing" = "up";
  const manager = new PreviewBuildManagerV1({
    enabled: true,
    sidecar: () => engine === "down" ? undefined : ({
      previewBuildPlan: async ({ chatId }) => ({ ...plan, chatId }),
      previewBuildReserve: async ({ holdId }) => {
        if (reserve === "busy") return { holdId: null, refusal: "busy" as const };
        holds.push(holdId);
        // Held and written, then the engine stops before answering (PR #25 review B1).
        if (reserve === "lost") throw new Error("sidecar exited");
        return { holdId, refusal: null };
      },
      previewBuildRelease: async ({ holdId }) => { if (engine === "failing") throw new Error("sidecar exited"); released.push(holdId); return { holdId, released: true }; },
      previewBuildHolds: async (input) => { if (engine === "failing") throw new Error("sidecar exited"); syncs.push(input); return { held: input.holds.length, blocking: !input.leftoversSettled && engineBlocks }; },
    }),
    confirm: async (consent) => { confirms.push(consent); return typeof answer === "function" ? (answer as () => Promise<boolean>)() : answer; },
    findTools: () => tools,
    isWorkspaceFolder: () => true,
    places: () => ({ caches: ["/Users/me/.npm/_cacache"], hidden: ["/Users/me/.ssh"] }),
    makeTemp: () => { const temp = `/tmp/elsecrew-build-${temps.length}`; temps.push(temp); return temp; },
    removeTemp: (path) => { temps.splice(temps.indexOf(path), 1); },
    confine: (_step, _workspace, _temp, _places, program, args) => ({ program: "/usr/bin/sandbox-exec", args: ["-p", "(profile)", program, ...args] }),
    killProcess: (pid, started, signal) => { if (table.get(pid)?.started === started) { killed.push([pid, signal]); if (started !== "immortal") table.delete(pid); } },
    processes: () => [...table].map(([pid, entry]) => ({ pid, ...entry })),
    isAlive: (pid, started) => table.get(pid)?.started === started,
    saveRunning: (records) => { running.push(records); },
    loadRunning: () => { if (records instanceof Error) throw records; return records as never; },
    outputWritten: () => written,
    spawn: (program, args, options) => {
      spawns.push({ program, args, cwd: options.cwd });
      const stream = () => Object.assign(new EventEmitter(), { setEncoding() {}, destroy() {} });
      const child = Object.assign(new EventEmitter(), { stdout: stream(), stderr: stream(), pid: 100 + children.length }) as FakeChild;
      table.set(child.pid, { ppid: 1, pgid: child.pid, started: `t${child.pid}` });
      // The leader's end: it leaves the table, then Node reports exit and close.
      const emit = child.emit.bind(child);
      child.emit = ((event: string, ...rest: unknown[]) => { if (event === "close") { table.delete(child.pid); emit("exit", rest[0]); } return emit(event, ...rest); }) as typeof child.emit;
      children.push(child);
      return child as never;
    },
    killGroup: (pid, signal) => { kills.push([pid, signal]); for (const [other, entry] of table) if (entry.pgid === pid && signal === "SIGKILL" && entry.started !== "immortal") table.delete(other); },
    environment: { HOME: "/Users/me" },
    writeLog: (name, text) => { logs[name] = text; },
    log: () => undefined,
    timeoutMs: { install: 5_000, build: 5_000 },
    killGraceMs: 20,
    pipeGraceMs: 20,
    trackMs: 5,
    ...overrides,
  });
  const ask = (action: "start" | "cancel" | "status" | "plan", chatId = "c1", extra: Record<string, unknown> = {}) =>
    manager.request({ action, projectId: "p", chatId, ...extra } as never) as Promise<{ outcome?: string; job: PreviewBuildJobViewV1; plan?: unknown }>;
  return {
    manager, spawns, children, kills, logs, confirms, ask, table, killed, running, holds, released, temps, syncs,
    setReserve(value: "ok" | "busy" | "lost") { reserve = value; },
    setEngineBlocks(value: boolean) { engineBlocks = value; },
    setRecords(value: unknown) { records = value; },
    setAnswer(value: boolean | (() => Promise<boolean>)) { answer = value as boolean; },
    setPlan(next: Partial<PreviewBuildPlanResultV1>) { plan = { ...plan, ...next }; },
    setTools(next: Partial<typeof tools>) { tools = { ...tools, ...next }; },
    setWritten(value: boolean) { written = value; },
    setEngine(value: "up" | "down" | "failing") { engine = value; },
  };
}
const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

const settle = () => wait(360);

test("a build runs once agreed, in its sandbox, keeps a masked log, and its output opens in the preview", async () => {
  const r = runner();
  r.setAnswer(false);
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "declined");
  assert.equal(r.spawns.length, 0);
  r.setAnswer(true);
  const started = await r.ask("start", "c1", { step: "build" });
  assert.equal(started.outcome, "started");
  // The first line (the command) comes with the answer (review minor 2).
  assert.deepEqual(started.job.lines, ["$ npm run build"]);
  assert.deepEqual(r.spawns, [{ program: "/usr/bin/sandbox-exec", args: ["-p", "(profile)", "/bin/npm", "run", "build"], cwd: "/ws/one" }]);
  assert.equal(r.manager.buildOutput("p", "c1"), null, "nothing to show while it runs");
  const child = r.children[0]!;
  child.stdout.emit("data", "vite v6\nsecret=hunter2hunter2\n");
  child.stderr.emit("data", "warn");
  const status = await r.ask("status", "c1", { cursor: started.job.cursor });
  assert.deepEqual(status.job.lines, ["vite v6", "secret=••••"]);
  child.emit("close", 0);
  await settle();
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
  await settle();
  assert.equal((await r.ask("status", "c1", { cursor: 0 })).job.state, "failed");
  assert.equal(r.manager.buildOutput("p", "c1"), null);
});

test("PR #22 review I2·I1: a build counts only if it wrote a new page and nothing Git would record", async () => {
  const r = runner();
  r.setWritten(false);
  await r.ask("start", "c1", { step: "build" });
  r.children[0]!.emit("close", 0);
  await settle();
  const stale = await r.ask("status", "c1", { cursor: 0 });
  assert.equal(stale.job.state, "failed");
  assert.ok(stale.job.lines.some(line => line.includes("dist/index.html")));
  assert.equal(r.manager.buildOutput("p", "c1"), null);
  r.setWritten(true);
  await r.ask("start", "c1", { step: "build" });
  // The build wrote a file a `!negation` rule brings back into the task.
  r.setPlan({ refusal: "outputNotIgnored", workspacePath: null, packageManager: null, buildScript: null, buildHooks: null, dependencies: null, dependencyCount: null, outputDir: null, needsInstall: false, lockfile: false, fingerprint: null });
  r.children[1]!.emit("close", 0);
  await settle();
  assert.equal((await r.ask("status", "c1", { cursor: 0 })).job.state, "failed");
  assert.equal(r.manager.buildOutput("p", "c1"), null);
});

test("one job per task, two over all; building waits for the packages; installing runs no install scripts", async () => {
  const r = runner();
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "started");
  assert.equal((await r.ask("start", "c1", { step: "install" })).outcome, "busy");
  assert.equal((await r.ask("start", "c2", { step: "build" })).outcome, "started");
  assert.equal((await r.ask("start", "c3", { step: "build" })).outcome, "busy");
  assert.equal(r.confirms.length, 2, "busy never asks");
  r.children[0]!.emit("close", 0);
  await settle();
  r.setPlan({ needsInstall: true });
  assert.equal((await r.ask("start", "c3", { step: "build" })).outcome, "refused");
  assert.equal((await r.ask("start", "c3", { step: "install" })).outcome, "started");
  assert.deepEqual(r.spawns.at(-1)!.args.slice(2), ["/bin/npm", "ci", "--ignore-scripts", "--no-audit", "--no-fund"]);
  r.children[1]!.emit("close", 0);
  await settle();
  r.setPlan({ refusal: "outputNotIgnored", workspacePath: null, packageManager: null, buildScript: null, buildHooks: null, dependencies: null, dependencyCount: null, outputDir: null, needsInstall: false, lockfile: false, fingerprint: null });
  assert.equal((await r.ask("start", "c4", { step: "build" })).outcome, "refused");
  const plan = await r.ask("plan", "c4");
  assert.deepEqual(plan.plan, { refusal: "outputNotIgnored", installCommand: null, buildCommand: null, needsInstall: false, outputDir: null });
});

test("PR #22 review B1: a yes runs only what it was given; anything changed is asked again, never run on the old yes", async () => {
  // The lockfile (fingerprint) changes while the box is open; the second box says so; "취소" runs nothing.
  const r = runner();
  const answers: (() => boolean)[] = [() => { r.setPlan({ fingerprint: "b".repeat(64) }); return true; }, () => false];
  r.setAnswer(async () => answers.shift()!());
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "changed");
  assert.equal(r.spawns.length, 0);
  assert.deepEqual(r.confirms.map(consent => (consent as { changed: boolean }).changed), [false, true]);
  // The program changes (another npm at the same path): asked again; agreeing to what is now there runs it.
  const t = runner();
  const more: (() => boolean)[] = [() => { t.setTools({ identity: "npm-2" }); return true; }, () => true];
  t.setAnswer(async () => more.shift()!());
  assert.equal((await t.ask("start", "c1", { step: "build" })).outcome, "started");
  assert.equal(t.confirms.length, 2);
  // Another folder at the same path (workspace identity) or a path change is a change too.
  const f = runner();
  const third: (() => boolean)[] = [() => { f.setPlan({ workspacePath: "/ws/other" }); return true; }, () => false];
  f.setAnswer(async () => third.shift()!());
  assert.equal((await f.ask("start", "c1", { step: "build" })).outcome, "changed");
  assert.equal(f.spawns.length, 0);
  // Changing every time: at most three boxes, then nothing runs.
  const g = runner();
  let n = 0;
  g.setAnswer(async () => { g.setPlan({ fingerprint: String(++n).padStart(64, "0") }); return true; });
  assert.equal((await g.ask("start", "c1", { step: "build" })).outcome, "changed");
  assert.equal(g.confirms.length, 3);
  assert.equal(g.spawns.length, 0);
  // What the box says: the command, the scripts npm runs with it, the packages.
  const h = runner({}, { buildHooks: [{ name: "prebuild", script: "node gen.js" }] });
  await h.ask("start", "c1", { step: "build" });
  assert.deepEqual((h.confirms[0] as { hooks: unknown }).hooks, [{ name: "prebuild", script: "node gen.js" }]);
});

test("PR #22 review B3: a start waiting for the box is called off by cancel, a page reload or quit preparation", async () => {
  for (const callOff of ["cancel", "stopAll", "suspend", "shutdown"] as const) {
    const r = runner();
    let release!: (value: boolean) => void;
    r.setAnswer(() => new Promise<boolean>(resolve => { release = resolve; }));
    const asked = r.ask("start", "c1", { step: "build" });
    await wait(5);
    if (callOff === "cancel") await r.ask("cancel", "c1");
    else r.manager[callOff]();
    release(true);
    const result = await asked.catch(error => ({ outcome: String(error) }));
    assert.ok(result.outcome === "cancelled" || /quits/.test(String(result.outcome)), `${callOff}: ${result.outcome}`);
    assert.equal(r.spawns.length, 0, `${callOff}: nothing ran`);
  }
  const r = runner();
  r.manager.suspend();
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "refused");
  r.manager.resume();
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "started");
});

test("PR #22 review B3: stopping ends the group, the time limit and quitting too, and every descendant seen, also one that left the group", async () => {
  const r = runner();
  await r.ask("start", "c1", { step: "build" });
  // A child in the group and a grandchild that made its own session.
  r.table.set(500, { ppid: 100, pgid: 100, started: "t500" });
  r.table.set(501, { ppid: 500, pgid: 501, started: "t501" });
  await wait(20);
  assert.deepEqual(r.manager.seenProcesses("p", "c1").sort(), [100, 500, 501]);
  await r.ask("cancel", "c1");
  assert.deepEqual(r.kills[0], [100, "SIGTERM"]);
  assert.ok(r.killed.some(([pid, signal]) => pid === 501 && signal === "SIGTERM"), "the one outside the group is signalled by id");
  r.children[0]!.emit("close", null);
  await settle();
  const stopped = await r.ask("status", "c1", { cursor: 0 });
  assert.equal(stopped.job.state, "cancelled");
  assert.equal(stopped.job.leftovers, 0);
  assert.ok(!r.table.has(500) && !r.table.has(501));

  // A build over its time that never answers is killed after the grace, and is not "running" for ever.
  const slow = runner({ timeoutMs: { install: 60, build: 40 } });
  await slow.ask("start", "c2", { step: "build" });
  await wait(60);
  assert.deepEqual(slow.kills[0], [100, "SIGTERM"]);
  await wait(1_200);
  assert.equal((await slow.ask("status", "c2", { cursor: 0 })).job.state, "timedOut");

  r.kills.length = 0;
  await r.ask("start", "c3", { step: "build" });
  r.manager.shutdown();
  assert.deepEqual(r.kills, [[101, "SIGKILL"]]);
  await assert.rejects(r.ask("status", "c3", { cursor: 0 }), /quits/);
});

test("PR #22 review B3: what does not end is told, blocks the task, and the next app start ends it if it is still the same process", async () => {
  const r = runner();
  await r.ask("start", "c1", { step: "build" });
  // Survives every signal (the fake's "immortal" start time).
  r.table.set(600, { ppid: 100, pgid: 600, started: "immortal" });
  await wait(20);
  r.children[0]!.emit("close", 0);
  await settle();
  const ended = await r.ask("status", "c1", { cursor: 0 });
  assert.equal(ended.job.leftovers, 1);
  assert.ok(ended.job.lines.some(line => line.includes("끝나지 않은 프로그램 1개")));
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "leftovers");
  assert.ok(JSON.stringify(r.running.at(-1)).includes("immortal"), "kept on disk for the next start");
  // Once it is gone, the task can build again.
  r.table.delete(600);
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "started");

  // The next start: recorded processes still the same are ended; a reused id is not touched.
  const next = runner({ loadRunning: () => [{ leader: 100, processes: [{ pid: 700, started: "t700" }, { pid: 701, started: "old" }] }] });
  next.table.set(700, { ppid: 1, pgid: 700, started: "t700" });
  next.table.set(701, { ppid: 1, pgid: 701, started: "someone-else" });
  assert.equal(next.manager.recoverLeftovers(), 1);
  assert.deepEqual(next.killed, [[700, "SIGKILL"]]);
  assert.ok(next.table.has(701));
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

// ---------------------------------------------------------------------------
// PR #22 review B4: secrets spread over lines, through a real process.

import { mkdtempSync, readFileSync, readdirSync, writeFileSync, existsSync, rmSync, realpathSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { join } from "node:path";
import { spawn as realSpawn, spawnSync } from "node:child_process";
import { PreviewLogMasker, previewBuildCaches, previewBuildHiddenPaths, previewBuildSandboxProfile } from "../electron/preview-build-policy.ts";
import { hiddenWithTargets } from "../electron/preview-build-host.ts";

/** Synthetic secrets only (as in tests/fixtures/m5/secret-mask-cases.json). */
const FAKE = {
  pem: ["-----BEGIN RSA PRIVATE KEY-----", "FAKEEXAMPLEKEYBODYLINEONE0123456789abcdefABCDEF", "FAKEEXAMPLEKEYBODYLINETWO", "-----END RSA PRIVATE KEY-----"],
  jsonKey: ['{', '  "type": "service_account",', '  "private_key": "-----BEGIN PRIVATE KEY-----\\nFAKEJSONKEYBODY\\n-----END PRIVATE KEY-----\\n",', '  "client_email": "fake@example.invalid"', '}'],
  token: "GITHUB_TOKEN=ghp_FAKEexampleTOKEN0123456789abcd",
  blob: "FAKEunnamedKEYmaterial0123456789ABCDEFghijklmnopQRSTUV",
};
const FRAGMENTS = ["FAKEEXAMPLEKEYBODYLINEONE", "FAKEEXAMPLEKEYBODYLINETWO", "FAKEJSONKEYBODY", "FAKEexampleTOKEN", "FAKEunnamedKEYmaterial"];

test("PR #22 review B4: a PEM block and nameless key material are hidden whole, line by line, as the shared PR #11 case", () => {
  const shared = JSON.parse(readFileSync(new URL("./fixtures/m5/secret-mask-cases.json", import.meta.url), "utf8")) as { mask: { text: string }[] };
  const pemCase = shared.mask.find(entry => entry.text.includes("BEGIN RSA PRIVATE KEY"))!;
  const masker = new PreviewLogMasker();
  const out = pemCase.text.split("\n").map(line => masker.line(line));
  assert.ok(!out.join("\n").includes("FAKEEXAMPLEKEYBODY"), out.join("\n"));
  assert.equal(out.at(-1), "after", "the block ends at its END line");
  const lines = new PreviewLogMasker();
  const all = [...FAKE.pem, ...FAKE.jsonKey, FAKE.token, FAKE.blob, "✓ built in 215ms", "dist/assets/index-DAjp-lVP.js 143.61 kB", "/Users/me/elsecrew/workspaces/todo-vite/task-a33340ee/src/App.jsx:3:1"].map(line => lines.line(line));
  for (const fragment of FRAGMENTS) assert.ok(!all.join("\n").includes(fragment), fragment);
  // Ordinary build output and paths stay readable.
  assert.ok(all.includes("✓ built in 215ms") && all.includes("dist/assets/index-DAjp-lVP.js 143.61 kB"));
  assert.ok(all.some(line => line.endsWith("src/App.jsx:3:1")));
  // A secret straddling chunks and a line too long to keep.
  const split = new PreviewLineSplitter();
  const masked = new PreviewLogMasker();
  const got = [...split.push("token=ghp_FAKEexample"), ...split.push("TOKEN0123456789abcd\n"), ...split.push("y".repeat(70_000)), ...split.push("tail-of-long\nnext\n")].map(line => masked.line(line));
  assert.ok(!got.join("\n").includes("FAKEexampleTOKEN") && !got.join("\n").includes("tail-of-long"));
  assert.deepEqual(got.slice(1), ["(가림)", "next"]);
});

test("PR #22 review B4: what a real build prints reaches neither the page nor the disk with a secret in it", async () => {
  const dir = mkdtempSync(join(tmpdir(), "elsecrew-build-log-"));
  try {
    const script = join(dir, "print.js");
    const text = [...FAKE.pem, ...FAKE.jsonKey, FAKE.token, FAKE.blob, "plain output"].join("\n");
    // Printed in small pieces on both streams, as a real tool might.
    writeFileSync(script, `const t = ${JSON.stringify(text)}; for (let i = 0; i < t.length; i += 7) { (i % 2 ? process.stderr : process.stdout).write(t.slice(i, i + 7)); } process.stdout.write("\\n");`);
    const logs = join(dir, "logs");
    const plan = { chatId: "c1", projectId: "p", worktreeId: "w", workspaceIdentity: "i1", refusal: null, workspacePath: dir, packageManager: "npm" as const, lockfile: true,
      buildScript: "x", buildHooks: [], dependencies: [], dependencyCount: 0, yarnBerry: false, outputDir: "dist", needsInstall: false, fingerprint: "a".repeat(64),
      folderIdentity: "1:2", inputs: [{ name: "package.json", sha256: "b".repeat(64) }] };
    const manager = new PreviewBuildManagerV1({
      enabled: true, confirm: async () => true,
      sidecar: () => ({ previewBuildPlan: async () => plan, previewBuildReserve: async ({ holdId }) => ({ holdId, refusal: null }), previewBuildRelease: async ({ holdId }) => ({ holdId, released: true }), previewBuildHolds: async ({ holds }) => ({ held: holds.length, blocking: false }) }),
      findTools: () => ({ program: process.execPath, pathDirs: [], identity: "node", userConfigs: [] }), isWorkspaceFolder: () => true,
      places: () => ({ caches: [], hidden: [] }), makeTemp: () => dir, removeTemp: () => undefined,
      // The real program, not npm: node printing the secrets.
      confine: () => ({ program: process.execPath, args: [script] }),
      spawn: (program, args, options) => realSpawn(program, args, { ...options, detached: false }),
      killGroup: () => undefined, killProcess: () => undefined, processes: () => [], isAlive: () => false,
      saveRunning: () => undefined, loadRunning: () => [], outputWritten: () => false,
      environment: {}, writeLog: (name, body) => { spawnSync(process.execPath, ["-e", "require('fs').mkdirSync(process.argv[1],{recursive:true})", logs]); writeFileSync(join(logs, name), body); },
      log: () => undefined, pipeGraceMs: 200,
    });
    const started = await manager.request({ action: "start", projectId: "p", chatId: "c1", step: "build" }) as { outcome: string };
    assert.equal(started.outcome, "started");
    let job: { state: string; lines: string[] } = { state: "running", lines: [] };
    for (let i = 0; i < 100 && job.state === "running"; i++) {
      await wait(50);
      job = (await manager.request({ action: "status", projectId: "p", chatId: "c1", cursor: 0 }) as { job: typeof job }).job;
    }
    assert.notEqual(job.state, "running");
    const shown = job.lines.join("\n");
    const disk = readdirSync(logs).map(name => readFileSync(join(logs, name))).reduce((all, bytes) => Buffer.concat([all, bytes]), Buffer.alloc(0));
    assert.ok(disk.length > 0, "a log was written");
    for (const fragment of FRAGMENTS) {
      assert.ok(!shown.includes(fragment), `page: ${fragment}`);
      assert.equal(disk.indexOf(Buffer.from(fragment)), -1, `disk: ${fragment}`);
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

// ---------------------------------------------------------------------------
// PR #22 review B2: the sandbox.

test("PR #22 re-review B2: the profile writes only this workspace, this job's temporary folder and the exact caches; hides keys and the app", () => {
  const places = { workspace: "/Users/me/elsecrew/workspaces/app/t1", temp: "/private/var/folders/ab/cd/T/elsecrew-build-x1",
    caches: previewBuildCaches("npm", false, "/Users/me"), hidden: previewBuildHiddenPaths("/Users/me", ["/Users/me/Library/Application Support/Elsecrew"]) };
  const build = previewBuildSandboxProfile("build", places);
  assert.match(build, /\(deny file-write\*\)/);
  for (const allowed of [places.workspace, places.temp, "/Users/me/.npm/_cacache", "/Users/me/.npm/_logs"]) assert.ok(build.includes(`(subpath "${allowed}")`), allowed);
  // No longer: the whole cache folders, the whole temporary root, /private/tmp.
  for (const wide of ['(subpath "/Users/me/Library/Caches")', '(subpath "/Users/me/.cache")', '(subpath "/private/tmp")', '(subpath "/private/var/folders/ab/cd")', '(subpath "/Users/me/.npm")']) assert.ok(!build.includes(wide), wide);
  assert.ok(build.includes('(allow file-write-create (literal "/Users/me/.npm"))'), "only the missing parent may be made");
  for (const hidden of ["/Users/me/.ssh", "/Users/me/.aws", "/Users/me/.config/gh", "/Users/me/Library/Keychains", "/Users/me/.netrc", "/Users/me/.claude", "/Users/me/.codex", "/Users/me/Library/Application Support/Elsecrew"]) {
    assert.ok(build.includes(`(subpath "${hidden}")`) && places.hidden.includes(hidden), hidden);
  }
  assert.match(build, /\(deny network\*\)/);
  assert.doesNotMatch(previewBuildSandboxProfile("install", places), /deny network/);
  assert.deepEqual(previewBuildCaches("pnpm", false, "/h"), ["/h/Library/pnpm/store", "/h/Library/Caches/pnpm"]);
  assert.deepEqual(previewBuildCaches("yarn", true, "/h"), ["/h/.yarn/berry/cache"]);
  for (const bad of ['/a"b', "/a\\b", "relative", "/a\nb"]) assert.throws(() => previewBuildSandboxProfile("build", { ...places, workspace: bad }));
});

test("PR #22 re-review B2: under the real sandbox every key and app place is refused (also through a link's real target), and only the exact places are writable (macOS)", { skip: process.platform !== "darwin" || !existsSync("/usr/bin/sandbox-exec") }, () => {
  const root = realpathSync(mkdtempSync(join(homedir(), ".elsecrew-sbx-test-")));
  try {
    // A home of its own (the profile only knows the paths it is given).
    const home = join(root, "home");
    const workspace = join(root, "ws");
    const temps = join(root, "tmp");
    const temp = join(temps, "elsecrew-build-a");
    const otherTemp = join(temps, "other");
    const app = join(root, "app-data");
    const keyTarget = join(root, "elsewhere-keys");
    for (const dir of [workspace, temp, otherTemp, app, keyTarget, join(home, ".aws"), join(home, ".config/gh"), join(home, "Library/Keychains"), join(home, ".npm"), join(home, "Library/Caches/other"), join(home, ".claude")]) spawnSync("/bin/mkdir", ["-p", dir]);
    writeFileSync(join(keyTarget, "id_ed25519"), "FAKEKEY");
    // ~/.ssh is a link to a folder elsewhere: refused by its name and by its real target.
    spawnSync("/bin/ln", ["-s", keyTarget, join(home, ".ssh")]);
    for (const [path, text] of [[join(home, ".aws/credentials"), "x"], [join(home, ".config/gh/hosts.yml"), "x"], [join(home, "Library/Keychains/login.keychain-db"), "x"], [join(home, ".netrc"), "x"], [join(home, ".claude/.credentials.json"), "x"], [join(app, "secret.txt"), "x"]]) writeFileSync(path, text);
    const hidden = hiddenWithTargets(previewBuildHiddenPaths(home, [app]));
    assert.ok(hidden.includes(keyTarget), "the link's real target is hidden too");
    const profile = previewBuildSandboxProfile("build", { workspace, temp, caches: previewBuildCaches("npm", false, home), hidden });
    const checks: Record<string, [string, "read" | "write"]> = {
      sshByName: [join(home, ".ssh/id_ed25519"), "read"], sshByTarget: [join(keyTarget, "id_ed25519"), "read"],
      aws: [join(home, ".aws/credentials"), "read"], gh: [join(home, ".config/gh/hosts.yml"), "read"], keychain: [join(home, "Library/Keychains/login.keychain-db"), "read"],
      netrc: [join(home, ".netrc"), "read"], claude: [join(home, ".claude/.credentials.json"), "read"], app: [join(app, "secret.txt"), "read"],
      writeSsh: [join(keyTarget, "new"), "write"], writeHome: [join(home, "x.txt"), "write"], writeOtherCache: [join(home, "Library/Caches/other/x"), "write"],
      writeOtherTemp: [join(otherTemp, "x"), "write"], writeNpmRoot: [join(home, ".npm/x"), "write"],
      workspace: [join(workspace, "x"), "write"], temp: [join(temp, "x"), "write"], npmCache: [join(home, ".npm/_cacache/x"), "write"],
    };
    const script = `const fs = require("fs"); const out = {};
      for (const [name, [path, how]] of Object.entries(${JSON.stringify(checks)})) {
        try { if (how === "read") fs.readFileSync(path); else { fs.mkdirSync(require("path").dirname(path), { recursive: true }); fs.writeFileSync(path, "x"); } out[name] = "ok"; } catch (e) { out[name] = e.code || String(e); }
      }
      console.log(JSON.stringify(out));`;
    const run = spawnSync("/usr/bin/sandbox-exec", ["-p", profile, process.execPath, "-e", script], { cwd: workspace, encoding: "utf8" });
    const out = JSON.parse(run.stdout.trim().split("\n").at(-1)!) as Record<string, string>;
    for (const name of ["sshByName", "sshByTarget", "aws", "gh", "keychain", "netrc", "claude", "app", "writeSsh", "writeHome", "writeOtherCache", "writeOtherTemp", "writeNpmRoot"]) {
      assert.equal(out[name], "EPERM", `${name}: ${out[name]}`);
    }
    for (const name of ["workspace", "temp", "npmCache"]) assert.equal(out[name], "ok", `${name}: ${out[name]}`);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

// ---------------------------------------------------------------------------
// PR #22 re-review.

test("PR #22 re-review B1: a second reading that shows anything else — even with the same fingerprint — is asked again", async () => {
  // Only the shown script differs; the fingerprint is the same (the race the review found).
  const r = runner();
  const answers: (() => boolean)[] = [() => { r.setPlan({ buildScript: "vite build && curl evil" }); return true; }, () => false];
  r.setAnswer(async () => answers.shift()!());
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "changed");
  assert.equal(r.spawns.length, 0);
  assert.equal((r.confirms[1] as { buildScript: string }).buildScript, "vite build && curl evil", "the second box shows what would run now");
  // A settings file of the person's own that appears, or an input's hash: asked again too.
  for (const change of [{ tools: { userConfigs: ["~/.npmrc"], identity: "npm-1#npmrc" } }, { plan: { inputs: [{ name: "package.json", sha256: "c".repeat(64) }] } }]) {
    const t = runner();
    const more: (() => boolean)[] = [() => { if (change.plan) t.setPlan(change.plan); if (change.tools) t.setTools(change.tools); return true; }, () => false];
    t.setAnswer(async () => more.shift()!());
    assert.equal((await t.ask("start", "c1", { step: "build" })).outcome, "changed", JSON.stringify(change));
    assert.equal(t.spawns.length, 0);
  }
  // What the box was given: every input and hash, the folder's identity, the sandbox's lists.
  const shown = runner().confirms;
  const u = runner();
  await u.ask("start", "c1", { step: "build" });
  assert.deepEqual(Object.keys(u.confirms[0] as object).sort(), ["buildScript", "caches", "changed", "command", "dependencies", "dependencyCount", "folder", "folderIdentity", "hidden", "hooks", "inputs", "program", "step", "userConfigs"]);
  assert.deepEqual((u.confirms[0] as { inputs: unknown }).inputs, [{ name: "package.json", sha256: "b".repeat(64) }]);
  assert.equal(shown.length, 0);
});

test("PR #22 re-review: the engine's write hold is taken before anything runs, kept while anything of the job lives, released after", async () => {
  // A team works on the task (or one whose writes meet it): nothing runs.
  const busy = runner();
  busy.setReserve("busy");
  assert.equal((await busy.ask("start", "c1", { step: "build" })).outcome, "busy");
  assert.equal(busy.spawns.length, 0);
  // Changed after the hold: nothing runs and the hold goes back.
  const changed = runner();
  const answers: (() => boolean)[] = [() => { changed.setPlan({ fingerprint: "9".repeat(64) }); return true; }, () => false];
  changed.setAnswer(async () => answers.shift()!());
  assert.equal((await changed.ask("start", "c1", { step: "build" })).outcome, "changed");
  await wait(5);
  assert.deepEqual(changed.released, changed.holds);
  // Held while it runs; its own temporary folder; both go when it ends.
  const r = runner();
  await r.ask("start", "c1", { step: "build" });
  assert.equal(r.holds.length, 1);
  assert.deepEqual(r.released, []);
  assert.equal(r.temps.length, 1);
  r.children[0]!.emit("close", 0);
  await settle();
  assert.deepEqual(r.released, r.holds);
  assert.deepEqual(r.temps, []);
  // A job that leaves a process alive keeps the workspace held.
  const k = runner();
  await k.ask("start", "c1", { step: "build" });
  k.table.set(800, { ppid: 100, pgid: 800, started: "immortal" });
  await wait(20);
  k.children[0]!.emit("close", 0);
  await settle();
  assert.deepEqual(k.released, [], "still held: a process of the build may still write");
  k.table.delete(800);
  assert.equal((await k.ask("start", "c1", { step: "build" })).outcome, "started");
  await wait(5);
  assert.deepEqual(k.released, [k.holds[0]], "the first hold went once its process was gone");
});

test("PR #22 re-review B3: what the next start cannot end stays on record and blocks building; the group is ended unless its number is someone else's", async () => {
  // Signalling fails for a live process: the record stays, starts wait.
  const failing = runner({ killProcess: () => { throw new Error("EPERM"); } });
  failing.setRecords([{ leader: 900, leaderStarted: "t900", processes: [{ pid: 901, started: "t901" }] }]);
  failing.table.set(901, { ppid: 1, pgid: 900, started: "t901" });
  failing.manager.recoverLeftovers();
  assert.ok(JSON.stringify(failing.running.at(-1)).includes("901"), "kept on record");
  assert.equal((await failing.ask("start", "c1", { step: "build" })).outcome, "leftovers");
  failing.table.delete(901);
  assert.equal((await failing.ask("start", "c1", { step: "build" })).outcome, "started", "gone: building again");

  // The leader is gone: every member of its group is ended, also one never seen.
  const group = runner();
  group.setRecords([{ leader: 910, leaderStarted: "t910", processes: [{ pid: 911, started: "t911" }] }]);
  group.table.set(911, { ppid: 1, pgid: 910, started: "t911" });
  group.table.set(912, { ppid: 911, pgid: 910, started: "t912-unseen" });
  assert.equal(group.manager.recoverLeftovers(), 2);
  assert.ok(!group.table.has(911) && !group.table.has(912));

  // The group's number belongs to another process now: its group is not touched.
  const reused = runner();
  reused.setRecords([{ leader: 920, leaderStarted: "t920", processes: [] }]);
  reused.table.set(920, { ppid: 1, pgid: 920, started: "someone-else" });
  reused.table.set(921, { ppid: 920, pgid: 920, started: "t921" });
  assert.equal(reused.manager.recoverLeftovers(), 0);
  assert.ok(reused.table.has(920) && reused.table.has(921));

  // A record that cannot be read: nothing starts (it may name live processes).
  const unreadable = runner();
  unreadable.setRecords(new Error("bad json"));
  unreadable.manager.recoverLeftovers();
  assert.equal((await unreadable.ask("start", "c1", { step: "build" })).outcome, "leftovers");

  // While running: after the leader ended, its number used by another group is not signalled.
  const live = runner();
  await live.ask("start", "c1", { step: "build" });
  await wait(20);
  live.table.set(100, { ppid: 1, pgid: 100, started: "another-process" });
  live.kills.length = 0;
  await live.ask("cancel", "c1");
  assert.deepEqual(live.kills, [], "the group number is someone else's now");
});

test("PR #22 re-review B4: a block begun on a line too long to keep hides every line after it until its end, however the lines come", () => {
  const body = ["FAKEBODYALPHA0001", "FAKEBODYBRAVO0002", "FAKEBODYCHARLIE003"];
  const longBegin = `${"x".repeat(70_000)} -----BEGIN OPENSSH PRIVATE KEY-----`;
  for (const pieces of [
    [`${longBegin}\n${body.join("\n")}\n-----END OPENSSH PRIVATE KEY-----\nafter\n`],
    // The begin marker far into a line that already overflowed, split mid-marker.
    [`${"y".repeat(70_000)}`, "zz -----BEG", "IN RSA PRIVATE KEY-----\n", `${body[0]}\n${body[1]}\n`, `${body[2]}\n-----END RSA PRIVATE KEY-----\nafter\n`],
    // No end: the stream ends inside the block.
    [`${longBegin}\n${body.join("\n")}\n`],
  ]) {
    const split = new PreviewLineSplitter();
    const mask = new PreviewLogMasker();
    const out = [...pieces.flatMap(piece => split.push(piece)), ...split.flush()].map(line => mask.line(line));
    const text = out.join("\n");
    assert.ok(!text.includes("FAKEBODY") && !text.includes("x".repeat(40)), text.slice(0, 200));
    if (pieces.join("").includes("after")) assert.equal(out.at(-1), "after", "the block ends at its END line");
  }
  // The last marker on a line decides: a line that ends a block and begins another keeps hiding.
  const mask = new PreviewLogMasker();
  mask.line("-----BEGIN A KEY-----");
  mask.line("-----END A KEY----- -----BEGIN B KEY-----");
  assert.equal(mask.inBlock, true);
  assert.equal(mask.line("FAKEBODYDELTA"), "(가림)");
});

test("PR #22 re-review B4: a real process's long begin line and the body after it reach neither the page nor the disk", async () => {
  const dir = mkdtempSync(join(tmpdir(), "elsecrew-build-log-"));
  try {
    const body = Array.from({ length: 5 }, (_, index) => `FAKEBODYLINE${index}QWERTYUIOPASDFG`);
    const script = join(dir, "print.js");
    // One stream, in pieces: a begin line too long to keep, then a short body.
    writeFileSync(script, `const t = ${JSON.stringify(`${"q".repeat(80_000)} -----BEGIN PRIVATE KEY-----\n${body.join("\n")}\n-----END PRIVATE KEY-----\nplain output\n`)}; for (let i = 0; i < t.length; i += 4096) process.stdout.write(t.slice(i, i + 4096));`);
    const logs = join(dir, "logs");
    const plan = { chatId: "c1", projectId: "p", worktreeId: "w", workspaceIdentity: "i1", refusal: null, workspacePath: dir, packageManager: "npm" as const, lockfile: true,
      buildScript: "x", buildHooks: [], dependencies: [], dependencyCount: 0, yarnBerry: false, outputDir: "dist", needsInstall: false, fingerprint: "a".repeat(64),
      folderIdentity: "1:2", inputs: [{ name: "package.json", sha256: "b".repeat(64) }] };
    const manager = new PreviewBuildManagerV1({
      enabled: true, confirm: async () => true,
      sidecar: () => ({ previewBuildPlan: async () => plan, previewBuildReserve: async ({ holdId }) => ({ holdId, refusal: null }), previewBuildRelease: async ({ holdId }) => ({ holdId, released: true }), previewBuildHolds: async ({ holds }) => ({ held: holds.length, blocking: false }) }),
      findTools: () => ({ program: process.execPath, pathDirs: [], identity: "node", userConfigs: [] }), isWorkspaceFolder: () => true,
      places: () => ({ caches: [], hidden: [] }), makeTemp: () => dir, removeTemp: () => undefined,
      confine: () => ({ program: process.execPath, args: [script] }),
      spawn: (program, args, options) => realSpawn(program, args, { ...options, detached: false }),
      killGroup: () => undefined, killProcess: () => undefined, processes: () => [], isAlive: () => false,
      saveRunning: () => undefined, loadRunning: () => null, outputWritten: () => false,
      environment: {}, writeLog: (name, text) => { spawnSync(process.execPath, ["-e", "require('fs').mkdirSync(process.argv[1],{recursive:true})", logs]); writeFileSync(join(logs, name), text); },
      log: () => undefined, pipeGraceMs: 200,
    });
    assert.equal((await manager.request({ action: "start", projectId: "p", chatId: "c1", step: "build" }) as { outcome: string }).outcome, "started");
    let job: { state: string; lines: string[] } = { state: "running", lines: [] };
    for (let i = 0; i < 100 && job.state === "running"; i++) {
      await wait(50);
      job = (await manager.request({ action: "status", projectId: "p", chatId: "c1", cursor: 0 }) as { job: typeof job }).job;
    }
    const disk = readFileSync(join(logs, readdirSync(logs)[0]!));
    for (const line of body) for (const piece of [line, line.slice(0, 12), line.slice(-12)]) {
      assert.ok(!job.lines.join("\n").includes(piece), `page: ${piece}`);
      assert.equal(disk.indexOf(Buffer.from(piece)), -1, `disk: ${piece}`);
    }
    assert.ok(job.lines.includes("plain output"), "the stream goes on after the block");
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("ADE stabilization A: a release the engine never got (stopped or restarting) is sent again once an engine is ready", async () => {
  for (const outage of ["down", "failing"] as const) {
    const r = runner();
    assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "started");
    assert.equal(r.holds.length, 1);
    // The engine stops while the build runs; the build ends meanwhile.
    r.setEngine(outage);
    r.children[0]!.emit("close", 0);
    await settle();
    assert.deepEqual(r.released, [], outage);
    assert.deepEqual(r.manager.unreleasedHolds, r.holds, `${outage}: kept to send again`);
    // A new engine is ready (it restored the hold): the release goes out.
    r.setEngine("up");
    r.manager.engineReady();
    await wait(5);
    assert.deepEqual(r.released, r.holds, outage);
    assert.deepEqual(r.manager.unreleasedHolds, [], outage);
  }
  // Still restarting when asked again: kept, and tried again by itself.
  const r = runner();
  await r.ask("start", "c1", { step: "build" });
  r.setEngine("failing");
  r.children[0]!.emit("close", 0);
  await settle();
  r.manager.engineReady();
  await wait(5);
  assert.deepEqual(r.manager.unreleasedHolds, r.holds);
  r.setEngine("up");
  await wait(3_200);
  assert.deepEqual(r.released, r.holds, "the retry sent it");
  assert.deepEqual(r.manager.unreleasedHolds, []);
});

test("PR #25 review B1: a reserve whose answer was lost is released by the name main gave it", async () => {
  const r = runner();
  r.setReserve("lost");
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "busy");
  assert.equal(r.spawns.length, 0, "nothing ran");
  assert.equal(r.holds.length, 1);
  assert.match(r.holds[0]!, /^pbh_[0-9a-f]{32}$/, "main named it");
  await wait(5);
  assert.deepEqual(r.released, r.holds, "released by that name at once");
  assert.deepEqual(r.manager.heldHolds, []);
  // The engine was down for the release too: it goes out once one is ready.
  const down = runner();
  down.setReserve("lost");
  down.setEngine("failing");
  await down.ask("start", "c1", { step: "build" });
  await wait(5);
  assert.deepEqual(down.manager.unreleasedHolds, down.holds);
  down.setEngine("up");
  down.manager.engineReady();
  await wait(5);
  assert.deepEqual(down.syncs.at(-1)!.holds, [], "the new engine learns main holds nothing");
  assert.deepEqual(down.released, down.holds);
});

test("PR #25 review B2: each new engine learns main's holds, and leftovers count as settled only once checked and gone", async () => {
  const r = runner();
  assert.equal((await r.ask("start", "c1", { step: "build" })).outcome, "started");
  const hold = r.holds[0]!;
  // Before the app has looked at an earlier run's leftovers: not settled.
  r.manager.engineReady();
  await wait(5);
  assert.deepEqual(r.syncs.at(-1), { holds: [{ holdId: hold, chatId: "c1" }], leftoversSettled: false });
  // Looked, an earlier run's process still alive: still not settled, and
  // the engine keeps blocking; main looks again later.
  r.table.set(900, { ppid: 1, pgid: 900, started: "immortal" });
  r.setRecords([{ leader: 900, leaderStarted: "immortal", processes: [{ pid: 900, started: "immortal" }] }]);
  r.setEngineBlocks(true);
  r.manager.recoverLeftovers();
  await wait(5);
  assert.equal(r.syncs.at(-1)!.leftoversSettled, false);
  assert.equal(r.manager.engineBlocking, true);
  // Gone: settled, and the engine lets go.
  r.table.delete(900);
  r.manager.engineReady();
  await wait(5);
  assert.deepEqual(r.syncs.at(-1), { holds: [{ holdId: hold, chatId: "c1" }], leftoversSettled: true });
  assert.equal(r.manager.engineBlocking, false);
  // The job ends: the hold leaves main's list.
  r.children[0]!.emit("close", 0);
  await settle();
  r.manager.engineReady();
  await wait(5);
  assert.deepEqual(r.syncs.at(-1)!.holds, []);
});

