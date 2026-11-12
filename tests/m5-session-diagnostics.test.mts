import test from "node:test";
import assert from "node:assert/strict";
import {classifySessionDiagnostics,validateSessionDiagnostics} from "../src/m5/contracts/session-diagnostics-v1.ts";

test("session diagnostics expose fixed checkpoint names, never raw provider data", () => {
  const result = classifySessionDiagnostics([
    "secret-token", "native Claude transport failed at init model",
    "native Claude transport failed at init model secret-token",
    "native Claude transport failed at /private/credential",
    "prefix native Claude transport failed at channel receive",
    "native Claude transport failed at session channel",
  ].join("\n"));
  assert.deepEqual(result,{scope:"currentAppSession",claudeCheckpoints:["init model","session channel"],codexCheckpoints:[]});
  assert.deepEqual(validateSessionDiagnostics(result),result);
});
test("Codex diagnostics use a separate bounded allowlist and preserve legacy replies", () => {
  const result = classifySessionDiagnostics("native Codex transport failed at turn stream other frame\nnative Codex transport failed at secret-token\nnative Claude transport failed at init model");
  assert.deepEqual(result.codexCheckpoints,["turn stream other frame"]);
  assert.deepEqual(validateSessionDiagnostics(result),result);
  assert.deepEqual(validateSessionDiagnostics({scope:"currentAppSession",claudeCheckpoints:[]}),{scope:"currentAppSession",claudeCheckpoints:[]});
  for (const codexCheckpoints of [null,undefined,["secret"],["turn stream other frame token"],Array(33).fill("turn start state")]) {
    assert.throws(()=>validateSessionDiagnostics({scope:"currentAppSession",claudeCheckpoints:[],codexCheckpoints}));
  }
  assert.equal(classifySessionDiagnostics("native Codex transport failed at turn start state\n".repeat(1000)).codexCheckpoints?.length,32);
});
test("diagnostics are bounded and unknown text is not a successful diagnosis", () => {
  assert.equal(classifySessionDiagnostics(("native Claude transport failed at init shape\n").repeat(10000)).claudeCheckpoints.length,32);
  assert.deepEqual(classifySessionDiagnostics("no known checkpoint").claudeCheckpoints,[]);
  for(const value of [null, {scope:"run",claudeCheckpoints:[]},
    {scope:"currentAppSession",claudeCheckpoints:["secret"]},
    {scope:"currentAppSession",claudeCheckpoints:[],raw:"secret"},
    {scope:"currentAppSession",claudeCheckpoints:Array(33).fill("init model")}]) {
    assert.throws(()=>validateSessionDiagnostics(value));
  }
});


test("all fixed Rust item-start checkpoints cross the renderer allowlist without raw payloads", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = await readFile(new URL("../crates/elsecrew-core/src/provider/vendor_observation/codex_events/stream.rs", import.meta.url), "utf8");
  const labels = [...new Set([...source.matchAll(/"(turn item start [a-z ]+)"/g)].map(match => match[1]!))];
  assert.ok(labels.length >= 18);
  const result = classifySessionDiagnostics(labels.map(label => `native Codex transport failed at ${label}`).join("\n"));
  assert.deepEqual(result.codexCheckpoints, labels);
  assert.deepEqual(validateSessionDiagnostics(result), result);
  assert.deepEqual(classifySessionDiagnostics("native Codex transport failed at turn item start SECRET_TYPE").codexCheckpoints, []);
});
