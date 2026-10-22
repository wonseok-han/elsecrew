import assert from 'node:assert/strict';
import test from 'node:test';
import {validateSaveRaceObservation, validateTerminationObservations, parseNativeObservationLine} from '../scripts/m5-save-race-metrics.mjs';

function observation(): any {
  return {evidence:'native-workspace-save-races', diagnostic:true, gateEligible:false,
    saveRaces:100, controlSaves:100, shutdown:'accepted',
    counters:{silentOverwrites:0, unreportedConflicts:0, crossChatWrites:0},
    observations:Array.from({length:100}, (_, trial) => ({trial, chatId:`chat_${trial % 2}`,
      externalWriteStage:trial % 2 ? 'afterChunk' : 'afterBegin', reportedConflict:true,
      externalBytesPreserved:true, controlSaveCommitted:true, otherChatUnchanged:true, sourceUnchanged:true}))};
}
test('complete native observations remain diagnostic and retain all rows', () => {
  const value = observation();
  assert.equal(validateSaveRaceObservation(value), value);
  assert.equal(value.gateEligible, false);
});
test('missing rows, duplicate trials, wrong chat isolation and uncertain shutdown are rejected', () => {
  for (const mutate of [
    (v:any) => v.observations.pop(), (v:any) => v.observations[99].trial = 98,
    (v:any) => v.observations[2].chatId = 'third',
    (v:any) => v.observations.forEach((r:any) => r.chatId = 'same'),
    (v:any) => v.shutdown = 'unknown', (v:any) => v.controlSaves = 99,
    (v:any) => v.gateEligible = true, (v:any) => delete v.observations[0].sourceUnchanged,
  ]) { const value = observation(); mutate(value); assert.throws(() => validateSaveRaceObservation(value)); }
});
test('claimed zero counters cannot hide failed raw observations', () => {
  for (const key of ['reportedConflict', 'externalBytesPreserved', 'controlSaveCommitted', 'otherChatUnchanged', 'sourceUnchanged']) {
    const value = observation(); value.observations[50][key] = false;
    assert.throws(() => validateSaveRaceObservation(value));
  }
  const value = observation(); value.counters.silentOverwrites = 1;
  assert.throws(() => validateSaveRaceObservation(value));
});

function terminations(): any[] {
  return ['cancel-clean','cancel-uncertain'].map(scenario => ({
    evidence:'fixture-cancel-external-process-observation', scenario, diagnostic:true, gateEligible:false,
    observedDescendants:2, detachedSessionDescendants:1, survivingObservedProcesses:0,
    uncertainReported:scenario === 'cancel-uncertain', shutdown:'accepted',
  }));
}
test('native stdout parser accepts only plain JSON or exact sequential test prefixes', () => {
  const value = terminations()[0], json = JSON.stringify(value);
  assert.deepEqual(parseNativeObservationLine(json), value);
  for (const name of ['binary_cancel_clean_proves_fixture_parent_and_descendant_gone', 'binary_cancel_uncertain_blocks_then_abandon_resolution_clears_quit']) {
    assert.deepEqual(parseNativeObservationLine(`test ${name} ... ${json}`), value);
  }
  assert.equal(parseNativeObservationLine(`test unrelated ... ${json}`), null);
  assert.equal(parseNativeObservationLine(`arbitrary log ${json}`), null);
  assert.equal(parseNativeObservationLine('{'), null);
});
test('termination evidence requires both scenarios and preserves uncertainty reporting', () => {
  const values = terminations();
  assert.equal(validateTerminationObservations(values), values);
  for (const mutate of [
    (v:any[]) => v.pop(), (v:any[]) => v.push(v[0]), (v:any[]) => v[1] = v[0],
    (v:any[]) => v[1].uncertainReported = false, (v:any[]) => v[0].survivingObservedProcesses = 1,
    (v:any[]) => delete v[0].survivingObservedProcesses, (v:any[]) => v[0].detachedSessionDescendants = 0,
    (v:any[]) => v[0].observedDescendants = 1, (v:any[]) => v[1].shutdown = 'unknown',
    (v:any[]) => v[0].gateEligible = true,
  ]) { const changed = terminations(); mutate(changed); assert.throws(() => validateTerminationObservations(changed)); }
});
