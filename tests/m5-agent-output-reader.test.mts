import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { AgentOutputReader, type AgentOutputSource } from "../src/m5/agent-output-reader.ts";
import type { OutputPage, OutputPageRequest } from "../src/m5/contracts/agent-output-pages.ts";

const golden: OutputPage = JSON.parse(readFileSync(new URL("./fixtures/m5/agent-output-page.json", import.meta.url), "utf8"));
const binding = golden.request.binding;
class Source implements AgentOutputSource {
  committed = 0;
  sealed = false;
  registered = false;
  detached = false;
  notify = () => {};
  heads = 0;
  async subscribe(_binding: unknown, notify: () => void): Promise<() => void> {
    this.notify = notify;
    this.registered = true;
    return () => { this.detached = true; };
  }
  async head() {
    assert.ok(this.registered);
    this.heads++;
    return { formatVersion: 1, binding, committedSequence: this.committed, sealed: this.sealed };
  }
  async page(request: OutputPageRequest) {
    const end = Math.min(request.throughSequence, request.afterSequence + request.limit);
    return new TextEncoder().encode(JSON.stringify({
      request,
      rows: Array.from({ length: end - request.afterSequence }, (_, i) => ({
        outputSequence: request.afterSequence + i + 1,
        event: { ...golden.rows[0]!.event, eventId: `agent_event_${request.afterSequence + i + 1}` },
      })),
      nextAfterSequence: end,
    }));
  }
}
test("catches last commit during page consumption without a new notification", async () => {
  const source = new Source(); source.committed = 1;
  const seen: number[] = [];
  const errors: unknown[] = [];
  const reader = new AgentOutputReader(source, binding, async rows => {
    seen.push(...rows.map(row => row.outputSequence));
    source.committed = 2; source.sealed = true;
  }, error => errors.push(error));
  await reader.start();
  assert.deepEqual(seen, [1, 2]);
  assert.equal(reader.cursor, 2);
  assert.ok(source.heads >= 3);
  assert.deepEqual(errors, []);
  reader.stop(); assert.ok(source.detached);
});
test("reconciliation recovers missed hints and serializes concurrent refreshes", async () => {
  const source = new Source();
  let consuming = 0; let max = 0; let count = 0;
  const reader = new AgentOutputReader(source, binding, async rows => {
    consuming++; max = Math.max(max, consuming);
    await Promise.resolve(); count += rows.length; consuming--;
  }, () => assert.fail("unexpected output failure"));
  await reader.start();
  source.committed = 3000;
  await Promise.all([reader.refresh(), reader.refresh(), reader.refresh()]);
  assert.equal(count, 3000); assert.equal(max, 1); assert.equal(reader.cursor, 3000);
  reader.stop();
});
test("consumer failure does not advance cursor and closes subscription", async () => {
  const source = new Source(); source.committed = 1;
  const errors: unknown[] = [];
  const reader = new AgentOutputReader(source, binding, async () => { throw new Error("consumer failed"); }, error => errors.push(error));
  await assert.rejects(reader.start(), /consumer failed/);
  assert.equal(reader.cursor, 0); assert.equal(errors.length, 1); assert.ok(source.detached);
});
test("rejects changed sealed head instead of inventing completion", async () => {
  const source = new Source(); source.sealed = true;
  const errors: unknown[] = [];
  const reader = new AgentOutputReader(source, binding, async () => {}, error => errors.push(error));
  await reader.start(); source.committed = 1;
  await assert.rejects(reader.refresh(), /sealing/);
  assert.equal(errors.length, 1); assert.ok(source.detached);
});
test("stop during head request prevents late display writes", async () => {
  const source = new Source(); source.committed = 1;
  let release!: () => void;
  const waiting = new Promise<void>(resolve => { release = resolve; });
  const original = source.head.bind(source);
  source.head = async () => { await waiting; return original(); };
  const reader = new AgentOutputReader(source, binding, async () => assert.fail("late display"), () => assert.fail("unexpected error"));
  const started = reader.start();
  await Promise.resolve(); await Promise.resolve();
  reader.stop(); release(); await started;
  assert.equal(reader.cursor, 0);
});
