// PR #15 review B1: the latency check passes only when the conditions it names
// held. The reviewer's false pass (one 50 ms sample per group, the run gone,
// two clicks, failed seeds, a background check already over) must fail.
import assert from "node:assert/strict";
import test from "node:test";
import { judgeLatency } from "../scripts/m5-latency-judge.mjs";

const SAMPLES = 3;
function round(condition: string, loaded: boolean | null, overrides: Record<string, unknown> = {}) {
  const measurements = [];
  for (let index = 0; index < SAMPLES; index += 1) {
    measurements.push({ label: "getChat", condition, ok: true, totalMs: 20, loaded });
    if (condition === "running") measurements.push({ label: "approval:list", condition, ok: true, totalMs: 5, loaded });
    measurements.push({ label: `ui-open:src/part${index}/file0.ts`, condition, ok: true, clicks: 1, totalMs: 90, loaded });
  }
  return { round: { condition, samples: SAMPLES, loadedSamples: loaded ? SAMPLES : 0, ...overrides }, measurements };
}
function good() {
  const idle = round("idle", null), running = round("running", true);
  const sealIdle = round("idle-from-seal", null), background = round("background-check", true);
  return {
    stages: ["first", "second"], exits: { first: 0, second: 0 }, samples: SAMPLES, seedChats: 4, seedRuns: 8,
    reports: {
      first: { stage: "first", status: "DONE", engineStarts: [{ fromSeal: false }], seedChatsDone: 4, seedRunsDone: 8, seedRunErrors: [],
        rounds: [idle.round, running.round], measurements: [...idle.measurements, ...running.measurements] },
      second: { stage: "second", status: "DONE", engineStarts: [{ fromSeal: true }],
        rounds: [sealIdle.round, background.round], measurements: [...sealIdle.measurements, ...background.measurements] },
    },
  };
}
type Input = ReturnType<typeof good>;
const failsWith = (input: Input, pattern: RegExp) => {
  const result = judgeLatency(input);
  assert.equal(result.status, "FAIL");
  assert.ok(result.problems.some(problem => pattern.test(problem)) || result.verdicts.some(v => !v.pass), JSON.stringify(result.problems));
  if (pattern.source !== "budget") assert.ok(result.problems.some(problem => pattern.test(problem)), `${pattern} in ${JSON.stringify(result.problems)}`);
};

test("latency check passes a complete run whose loads held", () => {
  const result = judgeLatency(good());
  assert.deepEqual(result.problems, []);
  assert.equal(result.status, "PASS");
  assert.equal(result.verdicts.length, 9);
});

test("the reviewer's false pass fails: one fast sample, the run gone, two clicks, failed seeds, the check over", () => {
  const input = good();
  for (const report of Object.values(input.reports)) {
    const kept = new Map<string, unknown>();
    for (const m of report.measurements) { const key = `${m.condition}:${m.label.split(":")[0]}`; if (!kept.has(key)) kept.set(key, { ...m, totalMs: 50 }); }
    report.measurements = [...kept.values()] as typeof report.measurements;
  }
  for (const m of input.reports.first.measurements) if (m.condition === "running") { (m as { loaded: boolean }).loaded = false; if (m.label.startsWith("ui-open")) (m as { clicks: number }).clicks = 2; }
  input.reports.first.rounds[1].loadedSamples = 0;
  input.reports.first.seedRunErrors = ["start failed"] as never[];
  input.reports.second.rounds[1].loadedSamples = 0;
  const result = judgeLatency(input);
  assert.equal(result.status, "FAIL");
  for (const pattern of [/running: the load held in 0/, /measured without the load/, /took 2 clicks/, /seed runs/, /background-check: the load held in 0/, /has 1 of 3 samples/]) {
    assert.ok(result.problems.some(problem => pattern.test(problem)), `${pattern} in ${JSON.stringify(result.problems)}`);
  }
});

test("each missing condition fails the check on its own", () => {
  let input = good(); input.exits.second = 1; failsWith(input, /second: the app exited 1/);
  input = good(); input.reports.first.engineStarts.push({ fromSeal: false }); failsWith(input, /first: 2 engine starts/);
  input = good(); input.reports.second.engineStarts = [{ fromSeal: false }]; failsWith(input, /did not start from the seal/);
  input = good(); input.reports.first.seedChatsDone = 3; failsWith(input, /3 of 4 seed tasks/);
  input = good(); input.reports.first.status = "FAIL"; failsWith(input, /first: status FAIL/);
  input = good(); input.reports.second.rounds.pop(); failsWith(input, /background-check: not measured/);
  input = good(); input.reports.first.rounds[0].samples = 2; failsWith(input, /idle: 2 of 3 samples/);
  input = good(); (input.reports.first.measurements[0] as { ok: boolean }).ok = false; failsWith(input, /getChat failed/);
  input = good(); delete (input.reports as Record<string, unknown>).second; failsWith(input, /second: no report/);
  input = good(); input.reports.first.measurements.filter(m => m.condition === "idle" && m.label.startsWith("ui-open")).forEach(m => { m.totalMs = 400; });
  const slow = judgeLatency(input);
  assert.equal(slow.status, "FAIL");
  assert.deepEqual(slow.problems, []);
  assert.equal(slow.verdicts.find(v => v.label === "ui-open" && v.condition === "idle")?.pass, false);
});

