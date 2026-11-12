// Disposable renderer fixture only; never attaches to the installed app.
exports.probeRecoveryHistory = async function probeRecoveryHistory(detail) {
  const wait = async predicate => {
    const end = Date.now() + 4000;
    while (!predicate()) { if (Date.now() > end) throw new Error('recovery history UI timeout'); await new Promise(resolve => setTimeout(resolve, 20)); }
  };
  const original = {};
  const names = ['listChats', 'getChat', 'createProjectWorktree', 'getProjectTree', 'watchProjectFiles', 'getDiffSummary', 'getWorkspaceTree', 'watchWorkspaceFiles', 'getWorkspaceDiffSummary', 'startAgentRun', 'startFixtureRun', 'resolveAgentRecovery'];
  for (const name of names) original[name] = window.m5[name];
  const calls = [];
  try {
    for (const name of names) window.m5[name] = async () => { calls.push(name); throw new Error('forbidden history operation: ' + name); };
    window.m5.listChats = async request => {
      calls.push('listChats');
      detail = JSON.parse(JSON.stringify(detail).replaceAll(detail.chat.projectId, request.projectId));
      return {projectId:request.projectId, items:[detail.chat], nextCursor:null};
    };
    window.m5.getChat = async () => { calls.push('getChat'); return detail; };
    const button = text => [...document.querySelectorAll('button')].find(node => node.textContent === text);
    button('대화 기록 확인').click();
    await wait(() => button(detail.chat.goal));
    button(detail.chat.goal).click();
    await wait(() => document.querySelector('.m5-recovery-history-messages'));
    const section = document.querySelector('.m5-recovery-history-messages').closest('section');
    if (section.querySelector('button')) throw new Error('identity-unavailable history exposed mutation');
    if (!section.textContent.includes('종료 상태를 안전하게 확인할 수 없어')) throw new Error('missing identity-unavailable guidance');
    if (calls.join(',') !== 'listChats,getChat') throw new Error('history touched workspace or mutation: ' + calls.join(','));
    return {calls, identityUnavailableProtected:true};
  } finally { for (const name of names) window.m5[name] = original[name]; }
};
