// Collect the existing native protocol diagnostic; never promote it to G6 PASS.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';
import { qaEnvironment, lowerQaPriority } from './m5-qa-resources.mjs';
import { assertMeasurementSourceUnchanged } from './m5-measurement-source.mjs';

export function parseNativeObservationLine(line) {
  // libtest's sequential runner prefixes stdout with the exact test name.
  const json = line.replace(/^test (?:binary_cancel_clean_proves_fixture_parent_and_descendant_gone|binary_cancel_uncertain_blocks_then_abandon_resolution_clears_quit) \.\.\. /, '');
  try { return JSON.parse(json); } catch { return null; }
}

export function validateSaveRaceObservation(value) {
  assert.equal(value?.evidence, 'native-workspace-save-races');
  assert.equal(value.diagnostic, true);
  assert.equal(value.gateEligible, false);
  assert.equal(value.saveRaces, 100);
  assert.equal(value.controlSaves, 100);
  assert.equal(value.shutdown, 'accepted');
  assert.equal(value.observations?.length, 100);
  const chats = new Set();
  for (const [index, row] of value.observations.entries()) {
    assert.equal(row.trial, index);
    assert.equal(typeof row.chatId, 'string');
    assert.ok(row.chatId.length > 0);
    chats.add(row.chatId);
    assert.equal(row.chatId, value.observations[index % 2].chatId);
    assert.equal(row.externalWriteStage, index % 2 === 0 ? 'afterBegin' : 'afterChunk');
    for (const key of ['reportedConflict', 'externalBytesPreserved', 'controlSaveCommitted', 'otherChatUnchanged', 'sourceUnchanged']) {
      assert.equal(row[key], true, `Trial ${index}: ${key}`);
    }
  }
  assert.equal(chats.size, 2);
  assert.deepEqual(value.counters, {silentOverwrites:0, unreportedConflicts:0, crossChatWrites:0});
  return value;
}

export function validateTerminationObservations(values) {
  assert.equal(values.length, 2, 'Both clean and uncertain cancellation observations are required');
  const scenarios = new Set();
  for (const value of values) {
    assert.equal(value.evidence, 'fixture-cancel-external-process-observation');
    assert.ok(['cancel-clean', 'cancel-uncertain'].includes(value.scenario));
    assert.ok(!scenarios.has(value.scenario), 'Duplicate cancellation scenario');
    scenarios.add(value.scenario);
    assert.equal(value.diagnostic, true);
    assert.equal(value.gateEligible, false);
    assert.equal(value.shutdown, 'accepted');
    assert.ok(Number.isSafeInteger(value.observedDescendants) && value.observedDescendants >= 2);
    assert.ok(Number.isSafeInteger(value.detachedSessionDescendants) && value.detachedSessionDescendants >= 1 &&
      value.detachedSessionDescendants <= value.observedDescendants);
    assert.equal(value.survivingObservedProcesses, 0);
    assert.equal(value.uncertainReported, value.scenario === 'cancel-uncertain');
  }
  return values;
}

async function main() {
  const terminationMode = process.argv.length === 3 && process.argv[2] === '--termination';
  assert.ok(process.argv.length === 2 || terminationMode, 'Only --termination is supported');
  assert.equal(process.platform, 'darwin', 'macOS native diagnostic only');
  const repo = fileURLToPath(new URL('..', import.meta.url));
  const git = (...args) => execFileSync('/usr/bin/git', args, {cwd:repo, encoding:'utf8'}).trim();
  const source = () => ({head:git('rev-parse', 'HEAD'), gitStatus:git('status', '--porcelain')});
  const root = fs.mkdtempSync(path.join(os.tmpdir(), terminationMode ? 'elsecrew-termination-metrics-' : 'elsecrew-save-race-metrics-'));
  const report = {schemaVersion:1, diagnostic:true, gateEligible:false, status:'RUNNING',
    sourceBefore:source(), measuredAt:new Date().toISOString(), priority:lowerQaPriority(),
    mode:terminationMode ? 'fixture-termination' : 'save-races',
    limitations:terminationMode
      ? ['Session-detached fixtures, not independently reparented orphans or real providers.',
        'External observer shares the core OS probe implementation; not full G6 evidence.']
      : ['Native release sidecar protocol only, not packaged UI or agent writers.',
        '100 controlled pre-commit interleavings, not arbitrary scheduling or descendant termination coverage.']};
  const persist = () => fs.writeFileSync(path.join(root, 'measurement.json'), JSON.stringify(report, null, 2));
  persist(); console.log(JSON.stringify({stage:'COLLECTION_STARTED', root}));
  const log = fs.createWriteStream(path.join(root, 'cargo.log'));
  let logError;
  log.on('error', error => { logError = String(error); });
  let pending = '', parseFailure, observations = [];
  const args = ['test', '--release', '--locked', '-p', 'elsecrew-sidecar', '--test', 'stdio_process',
    ...(terminationMode ? ['binary_cancel_', '--', '--test-threads=1', '--nocapture']
      : ['binary_workspace_save_races_preserve_external_bytes_across_two_chats', '--', '--exact', '--ignored', '--nocapture'])];
  const expectedEvidence = terminationMode ? 'fixture-cancel-external-process-observation' : 'native-workspace-save-races';
  const expectedCount = terminationMode ? 2 : 1;
  report.command = ['cargo', ...args];
  const child = spawn('cargo', args, {cwd:repo, env:{...qaEnvironment(), CARGO_INCREMENTAL:'0'}, stdio:['ignore','pipe','pipe']});
  report.pid = child.pid; persist();
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', chunk => {
    log.write(chunk);
    if (parseFailure) return;
    pending += chunk;
    if (pending.length > 1024 * 1024) { parseFailure = 'Diagnostic line exceeded 1 MiB'; pending = ''; return; }
    let end;
    while ((end = pending.indexOf('\n')) !== -1) {
      const line = pending.slice(0, end); pending = pending.slice(end + 1);
      {
        const value = parseNativeObservationLine(line);
        if (value?.evidence === expectedEvidence) {
          if (observations.length >= expectedCount) parseFailure = 'Excess observations';
          else observations.push(value);
        }
      }
    }
  });
  child.stderr.on('data', chunk => log.write(chunk));
  report.termination = await new Promise(resolve => {
    child.once('error', error => resolve({error:String(error)}));
    child.once('close', (code, signal) => resolve({code, signal}));
  });
  // Never timeout-kill the test or remove its uncertain fixture.
  await new Promise(resolve => { if (log.destroyed) resolve(); else log.end(resolve); });
  report.observations = observations;
  try {
    assert.deepEqual(report.termination, {code:0, signal:null});
    assert.ok(!parseFailure && !logError, parseFailure ?? logError);
    assert.equal(observations.length, expectedCount, 'Missing native observations');
    if (terminationMode) validateTerminationObservations(observations);
    else validateSaveRaceObservation(observations[0]);
    report.sourceAfter = source();
    assertMeasurementSourceUnchanged(report.sourceBefore, report.sourceAfter);
    report.status = 'NOT_READY';
  } catch (error) { report.status = 'FAIL'; report.error = String(error); }
  persist(); console.log(JSON.stringify({root, status:report.status}));
  process.exitCode = report.status === 'FAIL' ? 1 : 2;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
