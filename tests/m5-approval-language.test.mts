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

/** Renders the card with fixed hook state.
 * useState order: pending, error, sending, queued, inspections, opened, unreadable. */
function renderCard(dictionary: typeof ko, states: unknown[]): string {
  let index = 0;
  const context: any = { exports: {}, require(name: string) {
    if (name === "react") return { useEffect() {}, useRef: (value: unknown) => ({ current: value }), useState: (initial: unknown) => [index < states.length ? states[index++] : (typeof initial === "function" ? (initial as () => unknown)() : initial), () => {}] };
    if (name === "../i18n.tsx") return { useI18n: () => ({ t(key: keyof typeof ko, params: Record<string, unknown> = {}) {
      return dictionary[key].replace(/\{(\w+)\}/g, (_, key) => String(params[key]));
    } }) };
    if (name === "./contracts/approval-v1.ts") return contract;
    if (name === "./approval-display.ts") return display;
    return require(name);
  } };
  runInNewContext(compiled, context);
  return renderToStaticMarkup(context.exports.ApprovalCard({ identity: { runId: "run_test" }, onCancel() {} }));
}
const LONG_COMMAND = "npm run build --   --workspace  packages/app " + "--flag ".repeat(60) + "--last-option=keep";
const command = { approvalId: "approval_test", kind: "command" as const, command: LONG_COMMAND, cwd: "/work space/app", reason: "run the tests", grantRoot: null, host: null, protocol: null };
const pendingOne = [{ approvalId: "approval_test", kind: "command" }];
const acceptButton = (html: string, dictionary: typeof ko) => {
  const label = renderToStaticMarkup(dictionary["m5.approval.acceptOnce"]);
  return [...html.matchAll(/<button[^>]*>([^<]*)<\/button>/g)].find(match => match[1] === label)?.[0] ?? null;
};

test("approval translations preserve warnings, raw detail and inspected acceptance controls", () => {
  for (const dictionary of [ko, en]) {
    const html = renderCard(dictionary, [pendingOne, false, false, true, new Map([["approval_test", command]]), new Set(["approval_test"]), new Set()]);
    for (const key of ["help", "warning", "queued", "cancel", "decline", "acceptWarning", "fullContent"] as const) {
      assert.ok(html.includes(renderToStaticMarkup(dictionary[`m5.approval.${key}`])), key);
    }
    // The first line says what is asked and why, before any click (ADE D13).
    const line = html.slice(html.indexOf("m5-approval-line"), html.indexOf("m5-approval-details"));
    assert.ok(line.includes(renderToStaticMarkup(dictionary["m5.approval.kind.command"])));
    assert.ok(line.includes(renderToStaticMarkup(dictionary["m5.approval.why"].replace("{reason}", "run the tests"))));
    assert.ok(acceptButton(html, dictionary) && !acceptButton(html, dictionary)!.includes("disabled"), "opened: allowing is possible");
  }
});

test("allowing needs the full request opened on the card: summary alone is not enough (review #1)", () => {
  for (const dictionary of [ko, en]) {
    // Read automatically but not opened: the button is there and locked.
    const closed = renderCard(dictionary, [pendingOne, false, false, false, new Map([["approval_test", command]]), new Set(), new Set()]);
    assert.ok(acceptButton(closed, dictionary)?.includes("disabled"), "unopened: allowing is locked");
    assert.ok(closed.includes(renderToStaticMarkup(dictionary["m5.approval.openFirst"])));
    // The full content is there exactly: every option, the place, inner spaces kept.
    const full = closed.slice(closed.indexOf("m5-approval-details"));
    assert.ok(full.includes(renderToStaticMarkup(LONG_COMMAND)), "the whole command, untrimmed");
    assert.ok(full.includes("--last-option=keep") && full.includes("--   --workspace  packages"));
    assert.ok(full.includes("/work space/app"), "where it runs");
    // Opened for another request (an earlier id) does not count for this one.
    const otherOpened = renderCard(dictionary, [pendingOne, false, false, false, new Map([["approval_test", command]]), new Set(["approval_other"]), new Set()]);
    assert.ok(acceptButton(otherOpened, dictionary)?.includes("disabled"));
    // Not read yet: no allow button at all.
    const unread = renderCard(dictionary, [pendingOne, false, false, false, new Map(), new Set(["approval_test"]), new Set()]);
    assert.equal(acceptButton(unread, dictionary), null);
    // Sending or an error locks it again.
    for (const [sending, error] of [[true, false], [false, true]]) {
      const locked = renderCard(dictionary, [pendingOne, error, sending, false, new Map([["approval_test", command]]), new Set(["approval_test"]), new Set()]);
      assert.ok(acceptButton(locked, dictionary)?.includes("disabled"));
    }
  }
  assert.equal(display.canAcceptApproval({ approvalId: "a", inspectedId: "a", openedIds: new Set(["a"]), sending: false, error: false, fullyRead: true }), true);
  assert.equal(display.canAcceptApproval({ approvalId: "a", inspectedId: "a", openedIds: new Set(), sending: false, error: false, fullyRead: true }), false);
  assert.equal(display.canAcceptApproval({ approvalId: "b", inspectedId: "a", openedIds: new Set(["a", "b"]), sending: false, error: false, fullyRead: true }), false);
  assert.equal(display.canAcceptApproval({ approvalId: "a", inspectedId: null, openedIds: new Set(["a"]), sending: false, error: false, fullyRead: true }), false);
  // PR #19 review B1: a large input read in pages is never allowed before
  // every page was read and checked, even with its details open.
  assert.equal(display.canAcceptApproval({ approvalId: "a", inspectedId: "a", openedIds: new Set(["a"]), sending: false, error: false, fullyRead: false }), false);
});

