#!/usr/bin/env node
// Result preview isolation probe (plan 2.4 "확인" column, 2.6 Electron 통합
// 프로브). macOS development build, synthetic project, isolated app data.
//
//   node scripts/m5-preview-isolation-probe.mjs [--build] [--out <dir>]
//
// --build   builds main/preload/renderer into .vite/ first (no packaging)
// Needs target/debug/elsecrew-sidecar (`cargo build -p elsecrew-sidecar`).
//
// Judges with three sources: local collectors (HTTP/WebSocket on one TCP
// port, a UDP port for STUN) that must see zero connections; Chromium's net
// log, which must show no host lookup or connection for the probe's unique
// host names; and what the page itself reports. No outside network is used:
// outside hosts only need to be refused before any lookup.
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
const outIndex = args.indexOf("--out");
const outDirectory = resolve(outIndex >= 0 ? args[outIndex + 1] : join(repoRoot, ".artifacts", "m5-preview-isolation"));
if (process.platform !== "darwin") throw new Error("the preview isolation probe runs on macOS");

if (args.includes("--build")) await buildApp();
for (const required of [".vite/build/main.cjs", ".vite/build/preload.cjs", ".vite/renderer/main_window/index.html", "target/debug/elsecrew-sidecar"]) {
  if (!existsSync(join(repoRoot, required))) throw new Error(`missing ${required}; run with --build and cargo build -p elsecrew-sidecar`);
}

