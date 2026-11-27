// Real renderer interaction only; never submits a message or replaces IPC.
// The comparison is Monaco's diff view (L1 layout): this probe checks what it
// draws in both layouts, the file follow-up action and the draft it keeps.
// Per-line reference buttons belong to the plain-table fallback, checked by
// the renderer smoke (tests/m5-renderer-smoke-runner.cjs).
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
  // What Monaco actually draws (PR #24 review I3): which layout it uses,
  // both sides' text, and the added line marked. Monaco draws spaces as
  // no-break spaces. The file is hello.txt: "original" before,
  // "edited in snapshot" added above it after.
  const drawn = () => {
    const diff = document.querySelector('.m5-diff-details .monaco-diff-editor');
    if (!diff) return null;
    const side = name => diff.querySelector(`.editor.${name}`);
    const text = name => (side(name)?.querySelector('.view-lines')?.textContent ?? '').replace(/\u00a0/g, ' ');
    const width = name => side(name)?.getBoundingClientRect().width ?? 0;
    return { sideBySide: diff.classList.contains('side-by-side'), before: text('original'), after: text('modified'),
      beforeWidth: width('original'), afterWidth: width('modified'),
      added: side('modified')?.querySelectorAll('.line-insert, .char-insert').length ?? 0 };
  };
  const shows = (view, sideBySide) => Boolean(view) && view.sideBySide === sideBySide
    && view.before.includes('original') && !view.before.includes('edited in snapshot')
    && view.after.includes('edited in snapshot') && view.after.includes('original') && view.added > 0
    && view.afterWidth > 200 && (sideBySide ? view.beforeWidth > 200 : view.beforeWidth < view.afterWidth / 4);
  // Inline and side by side (the options menu): Monaco redraws the same comparison.
  const modes = [...document.querySelectorAll('.m5-diff-details .m5-diff-mode button')];
  const split = modes.find(button => button.textContent === '나란히');
  const inline = modes.find(button => button.textContent === '한 화면');
  if (!split || !inline) throw new Error('Diff display controls missing');
  if (split.getAttribute('aria-pressed') !== 'true') throw new Error('Side by side is not the first view');
  await wait(() => shows(drawn(), true)).catch(() => { throw new Error('Side-by-side comparison not drawn: ' + JSON.stringify(drawn())); });
  const sideBySide = drawn();
  inline.click();
  await wait(() => inline.getAttribute('aria-pressed') === 'true' && shows(drawn(), false)).catch(() => { throw new Error('Inline comparison not drawn: ' + JSON.stringify(drawn())); });
  const oneView = drawn();
  split.click();
  await wait(() => split.getAttribute('aria-pressed') === 'true' && shows(drawn(), true)).catch(() => { throw new Error('Side-by-side comparison not drawn again: ' + JSON.stringify(drawn())); });
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
  return { file: true, draftPreserved: true, draftRestored: true, sideBySide, inline: oneView };
};
