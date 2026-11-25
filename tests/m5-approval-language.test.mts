import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import test from "node:test";
import ts from "typescript";
import { renderToStaticMarkup } from "react-dom/server";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";
import * as contract from "../src/m5/contracts/approval-v1.ts";
import * as display from "../src/m5/approval-display.ts";

const require = createRequire(import.meta.url);
const compiled = ts.transpileModule(readFileSync(new URL("../src/m5/approval-card.tsx", import.meta.url), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020 },
}).outputText;

test("approval translations preserve warnings, raw detail and inspected acceptance controls", () => {
  for (const dictionary of [ko, en]) {
    let index = 0;
    // useState order: pending, error, sending, queued, inspection, withdrawn, unreadable.
    const states = [[{approvalId:"approval_test",kind:"command"}], true, true, true,
      {approvalId:"approval_test",kind:"command",command:"echo PRIVATE <tag>",cwd:"/snapshot",reason:"run the tests",grantRoot:null,host:null,protocol:null}, [], new Set()];
    const context: any = { exports: {}, require(name: string) {
      if (name === "react") return { useEffect() {}, useRef: (value: unknown) => ({current:value}), useState: () => [states[index++], () => {}] };
      if (name === "../i18n.tsx") return { useI18n: () => ({ t(key: keyof typeof ko, params: Record<string, unknown> = {}) {
        return dictionary[key].replace(/\{(\w+)\}/g, (_, key) => String(params[key]));
      } }) };
      if (name === "./contracts/approval-v1.ts") return contract;
      if (name === "./approval-display.ts") return display;
      return require(name);
    } };
    runInNewContext(compiled, context);
    const html = renderToStaticMarkup(context.exports.ApprovalCard({identity:{runId:"run_test"},onCancel() {}}));
    for (const key of ["help","warning","queued","error","cancel","decline"] as const) {
      const expected = renderToStaticMarkup(dictionary[`m5.approval.${key}`]);
      assert.ok(html.includes(expected), key);
    }
    // The first line says what is asked and why, before any click (ADE D13).
    const line = html.slice(html.indexOf("m5-approval-line"), html.indexOf("m5-approval-actions"));
    assert.ok(line.includes("echo PRIVATE &lt;tag&gt;"), line);
    assert.ok(line.includes(renderToStaticMarkup(dictionary["m5.approval.kind.command"])));
    assert.ok(line.includes(renderToStaticMarkup(dictionary["m5.approval.why"].replace("{reason}", "run the tests"))));
    assert.ok(html.includes(renderToStaticMarkup(dictionary["m5.approval.acceptOnce"])));
    assert.ok(html.includes(renderToStaticMarkup(dictionary["m5.approval.acceptWarning"])));
    // Accept, decline (both locked while sending or after an error) and cancel.
    assert.equal((html.match(/<button/g) ?? []).length, 3);
    assert.equal((html.match(/disabled=""/g) ?? []).length, 2);
  }
});

test("a request the agent took back is said briefly instead of vanishing", () => {
  for (const dictionary of [ko, en]) {
    let index = 0;
    const states = [[], false, false, false, null, [{approvalId:"approval_gone",kind:"tool",summary:"npm test"}], new Set()];
    const context: any = { exports: {}, require(name: string) {
      if (name === "react") return { useEffect() {}, useRef: (value: unknown) => ({current:value}), useState: () => [states[index++], () => {}] };
      if (name === "../i18n.tsx") return { useI18n: () => ({ t(key: keyof typeof ko) { return dictionary[key]; } }) };
      if (name === "./contracts/approval-v1.ts") return contract;
      if (name === "./approval-display.ts") return display;
      return require(name);
    } };
    runInNewContext(compiled, context);
    const html = renderToStaticMarkup(context.exports.ApprovalCard({identity:{runId:"run_test"},onCancel() {}}));
    assert.ok(html.includes(renderToStaticMarkup(dictionary["m5.approval.withdrawn"])));
    assert.ok(html.includes("npm test"));
    assert.equal((html.match(/<button/g) ?? []).length, 0, "nothing to answer once the request is gone");
  }
});

test("a request that cannot be read says so instead of loading forever", () => {
  let index = 0;
  const states = [[{approvalId:"approval_x",kind:"tool"}], false, false, false, null, [], new Set(["approval_x"])];
  const context: any = { exports: {}, require(name: string) {
    if (name === "react") return { useEffect() {}, useRef: (value: unknown) => ({current:value}), useState: () => [states[index++], () => {}] };
    if (name === "../i18n.tsx") return { useI18n: () => ({ t(key: keyof typeof ko) { return ko[key]; } }) };
    if (name === "./contracts/approval-v1.ts") return contract;
    if (name === "./approval-display.ts") return display;
    return require(name);
  } };
  runInNewContext(compiled, context);
  const html = renderToStaticMarkup(context.exports.ApprovalCard({identity:{runId:"run_test"},onCancel() {}}));
  assert.ok(html.includes(renderToStaticMarkup(ko["m5.approval.unreadable"])));
  assert.ok(!html.includes(ko["m5.approval.reading"]));
  assert.ok(!html.includes(renderToStaticMarkup(ko["m5.approval.acceptOnce"])), "nothing to accept without its content");
});

test("withdrawn requests are those that left without an answer, and the headline reads the request", () => {
  const before = [{approvalId:"a",kind:"command"},{approvalId:"b",kind:"tool"},{approvalId:"c",kind:"tool"}];
  assert.deepEqual(display.withdrawnRequests(before, [{approvalId:"c",kind:"tool"}], new Set(["a"])).map(item => item.approvalId), ["b"]);
  assert.deepEqual(display.withdrawnRequests(before, before, new Set()), []);
  const tool = {approvalId:"t",kind:"tool" as const,command:null,cwd:"/w",reason:"Bash\u202e tool",grantRoot:null,host:null,protocol:null,toolInput:JSON.stringify({command:"npm run build",description:"Build"})};
  assert.deepEqual(display.approvalHeadline(tool), { target: "npm run build", reason: "Bash\\u202e tool" });
  const file = {...tool, toolInput: JSON.stringify({file_path:"src/app.ts"}), reason: null};
  assert.deepEqual(display.approvalHeadline(file), { target: "src/app.ts", reason: null });
  const network = {approvalId:"n",kind:"network" as const,command:null,cwd:null,reason:null,grantRoot:null,host:"registry.npmjs.org",protocol:"https"};
  assert.equal(display.approvalHeadline(network).target, "https://registry.npmjs.org");
  assert.ok(Array.from(display.approvalHeadline({...network, host: "x".repeat(500)}).target).length <= 240);
});
