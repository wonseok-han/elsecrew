import assert from "node:assert/strict";
import test from "node:test";
import { emptyProjectGuide, parseProjectGuide, projectGuideKey, projectGuideStep } from "../src/m5/project-guide-state.ts";

test("guide preferences round trip without storing execution authority", () => {
  const preference = { ...emptyProjectGuide(), chatId: "chat-1", teamReviewed: true, dismissed: true };
  assert.deepEqual(parseProjectGuide(JSON.stringify(preference)), preference);
  for (const raw of [null, "{", "null", "[]", JSON.stringify({ ...preference, approved: true }), JSON.stringify({ ...preference, chatId: "../other" }), "x".repeat(1025)]) {
    assert.deepEqual(parseProjectGuide(raw), emptyProjectGuide());
  }
  assert.notEqual(projectGuideKey("project/a"), projectGuideKey("project%2Fa"));
});

test("guide resumes its pinned chat rather than adopting another chat", () => {
  const preference = { ...emptyProjectGuide(), chatId: "first", dismissed: true };
  assert.equal(projectGuideStep(preference, null, null), "resume");
  assert.equal(projectGuideStep(preference, "other", "completed"), "resume");
  assert.equal(projectGuideStep(preference, "first", "running"), "observe");
  assert.equal(projectGuideStep(preference, "first", "recoveryRequired"), "recovery");
  assert.equal(projectGuideStep(preference, "first", "needsAttention"), "recovery");
  assert.equal(projectGuideStep(preference, "first", "reviewReady"), "review");
  assert.equal(projectGuideStep(preference, "first", "completed"), "review");
});

test("initial guidance can advance without claiming execution or completion", () => {
  assert.equal(projectGuideStep(emptyProjectGuide(), null, null), "team");
  assert.equal(projectGuideStep({ ...emptyProjectGuide(), teamReviewed: true }, null, null), "goal");
  assert.equal(projectGuideStep(emptyProjectGuide(), "new", "preparing"), "observe");
});
