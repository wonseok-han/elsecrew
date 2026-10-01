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
