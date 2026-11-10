// Post-measurement diagnosis only. No addresses, paths, dumps, or forced GC.
const assert = require('node:assert/strict');
function summarizeRegions(text) {
  assert.ok(typeof text === 'string' && Buffer.byteLength(text) <= 4 * 1024 * 1024, 'Native map exceeds bound');
  assert.match(text, /\[\s*VSIZE\s+RSDNT\s+DIRTY\s+SWAP\s*\]/, 'Unknown native map columns');
  const groups = new Map();
  let regions = 0;
  const bytes = value => {
    const match = /^(\d+(?:\.\d+)?)([BKMG])?$/.exec(value);
    assert.ok(match && (match[2] || /^\d+$/.test(match[1])), 'Unknown native size');
    const result = Math.round(Number(match[1]) * 1024 ** 'BKMG'.indexOf(match[2] ?? 'B'));
    assert.ok(Number.isSafeInteger(result), 'Native size overflow');
    return result;
  };
  for (const line of text.split('\n')) {
    const row = /^(.+?)\s+[0-9a-f]+-[0-9a-f]+\s+\[\s*(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s*\]/i.exec(line);
    if (!row) continue;
    assert.ok(++regions <= 100000, 'Too many native regions');
    const type = row[1].trim();
    assert.ok(type.length <= 128 && /^[A-Za-z_][A-Za-z0-9_ ():.+-]*$/.test(type), 'Unknown native region type');
    if (!groups.has(type)) {
      assert.ok(groups.size < 512, 'Too many native region types');
      groups.set(type, { type, count: 0, virtualBytes: 0, residentBytes: 0, dirtyBytes: 0, swappedBytes: 0 });
    }
    const group = groups.get(type); group.count++;
    for (const [index, field] of ['virtualBytes', 'residentBytes', 'dirtyBytes', 'swappedBytes'].entries()) {
      group[field] += bytes(row[index + 2]);
      assert.ok(Number.isSafeInteger(group[field]), 'Native sum overflow');
    }
  }
  assert.ok(regions > 0, 'No native regions observed');
  return { regions, groups: [...groups.values()] };
}
async function collectRetainedNativeMemory(targets, execute = require('node:util').promisify(require('node:child_process').execFile)) {
  assert.ok(Array.isArray(targets) && targets.length > 0 && targets.length <= 3);
  assert.equal(new Set(targets.map(row => row.pid)).size, targets.length);
  for (const row of targets) assert.ok(Number.isSafeInteger(row.pid) && row.pid > 0 && ['main', 'renderer', 'gpu'].includes(row.role));
  const report = { diagnostic: true, gateEligible: false, scope: 'After all timed measurements; native region residency is not RSS or additive physical memory. No forced GC or RSS subtraction.', processes: [] };
  for (const target of targets) {
    const startedAt = Date.now();
    let stage = 'execute';
    try {
      const { stdout } = await execute('/usr/bin/vmmap', ['-wide', String(target.pid)], { encoding: 'utf8', timeout: 5000, maxBuffer: 4 * 1024 * 1024 });
      stage = 'parse';
      report.processes.push({ ...target, startedAt, completedAt: Date.now(), ...summarizeRegions(stdout) });
    } catch (error) {
      // Fixed classification and bounded exit metadata only; never stderr,
      // exception text, command strings, raw maps, or process paths.
      report.processes.push({ ...target, startedAt, completedAt: Date.now(), error: 'nativeMapUnavailableOrInvalid', stage,
        exitCode: Number.isSafeInteger(error?.code) ? error.code : null,
        timedOut: error?.killed === true && error?.signal === 'SIGTERM',
        permissionDenied: error?.code === 'EPERM' || error?.code === 'EACCES',
        outputLimitExceeded: error?.code === 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER',
      });
    }
  }
  return report;
}
module.exports = { summarizeRegions, collectRetainedNativeMemory };
