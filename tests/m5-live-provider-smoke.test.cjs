const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../scripts/m5-live-provider-smoke.cjs'), 'utf8');

function harness() {
  const events = new Map();
  const timers = new Map();
  const exits = [];
  let evidence;
  const app = {
    setPath() {}, on() {}, quit() {},
    once(name, callback) { events.set(name, callback); },
    exit(code) { exits.push(code); },
  };
  const context = vm.createContext({
    __dirname: path.resolve(__dirname, '../scripts'),
    process: {env:{HELLO_AGENT_APPROVED_LIVE_SMOKE:'1',HELLO_AGENT_LIVE_USER_DATA:'/synthetic/userdata'}},
    console: {log() {}},
    setTimeout(callback, delay) { timers.set(delay, callback); return delay; },
    clearTimeout(id) { timers.delete(id); },
    require(name) {
      if (name === 'electron') return {app,dialog:{}};
      if (name === 'node:fs') return {
        statSync: () => ({isDirectory: () => true}),
        mkdtempSync: () => '/synthetic/run', mkdirSync() {}, realpathSync: value => value,
        writeFileSync(file, value) { if (file.endsWith('result.json')) evidence = JSON.parse(value); },
      };
      if (name === 'node:child_process') return {execFileSync: () => ''};
      if (name.endsWith('/.vite/build/main.cjs')) return {};
      return require(name);
    },
  });
  vm.runInContext(source, context);
  vm.runInContext(`outcomes.push(...['codex','claude'].map(provider=>({provider,state:'succeeded',terminationOutcome:'cleanExit',responseObserved:true})));finish();`, context);
  return {events,timers,exits,evidence: () => evidence};
}

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
