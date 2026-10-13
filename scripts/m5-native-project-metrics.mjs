// Repeated native development UI evidence, not the packaged G6 collector.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
if (process.platform !== 'darwin') throw new Error('Native project measurement requires macOS');
const diffOnly = process.argv.length === 3 && process.argv[2] === '--diff';
if (process.argv.length !== 2 && !diffOnly) throw new Error('Usage: m5-native-project-metrics.mjs [--diff]');
const output = fs.mkdtempSync(path.join(os.tmpdir(), 'hello-agent-native-metrics-'));
const git = (...args) => execFileSync('/usr/bin/git', args, { cwd: repo, encoding: 'utf8' }).trim();
const report = {
  schemaVersion: 1, diagnostic: true, gateEligible: false, status: 'RUNNING',
  head: git('rev-parse', 'HEAD'), gitStatusBefore: git('status', '--porcelain'),
  measuredAt: new Date().toISOString(), platform: process.platform, arch: process.arch,
  machine: os.hostname(), cpu: os.cpus()[0]?.model, ramBytes: os.totalmem(),
  warmupRuns: 1, measuredRuns: 10, trials: [],
  scenario: diffOnly ? 'nativeLargeDiffOnly' : 'nativeLargeTreeAndFile',
  scope: 'Development native UI; not packaged, RSS, streams, save races or detached descendants',
};
const persist = () => fs.writeFileSync(path.join(output, 'measurement.json'), JSON.stringify(report, null, 2));
persist();
console.log(JSON.stringify({ stage: 'COLLECTION_STARTED', output }));

async function trial(index) {
  const log = fs.createWriteStream(path.join(output, `trial-${index}.log`));
  const env = { ...process.env };
  // Do not inherit source writes, reload, or memory scenarios into this run.
  for (const key of Object.keys(env)) if (key.startsWith('HELLO_AGENT_SMOKE_')) delete env[key];
  if (diffOnly) env.HELLO_AGENT_SMOKE_LARGE_DIFF = '1';
  else {
    env.HELLO_AGENT_SMOKE_LARGE_TREE = '1';
    env.HELLO_AGENT_SMOKE_LARGE_FILE = '1';
  }
  delete env.ELECTRON_RUN_AS_NODE;
  let pending = '';
  let result;
  const child = spawn(path.join(repo, 'node_modules/.bin/electron'), ['scripts/m5-project-ui-smoke.cjs'], { cwd: repo, env, stdio: ['ignore', 'pipe', 'pipe'] });
  console.log(JSON.stringify({ stage: 'TRIAL_STARTED', index, warmup: index === 0, pid: child.pid }));
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', chunk => {
    log.write(chunk);
    pending += chunk;
    let end;
    while ((end = pending.indexOf('\n')) !== -1) {
      const line = pending.slice(0, end); pending = pending.slice(end + 1);
      try {
        const value = JSON.parse(line);
        if (value.root && value.shutdown) result = value;
      } catch { /* Electron diagnostics are preserved verbatim in the log. */ }
    }
  });
  child.stderr.on('data', chunk => log.write(chunk));
  // Await actual close, not a timeout or an intermediate result. Never force-kill.
  const termination = await new Promise(resolve => {
    child.once('error', error => resolve({ error: error.message }));
    child.once('close', (code, signal) => resolve({ code, signal }));
  });
  await new Promise(resolve => log.end(resolve));
  return { index, warmup: index === 0, ...termination, result };
}

try {
  for (let index = 0; index <= report.measuredRuns; index++) {
    const sample = await trial(index);
    report.trials.push(sample);
    persist();
    console.log(JSON.stringify({ stage: 'TRIAL_FINISHED', index, code: sample.code, status: sample.result?.status }));
    if (sample.signal || sample.error || sample.result?.shutdown !== 'accepted') {
      throw new Error(`Trial ${index} did not confirm normal shutdown; no further app will be launched`);
    }
  }
  const measured = report.trials.filter(sample => !sample.warmup);
  report.timings = {};
  const metrics = diffOnly
    ? [['largeDiff', 'diffSummaryMs'], ['largeDiff', 'diffFirstFileMs']]
    : [['largeTree', 'snapshotReadyMs'], ['largeTree', 'treeRootMs'], ['largeTree', 'treeExpandMs'], ['largeFile', 'fileEditableMs'], ['confirmation', 'readyMs']];
  for (const [group, key] of metrics) {
    const values = measured.map(sample => sample.result?.[group]?.[key]);
    report.timings[key] = { values, p95: values.every(value => Number.isFinite(value) && value >= 0) ? Math.max(...values) : null };
  }
  report.status = report.trials.some(sample => sample.code !== 0 || sample.result?.status !== 'PASS') ? 'FAIL' : 'NOT_READY';
  if (diffOnly && report.trials.some(sample => sample.result?.scenario !== 'nativeLargeDiffOnly'
    || sample.result?.largeDiff?.diffFiles !== 500
    || sample.result?.largeDiff?.diffChangedLines !== 20000
    || sample.result?.largeDiff?.diffBytes !== 10485760)) {
    report.status = 'FAIL';
    report.error = 'Missing or mismatched native diff fixture evidence';
  }
} catch (error) {
  report.status = 'FAIL'; report.error = error.message;
} finally {
  report.gitStatusAfter = git('status', '--porcelain');
  report.headAfter = git('rev-parse', 'HEAD');
  persist();
}
console.log(JSON.stringify({ output, status: report.status, timings: report.timings }));
process.exitCode = report.status === 'FAIL' ? 1 : 2;
