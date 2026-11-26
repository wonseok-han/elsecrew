import assert from "node:assert/strict";
import test from "node:test";
import { validateApprovalRequest, validateApprovalResult, visibleApprovalText } from "../src/m5/contracts/approval-v1.ts";

const identity = { chatId:"chat_01",taskId:"task_01",projectId:"project_01",worktreeId:"worktree_01",workspaceIdentity:"workspace_01",runId:"agent_run_01" };
const approvalId = `approval_${"a".repeat(32)}`;
test("inspection is explicit, scoped, bounded and not a list payload", () => {
  const request = validateApprovalRequest({action:"inspect",identity,approvalId});
  const inspection = {approvalId,kind:"command",command:"echo PRIVATE",cwd:"/snapshot",reason:null,grantRoot:null,host:null,protocol:null};
  const response = {identity,pending:[{approvalId,kind:"command"}],queued:null,inspection};
  assert.equal(validateApprovalResult(response,request).inspection?.command,"echo PRIVATE");
  assert.throws(()=>validateApprovalResult(response,validateApprovalRequest({action:"list",identity})));
  for (const changed of [{...inspection,approvalId:`approval_${"b".repeat(32)}`},{...inspection,kind:"network"},{...inspection,command:"x".repeat(65537)},{...inspection,env:"SECRET"},{...inspection,grantRoot:"/other"}]) assert.throws(()=>validateApprovalResult({...response,inspection:changed},request));
  assert.throws(()=>validateApprovalResult({...response,pending:[]},request));
  assert.equal(visibleApprovalText("echo\n\u202ePRIVATE"),"echo\\u000a\\u202ePRIVATE");
  for (const point of [0x61c,0x200e,0x200f,0x202a,0x202b,0x202c,0x202d,0x202e,0x2066,0x2067,0x2068,0x2069,0x200d,0xfeff]) assert.equal(visibleApprovalText(String.fromCodePoint(point)), `\\u${point.toString(16).padStart(4,"0")}`);
  assert.equal(visibleApprovalText(String.fromCodePoint(0xe0001)), "\\u{e0001}");
});
test("viewing a request that is no longer waiting shows nothing and answers nothing", () => {
  const request = validateApprovalRequest({action:"inspect",identity,approvalId});
  const other = `approval_${"b".repeat(32)}`;
  const gone = validateApprovalResult({identity,pending:[{approvalId:other,kind:"tool"}],queued:null,inspection:null},request);
  assert.deepEqual(gone,{identity,pending:[{approvalId:other,kind:"tool"}],queued:null,inspection:null});
  // Sidecar client, main, preload and the screen each check the result again.
  assert.deepEqual(validateApprovalResult(validateApprovalResult(validateApprovalResult(gone,request),request),request),gone);
  // Still waiting means its details must be there; a receipt is never part of a view.
  assert.throws(()=>validateApprovalResult({identity,pending:[{approvalId,kind:"tool"}],queued:null,inspection:null},request));
  assert.throws(()=>validateApprovalResult({identity,pending:[],queued:approvalId,inspection:null},request));
  assert.throws(()=>validateApprovalResult({identity,pending:[],queued:null},request));
  // A list or a decision never carries an inspection, not even null.
  assert.throws(()=>validateApprovalResult({identity,pending:[],queued:null,inspection:null},validateApprovalRequest({action:"list",identity})));
});
test("every approval result passes its own check again unchanged", () => {
  const inspection = {approvalId,kind:"command",command:"echo",cwd:"/w",reason:null,grantRoot:null,host:null,protocol:null};
  const cases: [Parameters<typeof validateApprovalRequest>[0], unknown][] = [
    [{action:"list",identity},{identity,pending:[{approvalId,kind:"command"}],queued:null}],
    [{action:"inspect",identity,approvalId},{identity,pending:[{approvalId,kind:"command"}],queued:null,inspection}],
    [{action:"inspect",identity,approvalId},{identity,pending:[],queued:null,inspection:null}],
    [{action:"decline",identity,approvalId},{identity,pending:[],queued:approvalId}],
    [{action:"acceptOnce",identity,approvalId},{identity,pending:[],queued:approvalId}],
  ];
  for (const [input, response] of cases) {
    const request = validateApprovalRequest(input);
    const once = validateApprovalResult(response,request);
    assert.deepEqual(validateApprovalResult(validateApprovalResult(once,request),request),once);
  }
});
test("approval summaries admit only fixed types and never raw command fields", () => {
  const request = validateApprovalRequest({action:"list",identity});
  for (const kind of ["command","writeStdin","network","fileChange","tool"]) {
    const summary = {approvalId,kind};
    assert.deepEqual(validateApprovalResult({identity,pending:[summary],queued:null},request).pending,[summary]);
    for (const changed of [{...summary,command:"PRIVATE"},{...summary,kind:"unknown"},{...summary,kind:"toString"}]) assert.throws(()=>validateApprovalResult({identity,pending:[changed],queued:null},request));
    assert.throws(()=>validateApprovalResult({identity,pending:[summary,summary],queued:null},request));
  }
});
test("Claude tool input is bounded and only available in explicit inspection", () => {
  const request = validateApprovalRequest({action:"inspect",identity,approvalId});
  const inspection = {approvalId,kind:"tool",command:null,cwd:null,reason:"Edit",grantRoot:null,host:null,protocol:null,toolInput:'{"file_path":"PRIVATE","new_string":"<tag>"}'};
  const response = {identity,pending:[{approvalId,kind:"tool"}],queued:null,inspection};
  assert.equal(validateApprovalResult(response,request).inspection?.toolInput,inspection.toolInput);
  for (const toolInput of [null, undefined, {}, "x".repeat(128 * 1024 + 1)]) {
    assert.throws(()=>validateApprovalResult({...response,inspection:{...inspection,toolInput}},request));
  }
  assert.throws(()=>validateApprovalResult({...response,pending:[{approvalId,kind:"tool",toolInput:inspection.toolInput}]},request));
});
test("a large Claude tool input arrives in pages, each naming its full size, SHA-256 and place (PR #19 review B1)", () => {
  const sha = "a".repeat(64);
  const pageOf = (page: number) => ({approvalId,kind:"tool",command:null,cwd:null,reason:"Write",grantRoot:null,host:null,protocol:null,
    toolInput:`page ${page}`,toolInputBytes:300_100,toolInputSha256:sha,toolInputPage:page,toolInputPages:5});
  for (const page of [0, 3]) {
    const request = page === 0 ? validateApprovalRequest({action:"inspect",identity,approvalId}) : validateApprovalRequest({action:"inspect",identity,approvalId,page});
    const response = {identity,pending:[{approvalId,kind:"tool"}],queued:null,inspection:pageOf(page)};
    const checked = validateApprovalResult(response,request).inspection;
    assert.equal(checked?.toolInputPage, page);
    // Idempotent: the same result passes again in main, preload and the screen.
    assert.deepEqual(validateApprovalResult(validateApprovalResult(response,request),request).inspection, checked);
  }
  const request = validateApprovalRequest({action:"inspect",identity,approvalId,page:2});
  const response = {identity,pending:[{approvalId,kind:"tool"}],queued:null,inspection:pageOf(2)};
  for (const changed of [
    {toolInputPage:1}, {toolInputPage:5}, {toolInputPages:1}, {toolInputPages:4097}, {toolInputBytes:10}, {toolInputBytes:4 * 1024 * 1024 + 1},
    {toolInputSha256:"A".repeat(64)}, {toolInputSha256:undefined}, {toolInputPages:undefined}, {toolInput:"x".repeat(64 * 1024 + 1)},
  ]) {
    const broken = {...pageOf(2), ...changed} as Record<string, unknown>;
    for (const [key, value] of Object.entries(changed)) if (value === undefined) delete broken[key];
    assert.throws(()=>validateApprovalResult({...response,inspection:broken},request), JSON.stringify(changed));
  }
  // A page request must get a paged answer; pages are only for tool requests.
  const whole = {approvalId,kind:"tool",command:null,cwd:null,reason:"Write",grantRoot:null,host:null,protocol:null,toolInput:"{}"};
  assert.throws(()=>validateApprovalResult({...response,inspection:whole},request));
  for (const page of [-1, 1.5, 4096, "1"]) assert.throws(()=>validateApprovalRequest({action:"inspect",identity,approvalId,page}));
  assert.throws(()=>validateApprovalRequest({action:"acceptOnce",identity,approvalId,page:0}));
  const command = {approvalId,kind:"command",command:"ls",cwd:"/w",reason:null,grantRoot:null,host:null,protocol:null,toolInputBytes:300_100,toolInputSha256:sha,toolInputPage:0,toolInputPages:5};
  assert.throws(()=>validateApprovalResult({identity,pending:[{approvalId,kind:"command"}],queued:null,inspection:command},validateApprovalRequest({action:"inspect",identity,approvalId})));
});
test("the card's full content is the pages joined, checked against the engine's size and SHA-256 (PR #19 review B1)", async () => {
  const { assembleToolInput, pagedTarget } = await import("../src/m5/approval-display.ts");
  const { createHash } = await import("node:crypto");
  const full = JSON.stringify({ file_path: "big.txt", content: "한글 줄\n".repeat(20_000) });
  const bytes = Buffer.byteLength(full);
  const sha = createHash("sha256").update(full).digest("hex");
  // Pages cut at character boundaries, as the engine does.
  const pages: string[] = [];
  for (let start = 0; start < full.length; start += 30_000) pages.push(full.slice(start, start + 30_000));
  const inspection = { toolInputBytes: bytes, toolInputSha256: sha, toolInputPages: pages.length };
  assert.equal(await assembleToolInput(pages, inspection), full, "what is shown is exactly what is allowed");
  await assert.rejects(assembleToolInput(pages.slice(0, -1), inspection), /missing/);
  await assert.rejects(assembleToolInput([...pages.slice(0, -1), pages.at(-1)!.replace("줄", "줄")], { ...inspection, toolInputSha256: "b".repeat(64) }), /hash/);
  const swapped = [...pages]; [swapped[0], swapped[1]] = [swapped[1], swapped[0]];
  await assert.rejects(assembleToolInput(swapped, inspection), /hash/);
  await assert.rejects(assembleToolInput([...pages.slice(0, -1), pages.at(-1) + "x"], inspection), /size/);
  assert.equal(pagedTarget(pages[0]), "big.txt");
  assert.equal(pagedTarget('{"content":"x'), null);
});
test("approval contract only admits exact list/decline and binds the receipt", () => {
  const request = validateApprovalRequest({action:"decline",identity,approvalId});
  assert.equal(validateApprovalResult({identity,pending:[],queued:approvalId},request).queued,approvalId);
  for (const value of [{action:"accept",identity,approvalId},{action:"decline",identity,approvalId,force:true},{action:"list",identity:{...identity,command:"rm"}},{action:"decline",identity,approvalId:"../x"}]) assert.throws(()=>validateApprovalRequest(value));
  for (const value of [{identity:{...identity,runId:"run_other"},pending:[],queued:approvalId},{identity,pending:[approvalId],queued:approvalId},{identity,pending:[],queued:null}]) assert.throws(()=>validateApprovalResult(value,request));
  const list = validateApprovalRequest({action:"list",identity});
  assert.throws(()=>validateApprovalResult({identity,pending:[approvalId,approvalId],queued:null},list));
  assert.throws(()=>validateApprovalResult({identity,pending:[],queued:approvalId},list));
});
test("one-time acceptance has exact identity and a consumed-request receipt", () => {
  const request = validateApprovalRequest({action:"acceptOnce",identity,approvalId});
  assert.equal(validateApprovalResult({identity,pending:[],queued:approvalId},request).queued,approvalId);
  assert.throws(()=>validateApprovalRequest({action:"acceptOnce",identity,approvalId,always:true}));
  assert.throws(()=>validateApprovalResult({identity,pending:[],queued:null},request));
  assert.throws(()=>validateApprovalResult({identity,pending:[{approvalId,kind:"command"}],queued:approvalId},request));
  assert.throws(()=>validateApprovalResult({identity,pending:[{approvalId,kind:"tool"}],queued:approvalId},request));
  assert.throws(()=>validateApprovalRequest({action:"acceptOnce",identity,approvalId,toolInput:"replacement"}));
});