// ADE stabilization A: `--starting` (open loop). Only reads sent while a
// start was pending count; the warm control must hold; a failed start or too
// few samples fail; a slow read fails its budget.
const FOUR_STARTS = ["claude-code", "claude-code", "codex", "codex"].map(runtime => ({ runtime, accepted: { runId: "r" } }));
function starting(overrides: { warmHeld?: boolean; warmAlive?: boolean; starts?: unknown[]; startingMs?: number; startingLoaded?: number } = {}) {
  const measurements = [];
  for (const condition of ["warm", "starting"]) {
    for (let index = 0; index < 8; index += 1) {
      const loaded = condition === "warm" ? overrides.warmHeld !== false : index < (overrides.startingLoaded ?? 8);
      for (const label of ["getChat", "approval:list", "file:read"]) {
        measurements.push({ label, condition, ok: true, loaded, totalMs: condition === "starting" ? (overrides.startingMs ?? 120) : 40 });
      }
    }
  }
  // Reads sent after the starts ended (not counted, however slow).
  measurements.push({ label: "getChat", condition: "starting", ok: true, loaded: false, totalMs: 99_000 });
  return {
    stages: ["starting"], exits: { starting: 0 }, samples: 8, seedChats: 0, seedRuns: 0,
    reports: { starting: { stage: "starting", status: "DONE", engineStarts: [{}], starts: overrides.starts ?? FOUR_STARTS,
      rounds: [{ condition: "warm", samples: 8, warmHeld: overrides.warmHeld !== false }, { condition: "starting", samples: 9, warmStillRunning: overrides.warmAlive !== false }], measurements } },
  };
}

test("starting: reads sent while runs start answer within budget; reads after the window do not count", () => {
  const result = judgeLatency(starting() as never);
  assert.deepEqual(result.problems, []);
  assert.equal(result.status, "PASS");
  assert.equal(result.verdicts.length, 6);
  assert.ok(result.verdicts.every(v => v.n === 8));
});

test("starting: a slow read, a failed start, too few samples or a warm run that ended fail", () => {
  assert.ok(judgeLatency(starting({ startingMs: 1500 }) as never).verdicts.some(v => !v.pass && v.condition === "starting"));
  failsWith(starting({ starts: [{ runtime: "codex", error: "boom" }] }) as never, /a start failed/);
  // PR #25 review I5: no starts at all, or the warm run gone, is not the load.
  failsWith(starting({ starts: [] }) as never, /0 Claude Code and 0 Codex starts accepted/);
  failsWith(starting({ starts: FOUR_STARTS.slice(0, 3) }) as never, /starts accepted of 2 \+ 2/);
  failsWith(starting({ warmAlive: false }) as never, /warm run did not live through the starts/);
  failsWith(starting({ startingLoaded: 3 }) as never, /has 3 of at least 5 samples/);
  failsWith(starting({ warmHeld: false }) as never, /warm run did not hold/);
});

// PR #25 review I5: the open-loop sampler keeps one timer at a time. Two
// rounds switched back to back (and an end before a begin) never double the
// rate: one tick per interval, one pending timer.
test("starting: the sampler keeps one timer however rounds switch", async () => {
  const { createRequire } = await import("node:module");
  const { createSampler } = createRequire(import.meta.url)("../scripts/m5-latency-sampler.cjs");
  let clock = 0, nextId = 1;
  const timers = new Map<number, { at: number; fn: () => void }>();
  const env = {
    now: () => clock, startsPending: () => true, tickMs: 1000,
    setTimeout: (fn: () => void, ms: number) => { const id = nextId++; timers.set(id, { at: clock + ms, fn }); return id; },
    clearTimeout: (id: number) => { timers.delete(id); },
    reads: { getChat: async () => undefined, "approval:list": async () => undefined, "file:read": async () => undefined },
  };
  const advance = (ms: number) => {
    const end = clock + ms;
    for (;;) {
      const due = [...timers].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
      if (!due) break;
      timers.delete(due[0]); clock = due[1].at; due[1].fn();
    }
    clock = end;
  };
  const s = createSampler(env);
  s.begin("warm");
  advance(2500);
  s.end();
  s.begin("starting");
  advance(3500);
  assert.equal(timers.size, 1, "one pending timer");
  assert.deepEqual(s.ticks, { warm: 3, starting: 4 });
  s.begin("starting");
  assert.equal(timers.size, 1, "switching again keeps one");
  s.end();
  assert.equal(timers.size, 0);
  await new Promise(resolve => setTimeout(resolve, 5));
  assert.equal(s.measurements.length, 3 * (3 + 4 + 1));
  assert.equal(s.outstanding, 0);
});
