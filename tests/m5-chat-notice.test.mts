import assert from "node:assert/strict";
import test from "node:test";
import { chatNotice, formatChatNotice, recoveryConfirmation } from "../src/m5/chat-notice.ts";

test("role and provider notices retain context across display languages", () => {
  for (const roleId of ["planning", "coding", "review"] as const) {
    for (const runtime of ["codex", "claude-code"] as const) {
      for (const reason of ["platformUnsupported", "notInstalled", "installationIncomplete", "unsupportedVersion", "notAuthenticated", "authExpired", "capabilityUnknown", "exactModelUnavailable", "toString", "PRIVATE_UNKNOWN", null]) {
        const notice = chatNotice("m5.agentNotice.blocked", {roleId,runtime,reason});
        const before = JSON.stringify(notice);
        const english = formatChatNotice(notice, "en");
        assert.equal(formatChatNotice(notice, "ko"), notice.notice);
        assert.ok(english.includes(runtime === "codex" ? "Codex" : "Claude Code"));
        assert.ok(english.includes("No automatic switch to another model was made."));
        assert.ok(!english.includes("{") && !english.includes("PRIVATE_UNKNOWN") && !english.includes("toString"));
        assert.equal(JSON.stringify(notice), before);
      }
      for (const key of ["m5.agentNotice.checking", "m5.agentNotice.ready"] as const) {
        const notice = chatNotice(key, {roleId,runtime});
        assert.notEqual(formatChatNotice(notice,"ko"),formatChatNotice(notice,"en"));
        assert.ok(!formatChatNotice(notice,"en").includes("{"));
      }
    }
  }
  assert.equal(formatChatNotice({notice:"literal caller notice"},"en"),"literal caller notice");
});

test("both recovery confirmations retain termination and original-project boundaries", () => {
  for (const actual of [true,false]) {
    assert.match(recoveryConfirmation(actual,"ko"), /종료 증거를 다시 확인/);
    assert.match(recoveryConfirmation(actual,"ko"), /원본 프로젝트에는 적용하지 않습니다/);
    assert.match(recoveryConfirmation(actual,"en"), /rechecking termination evidence/);
    assert.match(recoveryConfirmation(actual,"en"), /Nothing will be applied to the original project/);
  }
});
