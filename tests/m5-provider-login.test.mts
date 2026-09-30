import assert from "node:assert/strict";
import test from "node:test";
test("explicit recovery uses fixed provider and attempt without filesystem parameters",()=>{
  for (const provider of ["codex","claude"] as const) {
    assert.deepEqual(validateLoginRequest({action:"recover",attemptId:"repair-1",provider}),{action:"recover",attemptId:"repair-1",provider});
  }
  for (const input of [{action:"recover"},{action:"recover",attemptId:"../x"},{action:"recover",attemptId:"a",path:"/tmp/fence"},{action:"recover",attemptId:"a",force:true}]) assert.throws(()=>validateLoginRequest(input));
});
import { loginErrorMessage } from "../src/m5/provider-login-message.ts";
test("English login guidance preserves credential and recovery cautions", () => {
  assert.match(loginErrorMessage("codex", "providerExecutableChanged", "en"), /does not mean you are signed out/);
  assert.match(loginErrorMessage("codex", "providerExecutableMissing", "en"), /credentials have not been deleted/);
  assert.match(loginErrorMessage("codex", "providerProfileUnavailable", "en"), /Sign-out has not been confirmed/);
  assert.match(loginErrorMessage("claude", "providerLoginRecoveryRequired", "en"), /safe shutdown are blocked/);
  assert.match(loginErrorMessage("claude", "providerLoginFailed", "en"), /cancel it instead of resetting/);
  assert.match(loginErrorMessage("codex", null, "en"), /Do not delete credentials/);
});
test("login feedback distinguishes executable drift from authentication and recovery",()=>{
  for (const provider of ["codex", "claude"] as const) {
    const ko = loginErrorMessage(provider, "providerExecutableChanged");
    const en = loginErrorMessage(provider, "providerExecutableChanged", "en");
    assert.match(ko, /제작자 서명이나 파일 상태/);
    assert.match(ko, /완료 후 다시 확인/);
    assert.doesNotMatch(ko, /호환성 업데이트가 필요/);
    assert.match(en, /publisher signature or file state/);
    assert.match(en, /finish and check again/);
    assert.doesNotMatch(en, /compatibility update is needed/);
  }
  assert.match(loginErrorMessage("codex","providerExecutableChanged"), /로그아웃된 것은 아니/);
  assert.match(loginErrorMessage("codex","providerExecutableMissing"), /실행 파일을 찾지/);
  assert.match(loginErrorMessage("codex","providerProfileUnavailable"), /아직 확인하지/);
  assert.match(loginErrorMessage("claude","providerLoginFailed"), /재설정하지 말고/);
  assert.match(loginErrorMessage("claude","providerLoginRecoveryRequired"), /안전 종료를 차단/);
});
import { validateLoginRequest, validateLoginStatus } from "../src/m5/contracts/provider-login-v1.ts";
test("installation diagnosis is fixed, model-free and separate from authentication", () => {
  for (const provider of ["codex", "claude"] as const) {
    assert.deepEqual(validateLoginRequest({action:"diagnose",provider,attemptId:"diagnosis-1"}), {action:"diagnose",provider,attemptId:"diagnosis-1"});
    for (const state of ["diagnosing", "installed"]) {
      const status = {state,attemptId:"diagnosis-1",authUrl:null,error:null};
      assert.equal(validateLoginStatus(status,provider).state,state);
      assert.throws(() => validateLoginStatus({...status,authUrl:"https://auth.openai.com/authorize"},provider));
    }
  }
  for (const extra of [{path:"/tmp/tool"},{command:"install"},{force:true}]) {
    assert.throws(() => validateLoginRequest({action:"diagnose",attemptId:"a",...extra}));
  }
});
test("connection code submission is Claude-only, bounded and never accepts commands or URLs",()=>{
  const request={action:"submitCode",provider:"claude",attemptId:"a",code:"synthetic#state"};
  assert.deepEqual(validateLoginRequest(request),request);
  for(const code of ["","a\nb","https://platform.claude.com/","a".repeat(4097)]) assert.throws(()=>validateLoginRequest({...request,code}));
  assert.throws(()=>validateLoginRequest({...request,provider:"codex"}));
  assert.throws(()=>validateLoginRequest({...request,extra:true}));
  assert.equal(validateLoginStatus({state:"verifying",attemptId:"a",authUrl:null,error:null},"claude").state,"verifying");
});
test("login requests are fixed and attempts cannot carry paths or commands",()=>{
  assert.deepEqual(validateLoginRequest({action:"start",attemptId:"abc-123"}),{action:"start",attemptId:"abc-123"});
  assert.deepEqual(validateLoginRequest({action:"check",provider:"claude",attemptId:"abc-123"}),{action:"check",provider:"claude",attemptId:"abc-123"});
  for (const provider of [null,undefined,"unknown","../codex"]) assert.throws(()=>validateLoginRequest({action:"status",provider}));
  for (const value of [{action:"status",extra:1},{action:"start",attemptId:"../foo"},{action:"exec",attemptId:"a"},{action:"cancel"}]) assert.throws(()=>validateLoginRequest(value));
});
test("Claude OAuth host cannot be substituted into a Codex response or vice versa",()=>{
  const status={state:"waiting",attemptId:"a",authUrl:"https://claude.ai/oauth/authorize?state=private",error:null};
  assert.equal(validateLoginStatus(status,"claude").state,"waiting");
  assert.equal(validateLoginStatus({...status,authUrl:"https://claude.com/cai/oauth/authorize?state=private"},"claude").state,"waiting");
  assert.throws(()=>validateLoginStatus(status,"codex"));
  for (const authUrl of ["https://claude.ai/other","https://auth.openai.com/authorize","https://claude.ai.evil.test/oauth/authorize","https://claude.ai:444/oauth/authorize"]) assert.throws(()=>validateLoginStatus({...status,authUrl},"claude"));
});
test("only transient HTTPS OAuth URLs from fixed official hosts can reach browser",()=>{
  const status={state:"waiting",attemptId:"a",authUrl:"https://auth.openai.com/authorize?state=private",error:null};
  assert.equal(validateLoginStatus(status).state,"waiting");
  for (const url of ["https://auth.openai.com.evil.test/","http://auth.openai.com/","https://user@auth.openai.com/","https://auth.openai.com:444/","file:///tmp/a","https://auth.openai.com\\@evil.test"]) assert.throws(()=>validateLoginStatus({...status,authUrl:url}));
  assert.throws(()=>validateLoginStatus({...status,state:"connected"}));
});
