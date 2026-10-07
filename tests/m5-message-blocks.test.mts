import assert from "node:assert/strict";
import test from "node:test";
import { messageBlocks } from "../src/m5/message-blocks.ts";

test("ordinary and HTML-like messages remain literal text", () => {
  const text = '<script>alert(1)</script>\n[link](javascript:alert(1))';
  assert.deepEqual(messageBlocks(text), [{ kind: "text", text }]);
});
test("fenced code preserves whitespace and CRLF for copying", () => {
  assert.deepEqual(messageBlocks("Before\r\n```ts\r\n  const n = 1;\r\n```\r\nAfter"), [
    { kind: "text", text: "Before\r\n" },
    { kind: "code", language: "ts", text: "  const n = 1;\r\n" },
    { kind: "text", text: "After" },
  ]);
});
test("shorter and mismatched fences do not close a code block", () => {
  assert.deepEqual(messageBlocks("````md\n```\n~~~\n````\n"), [{ kind: "code", language: "md", text: "```\n~~~\n" }]);
});
test("unfinished streamed blocks and empty blocks are represented", () => {
  assert.deepEqual(messageBlocks("~~~sh\necho hi"), [{ kind: "code", language: "sh", text: "echo hi" }]);
  assert.deepEqual(messageBlocks("```\n```"), [{ kind: "code", language: "", text: "" }]);
  assert.deepEqual(messageBlocks(""), [{ kind: "text", text: "" }]);
});
