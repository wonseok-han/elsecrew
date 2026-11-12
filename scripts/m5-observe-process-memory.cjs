// Read-only macOS observer. Never starts, stops, or injects into the measured app.
const { execFileSync } = require('node:child_process');
const { mkdtempSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { join } = require('node:path');
const { processTreeMemory } = require('./m5-process-memory.cjs');

async function observe(rootPid, count, phase, read, sleep) {
  if (!Number.isSafeInteger(rootPid) || rootPid < 1 || !Number.isSafeInteger(count) || count < 1 || count > 240
      || !['idle', 'loaded', 'retained'].includes(phase)) throw new Error('invalid observation arguments');
  const report = { schemaVersion: 1, diagnostic: true, gateEligible: false, status: 'OBSERVING', rootPid, phase,
    intervalMs: 250, scope: 'Current root descendants only; escaped processes and PID reuse are not verified', samples: [] };
  try {
    for (let i = 0; i < count; i++) {
      report.samples.push({ at: Date.now(), ...processTreeMemory(await read(), rootPid) });
      if (i + 1 < count) await sleep(250);
    }
    report.maxRssBytes = Math.max(...report.samples.map(sample => sample.rssBytes));
    report.status = 'OBSERVED'; // Collection success, never a G6 PASS.
  } catch (error) {
    report.status = 'FAIL'; report.error = String(error);
  }
  return report;
}

if (require.main === module) {
  void (async () => {
    if (process.platform !== 'darwin' || process.argv.length !== 5) throw new Error('usage: node scripts/m5-observe-process-memory.cjs PID COUNT idle|loaded|retained (macOS)');
    const report = await observe(Number(process.argv[2]), Number(process.argv[3]), process.argv[4],
      () => execFileSync('/bin/ps', ['-axo', 'pid=,ppid=,rss='], { encoding: 'utf8', timeout: 5000, maxBuffer: 4 * 1024 * 1024 }),
      ms => new Promise(resolve => setTimeout(resolve, ms)));
    const directory = mkdtempSync(join(tmpdir(), 'elsecrew-rss-observation-'));
    const evidence = join(directory, 'observation.json');
    writeFileSync(evidence, JSON.stringify(report, null, 2), { flag: 'wx', mode: 0o600 });
    console.log(JSON.stringify({ status: report.status, diagnostic: true, gateEligible: false, evidence,
      samples: report.samples.length, maxRssBytes: report.maxRssBytes, error: report.error }));
    process.exitCode = report.status === 'OBSERVED' ? 0 : 1;
  })().catch(error => { console.error(String(error)); process.exitCode = 1; });
}
module.exports = { observe };
