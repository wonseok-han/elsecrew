// Actual React controls with a synthetic bridge; never writes a source project.
async function probeApplicationResponseLoss() {
  const original = window.m5Projects.request;
  const projectId = 'smoke_project', chatId = 'smoke_chat';
  const oldId = 'application_' + 'a'.repeat(32), newId = 'application_' + 'b'.repeat(32);
  let applicationId = oldId, running = false, starts = 0, cancels = 0;
  let failBaseline = false, baselineFailures = 0;
  let cancelFirst = true, rejectPendingStart, preparationCancels = 0;
  const wait = async predicate => {
    const deadline = Date.now() + 5000;
    while (!predicate()) {
      if (Date.now() > deadline) throw new Error('application lost response UI timeout: ' + JSON.stringify({starts,cancels,baselineFailures,applicationId,text:panel.textContent}));
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  };
  const panel = document.querySelector('.m5-chat-review');
  const button = text => [...panel.querySelectorAll('button')].find(node => node.textContent === text);
  window.m5Projects.request = async input => {
    const common = {schemaVersion:1, action:input.action};
    if (input.action === 'applicationCapabilities') return {...common, enabled:true};
    if (input.projectId !== projectId && input.action !== 'cancelApplicationConfirmation') return original(input);
    if (['applicationPreviewPage','openApplicationConfirmation','chatApplicationStatus','startApplication'].includes(input.action) && input.chatId !== chatId) throw new Error('cross-chat application request');
    if (input.action === 'chatApplicationStatus') {
      if (failBaseline) { failBaseline = false; baselineFailures++; throw new Error('baseline unavailable before sending start'); }
      return {...common, chatId, status:{projectId, applicationId, state:running ? 'applying' : 'abandoned', currentIndex:null, total:1, running, failure:null}};
    }
    if (input.action === 'applicationPreviewPage') return {...common, projectId, chatId, page:{worktreeId:'smoke_chat_worktree', total:1, offset:0, fingerprint:'c'.repeat(64), changes:[{path:'hello.txt',kind:'modified'}]}};
    if (input.action === 'openApplicationConfirmation') return {...common, projectId, chatId, worktreeId:'smoke_chat_worktree', confirmationId:'confirmation_'+'d'.repeat(32), total:1, expiresInSeconds:300};
    if (input.action === 'startApplication') {
      if (cancelFirst) return new Promise((_resolve, reject) => { rejectPendingStart = reject; });
      starts++; applicationId = newId; running = true;
      throw new Error('accepted start response lost');
    }
    if (input.action === 'cancelApplicationConfirmation') {
      if (input.confirmationId !== 'confirmation_'+'d'.repeat(32)) throw new Error('wrong confirmation cancellation');
      if (!rejectPendingStart) throw new Error('no pending preparation to cancel');
      preparationCancels++; cancelFirst = false;
      rejectPendingStart(new Error('confirmation canceled before intent creation'));
      return {...common, confirmationId:input.confirmationId};
    }
    if (input.action === 'cancelApplication') {
      if (input.applicationId !== newId) throw new Error('cancel targeted old application');
      cancels++; running = false;
      return {...common, projectId, applicationId:newId};
    }
    return original(input);
  };
  try {
    await wait(() => button('이 채팅의 반영 전 변경 목록 보기') && !button('이 채팅의 반영 전 변경 목록 보기').disabled);
    button('이 채팅의 반영 전 변경 목록 보기').click();
    await wait(() => button('이 변경 묶음으로 확인 준비'));
    button('이 변경 묶음으로 확인 준비').click();
    await wait(() => panel.querySelector('input[type=checkbox]'));
    panel.querySelector('input[type=checkbox]').click();
    await wait(() => button('백업 후 원본에 반영') && !button('백업 후 원본에 반영').disabled);
    button('백업 후 원본에 반영').click();
    await wait(() => rejectPendingStart && button('준비·반영 중단 요청'));
    button('준비·반영 중단 요청').click();
    await wait(() => preparationCancels === 1 && !panel.querySelector('input[type=checkbox]') && !button('이 채팅의 반영 전 변경 목록 보기').disabled);
    if (applicationId !== oldId || starts !== 0) throw new Error('cancel-first created or hid an application');
    button('이 채팅의 반영 전 변경 목록 보기').click();
    await wait(() => button('이 변경 묶음으로 확인 준비'));
    button('이 변경 묶음으로 확인 준비').click();
    await wait(() => panel.querySelector('input[type=checkbox]'));
    panel.querySelector('input[type=checkbox]').click();
    await wait(() => button('백업 후 원본에 반영') && !button('백업 후 원본에 반영').disabled);
    failBaseline = true;
    button('백업 후 원본에 반영').click();
    await wait(() => baselineFailures === 1 && panel.textContent.includes('반영 전 목록을 확인하지 못했어요') && button('백업 후 원본에 반영') && !button('백업 후 원본에 반영').disabled);
    if (starts !== 0) throw new Error('failed baseline sent a start command');
    button('백업 후 원본에 반영').click();
    await wait(() => button('처리 중단') && !button('처리 중단').disabled);
    if (starts !== 1) throw new Error('lost response replayed start');
    button('처리 중단').click();
    await wait(() => cancels === 1 && !button('처리 중단'));
    if (starts !== 1 || applicationId !== newId) throw new Error('application identity changed');
    return {starts, cancels, baselineFailures, preparationCancels, canceledApplicationId:newId};
  } finally { window.m5Projects.request = original; }
}
module.exports = { probeApplicationResponseLoss };
