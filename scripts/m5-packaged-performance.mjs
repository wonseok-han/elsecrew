// Repeated real-package diagnostics. This is not the complete G6 collector.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';

const mode = process.argv[2];
assert.ok(process.argv.length === 3 && ['--diff', '--tree-file'].includes(mode));
const repo = fileURLToPath(new URL('..', import.meta.url));
const output = fs.mkdtempSync(path.join(os.tmpdir(), 'hello-agent-packaged-performance-'));
const git = (...args) => execFileSync('/usr/bin/git', args, {cwd:repo, encoding:'utf8'}).trim();
const report = {schemaVersion:1, diagnostic:true, gateEligible:false, status:'RUNNING',
  head:git('rev-parse', 'HEAD'), gitStatusBefore:git('status', '--porcelain'),
  mode, warmupRuns:1, measuredRuns:10, trials:[]};
const persist = () => fs.writeFileSync(path.join(output, 'measurement.json'), JSON.stringify(report, null, 2));
persist(); console.log(JSON.stringify({stage:'COLLECTION_STARTED', output}));

async function trial(index) {
  const child = spawn(process.execPath, ['--experimental-strip-types', 'scripts/m5-packaged-ui-smoke.mjs', mode === '--diff' ? '--large-diff' : '--large-project'], {cwd:repo, stdio:['ignore','pipe','pipe']});
  const log = fs.createWriteStream(path.join(output, `trial-${index}.log`));
  let pending = '', root;
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', chunk => {
    log.write(chunk); pending += chunk;
    let end;
    while ((end = pending.indexOf('\n')) !== -1) {
      const line = pending.slice(0, end); pending = pending.slice(end + 1);
      try { const message = JSON.parse(line); if (message.stage === 'STARTED') root = message.root; } catch { /* Preserve raw diagnostics. */ }
    }
  });
  child.stderr.on('data', chunk => log.write(chunk));
  const termination = await new Promise(resolve => {
    child.once('error', error => resolve({error:String(error)}));
    child.once('close', (code, signal) => resolve({code, signal}));
  });
  await new Promise(resolve => log.end(resolve));
  assert.ok(!termination.error && termination.signal === null && [1,2].includes(termination.code), 'Collector process did not terminate normally');
  assert.equal(typeof root, 'string');
  assert.equal(path.dirname(fs.realpathSync(root)), fs.realpathSync(os.tmpdir()));
  assert.match(path.basename(root), /^hello-agent-packaged-ui-[A-Za-z0-9]+$/);
  const result = JSON.parse(fs.readFileSync(path.join(root, 'result.json'), 'utf8'));
  assert.equal(result.root, root);
  const sample = {index, warmup:index === 0, termination, result};
  report.trials.push(sample); persist();
  assert.equal(result.shutdown, 'confirmed');
  assert.deepEqual(result.termination, {code:0, signal:null});
  assert.equal(result.head, report.head); assert.equal(result.headAfter, report.head);
  if (index > 0) {
    assert.equal(result.asarSha256, report.trials[0].result.asarSha256);
    assert.equal(result.sidecarSha256, report.trials[0].result.sidecarSha256);
  }
  const metric = mode === '--diff' ? result.largeDiff : result.largeProject;
  assert.ok(metric?.values, 'Incomplete measurement; fixture retained');
  const limitError = `Large ${mode === '--diff' ? 'diff' : 'project'} limits exceeded: ${metric.failedMetrics?.join(', ')}`;
  if (result.error && result.error !== limitError) throw new Error('Observation failed; fixture retained');
  if (mode === '--diff') {
    assert.equal(metric.diffFiles, 500); assert.equal(metric.diffChangedLines, 20000);
    assert.equal(metric.diffBytes, 10485760); assert.equal(metric.originalUnchanged, true);
  } else {
    assert.deepEqual(metric.fixture, {treeFiles:10000, treeDirectories:1000, controlFiles:2, fileBytes:5242880, fileLines:100000});
  }
  // Cleanup only these newly generated fixture children, never evidence/root.
  const live = new Set(execFileSync('/bin/ps', ['-axo','pid='], {encoding:'utf8'}).trim().split(/\s+/).map(Number));
  const identities = [result.pid, result.project.seedPid, ...result.memory.flatMap(row => row.processes.map(process => process.pid))];
  assert.ok(identities.every(pid => !live.has(pid)), 'Observed process still live; fixture retained');
  for (const name of ['project','other-project','userdata']) {
    const target = path.join(root, name), stat = fs.lstatSync(target);
    assert.ok(stat.isDirectory() && !stat.isSymbolicLink());
    fs.rmSync(target, {recursive:true});
  }
  sample.fixtureCleaned = true; persist();
  console.log(JSON.stringify({stage:'TRIAL_FINISHED', index, status:result.status, values:metric.values}));
}
try {
  for (let index = 0; index <= report.measuredRuns; index++) await trial(index);
  const measured = report.trials.filter(row => !row.warmup);
  const group = mode === '--diff' ? 'largeDiff' : 'largeProject';
  report.metrics = {};
  for (const key of Object.keys(measured[0].result[group].values)) {
    const values = measured.map(row => row.result[group].values[key]);
    assert.ok(values.every(value => Number.isFinite(value) && value >= 0));
    report.metrics[key] = {values, p95:Math.max(...values)};
  }
  report.status = report.trials.some(row => row.result.status === 'FAIL') ? 'FAIL' : 'NOT_READY';
} catch (error) { report.status = 'FAIL'; report.error = String(error); }
finally { report.headAfter = git('rev-parse','HEAD'); persist(); }
console.log(JSON.stringify({output, status:report.status, metrics:report.metrics, error:report.error}));
process.exitCode = report.status === 'FAIL' ? 1 : 2;
