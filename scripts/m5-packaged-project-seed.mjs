// Seed only caller-owned temporary fixtures through the real packaged sidecar.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { createInterface } from 'node:readline';
import { decodeJsonl, parseReadyPayload } from '../src/m5/contracts/protocol-v1.ts';

export async function seedPackagedProject(binary, root, userData, env) {
  const source = path.join(root, 'project');
  fs.mkdirSync(source);
  fs.writeFileSync(path.join(source, 'hello.txt'), 'original\n');
  for (const args of [['init', '-q', '--template='], ['add', '.'], ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Smoke', '-c', 'user.email=smoke@example.invalid', 'commit', '-qm', 'fixture']]) {
    execFileSync('/usr/bin/git', args, {cwd: source, env: {...env, GIT_CONFIG_NOSYSTEM:'1', GIT_CONFIG_GLOBAL:'/dev/null', GIT_CONFIG_COUNT:'0'}});
  }
  const directory = path.join(userData, 'com.wonseokhan.helloagent/m5');
  fs.mkdirSync(directory, {recursive: true});
  const child = spawn(binary, ['--stdio'], {cwd: directory, env, stdio:['pipe','pipe','pipe']});
  console.log(JSON.stringify({stage:'SEED_STARTED', pid:child.pid, root}));
  const closed = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => resolve({code, signal}));
  });
  void closed.catch(() => {});
  child.stderr.resume();
  child.stdin.on('error', () => {});
  const lines = createInterface({input:child.stdout, crlfDelay:Infinity});
  const iterator = lines[Symbol.asyncIterator]();
  let timer;
  const deadline = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('Seed sidecar deadline exceeded; preserve fixture')), 20000); });
  const next = async () => {
    const line = await Promise.race([iterator.next(), deadline]);
    assert.equal(line.done, false);
    return decodeJsonl(`${line.value}\n`);
  };
  try {
    const ready = parseReadyPayload(await next());
    assert.equal(ready.pid, child.pid);
    let sequence = 0;
    const request = async (method, params = {}) => {
      const requestId = `seed_${++sequence}`;
      child.stdin.write(`${JSON.stringify({protocolVersion:1, kind:'request', sidecarInstanceId:ready.sidecarInstanceId, requestId, method, params})}\n`);
      for (let count = 0; count < 100; count++) {
        const reply = await next();
        assert.equal(reply.sidecarInstanceId, ready.sidecarInstanceId);
        if (reply.kind === 'event') continue;
        assert.equal(reply.requestId, requestId);
        assert.equal(reply.ok, true, `Seed request failed: ${method}`);
        return reply.result;
      }
      throw new Error('Seed event budget exceeded');
    };
    while ((await request('system.status')).storageAdmission.state !== 'ready') await new Promise(resolve => setTimeout(resolve, 25));
    const project = await request('project.open', {selectedPath:source});
    const quit = await request('app.prepareQuit');
    assert.equal(quit.safeToQuit, true);
    assert.deepEqual(quit.blockers, []);
    child.stdin.end();
    assert.deepEqual(await Promise.race([closed, deadline]), {code:0, signal:null});
    return {source, projectId:project.projectId, seedPid:child.pid, shutdown:'confirmed'};
  } finally { clearTimeout(timer); lines.close(); child.stdin.end(); }
}
