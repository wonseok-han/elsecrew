import assert from "node:assert/strict";
import test from "node:test";
import { parseWorkActivity } from "../src/m5/work-history.ts";

test("work history rows show the kind of work and never unknown fields", () => {
  assert.deepEqual(parseWorkActivity('{"action":"read","target":"src/app.ts"}'), { action: "read", target: "src/app.ts" });
  assert.deepEqual(parseWorkActivity('{"action":"command","outcome":"refused"}'), { action: "command", outcome: "refused" });
  assert.deepEqual(parseWorkActivity('{"action":"other","tool":"WebFetch"}'), { action: "other", tool: "WebFetch" });
  for (const text of [
    "plain model text", "[]", "null",
    '{"action":"delete"}',
    '{"action":"read","command":"cat secret"}',
    '{"action":"read","target":""}',
    '{"action":"read","target":"a\\nb"}',
    `{"action":"read","target":"${"a".repeat(300)}"}`,
    '{"action":"read","outcome":"maybe"}',
    '{"action":"read","target":"file\u202etxt.exe"}',
  ]) assert.equal(parseWorkActivity(text), null, text);
  const emoji = "😀".repeat(200);
  assert.deepEqual(parseWorkActivity(JSON.stringify({ action: "read", target: emoji })), { action: "read", target: emoji });
});
