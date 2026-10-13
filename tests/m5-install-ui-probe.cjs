// Executed inside the existing real Chromium renderer; never runs an installer.
async function probeInstallConsent() {
  const panel = document.querySelector('.m5-provider-install');
  if (!panel) throw new Error('installation panel missing');
  const button = text => [...panel.querySelectorAll('button')].find(node => node.textContent === text);
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
    button('상태 다시 확인').click();
    await wait(() => !button('Codex 설치 내용 확인').disabled);
    button('Codex 설치 내용 확인').click();
    await wait(() => button('동의하고 설치'));
    if (!button('동의하고 설치').disabled || panel.querySelectorAll('li').length !== 2) throw new Error('Codex consent or locations missing');
    button('동의하고 설치').click();
    if (starts) throw new Error('installation without consent');
    panel.querySelector('input').click();
    await wait(() => !button('동의하고 설치').disabled);
    button('Claude Code 설치 내용 확인').click();
    await wait(() => panel.querySelectorAll('li').length === 5);
    if (panel.querySelector('input').checked || !button('동의하고 설치').disabled) throw new Error('new plan reused consent');
    panel.querySelector('input').click();
    await wait(() => !button('동의하고 설치').disabled);
    button('동의하고 설치').click(); button('동의하고 설치').click();
    await wait(() => panel.querySelector('[role=alert]'));
    await wait(() => button('설치 중단 요청') && !button('설치 중단 요청').disabled);
    if (starts !== 1 || !button('Codex 설치 내용 확인').disabled) throw new Error('uncertain start repeated or parallel install allowed');
    button('설치 중단 요청').click();
    await wait(() => button('종료 확인된 설치 기록 정리'));
    button('종료 확인된 설치 기록 정리').click();
    await wait(() => !button('Codex 설치 내용 확인').disabled);
    if (starts !== 1 || cancels !== 1 || recoveries !== 1) throw new Error('installation lifecycle call counts differ');
  } finally {
    window.m5Install.request = original;
    button('상태 다시 확인').click();
    await wait(() => panel.textContent.includes('설치 기능이 아직 활성화되지'));
  }
}
module.exports = { probeInstallConsent };
