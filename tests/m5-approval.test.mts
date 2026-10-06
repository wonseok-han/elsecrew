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
});
