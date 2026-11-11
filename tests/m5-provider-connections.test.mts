import test from 'node:test';
import assert from 'node:assert/strict';
import {validateProjectCatalogRequest as request,validateProjectCatalogResult as result} from '../src/m5/contracts/project-catalog-v1.ts';
test('connection choices are explicit booleans for the two providers',()=>{
 for(const runtime of ['codex','claude-code'] as const){
  const input={action:'setProviderConnection',runtime,connected:false} as const;
  assert.deepEqual(request(input),input);
  assert.doesNotThrow(()=>result({schemaVersion:1,action:input.action,connections:{codex:false,'claude-code':false}},input));
  assert.throws(()=>result({schemaVersion:1,action:input.action,connections:{codex:true,'claude-code':true}},input));
 }
 for(const input of [{action:'setProviderConnection',runtime:'other',connected:true},{action:'setProviderConnection',runtime:'codex',connected:'yes'},{action:'getProviderConnections',path:'/other'}]) assert.throws(()=>request(input));
});
