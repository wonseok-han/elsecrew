// Real renderer interaction only; never submits a message or replaces IPC.
module.exports = async function diffFollowupProbe() {
  const wait = async predicate => {
    const deadline = performance.now() + 5000;
    while (!predicate()) {
      if (performance.now() > deadline) throw new Error('Diff follow-up UI timeout');
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  };
  const composer = () => document.querySelector('#m5-followup-text');
  const setDraft = text => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(composer(), text);
    composer().dispatchEvent(new Event('input', { bubbles: true }));
  };
  const draft = composer()?.value;
  if (draft === undefined) throw new Error('Follow-up composer missing');
  setDraft('Keep this draft');
  await wait(() => composer().value === 'Keep this draft');
  const button = [...document.querySelectorAll('.m5-diff-details button')].find(b => b.textContent === '이 파일에 대해 후속 요청 쓰기');
  if (!button || button.disabled) throw new Error('Diff follow-up action unavailable');
  button.click();
  await wait(() => composer().value === 'Keep this draft\n\n검토할 변경 파일: hello.txt\n'
    && composer().getBoundingClientRect().height > 0);
  const changes = [...document.querySelectorAll('.m5-dock-group-tabs button')].find(b => b.firstChild?.textContent === '변경');
  if (!changes) throw new Error('Changes tab missing');
  changes.click();
  await wait(() => document.querySelector('.m5-diff-line-reference')?.getBoundingClientRect().height > 0);
  const modes = [...document.querySelectorAll('.m5-diff-mode button')];
  const split = modes.find(button => button.textContent === '나란히');
  const inline = modes.find(button => button.textContent === '한 화면');
  if (!split || !inline) throw new Error('Diff display controls missing');
  split.click();
  await wait(() => split.getAttribute('aria-pressed') === 'true' && document.querySelector('.m5-diff-split'));
  if (document.querySelectorAll('.m5-diff-split thead th').length !== 2) throw new Error('Split comparison headings missing');
  if (!document.querySelector('.m5-diff-split .m5-diff-side-code.m5-diff-deleted') || !document.querySelector('.m5-diff-split .m5-diff-side-code.m5-diff-added')) throw new Error('Split comparison lost a changed side');
  const line = document.querySelector('.m5-diff-line-reference');
  const label = line.getAttribute('aria-label').replace('을 후속 요청 초안에 추가', '');
  line.click();
  await wait(() => composer().value.includes(`검토할 변경 위치: hello.txt — ${label}`)
    && composer().getBoundingClientRect().height > 0);
  if (!composer().value.startsWith('Keep this draft\n\n검토할 변경 파일: hello.txt')) throw new Error('Line reference replaced the existing draft');
  setDraft(draft);
  await wait(() => composer().value === draft);
  inline.click();
  await wait(() => inline.getAttribute('aria-pressed') === 'true' && !document.querySelector('.m5-diff-split'));
  return { file: true, line: true, draftPreserved: true, draftRestored: true, splitComparison: true };
};
