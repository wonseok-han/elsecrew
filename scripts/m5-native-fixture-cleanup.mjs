import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

// Only called by the synthetic collector after the Electron child's close event.
// Authentication trials and product backup storage never use this helper.
export function cleanupCompletedNativeFixture(sample) {
  const result = sample.result;
  if (sample.signal || sample.error || ![0, 1].includes(sample.code)
    || result?.shutdown !== 'accepted'
    || result.status !== (sample.code === 0 ? 'PASS' : 'FAIL')) {
    throw new Error('Native fixture cleanup requires a completed, accepted shutdown');
  }
  const root = result.root;
  if (typeof root !== 'string' || !path.isAbsolute(root)
    || !/^elsecrew-project-ui-[a-zA-Z0-9]{6}$/.test(path.basename(root))
    || fs.realpathSync(path.dirname(root)) !== fs.realpathSync(os.tmpdir())) {
    throw new Error('Native fixture cleanup target is outside the synthetic temp namespace');
  }
  const rootStat = fs.lstatSync(root);
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) throw new Error('Native fixture root is not a real directory');
  const evidence = path.join(root, 'result.json');
  const evidenceStat = fs.lstatSync(evidence);
  if (!evidenceStat.isFile() || evidenceStat.isSymbolicLink() || evidenceStat.size > 1024 * 1024) {
    throw new Error('Native fixture result is not a bounded regular file');
  }
  const saved = JSON.parse(fs.readFileSync(evidence, 'utf8'));
  if (saved.root !== root || saved.shutdown !== result.shutdown || saved.status !== result.status) {
    throw new Error('Native fixture result differs from the completed trial');
  }
  const targets = ['project', 'userdata'].map(name => path.join(root, name));
  // Check all targets before removing either. rm does not follow nested symlinks.
  for (const target of targets) {
    const stat = fs.lstatSync(target);
    if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error('Native fixture child is not a real directory');
  }
  for (const target of targets) fs.rmSync(target, { recursive: true });
  return { removed: ['project', 'userdata'], evidenceRoot: root };
}
