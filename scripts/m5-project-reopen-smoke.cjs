// Reopen only a previously successful generated-project UI fixture in a new app process.
const { app, dialog } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const candidate = process.argv[2];
if (!candidate || process.platform !== 'darwin') throw new Error('macOS generated fixture path required');
const root = fs.realpathSync(candidate);
const temporaryRoot = fs.realpathSync(os.tmpdir());
if (path.dirname(root) !== temporaryRoot || !path.basename(root).startsWith('hello-agent-project-ui-')) throw new Error('Not a generated UI fixture');
const resultPath = path.join(root, 'result.json');
if (!fs.lstatSync(resultPath).isFile() || fs.statSync(resultPath).size > 1024 * 1024) throw new Error('Invalid prior evidence');
const previous = JSON.parse(fs.readFileSync(resultPath, 'utf8'));
if (previous.status !== 'PASS' || previous.shutdown !== 'accepted' || fs.realpathSync(previous.root) !== root) throw new Error('Prior app did not finish successfully');
const userData = path.join(root, 'userdata');
if (!fs.lstatSync(userData).isDirectory()) throw new Error('Fixture data is not a directory');
const original = path.join(root, 'project', 'hello.txt');
if (fs.readFileSync(original, 'utf8') !== 'original\n') throw new Error('Fixture original is not restored');
app.setPath('userData', userData);
dialog.showOpenDialog = async () => { throw new Error('Reopen must use the saved project, not a new selection'); };
let finished = false;
const result = { verificationKind:'generatedProjectFullReopen', root, checkedChats:[], sourceUnchanged:false, shutdown:'pending', status:'PENDING' };
const persist = () => fs.writeFileSync(path.join(root, 'reopen-result.json'), JSON.stringify(result, null, 2));
const timer = setTimeout(() => finish(new Error('reopen timeout')), 90000);
function finish(error) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  result.sourceUnchanged = fs.readFileSync(original, 'utf8') === 'original\n';
  result.error = error?.message;
  const passed = !error && result.sourceUnchanged && result.checkedChats.length === 2;
  persist();
  const shutdownTimer = setTimeout(() => {
    result.shutdown = 'notConfirmed'; result.status = 'FAIL'; persist();
    console.log(JSON.stringify(result));
  }, 15000);
  app.once('will-quit', () => {
    clearTimeout(shutdownTimer);
    result.shutdown = 'accepted'; result.status = passed ? 'PASS' : 'FAIL'; persist();
    console.log(JSON.stringify(result));
    app.exit(passed ? 0 : 1);
  });
  app.quit();
}
app.on('browser-window-created', (_event, window) => {
  window.webContents.setBackgroundThrottling(false);
  window.webContents.once('did-finish-load', async () => {
    const run = async code => {
      if (finished) throw new Error('Reopen session finished');
      const value = await window.webContents.executeJavaScript(code);
      if (finished) throw new Error('Reopen session finished');
      return value;
    };
    const wait = async predicate => {
      for (let i = 0; i < 200; i++) {
        if (await run(predicate)) return;
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      throw new Error('Reopen UI condition timeout');
    };
    try {
      await wait(`document.querySelector('.m5-project-catalog-list li > button') && !document.querySelector('.m5-project-catalog-list li > button').disabled`);
      await run(`document.querySelector('.m5-project-catalog-list li > button').click()`);
      for (const [goal, edited] of [['Improve hello', true], ['Second task', false]]) {
        await wait(`Array.from(document.querySelectorAll('.m5-chat-chip')).some(b=>b.querySelector('.m5-chat-goal')?.textContent===${JSON.stringify(goal)})`);
        await run(`Array.from(document.querySelectorAll('.m5-chat-chip')).find(b=>b.querySelector('.m5-chat-goal')?.textContent===${JSON.stringify(goal)}).click()`);
        await wait(`document.querySelector('.m5-timeline h2')?.textContent===${JSON.stringify(goal)}`);
        await run(`Array.from(document.querySelectorAll('.m5-dock-group-tabs button')).find(b=>b.textContent==='파일')?.click()`);
        await wait(`Array.from(document.querySelectorAll('.m5-files button')).some(b=>b.textContent.includes('hello.txt'))`);
        await run(`Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes('hello.txt')).click()`);
        app.focus({steal:true}); window.show(); window.focus(); window.webContents.focus();
        await wait(`document.querySelector('.monaco-editor textarea')`);
        await run(`document.querySelector('.monaco-editor textarea').scrollIntoView({block:'center'})`);
        await wait(`document.querySelector('.monaco-editor .view-lines')?.textContent.includes('original')`);
        const observed = await run(`document.querySelector('.monaco-editor .view-lines').textContent.includes('edited')`);
        if (observed !== edited) throw new Error('Reopened chat snapshot differs');
        result.checkedChats.push(goal);
      }
      fs.writeFileSync(path.join(root, 'reopen.png'), (await window.webContents.capturePage()).toPNG());
      finish();
    } catch (error) { finish(error); }
  });
});
require(path.resolve(__dirname, '../.vite/build/main.cjs'));
