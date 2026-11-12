import assert from "node:assert/strict";
import test from "node:test";
import { validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";
test("project permission requires explicit complete consent and matching receipt", () => {
 const request = {action:"setAutomationPermissions" as const, projectId:"project_a", permissions:{files:true,commands:false},confirmed:true as const};
 assert.deepEqual(validateProjectCatalogRequest(request),request);
 for (const invalid of [{...request,confirmed:false},{...request,permissions:{files:true}},{...request,permissions:{files:true,commands:true,network:true}}]) assert.throws(()=>validateProjectCatalogRequest(invalid));
 const result={schemaVersion:1,action:request.action,projectId:request.projectId,permissions:request.permissions};
 assert.deepEqual(validateProjectCatalogResult(result,request),result);
 assert.throws(()=>validateProjectCatalogResult({...result,projectId:"project_b"},request));
 assert.throws(()=>validateProjectCatalogResult({...result,permissions:{files:true,commands:true}},request));
});
