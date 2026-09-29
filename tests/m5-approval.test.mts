import assert from "node:assert/strict";
import test from "node:test";
import { validateApprovalRequest, validateApprovalResult } from "../src/m5/contracts/approval-v1.ts";

const identity = { chatId:"chat_01",taskId:"task_01",projectId:"project_01",worktreeId:"worktree_01",workspaceIdentity:"workspace_01",runId:"agent_run_01" };
const approvalId = `approval_${"a".repeat(32)}`;
test("approval contract only admits exact list/decline and binds the receipt", () => {
  const request = validateApprovalRequest({action:"decline",identity,approvalId});
  assert.equal(validateApprovalResult({identity,pending:[],queued:approvalId},request).queued,approvalId);
  for (const value of [{action:"accept",identity,approvalId},{action:"decline",identity,approvalId,force:true},{action:"list",identity:{...identity,command:"rm"}},{action:"decline",identity,approvalId:"../x"}]) assert.throws(()=>validateApprovalRequest(value));
  for (const value of [{identity:{...identity,runId:"run_other"},pending:[],queued:approvalId},{identity,pending:[approvalId],queued:approvalId},{identity,pending:[],queued:null}]) assert.throws(()=>validateApprovalResult(value,request));
  const list = validateApprovalRequest({action:"list",identity});
  assert.throws(()=>validateApprovalResult({identity,pending:[approvalId,approvalId],queued:null},list));
  assert.throws(()=>validateApprovalResult({identity,pending:[],queued:approvalId},list));
});
