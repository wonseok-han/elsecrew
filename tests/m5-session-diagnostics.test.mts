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
  assert.deepEqual(result,{scope:"currentAppSession",claudeCheckpoints:["init model","session channel"]});
  assert.deepEqual(validateSessionDiagnostics(result),result);
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
