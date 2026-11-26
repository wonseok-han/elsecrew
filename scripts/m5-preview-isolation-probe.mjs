#!/usr/bin/env node
// Result preview isolation probe (plan 2.4 "확인" column, 2.6 Electron 통합
// 프로브). macOS development build, synthetic project, isolated app data.
//
//   node scripts/m5-preview-isolation-probe.mjs [--build] [--out <dir>] [--workspace git|snapshot|both]
//
// --build      builds main/preload/renderer into .vite/ first (no packaging)
// --workspace  task workspace kind(s) to run with (default both: real Git
//              worktrees in a task folder inside the probe's temporary
//              folder, then snapshot copies inside the isolated app data)
// Needs target/debug/elsecrew-sidecar (`cargo build -p elsecrew-sidecar`).
//
// Judges with three sources, each with a positive control so an empty
// observation means "blocked", not "not looking" (PR #6 review I5):
// - local collectors (TCP and UDP, IPv4 and IPv6) that must see nothing from
//   the page; after the run the probe itself reaches each one once and it
//   must count exactly that one;
// - Chromium's net log (complete, never repaired), which must show no network
//   host lookup and no connection but to the dead proxy for the page, while it
//   does show the probe's own control lookup and the page's attempts at the
//   dead proxy;
// - what the page reports: every attempt must have a result, and each
//   refusal must come from the boundary (permissions are tried with a user
//   gesture on a focused view).
// No outside network is used by the page; the control lookup is one DNS
// query for an example.com name by the app's own session.
import { spawn, execFileSync } from "node:child_process";
import { createServer, connect } from "node:net";
import { createSocket } from "node:dgram";
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync, copyFileSync, readdirSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { builtinModules } from "node:module";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : fallback; };
const outDirectory = resolve(option("--out", join(repoRoot, ".artifacts", "m5-preview-isolation")));
const modes = { both: ["git", "snapshot"], git: ["git"], snapshot: ["snapshot"] }[option("--workspace", "both")];
if (!modes) throw new Error("--workspace must be git, snapshot or both");
if (process.platform !== "darwin") throw new Error("the preview isolation probe runs on macOS");

