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
