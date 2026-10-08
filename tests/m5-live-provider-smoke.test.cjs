const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../scripts/m5-live-provider-smoke.cjs'), 'utf8');

function harness(catalogOnly = false, autoFinish = true, projectUi = false, selectedProvider = undefined) {
  const events = new Map();
  const timers = new Map();
  const exits = [];
  let evidence;
  const spawned = [];
  const childProcess = {execFileSync: () => '', spawn(...args) {
    const child = {stderr: new (require('node:events').EventEmitter)()};
    spawned.push({args, child});
    return child;
  }};
  const app = {
    setPath() {}, on(name, callback) { events.set(name, callback); }, quit() {},
    once(name, callback) { events.set(name, callback); },
    exit(code) { exits.push(code); },
  };
  const context = vm.createContext({
    __dirname: path.resolve(__dirname, '../scripts'),
    process: {platform:'darwin',env:{HELLO_AGENT_APPROVED_LIVE_SMOKE:'1',HELLO_AGENT_LIVE_USER_DATA:'/synthetic/userdata',HELLO_AGENT_LIVE_MODEL_CHOICES_ONLY:catalogOnly?'1':'0',HELLO_AGENT_LIVE_PROVIDER:selectedProvider}},
    console: {log() {}},
    setTimeout(callback, delay) { timers.set(delay, callback); return delay; },
    clearTimeout(id) { timers.delete(id); },
    clearInterval() {},
    require(name) {
      if (name === 'electron') return {app,dialog:{}};
      if (name === 'node:fs') return {
        statSync: () => ({isDirectory: () => true}),
        mkdtempSync: () => '/synthetic/run', mkdirSync() {}, realpathSync: value => value,
        writeFileSync(file, value) { if (file.endsWith('result.json')) evidence = JSON.parse(value); },
      };
      if (name === 'node:child_process') return childProcess;
      if (name.endsWith('/.vite/build/main.cjs')) return {};
      if (name === './m5-process-memory.cjs') return {};
      return require(name);
    },
  });
  vm.runInContext(projectUi ? fs.readFileSync(path.join(__dirname, '../scripts/m5-project-ui-smoke.cjs'), 'utf8') : source, context);
  if (projectUi) vm.runInContext('finish(null);', context);
  if (autoFinish && !projectUi) vm.runInContext(catalogOnly
    ? `outcomes.push({provider:'codex',state:'catalogObserved',models:[]});finish();`
    : `outcomes.push(...providers.map(provider=>({provider,state:'succeeded',terminationOutcome:'cleanExit',responseObserved:true})));finish();`, context);
  return {events,timers,exits,evidence: () => evidence, childProcess, spawned};
}

test('transport diagnostics collect only fixed sidecar labels across chunks', () => {
  const h = harness(false, false);
  const options = {stdio:'pipe'};
  const child = h.childProcess.spawn('/synthetic/hello-agent-sidecar', ['--stdio'], options);
  assert.equal(child, h.spawned[0].child);
  assert.equal(h.spawned[0].args[2], options);
  child.stderr.emit('data', Buffer.from('PRIVATE_TOKEN\nnative Codex transport failed at turn sta'));
  child.stderr.emit('data', Buffer.from('rt\nnative Codex transport failed at turn stream\n'));
  child.stderr.emit('data', Buffer.from('native Codex transport failed at turn start\nnative Codex transport failed at PRIVATE_TOKEN\n'));
  child.stderr.emit('data', Buffer.from('native Claude transport failed at session initialization\n'));
  child.stderr.emit('data', Buffer.from('native Claude transport failed at prompt admission\n'));
  child.stderr.emit('data', Buffer.from('native Codex transport failed at turn start reply shape\nnative Claude transport failed at session persistence\n'));
  child.stderr.emit('data', Buffer.from('native Claude transport failed at init model\nnative Claude transport failed at PRIVATE_TOKEN\n'));
  child.stderr.emit('data', Buffer.from('native Codex transport failed at turn stream token usage\n'));
  const unrelated = h.childProcess.spawn('/synthetic/other', []);
  unrelated.stderr.emit('data', Buffer.from('native Codex transport failed at thread/start\n'));
  h.timers.get(240000)();
  assert.deepEqual(h.evidence().transportCheckpoints, ['codex:turn start', 'codex:turn stream', 'claude:session initialization', 'claude:prompt admission', 'codex:turn start reply shape', 'claude:session persistence', 'claude:init model', 'codex:turn stream token usage']);
  assert.ok(!JSON.stringify(h.evidence()).includes('PRIVATE_TOKEN'));
  assert.equal(h.evidence().status, 'PENDING');
});

