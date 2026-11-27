// Real renderer interaction only; never submits a message or replaces IPC.
// The comparison is Monaco's diff view (L1 layout): its per-line reference
// buttons belong to the plain-table fallback, so this probe checks the file
// follow-up action, the draft it keeps, and the inline/side-by-side switch.
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
  if (!document.querySelector('.m5-diff-details .monaco-diff-editor, .m5-diff-details .monaco-editor')) throw new Error('Comparison view missing');
  // Inline and side by side (the options menu): Monaco redraws the same comparison.
  const modes = [...document.querySelectorAll('.m5-diff-details .m5-diff-mode button')];
  const split = modes.find(button => button.textContent === '나란히');
  const inline = modes.find(button => button.textContent === '한 화면');
  if (!split || !inline) throw new Error('Diff display controls missing');
  inline.click();
  await wait(() => inline.getAttribute('aria-pressed') === 'true' && document.querySelector('.m5-diff-details .monaco-diff-editor, .m5-diff-details .monaco-editor'));
  split.click();
  await wait(() => split.getAttribute('aria-pressed') === 'true' && document.querySelector('.m5-diff-details .monaco-diff-editor, .m5-diff-details .monaco-editor'));
  // A follow-up about this file joins the draft instead of replacing it.
  setDraft('Keep this draft');
  await wait(() => composer().value === 'Keep this draft');
  const button = [...document.querySelectorAll('.m5-diff-details button')].find(b => b.textContent === '이 파일에 대해 후속 요청 쓰기');
  if (!button || button.disabled) throw new Error('Diff follow-up action unavailable');
  button.click();
  await wait(() => composer().value === 'Keep this draft\n\n검토할 변경 파일: hello.txt\n'
    && composer().getBoundingClientRect().height > 0);
  setDraft(draft);
  await wait(() => composer().value === draft);
  return { file: true, draftPreserved: true, draftRestored: true, displayModes: true };
};
