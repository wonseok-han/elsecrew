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
test("a large Claude tool input arrives in part, with its full size and SHA-256 (2026-11-26 big Write)", () => {
  const request = validateApprovalRequest({action:"inspect",identity,approvalId});
  const sha = "a".repeat(64);
  const preview = JSON.stringify({file_path:"big.txt",content:"x".repeat(2048) + "…[298000 more bytes]"});
  const inspection = {approvalId,kind:"tool",command:null,cwd:null,reason:"Write",grantRoot:null,host:null,protocol:null,toolInput:preview,toolInputBytes:300_100,toolInputSha256:sha};
  const response = {identity,pending:[{approvalId,kind:"tool"}],queued:null,inspection};
  const checked = validateApprovalResult(response,request).inspection;
  assert.equal(checked?.toolInputBytes, 300_100);
  assert.equal(checked?.toolInputSha256, sha);
  // Idempotent: the same result passes again in main, preload and the screen.
  assert.deepEqual(validateApprovalResult(validateApprovalResult(response,request),request).inspection, checked);
  for (const changed of [
    {toolInputBytes:undefined}, {toolInputSha256:undefined}, {toolInputSha256:"A".repeat(64)}, {toolInputSha256:"a".repeat(63)},
    {toolInputBytes:10}, {toolInputBytes:4 * 1024 * 1024 + 1}, {toolInputBytes:1.5}, {toolInputBytes:"300100"},
  ]) {
    const broken = {...inspection, ...changed} as Record<string, unknown>;
    for (const [key, value] of Object.entries(changed)) if (value === undefined) delete broken[key];
    assert.throws(()=>validateApprovalResult({...response,inspection:broken},request), JSON.stringify(changed));
  }
  // Only a tool request may carry them.
  const command = {approvalId,kind:"command",command:"ls",cwd:"/w",reason:null,grantRoot:null,host:null,protocol:null,toolInputBytes:300_100,toolInputSha256:sha};
  assert.throws(()=>validateApprovalResult({identity,pending:[{approvalId,kind:"command"}],queued:null,inspection:command},request));
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