const root = mkdtempSync(join(tmpdir(), "elsecrew-preview-probe-"));
const marker = `elsecrewprobe${randomBytes(6).toString("hex")}`;
const projectName = `preview-probe-${randomBytes(4).toString("hex")}`;
const source = join(root, "project");
mkdirSync(source);
// Renamed so the default task folder would show it if snapshot mode were ignored.
const project = join(root, projectName);
writeFileSync(join(source, "README.md"), "preview probe\n");
const git = (...gitArgs) => execFileSync("/usr/bin/git", gitArgs, { cwd: source, env: { ...process.env, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null" } });
git("init", "-q", "--template=");
git("add", ".");
git("-c", "core.hooksPath=/dev/null", "-c", "user.name=Probe", "-c", "user.email=probe@example.invalid", "commit", "-qm", "fixture");
void project;

// Collectors: any connection at all is a leak.
const tcp = { connections: 0, requests: [] };
const server = createServer(socket => {
  tcp.connections++;
  socket.once("data", chunk => { tcp.requests.push(chunk.toString("latin1").split("\r\n")[0].slice(0, 120)); socket.destroy(); });
  socket.on("error", () => undefined);
});
await new Promise(done => server.listen(0, "127.0.0.1", done));
const udp = { packets: 0 };
const udpSocket = createSocket("udp4");
udpSocket.on("message", () => { udp.packets++; });
await new Promise(done => udpSocket.bind(0, "127.0.0.1", done));
const discardPortClosed = await new Promise(done => {
  const socket = connect(9, "127.0.0.1");
  socket.once("connect", () => { socket.destroy(); done(false); });
  socket.once("error", error => done(error.code === "ECONNREFUSED"));
});

const electron = join(repoRoot, "node_modules", ".bin", "electron");
const env = {
  ...process.env,
  ELSECREW_PREVIEW: "1",
  ELSECREW_GIT_WORKTREES: "0",
  PROBE_ROOT: root,
  PROBE_HTTP_PORT: String(server.address().port),
  PROBE_UDP_PORT: String(udpSocket.address().port),
  PROBE_MARKER: marker,
};
delete env.ELSECREW_PREVIEW_DEVTOOLS;
delete env.ELECTRON_RUN_AS_NODE;
const startedAt = Date.now();
const child = spawn(electron, [join(repoRoot, "scripts", "m5-preview-isolation-probe-app.cjs")], { cwd: repoRoot, env, stdio: ["ignore", "pipe", "pipe"] });
let output = "";
child.stdout.on("data", chunk => { output += chunk; process.stdout.write(chunk); });
child.stderr.on("data", chunk => { output += chunk; });
const exitCode = await new Promise(done => {
  const timer = setTimeout(() => { child.kill("SIGTERM"); done("timeout"); }, 300_000);
  child.once("exit", code => { clearTimeout(timer); done(code); });
});
await new Promise(done => setTimeout(done, 500));
server.close();
udpSocket.close();

const app = JSON.parse(readFileSync(join(root, "app-result.json"), "utf8"));
const net = analyzeNetLog(join(root, "netlog.json"), marker);
const defaultTaskFolder = join(homedir(), "elsecrew", "workspaces");
const leakedToHome = existsSync(defaultTaskFolder) && readdirSync(defaultTaskFolder).some(name => name.includes("preview-probe") || name === "project");

const leak = app.leak ?? {};
const MUST_FAIL = ["fetchLoopback", "fetchNoCors", "fetchExternal", "xhr", "image", "imageExternal", "websocket", "eventSource", "script", "stylesheet", "font", "dynamicImport"];
const failed = (name) => leak[name] && leak[name].ok === false;
const checks = {
  // T1
  "T1 no TCP connection reached the local HTTP/WebSocket collector": tcp.connections === 0,
  // The collectors are the judge; these say each way was attempted and failed.
  "T1 fetch/xhr/img/ws/sse/script/css/font/import/worker failed in the page (CSP layer)": MUST_FAIL.every(name => failed(name)) && /^error/.test(leak.worker?.value ?? "error"),
  "T1 with CSP bypassed, webRequest alone stopped them": MUST_FAIL.every(name => app.layerWebRequest?.[name]?.ok === false) && (app.layerWebRequestBlocked ?? 0) > 0,
  "T1 with CSP bypassed and no webRequest, the dead proxy alone stopped them": MUST_FAIL.filter(name => name !== "dynamicImport").every(name => app.layerProxy?.[name]?.ok === false),
  "T1 no unexpected outside answer reached the preview": (app.diagnostics?.unexpectedResponses ?? 1) === 0,
  "T1 net log: no connection attempt to anything but the dead proxy": net.connects.length === 0,
  // T2
  "T2 no UDP packet reached the STUN collector": udp.packets === 0,
  "T2 no ICE candidate beyond none/host-only": Array.isArray(leak.webrtc?.value) ? leak.webrtc.value.every(type => type === "host") : true,
  // T3
  "T3 net log: no network host lookup for the probe's unique names (dns-prefetch included)": net.lookups.length === 0,
  // T4·T5
  "T4/T5 hidden, secret, linked and escaping paths are not served": Object.entries(leak.paths ?? {}).every(([path, status]) => path === "data.txt" ? status === 200 : status !== 200) && leak.paths?.["data.txt"] === 200,
  // T6
  "T6 the page has no app bridge, ipcRenderer, require or process": Object.values(leak.bridges ?? { missing: "x" }).every(type => type === "undefined"),
  // T7
  "T7 POST/PUT/DELETE/PATCH answer 405": ["methodPOST", "methodPUT", "methodDELETE", "methodPATCH"].every(name => leak[name]?.value === 405),
  "T7 file pickers refused": failed("saveFilePicker") && failed("directoryPicker"),
  "T7 the download was cancelled in main": (app.diagnostics?.blockedDownloads ?? 0) >= 1,
  "T7 the workspace is byte-for-byte unchanged": app.workspaceUnchanged === true,
  // T9
  "T9 window.open returned nothing; _blank and frames stayed inside": leak.windowOpen?.value === "null" && (app.diagnostics?.blockedNavigations ?? 0) > 0,
  "T9 top-level navigation left the page where it was": app.urlAfterNavigation?.same === true && app.urlAfterNavigation?.origin === true,
  "T9 a meta refresh in a frame did not leave the origin": leak.frameRefresh?.value === "/refresh.html" || failed("frameRefresh"),
  // T10
  "T10 camera, microphone, screen, location, notification, clipboard, HID/USB/serial, MIDI, fullscreen refused": [
    "camera", "microphone", "displayMedia", "geolocation", "clipboardRead", "hid", "usb", "serial", "midi", "fullscreen",
  ].every(name => failed(name)) && (leak.notification?.value === "denied" || failed("notification")),
  // T11
  "T11 a hung page leaves the app responsive (<250 ms) and closing ends its process": app.hangAppLatencyMs < 250 && app.hangClosed === "closed" && app.hangProcessEnded === true,
  // T12
  "T12 another task's preview does not see this one's storage": app.bStorage === "workspace-b" && (leak.storageBefore === null || leak.storageBefore === undefined),
  "T12 no preview partition on disk": Array.isArray(app.partitionsOnDisk) && app.partitionsOnDisk.length === 0,
  // T13
  "T13 service worker registration fails": failed("serviceWorker"),
  // T14
  "T14 the app window, default session and a plain window cannot load a preview URL":
    /^error/.test(app.appWindowFetch ?? "") && /^error/.test(app.defaultSessionFetch ?? "") && /^error/.test(app.defaultSessionWindow ?? ""),
  // T15
  "T15 another (closed) token is not readable from this preview": !/^status 200/.test(app.crossToken ?? "status 200"),
  "T15 tokens differ per preview and never reach the renderer": app.tokensDiffer === true && app.previewIdHasToken === false,
  // Function
  "the todo app runs (JavaScript, storage) wide and narrow": app.todoCount === "1개 남음" && /좁은 화면/.test(app.todoStorage ?? "") && app.todoNarrowWidth === 390,
  "obscuring hides the view and returns a still image": app.obscured?.state === "hidden" && app.obscured?.snapshot === true,
  "no preview view while none is open (idle, after close)": app.idleViews === 0 && app.viewsAfterClose === 0,
  "closing the preview returns to the idle renderer count": app.memory?.afterClose?.renderers === app.memory?.idle?.renderers,
  "snapshot workspaces stayed in the isolated app data": (app.chats?.kinds ?? []).every(kind => kind === "insideAppData") && !leakedToHome,
  "the probe finished and the app quit cleanly": app.steps?.includes("done") && app.errors?.length === 0 && exitCode === 0,
  "the discard port the dead proxy points at is closed": discardPortClosed === true,
};
const status = Object.values(checks).every(Boolean) ? "PASS" : "FAIL";
mkdirSync(outDirectory, { recursive: true });
for (const image of ["todo-wide-view.png", "todo-narrow-view.png"]) if (existsSync(join(root, image))) copyFileSync(join(root, image), join(outDirectory, image));
const evidence = {
  schemaVersion: 1,
  probe: "m5-preview-isolation",
  status,
  measuredAt: new Date().toISOString(),
  durationMs: Date.now() - startedAt,
  head: execFileSync("/usr/bin/git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim(),
  electron: JSON.parse(readFileSync(join(repoRoot, "node_modules", "electron", "package.json"), "utf8")).version,
  scope: "macOS development build, synthetic project, snapshot workspaces in isolated app data, no provider, no outside network",
  checks,
  collectors: { tcpConnections: tcp.connections, tcpRequests: tcp.requests, udpPackets: udp.packets, discardPortClosed },
  netLog: { lookups: net.lookups, connects: net.connects, localOnly: net.localOnly, reachability: net.reachability, events: net.events },
  app: { ...app, leak },
  exitCode,
  root,
};
writeFileSync(join(outDirectory, "evidence.json"), JSON.stringify(evidence, null, 2));
console.log(JSON.stringify({ status, failed: Object.entries(checks).filter(([, ok]) => !ok).map(([name]) => name), evidence: join(outDirectory, "evidence.json") }, null, 2));
if (status !== "PASS") console.error(output.split("\n").filter(line => /error|Error|panic/.test(line)).slice(-20).join("\n"));
process.exit(status === "PASS" ? 0 : 1);

/** Chromium's net log, all sessions. A lookup counts when it reaches the
 * network (a DNS transaction, a system or DNS task, or a resolver task list
 * with SYSTEM/DNS/SECURE_DNS/MDNS = 0/1/2/3); a cache or hosts-file look
 * (tasks 4/9 only) sends nothing. A connection counts by its real address:
 * anything but the dead proxy 127.0.0.1:9. */
function analyzeNetLog(file, needle) {
  if (!existsSync(file)) return { lookups: ["net log missing"], connects: ["net log missing"], localOnly: [], reachability: [], events: 0 };
  let log;
  const text = readFileSync(file, "utf8");
  try { log = JSON.parse(text); }
  catch {
    // A log cut short at exit: close its event list.
    log = JSON.parse(text.replace(/,?\s*$/, "") + "]}");
  }
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
  for (const event of log.events ?? []) {
    const name = names[event.type] ?? String(event.type);
    const params = event.params ? JSON.stringify(event.params) : "";
    const source = event.source?.id;
    if (/DNS_TRANSACTION|HOST_RESOLVER_SYSTEM_TASK|HOST_RESOLVER_DNS_TASK|MDNS/.test(name) && params.includes(needle)) lookups.add(`${name} ${params.slice(0, 160)}`);
    if (name === "HOST_RESOLVER_MANAGER_REQUEST" && params.includes(needle)) pending.set(source, params.slice(0, 160));
    if (name === "HOST_RESOLVER_MANAGER_TASK_SEQUENCE_CREATED" && pending.has(source)) {
      const tasks = event.params?.tasks ?? [];
      (tasks.some(task => task <= 3) ? lookups : localOnly).add(`${pending.get(source)} tasks=${JSON.stringify(tasks)}`);
      pending.delete(source);
    }
    if (name === "HOST_RESOLVER_MANAGER_IPV6_REACHABILITY_CHECK" && event.params?.cached === false && probeConnect) {
      reachability.add(probeConnect);
      probeConnect = null;
    }
    if (/CONNECT_ATTEMPT|^TCP_CONNECT$|^UDP_CONNECT$/.test(name) && typeof event.params?.address === "string" && event.params.address !== "127.0.0.1:9") {
      const label = `${name} ${event.params.address}`;
      if (name === "UDP_CONNECT" && event.params.address === IPV6_PROBE) { if (probeConnect) connects.add(probeConnect); probeConnect = label; }
      else connects.add(label);
    }
  }
  if (probeConnect) connects.add(probeConnect);
  return { lookups: [...lookups], connects: [...connects], localOnly: [...localOnly], reachability: [...reachability], events: log.events?.length ?? 0 };
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