if (args.includes("--build")) await buildApp();
for (const required of [".vite/build/main.cjs", ".vite/build/preload.cjs", ".vite/renderer/main_window/index.html", "target/debug/elsecrew-sidecar"]) {
  if (!existsSync(join(repoRoot, required))) throw new Error(`missing ${required}; run with --build and cargo build -p elsecrew-sidecar`);
}
mkdirSync(outDirectory, { recursive: true });
const head = execFileSync("/usr/bin/git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim();
const dirty = execFileSync("/usr/bin/git", ["status", "--porcelain", "--untracked-files=no"], { cwd: repoRoot, encoding: "utf8" }).trim() !== "";
const results = [];
for (const mode of modes) results.push(await runMode(mode));
const status = results.every(result => result.status === "PASS") ? "PASS" : "FAIL";
writeFileSync(join(outDirectory, "evidence.json"), JSON.stringify({
  schemaVersion: 2, probe: "m5-preview-isolation", status, head, dirtySources: dirty, measuredAt: new Date().toISOString(),
  modes: results.map(result => ({ mode: result.mode, status: result.status, failed: result.failed, evidence: result.evidence })),
}, null, 2));
console.log(JSON.stringify({ status, head, dirtySources: dirty, modes: results.map(({ mode, status: modeStatus, failed }) => ({ mode, status: modeStatus, failed })) }, null, 2));
process.exit(status === "PASS" ? 0 : 1);

async function runMode(mode) {
  const root = mkdtempSync(join(tmpdir(), `elsecrew-preview-probe-${mode}-`));
  const marker = `elsecrewprobe${randomBytes(6).toString("hex")}`;
  const controlHost = `elsecrewctl${randomBytes(6).toString("hex")}.example.com`;
  const source = join(root, "project");
  mkdirSync(source);
  writeFileSync(join(source, "README.md"), "preview probe\n");
  const git = (...gitArgs) => execFileSync("/usr/bin/git", gitArgs, { cwd: source, env: { ...process.env, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null" } });
  git("init", "-q", "--template=");
  git("add", ".");
  git("-c", "core.hooksPath=/dev/null", "-c", "user.name=Probe", "-c", "user.email=probe@example.invalid", "commit", "-qm", "fixture");

  // Collectors: any connection or datagram at all is a leak.
  const collectors = { tcp4: { count: 0, requests: [] }, tcp6: { count: 0, requests: [] }, udp4: { count: 0 }, udp6: { count: 0 } };
  const tcpServer = (record) => createServer(socket => {
    record.count++;
    socket.once("data", chunk => { record.requests.push(chunk.toString("latin1").split("\r\n")[0].slice(0, 120)); socket.destroy(); });
    socket.on("error", () => undefined);
  });
  const tcp4 = tcpServer(collectors.tcp4);
  const tcp6 = tcpServer(collectors.tcp6);
  await new Promise(done => tcp4.listen(0, "127.0.0.1", done));
  await new Promise(done => tcp6.listen(0, "::1", done));
  const udp4 = createSocket("udp4");
  const udp6 = createSocket("udp6");
  udp4.on("message", () => { collectors.udp4.count++; });
  udp6.on("message", () => { collectors.udp6.count++; });
  await new Promise(done => udp4.bind(0, "127.0.0.1", done));
  await new Promise(done => udp6.bind(0, "::1", done));
  const discardClosed = () => new Promise(done => {
    const socket = connect(9, "127.0.0.1");
    socket.once("connect", () => { socket.destroy(); done(false); });
    socket.once("error", error => done(error.code === "ECONNREFUSED"));
  });
  const discardClosedBefore = await discardClosed();

  const env = {
    ...process.env,
    ELSECREW_PREVIEW: "1",
    PROBE_ROOT: root,
    PROBE_WORKSPACE: mode,
    PROBE_HTTP_PORT: String(tcp4.address().port),
    PROBE_TCP6_PORT: String(tcp6.address().port),
    PROBE_UDP_PORT: String(udp4.address().port),
    PROBE_UDP6_PORT: String(udp6.address().port),
    PROBE_MARKER: marker,
    PROBE_CONTROL_HOST: controlHost,
  };
  if (mode === "snapshot") env.ELSECREW_GIT_WORKTREES = "0";
  else delete env.ELSECREW_GIT_WORKTREES;
  delete env.ELSECREW_PREVIEW_DEVTOOLS;
  delete env.ELECTRON_RUN_AS_NODE;
  const startedAt = Date.now();
  const electron = join(repoRoot, "node_modules", ".bin", "electron");
  const child = spawn(electron, [join(repoRoot, "scripts", "m5-preview-isolation-probe-app.cjs")], { cwd: repoRoot, env, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  child.stdout.on("data", chunk => { output += chunk; process.stdout.write(chunk); });
  child.stderr.on("data", chunk => { output += chunk; });
  const exitCode = await new Promise(done => {
    const timer = setTimeout(() => { child.kill("SIGTERM"); done("timeout"); }, 300_000);
    child.once("exit", code => { clearTimeout(timer); done(code); });
  });
  await new Promise(done => setTimeout(done, 500));

  // What the page sent, then the positive control: each collector counts
  // exactly the one contact the probe makes now.
  const fromPage = JSON.parse(JSON.stringify(collectors));
  await new Promise(done => { const socket = connect(tcp4.address().port, "127.0.0.1", () => { socket.end("GET /control4 HTTP/1.0\r\n\r\n"); done(); }); socket.on("error", done); });
  await new Promise(done => { const socket = connect(tcp6.address().port, "::1", () => { socket.end("GET /control6 HTTP/1.0\r\n\r\n"); done(); }); socket.on("error", done); });
  await new Promise(done => { const socket = createSocket("udp4"); socket.send("control", udp4.address().port, "127.0.0.1", () => { socket.close(); done(); }); });
  await new Promise(done => { const socket = createSocket("udp6"); socket.send("control", udp6.address().port, "::1", () => { socket.close(); done(); }); });
  await new Promise(done => setTimeout(done, 300));
  const controlCounted = ["tcp4", "tcp6", "udp4", "udp6"].every(name => collectors[name].count - fromPage[name].count === 1);
  const discardClosedAfter = await discardClosed();
  tcp4.close(); tcp6.close(); udp4.close(); udp6.close();

  const app = JSON.parse(readFileSync(join(root, "app-result.json"), "utf8"));
  const net = analyzeNetLog(join(root, "netlog.json"), marker, controlHost);
  const defaultTaskFolder = join(homedir(), "elsecrew", "workspaces");
  const leakedToHome = existsSync(defaultTaskFolder) && readdirSync(defaultTaskFolder).some(name => name === "project");

  const leak = app.leak ?? {};
  const has = (bag, name) => Boolean(bag?.[name]) && typeof bag[name].ok === "boolean";
  const refused = (bag, name) => has(bag, name) && bag[name].ok === false;
  const NETWORK = ["fetchLoopback", "fetchLoopback6", "fetchNoCors", "fetchExternal", "xhr", "image", "imageExternal", "websocket", "websocket6", "eventSource", "script", "stylesheet", "font", "dynamicImport"];
  const ATTEMPTED = ["beacon", "cssImport", "audio", "hints", "iframeExternal", "worker", "formPost", "webrtc"];
  const layers = [["CSP + webRequest + proxy", leak], ["webRequest + proxy (CSP bypassed)", app.layerWebRequest], ["proxy alone (CSP bypassed, no webRequest)", app.layerProxy]];
  const permissions = app.permissions ?? {};
  // A refusal that only says "no user gesture" or "not focused" proves nothing.
  const boundaryRefusal = (name) => refused(permissions, name) && !/gesture|activation|not focused|focus/i.test(permissions[name].error);
  const PERMISSIONS = ["camera", "microphone", "displayMedia", "geolocation", "clipboardRead", "hid", "usb", "serial", "midi", "fullscreen"];
  const placeOk = (app.chats?.kinds ?? []).length === 2 && app.chats.kinds.every(kind => mode === "git"
    ? kind.manifest === "gitWorktree" && kind.place === "insideTaskFolder"
    : kind.manifest === "standaloneSnapshot" && kind.place === "insideAppData");
  const checks = {
    // Positive controls first: without them an empty observation proves nothing.
    "control: each collector counted exactly the probe's own contact": controlCounted,
    "control: the net log is complete (never repaired) and has events": net.complete && net.events > 100,
    "control: the net log shows the probe's own control lookup": net.controlLookups > 0,
    "control: the net log shows the page's attempts reaching only the dead proxy": net.proxyAttempts > 0 && net.markerRequests > 0,
    "control: the dead proxy port was closed before and after": discardClosedBefore === true && discardClosedAfter === true,
    // T1·T2
    "T1/T2 nothing from the page reached a TCP or UDP collector (IPv4, IPv6)": ["tcp4", "tcp6", "udp4", "udp6"].every(name => fromPage[name].count === 0),
    ...Object.fromEntries(layers.map(([label, bag]) => [
      `T1 every network attempt has a result and failed — ${label}`,
      NETWORK.every(name => refused(bag, name)) && ATTEMPTED.every(name => has(bag, name)) && /^error/.test(bag?.worker?.value ?? ""),
    ])),
    ...Object.fromEntries(layers.map(([label, bag]) => [
      `T2 WebRTC (STUN v4/v6, TURN udp/tcp/tls) gathered no candidate — ${label}`,
      bag?.webrtc?.ok === true && Array.isArray(bag.webrtc.value) && bag.webrtc.value.length === 0,
    ])),
    "T1 webRequest alone cancelled the page's requests (main counter)": (app.layerWebRequestBlocked ?? 0) >= NETWORK.length,
    "T1 no outside answer reached the preview": app.diagnostics?.unexpectedResponses === 0,
    "T1 net log: no connection attempt to anything but the dead proxy": net.connects.length === 0,
    // T3
    "T3 net log: no network host lookup for the page's names (dns-prefetch, preconnect, STUN, fetch)": net.lookups.length === 0,
    // T4·T5
    "T4/T5 hidden, secret, linked and escaping paths are not served": Object.keys(leak.paths ?? {}).length === 10
      && Object.entries(leak.paths).every(([path, code]) => path === "data.txt" ? code === 200 : code === 404),
    // T6
    "T6 the page has no app bridge, ipcRenderer, require or process": Object.keys(leak.bridges ?? {}).length === 10 && Object.values(leak.bridges).every(type => type === "undefined"),
    // T7
    "T7 POST/PUT/DELETE/PATCH answer 405": ["methodPOST", "methodPUT", "methodDELETE", "methodPATCH"].every(name => leak[name]?.value === 405),
    "T7 no file picker exists in the page": Array.isArray(leak.filePickers) && leak.filePickers.length === 3 && leak.filePickers.every(type => type === "undefined"),
    "T7 the download was cancelled in main": (app.diagnostics?.blockedDownloads ?? 0) >= 1 && has(leak, "download"),
    "T7 the workspace is byte-for-byte unchanged": app.workspaceUnchanged === true,
    // T9
    "T9 window.open and _blank under a user gesture opened nothing": permissions.windowOpen?.value === "null" && has(permissions, "targetBlank") && app.newWebContents === 0 && (app.pageNavigationsBlocked ?? 0) >= 2,
    "T9 three top-level navigations were each refused and the page stayed": app.urlAfterNavigation?.same === true && app.urlAfterNavigation?.origin === true && (app.topNavigationsBlocked ?? 0) >= 3,
    "T9 frames (meta refresh, src change) never reached their outside target": has(leak, "frameRefresh") && has(leak, "frameNavigate")
      && leak.frameRefresh.value !== "/refresh" && leak.frameNavigate.value !== "/framenav" && fromPage.tcp4.count === 0,
    // T10
    "T10 every permission refused by the boundary under a user gesture on a focused view": PERMISSIONS.every(boundaryRefusal)
      && permissions.notification?.value === "denied" && permissions.permissionGeolocation?.value === "denied" && permissions.permissionCamera?.value === "denied",
    // T11
    "T11 a hung page leaves the app responsive (<250 ms) and closing ends its process": app.hangAppLatencyMs < 250 && app.hangClosed === "closed" && app.hangProcessEnded === true,
    // T12
    "T12 another task's preview does not see this one's storage": app.bStorage === "workspace-b" && leak.storageBefore === null,
    "T12 no preview partition on disk": Array.isArray(app.partitionsOnDisk) && app.partitionsOnDisk.length === 0,
    // T13
    "T13 service worker registration fails": refused(leak, "serviceWorker"),
    // T14
    "T14 the app window, default session and a plain window cannot load a preview URL":
      /^error/.test(app.appWindowFetch ?? "") && /^error/.test(app.defaultSessionFetch ?? "") && /^error/.test(app.defaultSessionWindow ?? ""),
    // T15
    "T15 another (closed) token is not readable from this preview": /^(error|status 404)/.test(app.crossToken ?? ""),
    "T15 tokens differ per preview and never reach the renderer": app.tokensDiffer === true && app.previewIdHasToken === false,
    // Function and lifetime
    "the todo app runs (JavaScript, storage) wide and narrow": app.todoCount === "1개 남음" && /좁은 화면/.test(app.todoStorage ?? "") && app.todoNarrowWidth === 390,
    "obscuring hides the view and returns a still image": app.obscured?.state === "hidden" && app.obscured?.snapshot === true,
    "no preview view while none is open (idle, after close)": app.idleViews === 0 && app.viewsAfterClose === 0,
    "closing the preview returns to the idle renderer count": app.memory?.afterClose?.renderers === app.memory?.idle?.renderers,
    [`the task workspaces are ${mode === "git" ? "real Git worktrees in the probe's task folder" : "snapshot copies in the isolated app data"}`]: placeOk && !leakedToHome,
    "the probe finished and the app quit cleanly": app.steps?.includes("done") && app.errors?.length === 0 && exitCode === 0,
  };
  const failed = Object.entries(checks).filter(([, ok]) => !ok).map(([name]) => name);
  const modeStatus = failed.length === 0 ? "PASS" : "FAIL";
  for (const image of ["todo-wide-view.png", "todo-narrow-view.png"]) if (existsSync(join(root, image))) copyFileSync(join(root, image), join(outDirectory, `${mode}-${image}`));
  const evidence = join(outDirectory, `evidence-${mode}.json`);
  writeFileSync(evidence, JSON.stringify({
    schemaVersion: 2, probe: "m5-preview-isolation", mode, status: modeStatus, failed, measuredAt: new Date().toISOString(),
    durationMs: Date.now() - startedAt, head, dirtySources: dirty,
    electron: JSON.parse(readFileSync(join(repoRoot, "node_modules", "electron", "package.json"), "utf8")).version,
    scope: `macOS development build, synthetic project, ${mode} task workspaces, no provider; the page used no outside network`,
    checks, collectors: { fromPage, afterControl: collectors },
    netLog: net, app: { ...app, leak }, exitCode, root,
  }, null, 2));
  if (modeStatus !== "PASS") console.error(output.split("\n").filter(line => /error|Error|panic/.test(line)).slice(-20).join("\n"));
  return { mode, status: modeStatus, failed, evidence };
}

/** Chromium's net log, all sessions. A lookup counts when it reaches the
 * network (a DNS transaction, a system or DNS task, or a resolver task list
 * with SYSTEM/DNS/SECURE_DNS/MDNS = 0/1/2/3); a cache or hosts-file look
 * (tasks 4/9 only) sends nothing. A connection counts by its real address:
 * anything but the dead proxy 127.0.0.1:9. A log that does not parse as
 * written is incomplete, and incomplete fails. */
function analyzeNetLog(file, needle, control) {
  const empty = { complete: false, lookups: ["net log missing"], connects: ["net log missing"], localOnly: [], reachability: [], controlLookups: 0, proxyAttempts: 0, markerRequests: 0, events: 0 };
  if (!existsSync(file)) return empty;
  let log;
  try { log = JSON.parse(readFileSync(file, "utf8")); } catch { return { ...empty, lookups: ["net log incomplete"], connects: ["net log incomplete"] }; }
  const names = Object.fromEntries(Object.entries(log.constants.logEventTypes).map(([name, id]) => [id, name]));
  // The resolver learns whether IPv6 is reachable by a UDP connect() to
  // Chromium's fixed probe address (net/dns: 2001:4860:4860::8888, port 443),
  // logged right before an uncached IPV6_REACHABILITY_CHECK. connect() on UDP
  // sends no packet and carries nothing of the page.
  const IPV6_PROBE = "[2001:4860:4860::8888]:443";
  const reachability = new Set();
  let probeConnect = null;
  const lookups = new Set();
  const localOnly = new Set();
  const connects = new Set();
  const pending = new Map();
  let controlLookups = 0;
  // Network lookups of any other name, and the DNS servers contacted: the
  // servers may only serve the probe's own control lookup.
  const otherLookups = new Set();
  const resolverConnects = new Set();
  let proxyAttempts = 0;
  let markerRequests = 0;
  for (const event of log.events ?? []) {
    const name = names[event.type] ?? String(event.type);
    const params = event.params ? JSON.stringify(event.params) : "";
    const source = event.source?.id;
    if (/DNS_TRANSACTION|HOST_RESOLVER_SYSTEM_TASK|HOST_RESOLVER_DNS_TASK|MDNS/.test(name) && params.includes(needle)) lookups.add(`${name} ${params.slice(0, 160)}`);
    if (name === "HOST_RESOLVER_MANAGER_REQUEST" && event.phase !== 2) pending.set(source, params);
    if (name === "HOST_RESOLVER_MANAGER_TASK_SEQUENCE_CREATED" && pending.has(source)) {
      const request = pending.get(source);
      const host = String(JSON.parse(request).host ?? "");
      const network = (event.params?.tasks ?? []).some(task => task <= 3);
      // An address written as numbers needs no lookup to leave the machine.
      const literal = /^(?:[a-z-]+:\/\/)?(?:\d{1,3}(?:\.\d{1,3}){3}|\[[0-9a-f:]+\])(?::\d+)?$/i.test(host);
      if (request.includes(control)) { if (network) controlLookups++; }
      else if (request.includes(needle)) (network ? lookups : localOnly).add(`${request.slice(0, 160)} tasks=${JSON.stringify(event.params?.tasks ?? [])}`);
      else if (network && !literal) otherLookups.add(host);
      pending.delete(source);
    }
    if (name === "REQUEST_ALIVE" && params.includes(needle)) markerRequests++;
    if (name === "HOST_RESOLVER_MANAGER_IPV6_REACHABILITY_CHECK" && event.params?.cached === false && probeConnect) {
      reachability.add(probeConnect);
      probeConnect = null;
    }
    if (/CONNECT_ATTEMPT|^TCP_CONNECT$|^UDP_CONNECT$/.test(name) && typeof event.params?.address === "string") {
      const address = event.params.address;
      if (address === "127.0.0.1:9") { proxyAttempts++; continue; }
      // A DNS server: judged below by which lookups needed one.
      if (name === "UDP_CONNECT" && /:53$/.test(address)) { resolverConnects.add(address); continue; }
      const label = `${name} ${address}`;
      if (name === "UDP_CONNECT" && address === IPV6_PROBE) { if (probeConnect) connects.add(probeConnect); probeConnect = label; }
      else connects.add(label);
    }
  }
  if (probeConnect) connects.add(probeConnect);
  if (resolverConnects.size && (otherLookups.size || lookups.size || !controlLookups)) for (const address of resolverConnects) connects.add(`UDP_CONNECT ${address} (DNS server, not only for the control lookup)`);
  return {
    otherLookups: [...otherLookups], resolverConnects: [...resolverConnects],
    complete: true, lookups: [...lookups], connects: [...connects], localOnly: [...localOnly], reachability: [...reachability],
    controlLookups, proxyAttempts, markerRequests, events: log.events?.length ?? 0,
  };
}

async function buildApp() {
  const { build } = await import("vite");
  const external = ["electron", "electron/common", ...builtinModules.flatMap(name => [name, `node:${name}`])];
  const define = { MAIN_WINDOW_VITE_DEV_SERVER_URL: "undefined", MAIN_WINDOW_VITE_NAME: JSON.stringify("main_window") };
  await build({
    configFile: false, root: repoRoot, mode: "production", logLevel: "warn", define,
    build: { emptyOutDir: false, outDir: ".vite/build", minify: false,
      lib: { entry: "electron/main.ts", formats: ["cjs"], fileName: () => "main.cjs" }, rollupOptions: { external } },
  });
  await build({
    configFile: false, root: repoRoot, mode: "production", logLevel: "warn", define,
    build: { emptyOutDir: false, outDir: ".vite/build", minify: false,
      rollupOptions: { external, input: "electron/preload.ts", output: { format: "cjs", inlineDynamicImports: true, entryFileNames: "[name].cjs", chunkFileNames: "[name].cjs" } } },
  });
  await build({ configFile: join(repoRoot, "electron", "vite.renderer.config.ts"), logLevel: "warn" });
}
