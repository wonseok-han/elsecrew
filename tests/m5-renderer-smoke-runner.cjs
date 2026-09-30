const { app, BrowserWindow, net, protocol } = require("electron");
const { resolve, sep } = require("node:path");
const { pathToFileURL } = require("node:url");

const rendererDirectory = resolve(process.env.M5_RENDERER_DIR || "");
const preload = resolve(__dirname, "m5-renderer-smoke-preload.cjs");

protocol.registerSchemesAsPrivileged([{ scheme: "m5smoke", privileges: { standard: true, secure: true, supportFetchAPI: true } }]);

function fail(error) {
  process.stderr.write(`${error instanceof Error ? error.stack || error.message : String(error)}\n`);
  app.exit(1);
}

function waitFor(script, label) {
  return `new Promise((resolve, reject) => {
    const deadline = Date.now() + 7000;
    const timer = setInterval(() => {
      if (${script}) { clearInterval(timer); resolve(true); }
      else if (Date.now() > deadline) { clearInterval(timer); reject(new Error(\`renderer smoke timed out: ${label}; body=\${document.body?.innerText?.trim().slice(0, 1000) || "<empty>"}; controls=\${JSON.stringify([...document.querySelectorAll("button")].map((button) => ({ text: button.textContent?.trim(), ariaLabel: button.getAttribute("aria-label"), disabled: button.disabled }))) }\`)); }
    }, 25);
  })`;
}