test("every waiting request shows its own summary, and an error keeps the list (review #7, #8)", () => {
  const second = { ...command, approvalId: "approval_two", command: "rm -rf build", reason: "clean" };
  const html = renderCard(ko, [[...pendingOne, { approvalId: "approval_two", kind: "command" }], false, false, false, new Map([["approval_test", command], ["approval_two", second]]), new Set(), new Set()]);
  assert.equal((html.match(/m5-approval-reading/g) ?? []).length, 0, "nothing shows as loading once read");
  assert.ok(html.includes("rm -rf build") && html.includes("npm run build"));
  // After a failed list the last answered list stays (nothing can be answered meanwhile).
  const failed = renderCard(ko, [pendingOne, true, false, false, new Map([["approval_test", command]]), new Set(["approval_test"]), new Set()]);
  assert.ok(failed.includes("data-approval-id=\"approval_test\""));
  assert.ok(failed.includes(renderToStaticMarkup(ko["m5.approval.error"])));
});

test("a request the agent took back is said briefly, by the conversation (it outlives the card)", () => {
  for (const dictionary of [ko, en]) {
    const context: any = { exports: {}, require(name: string) {
      if (name === "react") return { useEffect() {}, useRef: (value: unknown) => ({ current: value }), useState: (initial: unknown) => [initial, () => {}] };
      if (name === "../i18n.tsx") return { useI18n: () => ({ t(key: keyof typeof ko) { return dictionary[key]; } }) };
      if (name === "./contracts/approval-v1.ts") return contract;
      if (name === "./approval-display.ts") return display;
      return require(name);
    } };
    runInNewContext(compiled, context);
    const html = renderToStaticMarkup(context.exports.ApprovalWithdrawnNote({ item: { chatId: "chat_1", approvalId: "approval_gone", kind: "tool", summary: "npm test" } }));
    assert.ok(html.includes(renderToStaticMarkup(dictionary["m5.approval.withdrawn"])));
    assert.ok(html.includes("npm test"));
    assert.ok(!html.includes("<button"), "nothing to answer once the request is gone");
  }
  // The card hands notices to the conversation and keeps none itself; only
  // answers the engine confirmed count as answered (review #8).
  const source = readFileSync(new URL("../src/m5/approval-card.tsx", import.meta.url), "utf8");
  assert.ok(!source.includes("setWithdrawn"), "the card no longer keeps the notice");
  const decide = source.slice(source.indexOf("async function decide("), source.indexOf("async function inspect("));
  assert.ok(decide.indexOf("decided.current.add(approvalId)") > decide.indexOf("await window.m5Approval.request(request)"), "recorded only after the engine answered");
  assert.ok(decide.includes("deciding.current.delete(approvalId)"));
  assert.deepEqual(display.withdrawnRequests([{ approvalId: "a" }, { approvalId: "b" }], [], new Set(["a"])).map(item => item.approvalId), ["b"]);
});

test("a request that cannot be read says so instead of loading forever", () => {
  const html = renderCard(ko, [[{ approvalId: "approval_x", kind: "tool" }], false, false, false, new Map(), new Set(), new Set(["approval_x"])]);
  assert.ok(html.includes(renderToStaticMarkup(ko["m5.approval.unreadable"])));
  assert.ok(!html.includes(ko["m5.approval.reading"]));
  assert.ok(!html.includes(renderToStaticMarkup(ko["m5.approval.acceptOnce"])), "nothing to accept without its content");
});

test("withdrawn requests are those that left without an answer, and the headline reads the request", () => {
  const before = [{approvalId:"a",kind:"command"},{approvalId:"b",kind:"tool"},{approvalId:"c",kind:"tool"}];
  assert.deepEqual(display.withdrawnRequests(before, [{approvalId:"c",kind:"tool"}], new Set(["a"])).map(item => item.approvalId), ["b"]);
  assert.deepEqual(display.withdrawnRequests(before, before, new Set()), []);
  const tool = {approvalId:"t",kind:"tool" as const,command:null,cwd:"/w",reason:"Bash\u202e tool",grantRoot:null,host:null,protocol:null,toolInput:JSON.stringify({command:"npm run build",description:"Build"})};
  // The full fields keep every character (only invisible ones escaped), unlike the summary.
  const fields = display.approvalFields({ ...tool, command: "a  b\tc" });
  assert.deepEqual(fields.map(item => item.field), ["command", "toolInput", "cwd", "reason"]);
  assert.equal(fields[0]!.text, "a  b\\u0009c");
  assert.deepEqual(display.approvalHeadline(tool), { target: "npm run build", reason: "Bash\\u202e tool" });
  const file = {...tool, toolInput: JSON.stringify({file_path:"src/app.ts"}), reason: null};
  assert.deepEqual(display.approvalHeadline(file), { target: "src/app.ts", reason: null });
  const network = {approvalId:"n",kind:"network" as const,command:null,cwd:null,reason:null,grantRoot:null,host:"registry.npmjs.org",protocol:"https"};
  assert.equal(display.approvalHeadline(network).target, "https://registry.npmjs.org");
  assert.ok(Array.from(display.approvalHeadline({...network, host: "x".repeat(500)}).target).length <= 240);
});
