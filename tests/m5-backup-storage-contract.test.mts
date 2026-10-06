import assert from "node:assert/strict";
import test from "node:test";
import { validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";

test("backup storage is app-wide with a consistent bounded byte budget", () => {
  const request = { action: "backupStorageUsage" } as const;
  const limit = 5 * 1024 ** 3;
  assert.deepEqual(validateProjectCatalogRequest(request), request);
  assert.throws(() => validateProjectCatalogRequest({ ...request, projectId: "p" }));
  const response = (usedBytes: number) => ({ schemaVersion: 1, ...request, usage: { usedBytes, limitBytes: limit, remainingBytes: Math.max(0, limit - usedBytes) } });
  for (const used of [0, 1024, limit, limit + 1]) assert.doesNotThrow(() => validateProjectCatalogResult(response(used), request));
  for (const used of [-1, 0.5, Number.MAX_SAFE_INTEGER + 1, NaN]) assert.throws(() => validateProjectCatalogResult(response(used), request));
  for (const patch of [{ remainingBytes: 1 }, { limitBytes: limit + 1 }, { unexpected: true }]) {
    const invalid = response(limit);
    Object.assign(invalid.usage, patch);
    assert.throws(() => validateProjectCatalogResult(invalid, request));
  }
});

test("budget changes require the inspected budget and whole GiB values", () => {
  const request = {action:"setBackupStorageLimit" as const,expectedLimitBytes:5*1024**3,limitBytes:10*1024**3};
  assert.deepEqual(validateProjectCatalogRequest(request), request);
  for (const value of [0, -1, 1024**3+1, 1025*1024**3, Number.MAX_SAFE_INTEGER]) {
    assert.throws(() => validateProjectCatalogRequest({...request,limitBytes:value}));
    assert.throws(() => validateProjectCatalogRequest({...request,expectedLimitBytes:value}));
  }
  assert.doesNotThrow(() => validateProjectCatalogResult({schemaVersion:1,action:request.action,limitBytes:request.limitBytes},request));
  assert.throws(() => validateProjectCatalogResult({schemaVersion:1,action:request.action,limitBytes:request.expectedLimitBytes},request));
});

test("retention replies cannot substitute another project or duplicate records", () => {
  const request = {action:"backupRetention" as const,projectId:"project_a"};
  const entry = {reviewId:"review_a",state:"abandoned",retainedFor:"applicationHistory"};
  const response = {schemaVersion:1,...request,entries:[entry]};
  assert.doesNotThrow(() => validateProjectCatalogResult(response, request));
  assert.throws(() => validateProjectCatalogResult({...response,projectId:"project_b"}, request));
  assert.throws(() => validateProjectCatalogResult({...response,entries:[entry,entry]}, request));
  assert.throws(() => validateProjectCatalogResult({...response,entries:[{...entry,retainedFor:"safeToDelete"}]}, request));
});

test("cleanup inspection and command bind exact project, review and fingerprint", () => {
  const reviewId = `review_${"a".repeat(32)}`;
  const fingerprint = "b".repeat(64);
  const request = {action:"inspectBackupCleanup" as const,projectId:"project_a",reviewId};
  const inspection = {reviewId,fingerprint,filePresent:true,state:"available"};
  const result = {schemaVersion:1,...request,enabled:false,inspection};
  // reviewId belongs inside inspection, not the response envelope.
  const {reviewId: omitted, ...response} = result;
  assert.equal(omitted, reviewId);
  assert.doesNotThrow(() => validateProjectCatalogResult(response,request));
  for (const changed of [{...inspection,reviewId:`review_${"c".repeat(32)}`},{...inspection,fingerprint:"bad"},{...inspection,state:"deleted"}]) {
    assert.throws(() => validateProjectCatalogResult({...response,inspection:changed},request));
  }
  assert.throws(() => validateProjectCatalogResult({...response,projectId:"project_b"},request));
  const command = {action:"cleanupBackup" as const,projectId:"project_a",reviewId,fingerprint};
  assert.doesNotThrow(() => validateProjectCatalogRequest(command));
  assert.throws(() => validateProjectCatalogRequest({...command,filename:"backup.bin"}));
  assert.throws(() => validateProjectCatalogResult({schemaVersion:1,action:command.action,projectId:"project_a",reviewId:`review_${"c".repeat(32)}`},command));
});
