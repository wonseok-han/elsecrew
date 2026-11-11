import test from 'node:test';
import assert from 'node:assert/strict';
import {validCredentialFile,parseClaudeQuota,queryClaudeQuota,CLAUDE_USAGE_URL} from '../electron/claude-usage.ts';
import {validateClaudeUsageResult} from '../src/m5/contracts/provider-usage-v1.ts';
test('quota projects only bounded display windows and accepts fractional ISO reset',()=>{
 const usage=parseClaudeQuota({five_hour:{utilization:12.5,resets_at:'2026-11-11T01:00:00.123Z'},seven_day:null,account:'private'});
 assert.deepEqual(usage,{windows:[{id:'primary',usedPercent:12.5,windowMinutes:300,resetsAt:1794358800}]});
 for(const utilization of [-1,101,NaN,'42']) assert.throws(()=>parseClaudeQuota({five_hour:{utilization}}));
 assert.throws(()=>parseClaudeQuota({five_hour:{utilization:2,resets_at:'bad'}}));
});
test('usage GET has fixed host, no cookies/redirects and only normalized output',async()=>{
 const result=await queryClaudeQuota('synthetic-test-token',async (url,init)=>{
  assert.equal(url,CLAUDE_USAGE_URL); assert.equal(init?.method,'GET'); assert.equal(init?.redirect,'error'); assert.equal(init?.credentials,'omit');
  assert.equal(new Headers(init?.headers).get('Authorization'),'Bearer synthetic-test-token');
  return Response.json({seven_day:{utilization:44,resets_at:null},secret:'do not expose'});
 },123);
 assert.equal(result.state,'available');assert.equal(result.usage?.windows[0].usedPercent,44);
 assert.equal(JSON.stringify(result).includes('secret'),false);validateClaudeUsageResult(result);
});
test('errors and large responses never leak upstream content',async()=>{
 for(const [status,state] of [[401,'expired'],[429,'retryLater'],[500,'unavailable']] as const){
  assert.deepEqual(await queryClaudeQuota('synthetic',async()=>new Response('secret',{status}),123),{state,usage:null,checkedAt:123});
 }
 for(const fetcher of [async()=>new Response('x'.repeat(65537)),async()=>Response.json({five_hour:{utilization:101}}),async()=>{throw Error('secret');}]){
  assert.deepEqual(await queryClaudeQuota('synthetic',fetcher,123),{state:'unavailable',usage:null,checkedAt:123});
 }
 assert.throws(()=>validateClaudeUsageResult({state:'available',usage:null,checkedAt:123}));
 assert.throws(()=>validateClaudeUsageResult({state:'unavailable',usage:null,checkedAt:123,token:'secret'}));
});

test('credential source rejects hard links and non-private files',()=>{
 const stat={isFile:()=>true,uid:501,mode:0o600,size:512,nlink:1};
 assert.equal(validCredentialFile(stat,501),true);
 assert.equal(validCredentialFile({...stat,nlink:2},501),false);
 assert.equal(validCredentialFile({...stat,mode:0o644},501),false);
 assert.equal(validCredentialFile(stat,502),false);
});
