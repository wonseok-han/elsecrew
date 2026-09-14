// Offline comparison only. Never runs a vendor or grants launch/version admission.
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export const loginRoots = [
  'InitializeParams.ts', 'InitializeResponse.ts',
  'v2/LoginAccountParams.ts', 'v2/LoginAccountResponse.ts',
  'v2/CancelLoginAccountParams.ts', 'v2/CancelLoginAccountResponse.ts',
  'v2/GetAccountParams.ts', 'v2/GetAccountResponse.ts',
  'v2/AccountLoginCompletedNotification.ts', 'v2/AccountUpdatedNotification.ts',
];

async function snapshot(root) {
  const files = new Map();
  const pending = [...loginRoots];
  while (pending.length) {
    const name = pending.pop();
    if (files.has(name)) continue;
    const source = await readFile(path.join(root, name), 'utf8');
    files.set(name, createHash('sha256').update(source).digest('hex'));
    // ts-rs emits static relative imports. Compare dependencies too: matching
    // root files alone cannot establish that the referenced types are unchanged.
    for (const match of source.matchAll(/^import\s+type\s+.*?\s+from\s+["']([^"']+)["'];?$/gm)) {
      const specifier = match[1];
      if (!specifier.startsWith('.')) throw new Error('non-relative schema dependency');
      const dependency = path.posix.normalize(path.posix.join(path.posix.dirname(name), specifier));
      if (dependency.startsWith('../') || path.posix.isAbsolute(dependency)) {
        throw new Error('schema dependency outside root');
      }
      pending.push(dependency.endsWith('.ts') ? dependency : `${dependency}.ts`);
    }
  }
  return files;
}

export async function compareLoginSchema(before, after) {
  const [baseline, candidate] = await Promise.all([snapshot(before), snapshot(after)]);
  const files = [...new Set([...baseline.keys(), ...candidate.keys()])].sort().map(name => ({
    name, before: baseline.get(name) ?? null, after: candidate.get(name) ?? null,
    identical: baseline.has(name) && baseline.get(name) === candidate.get(name),
  }));
  return {
    status: files.every(file => file.identical) ? 'IDENTICAL' : 'REVIEW_REQUIRED',
    scope: 'login schema and imported types only; not runtime compatibility or launch authorization',
    files,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 4) throw new Error('expected two generated schema directories');
    const result = await compareLoginSchema(process.argv[2], process.argv[3]);
    console.log(JSON.stringify(result, null, 2));
    process.exitCode = result.status === 'IDENTICAL' ? 0 : 2;
  } catch {
    console.error('Schema comparison unavailable: provide two complete generated schema directories.');
    process.exitCode = 1;
  }
}
