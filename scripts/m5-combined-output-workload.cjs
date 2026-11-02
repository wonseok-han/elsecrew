// Diagnostic-only fixtures owned by the output QA runner. Never a provider call.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const diff = require('../tests/m5-native-diff-fixture.cjs');
function prepare(source) {
  const tree = path.join(source, 'aaa-tree');
  fs.mkdirSync(tree);
  for (let index = 0; index < 1000; index++) {
    const folder = index ? path.join(tree, `folder-${String(index).padStart(4, '0')}`) : tree;
    if (index) fs.mkdirSync(folder);
    for (let file = 0; file < 10; file++) fs.writeFileSync(path.join(folder, `file-${file}.txt`), 'tree fixture\n', {flag:'wx'});
  }
  const text = require('../tests/m5-renderer-performance-fixture.cjs').getFileText();
  assert.equal(Buffer.byteLength(text), 5242880);
  fs.writeFileSync(path.join(source, 'large.txt'), text, {flag:'wx'});
  diff.prepare(source);
  return {treeFiles:10000, treeDirectories:1000, fileBytes:5242880, fileLines:100000};
}
async function mount(run, userData, chat) {
  const root = fs.realpathSync(path.join(userData, 'com.wonseokhan.helloagent/m5/worktrees-v1'));
  assert.match(chat.worktreeId, /^worktree_[a-zA-Z0-9_]+$/);
  const snapshot = fs.realpathSync(path.join(root, chat.worktreeId));
  assert.ok(snapshot.startsWith(root + path.sep), 'QA snapshot escaped its temporary root');
  const {probeLargeProject} = await import('./m5-packaged-large-project-probe.mjs');
  const {probeLargeDiff} = await import('./m5-packaged-diff-probe.mjs');
  await run(`(() => {
    const tab = [...document.querySelectorAll('.m5-dock-group-tabs button')].find(b => ['파일','Files'].includes(b.textContent.trim()));
    if (tab) tab.click();
  })()`);
  const readyBy = Date.now() + 65000;
  while (!await run(`!![...document.querySelectorAll('.m5-files button')].find(b => b.textContent.includes('aaa-tree') && !b.disabled)`)) {
    assert.ok(Date.now() < readyBy, 'Combined workspace file list did not become ready');
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  const dimensions = diff.change(snapshot);
  const largeProject = await run(`(${probeLargeProject.toString()})(${JSON.stringify(chat.projectId)})`);
  const largeDiff = {...await run(`(${probeLargeDiff.toString()})()`), ...dimensions};
  const coexist = await run(`!!document.querySelector('.monaco-editor') && !!document.querySelector('.m5-diff-details')`);
  assert.equal(coexist, true, 'Large model and diff did not coexist');
  return {largeProject, largeDiff, coexist};
}
module.exports = {prepare, mount};
