import assert from "node:assert/strict";
import test from "node:test";
import { validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";
import { en } from "../src/locales/en.ts";
import { ko } from "../src/locales/ko.ts";

test("chat erasure requires exact selection and explicit permanent consent", () => {
  const request = { action: "eraseChatContent" as const, chatId: "chat_a", confirmed: true as const };
  assert.deepEqual(validateProjectCatalogRequest(request), request);
  for (const invalid of [{ ...request, confirmed: false }, { ...request, chatId: "" }, { ...request, chatIds: ["chat_a"] }, { action: "eraseChatContent", chatId: "chat_a" }])
    assert.throws(() => validateProjectCatalogRequest(invalid));
  const result = { schemaVersion: 1, action: request.action, chatId: "chat_a", erased: true };
  assert.deepEqual(validateProjectCatalogResult(result, request), result);
  // A partial or mismatched receipt is never accepted as completion.
  for (const invalid of [{ ...result, chatId: "chat_b" }, { ...result, erased: false }, { ...result, state: "redacted" }])
    assert.throws(() => validateProjectCatalogResult(invalid, request));
});

test("chat erasure list is bounded, unique and exposes only resumable states", () => {
  const request = { action: "chatErasureList" as const };
  assert.deepEqual(validateProjectCatalogRequest(request), request);
  assert.throws(() => validateProjectCatalogRequest({ ...request, projectId: "project_a" }));
  const entry = { chatId: "chat_a", projectId: "project_a", preview: "목표 미리보기", eligible: true, state: "none" };
  const result = { schemaVersion: 1, action: request.action, entries: [entry, { ...entry, chatId: "chat_b", eligible: false, state: "redacted" }] };
  assert.deepEqual(validateProjectCatalogResult(result, request), result);
  for (const entries of [[entry, entry], [{ ...entry, state: "erased" }], [{ ...entry, preview: "x".repeat(161) }], [{ ...entry, title: "x" }], Array.from({ length: 201 }, (_, index) => ({ ...entry, chatId: `chat_${index}` }))])
    assert.throws(() => validateProjectCatalogResult({ ...result, entries }, request));
  assert.deepEqual(validateProjectCatalogResult({ ...result, entries: [{ ...entry, preview: "가".repeat(160) }] }, request).action, request.action);
});

test("chat erasure copy exists in both languages and discloses retained and outside copies", () => {
  const keys = Object.keys(ko).filter(key => key.startsWith("m5.erasure."));
  assert.ok(keys.length >= 15);
  for (const key of keys) assert.equal(typeof (en as Record<string, string>)[key], "string", key);
  assert.match(ko["m5.erasure.confirmOutside"], /Claude Code·Codex/);
  assert.match(en["m5.erasure.confirmKept"], /Kept/);
});
