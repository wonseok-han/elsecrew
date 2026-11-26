// Result preview tab in the real app (ADE 1차-나 P3): a synthetic project,
// isolated app data, snapshot workspaces, no provider, no debugging port.
//
//   ELSECREW_GIT_WORKTREES=0 electron scripts/m5-preview-ui-smoke.cjs   (needs .vite/ built: see
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
//
// macOS only (PR #9 review I5): the preview is off by default on Windows and
// Linux, and this check uses /usr/bin/git. Elsewhere it exits 2 (not run).
// `pnpm m5:test:preview-ui` starts it through scripts/m5-preview-ui-run.mjs.
'use strict';
if (process.platform !== 'darwin') {
  console.log(JSON.stringify({ status: 'NOT_RUN', reason: 'macOS only: the result preview is off by default on this platform and its real-app check has not been made for it' }));
  process.exit(2);
}
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
// PREVIEW_UI_THEME=light|dark starts the app with that brightness choice (main reads appearance.json).
if (process.env.PREVIEW_UI_THEME) {
  fs.mkdirSync(path.join(root, 'userdata'), { recursive: true });
  fs.writeFileSync(path.join(root, 'userdata', 'appearance.json'), JSON.stringify({ theme: process.env.PREVIEW_UI_THEME }));
}
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
  // Every request main gets from the tab, for the failure report.
  const traffic = [];
  const recorded = manager.request.bind(manager);
  manager.request = async (request) => {
    const entry = { at: Date.now(), action: request.action, obscured: request.obscured, bounds: request.bounds && [request.bounds.x, request.bounds.y, request.bounds.width, request.bounds.height].map(Math.round).join(',') };
    traffic.push(entry);
    if (traffic.length > 20000) traffic.shift();
    try { const answer = await recorded(request); entry.answered = Date.now() - entry.at; entry.state = answer.state; return answer; }
    catch (error) { entry.error = String(error.message || error).slice(0, 80); throw error; }
  };
  result.traffic = traffic;
  // The app page's own warnings and errors, for the failure report.
  result.console = [];
  window.webContents.on('console-message', (event) => { if ((event.level === 'error' || event.level === 'warning') && result.console.length < 200) result.console.push(`${event.level}: ${String(event.message).slice(0, 300)}`); });
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
  const other = await make('소개 페이지 만들기');
  fs.writeFileSync(path.join(other.worktreePath, 'index.html'), '<!doctype html><meta charset="utf-8"><h1>다른 작업의 페이지</h1>');
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
  let view = await waitFor(() => manager.currentWebContents, 10_000, 'view');
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

  // PR #9 review I1: what the page held invisible (data-preview-wait) and
  // when, next to when main hid the view (manager.visibility).
  await page(`(() => { window.__holds = []; new MutationObserver(records => { for (const r of records) window.__holds.push({ at: Date.now(), held: r.target.hasAttribute('data-preview-wait'), what: String(r.target.className || r.target.tagName).slice(0, 60) }); }).observe(document.body, { subtree: true, attributes: true, attributeFilter: ['data-preview-wait'] }); })()`);
  const ordering = async (since) => {
    const holds = (await page('window.__holds')).filter(entry => entry.at >= since);
    result.holdLog = (result.holdLog || []).concat(holds).slice(-200);
    const hides = manager.visibility.filter(entry => entry.at >= since);
    const heldAt = holds.find(entry => entry.held)?.at;
    const releasedAt = holds.find(entry => !entry.held)?.at;
    const hiddenAt = hides.find(entry => !entry.visible)?.at;
    // Held before it could be painted, and let go only after main hid the view.
    return { held: heldAt !== undefined, hiddenFirst: hiddenAt !== undefined && releasedAt !== undefined && hiddenAt <= releasedAt, heldAt, hiddenAt, releasedAt };
  };

  // T16: every cover hides the view at once; a still image takes its place.
  // The cover itself is held invisible until main has hidden the view (I1).
  const covered = async (label, open, close) => {
    await sleep(700); // a still image is taken while shown
    const since = Date.now();
    await open();
    const hidden = await waitFor(() => manager.currentLayout?.state === 'hidden', 3_000, `${label} hides`).then(() => true, () => false);
    const still = await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-stage] img.m5-preview-still'))`), 3_000, `${label} still`).then(() => true, () => false);
    await sleep(300);
    const order = await ordering(since);
    await capture(window, view, manager, `cover-${label}`);
    await close();
    const back = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, `${label} shows again`).then(() => true, () => false);
    check(`T16 ${label}: hidden while open, a still image in its place, shown again after`, hidden && still && back, { hidden, still, back });
    check(`I1 ${label}: the cover was held invisible until main hid the view`, order.held && order.hiddenFirst, order);
  };
  // The notification panel as it is with a long list: down over the stage.
  await covered('notifications', () => page(`(() => { document.querySelector('.m5-notification-panel').style.minHeight = '520px'; document.querySelector('.m5-notification-bell').open = true; })()`),
    () => page(`(() => { document.querySelector('.m5-notification-bell').open = false; document.querySelector('.m5-notification-panel').style.minHeight = ''; })()`));
  await covered('new-task-dialog', () => page(`document.querySelector('.m5-new-chat-button').click()`), () => page(`document.querySelector('.m5-new-chat-dialog header button').click()`));
  // The notification panel at its real size where it opens: hidden exactly
  // when it reaches the stage (I1: no 520px stretch).
  {
    await sleep(700);
    const since = Date.now();
    const reach = await page(`(() => { document.querySelector('.m5-notification-bell').open = true; const p = document.querySelector('.m5-notification-panel').getBoundingClientRect(); const s = document.querySelector('[data-testid=preview-stage]').getBoundingClientRect(); return { panel: [p.left, p.top, p.width, p.height], stage: [s.left, s.top, s.width, s.height], overlaps: p.left < s.right && s.left < p.right && p.top < s.bottom && s.top < p.bottom }; })()`);
    await sleep(800);
    const state = manager.currentLayout?.state;
    await capture(window, view, manager, 'cover-notifications-real-size');
    await page(`document.querySelector('.m5-notification-bell').open = false`);
    const back = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'real-size panel closed').then(() => true, () => false);
    check('I1 notification panel at its real size: hidden exactly when it reaches the stage', (state === 'hidden') === reach.overlaps && back, { ...reach, state, order: await ordering(since) });
  }
  // Only a 1.5px sliver of the panel over the stage's left edge, missed by
  // every grid point: found by its rectangle, held, then hidden (I1).
  {
    await sleep(700);
    const since = Date.now();
    const sliver = await page(`(() => {
      document.querySelector('.m5-notification-bell').open = true;
      const panel = document.querySelector('.m5-notification-panel');
      const p = panel.getBoundingClientRect(); const s = document.querySelector('[data-testid=preview-stage]').getBoundingClientRect();
      panel.style.transform = 'translate(' + (s.left + 1.5 - p.right) + 'px, ' + (s.top + 40 - p.top) + 'px)';
      const q = panel.getBoundingClientRect();
      return { overlapX: q.right - s.left, top: q.top - s.top };
    })()`);
    const hidden = await waitFor(() => manager.currentLayout?.state === 'hidden', 3_000, 'sliver hides').then(() => true, () => false);
    await sleep(1000);
    const stays = manager.currentLayout?.state === 'hidden';
    const order = await ordering(since);
    await capture(window, view, manager, 'cover-notification-edge');
    await page(`(() => { document.querySelector('.m5-notification-bell').open = false; document.querySelector('.m5-notification-panel').style.transform = ''; })()`);
    const back = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'sliver gone').then(() => true, () => false);
    check('I1 a notification panel over only the stage edge (1.5px) hides the view, held first', hidden && stays && back && order.held && order.hiddenFirst, { sliver, hidden, stays, back, order });
  }
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

  // PR #9 re-review B1·I1: main's answers held back on purpose. The cover
  // (dialog with its ::backdrop, the review sheet) shows and takes input only
  // after main answered the hide; a band that moves the stage appears only
  // after it; with no answer at all the preview is detached first.
  {
    const original = manager.request.bind(manager);
    const delays = { before: 0, after: 0, hang: false, close: 0 };
    let hideAnsweredAt = 0;
    let closeAnsweredAt = 0;
    manager.request = async (request) => {
      const hide = request.action === 'layout' && request.obscured;
      // No hide is answered (nor applied) until the tab detaches the preview.
      if (hide && delays.hang) return new Promise(() => undefined);
      if (request.action === 'close') delays.hang = false;
      if (hide && delays.before) await sleep(delays.before);
      const answer = await original(request);
      if (hide && delays.after) await sleep(delays.after);
      if (hide) hideAnsweredAt = Date.now();
      if (request.action === 'close') { if (delays.close) await sleep(delays.close); closeAnsweredAt = Date.now(); }
      return answer;
    };
    const look = selector => page(`(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      if (!element) return null;
      const style = getComputedStyle(element);
      const backdrop = element instanceof HTMLDialogElement ? getComputedStyle(element, '::backdrop') : null;
      const field = element.querySelector('textarea, input, button');
      let typed = false;
      if (field && element.hasAttribute('data-preview-wait')) { field.focus(); typed = document.activeElement === field; }
      return { held: element.hasAttribute('data-preview-wait'), inert: element.inert === true, opacity: style.opacity, backdrop: backdrop ? backdrop.opacity : null,
        focusInside: element.contains(document.activeElement) && document.activeElement !== element, typed, at: Date.now() };
    })()`);
    // While held: invisible (also the backdrop), inert, no focus or typing inside.
    const sealed = sample => sample && sample.held && sample.inert && sample.opacity === '0' && (sample.backdrop === null || sample.backdrop === '0') && !sample.focusInside && !sample.typed;
    const watch = async (selector, ms) => {
      const samples = [];
      const end = Date.now() + ms;
      while (Date.now() < end) { const sample = await look(selector); samples.push({ ...sample, view: manager.currentLayout?.state }); await sleep(60); }
      return samples;
    };

    // (1) The hide is applied at once but its answer comes 900 ms late.
    await sleep(700);
    let since = Date.now();
    delays.after = 900;
    await page(`document.querySelector('.m5-new-chat-button').click()`);
    let samples = await watch('.m5-new-chat-dialog', 650);
    const releasedLate = await waitFor(() => look('.m5-new-chat-dialog').then(sample => sample && !sample.held && sample), 5_000, 'dialog shown after the answer').catch(() => null);
    let order = await ordering(since);
    check('I1 a dialog (and its backdrop) stays invisible and inert until main answers the hide (answer 900 ms late)',
      samples.length > 3 && samples.every(sealed) && releasedLate && releasedLate.opacity !== '0' && releasedLate.focusInside && order.releasedAt >= hideAnsweredAt - 5 && order.hiddenFirst,
      { samples: samples.slice(0, 3), releasedLate, order, hideAnsweredAt });
    await page(`document.querySelector('.m5-new-chat-dialog header button').click()`);
    await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'shown after dialog (1)');
    delays.after = 0;

    // (2) Main itself is 900 ms late hiding: the view is still drawn, so the dialog must not be.
    await sleep(700);
    since = Date.now();
    delays.before = 900;
    await page(`document.querySelector('.m5-new-chat-button').click()`);
    samples = await watch('.m5-new-chat-dialog', 650);
    const whileShown = samples.filter(sample => sample.view === 'shown');
    await waitFor(() => look('.m5-new-chat-dialog').then(sample => sample && !sample.held), 5_000, 'dialog shown after main hid');
    order = await ordering(since);
    check('I1 while main is slow to hide, the view stays drawn and the dialog never shows over it (hide 900 ms late)',
      whileShown.length > 2 && whileShown.every(sealed) && order.hiddenFirst, { whileShown: whileShown.length, order });
    await page(`document.querySelector('.m5-new-chat-dialog header button').click()`);
    await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'shown after dialog (2)');
    delays.before = 0;

    // (3) The review sheet, answer 900 ms late. A task still being drafted
    // offers no "변경 보기" while the preview tab is shown, so the sheet is
    // shown the way the layout shows it: its \`hidden\` attribute goes away.
    await sleep(700);
    since = Date.now();
    delays.after = 900;
    const opener = await page(`(() => { const sheet = document.querySelector('[data-testid=review-sheet]'); if (!sheet || !sheet.hidden) return false; sheet.hidden = false; return true; })()`);
    samples = opener ? await watch('[data-testid=review-sheet]', 650) : [];
    const sheetOpen = opener && await waitFor(() => look('[data-testid=review-sheet]').then(sample => sample && !sample.held && sample.opacity !== '0'), 5_000, 'sheet shown after the answer').then(() => true, () => false);
    order = await ordering(since);
    check('I1 the review sheet stays invisible and inert until main answers the hide (answer 900 ms late)',
      opener && samples.length > 3 && samples.every(sealed) && sheetOpen && order.releasedAt >= hideAnsweredAt - 5, { opener, samples: samples.slice(0, 2), sheetOpen, order, hideAnsweredAt });
    if (opener) await page(`document.querySelector('[data-testid=review-sheet]').hidden = true`);
    await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'shown after sheet');
    delays.after = 0;

    // (4) A band in the tab ("링크를 막았어요") pushes the stage down: it
    // appears only after main answered the hide, and the view comes back at
    // the stage's new place.
    await sleep(700);
    await page(`(() => { window.__band = null; new MutationObserver(() => { if (!window.__band && document.querySelector('[data-testid=preview-blocked]')) window.__band = Date.now(); }).observe(document.body, { subtree: true, childList: true }); })()`);
    const stageBefore = await page(`document.querySelector('[data-testid=preview-stage]').getBoundingClientRect().top`);
    since = Date.now();
    delays.after = 900;
    const live = manager.currentWebContents;
    await live.executeJavaScript(`location.href = 'https://example.com/elsewhere'`).catch(() => undefined);
    const bandAt = await waitFor(() => page('window.__band'), 5_000, 'band shown').catch(() => null);
    const hiddenAt = manager.visibility.filter(entry => entry.at >= since && !entry.visible).map(entry => entry.at)[0];
    const back = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'shown under the band').then(() => true, () => false);
    const stageAfter = await page(`document.querySelector('[data-testid=preview-stage]').getBoundingClientRect().top`);
    const placed = manager.currentLayout?.bounds;
    check('B1 a band that pushes the stage appears only after main answered the hide; the view returns at the new place',
      bandAt !== null && hiddenAt !== undefined && hiddenAt <= bandAt && bandAt >= hideAnsweredAt - 5 && back && stageAfter > stageBefore && placed && Math.abs(placed.y - Math.ceil(stageAfter)) <= 1,
      { bandAt, hiddenAt, hideAnsweredAt, stageBefore, stageAfter, placed });
    delays.after = 0;
    // The band leaves after a while; the view is hidden first again.
    await waitFor(() => page(`!document.querySelector('[data-testid=preview-blocked]')`), 10_000, 'band gone');
    await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'shown after the band');

    // (5) A window resize: main hides before anything moves, before the page says a word.
    await sleep(500);
    window.setSize(1600, 920);
    const hiddenAtResize = manager.currentLayout?.state === 'hidden';
    const backAfterResize = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'shown after resize').then(() => true, () => false);
    const stageNow = await page(`(() => { const r = document.querySelector('[data-testid=preview-stage]').getBoundingClientRect(); return { left: r.left, top: r.top, right: r.right, bottom: r.bottom }; })()`);
    const boundsNow = manager.currentLayout?.bounds;
    const insideNow = boundsNow && boundsNow.x >= stageNow.left - 0.01 && boundsNow.y >= stageNow.top - 0.01 && boundsNow.x + boundsNow.width <= stageNow.right + 0.01 && boundsNow.y + boundsNow.height <= stageNow.bottom + 0.01;
    check('B1 a window resize hides the view in main at once; it returns inside the new stage', hiddenAtResize && backAfterResize && insideNow, { hiddenAtResize, backAfterResize, stageNow, boundsNow });
    window.setSize(1680, 945);
    await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'shown at the old size');

    // (6) The hide is never answered: at the limit the preview is detached,
    // and the dialog shows only once main has answered that close (400 ms late).
    await sleep(700);
    since = Date.now();
    delays.hang = true;
    delays.close = 400;
    closeAnsweredAt = 0;
    await page(`document.querySelector('.m5-new-chat-button').click()`);
    samples = await watch('.m5-new-chat-dialog', 1300);
    const released = await waitFor(() => look('.m5-new-chat-dialog').then(sample => sample && !sample.held && sample), 6_000, 'dialog after detach').catch(() => null);
    order = await ordering(since);
    check('I1 with no answer the preview is detached and the dialog shows only after main answered the close',
      samples.every(sealed) && released && closeAnsweredAt > 0 && released.at >= closeAnsweredAt && order.releasedAt >= closeAnsweredAt - 5 && order.releasedAt - since >= 1500,
      { released, closeAnsweredAt, order, since });
    delays.hang = false;
    delays.close = 0;
    await page(`document.querySelector('.m5-new-chat-dialog header button').click()`);
    manager.request = original;
    // The tab opens the preview again by itself.
    await waitFor(() => manager.currentLayout?.state === 'shown', 15_000, 'reopened after detach');
    view = manager.currentWebContents;
  }
  step('delayedAnswers');

  // B1: zoomed in and out (the View menu's zoom), the view stays inside the
  // stage and off the frame; a dialog still hides it, also when the zoom
  // changes while the dialog is open.
  const measure = () => page(`(() => { const r = e => { const b = document.querySelector(e).getBoundingClientRect(); return { left: b.left, top: b.top, width: b.width, height: b.height }; }; return { stage: r('[data-testid=preview-stage]'), frame: r('[data-testid=preview-frame]'), ratio: devicePixelRatio }; })()`);
  for (const zoom of [1.25, 1.5, 0.8, 0.67]) {
    main.setPageZoomFactor(zoom);
    await sleep(400);
    const shown = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, `zoom ${zoom} shown`).then(() => true, () => false);
    await sleep(400);
    const css = await measure();
    const b = manager.currentLayout?.bounds;
    const stageWin = { left: css.stage.left * zoom, top: css.stage.top * zoom, right: (css.stage.left + css.stage.width) * zoom, bottom: (css.stage.top + css.stage.height) * zoom };
    const frameWin = { left: css.frame.left * zoom, top: css.frame.top * zoom, right: (css.frame.left + css.frame.width) * zoom, bottom: (css.frame.top + css.frame.height) * zoom };
    const inside = Boolean(b) && b.x >= stageWin.left - 0.01 && b.y >= stageWin.top - 0.01 && b.x + b.width <= stageWin.right + 0.01 && b.y + b.height <= stageWin.bottom + 0.01;
    const offFrame = Boolean(b) && !(b.x < frameWin.right && frameWin.left < b.x + b.width && b.y < frameWin.bottom && frameWin.top < b.y + b.height);
    const label = `zoom-${Math.round(zoom * 100)}`;
    await capture(window, view, manager, `preview-${label}`);
    await page(`document.querySelector('.m5-new-chat-button').click()`);
    const dialogHidden = await waitFor(() => manager.currentLayout?.state === 'hidden', 3_000, `${label} dialog hides`).then(() => true, () => false);
    await capture(window, view, manager, `cover-dialog-${label}`);
    await page(`document.querySelector('.m5-new-chat-dialog header button').click()`);
    const back = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, `${label} back`).then(() => true, () => false);
    check(`B1 at ${Math.round(zoom * 100)}%: the view is inside the stage and off the frame; a dialog hides it`, shown && inside && offFrame && dialogHidden && back, { zoom, ratio: css.ratio, stageWin, frameWin, bounds: b, shown, dialogHidden, back });
  }
  // Zoomed out while a dialog is open: the view stays hidden (main used to
  // refuse the measurement and keep the view as it was).
  main.setPageZoomFactor(1);
  await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'zoom 100 shown');
  await page(`document.querySelector('.m5-new-chat-button').click()`);
  await waitFor(() => manager.currentLayout?.state === 'hidden', 3_000, 'dialog at 100%');
  main.setPageZoomFactor(0.5);
  await sleep(1200);
  const hiddenWhileZoomed = manager.currentLayout?.state === 'hidden' && !manager.visibility.slice(-3).some(entry => entry.visible && entry.at > Date.now() - 1100);
  main.setPageZoomFactor(1);
  await sleep(400);
  await page(`document.querySelector('.m5-new-chat-dialog header button').click()`);
  const backAt100 = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'back at 100%').then(() => true, () => false);
  check('B1 zooming out with a dialog open keeps the view hidden; it returns in place after', hiddenWhileZoomed && backAt100);
  step('zoom');

  // I1: an open still on its way never shows the view over a dialog opened
  // meanwhile, nor after another task was chosen meanwhile.
  {
    const original = manager.request.bind(manager);
    let release = null;
    manager.request = async (request) => { if (request.action === 'open' && release) await release.promise; return original(request); };
    const gate = () => { let open; const promise = new Promise(resolve => { open = resolve; }); release = { promise, open }; };
    const toPreview = async goal => { await openTask(goal); await sleep(300); await page(`document.querySelector('[data-testid=right-tab-preview]').click()`); };
    // A dialog during the open.
    await toPreview('README 고치기');
    await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-empty]'))`), 20_000, 'empty again');
    gate();
    let since = Date.now();
    await toPreview('할 일 앱 만들기');
    await waitFor(() => page(`document.querySelector('[data-testid=preview-tab]')?.dataset.phase === 'opening'`), 10_000, 'opening');
    await page(`document.querySelector('.m5-new-chat-button').click()`);
    await sleep(300);
    const gated = release; release = null; gated.open();
    await waitFor(() => page(`document.querySelector('[data-testid=preview-tab]')?.dataset.phase === 'open'`), 15_000, 'opened under dialog');
    await sleep(1500);
    const shownUnderDialog = manager.visibility.filter(entry => entry.at >= since && entry.visible);
    await capture(window, null, manager, 'delayed-open-under-dialog');
    await page(`document.querySelector('.m5-new-chat-dialog header button').click()`);
    const afterDialog = await waitFor(() => manager.currentLayout?.state === 'shown', 5_000, 'shown after dialog').then(() => true, () => false);
    check('I1 an open on its way while a dialog opens stays hidden until the dialog closes', shownUnderDialog.length === 0 && afterDialog, { shownUnderDialog });
    // Another task chosen during the open, then back: the first answer is never shown.
    gate();
    since = Date.now();
    const before = manager.currentLayout ? 1 : 0;
    await toPreview('소개 페이지 만들기');
    await waitFor(() => page(`document.querySelector('[data-testid=preview-tab]')?.dataset.phase === 'opening'`), 10_000, 'other task opening');
    await toPreview('할 일 앱 만들기');
    const second = release; release = null; second.open();
    await waitFor(() => page(`document.querySelector('[data-testid=preview-tab]')?.dataset.phase === 'open'`), 15_000, 'open after switch');
    await waitFor(() => manager.currentLayout?.state === 'shown', 10_000, 'shown after switch');
    const final = await waitFor(() => manager.currentWebContents, 5_000, 'final view');
    await waitFor(() => !final.isLoading(), 10_000, 'final load');
    const finalText = await final.executeJavaScript('document.title + "|" + (document.querySelector("h1")?.textContent ?? "")');
    const shownIds = [...new Set(manager.visibility.filter(entry => entry.at >= since && entry.visible).map(entry => entry.previewId))];
    check('I1 an open for a task left meanwhile is never shown; the current task\'s page is', shownIds.length === 1 && !/다른 작업의 페이지/.test(finalText), { before, shownIds, finalText });
    manager.request = original;
  }
  step('delayedOpen');

  // I4: quit preparation closes the preview; when it is called off the tab
  // opens it again by itself.
  manager.suspend();
  const suspendedShown = await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-suspended]'))`), 5_000, 'suspended').then(() => true, () => false);
  await capture(window, null, manager, 'preview-suspended');
  const noViewWhileSuspended = !manager.currentLayout;
  manager.resume();
  const reopened = await waitFor(() => manager.currentLayout?.state === 'shown', 10_000, 'reopened after resume').then(() => true, () => false);
  check('I4 a called-off quit brings the preview back by itself', suspendedShown && noViewWhileSuspended && reopened, { suspendedShown, noViewWhileSuspended, reopened });
  step('resume');

  // The app's ⌘2 typed while the preview has focus reaches the app (minor).
  {
    const current = await waitFor(() => manager.currentWebContents, 5_000, 'view for keys');
    await waitFor(() => !current.isLoading(), 10_000, 'view loaded for keys');
    current.focus();
    await sleep(200);
    const modifier = process.platform === 'darwin' ? 'meta' : 'control';
    current.sendInputEvent({ type: 'keyDown', keyCode: '2', modifiers: [modifier] });
    current.sendInputEvent({ type: 'keyUp', keyCode: '2', modifiers: [modifier] });
    const reached = await waitFor(() => page(`(() => { const a = document.activeElement; return Boolean(a) && (a.id === 'm5-followup-text' || a.classList.contains('m5-timeline-list')); })()`), 3_000, 'shortcut reached the app').then(() => true, () => false);
    check('a window shortcut typed in the preview reaches the app', reached);
  }
  step('shortcut');

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