test('a single approved provider produces explicitly scoped evidence', () => {
  const h = harness(false, true, false, 'codex');
  h.events.get('will-quit')();
  assert.equal(h.evidence().status, 'PASS');
  assert.deepEqual(h.evidence().providers, ['codex']);
  assert.deepEqual(h.evidence().outcomes.map(o => o.provider), ['codex']);
  assert.throws(() => harness(false, false, false, 'unknown'), /Unknown live provider/);
});

test('project UI smoke also requires accepted shutdown before PASS', () => {
  const h = harness(false, true, true);
  assert.equal(h.evidence().status, 'PENDING');
  assert.deepEqual(h.exits, []);
  h.events.get('will-quit')();
  assert.equal(h.evidence().status, 'PASS');
  assert.equal(h.evidence().shutdown, 'accepted');
  assert.deepEqual(h.exits, [0]);
});

test('project UI smoke preserves blocked shutdown without forcing exit', () => {
  const h = harness(false, true, true);
  h.timers.get(15000)();
  assert.equal(h.evidence().status, 'FAIL');
  assert.equal(h.evidence().shutdown, 'notConfirmed');
  assert.deepEqual(h.exits, []);
});

test('watchdog finish prevents late UI response from starting another request', async () => {
  const h = harness(false, false);
  let loaded;
  let resolveRequest;
  let requests = 0;
  h.events.get('browser-window-created')(null, {webContents: {
    setBackgroundThrottling() {},
    once(_name, callback) { loaded = callback; },
    executeJavaScript() {
      requests += 1;
      return new Promise(resolve => { resolveRequest = resolve; });
    },
  }});
  const pending = loaded();
  assert.equal(requests, 1);
  h.timers.get(240000)();
  resolveRequest(true);
  await pending;
  assert.equal(requests, 1);
  assert.equal(h.evidence().error, 'timeout');
  assert.equal(h.evidence().shutdown, 'pending');
  assert.deepEqual(h.exits, []);
});

test('a page loaded after watchdog shutdown sends no UI requests', async () => {
  const h = harness(false, false);
  let loaded;
  let requests = 0;
  h.events.get('browser-window-created')(null, {webContents: {
    setBackgroundThrottling() {},
    once(_name, callback) { loaded = callback; },
    executeJavaScript() { requests += 1; return Promise.resolve(true); },
  }});
  h.timers.get(240000)();
  await loaded();
  assert.equal(requests, 0);
  assert.equal(h.evidence().error, 'timeout');
});

test('live smoke cannot pass before normal shutdown is accepted', () => {
  const h = harness();
  assert.equal(h.evidence().status, 'PENDING');
  assert.deepEqual(h.exits, []);
  h.events.get('will-quit')();
  assert.equal(h.evidence().status, 'PASS');
  assert.equal(h.evidence().shutdown, 'accepted');
  assert.equal(h.timers.has(15000), false);
  assert.deepEqual(h.exits, [0]);
});

test('blocked shutdown writes failure without forcing process exit', () => {
  const h = harness();
  h.timers.get(15000)();
  assert.equal(h.evidence().status, 'FAIL');
  assert.equal(h.evidence().shutdown, 'notConfirmed');
  assert.deepEqual(h.exits, []);
});

test('catalog-only evidence cannot be mistaken for actual model-run evidence', () => {
  const h = harness(true);
  assert.equal(h.evidence().verificationKind, 'modelChoicesOnly');
  assert.equal(h.evidence().status, 'PENDING');
  h.events.get('will-quit')();
  assert.equal(h.evidence().status, 'PASS');
  assert.equal(h.evidence().outcomes[0].state, 'catalogObserved');
  assert.equal(h.evidence().outcomes[0].responseObserved, undefined);
});
