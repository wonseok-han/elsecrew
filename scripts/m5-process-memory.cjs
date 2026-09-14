// ps RSS is in KiB. Keep only the measured root and its current descendants.
function processTreeMemory(output, rootPid) {
  const rows = output.trim().split(/\r?\n/).filter(Boolean).map(line => {
    const match = /^\s*(\d+)\s+(\d+)\s+(\d+)\s*$/.exec(line);
    if (!match) throw new Error("invalid process memory row");
    const [pid, parentPid, rssKiB] = match.slice(1).map(Number);
    if (![pid, parentPid, rssKiB].every(Number.isSafeInteger) || pid < 1 || !Number.isSafeInteger(rssKiB * 1024)) throw new Error("invalid process memory number");
    return { pid, parentPid, rssBytes: rssKiB * 1024 };
  });
  if (new Set(rows.map(row => row.pid)).size !== rows.length) throw new Error("duplicate process identity");
  if (!rows.some(row => row.pid === rootPid)) throw new Error("measurement root missing");
  const selected = new Set([rootPid]);
  let changed;
  do {
    changed = false;
    for (const row of rows) if (selected.has(row.parentPid) && !selected.has(row.pid)) { selected.add(row.pid); changed = true; }
  } while (changed);
  const processes = rows.filter(row => selected.has(row.pid));
  const rssBytes = processes.reduce((sum, row) => sum + row.rssBytes, 0);
  if (!Number.isSafeInteger(rssBytes) || rssBytes <= 0) throw new Error("invalid total RSS");
  return { rssBytes, processes };
}

function summarizeMemory(samples) {
  const values = phase => samples.filter(sample => sample.phase === phase).map(sample => sample.rssBytes);
  const idle = values("idle"), loaded = values("loaded"), retained = values("retained");
  if (!idle.length || !loaded.length || !retained.length || samples.some(sample => !Number.isSafeInteger(sample.rssBytes) || sample.rssBytes <= 0)) throw new Error("incomplete memory samples");
  const idleRssBytes = Math.max(...idle);
  const peakRssBytes = Math.max(...samples.map(sample => sample.rssBytes));
  const retainedRssBytes = Math.max(0, Math.max(...retained) - idleRssBytes);
  const maxima = { idleRssBytes, peakRssBytes, retainedRssBytes };
  const limits = { idleRssBytes: 400 * 1024 ** 2, peakRssBytes: 800 * 1024 ** 2, retainedRssBytes: 120 * 1024 ** 2 };
  return { maxima, failedMetrics: Object.keys(limits).filter(key => maxima[key] > limits[key]) };
}
module.exports = { processTreeMemory, summarizeMemory };
