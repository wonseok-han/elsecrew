// Runs in the real renderer; no bridge or filesystem responses are replaced.
module.exports = async function changeNavigationProbe() {
  const wait = async (ready, label) => {
    const until = performance.now() + 15000;
    while (!ready()) {
      if (performance.now() > until) throw new Error(`Change navigation timeout: ${label}`);
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  };
  const rows = () => [...document.querySelectorAll('.m5-change-row')];
  const paths = new Set();
  await wait(() => rows().length === 100 && rows()[0]?.title === 'native-diff-fixture/file-000.txt', 'first file block');
  // Further changes load as the list scrolls; the button is the same request.
  for (let block = 1; block < 5; block++) {
    const more = document.querySelector('.m5-change-list .m5-load-more button');
    if (more && rows().length < (block + 1) * 100) more.click();
    await wait(() => rows().length >= (block + 1) * 100, 'next file block');
  }
  if (document.querySelector('.m5-change-list .m5-load-more')) throw new Error('All changed files loaded but more remain');
  rows().forEach((row, index) => { if (row.title !== `native-diff-fixture/file-${String(index).padStart(3, '0')}.txt`) throw new Error('Changed files out of order'); paths.add(row.title); });
  if (paths.size !== 500) throw new Error('File pagination lost or duplicated changes');
  const input = document.querySelector('.m5-changes input[type=search]');
  const filter = value => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  };
  filter('FILE-499');
  await wait(() => rows().length === 1 && rows()[0].title.endsWith('file-499.txt'), 'case-insensitive filter');
  filter('no-such-change');
  await wait(() => rows().length === 0 && document.querySelector('.m5-change-list').textContent.includes('일치하는'), 'empty filter');
  if (!document.querySelector('.m5-diff-details')?.textContent.includes('file-000.txt')) throw new Error('Filter discarded selected diff');
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
  await wait(() => rows().length === 100 && rows()[0].title.endsWith('file-000.txt') && input.value === '', 'clear filter');
  const button = (scope, label) => [...document.querySelectorAll(`${scope} button`)].find(item => item.textContent === label || item.getAttribute('aria-label') === label);
  const open = button('.m5-diff-details', '이 파일 편집하기');
  if (!open || open.disabled) throw new Error('Diff editor navigation unavailable');
  open.click();
  await wait(() => document.querySelector('.m5-tab-list [aria-selected=true]')?.title.endsWith('file-000.txt')
    && document.querySelector('.view-lines')?.textContent.includes('after--'), 'open reviewed file');
  const review = button('.m5-editor-head', '저장된 변경 보기');
  if (!review || review.disabled) throw new Error('Editor diff navigation unavailable');
  if (document.querySelector('.m5-changes .m5-panel-title')?.getAttribute('aria-pressed') !== 'false') throw new Error('Opening editor did not leave diff focus');
  review.click();
  // The diff now opens in the editor dock; the change list need not take focus.
  await wait(() => document.querySelector('.m5-diff-details')?.getClientRects().length > 0
    && document.querySelector('.m5-diff-details').textContent.includes('after--'), 'return to diff');
  return { filesVisited: paths.size, pageSize: 100, filter: true, empty: true, escape: true, preservedDiff: true, editorRoundTrip: true };
};
