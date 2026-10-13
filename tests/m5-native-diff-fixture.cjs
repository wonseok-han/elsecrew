const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const folder = 'native-diff-fixture';
const names = Array.from({length:500}, (_, i) => `file-${String(i).padStart(3, '0')}.txt`);
function prepare(root) {
  fs.mkdirSync(path.join(root, folder));
  for (const name of names) fs.writeFileSync(path.join(root, folder, name), ('before-' + 'x'.repeat(493) + '\n').repeat(20), {flag:'wx'});
}
function change(root) {
  const env = {...process.env, GIT_CONFIG_NOSYSTEM:'1', GIT_CONFIG_GLOBAL:'/dev/null', GIT_CONFIG_COUNT:'0'};
  const diff = () => execFileSync('/usr/bin/git', ['-c','core.hooksPath=/dev/null','-c','diff.algorithm=myers','-C',root,'diff','--no-ext-diff','--no-textconv','--no-renames','--no-color','--src-prefix=a/','--dst-prefix=b/','HEAD','--',folder], {env,maxBuffer:12*1024*1024});
  for (const name of names) {
    const file = path.join(root, folder, name);
    if (fs.readFileSync(file,'utf8') !== ('before-' + 'x'.repeat(493) + '\n').repeat(20)) throw new Error('fixture baseline changed');
    fs.writeFileSync(file, ('after--' + 'y'.repeat(493) + '\n').repeat(20));
  }
  const padding = 10*1024*1024 - diff().length;
  if (padding < 0 || padding > 1024*1024) throw new Error('unexpected native diff overhead');
  const last = path.join(root, folder, names.at(-1));
  const text = fs.readFileSync(last,'utf8');
  fs.writeFileSync(last, text.slice(0,-1) + 'z'.repeat(padding) + '\n');
  const patch = diff();
  const lines = patch.toString('utf8').split('\n');
  const changedLines = lines.filter(line => /^[+-]/.test(line) && !line.startsWith('+++') && !line.startsWith('---')).length;
  if (patch.length !== 10*1024*1024 || changedLines !== 20000 || lines.filter(line=>line.startsWith('diff --git ')).length !== 500) throw new Error('native diff fixture dimensions changed');
  return {diffFiles:500,diffChangedLines:changedLines,diffBytes:patch.length};
}
module.exports = {prepare,change};
