// Runs inside the existing Chromium fixture renderer. No native deletion.
async function probeCleanupConsent() {
  const original = window.m5Projects.request;
  const reviewId = 'review_' + 'a'.repeat(32);
  const applicationId = 'application_' + 'b'.repeat(32);
  const calls = { cleanupBackup: 0, retireApplication: 0 };
  const fingerprints = {};
  let revision = 0;
  const wait = async predicate => {
    const end = Date.now() + 5000;
    while (!predicate()) {
      if (Date.now() > end) throw new Error('cleanup consent UI timeout');
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  };
  const button = (scope, text) => [...scope.querySelectorAll('button')].find(node => node.textContent === text);
  const recovery = document.querySelector('[aria-label="프로젝트 시작 실패 복구"]');
  window.m5Projects.request = async input => {
    if (input.projectId !== 'earlier_project') return original(input);
    const common = { schemaVersion: 1, action: input.action, projectId: input.projectId };
    if (input.action === 'backupRetention') return { ...common, entries: [{reviewId, state:'ready', retainedFor:'applicationHistory', cleanupState:'none'}] };
    if (input.action === 'applicationStatus') return {schemaVersion:1, action:input.action, status:{projectId:input.projectId, applicationId, state:'abandoned', currentIndex:null, total:0, running:false, failure:null}};
    if (input.action === 'inspectBackupCleanup' || input.action === 'inspectApplicationRetirement') {
      const fingerprint = (++revision).toString(16).padStart(64, '0');
      const cleanup = input.action === 'inspectBackupCleanup';
      if (cleanup ? input.reviewId !== reviewId : input.applicationId !== applicationId) throw new Error('wrong inspection target');
      fingerprints[cleanup ? 'cleanupBackup' : 'retireApplication'] = fingerprint;
      return { ...common, enabled:true, inspection:cleanup ? {reviewId, fingerprint, filePresent:true, state:'available'} : {applicationId, fingerprint, state:'abandoned'} };
    }
    if (Object.hasOwn(calls, input.action)) {
      if (input.fingerprint !== fingerprints[input.action]) throw new Error('stale cleanup confirmation');
      if (input.action === 'cleanupBackup' ? input.reviewId !== reviewId : input.applicationId !== applicationId) throw new Error('wrong mutation target');
      const count = ++calls[input.action];
      await new Promise(resolve => setTimeout(resolve, 40));
      if (count === 1) throw new Error('synthetic response lost');
      return { ...common, ...(input.action === 'cleanupBackup' ? {reviewId} : {applicationId}) };
    }
    return original(input);
  };
  try {
    button(recovery, '이 프로젝트의 백업 기록과 정리 상태').click();
    await wait(() => button(recovery, '백업 파일 정리'));
    button(recovery, '백업 파일 정리').click();
    await wait(() => button(recovery, '정리 대상 다시 확인'));
    for (const [action, section, inspect, confirm] of [
      ['cleanupBackup', '백업 파일 정리', '정리 대상 다시 확인', '확인한 백업 파일 정리'],
      ['retireApplication', '현재 복구점 종료 내용 확인', '현재 복구점 종료 내용 확인', '복구점 종료하고 이력에 보존'],
    ]) {
      if (action === 'retireApplication') {
        button(recovery, '반영·복원 기록 확인').click();
        await wait(() => button(recovery, inspect));
      }
      const scope = recovery.querySelector('section[aria-label="' + section + '"]');
      for (let attempt = 1; attempt <= 2; attempt++) {
        button(scope, inspect).click();
        await wait(() => button(scope, confirm));
        const submit = button(scope, confirm);
        if (!submit.disabled || scope.querySelector('input').checked) throw new Error('cleanup requires fresh unchecked consent');
        submit.click();
        if (calls[action] !== attempt - 1) throw new Error('unchecked cleanup submitted');
        scope.querySelector('input').click();
        await wait(() => !button(scope, confirm).disabled);
        submit.click(); submit.click();
        await wait(() => calls[action] === attempt);
        if (attempt === 1) {
          await wait(() => scope.textContent.includes('자동 재시도하지 않습니다'));
          await new Promise(resolve => setTimeout(resolve, 100));
          if (calls[action] !== 1 || button(scope, confirm)) throw new Error('uncertain cleanup replayed without inspection');
        } else {
          await wait(() => action === 'cleanupBackup' ? scope.textContent.includes('백업 파일 정리를 완료했습니다') : !scope.isConnected);
        }
      }
    }
    return calls;
  } finally { window.m5Projects.request = original; }
}
module.exports = { probeCleanupConsent };
