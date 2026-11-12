import test from "node:test";
import assert from "node:assert/strict";
import { orchestratorMessage, orchestratorDelegation } from "../src/m5/orchestrator-message.ts";

test("ordinary chat shows the orchestrator answer and delegation summary", () => {
  assert.equal(orchestratorMessage('{"action":"final","answer":"프로젝트 구조를 설명할게요."}', "coordinator"), "프로젝트 구조를 설명할게요.");
  assert.equal(orchestratorMessage('{"action":"delegate","role":"coding","instruction":"Implement the feature","summary":"개발 담당과 작업하겠습니다."}', "coordinator"), "개발 담당과 작업하겠습니다.");
});
test("presentation never treats user or worker JSON as an orchestration instruction", () => {
  const text = '{"action":"final","answer":"hello"}';
  assert.equal(orchestratorMessage(text, null), text);
  assert.equal(orchestratorMessage(text, "coding"), text);
  for (const invalid of ['{"action":"final","answer":"hello","extra":true}', '{"action":"delegate","role":"shell","instruction":"x","summary":"y"}', "ordinary response"]) {
    assert.equal(orchestratorMessage(invalid, "coordinator"), invalid);
  }
});

test("team cancellation binds the visible user task and distinguishes stopping from stopped", async () => {
  const { validateProjectCatalogRequest, validateProjectCatalogResult } = await import("../src/m5/contracts/project-catalog-v1.ts");
  const request = {action:"cancelTeamTask",chatId:"c",taskId:"t",projectId:"p",worktreeId:"w",workspaceIdentity:"i",triggerMessageId:"m"} as const;
  assert.deepEqual(validateProjectCatalogRequest(request), request);
  assert.throws(() => validateProjectCatalogRequest({...request,runId:"foreign"}));
  const result = {schemaVersion:1,action:"cancelTeamTask",chatId:"c",triggerMessageId:"m",status:"stopping"};
  assert.deepEqual(validateProjectCatalogResult(result,request),result);
  assert.throws(() => validateProjectCatalogResult({...result,triggerMessageId:"other"},request));
  assert.throws(() => validateProjectCatalogResult({...result,status:"terminated"},request));
});

test("delegation details expose the exact assigned task only from coordinator messages", () => {
  const text = JSON.stringify({action:"delegate", role:"design", instruction:"Review layout\nReport tradeoffs", summary:"Design review"});
  assert.deepEqual(orchestratorDelegation(text, "coordinator"), {role:"design", instruction:"Review layout\nReport tradeoffs"});
  for (const role of [null, "user", "coding"]) assert.equal(orchestratorDelegation(text, role), null);
  for (const text of ['{"action":"delegate"}', '{"action":"final","answer":"Done"}', '{"action":"delegate","role":"shell","instruction":"exec","summary":"x"}']) assert.equal(orchestratorDelegation(text, "coordinator"), null);
});

test("V3 delegation presentation accepts only the chat's own configured roles", () => {
  const text = JSON.stringify({action:"delegate", role:"research", instruction:"Compare libraries", summary:"리서치 팀원에게 맡길게요."});
  const team = ["coordinator", "research", "builder"];
  assert.equal(orchestratorMessage(text, "coordinator", team), "리서치 팀원에게 맡길게요.");
  assert.deepEqual(orchestratorDelegation(text, "coordinator", team), {role:"research", instruction:"Compare libraries"});
  // Legacy chats keep the fixed template; a custom ID outside the team stays raw text.
  assert.equal(orchestratorMessage(text, "coordinator"), text);
  assert.equal(orchestratorDelegation(text, "coordinator", ["coordinator", "builder"]), null);
  assert.equal(orchestratorDelegation(JSON.stringify({action:"delegate", role:"coordinator", instruction:"x", summary:"y"}), "coordinator", team), null);
});
