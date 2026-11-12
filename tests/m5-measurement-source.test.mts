import test from 'node:test';
import assert from 'node:assert/strict';
import { assertMeasurementSourceUnchanged } from '../scripts/m5-measurement-source.mjs';

test('measurement preserves unchanged clean or diagnostic dirty source', () => {
  for (const gitStatus of ['', ' M src/example.ts']) {
    const source = {head:'abc123', gitStatus};
    assert.doesNotThrow(() => assertMeasurementSourceUnchanged(source, {...source}));
  }
});

test('measurement rejects changed HEAD, working tree and missing evidence', () => {
  const source = {head:'abc123', gitStatus:''};
  for (const after of [
    {head:'def456', gitStatus:''},
    {head:'abc123', gitStatus:' M src/example.ts'},
    {head:'abc123'},
  ]) assert.throws(() => assertMeasurementSourceUnchanged(source, after));
  assert.throws(() => assertMeasurementSourceUnchanged({head:'', gitStatus:''}, source));
});
