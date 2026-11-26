import assert from "node:assert/strict";
import test from "node:test";
import { submitChatShortcut } from "../src/m5/chat-submit-shortcut.ts";

function press(overrides: Record<string, unknown> = {}, disabled = false) {
  let clicks = 0;
  let prevented = false;
  const event = {
    key: "Enter", metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, repeat: false,
    nativeEvent: { isComposing: false, keyCode: 13 }, target: { tagName: "TEXTAREA" },
    currentTarget: { querySelector: () => ({ disabled, click: () => clicks++ }) },
    preventDefault: () => { prevented = true; }, ...overrides,
  };
  submitChatShortcut(event as unknown as Parameters<typeof submitChatShortcut>[0]);
  return { clicks, prevented };
}

test("Enter and Command/Ctrl Enter use the enabled send button in new and existing chats", () => {
  assert.equal(press().clicks, 1);
  assert.equal(press({ target: { tagName: "INPUT" } }).clicks, 1);
  assert.equal(press({ metaKey: true }).clicks, 1);
  assert.equal(press({ ctrlKey: true }).clicks, 1);
  assert.deepEqual(press({}, true), { clicks: 0, prevented: true });
});

test("Shift Enter newline, composition, held keys and other controls never submit", () => {
  for (const overrides of [
    { shiftKey: true }, { metaKey: true, shiftKey: true },
    { nativeEvent: { isComposing: true } }, { nativeEvent: { keyCode: 229 } },
    { repeat: true }, { altKey: true }, { ctrlKey: true, altKey: true },
    { target: { tagName: "SELECT" } }, { key: "a" },
  ]) assert.deepEqual(press(overrides), { clicks: 0, prevented: false });
});
