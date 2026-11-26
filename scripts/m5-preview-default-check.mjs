#!/usr/bin/env node
// Is the result preview on by default where it should be (macOS), off by
// default elsewhere, off with ELSECREW_PREVIEW=0 and on with =1 — the same in
// a development build and a packaged app? Starts the app three times with
// isolated app data and a free debugging port on 127.0.0.1, reads
// `m5Preview.status()` from the app page and the browser's own command line
// (the app-wide `--blink-settings=dnsPrefetchingEnabled=false`), then closes it.
//
//   node scripts/m5-preview-default-check.mjs dev
//   node scripts/m5-preview-default-check.mjs packaged [path/to/the app's executable or Elsecrew.app]
//
// macOS and Windows (PR #9 review I5). The Windows packaged path is
// out/Elsecrew-win32-<arch>/Elsecrew.exe and has not been run yet; other
// platforms exit 2 (not run).
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { mkdtempSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const kind = process.argv[2];
if (kind !== "dev" && kind !== "packaged") throw new Error("usage: m5-preview-default-check.mjs dev|packaged [app]");
if (process.platform !== "darwin" && process.platform !== "win32") {
  console.log(JSON.stringify({ kind, status: "NOT_RUN", reason: `the default check covers macOS and Windows only, not ${process.platform}` }));
  process.exit(2);
}
// Where the preview is on by default (electron/preview-flag.ts PREVIEW_DEFAULT_PLATFORMS).
const onByDefault = process.platform === "darwin";
const packagedPath = () => {
  const given = process.argv[3];
  if (process.platform === "win32") return given ?? join(repoRoot, "out", `Elsecrew-win32-${process.arch}`, "Elsecrew.exe");
  const bundle = given ?? join(repoRoot, "out", `Elsecrew-darwin-${process.arch}`, "Elsecrew.app");
  return bundle.endsWith(".app") ? join(bundle, "Contents", "MacOS", "Elsecrew") : bundle;
};
// The Electron binary itself (`require("electron")` is its path), on every platform.
const executable = kind === "dev" ? createRequire(import.meta.url)("electron") : packagedPath();
if (!existsSync(executable)) throw new Error(`missing ${executable}`);

const freePort = () => new Promise(done => { const server = createServer(); server.listen(0, "127.0.0.1", () => { const { port } = server.address(); server.close(() => done(port)); }); });
const sleep = ms => new Promise(done => setTimeout(done, ms));

async function cdp(url) {
  const socket = new WebSocket(url);
  await new Promise((done, fail) => { socket.onopen = done; socket.onerror = fail; });
  let next = 0;
  const pending = new Map();
  socket.onmessage = message => { const data = JSON.parse(message.data); pending.get(data.id)?.(data); pending.delete(data.id); };
  return {
    // Never waits past 15 s: closing the browser may end the socket without an answer.
    send: (method, params = {}) => Promise.race([
      new Promise(done => { const id = ++next; pending.set(id, done); socket.send(JSON.stringify({ id, method, params })); }),
      sleep(15_000).then(() => ({ timeout: true })),
    ]),
    close: () => socket.close(),
  };
}

async function run(setting) {
  const port = await freePort();
  const userData = mkdtempSync(join(tmpdir(), `elsecrew-preview-default-${kind}-`));
  const env = { ...process.env };
  delete env.ELSECREW_PREVIEW;
  delete env.ELECTRON_RUN_AS_NODE;
  if (setting !== null) env.ELSECREW_PREVIEW = setting;
  const args = [...(kind === "dev" ? ["."] : []), `--user-data-dir=${userData}`, `--remote-debugging-port=${port}`, "--remote-debugging-address=127.0.0.1", "--enable-automation"];
  const child = spawn(executable, args, { cwd: repoRoot, env, stdio: "ignore" });
  try {
    let version = null;
    for (let index = 0; index < 120 && !version; index++) {
      await sleep(250);
      version = await fetch(`http://127.0.0.1:${port}/json/version`).then(r => r.json(), () => null);
    }
    if (!version) throw new Error("debugging endpoint never answered");
    console.error(`[${kind} ${setting ?? "default"}] browser ${version.Browser}`);
    const browser = await cdp(version.webSocketDebuggerUrl);
    const commandLine = (await browser.send("Browser.getBrowserCommandLine")).result?.arguments ?? [];
    let target = null;
    for (let index = 0; index < 120 && !target; index++) {
      const list = await fetch(`http://127.0.0.1:${port}/json/list`).then(r => r.json(), () => []);
      target = list.find(entry => entry.type === "page" && /index\.html/.test(entry.url));
      if (!target) await sleep(250);
    }
    if (!target) throw new Error("app page never appeared");
    const pageSession = await cdp(target.webSocketDebuggerUrl);
    let status = null;
    for (let index = 0; index < 60 && status === null; index++) {
      const answer = await pageSession.send("Runtime.evaluate", { expression: "window.m5Preview ? window.m5Preview.status() : null", awaitPromise: true, returnByValue: true });
      status = answer.result?.result?.value ?? null;
      if (status === null) await sleep(250);
    }
    pageSession.close();
    await browser.send("Browser.close").catch(() => undefined);
    browser.close();
    return {
      setting: setting ?? "(none)", enabled: status?.enabled ?? null,
      dnsPrefetchOff: commandLine.some(arg => /^--blink-settings=.*dnsPrefetchingEnabled=false/.test(arg)),
    };
  } finally {
    await sleep(1500);
    if (child.exitCode === null) child.kill("SIGTERM");
  }
}

const expected = { "(none)": onByDefault, "0": false, "1": true };
const results = [await run(null), await run("0"), await run("1")];
for (const result of results) result.expected = expected[result.setting];
const ok = results.every(result => result.enabled === result.expected && result.dnsPrefetchOff === result.expected);
console.log(JSON.stringify({ kind, platform: process.platform, executable, status: ok ? "PASS" : "FAIL", results }, null, 2));
process.exit(ok ? 0 : 1);
