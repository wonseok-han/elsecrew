// Result preview tab in the real app (ADE 1차-나 P3): a synthetic project,
// isolated app data, snapshot workspaces, no provider, no debugging port.
//
//   electron scripts/m5-preview-ui-smoke.cjs        (needs .vite/ built: see
//   scripts/m5-preview-isolation-probe.mjs --build, and the debug sidecar)
//
// Opens a task's todo page in the preview tab with real clicks and key
// input, wide and narrow; checks the app's frame stays outside the view (T8);
// opens the notification panel, a dialog, the drawer (narrow window) and
// another tab and checks the view hides each time with a still image in its
// place (T16); opens a page from the files tab; shows the empty state for a
// task without a page and the stalled screen for a page that hangs.
// Writes captures (the app page with the view composed at its place) and
// `result.json` to PREVIEW_UI_OUT (default .artifacts/m5-preview-ui).
'use strict';
const { app, BrowserWindow, dialog } = require('electron');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { FILES } = require('./m5-preview-pages.cjs');

const repoRoot = path.resolve(__dirname, '..');
const out = path.resolve(process.env.PREVIEW_UI_OUT || path.join(repoRoot, '.artifacts', 'm5-preview-ui'));
fs.mkdirSync(out, { recursive: true });
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'elsecrew-preview-ui-'));
const source = path.join(root, 'project');
fs.mkdirSync(source);
fs.writeFileSync(path.join(source, 'README.md'), 'preview ui smoke\n');
for (const args of [['init', '-q', '--template='], ['add', '.'], ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Smoke', '-c', 'user.email=smoke@example.invalid', 'commit', '-qm', 'fixture']]) {
  execFileSync('/usr/bin/git', args, { cwd: source, env: { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' } });
}
if (process.env.ELSECREW_GIT_WORKTREES !== '0') throw new Error('run with ELSECREW_GIT_WORKTREES=0 (snapshot workspaces inside the isolated app data)');
app.setPath('userData', path.join(root, 'userdata'));
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [source] });

const result = { steps: [], checks: {}, captures: [], errors: [] };
const check = (name, ok, detail) => { result.checks[name] = Boolean(ok); if (detail !== undefined) result[`detail:${name}`] = detail; console.log(JSON.stringify({ check: name, ok: Boolean(ok) })); };
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let finished = false;
const deadline = setTimeout(() => finish(new Error('ui smoke timeout')), 300_000);
function finish(error) {
  if (finished) return;
  finished = true;
  clearTimeout(deadline);
  if (error) result.errors.push(String(error.stack || error));
  if (error && lastWindow) {
    lastWindow.webContents.executeJavaScript(`({ covered: document.querySelector('[data-testid=task-layout]')?.dataset.previewCovered, obscured: document.querySelector('[data-testid=preview-tab]')?.dataset.obscured, tab: { ...document.querySelector('[data-testid=preview-tab]')?.dataset },
      hits: (() => { const st = document.querySelector('[data-testid=preview-stage]'); if (!st) return null; const r = st.getBoundingClientRect(); const out = []; for (let row = 0; row <= 4; row++) for (let col = 0; col <= 4; col++) { const fx = col / 4, fy = row / 4; const e = document.elementFromPoint(r.left + 8 + (r.width - 16) * fx, r.top + 8 + (r.height - 16) * fy); out.push(e ? (e === st || st.contains(e) ? 'stage' : e.tagName + '.' + e.className) : null); } return { rect: [r.left, r.top, r.width, r.height], out }; })(),
      text: document.body.innerText.slice(0, 300), rows: Array.from(document.querySelectorAll('.m5-task-row')).map(r => r.outerHTML.slice(0, 200)) })`).then(state => {
      fs.writeFileSync(path.join(out, 'failure-state.json'), JSON.stringify(state, null, 2));
    }, () => undefined);
  }
  result.status = !error && Object.values(result.checks).every(Boolean) ? 'PASS' : 'FAIL';
  result.root = root;
  fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(result, null, 2));
  setTimeout(() => undefined, 0);
  console.log(JSON.stringify({ status: result.status, failed: Object.entries(result.checks).filter(([, ok]) => !ok).map(([name]) => name), errors: result.errors.map(e => e.slice(0, 300)), out }));
  setTimeout(() => app.quit(), 1000);
  setTimeout(() => app.exit(result.status === 'PASS' ? 0 : 1), 20_000).unref();
}

let started = false;
let lastWindow = null;
app.on('browser-window-created', (_event, window) => {
  if (started) return;
  started = true;
  lastWindow = window;
  window.webContents.once('did-finish-load', () => { run(window).then(() => finish(null), finish); });
});

async function run(window) {
  const main = require(path.join(repoRoot, '.vite/build/main.cjs'));
  const manager = main.previewManager;
  const page = code => window.webContents.executeJavaScript(code);
  const waitFor = async (predicate, ms = 20_000, label = 'condition') => {
    const end = Date.now() + ms;
    for (;;) {
      const value = await predicate();
      if (value) return value;
      if (Date.now() > end) throw new Error(`timeout: ${label}`);
      await sleep(100);
    }
  };
  const step = name => { result.steps.push(name); console.log(JSON.stringify({ stage: name })); };
  window.setSize(1680, 945);
  window.show();
  window.focus();
  await waitFor(() => page(`Array.from(document.querySelectorAll('button')).some(b => b.textContent === '프로젝트 선택' && !b.disabled)`), 60_000, 'start screen');
  await page(require('./m5-smoke-complete-team.cjs'));
  await page(`Array.from(document.querySelectorAll('button')).find(b => b.textContent === '프로젝트 선택').click()`);
  await waitFor(() => page(`Boolean(document.querySelector('[data-testid=task-layout]'))`), 60_000, 'project open');
  const projectId = await page(`window.m5Projects.request({ action: 'list', cursor: null }).then(r => r.projects[0]?.projectId ?? null)`);
  if (!projectId) throw new Error('project id unavailable');
  // The task (and a second one with no page) are made through the bridge,
  // as the app would after a team run; the screen then picks them up.
  const make = async (goal) => page(`(async () => {
    const status = await window.m5.getSystemStatus();
    const projectId = ${JSON.stringify(projectId)};
    const settings = await window.m5.getProjectRoleSettings({ projectId });
    const prepared = await window.m5.prepareChatWorkspace({ projectId });
    const m = prepared.manifest;
    const chat = await window.m5.createChat({ preparationId: prepared.preparationId, projectId, worktreeId: m.worktreeId, workspaceIdentity: m.workspaceIdentity, goal: ${JSON.stringify(goal)}, expectedSettingsRevision: settings.revision });
    return { projectId, chatId: chat.chatId, worktreePath: m.worktreePath, status: status.state };
  })()`);
  const todo = await make('할 일 앱 만들기');
  const plain = await make('README 고치기');
  for (const [name, text] of Object.entries({ 'index.html': FILES['index.html'], 'todo.css': FILES['todo.css'], 'todo.js': FILES['todo.js'], 'about.html': '<!doctype html><meta charset="utf-8"><h1>소개 페이지</h1>', 'hang.html': FILES['hang.html'] })) {
    fs.writeFileSync(path.join(todo.worktreePath, name), text);
  }
  step('tasks');
  // The list learns new tasks from its own refresh; a reload shows both.
  window.webContents.reload();
  await waitFor(() => page(`document.readyState === 'complete' && Boolean(document.querySelector('[data-testid=task-layout]'))`).catch(() => false), 60_000, 'reload');
  // Clicked until it is the selected task (the list may still be disabled while it loads).
  const openTask = goal => waitFor(() => page(`(() => { const b = Array.from(document.querySelectorAll('.m5-task-row button.m5-chat-chip')).find(b => b.textContent.includes(${JSON.stringify(goal)})); if (!b) return false; if (b.getAttribute('aria-current') === 'true') return true; if (!b.disabled) b.click(); return false; })()`), 30_000, `task ${goal}`);
  await openTask('할 일 앱 만들기');
  await page(`document.querySelector('[data-testid=right-tab-preview]').click()`);
  await waitFor(() => page(`document.querySelector('[data-testid=preview-tab]')?.dataset.phase === 'open'`), 30_000, 'preview open');
  const view = await waitFor(() => manager.currentWebContents, 10_000, 'view');
  await waitFor(() => !view.isLoading() && view.getURL().startsWith('elsecrew-preview://'), 15_000, 'view load');
  await waitFor(() => manager.currentLayout?.state === 'shown', 10_000, 'view shown');
  step('previewOpen');

  // T8: the app's frame is outside the view, with its three labels.
  const frame = await page(`(() => { const r = document.querySelector('[data-testid=preview-frame]').getBoundingClientRect(); return { left: r.left, top: r.top, width: r.width, height: r.height, text: document.querySelector('[data-testid=preview-frame]').textContent }; })()`);
  const stageRect = await page(`(() => { const r = document.querySelector('[data-testid=preview-stage]').getBoundingClientRect(); return { left: r.left, top: r.top, width: r.width, height: r.height }; })()`);
  const bounds = manager.currentLayout.bounds;
  const overlapsFrame = bounds.x < frame.left + frame.width && frame.left < bounds.x + bounds.width && bounds.y < frame.top + frame.height && frame.top < bounds.y + bounds.height;
  check('T8 the view never covers the app frame', !overlapsFrame, { frame, bounds });
  check('T8 the frame says preview, read-only, no internet', /미리보기 — 팀이 만든 결과/.test(frame.text) && /읽기 전용/.test(frame.text) && /인터넷 차단/.test(frame.text));
  check('the view sits inside the stage', bounds.x >= stageRect.left - 1 && bounds.y >= stageRect.top - 1 && bounds.x + bounds.width <= stageRect.left + stageRect.width + 1 && bounds.y + bounds.height <= stageRect.top + stageRect.height + 1, { stageRect, bounds });

  // Real input into the page: click the field, type, Enter; click an item.
  const clickIn = async (selector) => {
    const r = await view.executeJavaScript(`(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) }; })()`);
    view.focus();
    view.sendInputEvent({ type: 'mouseDown', x: r.x, y: r.y, button: 'left', clickCount: 1 });
    view.sendInputEvent({ type: 'mouseUp', x: r.x, y: r.y, button: 'left', clickCount: 1 });
    await sleep(150);
  };
  const type = async (text) => { for (const ch of text) { view.sendInputEvent({ type: 'char', keyCode: ch }); await sleep(15); } };
  const enter = async () => { view.sendInputEvent({ type: 'keyDown', keyCode: 'Enter' }); view.sendInputEvent({ type: 'char', keyCode: '\r' }); view.sendInputEvent({ type: 'keyUp', keyCode: 'Enter' }); await sleep(200); };
  await clickIn('#text'); await type('preview check'); await enter();
  await clickIn('#text'); await type('narrow screen'); await enter();
  await clickIn('li');
  const afterInput = await view.executeJavaScript(`({ items: Array.from(document.querySelectorAll('li')).map(li => li.textContent + (li.className ? ' [' + li.className + ']' : '')), count: document.getElementById('count').textContent, width: innerWidth })`);
  check('the todo app takes real clicks and typing (wide)', afterInput.items.length === 2 && afterInput.items[0].includes('[done]') && afterInput.count === '1개 남음', afterInput);
  await capture(window, view, manager, 'preview-wide');

  await page(`document.querySelector('[data-testid=preview-narrow]').click()`);
  await waitFor(() => manager.currentLayout?.bounds.width === 390, 5_000, 'narrow');
  await sleep(300);
  await clickIn('#text'); await type('third'); await enter();
  const narrow = await view.executeJavaScript(`({ items: document.querySelectorAll('li').length, width: innerWidth })`);
  check('narrow is 390px and the page still works', narrow.width === 390 && narrow.items === 3, narrow);
  await capture(window, view, manager, 'preview-narrow');
  await page(`document.querySelector('[data-testid=preview-wide]').click()`);
  await waitFor(() => manager.currentLayout?.bounds.width > 390, 5_000, 'wide again');
  step('wideNarrow');

  // T16: every cover hides the view at once; a still image takes its place.
  const covered = async (label, open, close) => {
    await sleep(700); // a still image is taken while shown
    await open();
    const hidden = await waitFor(() => manager.currentLayout?.state === 'hidden', 3_000, `${label} hides`).then(() => true, () => false);
    const still = await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-stage] img.m5-preview-still'))`), 3_000, `${label} still`).then(() => true, () => false);
    await capture(window, view, manager, `cover-${label}`);
    await close();
    const back = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, `${label} shows again`).then(() => true, () => false);
    check(`T16 ${label}: hidden while open, a still image in its place, shown again after`, hidden && still && back, { hidden, still, back });
  };
  // The notification panel as it is with a long list: down over the stage.
  await covered('notifications', () => page(`(() => { document.querySelector('.m5-notification-panel').style.minHeight = '520px'; document.querySelector('.m5-notification-bell').open = true; })()`),
    () => page(`(() => { document.querySelector('.m5-notification-bell').open = false; document.querySelector('.m5-notification-panel').style.minHeight = ''; })()`));
  await covered('new-task-dialog', () => page(`document.querySelector('.m5-new-chat-button').click()`), () => page(`document.querySelector('.m5-new-chat-dialog header button').click()`));
  // A menu away from the stage covers nothing: the view stays.
  await page(`document.querySelector('.m5-task-menu').open = true`);
  await sleep(600);
  check('T16 a menu that does not reach the stage leaves the view shown', manager.currentLayout?.state === 'shown');
  await page(`document.querySelector('.m5-task-menu').open = false`);
  await covered('another-tab', () => page(`document.querySelector('[data-testid=right-tab-changes]').click()`), () => page(`document.querySelector('[data-testid=right-tab-preview]').click()`));
  // The drawer: below 1100px the results live in a closed drawer.
  window.setSize(1000, 820);
  const drawerHidden = await waitFor(() => manager.currentLayout?.state === 'hidden', 5_000, 'drawer closed hides').then(() => true, () => false);
  await capture(window, view, manager, 'cover-drawer-closed');
  await page(`document.querySelector('[data-testid=drawer-toggle-preview]').click()`);
  const drawerShown = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'drawer open shows').then(() => true, () => false);
  await sleep(500);
  await capture(window, view, manager, 'preview-in-drawer');
  check('T16 drawer: hidden while closed, shown in the open drawer', drawerHidden && drawerShown);
  window.setSize(1680, 945);
  await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'wide window').catch(() => undefined);
  step('covers');

  // The files tab opens a named page in the preview.
  await page(`document.querySelector('[data-testid=right-tab-files]').click()`);
  await waitFor(() => page(`Array.from(document.querySelectorAll('.m5-files button')).some(b => b.textContent.includes('about.html'))`), 15_000, 'files listed');
  await page(`Array.from(document.querySelectorAll('.m5-files button')).find(b => b.textContent.includes('about.html')).click()`);
  // The file opens in the editor sheet over the results; its header offers the preview.
  await waitFor(() => page(`!document.querySelector('[data-testid=review-sheet]').hidden && Boolean(document.querySelector('[data-testid=sheet-preview-open-file]'))`), 15_000, 'sheet with open in preview');
  await page(`document.querySelector('[data-testid=sheet-preview-open-file]').click()`);
  await waitFor(() => page(`document.querySelector('[data-testid=review-sheet]').hidden && document.querySelector('[data-testid=preview-tab] .m5-preview-entry')?.textContent === 'about.html'`), 15_000, 'about entry');
  const aboutView = await waitFor(() => manager.currentWebContents, 10_000, 'about view');
  await waitFor(() => !aboutView.isLoading() && aboutView.getURL().endsWith('/about.html'), 10_000, 'about load');
  check('editor sheet: "미리보기로 열기" closes the sheet and opens that page', (await aboutView.executeJavaScript('document.querySelector("h1").textContent')) === '소개 페이지');
  step('openFromFiles');

  // A task with no page: the empty state, and its way to the changes.
  await openTask('README 고치기');
  await page(`document.querySelector('[data-testid=right-tab-preview]').click()`);
  await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-empty]'))`), 20_000, 'empty state');
  check('a task without a page shows the empty state, no view', !manager.currentWebContents);
  await capture(window, null, manager, 'preview-empty');
  step('empty');

  // A page that hangs: the stalled screen, the app still answers, close works.
  await openTask('할 일 앱 만들기');
  await page(`document.querySelector('[data-testid=right-tab-files]').click()`);
  await waitFor(() => page(`Array.from(document.querySelectorAll('.m5-files button')).some(b => b.textContent.includes('hang.html'))`), 15_000, 'hang listed');
  await page(`Array.from(document.querySelectorAll('.m5-files button')).find(b => b.textContent.includes('hang.html')).click()`);
  // Closing the sheet, the files tab itself offers the selected page.
  await waitFor(() => page(`!document.querySelector('[data-testid=review-sheet]').hidden`), 15_000, 'sheet open');
  await page(`document.querySelector('[data-testid=sheet-close]').click()`);
  await waitFor(() => page(`document.querySelector('[data-testid=review-sheet]').hidden && Boolean(document.querySelector('[data-testid=preview-open-file]'))`), 15_000, 'files tab offer');
  await page(`document.querySelector('[data-testid=preview-open-file]').click()`);
  await waitFor(() => page(`document.querySelector('[data-testid=preview-tab] .m5-preview-entry')?.textContent === 'hang.html'`), 15_000, 'hang entry');
  check('files tab: "미리보기로 열기" opens the selected page', true);
  // Chromium notices a hung page by input it cannot handle: click into it.
  const hung = await waitFor(() => manager.currentWebContents?.getURL().endsWith('/hang.html') && manager.currentWebContents, 15_000, 'hang view');
  await sleep(1500);
  for (let index = 0; index < 3; index++) {
    hung.sendInputEvent({ type: 'mouseDown', x: 40, y: 40, button: 'left', clickCount: 1 });
    hung.sendInputEvent({ type: 'mouseUp', x: 40, y: 40, button: 'left', clickCount: 1 });
    await sleep(500);
  }
  const stalled = await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-stalled]'))`), 90_000, 'stalled screen').then(() => true, () => false);
  const appAnswers = Date.now(); await page('1'); const answerMs = Date.now() - appAnswers;
  // Main closes a page that stopped answering; the tab offers to open it again.
  const closed = stalled && await waitFor(() => !manager.currentLayout, 10_000, 'stalled view closed').then(() => true, () => false);
  const reopen = stalled && await page(`Array.from(document.querySelectorAll('[data-testid=preview-stalled] button')).some(b => b.textContent === '다시 열기')`);
  if (stalled) await capture(window, null, manager, 'preview-stalled');
  check('a hung page is closed with the stalled screen, the app keeps answering', stalled && answerMs < 500 && closed && reopen, { stalled, answerMs, closed, reopen });
  step('stalled');
}

