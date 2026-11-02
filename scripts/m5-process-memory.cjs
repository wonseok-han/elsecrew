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

// The collector's exact PID comes from the spawned child, never from a command-name
// filter: a real agent may legitimately execute its own ps child.
function processTreeMemoryWithoutCollector(output, rootPid, collectorPid) {
  if (!Number.isSafeInteger(collectorPid) || collectorPid <= 0 || collectorPid === rootPid) throw new Error("invalid collector PID");
  const snapshot = processTreeMemory(output, rootPid);
  const collector = snapshot.processes.find(row => row.pid === collectorPid);
  if (!collector || collector.parentPid !== rootPid) throw new Error("collector is not a direct child of measurement root");
  const processes = snapshot.processes.filter(row => row.pid !== collectorPid);
  const rssBytes = processes.reduce((sum, row) => sum + row.rssBytes, 0);
  if (!Number.isSafeInteger(rssBytes) || rssBytes <= 0) throw new Error("invalid app RSS");
  return {rssBytes, processes};
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
// ps lstart is only second-resolution. This retains observed orphans without
// claiming kernel-grade process identity or coverage between samples.
function createProcessMemoryTracker(rootPid) {
  if (!Number.isSafeInteger(rootPid) || rootPid < 1) throw new Error('invalid root PID');
  const known = new Map();
  let rootStart;
  return output => {
    const rows = output.trim().split(/\r?\n/).filter(Boolean).map(line => {
      const match = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+([A-Za-z]{3}\s+[A-Za-z]{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s+\d{4})\s*$/.exec(line);
      if (!match) throw new Error('invalid identified process memory row');
      const [pid, parentPid, rssKiB] = match.slice(1, 4).map(Number);
      const startedAt = match[4].replace(/\s+/g, ' ');
      if (![pid, parentPid, rssKiB].every(Number.isSafeInteger) || pid < 1
          || !Number.isSafeInteger(rssKiB * 1024) || !Number.isFinite(Date.parse(startedAt))) {
        throw new Error('invalid identified process memory number');
      }
      return {pid, parentPid, rssKiB, startedAt};
    });
    if (new Set(rows.map(row => row.pid)).size !== rows.length) throw new Error('duplicate process identity');
    const root = rows.find(row => row.pid === rootPid);
    if (!root) throw new Error('measurement root missing');
    if (rootStart !== undefined && root.startedAt !== rootStart) throw new Error('measurement root changed');
    rootStart = root.startedAt;
    const selected = new Set([rootPid]);
    for (const row of rows) if (known.get(row.pid) === row.startedAt) selected.add(row.pid);
    let changed;
    do {
      changed = false;
      for (const row of rows) if (selected.has(row.parentPid) && !selected.has(row.pid)) {
        selected.add(row.pid); changed = true;
      }
    } while (changed);
    const retained = rows.filter(row => selected.has(row.pid));
    // Reuse the bounded numeric/RSS validation. All retained identities are
    // explicitly selected, including formerly observed reparented children.
    const snapshot = processTreeMemory(retained.map(row =>
      `${row.pid} ${row.pid === rootPid ? row.parentPid : rootPid} ${row.rssKiB}`).join('\n'), rootPid);
    const processes = retained.map(row => ({pid:row.pid, parentPid:row.parentPid,
      rssBytes:row.rssKiB * 1024, startedAt:row.startedAt}));
    known.clear();
    for (const row of retained) known.set(row.pid, row.startedAt);
    return {rssBytes:snapshot.rssBytes, processes, identityResolution:'ps-lstart-seconds'};
  };
}
module.exports = { processTreeMemoryWithoutCollector, processTreeMemory, summarizeMemory, createProcessMemoryTracker };


// QA collectors must not synchronously stall the measured Electron main loop.
function collectProcessTreeMemory(rootPid) {
  return new Promise((resolve, reject) => {
    const child = require('node:child_process').execFile('/bin/ps', ['-axo', 'pid=,ppid=,rss='],
      {encoding:'utf8', timeout:2000, maxBuffer:4*1024*1024}, (error, stdout) => {
        if (error) { reject(error); return; }
        try { resolve(processTreeMemoryWithoutCollector(stdout, rootPid, child.pid)); }
        catch (error) { reject(error); }
      });
  });
}
function createProcessMemorySampler({rootPid, getPhase, onSample, onError, collect=collectProcessTreeMemory}) {
  let pending, stopped=false;
  return {
    tick() {
      if (stopped) return Promise.resolve();
      if (pending) { onError(new Error('RSS collection overlapped sampling interval')); return pending; }
      const phase=getPhase(), at=Date.now(), start=performance.now();
      pending=Promise.resolve().then(()=>collect(rootPid)).then(snapshot=>{
        if (getPhase() !== phase) throw new Error('RSS sample crossed a phase boundary');
        onSample({phase, at, completedAt:Date.now(), collectionMs:performance.now()-start,
          processes:snapshot.processes,totalBytes:snapshot.rssBytes});
      }).catch(onError).finally(()=>{pending=undefined;});
      return pending;
    },
    async drain() { await pending; },
    async stop() { stopped=true; await pending; },
  };
}
module.exports.collectProcessTreeMemory = collectProcessTreeMemory;
module.exports.createProcessMemorySampler = createProcessMemorySampler;
