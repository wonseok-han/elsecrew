import assert from "node:assert/strict";
import test from "node:test";
import { PROVIDER_SELECTION_KEY, parseProviderSelection, readProviderSelection, saveProviderSelection } from "../src/m5/provider-selection.ts";

test("only explicit known unique provider selections survive restart", () => {
  for (const providers of [["codex"], ["claude"], ["codex", "claude"]] as const) {
    let saved = "";
    assert.equal(saveProviderSelection(providers, {setItem(key, value) { assert.equal(key, PROVIDER_SELECTION_KEY); saved=value; }}), true);
    assert.deepEqual(readProviderSelection({getItem(key) { assert.equal(key, PROVIDER_SELECTION_KEY); return saved; }}), providers);
  }
  assert.deepEqual(parseProviderSelection('{"version":1,"providers":["claude","codex"]}'), ["codex","claude"]);
});

test("missing, corrupt and future preferences require a new explicit selection", () => {
  for (const raw of [null, "", "[", "null", "[]", "{}", "x".repeat(129),
    '{"version":2,"providers":["codex"]}', '{"version":1,"providers":[]}',
    '{"version":1,"providers":["codex","codex"]}', '{"version":1,"providers":["other"]}',
    '{"version":1,"providers":["codex"],"connected":true}', '{"version":1,"providers":"codex"}']) {
    assert.deepEqual(parseProviderSelection(raw), []);
  }
});

test("storage denial is recoverable and empty selections are not persisted", () => {
  assert.deepEqual(readProviderSelection({getItem() { throw new Error("denied"); }}), []);
  assert.equal(saveProviderSelection(["codex"], {setItem() { throw new Error("denied"); }}), false);
  assert.equal(saveProviderSelection([], {setItem() { assert.fail("must not write"); }}), false);
});