/** The app page with the preview view composed at its place (a window
 * capture holds only the app page). */
async function capture(window, view, manager, name) {
  try {
    const base = await window.webContents.capturePage();
    const layout = manager.currentLayout;
    let overlay = null;
    if (view && layout && layout.state === 'shown' && !view.isDestroyed()) overlay = (await view.capturePage()).toDataURL();
    const [contentWidth] = window.getContentSize();
    const scale = base.getSize().width / contentWidth;
    const helper = new BrowserWindow({ show: false, webPreferences: { sandbox: true, contextIsolation: true } });
    await helper.loadURL('about:blank');
    const png = await helper.webContents.executeJavaScript(`(async () => {
      const load = src => new Promise((resolve, reject) => { const i = new Image(); i.onload = () => resolve(i); i.onerror = reject; i.src = src; });
      const base = await load(${JSON.stringify(base.toDataURL())});
      const canvas = document.createElement('canvas'); canvas.width = base.width; canvas.height = base.height;
      const g = canvas.getContext('2d'); g.drawImage(base, 0, 0);
      ${overlay ? `const v = await load(${JSON.stringify(overlay)}); const b = ${JSON.stringify(layout.bounds)}; g.drawImage(v, b.x * ${scale}, b.y * ${scale}, b.width * ${scale}, b.height * ${scale});` : ''}
      return canvas.toDataURL('image/png');
    })()`);
    helper.destroy();
    const file = path.join(out, `${name}.png`);
    fs.writeFileSync(file, Buffer.from(png.split(',')[1], 'base64'));
    result.captures.push(file);
  } catch (error) { result.errors.push(`capture ${name}: ${String(error.message || error).slice(0, 120)}`); }
}

require(path.join(repoRoot, '.vite/build/main.cjs'));
