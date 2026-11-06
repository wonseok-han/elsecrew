import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import test from "node:test";
import * as contract from "../src/m5/contracts/project-catalog-v1.ts";
const require = createRequire(import.meta.url);
const ts = require("typescript");
function setup() {
  const calls: { command: string; args: string[]; options: any; body?: string; done: Function }[] = [];
  const module = { exports: {} as any };
  const source = readFileSync(new URL("../electron/git-preparation.ts", import.meta.url), "utf8");
  runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
    module, exports: module.exports, Buffer,
    require(name: string) {
      if (name.endsWith("project-catalog-v1.ts")) return contract;
      if (name === "node:child_process") return { execFile(command: string, args: string[], options: any, done: Function) {
        const call = { command, args, options, done, body: "" }; calls.push(call);
        return { stdin: { on() {}, end(body: string) { call.body = body; } } };
      } };
      throw new Error(`unexpected import ${name}`);
    },
  });
  return { client: new module.exports.GitPreparationClient({ command: "/fixed/sidecar", cwd: "/fixed/data" }), calls };
}
const result = JSON.stringify({ schemaVersion: 1, action: "revokeGitTrust", revoked: true });
const tick = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };

test("preparation serializes fixed helper requests and checks owner before and after execution", async () => {
  const { client, calls } = setup(); let checks = 0;
  const first = client.request({ action: "revokeGitTrust" }, () => checks++);
  const second = client.request({ action: "revokeGitTrust" }, () => checks++);
  await tick(); assert.equal(calls.length, 1); assert.equal(client.pending, true);
  assert.equal(calls[0].command, "/fixed/sidecar");
  assert.deepEqual(Array.from(calls[0].args), ["--git-preparation"]);
  assert.equal(calls[0].options.cwd, "/fixed/data"); assert.equal(calls[0].options.shell, false);
  assert.equal(calls[0].options.timeout, 30000); assert.equal(calls[0].options.maxBuffer, 65536);
  assert.equal(calls[0].body, '{"action":"revokeGitTrust"}');
  calls[0].done(null, result); await first; await tick(); assert.equal(calls.length, 2);
  calls[1].done(null, result); await second; await tick();
  assert.equal(checks, 4); assert.equal(client.pending, false);
});

test("close waits for active completion and rejects queued work without spawning", async () => {
  const { client, calls } = setup();
  const active = client.request({ action: "revokeGitTrust" }, () => {});
  const queued = client.request({ action: "revokeGitTrust" }, () => {});
  const rejected = assert.rejects(queued, /closing/);
  await tick(); let closed = false;
  const close = client.close().then(() => { closed = true; });
  await tick(); assert.equal(closed, false);
  calls[0].done(null, result); await active; await rejected; await close;
  assert.equal(calls.length, 1); assert.equal(client.pending, false);
  client.reopen(); const next = client.request({ action: "revokeGitTrust" }, () => {});
  await tick(); calls[1].done(null, result); await next;
});

test("owner loss, invalid results and helper errors never become successful trust updates", async () => {
  for (const failure of ["owner", "result", "process"]) {
    const { client, calls } = setup(); let count = 0;
    const request = client.request({ action: "revokeGitTrust" }, () => {
      if (++count === 2 && failure === "owner") throw new Error("owner changed");
    });
    const rejected = assert.rejects(request);
    await tick(); calls[0].done(failure === "process" ? new Error("timeout") : null, failure === "result" ? '{}' : result);
    await rejected; await client.close(); assert.equal(client.pending, false);
  }
  const { client, calls } = setup();
  await assert.rejects(client.request({ action: "applicationCapabilities" }, () => {}), /not Git preparation/);
  await assert.rejects(client.request({ action: "revokeGitTrust" }, () => { throw new Error("foreign frame"); }), /foreign frame/);
  assert.equal(calls.length, 0);
});
