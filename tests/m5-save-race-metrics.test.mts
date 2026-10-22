import assert from 'node:assert/strict';
import test from 'node:test';
import {validateSaveRaceObservation} from '../scripts/m5-save-race-metrics.mjs';

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
