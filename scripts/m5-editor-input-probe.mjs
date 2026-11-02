// Serialized into the QA renderer. Uses real input; never edits model or disk.
export async function probeEditorInput(action) {
  const key = '__m5EditorInputProbe';
  const input = document.querySelector('.monaco-editor textarea');
  const dirty = () => !!document.querySelector('.m5-editor-tab [aria-label="저장되지 않은 변경"], .m5-editor-tab [aria-label="Unsaved changes"]');
  const text = () => document.querySelector('.monaco-editor .view-line')?.textContent;
  const wait = async ready => {
    const deadline = performance.now() + 2000;
    while (!ready()) {
      if (performance.now() > deadline || document.visibilityState !== 'visible') throw new Error('Editor input observation unavailable');
      await new Promise(resolve => setTimeout(resolve, 5));
    }
  };
  const clean = () => {
    const state = globalThis[key];
    if (state) document.removeEventListener('beforeinput', state.listener, true);
  };
  if (action === 'cleanup') { clean(); delete globalThis[key]; return; }
  if (action === 'arm') {
    if (globalThis[key] || !input || input.readOnly || dirty() || !text() || document.visibilityState !== 'visible') throw new Error('Editor input baseline unavailable');
    const tab = [...document.querySelectorAll('.m5-editor-tab button[role=tab]')].find(button => button.textContent.includes('large.txt') && !button.disabled);
    if (!tab) throw new Error('QA large file tab unavailable');
    tab.click();
    await wait(() => input.getClientRects().length > 0);
    input.focus();
    await wait(() => document.hasFocus() && document.activeElement === input);
    const state = {baseline:text(), startedAt:performance.now(), receivedAt:null, trustedInputs:0};
    state.listener = event => {
      if (event.target === input && event.isTrusted && event.data === 'Z') {
        state.trustedInputs++; state.receivedAt ??= performance.now();
      }
    };
    globalThis[key] = state;
    document.addEventListener('beforeinput', state.listener, true);
    return;
  }
  const state = globalThis[key];
  if (!state) throw new Error('Editor input was not armed');
  if (action === 'measure') {
    try {
      await wait(() => dirty() && text() !== state.baseline && state.trustedInputs === 1);
      await new Promise((resolve, reject) => {
        let frame;
        const timer = setTimeout(() => { cancelAnimationFrame(frame); reject(new Error('Editor input presentation timeout')); }, 2000);
        frame = requestAnimationFrame(() => { frame = requestAnimationFrame(() => { clearTimeout(timer); resolve(); }); });
      });
      if (document.visibilityState !== 'visible' || state.trustedInputs !== 1) throw new Error('Editor input observation interrupted');
      const paintAt = performance.now();
      return {startedAt:state.startedAt,receivedAt:state.receivedAt,paintAt,
        inputPaintMs:paintAt-state.startedAt, inputDeliveryMs:state.receivedAt-state.startedAt,
        trustedInputs:state.trustedInputs, scope:'Native char dispatch to dirty text and two animation frames; not physical display latency'};
    } finally { clean(); }
  }
  if (action === 'restore') {
    await wait(() => !dirty() && text() === state.baseline);
    clean(); delete globalThis[key];
    const diff = document.querySelector('.m5-change-row');
    if (!diff || diff.disabled) throw new Error('QA diff cannot be restored');
    diff.click();
    await wait(() => !!document.querySelector('.m5-diff-details'));
    return {restored:true};
  }
  throw new Error('Unknown editor input probe action');
}
