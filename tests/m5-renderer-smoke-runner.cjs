const { app, BrowserWindow, net, protocol, session } = require("electron");
const { resolve, sep } = require("node:path");
const { pathToFileURL } = require("node:url");
const { mkdtempSync, writeFileSync } = require("node:fs");
const { measureMessagePaint } = require("../scripts/m5-message-paint-probe.cjs");
const { probeInstallConsent } = require("./m5-install-ui-probe.cjs");
const { probeCleanupConsent } = require("./m5-cleanup-ui-probe.cjs");
const { tmpdir } = require("node:os");

const userData = mkdtempSync(resolve(tmpdir(), "hello-agent-renderer-smoke-"));
app.setPath("userData", userData);

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
  await session.fromPartition("m5-renderer-smoke").protocol.handle("m5smoke", (request) => {
    const url = new URL(request.url);
    const relative = decodeURIComponent(url.pathname).replace(/^\/+/, "") || "index.html";
    const asset = resolve(rendererDirectory, relative);
    if (asset !== rendererDirectory && !asset.startsWith(`${rendererDirectory}${sep}`)) return new Response("not found", { status: 404 });
    return net.fetch(pathToFileURL(asset).toString());
  });
  const window = new BrowserWindow({ width: 960, height: 640, show: false, webPreferences: { preload, partition: "m5-renderer-smoke", contextIsolation: false, sandbox: false, nodeIntegration: false } });
  window.webContents.on("console-message", (event) => process.stderr.write(`renderer: ${event.message}\n`));
  window.webContents.on("render-process-gone", (_event, details) => fail(new Error(`renderer exited: ${details.reason}`)));
  await window.loadURL("m5smoke://m5/index.html");
  const mountScript = `(async () => {
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
    const selection = document.querySelector('.m5-provider-selection');
    if (!selection || window.m5SmokeLoginRequests.length !== 0) throw new Error('first-run agents were checked before selection');
    if (!selection.querySelector('button[type=submit]').disabled) throw new Error('empty selection is enabled');
    await ${waitFor("!document.querySelector('.m5-provider-selection input').matches(':disabled')", "storage admission before selection")};
    selection.querySelector('input[type=checkbox]').click();
    await ${waitFor("!document.querySelector('.m5-provider-selection button[type=submit]').disabled", "single agent selection")};
    selection.querySelector('button[type=submit]').click();
    await ${waitFor("document.querySelector('[aria-label=\"Codex connection\"]')?.textContent.includes('Codex is signed in')", "selected provider diagnosed and checked")};
    if (document.querySelector('[aria-label="Claude Code connection"]') || window.m5SmokeLoginRequests.some(r => r.provider === 'claude')) throw new Error('unselected Claude was checked');
    if (JSON.parse(localStorage.getItem('hello-agent.m5.providers.v1')).providers.join() !== 'codex') throw new Error('single provider selection not saved');
    if (!selection.open) selection.querySelector('summary').click();
    [...selection.querySelectorAll('button')].find(b => b.textContent === 'Connect Claude Code too').click();
    await ${waitFor("document.querySelector('[aria-label=\"Claude Code connection\"]')", "second agent added")};
    if (!document.querySelector('[aria-label="Codex connection"]') || !document.querySelector('[aria-label="Claude Code connection"]')) throw new Error('provider connection labels are not translated');
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
    if (!document.getElementById('codex-connection-details').hidden) throw new Error('connected account did not collapse');
    codexButton('연결 관리').click();
    await ${waitFor("!document.getElementById('codex-connection-details').hidden", "connection settings expanded")};
    await new Promise(resolve => setTimeout(resolve, 600));
    if (document.getElementById('codex-connection-details').hidden) throw new Error('status polling collapsed explicit settings');
    codexButton('연결 상태 다시 확인').click();
    await ${waitFor("[...document.querySelectorAll('[aria-label=\"Codex 연결\"] button')].some(b => b.textContent === '연결 잠금 복구' && !b.disabled)", "recoverable login UI")};
    if (!codexButton('Codex 로그인').disabled || !codexButton('연결 상태 다시 확인').disabled) throw new Error('fenced login permits start/check');
    if (document.getElementById('codex-connection-details').hidden || codexSection().querySelector('[aria-controls]')) throw new Error('recovery controls can be hidden');
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
    chooseLanguage('en');
    await ${waitFor("document.querySelector('#claude-code-help')?.textContent.includes('not the email verification code')", "English code guidance")};
    if (document.querySelector('#claude-connection-code') !== codeInput || codeInput.value !== 'synthetic#state' || codeInput.type !== 'password') throw new Error('language change replaced or exposed the connection code');
    chooseLanguage('ko');
    await ${waitFor("document.querySelector('#claude-code-help')?.textContent.includes('이메일 확인 번호가 아니에요')", "Korean code guidance")};
    await ${waitFor("[...document.querySelectorAll('button')].some(b => b.textContent === '연결 확인' && !b.disabled)", "code submit enabled")};
    [...document.querySelectorAll('button')].find(b => b.textContent === '연결 확인').click();
    await ${waitFor("document.querySelector('[aria-label=\"Claude Code 연결\"]').textContent.includes('이 앱에 Claude Code 로그인이 연결됐어요')", "Claude code accepted")};
    if (document.querySelector('#claude-connection-code')) throw new Error('submitted code input was retained');
    await ${waitFor("document.getElementById('codex-connection-details').hidden && document.getElementById('claude-connection-details').hidden", "both signed-in panels compact")};
    window.m5SmokeLoginStatusFailure = true;
    await ${waitFor("!document.getElementById('codex-connection-details').hidden && document.querySelector('[aria-label=\"Codex 연결\"] [role=alert]')", "poll failure reveals connection controls")};
    if (!document.getElementById('claude-connection-details').hidden) throw new Error('Codex error expanded unrelated Claude');
    window.m5SmokeLoginStatusFailure = false;
    await ${waitFor("document.getElementById('codex-connection-details').hidden", "status restoration compacts connection")};
    await (${probeInstallConsent.toString()})();
    await ${waitFor("[...document.querySelectorAll('button')].some((button) => !button.disabled && (button.getAttribute('aria-label') === controlLabel || button.textContent?.trim() === controlLabel))", "enabled accessible project-selection control")};
    const projectButton = [...document.querySelectorAll("button")].find((button) => !button.disabled && (button.getAttribute("aria-label") === controlLabel || button.textContent?.trim() === controlLabel));
    if (!projectButton) throw new Error("renderer smoke lost its enabled accessible project-selection control after it was observed");
    // Saved projects are now a persistent rail, not a toolbar popover.
    await ${waitFor("document.querySelector('.m5-project-catalog-list li button') && !document.querySelector('.m5-project-catalog-list li button').disabled", "saved project catalog loaded")};
    if (document.querySelector('.m5-project-catalog-list li span').textContent !== '/work/earlier') throw new Error('catalog first page missing');
    const recoveryButton = text => [...document.querySelectorAll('[aria-label="프로젝트 시작 실패 복구"] button')].find(b => b.textContent === text);
    recoveryButton('시작 실패 확인').click();
    await ${waitFor("document.querySelector('[aria-label=\"실패한 준비 종료 확인\"]')", "explicit preparation inspection")};
    recoveryButton('이 준비 기록 종료').click();
    await ${waitFor("document.querySelector('[aria-label=\"프로젝트 시작 실패 복구\"]').textContent.includes('결과를 확인하지 못했어요')", "lost preparation response")};
    const pendingPreparation = document.querySelector('[aria-label="실패한 준비 종료 확인"]');
    const pendingId = pendingPreparation.querySelector('code').textContent;
    chooseLanguage('en');
    await ${waitFor("document.querySelector('[aria-label=\\\"Project startup failure recovery\\\"]')?.textContent.includes('Could not verify the outcome.')", "English unknown preparation outcome")};
    if (pendingPreparation !== document.querySelector('[aria-label="Confirm closing failed preparation"]') || pendingPreparation.querySelector('code').textContent !== pendingId) throw new Error('language switch reset preparation confirmation');
    if (document.querySelector('.m5-project-catalog').getAttribute('aria-label') !== 'Saved projects' || document.querySelector('.m5-project-catalog-list li span').textContent !== '/work/earlier') throw new Error('language switch lost saved project');
    chooseLanguage('ko');
    await ${waitFor("document.querySelector('[aria-label=\\\"프로젝트 시작 실패 복구\\\"]')?.textContent.includes('결과를 확인하지 못했어요')", "Korean preparation outcome restored")};
    recoveryButton('이 준비 기록 종료').click();
    await ${waitFor("document.querySelector('[aria-label=\"프로젝트 시작 실패 복구\"]').textContent.includes('실패한 준비 기록을 종료했어요')", "preparation retry acknowledgment")};
    if (document.querySelector('[aria-label="실패한 준비 종료 확인"]')) throw new Error('completed preparation confirmation retained');
    globalThis.cleanupConsentReport = await (${probeCleanupConsent.toString()})();
    [...document.querySelectorAll('.m5-project-catalog-list button')].find(b => b.textContent === '다음 목록').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li span')?.textContent === '/work/smoke'", "next catalog page")};
    if (document.querySelector('.m5-project-catalog-list li span').textContent !== '/work/smoke') throw new Error('catalog path missing');
    document.querySelector('.m5-project-catalog-list li button').click();
    await ${waitFor("document.querySelector('.m5-file-row')", "project tree")};
    await ${waitFor("document.getElementById('m5-install-details')?.hidden", "installation controls compact in workbench")};
    const installToggle = document.querySelector('.m5-provider-install header button');
    if (installToggle.getAttribute('aria-expanded') !== 'false') throw new Error('installation toggle state incorrect');
    installToggle.click();
    await ${waitFor("!document.getElementById('m5-install-details').hidden", "installation controls expanded explicitly")};
    await (${probeInstallConsent.toString()})();
    installToggle.click();
    await ${waitFor("document.getElementById('m5-install-details').hidden", "installation controls collapsed after recovery")};
    document.querySelector('.m5-file-row').click();
    await ${waitFor("document.querySelector('.m5-breadcrumbs [aria-current=location]')?.textContent === '/ src'", "folder breadcrumb")};
    document.querySelector('.m5-breadcrumbs .m5-root').click();
    await ${waitFor("document.querySelector('.m5-file-row .m5-tree-name')?.textContent === 'src' && !document.querySelector('.m5-breadcrumbs [aria-current]')", "root breadcrumb")};
    document.querySelector('[data-panel=editor] .m5-dock-options summary').click();
    [...document.querySelectorAll('[data-panel=editor] .m5-dock-options button')].find(button => button.textContent === '편집기 패널 접기').click();
    await ${waitFor("document.querySelector('[data-panel=editor]').hidden", "editor hidden before search")};
    const fileSearch = document.querySelector('[aria-label="파일 이름 또는 경로 찾기"]');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(fileSearch, 'EXAMPLE.TS');
    fileSearch.dispatchEvent(new Event('input', { bubbles: true }));
    await ${waitFor("!document.querySelector('.m5-file-search button[type=submit]').disabled", "file search enabled")};
    document.querySelector('.m5-file-search button[type=submit]').click();
    await ${waitFor("document.querySelector('.m5-search-results button')?.textContent === 'src/example.ts'", "recursive file search result")};
    if (document.querySelector('.m5-file-row .m5-tree-name').textContent !== 'src') throw new Error('search changed folder navigation');
    const searchLanguage = document.querySelector('.m5-toolbar-actions select');
    searchLanguage.value = 'en'; searchLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('.m5-file-search input')?.getAttribute('aria-label') === 'Find by file name or path'", "English file search")};
    if (fileSearch !== document.querySelector('.m5-file-search input') || fileSearch.value !== 'EXAMPLE.TS' || document.querySelector('.m5-search-results button').textContent !== 'src/example.ts') throw new Error('language switch lost search');
    searchLanguage.value = 'ko'; searchLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('.m5-file-search input')?.getAttribute('aria-label') === '파일 이름 또는 경로 찾기'", "Korean search restored")};
    document.querySelector('.m5-search-results button').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && document.querySelector('.monaco-editor') && Object.hasOwn(globalThis, 'MonacoEnvironment') && typeof globalThis.MonacoEnvironment.getWorker === 'function'", "Monaco mount and reveal")};
    const localWorker = globalThis.MonacoEnvironment.getWorker("smoke", "editor");
    localWorker.terminate();
    const editorNode = document.querySelector('.monaco-editor');
    document.querySelector('[data-panel=editor] .m5-dock-options summary').click();
    [...document.querySelectorAll('[data-panel=editor] .m5-dock-options button')].find(button => button.textContent === '편집기 패널 접기').click();
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
    const failedSettingsDetails = [...document.querySelectorAll('[role=alert] pre')].map(node => node.textContent).join('|');
    const settingsLanguage = document.querySelector('.m5-toolbar-actions select');
    settingsLanguage.value = 'en'; settingsLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('.m5-notice')?.textContent === 'Finish role and model settings to continue working with files.'", "English file-operation block notice")};
    await ${waitFor("document.body.textContent.includes('Could not save settings. Your input is preserved.')", "English persisted settings error")};
    if (modelField !== document.querySelector('.m5-role-card input') || modelField.value !== 'draft-model' || failedSettingsDetails !== [...document.querySelectorAll('[role=alert] pre')].map(node => node.textContent).join('|')) throw new Error('error translation changed draft or technical details');
    settingsLanguage.value = 'ko'; settingsLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.body.textContent.includes('설정을 저장하지 못했어요')", "Korean persisted settings error")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '저장된 설정 다시 불러오기').click();
    await ${waitFor("document.querySelector('.m5-settings [role=alertdialog]')", "failed-save draft reload confirmation")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '설정 변경 버리고 다시 불러오기').click();
    await ${waitFor("document.querySelector('.m5-role-card input').value === 'test-model'", "explicit settings reload")};
    if (document.querySelector('.m5-chat-notice')?.textContent !== '저장된 역할과 모델을 다시 불러왔어요.') throw new Error('chat status notice missing');
    settingsLanguage.value = 'en'; settingsLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('.m5-chat-notice')?.textContent === 'Reloaded saved roles and models.'", "English chat status notice")};
    if (document.querySelector('.m5-role-card input').value !== 'test-model') throw new Error('notice language changed role value');
    settingsLanguage.value = 'ko'; settingsLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('.m5-chat-notice')?.textContent === '저장된 역할과 모델을 다시 불러왔어요.'", "Korean chat status notice restored")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '작업대로 돌아가기').click();
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('settings reload remounted editor');
    [...document.querySelectorAll('button')].find(button => button.textContent.trim() === '기본 배치로').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && document.querySelectorAll('.m5-dock-separator').length === 2", "reset panel visibility")};
    const editorOptions = document.querySelector('[data-panel=editor] .m5-dock-options');
    editorOptions.querySelector('summary').click();
    if (!editorOptions.open) throw new Error('panel options did not open');
    [...editorOptions.querySelectorAll('button')].find(button => button.textContent === '편집기 패널에 집중').click();
    await ${waitFor("document.querySelector('[data-panel=files]').hidden && document.querySelector('[data-panel=changes]').hidden && document.querySelectorAll('.m5-dock-separator').length === 0", "focus editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('focus remounted Monaco');
    document.querySelector('.m5-dock-restore').click();
    await ${waitFor("!document.querySelector('[data-panel=files]').hidden && document.querySelectorAll('.m5-dock-separator').length === 2", "restore pre-focus layout")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('focus restore remounted Monaco');
    editorOptions.querySelector('summary').click();
    [...editorOptions.querySelectorAll('button')].find(button => button.textContent === '편집기 패널에 집중').click();
    await ${waitFor("document.querySelectorAll('.m5-dock-separator').length === 0", "focus editor again")};
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '파일').click();
    await ${waitFor("!document.querySelector('[data-panel=files]').hidden", "reopen files")};
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '변경').click();
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    [...document.querySelectorAll('.m5-dock-group-tabs button')].find(button => button.textContent === '변경' && button.getClientRects().length)?.click();
    await ${waitFor("!document.querySelector('[data-panel=changes]').hidden", "reopen changes")};
    editorOptions.querySelector('summary').click();
    [...editorOptions.querySelectorAll('button')].find(button => button.textContent === '편집기 패널 접기').click();
    await ${waitFor("document.querySelector('[data-panel=editor]').hidden", "collapse editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('collapse discarded Monaco');
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '편집기').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden", "restore editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('restore remounted Monaco');
    const place = (source, target) => { const select = document.querySelector('[aria-label="' + source + ' 패널 배치"]'); select.closest('details').querySelector('summary').click(); select.value = target; select.dispatchEvent(new Event('change', {bubbles:true})); };
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
  })()`;
  const result = await window.webContents.executeJavaScript(mountScript).catch(error => {
    const line = /<anonymous>:(\d+):/.exec(error.stack || '')?.[1];
    if (line) process.stderr.write(`Synthetic mount statement: ${mountScript.split('\n')[Number(line) - 1]}\n`);
    throw error;
  });
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
    const confirmation = document.querySelector('[role=alertdialog]');
    const editorBeforeLanguage = document.querySelector('.monaco-editor');
    const draftBeforeLanguage = editorBeforeLanguage.textContent;
    const editorLanguage = document.querySelector('.m5-toolbar-actions select');
    editorLanguage.value = 'en'; editorLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('[role=alertdialog]')?.getAttribute('aria-label') === 'Confirm discarding edits'", "English editor confirmation")};
    if (confirmation !== document.querySelector('[role=alertdialog]') || editorBeforeLanguage !== document.querySelector('.monaco-editor') || editorBeforeLanguage.textContent !== draftBeforeLanguage) throw new Error('language switch lost editor or confirmation');
    if (!document.querySelector('[aria-label="Unsaved changes"]') || !confirmation.textContent.includes('Copy anything you need first.')) throw new Error('missing English draft warning');
    editorLanguage.value = 'ko'; editorLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('[role=alertdialog]')?.getAttribute('aria-label') === '편집 내용 버리기 확인'", "Korean editor confirmation restored")};
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
    const savedLayout = localStorage.getItem('hello-agent.m5.dock.v1');
    const panelStates = () => [...document.querySelectorAll('.m5-dock-slot')].map(node => [node.dataset.panel, node.hidden, node.getAttribute('style')]);
    const originalPanels = JSON.stringify(panelStates());
    for (const language of ['en', 'ko']) {
      const select = document.querySelector('.m5-toolbar-actions select');
      select.value = language;
      select.dispatchEvent(new Event('change', {bubbles:true}));
      await new Promise(resolve => setTimeout(resolve, 30));
      const dirtyLabel = language === 'en' ? 'Unsaved changes' : '저장되지 않은 변경';
      if (localStorage.getItem('hello-agent.m5.dock.v1') !== savedLayout || JSON.stringify(panelStates()) !== originalPanels) throw new Error('language switch changed dock layout');
      if (document.querySelector('.m5-dock-visibility').getAttribute('aria-label') !== (language === 'en' ? 'Panel visibility' : '패널 표시')) throw new Error('dock visibility not translated');
      if (document.querySelector('[data-panel=editor] select').getAttribute('aria-label') !== (language === 'en' ? 'Place Editor panel' : '편집기 패널 배치')) throw new Error('dock placement not translated');
      if (document.querySelector('main').lang !== language || document.querySelector('.monaco-editor') !== originalEditor || ![...document.querySelectorAll('[aria-label]')].some(node => node.getAttribute('aria-label') === dirtyLabel)) throw new Error('language switch lost the active editor draft');
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
    const fileTabs = () => [...document.querySelectorAll('.m5-tab-list [role=tab]')];
    for (const [key, index] of [['Home',0], ['ArrowLeft',3], ['ArrowRight',0], ['End',3]]) {
      const selected = fileTabs().find(tab => tab.getAttribute('aria-selected') === 'true');
      selected.focus();
      selected.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
      await new Promise((resolve, reject) => {
        const deadline = Date.now() + 3000;
        const timer = setInterval(() => {
          const tabs = fileTabs();
          if (tabs[index]?.getAttribute('aria-selected') === 'true' && document.activeElement === tabs[index]) {
            clearInterval(timer);
            if (tabs.filter(tab => tab.tabIndex === 0).length !== 1) reject(new Error('file tabs have multiple keyboard entry points'));
            else resolve();
          } else if (Date.now() > deadline) { clearInterval(timer); reject(new Error('file tab keyboard selection failed: ' + key)); }
        }, 25);
      });
    }
    await ${waitFor("document.querySelector('.m5-monaco')?.getAttribute('data-mode-id') === 'json'", "JSON tab ready for draft")};
    document.querySelector('.monaco-editor textarea').focus();
  })()`);
  await window.webContents.debugger.sendCommand("Input.insertText", { text: "unsaved-tab-draft" });
  await window.webContents.executeJavaScript(`(async () => {
    await ${waitFor("document.querySelector('.m5-editor-tab [aria-label=\"저장되지 않은 변경\"]') && document.querySelector('.view-lines')?.textContent.includes('unsaved-tab-draft')", "unsaved JSON draft")};
    const draft = document.querySelector('.view-lines').textContent;
    const selectedFile = () => document.querySelector('.m5-tab-list [aria-selected=true]');
    selectedFile().dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true, cancelable: true }));
    await ${waitFor("document.querySelector('.m5-monaco')?.getAttribute('data-mode-id') === 'typescript'", "leave unsaved JSON tab")};
    if (!document.querySelector('.m5-editor-tab [aria-label="저장되지 않은 변경"]')) throw new Error('tab switch lost dirty marker');
    selectedFile().dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true, cancelable: true }));
    await ${waitFor("document.querySelector('.m5-monaco')?.getAttribute('data-mode-id') === 'json' && document.querySelector('.view-lines')?.textContent.includes('unsaved-tab-draft')", "return to unsaved JSON draft")};
    if (document.querySelector('.view-lines').textContent !== draft) throw new Error('tab switch changed draft content');
    [...document.querySelectorAll('.m5-editor-head button')].find(button => button.textContent === '저장').click();
    await ${waitFor("!document.querySelector('.m5-editor-tab [aria-label=\"저장되지 않은 변경\"]')", "save preserved draft")};
    const jsonWorker = globalThis.MonacoEnvironment.getWorker('smoke', 'json');
    jsonWorker.terminate();
    document.querySelector('.m5-change-row').click();
    await ${waitFor("document.querySelectorAll('.m5-diff-lines tbody tr').length === 200", "bounded diff page")};
    if (!document.querySelector('.m5-diff-deleted') || !document.querySelector('.m5-diff-added')) throw new Error('missing distinct change rows');
    const wrapToggle = document.querySelector('.m5-diff-wrap input');
    const diffBox = document.querySelector('.m5-diff-lines');
    const changesToggle = [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '변경');
    if (changesToggle.getAttribute('aria-pressed') === 'false') changesToggle.click();
    await ${waitFor("!document.querySelector('[data-panel=changes]').hidden", "changes visible for wrap geometry")};
    if (!wrapToggle?.checked) throw new Error('diff should wrap by default');
    const wrapText = diffBox.textContent;
    const originalDiffStyle = diffBox.getAttribute('style');
    diffBox.style.width = '240px';
    const probeCode = diffBox.querySelector('code');
    const originalProbeText = probeCode.textContent;
    probeCode.textContent = '+' + 'long-unbroken-diff-content'.repeat(30);
    if (getComputedStyle(probeCode).whiteSpace !== 'pre-wrap' || getComputedStyle(probeCode).overflowWrap !== 'anywhere') throw new Error('diff wrap rules missing');
    if (diffBox.clientWidth === 0 || diffBox.scrollWidth > diffBox.clientWidth + 1) throw new Error('wrapped diff overflows narrow viewport');
    probeCode.textContent = originalProbeText;
    wrapToggle.click();
    await ${waitFor("!document.querySelector('.m5-diff-lines-wrapped')", "diff wrapping disabled")};
    if (diffBox.textContent !== wrapText || getComputedStyle(probeCode).whiteSpace !== 'pre') throw new Error('wrap toggle changed diff source');
    wrapToggle.click();
    await ${waitFor("document.querySelector('.m5-diff-lines-wrapped')", "diff wrapping restored")};
    if (originalDiffStyle === null) diffBox.removeAttribute('style'); else diffBox.setAttribute('style', originalDiffStyle);
    const next = () => [...document.querySelectorAll('button')].find(button => button.textContent === '다음 변경');
    next().click();
    await ${waitFor("document.querySelector('.m5-diff-pages span').textContent.startsWith('2 /')", "second diff page")};
    const diffLanguage = document.querySelector('.m5-toolbar-actions select');
    const diffContents = document.querySelector('.m5-diff-lines tbody').textContent;
    diffLanguage.value = 'en'; diffLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('.m5-diff-pages span').textContent.startsWith('Page 2 /')", "English diff pagination")};
    if (document.querySelector('.m5-diff-lines tbody').textContent !== diffContents) throw new Error('language switch changed diff contents or page');
    if (!document.querySelector('.m5-diff-wrap').textContent.includes('Wrap long lines')) throw new Error('diff wrap label not translated');
    diffLanguage.value = 'ko'; diffLanguage.dispatchEvent(new Event('change', { bubbles: true }));
    await ${waitFor("document.querySelector('.m5-diff-pages span').textContent.startsWith('2 /')", "Korean diff restored")};
    next().click();
    await ${waitFor("document.querySelectorAll('.m5-diff-lines tbody tr').length === 3", "last diff page")};
    if (!next().disabled || !document.querySelector('.m5-diff-lines').textContent.includes('+line 401')) throw new Error('last diff lines missing');
    document.querySelector('.m5-change-row').click();
    await ${waitFor("document.querySelector('.m5-diff-pages span').textContent.startsWith('1 /')", "reopened diff resets page")};
    document.querySelector('.m5-chat-chip').click();
    await ${waitFor("document.querySelector('.m5-followup button:not(:disabled)') || document.querySelector('#m5-followup-text')", "follow-up composer")};
    const input = document.querySelector('#m5-followup-text');
    const conversationToggle = [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '팀 대화');
    if (conversationToggle.getAttribute('aria-pressed') === 'false') conversationToggle.click();
    await ${waitFor("!document.querySelector('[data-panel=conversation]').hidden", "conversation visible for geometry")};
    const conversation = document.querySelector('.m5-conversation');
    const savedConversationStyle = conversation.getAttribute('style');
    try {
      for (const height of [280, 500]) {
        conversation.style.cssText = 'flex: none; height: ' + height + 'px';
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const bounds = conversation.getBoundingClientRect();
        const composer = document.querySelector('.m5-followup').getBoundingClientRect();
        const history = document.querySelector('.m5-conversation-scroll');
        const historyBounds = history.getBoundingClientRect();
        if (historyBounds.height < 1 || composer.height > bounds.height / 2 + 1 || composer.bottom > bounds.bottom + 1 || historyBounds.bottom > composer.top + 1) throw new Error('composer geometry: ' + JSON.stringify({height, bounds: bounds.toJSON(), composer: composer.toJSON(), history: historyBounds.toJSON()}));
        history.scrollTop = history.scrollHeight;
        if (Math.abs(document.querySelector('.m5-followup').getBoundingClientRect().bottom - composer.bottom) > 1) throw new Error('history scrolling moved the composer');
      }
    } finally {
      if (savedConversationStyle === null) conversation.removeAttribute('style');
      else conversation.setAttribute('style', savedConversationStyle);
    }
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(input, '설명을 추가해 주세요.');
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await ${waitFor("!document.querySelector('.m5-followup button').disabled", "enabled follow-up submit")};
    const previousMessages = [...document.querySelectorAll('.m5-message p')].map(node => node.textContent);
    const goalInput = document.querySelector('.m5-new-chat-form input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(goalInput, '새 목표 초안');
    goalInput.dispatchEvent(new Event('input', { bubbles: true }));
    const selectedChat = document.querySelector('.m5-chat-chip.selected');
    const languageSelect = document.querySelector('.m5-toolbar-actions select');
    languageSelect.value = 'en';
    languageSelect.dispatchEvent(new Event('change', {bubbles:true}));
    await ${waitFor("document.querySelector('.m5-followup button')?.textContent === 'Add to conversation' && document.querySelector('.m5-timeline')?.getAttribute('aria-label') === 'Team conversation'", "English team conversation")};
    if (input !== document.querySelector('#m5-followup-text') || input.value !== '설명을 추가해 주세요.') throw new Error('language change lost follow-up draft');
    if (goalInput !== document.querySelector('.m5-new-chat-form input') || goalInput.value !== '새 목표 초안' || goalInput.getAttribute('aria-label') !== 'New conversation goal') throw new Error('language change lost goal draft or label');
    if (selectedChat !== document.querySelector('.m5-chat-chip.selected')) throw new Error('language change changed selected chat');
    // Layout-only fixture: exercise a long title at the product minimum window
    // size without changing stored chat data or the selected conversation.
    const goalLabel = selectedChat.querySelector('.m5-chat-goal');
    const savedLabel = goalLabel.textContent;
    try {
      goalLabel.textContent = 'A very long project conversation title '.repeat(12);
      const chip = selectedChat.getBoundingClientRect();
      const goal = goalLabel.getBoundingClientRect();
      const status = selectedChat.querySelector('.m5-chat-status, .m5-chat-update').getBoundingClientRect();
      if (goal.width <= 0 || status.width <= 0 || goal.right > status.left + 1 || status.right > chip.right + 1) throw new Error('long chat title obscures status');
      selectedChat.focus();
      if (document.activeElement !== selectedChat) throw new Error('chat chip cannot receive keyboard focus');
    } finally { goalLabel.textContent = savedLabel; }
    if (document.querySelector('.m5-files').getAttribute('aria-label') !== 'Files' || !document.querySelector('.m5-tree-pages').textContent.includes('First page')) throw new Error('file navigation not translated');
    if (JSON.stringify(previousMessages) !== JSON.stringify([...document.querySelectorAll('.m5-message p')].map(node => node.textContent))) throw new Error('language change translated user messages');
    document.querySelector('.m5-followup button').click();
    await ${waitFor("[...document.querySelectorAll('.m5-message p')].some(node => node.textContent === '설명을 추가해 주세요.') && document.querySelector('#m5-followup-text').value === ''", "persisted follow-up visible and input cleared")};
    if ([...document.querySelectorAll('.m5-message p')].filter(node => node.textContent === '설명을 추가해 주세요.').length !== 1) throw new Error('duplicate follow-up message');
    languageSelect.value = 'ko';
    languageSelect.dispatchEvent(new Event('change', {bubbles:true}));
    await ${waitFor("document.querySelector('.m5-followup button')?.textContent === '대화에 추가'", "Korean team conversation restored")};
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
    const changeSettingsLanguage = language => {
      const select = document.querySelector('.m5-toolbar-actions select');
      select.value = language;
      select.dispatchEvent(new Event('change', {bubbles:true}));
    };
    changeSettingsLanguage('en');
    await ${waitFor("document.querySelector('.m5-settings h2')?.textContent === 'Roles and models'", "English role settings")};
    if (document.querySelector('.m5-settings input') !== modelInput || modelInput.value !== 'unsaved-model' || !leave.disabled) throw new Error('language switch lost or unlocked role settings draft');
    if (document.querySelector('.m5-role-card strong').textContent !== 'Planning') throw new Error('role name is not translated');
    changeSettingsLanguage('ko');
    await ${waitFor("document.querySelector('.m5-settings h2')?.textContent === '역할과 모델'", "Korean role settings")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '작업대로 돌아가기').click();
    await ${waitFor("document.querySelector('[aria-label=\"저장하지 않은 설정 확인\"]')", "role close confirmation")};
    changeSettingsLanguage('en');
    await ${waitFor("document.querySelector('.m5-settings [role=alertdialog]')?.textContent.includes('Discard your unsaved role and model settings?')", "English discard confirmation")};
    if (modelInput.value !== 'unsaved-model') throw new Error('translating discard confirmation changed draft');
    changeSettingsLanguage('ko');
    await ${waitFor("document.querySelector('[aria-label=\"저장하지 않은 설정 확인\"]')", "Korean discard confirmation retained")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '설정 계속 편집').click();
    await ${waitFor("!document.querySelector('.m5-settings [role=alertdialog]')", "role draft retained")};
    if (document.querySelector('.m5-settings input').value !== 'unsaved-model') throw new Error('cancel lost role draft');
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '저장된 설정 다시 불러오기').click();
    await ${waitFor("document.querySelector('.m5-settings [role=alertdialog]')", "role reload confirmation")};
    [...document.querySelectorAll('.m5-settings button')].find(button => button.textContent === '설정 변경 버리고 다시 불러오기').click();
    await ${waitFor("![...document.querySelectorAll('.m5-settings [role=status]')].some(node => node.textContent === '저장하지 않은 설정 변경이 있어요.') && !document.querySelector('.m5-settings [role=alertdialog]')", "role settings restored")};
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
    [...document.querySelectorAll('.m5-project-catalog-list button')].find(b => b.textContent === '처음부터 다시 확인').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li span')?.textContent === '/work/earlier'", "catalog reopened")};
    [...document.querySelectorAll('.m5-project-catalog-list button')].find(b => b.textContent === '다음 목록').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li span')?.textContent === '/work/smoke'", "saved project available again")};
    document.querySelector('.m5-project-catalog-list li button').click();
    await ${waitFor("document.querySelector('.m5-chat-chip')", "saved chat available again")};
    document.querySelector('.m5-chat-chip').click();
    await ${waitFor("[...document.querySelectorAll('.m5-message p')].some(node => node.textContent === '설명을 추가해 주세요.')", "chat history preserved after reopening")};
    window.m5SmokeAppendMessages(450);
    await ${waitFor("document.querySelectorAll('.m5-message').length === 200 && [...document.querySelectorAll('.m5-message p')].at(-1)?.textContent === 'history message 450'", "bounded latest conversation page")};
    const historyButton = label => [...document.querySelectorAll('.m5-timeline nav button')].find(button => button.textContent === label);
    historyButton('이전 대화').click();
    await ${waitFor("[...document.querySelectorAll('.m5-message p')].at(-1)?.textContent === 'history message 250'", "older conversation page")};
    const oldPage = [...document.querySelectorAll('.m5-message p')].map(node => node.textContent);
    window.m5SmokeAppendMessages(1);
    await ${waitFor("document.querySelector('.m5-new-messages')", "new message notice while reading history")};
    if (JSON.stringify(oldPage) !== JSON.stringify([...document.querySelectorAll('.m5-message p')].map(node => node.textContent))) throw new Error('new message moved the older page');
    historyButton('이전 대화').click();
    await ${waitFor("document.querySelector('.m5-message p')?.textContent === '처음 요청'", "first conversation page still reachable")};
    historyButton('최신 대화').click();
    await ${waitFor("document.querySelectorAll('.m5-message').length === 200 && [...document.querySelectorAll('.m5-message p')].at(-1)?.textContent === 'history message 451' && !document.querySelector('.m5-new-messages')", "return to latest conversation page")};
    ${process.env.M5_MEASURE_MESSAGE_PAINT === "1" ? `globalThis.messagePaintReport = await (${measureMessagePaint.toString()})({ emit: () => window.m5SmokeAppendMessages(1)[0], visible: () => [...document.querySelectorAll('.m5-message p')].map(node => node.textContent) });` : ""}
    const originalRequest = window.m5Projects.request;
    ${process.env.M5_MEASURE_PARALLEL_PAINT === "1" ? `
    window.m5Projects.request = originalRequest;
    [...document.querySelectorAll('button')].find(button => button.textContent.trim() === '프로젝트 화면 닫기').click();
    await ${waitFor("!document.querySelector('#m5-followup-text')", "parallel project closed")};
    window.m5SmokeEnableParallelChats();
    [...document.querySelectorAll('.m5-project-catalog-list button')].find(b => b.textContent === '처음부터 다시 확인').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li span')?.textContent === '/work/earlier'", "parallel catalog reset")};
    [...document.querySelectorAll('.m5-project-catalog-list button')].find(b => b.textContent === '다음 목록').click();
    await ${waitFor("document.querySelector('.m5-project-catalog-list li span')?.textContent === '/work/smoke'", "parallel project available")};
    document.querySelector('.m5-project-catalog-list li button').click();
    await ${waitFor("document.querySelectorAll('.m5-chat-chip').length === 4", "four chats listed")};
    document.querySelector('.m5-chat-chip').click();
    await ${waitFor("document.querySelector('#m5-followup-text')", "parallel chat selected")};
    globalThis.parallelPaintReport = await (${measureMessagePaint.toString()})({
      emit: () => window.m5SmokeEmitParallelRound()[0].text,
      visible: () => [...document.querySelectorAll('.m5-message p')].map(node => node.textContent),
    });
    parallelPaintReport.sourceCounts = window.m5SmokeParallelCounts();
    parallelPaintReport.restoredChats = [];
    for (let index = 0; index < 4; index++) {
      document.querySelectorAll('.m5-chat-chip')[index].click();
      await new Promise((resolve, reject) => {
        const deadline = Date.now() + 7000;
        const timer = setInterval(() => {
          const messages = [...document.querySelectorAll('.m5-message p')].map(node => node.textContent);
          if (messages.length === 200 && messages.every((text, offset) => text === 'paint_chat_' + index + ' message ' + (2801 + offset))) {
            clearInterval(timer); resolve();
          } else if (Date.now() > deadline) { clearInterval(timer); reject(new Error('parallel chat restore failed: ' + index)); }
        }, 25);
      });
      parallelPaintReport.restoredChats.push('paint_chat_' + index);
    }
    parallelPaintReport.limitations = ['Four synthetic sources; only chat 0 timed. Background chats checked after selection, not painted during input.', 'Two animation frames are a presentation proxy, not a compositor timestamp. No native sidecar transport, packaged startup, or complete G6 gate.'];
    ` : ""}
    window.m5Projects.request = input => input.action === 'releaseView' ? Promise.reject(new Error('synthetic release failure')) : originalRequest(input);
    [...document.querySelectorAll('button')].find(button => button.textContent.trim() === '프로젝트 화면 닫기').click();
    await ${waitFor("[...document.querySelectorAll('[role=alert]')].some(node => node.textContent.includes('파일 감시 해제를 확인하지 못해'))", "failed release blocks leaving")};
    if (!document.querySelector('#m5-followup-text') || document.querySelector('h1').textContent !== 'smoke') throw new Error('failed release cleared the project');
  })()`);
  process.stdout.write(`${JSON.stringify(result)}\n`);
  process.stdout.write(JSON.stringify({ cleanupConsent: await window.webContents.executeJavaScript('globalThis.cleanupConsentReport') }) + '\n');
  if (process.env.M5_MEASURE_PARALLEL_PAINT === "1") {
    const measurement = await window.webContents.executeJavaScript('globalThis.parallelPaintReport');
    const evidencePath = resolve(userData, 'parallel-message-paint.json');
    writeFileSync(evidencePath, JSON.stringify(measurement, null, 2));
    process.stdout.write(JSON.stringify({ evidencePath, diagnostic: true, maxPaintMs: measurement.maxPaintMs, sourceCounts: measurement.sourceCounts, restoredChats: measurement.restoredChats }) + '\n');
    if (measurement.observationError || measurement.unobserved.length || measurement.maxPaintMs > 100) throw new Error('diagnostic parallel message paint limit failed');
  }
  if (process.env.M5_MEASURE_MESSAGE_PAINT === "1") {
    const measurement = await window.webContents.executeJavaScript('globalThis.messagePaintReport');
    const evidencePath = resolve(userData, 'message-paint.json');
    writeFileSync(evidencePath, JSON.stringify(measurement, null, 2));
    process.stdout.write(JSON.stringify({ evidencePath, diagnostic: true, maxPaintMs: measurement.maxPaintMs, unobserved: measurement.unobserved.length }) + '\n');
    if (measurement.observationError || measurement.unobserved.length || measurement.maxPaintMs > 100) throw new Error('diagnostic message paint limit failed');
  }
  await window.loadURL("m5smoke://m5/index.html");
  await window.webContents.executeJavaScript(`(async () => {
    await ${waitFor("document.querySelector('[aria-label=\"Codex 연결\"]')?.textContent.includes('로그인이 연결됐어요')", "saved agent selection after reload")};
    if (document.querySelector('.m5-provider-selection form')) throw new Error('saved agent choice was lost');
    if (!document.querySelector('[aria-label="Claude Code 연결"]')) throw new Error('added agent lost after reload');
  })()`);
  if (window.isVisible() || window.isFocused()) throw new Error("renderer smoke captured desktop focus");
  window.webContents.debugger.detach();
  await window.close();
  app.exit(0);
}).catch(fail);
