import assert from "node:assert/strict";
import test from "node:test";
import { validateLoginRequest, validateLoginStatus } from "../src/m5/contracts/provider-login-v1.ts";
test("login requests are fixed and attempts cannot carry paths or commands",()=>{
  assert.deepEqual(validateLoginRequest({action:"start",attemptId:"abc-123"}),{action:"start",attemptId:"abc-123"});
  for (const value of [{action:"status",extra:1},{action:"start",attemptId:"../foo"},{action:"exec",attemptId:"a"},{action:"cancel"}]) assert.throws(()=>validateLoginRequest(value));
});
test("only transient HTTPS OAuth URLs from fixed official hosts can reach browser",()=>{
  const status={state:"waiting",attemptId:"a",authUrl:"https://auth.openai.com/authorize?state=private",error:null};
  assert.equal(validateLoginStatus(status).state,"waiting");
  for (const url of ["https://auth.openai.com.evil.test/","http://auth.openai.com/","https://user@auth.openai.com/","https://auth.openai.com:444/","file:///tmp/a","https://auth.openai.com\\@evil.test"]) assert.throws(()=>validateLoginStatus({...status,authUrl:url}));
  assert.throws(()=>validateLoginStatus({...status,state:"connected"}));
});