app.whenReady().then(async () => {
  if (!rendererDirectory || rendererDirectory === resolve("")) throw new Error("M5_RENDERER_DIR is required");
  await protocol.handle("m5smoke", (request) => {
    const url = new URL(request.url);
    const relative = decodeURIComponent(url.pathname).replace(/^\/+/, "") || "index.html";
    const asset = resolve(rendererDirectory, relative);
    if (asset !== rendererDirectory && !asset.startsWith(`${rendererDirectory}${sep}`)) return new Response("not found", { status: 404 });
    return net.fetch(pathToFileURL(asset).toString());
  });
  const window = new BrowserWindow({ show: false, webPreferences: { preload, contextIsolation: false, sandbox: false, nodeIntegration: false } });
  window.webContents.on("console-message", (_event, _level, message) => process.stderr.write(`renderer: ${message}\n`));
  window.webContents.on("render-process-gone", (_event, details) => fail(new Error(`renderer exited: ${details.reason}`)));
  await window.loadURL("m5smoke://m5/index.html");
  const result = await window.webContents.executeJavaScript(`(async () => {
    delete globalThis.MonacoEnvironment;
    if (typeof globalThis.MonacoEnvironment !== "undefined" || Object.hasOwn(globalThis, "MonacoEnvironment")) {
      throw new Error("MonacoEnvironment should be created by the product renderer");
    }
    const controlLabel = "프로젝트 선택";
    await ${waitFor("document.querySelector('.m5-toolbar-actions select')", "language control")};
    const chooseLanguage = value => {
      const select = document.querySelector('.m5-toolbar-actions select');
      select.value = value;
      select.dispatchEvent(new Event('change', {bubbles:true}));
    };
    chooseLanguage('en');
    await ${waitFor("document.querySelector('main').lang === 'en' && document.querySelector('h1').textContent === 'Choose a project to get started'", "English shell")};
    if (localStorage.getItem('hello-agent.lang') !== 'en') throw new Error('language choice was not saved');
    chooseLanguage('ko');
    await ${waitFor("document.querySelector('main').lang === 'ko' && document.querySelector('h1').textContent === '프로젝트를 선택해 시작하세요'", "Korean shell")};
    await ${waitFor("document.querySelector('[aria-label=\"진행 및 복구 상태\"] code')?.textContent === 'lock:before_chat'", "pre-chat lock notice")};
    const getStatus = window.m5.getSystemStatus;
    window.m5.getSystemStatus = async () => { throw new Error('synthetic status failure'); };
    document.querySelector('[aria-label="진행 및 복구 상태"] button').click();
    await ${waitFor("document.querySelector('[aria-label=\"진행 및 복구 상태\"]').textContent.includes('이전 조회')", "stale status labeled")};
    window.m5.getSystemStatus = getStatus;
    window.m5SmokeBlockers = [];
    document.querySelector('[aria-label="진행 및 복구 상태"] button').click();
    await ${waitFor("!document.querySelector('[aria-label=\"진행 및 복구 상태\"]')", "cleared status notice")};
    const codexSection = () => document.querySelector('[aria-label="Codex 연결"]');
    const codexButton = text => [...codexSection().querySelectorAll('button')].find(b => b.textContent === text);
    await ${waitFor("document.querySelector('[aria-label=\"Codex 연결\"]').textContent.includes('이 앱에 Codex 로그인이 연결됐어요')", "automatic saved-login restoration")};
    codexButton('연결 상태 다시 확인').click();
    await ${waitFor("[...document.querySelectorAll('[aria-label=\"Codex 연결\"] button')].some(b => b.textContent === '연결 잠금 복구' && !b.disabled)", "recoverable login UI")};
    if (!codexButton('Codex 로그인').disabled || !codexButton('연결 상태 다시 확인').disabled) throw new Error('fenced login permits start/check');
    codexButton('연결 잠금 복구').click();
    await ${waitFor("[...document.querySelectorAll('[aria-label=\"Codex 연결\"] button')].some(b => b.textContent === '연결 잠금 복구' && b.disabled)", "recovery single flight")};
    await ${waitFor("[...document.querySelectorAll('[aria-label=\"Codex 연결\"] button')].some(b => b.textContent === '연결 잠금 복구' && !b.disabled)", "rejected recovery remains visible")};
    if (!codexButton('Codex 로그인').disabled) throw new Error('failed recovery enabled login');
    codexButton('연결 잠금 복구').click();
    await ${waitFor("![...document.querySelectorAll('[aria-label=\"Codex 연결\"] button')].some(b => b.textContent === '연결 잠금 복구')", "recovery completed")};
    if (codexSection().textContent.includes('로그인이 연결됐어요')) throw new Error('recovery falsely claims authentication');
    await ${waitFor("[...document.querySelectorAll('[aria-label=\"Codex 연결\"] button')].some(b => b.textContent === '연결 상태 다시 확인' && !b.disabled)", "explicit credential recheck")};
    codexButton('연결 상태 다시 확인').click();
    await ${waitFor("document.querySelector('[aria-label=\"Codex 연결\"]').textContent.includes('이 앱에 Codex 로그인이 연결됐어요')", "restored Codex login without browser")};
    await ${waitFor("[...document.querySelectorAll('button')].some(b => b.textContent === 'Claude Code 로그인' && !b.disabled)", "enabled Claude login button")};
    [...document.querySelectorAll('button')].find(b => b.textContent === 'Claude Code 로그인').click();
    await ${waitFor("[...document.querySelectorAll('button')].some(b => b.textContent === '로그인 취소' && !b.disabled)", "login waiting and cancellation")};
    [...document.querySelectorAll('button')].find(b => b.textContent === '로그인 취소').click();
    await ${waitFor("document.querySelector('[aria-label=\"Claude Code 연결\"]').textContent.includes('로그인을 취소했어요')", "canceled Claude login feedback")};
    if (!document.querySelector('[aria-label="Codex 연결"]').textContent.includes('이 앱에 Codex 로그인이 연결됐어요')) throw new Error("Claude cancellation changed Codex status");
    await ${waitFor("[...document.querySelectorAll('button')].some(b => b.textContent === 'Claude Code 로그인' && !b.disabled)", "retry Claude login")};
    [...document.querySelectorAll('button')].find(b => b.textContent === 'Claude Code 로그인').click();
    await ${waitFor("document.querySelector('#claude-connection-code')", "connection code input")};
    const codeInput = document.querySelector('#claude-connection-code');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(codeInput,'synthetic#state');
    codeInput.dispatchEvent(new Event('input',{bubbles:true}));
    await ${waitFor("[...document.querySelectorAll('button')].some(b => b.textContent === '연결 확인' && !b.disabled)", "code submit enabled")};
    [...document.querySelectorAll('button')].find(b => b.textContent === '연결 확인').click();
    await ${waitFor("document.querySelector('[aria-label=\"Claude Code 연결\"]').textContent.includes('이 앱에 Claude Code 로그인이 연결됐어요')", "Claude code accepted")};
    if (document.querySelector('#claude-connection-code')) throw new Error('submitted code input was retained');
    await ${waitFor("[...document.querySelectorAll('button')].some((button) => !button.disabled && (button.getAttribute('aria-label') === controlLabel || button.textContent?.trim() === controlLabel))", "enabled accessible project-selection control")};
    const projectButton = [...document.querySelectorAll("button")].find((button) => !button.disabled && (button.getAttribute("aria-label") === controlLabel || button.textContent?.trim() === controlLabel));
    if (!projectButton) throw new Error("renderer smoke lost its enabled accessible project-selection control after it was observed");
    document.querySelector('.m5-project-catalog > button').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li button') && !document.querySelector('.m5-project-catalog-list li button').disabled", "saved project catalog loaded")};
    if (document.querySelector('.m5-project-catalog-list li span').textContent !== '/work/earlier') throw new Error('catalog first page missing');
    const recoveryButton = text => [...document.querySelectorAll('[aria-label="프로젝트 시작 실패 복구"] button')].find(b => b.textContent === text);
    recoveryButton('시작 실패 확인').click();
    await ${waitFor("document.querySelector('[aria-label=\"실패한 준비 종료 확인\"]')", "explicit preparation inspection")};
    recoveryButton('이 준비 기록 종료').click();
    await ${waitFor("document.querySelector('[aria-label=\"프로젝트 시작 실패 복구\"]').textContent.includes('결과를 확인하지 못했어요')", "lost preparation response")};
    recoveryButton('이 준비 기록 종료').click();
    await ${waitFor("document.querySelector('[aria-label=\"프로젝트 시작 실패 복구\"]').textContent.includes('실패한 준비 기록을 종료했어요')", "preparation retry acknowledgment")};
    if (document.querySelector('[aria-label="실패한 준비 종료 확인"]')) throw new Error('completed preparation confirmation retained');
    [...document.querySelectorAll('.m5-project-catalog-list button')].find(b => b.textContent === '다음 목록').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li span')?.textContent === '/work/smoke'", "next catalog page")};
    if (document.querySelector('.m5-project-catalog-list li span').textContent !== '/work/smoke') throw new Error('catalog path missing');
    document.querySelector('.m5-project-catalog-list li button').click();
    await ${waitFor("document.querySelector('.m5-file-row')", "project tree")};
    document.querySelector('.m5-file-row').click();
    await ${waitFor("document.querySelector('.m5-breadcrumbs [aria-current=location]')?.textContent === '/ src'", "folder breadcrumb")};
    document.querySelector('.m5-breadcrumbs .m5-root').click();
    await ${waitFor("document.querySelector('.m5-file-row')?.textContent === 'src' && !document.querySelector('.m5-breadcrumbs [aria-current]')", "root breadcrumb")};
    document.querySelector('[aria-label="편집기 패널 접기"]').click();
    await ${waitFor("document.querySelector('[data-panel=editor]').hidden", "editor hidden before search")};
    const fileSearch = document.querySelector('[aria-label="파일 이름 또는 경로 찾기"]');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(fileSearch, 'EXAMPLE.TS');
    fileSearch.dispatchEvent(new Event('input', { bubbles: true }));
    await ${waitFor("!document.querySelector('.m5-file-search button[type=submit]').disabled", "file search enabled")};
    document.querySelector('.m5-file-search button[type=submit]').click();
    await ${waitFor("document.querySelector('.m5-search-results button')?.textContent === 'src/example.ts'", "recursive file search result")};
    if (document.querySelector('.m5-file-row').textContent !== 'src') throw new Error('search changed folder navigation');
    document.querySelector('.m5-search-results button').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && document.querySelector('.monaco-editor') && Object.hasOwn(globalThis, 'MonacoEnvironment') && typeof globalThis.MonacoEnvironment.getWorker === 'function'", "Monaco mount and reveal")};
    const localWorker = globalThis.MonacoEnvironment.getWorker("smoke", "editor");
    localWorker.terminate();
    const editorNode = document.querySelector('.monaco-editor');
    document.querySelector('[aria-label="편집기 패널 접기"]').click();
    await ${waitFor("document.querySelector('[data-panel=editor]').hidden", "hide an already-open editor")};
    document.querySelector('.m5-search-results button').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden", "reveal already-open file")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('file navigation remounted Monaco');
    [...document.querySelectorAll('button')].find(button => button.textContent === '역할과 모델').click();
    await ${waitFor("document.querySelectorAll('.m5-role-card').length === 3", "role settings")};
    const modelField = document.querySelector('.m5-role-card input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(modelField,'draft-model');
    modelField.dispatchEvent(new Event('input',{bubbles:true}));
    await ${waitFor("document.querySelector('.m5-role-card input').value === 'draft-model'", "model draft")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '새 채팅 설정 저장').click();
    await ${waitFor("document.body.textContent.includes('설정을 저장하지 못했어요') && document.querySelector('.m5-role-card input').value === 'draft-model'", "settings failure preserves draft")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '저장된 설정 다시 불러오기').click();
    await ${waitFor("document.querySelector('.m5-settings [role=alertdialog]')", "failed-save draft reload confirmation")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '설정 변경 버리고 다시 불러오기').click();
    await ${waitFor("document.querySelector('.m5-role-card input').value === 'test-model'", "explicit settings reload")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '작업대로 돌아가기').click();
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('settings reload remounted editor');
    [...document.querySelectorAll('button')].find(button => button.textContent.trim() === '기본 배치로').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && document.querySelectorAll('.m5-dock-separator').length === 2", "reset panel visibility")};
    document.querySelector('[aria-label="편집기 패널에 집중"]').click();
    await ${waitFor("document.querySelector('[data-panel=files]').hidden && document.querySelector('[data-panel=changes]').hidden && document.querySelectorAll('.m5-dock-separator').length === 0", "focus editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('focus remounted Monaco');
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '파일').click();
    await ${waitFor("!document.querySelector('[data-panel=files]').hidden", "reopen files")};
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '변경').click();
    await ${waitFor("!document.querySelector('[data-panel=changes]').hidden", "reopen changes")};
    document.querySelector('[aria-label="편집기 패널 접기"]').click();
    await ${waitFor("document.querySelector('[data-panel=editor]').hidden", "collapse editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('collapse discarded Monaco');
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '편집기').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden", "restore editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('restore remounted Monaco');
    const place = (source, target) => { const select = document.querySelector('[aria-label="' + source + ' 패널 배치"]'); select.value = target; select.dispatchEvent(new Event('change', {bubbles:true})); };
    place('편집기', 'files:left');
    await ${waitFor("document.querySelector('[data-panel=editor]').getBoundingClientRect().left < document.querySelector('[data-panel=files]').getBoundingClientRect().left", "panel reorder")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('reorder remounted Monaco');
    const separator = document.querySelector('.m5-dock-separator');
    const before = Number(separator.getAttribute('aria-valuenow'));
    separator.dispatchEvent(new KeyboardEvent('keydown', {key:'ArrowRight', bubbles:true}));
    await ${waitFor("Number(document.querySelector('.m5-dock-separator').getAttribute('aria-valuenow')) > before", "keyboard resize")};
    const transfer = new DataTransfer();
    transfer.setData('application/x-hello-agent-panel', 'changes');
    const editorPanel = document.querySelector('[data-panel="editor"]');
    const bounds = editorPanel.getBoundingClientRect();
    editorPanel.dispatchEvent(new DragEvent('dragover', {dataTransfer:transfer, clientX:bounds.left+2, clientY:bounds.top+bounds.height/2, bubbles:true, cancelable:true}));
    await ${waitFor("document.querySelector('.m5-dock-drop-left')", "drop preview")};
    editorPanel.dispatchEvent(new DragEvent('drop', {dataTransfer:transfer, bubbles:true, cancelable:true}));
    await ${waitFor("document.querySelector('[data-panel=changes]').getBoundingClientRect().left < document.querySelector('[data-panel=editor]').getBoundingClientRect().left", "panel drop")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('drop remounted Monaco');
    await ${waitFor("JSON.parse(localStorage.getItem('hello-agent.m5.dock.v1'))?.order[0] === 'changes'", "layout persistence")};
    const directionButton = [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '세로 배치');
    directionButton.click();
    await ${waitFor("document.querySelector('.m5-dock-separator').getAttribute('aria-orientation') === 'horizontal'", "vertical layout")};
    const upper = document.querySelector('[data-panel=changes]').getBoundingClientRect();
    const lower = document.querySelector('[data-panel=editor]').getBoundingClientRect();
    if (lower.top <= upper.top || Math.abs(lower.left - upper.left) > 1) throw new Error('panels did not stack vertically');
    const verticalSeparator = document.querySelector('.m5-dock-separator');
    const verticalBefore = Number(verticalSeparator.getAttribute('aria-valuenow'));
    verticalSeparator.dispatchEvent(new KeyboardEvent('keydown', {key:'ArrowDown', bubbles:true}));
    await ${waitFor("Number(document.querySelector('.m5-dock-separator').getAttribute('aria-valuenow')) > verticalBefore", "vertical keyboard resize")};
    place('편집기', 'changes:top');
    await ${waitFor("document.querySelector('[data-panel=editor]').getBoundingClientRect().top < document.querySelector('[data-panel=changes]').getBoundingClientRect().top", "vertical reorder")};
    await ${waitFor("JSON.parse(localStorage.getItem('hello-agent.m5.dock.v1'))?.direction === 'vertical'", "direction persistence")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('direction change remounted Monaco');
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '가로 배치').click();
    await ${waitFor("document.querySelector('.m5-dock-separator').getAttribute('aria-orientation') === 'vertical'", "horizontal restored")};
    const tabsButton = [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '탭 배치');
    tabsButton.click();
    await ${waitFor("document.querySelector('[data-panel=editor] .m5-dock-group-tabs').children.length === 3 && !document.querySelector('[data-panel=editor]').hidden && document.querySelector('[data-panel=changes]').hidden", "tabbed layout")};
    if (document.querySelectorAll('.m5-dock-separator').length) throw new Error('tab group has splitter');
    const filesTab = [...document.querySelectorAll('[data-panel=editor] [role=tab]')].find(button => button.textContent === '파일');
    filesTab.click();
    await ${waitFor("!document.querySelector('[data-panel=files]').hidden && document.querySelector('[data-panel=editor]').hidden", "tab selection")};
    document.querySelector('[data-panel=files] [aria-selected=true]').dispatchEvent(new KeyboardEvent('keydown', {key:'Home', bubbles:true}));
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && document.activeElement.getAttribute('role') === 'tab' && document.activeElement.getAttribute('aria-selected') === 'true'", "tab keyboard focus")};
    place('편집기', 'files:right');
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && !document.querySelector('[data-panel=files]').hidden && document.querySelectorAll('.m5-dock-separator').length === 1", "split a tab group")};
    place('변경', 'files:bottom');
    await ${waitFor("document.querySelectorAll('.m5-dock-separator').length === 2 && document.querySelector('.m5-dock-split-vertical') && document.querySelector('.m5-dock-split-horizontal')", "nested split axes")};
    const filesBounds = document.querySelector('[data-panel=files]').getBoundingClientRect(), changesBounds = document.querySelector('[data-panel=changes]').getBoundingClientRect();
    if (changesBounds.top <= filesBounds.top || Math.abs(changesBounds.left-filesBounds.left)>1) throw new Error('nested geometry mismatch');
    await ${waitFor("JSON.parse(localStorage.getItem('hello-agent.m5.dock.v1'))?.tree?.type === 'split'", "nested persistence")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('tabs remounted Monaco');
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '가로 배치').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && !document.querySelector('[data-panel=files]').hidden && document.querySelectorAll('.m5-dock-separator').length === 2", "split restored")};
    return { workerCreated: true, dockVerified: true, text: document.body.textContent };
  })()`);
  if (!result.workerCreated || !result.text.includes("example.ts")) throw new Error("Monaco editor or local worker did not mount");
  if (!result.dockVerified) throw new Error("panel layout was not verified");
  // Keep the synthetic window hidden: foregrounding it captures the user's real
  // keyboard/IME input and can alter the draft while a confirmation is pending.
  window.webContents.debugger.attach("1.3");
  await window.webContents.debugger.sendCommand("Emulation.setFocusEmulationEnabled", { enabled: true });
  await window.webContents.executeJavaScript(`(async () => {
    document.querySelector('.monaco-editor').scrollIntoView({block:'center'});
    document.querySelector('.monaco-editor textarea').focus();
    await ${waitFor("document.activeElement === document.querySelector('.monaco-editor textarea')", "focused editor input")};
  })()`);
  await window.webContents.debugger.sendCommand("Input.insertText", { text: "Z" });
  if (window.isVisible() || window.isFocused()) throw new Error("synthetic input must not capture desktop focus");
  await window.webContents.executeJavaScript(`(async () => {
    await ${waitFor("document.querySelector('[aria-label=\"저장되지 않은 변경\"]')", "edited draft")};
    const reloadButton = () => [...document.querySelectorAll('button')].find(button => button.textContent === '저장된 내용 다시 불러오기');
    reloadButton().click();
    await ${waitFor("document.querySelector('[role=alertdialog]')", "reload confirmation")};
    [...document.querySelectorAll('[role=alertdialog] button')].find(button => button.textContent === '취소').click();
    await ${waitFor("!document.querySelector('[role=alertdialog]')", "cancel reload")};
    if (!document.querySelector('[aria-label=\"저장되지 않은 변경\"]')) throw new Error('cancel discarded draft');
    reloadButton().click();
    await ${waitFor("document.querySelector('[role=alertdialog]')", "confirm reload again")};
    [...document.querySelectorAll('[role=alertdialog] button')].find(button => button.textContent === '편집 내용 버리고 다시 불러오기').click();
    await ${waitFor("!document.querySelector('[role=alertdialog]') && !document.querySelector('[aria-label=\"저장되지 않은 변경\"]')", "saved text restored")};
  })()`);
  await window.webContents.executeJavaScript(`(async () => {
    globalThis.shortcutSaves = 0;
    const commit = window.m5.commitFileSave;
    window.m5.commitFileSave = async (...args) => { globalThis.shortcutSaves++; return commit(...args); };
    document.querySelector('.monaco-editor textarea').focus();
  })()`);
  await window.webContents.debugger.sendCommand("Input.insertText", { text: "S" });
  await window.webContents.executeJavaScript(`(async () => {
    await ${waitFor("document.querySelector('[aria-label=\"저장되지 않은 변경\"]')", "shortcut dirty draft")};
    const originalEditor = document.querySelector('.monaco-editor');
    for (const language of ['en', 'ko']) {
      const select = document.querySelector('.m5-toolbar-actions select');
      select.value = language;
      select.dispatchEvent(new Event('change', {bubbles:true}));
      await new Promise(resolve => setTimeout(resolve, 30));
      if (document.querySelector('main').lang !== language || document.querySelector('.monaco-editor') !== originalEditor || !document.querySelector('[aria-label="저장되지 않은 변경"]')) throw new Error('language switch lost the active editor draft');
    }
    document.querySelector('.monaco-editor textarea').focus();
  })()`);
  for (const type of ["keyDown", "keyUp"]) {
    await window.webContents.debugger.sendCommand("Input.dispatchKeyEvent", {
      type, key: "s", code: "KeyS", windowsVirtualKeyCode: 83,
      modifiers: process.platform === "darwin" ? 4 : 2,
    });
  }
  await window.webContents.executeJavaScript(`(async () => {
    await ${waitFor("globalThis.shortcutSaves === 1 && !document.querySelector('[aria-label=\"저장되지 않은 변경\"]')", "keyboard save commits the snapshot draft")};
    document.querySelector('.m5-file-row').click();
    await ${waitFor("[...document.querySelectorAll('.m5-file-row')].some(b => b.textContent.includes('sample.rs'))", "language fixture files")};
    for (const [name, language] of [['sample.rs','rust'], ['sample.py','python'], ['sample.json','json']]) {
      [...document.querySelectorAll('.m5-file-row')].find(b => b.textContent.includes(name)).click();
      await new Promise((resolve, reject) => {
        const deadline = Date.now() + 3000;
        const timer = setInterval(() => {
          if (document.querySelector('.m5-monaco')?.getAttribute('data-mode-id') === language) { clearInterval(timer); resolve(); }
          else if (Date.now() > deadline) { clearInterval(timer); reject(new Error('language mode not selected: ' + language + '; ' + document.body.innerText.slice(-1800))); }
        }, 25);
      });
    }
    const jsonWorker = globalThis.MonacoEnvironment.getWorker('smoke', 'json');
    jsonWorker.terminate();
    document.querySelector('.m5-change-row').click();
    await ${waitFor("document.querySelectorAll('.m5-diff-lines tbody tr').length === 200", "bounded diff page")};
    if (!document.querySelector('.m5-diff-deleted') || !document.querySelector('.m5-diff-added')) throw new Error('missing distinct change rows');
    const next = () => [...document.querySelectorAll('button')].find(button => button.textContent === '다음 변경');
    next().click();
    await ${waitFor("document.querySelector('.m5-diff-pages span').textContent.startsWith('2 /')", "second diff page")};
    next().click();
    await ${waitFor("document.querySelectorAll('.m5-diff-lines tbody tr').length === 3", "last diff page")};
    if (!next().disabled || !document.querySelector('.m5-diff-lines').textContent.includes('+line 401')) throw new Error('last diff lines missing');
    document.querySelector('.m5-change-row').click();
    await ${waitFor("document.querySelector('.m5-diff-pages span').textContent.startsWith('1 /')", "reopened diff resets page")};
    document.querySelector('.m5-chat-chip').click();
    await ${waitFor("document.querySelector('.m5-followup button:not(:disabled)') || document.querySelector('#m5-followup-text')", "follow-up composer")};
    const input = document.querySelector('#m5-followup-text');
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(input, '설명을 추가해 주세요.');
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await ${waitFor("!document.querySelector('.m5-followup button').disabled", "enabled follow-up submit")};
    document.querySelector('.m5-followup button').click();
    await ${waitFor("[...document.querySelectorAll('.m5-message p')].some(node => node.textContent === '설명을 추가해 주세요.') && document.querySelector('#m5-followup-text').value === ''", "persisted follow-up visible and input cleared")};
    if ([...document.querySelectorAll('.m5-message p')].filter(node => node.textContent === '설명을 추가해 주세요.').length !== 1) throw new Error('duplicate follow-up message');
    const leave = [...document.querySelectorAll('button')].find(button => button.textContent.trim() === '프로젝트 화면 닫기');
    if (!leave || leave.disabled) throw new Error('cannot leave idle project');
    [...document.querySelectorAll('.m5-toolbar-actions button')].find(button => button.textContent === '역할과 모델').click();
    await ${waitFor("document.querySelector('.m5-settings')", "role settings open")};
    if (!leave.disabled) throw new Error('leaving must protect open role settings');
    const modelInput = document.querySelector('.m5-settings input');
    const originalModel = modelInput.value;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(modelInput, 'unsaved-model');
    modelInput.dispatchEvent(new Event('input', {bubbles: true}));
    await ${waitFor("document.querySelector('.m5-settings [role=status]')", "role draft dirty")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '작업대로 돌아가기').click();
    await ${waitFor("document.querySelector('[aria-label=\"저장하지 않은 설정 확인\"]')", "role close confirmation")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '설정 계속 편집').click();
    await ${waitFor("!document.querySelector('.m5-settings [role=alertdialog]')", "role draft retained")};
    if (document.querySelector('.m5-settings input').value !== 'unsaved-model') throw new Error('cancel lost role draft');
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '저장된 설정 다시 불러오기').click();
    await ${waitFor("document.querySelector('.m5-settings [role=alertdialog]')", "role reload confirmation")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '설정 변경 버리고 다시 불러오기').click();
    await ${waitFor("!document.querySelector('.m5-settings [role=status]') && !document.querySelector('.m5-settings [role=alertdialog]')", "role settings restored")};
    if (document.querySelector('.m5-settings input').value !== originalModel) throw new Error('role reload did not restore saved model');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(document.querySelector('.m5-settings input'), 'discard-this-model');
    document.querySelector('.m5-settings input').dispatchEvent(new Event('input', {bubbles: true}));
    await ${waitFor("document.querySelector('.m5-settings [role=status]')", "new role draft before explicit discard")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '작업대로 돌아가기').click();
    await ${waitFor("document.querySelector('.m5-settings [role=alertdialog]')", "explicit role discard confirmation")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '설정 변경 버리고 닫기').click();
    await ${waitFor("!document.querySelector('.m5-settings')", "role settings closed")};
    leave.click();
    await ${waitFor("!document.querySelector('#m5-followup-text') && document.querySelector('h1').textContent === '프로젝트를 선택해 시작하세요'", "project and chat view cleared")};
    document.querySelector('.m5-project-catalog > button').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li span')?.textContent === '/work/earlier'", "catalog reopened")};
    [...document.querySelectorAll('.m5-project-catalog-list button')].find(b => b.textContent === '다음 목록').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li span')?.textContent === '/work/smoke'", "saved project available again")};
    document.querySelector('.m5-project-catalog-list li button').click();
    await ${waitFor("document.querySelector('.m5-chat-chip')", "saved chat available again")};
    document.querySelector('.m5-chat-chip').click();
    await ${waitFor("[...document.querySelectorAll('.m5-message p')].some(node => node.textContent === '설명을 추가해 주세요.')", "chat history preserved after reopening")};
    const originalRequest = window.m5Projects.request;
    window.m5Projects.request = input => input.action === 'releaseView' ? Promise.reject(new Error('synthetic release failure')) : originalRequest(input);
    [...document.querySelectorAll('button')].find(button => button.textContent.trim() === '프로젝트 화면 닫기').click();
    await ${waitFor("[...document.querySelectorAll('[role=alert]')].some(node => node.textContent.includes('파일 감시 해제를 확인하지 못해'))", "failed release blocks leaving")};
    if (!document.querySelector('#m5-followup-text') || document.querySelector('h1').textContent !== 'smoke') throw new Error('failed release cleared the project');
  })()`);
  process.stdout.write(`${JSON.stringify(result)}\n`);
  if (window.isVisible() || window.isFocused()) throw new Error("renderer smoke captured desktop focus");
  window.webContents.debugger.detach();
  await window.close();
  app.exit(0);
}).catch(fail);
