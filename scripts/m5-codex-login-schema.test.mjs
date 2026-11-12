import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { compareLoginSchema, loginRoots } from './m5-codex-login-schema.mjs';

async function fixture(t) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'login-schema-test-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const dirs = [path.join(root, 'before'), path.join(root, 'after')];
  for (const dir of dirs) {
    await mkdir(path.join(dir, 'v2'), { recursive: true });
    for (const file of loginRoots) await writeFile(path.join(dir, file), 'export type Test = string;\n');
  }
  return dirs;
}

test('identical roots remain evidence, not launch authorization', async t => {
  const result = await compareLoginSchema(...await fixture(t));
  assert.equal(result.status, 'IDENTICAL');
  assert.equal(result.files.length, loginRoots.length);
  assert.match(result.scope, /not runtime compatibility/);
});

test('follows transitive imports, including cycles and candidate-only dependencies', async t => {
  const dirs = await fixture(t);
  for (const dir of dirs) {
    await writeFile(path.join(dir, loginRoots[0]), 'import type { A } from "./A";\n');
    await writeFile(path.join(dir, 'A.ts'), 'import type { B } from "./B";\n');
    await writeFile(path.join(dir, 'B.ts'), 'import type { A } from "./A";\n');
  }
  assert.equal((await compareLoginSchema(...dirs)).status, 'IDENTICAL');
  await writeFile(path.join(dirs[1], 'B.ts'), 'import type { C } from "./C";\n');
  await writeFile(path.join(dirs[1], 'C.ts'), 'export type C = number;\n');
  const result = await compareLoginSchema(...dirs);
  assert.equal(result.status, 'REVIEW_REQUIRED');
  assert.deepEqual(result.files.filter(f => !f.identical).map(f => f.name), ['B.ts', 'C.ts']);
});

test('missing files and escaping imports fail instead of reporting compatibility', async t => {
  const dirs = await fixture(t);
  await writeFile(path.join(dirs[1], loginRoots[0]), 'import type { A } from "../outside";\n');
  await assert.rejects(compareLoginSchema(...dirs), /outside root/);
  await writeFile(path.join(dirs[1], loginRoots[0]), 'import type { A } from "./missing";\n');
  await assert.rejects(compareLoginSchema(...dirs), { code: 'ENOENT' });
});
