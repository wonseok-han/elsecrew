import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { validateChatAppendMessageRequestV1, validateChatAppendMessageTranscriptV1 } from "../src/m5/contracts/run-recovery-v1.ts";
const fixture = JSON.parse(readFileSync(new URL("./fixtures/m5/followup-message-v1.json", import.meta.url), "utf8"));

test("follow-up request and durable user message bind exact identity, text and sequence", () => {
  validateChatAppendMessageTranscriptV1(fixture);
  for (const [field, value] of Object.entries({ chatId: "other", taskId: "other", worktreeId: "other", text: "changed", messageId: "other", eventSequence: 5, kind: "status" })) {
    assert.throws(() => validateChatAppendMessageTranscriptV1({ ...fixture, message: { ...fixture.message, [field]: value } }));
  }
});

test("follow-up rejects spoofing, unknown fields and exhausted or invalid input", () => {
  for (const patch of [{ runId: "run" }, { clientMessageId: "role_0123456789abcdef0123456789abcdef" }, { expectedEventSequence: Number.MAX_SAFE_INTEGER }, { expectedEventSequence: 0 }, { text: "" }, { text: "가".repeat(5462) }]) {
    assert.throws(() => validateChatAppendMessageRequestV1({ ...fixture.request, ...patch }));
  }
  validateChatAppendMessageRequestV1({ ...fixture.request, text: "a".repeat(16384) });
});
