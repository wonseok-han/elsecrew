import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { validateDecodedOutputPage, decodeOutputPage, validateOutputPageRequest, OUTPUT_PAGE_MAX_BYTES, type OutputPageRequest, type OutputPage } from "../src/m5/contracts/agent-output-pages.ts";
import type { AgentPublicEventV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import { decodeJsonl } from "../src/m5/contracts/protocol-v1.ts";

const event: AgentPublicEventV1 = JSON.parse(readFileSync(new URL("./fixtures/m5/protocol-v1/valid/agent-public-event.jsonl", import.meta.url), "utf8")).payload;
const { chatId, taskId, projectId, worktreeId, workspaceIdentity, runId, roleId, capabilityFingerprint } = event;
const binding = { chatId, taskId, projectId, worktreeId, workspaceIdentity, runId, roleId, capabilityFingerprint };
test("output notification contains only a bound committed head", () => {
  const wire = JSON.parse(readFileSync(new URL("./fixtures/m5/protocol-v1/valid/agent-public-event.jsonl", import.meta.url), "utf8"));
  wire.event = "agent.outputAvailable";
  wire.payload = { formatVersion: 1, binding, committedSequence: 3000, sealed: true };
  assert.deepEqual(decodeJsonl(JSON.stringify(wire)), wire);
  for (const mutation of [
    (value: typeof wire) => { value.runId = "agent_run_other"; },
    (value: typeof wire) => { value.payload.committedSequence = -1; },
    (value: typeof wire) => { value.payload.committedSequence = Number.MAX_SAFE_INTEGER + 1; },
    (value: typeof wire) => { value.payload.text = "must not transmit bodies"; },
    (value: typeof wire) => { value.payload.binding.roleId = "unknown"; },
  ]) {
    const bad = structuredClone(wire); mutation(bad);
    assert.throws(() => decodeJsonl(JSON.stringify(bad)));
  }
});
function request(afterSequence = 0, throughSequence = 3000): OutputPageRequest {
  return { formatVersion: 1, binding, afterSequence, throughSequence, limit: 100 };
}
function page(query: OutputPageRequest, count: number): OutputPage {
  return {
    request: query,
    rows: Array.from({ length: count }, (_, i) => ({ outputSequence: query.afterSequence + i + 1, event: { ...event, eventId: `agent_event_${query.afterSequence + i + 1}` } })),
    nextAfterSequence: query.afterSequence + count,
  };
}
const encode = (value: unknown) => new TextEncoder().encode(JSON.stringify(value));

test("item updates retain append/complete identity and reject unsafe metadata", () => {
  const query = request(0, 1);
  const value = page(query, 1);
  value.rows[0]!.event.kind = "status";
  value.rows[0]!.itemUpdate = { itemId: "item-1", operation: "append", channel: "message" };
  assert.deepEqual(decodeOutputPage(encode(value), query), value);
  value.rows[0]!.itemUpdate.operation = "complete";
  value.rows[0]!.event.kind = "final";
  assert.deepEqual(decodeOutputPage(encode(value), query), value);
  value.rows[0]!.itemUpdate.operation = "append";
  assert.throws(() => decodeOutputPage(encode(value), query));
  value.rows[0]!.event.kind = "status";
  for (const itemId of ["", "a\u0000b", "가".repeat(43)]) {
    value.rows[0]!.itemUpdate.itemId = itemId;
    assert.throws(() => decodeOutputPage(encode(value), query));
  }
});

test("shared Rust/TypeScript golden page", () => {
  const bytes = readFileSync(new URL("./fixtures/m5/agent-output-page.json", import.meta.url));
  const query = request(2999);
  assert.deepEqual(decodeOutputPage(bytes, query), JSON.parse(bytes.toString("utf8")));
});

test("single run has 3000 ordered events across bounded pages", () => {
  for (let after = 0; after < 3000; after += 100) {
    const query = request(after);
    assert.equal(decodeOutputPage(encode(page(query, 100)), query).nextAfterSequence, after + 100);
  }
  const end = request(3000);
  assert.equal(decodeOutputPage(encode(page(end, 0)), end).rows.length, 0);
});
test("reject gaps, duplicate IDs, cross-run events and false progress", () => {
  const query = request();
  for (const mutate of [
    (p: OutputPage) => { p.rows[1]!.outputSequence = 3; },
    (p: OutputPage) => { p.rows[1]!.event.eventId = p.rows[0]!.event.eventId; },
    (p: OutputPage) => { p.rows[0]!.event.runId = "agent_run_other"; },
    (p: OutputPage) => { p.nextAfterSequence = 3; },
  ]) {
    const value = page(query, 2); mutate(value);
    assert.throws(() => decodeOutputPage(encode(value), query));
  }
  assert.throws(() => decodeOutputPage(encode(page(query, 0)), query));
});
test("reject unsafe cursor, oversized bytes, invalid UTF8 and extra keys", () => {
  for (const value of [-1, 0.5, Number.MAX_SAFE_INTEGER + 1]) {
    assert.throws(() => validateOutputPageRequest({ ...request(), throughSequence: value }));
  }
  assert.throws(() => decodeOutputPage(new Uint8Array(OUTPUT_PAGE_MAX_BYTES + 1), request()));
  assert.throws(() => decodeOutputPage(new Uint8Array([255]), request()));
  assert.throws(() => decodeOutputPage(encode({ ...page(request(), 1), extra: true }), request()));
});
test("maximum escaped body fits without splitting an event", () => {
  const query = request(0, 1);
  const value = page(query, 1);
  value.rows[0]!.event.text = "\u0001".repeat(64 * 1024);
  const bytes = encode(value);
  assert.ok(bytes.length > 256 * 1024);
  assert.equal(decodeOutputPage(bytes, query).rows.length, 1);
});


test("decoded transport path preserves binding, sequence, shape and UTF8 byte limits", () => {
  const query=request();
  const value=page(query,100);
  assert.deepEqual(validateDecodedOutputPage(value,query),decodeOutputPage(encode(value),query));
  for(const mutate of [
    (p:any)=>{p.rows[1].outputSequence=3;},
    (p:any)=>{p.rows[1].event.eventId=p.rows[0].event.eventId;},
    (p:any)=>{p.rows[0].event.runId='agent_run_other';},
    (p:any)=>{p.request.binding.projectId='project_other';},
    (p:any)=>{p.nextAfterSequence=99;},
    (p:any)=>{p.extra=true;},
    (p:any)=>{p.rows[0].itemUpdate={itemId:'x',operation:'unsafe',channel:'message'};},
    (p:any)=>{for(const row of p.rows)row.event.text='가'.repeat(10000);},
  ]) {
    const bad=structuredClone(value);mutate(bad);
    assert.throws(()=>validateDecodedOutputPage(bad,query));
    assert.throws(()=>decodeOutputPage(encode(bad),query));
  }
  const escaped=page(request(0,1),1);escaped.rows[0].event.text='\u0001'.repeat(64*1024);
  assert.deepEqual(validateDecodedOutputPage(escaped,escaped.request),decodeOutputPage(encode(escaped),escaped.request));
});
