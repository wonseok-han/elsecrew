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

test("Command/Ctrl Enter use the enabled send button in new and existing chats", () => {
  assert.equal(press({ metaKey: true }).clicks, 1);
  assert.equal(press({ ctrlKey: true }).clicks, 1);
  assert.equal(press({ ctrlKey: true, target: { tagName: "INPUT" } }).clicks, 1);
  assert.deepEqual(press({ metaKey: true }, true), { clicks: 0, prevented: true });
});

test("newline, composition, held keys and other controls never submit", () => {
  for (const overrides of [
    {}, { metaKey: true, nativeEvent: { isComposing: true } },
    { ctrlKey: true, nativeEvent: { keyCode: 229 } }, { ctrlKey: true, repeat: true },
    { metaKey: true, shiftKey: true }, { ctrlKey: true, altKey: true },
    { metaKey: true, target: { tagName: "SELECT" } },
  ]) assert.deepEqual(press(overrides), { clicks: 0, prevented: false });
});
