import { spawnSync } from 'node:child_process';
import { resolve, sep } from 'node:path';
import { realpathSync } from 'node:fs';

/** macOS packager deletes its old output before staging the replacement. */
export function assertPackageNotRunning(directory, { platform = process.platform, run = spawnSync } = {}) {
  if (platform !== 'darwin') return;
  const roots = new Set([resolve(directory)]);
  try { roots.add(realpathSync(directory)); } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  // comm contains executable paths, not arbitrary arguments or our ps query.
  const result = run('/bin/ps', ['-axo', 'pid=,comm='], { encoding: 'utf8', timeout: 5000, maxBuffer: 8 * 1024 * 1024 });
  if (result.error || result.status !== 0 || typeof result.stdout !== 'string' || !result.stdout.trim()) {
    throw new Error('Cannot verify running package processes; packaging was not started.');
  }
  const owners = [];
  for (const line of result.stdout.split('\n')) {
    if (!line.trim()) continue;
    const match = /^\s*(\d+)\s+(.+)$/.exec(line);
    if (!match) throw new Error('Invalid process inventory; packaging was not started.');
    if ([...roots].some(root => match[2] === root || match[2].startsWith(root + sep))) owners.push(Number(match[1]));
  }
  if (owners.length) throw new Error(`Close Elsecrew normally before packaging. Output is in use by PID(s): ${owners.join(', ')}. No process was terminated.`);
}
