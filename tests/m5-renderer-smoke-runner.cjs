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
    projectButton.click();
    await ${waitFor("document.querySelector('.m5-file-row')", "project tree")};
    document.querySelector(".m5-file-row").click();
    await ${waitFor("document.querySelector('.monaco-editor') && Object.hasOwn(globalThis, 'MonacoEnvironment') && typeof globalThis.MonacoEnvironment.getWorker === 'function'", "Monaco mount")};
    const localWorker = globalThis.MonacoEnvironment.getWorker("smoke", "editor");
    localWorker.terminate();
    const editorNode = document.querySelector('.monaco-editor');
    [...document.querySelectorAll('button')].find(button => button.textContent === '역할과 모델').click();
    await ${waitFor("document.querySelectorAll('.m5-role-card').length === 3", "role settings")};
    const modelField = document.querySelector('.m5-role-card input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(modelField,'draft-model');
    modelField.dispatchEvent(new Event('input',{bubbles:true}));
    await ${waitFor("document.querySelector('.m5-role-card input').value === 'draft-model'", "model draft")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '새 채팅 설정 저장').click();
    await ${waitFor("document.body.textContent.includes('설정을 저장하지 못했어요') && document.querySelector('.m5-role-card input').value === 'draft-model'", "settings failure preserves draft")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '저장된 설정 다시 불러오기').click();
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
    document.querySelector('[aria-label="편집기 패널 왼쪽으로"]').click();
    await ${waitFor("document.querySelector('.m5-dock-slot')?.dataset.panel === 'editor'", "panel reorder")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('reorder remounted Monaco');
    const separator = document.querySelector('.m5-dock-separator');
    const before = Number(separator.getAttribute('aria-valuenow'));
    separator.dispatchEvent(new KeyboardEvent('keydown', {key:'ArrowRight', bubbles:true}));
    await ${waitFor("Number(document.querySelector('.m5-dock-separator').getAttribute('aria-valuenow')) > before", "keyboard resize")};
    const transfer = new DataTransfer();
    transfer.setData('application/x-hello-agent-panel', 'changes');
    document.querySelector('[data-panel="editor"]').dispatchEvent(new DragEvent('drop', {dataTransfer:transfer, bubbles:true, cancelable:true}));
    await ${waitFor("document.querySelector('.m5-dock-slot')?.dataset.panel === 'changes'", "panel drop")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('drop remounted Monaco');
    await ${waitFor("JSON.parse(localStorage.getItem('hello-agent.m5.dock.v1'))?.order[0] === 'changes'", "layout persistence")};
    const directionButton = [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '세로 배치');
    directionButton.click();
    await ${waitFor("document.querySelector('.m5-dock-vertical') && document.querySelector('.m5-dock-separator').getAttribute('aria-orientation') === 'horizontal'", "vertical layout")};
    const upper = document.querySelector('[data-panel=changes]').getBoundingClientRect();
    const lower = document.querySelector('[data-panel=editor]').getBoundingClientRect();
    if (lower.top <= upper.top || Math.abs(lower.left - upper.left) > 1) throw new Error('panels did not stack vertically');
    const verticalSeparator = document.querySelector('.m5-dock-separator');
    const verticalBefore = Number(verticalSeparator.getAttribute('aria-valuenow'));
    verticalSeparator.dispatchEvent(new KeyboardEvent('keydown', {key:'ArrowDown', bubbles:true}));
    await ${waitFor("Number(document.querySelector('.m5-dock-separator').getAttribute('aria-valuenow')) > verticalBefore", "vertical keyboard resize")};
    document.querySelector('[aria-label="편집기 패널 위로"]').click();
    await ${waitFor("document.querySelector('.m5-dock-slot')?.dataset.panel === 'editor'", "vertical reorder")};
    await ${waitFor("JSON.parse(localStorage.getItem('hello-agent.m5.dock.v1'))?.direction === 'vertical'", "direction persistence")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('direction change remounted Monaco');
    directionButton.click();
    await ${waitFor("!document.querySelector('.m5-dock-vertical') && document.querySelector('.m5-dock-separator').getAttribute('aria-orientation') === 'vertical'", "horizontal restored")};
    const tabsButton = [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '탭 배치');
    tabsButton.click();
    await ${waitFor("document.querySelectorAll('.m5-dock-tabs [role=tab]').length === 3 && !document.querySelector('[data-panel=editor]').hidden && document.querySelector('[data-panel=changes]').hidden", "tabbed layout")};
    if (document.querySelectorAll('.m5-dock-separator').length) throw new Error('tab group has splitter');
    const filesTab = [...document.querySelectorAll('.m5-dock-tabs [role=tab]')].find(button => button.textContent === '파일');
    filesTab.click();
    await ${waitFor("!document.querySelector('[data-panel=files]').hidden && document.querySelector('[data-panel=editor]').hidden", "tab selection")};
    filesTab.dispatchEvent(new KeyboardEvent('keydown', {key:'Home', bubbles:true}));
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && document.activeElement.getAttribute('role') === 'tab' && document.activeElement.getAttribute('aria-selected') === 'true'", "tab keyboard focus")};
    const tabsTransfer = new DataTransfer();
    tabsTransfer.setData('application/x-hello-agent-panel', 'files');
    document.querySelector('.m5-dock-tabs [role=tab]').dispatchEvent(new DragEvent('drop', {dataTransfer:tabsTransfer, bubbles:true, cancelable:true}));
    await ${waitFor("document.querySelector('.m5-dock-tabs [role=tab]').textContent === '파일'", "tab drag reorder")};
    await ${waitFor("JSON.parse(localStorage.getItem('hello-agent.m5.dock.v1'))?.tabbed === true && JSON.parse(localStorage.getItem('hello-agent.m5.dock.v1'))?.activeTab === 'editor'", "tab persistence")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('tabs remounted Monaco');
    tabsButton.click();
    await ${waitFor("document.querySelectorAll('.m5-dock-tabs [role=tab]').length === 0 && !document.querySelector('[data-panel=editor]').hidden && !document.querySelector('[data-panel=files]').hidden", "split restored")};
    return { workerCreated: true, dockVerified: true, text: document.body.textContent };
  })()`);
  if (!result.workerCreated || !result.text.includes("example.ts")) throw new Error("Monaco editor or local worker did not mount");
  if (!result.dockVerified) throw new Error("panel layout was not verified");
  window.show();
  window.focus();
  window.webContents.focus();
  await window.webContents.executeJavaScript(`(async () => {
    document.querySelector('.monaco-editor').scrollIntoView({block:'center'});
    document.querySelector('.monaco-editor textarea').focus();
    await ${waitFor("document.hasFocus() && document.activeElement === document.querySelector('.monaco-editor textarea')", "focused editor input")};
  })()`);
  window.webContents.sendInputEvent({ type: "char", keyCode: "Z" });
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
  })()`);
  process.stdout.write(`${JSON.stringify(result)}\n`);
  await window.close();
  app.exit(0);
}).catch(fail);
