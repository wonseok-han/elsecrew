// Executed inside the existing real Chromium renderer; never runs an installer.
async function probeInstallConsent() {
  const panel = document.querySelector('.m5-agent-management');
  if (!panel) throw new Error('installation panel missing');
  const button = text => {
    if (text === 'Codex 설치 내용 확인' || text === 'Claude Code 설치 내용 확인') {
      const provider = text.startsWith('Codex') ? 'codex' : 'claude';
      return panel.querySelector('#' + provider + '-installation-slot .m5-provider-install-inline button');
    }
    return [...panel.querySelectorAll('button')].find(node => node.textContent === text || node.getAttribute('aria-label') === text);
  };
  const wait = async predicate => {
    const deadline = Date.now() + 5000;
    while (!predicate()) {
      if (Date.now() > deadline) throw new Error('installation UI timeout: ' + panel.textContent);
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  };
  const original = window.m5Install.request;
  const idle = {state:'idle', operationId:null, provider:null, plan:null, error:null, cancelRequested:false};
  let status = {...idle};
  let starts = 0;
  let cancels = 0;
  let recoveries = 0;
  let revision = 0;
  const operationId = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee';
  window.m5Install.request = async input => {
    if (input.action === 'prepare') {
      const suffixes = input.provider === 'codex' ? ['/.local/bin', '/.codex/packages/standalone']
        : ['/.local/bin', '/.claude/downloads', '/.local/share/claude', '/.local/state/claude', '/.cache/claude'];
      status = {...idle, state:'awaitingConsent', provider:input.provider, plan:{
        planId:'11111111-2222-3333-4444-' + String(++revision).padStart(12, '0'),
        provider:input.provider, action:'installOrUpdate', expiresInSeconds:180,
        writeDirectories:suffixes.map(path => '/Users/synthetic' + path),
        replacesExistingCommands:true, shellSettingsWritable:false, startsLogin:false,
      }};
    } else if (input.action === 'start') {
      if (input.planId !== status.plan?.planId || input.provider !== status.provider) throw new Error('wrong installation plan');
      starts++;
      status = {...idle, state:'installing', operationId, provider:input.provider};
      await new Promise(resolve => setTimeout(resolve, 40));
      throw new Error('synthetic start response lost');
    } else if (input.action === 'cancel') {
      if (input.operationId !== operationId) throw new Error('wrong cancellation');
      cancels++;
      status = {...status, state:'recoveryRequired', error:'providerInstallRecoveryRequired'};
    } else if (input.action === 'recover') {
      if (input.operationId !== operationId) throw new Error('wrong recovery');
      recoveries++;
      status = {...idle};
    } else if (input.action === 'dismiss') status = {...idle};
    else if (!['status', 'inspectRecovery'].includes(input.action)) throw new Error('unexpected install request');
    return structuredClone(status);
  };
  try {
    window.dispatchEvent(new Event('m5-install-status-refresh'));
    await wait(() => !(button('Codex 설치 내용 확인')?.disabled ?? true));
    button('Codex 설치 내용 확인').click();
    await wait(() => panel.querySelector('.m5-install-confirmation')?.open);
    const confirm = () => panel.querySelector('.m5-install-confirmation button[type=submit]');
    if (confirm().disabled || panel.querySelector('.m5-install-confirmation input[type=checkbox]') || panel.querySelectorAll('.m5-install-paths li').length !== 2 || starts) throw new Error('confirmation must require only final submit');
    button('취소').click();
    await wait(() => !panel.querySelector('.m5-install-confirmation'));
    if (starts) throw new Error('cancel started installation');
    button('Claude Code 설치 내용 확인').click();
    await wait(() => panel.querySelector('.m5-install-confirmation')?.open && panel.querySelectorAll('.m5-install-paths li').length === 5);
    if (!panel.querySelector('#m5-install-confirm-title').textContent.includes('Claude Code') || starts) throw new Error('wrong confirmation provider or automatic start');
    confirm().click(); confirm().click();
    await wait(() => panel.querySelector('[role=alert]'));
    if (panel.querySelector('#m5-install-details').hidden) throw new Error('uncertain installation hidden');
    const compactToggle = panel.querySelector('header button[aria-expanded]');
    if (compactToggle && (!compactToggle.disabled || compactToggle.getAttribute('aria-expanded') !== 'true')) throw new Error('active installation can be hidden');
    const managementToggle = document.querySelector('.m5-management-toggle');
    if (managementToggle) {
      await wait(() => managementToggle.disabled && managementToggle.getAttribute('aria-expanded') === 'true');
      managementToggle.click();
      if (document.getElementById('m5-agent-management-content').hidden) throw new Error('uncertain installation hidden by management wrapper');
    }
    await wait(() => button('설치 중단 요청') && !button('설치 중단 요청').disabled);
    if (starts !== 1 || !(button('Codex 설치 내용 확인')?.disabled ?? true)) throw new Error('uncertain start repeated or parallel install allowed');
    const activePanel = panel.querySelector('.m5-provider-install[data-active]');
    if (!activePanel || activePanel.querySelector('.m5-install-provider-actions') || !activePanel.querySelector('header .m5-install-stop') || activePanel.querySelector('.m5-install-refresh')) throw new Error('active install controls are not compact');
    button('설치 중단 요청').click();
    await wait(() => button('종료 확인된 설치 기록 정리'));
    button('종료 확인된 설치 기록 정리').click();
    await wait(() => !(button('Codex 설치 내용 확인')?.disabled ?? true));
    if (starts !== 1 || cancels !== 1 || recoveries !== 1) throw new Error('installation lifecycle call counts differ');
    if (managementToggle) {
      status = {...idle, state:'installed', operationId, provider:'codex'};
      window.dispatchEvent(new Event('m5-install-status-refresh'));
      await wait(() => !managementToggle.disabled);
      managementToggle.click();
      await wait(() => document.getElementById('m5-agent-management-content').hidden);
      managementToggle.click();
      await wait(() => !document.getElementById('m5-agent-management-content').hidden);
    }
  } finally {
    window.m5Install.request = original;
    window.dispatchEvent(new Event('m5-install-status-refresh'));
    await wait(() => button('Codex 설치 내용 확인')?.disabled);
  }
}
module.exports = { probeInstallConsent };
