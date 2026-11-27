// Judges the response-time probe (scripts/m5-latency-run.mjs --check).
// A number only counts when the condition it is named after really held
// (PR #15 review B1): every stage ended with exit 0 and one engine for the
// whole stage (no restart), the second stage started from the seal, the seed
// is complete, each round has the requested samples, every sample of
// "running" kept the team run running and every sample of "background-check"
// kept the background check busy, from before its first call to after its
// last, and each file opened with one click. Only then are the targets read.
//
// Targets (latency work 2026-11-26): a file opened from the files tab shows
// its text within 300 ms at rest and within 1 s while a team runs or the
// background storage check runs (95th percentile); the approval list and
// getChat answer within 1 s in every condition.
//
// `--starting` (ADE stabilization A, real accounts): two Claude Code and two
// Codex runs start at once beside a working Claude run ("warm" alone is the
// control). The screen sends file read, approval list and getChat every
// second without waiting (open loop), so a hold of the engine counts as every
// read sent during it (one timer at a time). The load must hold: two Claude
// Code and two Codex starts accepted and the warm run alive through them.
// Only reads sent inside the condition count
// ("starting": a start still pending when sent — a read the starts hold until
// they end counts with its full wait; "warm": the warm run running before and
// after the round), at least
// OPEN_LOOP_MIN_SAMPLES of each; each answers within 1 s (95th percentile).
export const TARGETS = {
  "ui-open": { idle: 300, "idle-from-seal": 300, running: 1000, "background-check": 1000 },
  "file:read": { warm: 1000, starting: 1000 },
  "approval:list": { running: 1000, warm: 1000, starting: 1000 },
  getChat: { idle: 1000, "idle-from-seal": 1000, running: 1000, "background-check": 1000, warm: 1000, starting: 1000 },
};
const ROUNDS = { first: ["idle", "running"], second: ["idle-from-seal", "background-check"], starting: ["warm", "starting"] };
const LOADED = new Set(["running", "background-check"]);
const OPEN_LOOP = new Set(["warm", "starting"]);
export const OPEN_LOOP_MIN_SAMPLES = 5;

export const percentile = (values, p) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)];
};
const group = label => label.startsWith("ui-open:") ? "ui-open" : label;

/**
 * @param {{ stages: string[], exits: Record<string, number|null>, reports: Record<string, any>,
 *   samples: number, seedChats: number, seedRuns: number }} input
 */
export function judgeLatency({ stages, exits, reports, samples, seedChats, seedRuns }) {
  const problems = [];
  const verdicts = [];
  for (const stage of stages) {
    const report = reports[stage];
    if (exits[stage] !== 0) problems.push(`${stage}: the app exited ${exits[stage]}`);
    if (!report) { problems.push(`${stage}: no report`); continue; }
    if (report.status !== "DONE") problems.push(`${stage}: status ${report.status}`);
    const starts = report.engineStarts ?? [];
    if (starts.length !== 1) problems.push(`${stage}: ${starts.length} engine starts (a restart or none)`);
    if (stage === "second" && starts[0]?.fromSeal !== true) problems.push("second: the engine did not start from the seal");
    if (stage === "first") {
      if (report.seedChatsDone !== seedChats) problems.push(`first: ${report.seedChatsDone} of ${seedChats} seed tasks`);
      if (report.seedRunsDone !== seedRuns || (report.seedRunErrors ?? []).length) problems.push(`first: ${report.seedRunsDone} of ${seedRuns} seed runs (${(report.seedRunErrors ?? []).length} errors)`);
    }
    for (const condition of ROUNDS[stage] ?? []) {
      const round = (report.rounds ?? []).find(r => r.condition === condition);
      if (!round) { problems.push(`${condition}: not measured`); continue; }
      const open = OPEN_LOOP.has(condition);
      if (!open && round.samples !== samples) problems.push(`${condition}: ${round.samples} of ${samples} samples`);
      if (!open && LOADED.has(condition) && round.loadedSamples !== samples) problems.push(`${condition}: the load held in ${round.loadedSamples} of ${samples} samples`);
      if (condition === "starting") {
        // The load itself (PR #25 review I5): two Claude Code and two Codex
        // runs started and answered, beside a warm run alive to the end.
        const starts = report.starts ?? [];
        const accepted = runtime => starts.filter(start => start.runtime === runtime && !start.error && start.accepted).length;
        if (starts.some(start => start.error)) problems.push("starting: a start failed");
        if (starts.length !== 4 || accepted("claude-code") !== 2 || accepted("codex") !== 2) problems.push(`starting: ${accepted("claude-code")} Claude Code and ${accepted("codex")} Codex starts accepted of 2 + 2 (${starts.length} answered)`);
        if (round.warmStillRunning !== true) problems.push("starting: the warm run did not live through the starts");
      }
      if (condition === "warm" && round.warmHeld !== true) problems.push("warm: the warm run did not hold for the round");
      const measured = (report.measurements ?? []).filter(m => m.condition === condition);
      for (const m of measured) {
        if (!m.ok) problems.push(`${condition}: ${m.label} failed`);
        if (m.label.startsWith("ui-open:") && m.clicks !== 1) problems.push(`${condition}: ${m.label} took ${m.clicks} clicks`);
        if (LOADED.has(condition) && m.loaded !== true) problems.push(`${condition}: ${m.label} measured without the load`);
      }
      // Open loop: only the reads inside the condition count.
      const counted = open ? measured.filter(m => m.loaded === true) : measured;
      for (const [label, budgets] of Object.entries(TARGETS)) {
        const budget = budgets[condition];
        if (budget === undefined) continue;
        const list = counted.filter(m => group(m.label) === label);
        const complete = open ? list.length >= OPEN_LOOP_MIN_SAMPLES : list.length === samples;
        if (!complete) problems.push(`${condition}: ${label} has ${list.length} of ${open ? `at least ${OPEN_LOOP_MIN_SAMPLES}` : samples} samples`);
        const p95 = percentile(list.map(m => m.totalMs), 95);
        verdicts.push({ label, condition, budgetMs: budget, n: list.length, p95: p95 == null ? null : Math.round(p95 * 10) / 10, max: list.length ? Math.round(Math.max(...list.map(m => m.totalMs))) : null, pass: complete && p95 <= budget });
      }
    }
  }
  const pass = problems.length === 0 && verdicts.length > 0 && verdicts.every(v => v.pass);
  return { status: pass ? "PASS" : "FAIL", problems, verdicts };
}
