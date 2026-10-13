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
  for (let page = 0; page < 5; page++) {
    await wait(() => rows()[0]?.title === `native-diff-fixture/file-${String(page * 100).padStart(3, '0')}.txt`, 'file page');
    if (rows().length !== 100) throw new Error('Changed file page is not bounded to 100');
    for (const row of rows()) paths.add(row.title);
    const buttons = document.querySelectorAll('.m5-change-pages button');
    if (page < 4) buttons[1].click();
    else if (!buttons[1].disabled) throw new Error('Last file page still has a next page');
  }
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
  const button = (scope, label) => [...document.querySelectorAll(`${scope} button`)].find(item => item.textContent === label);
  const open = button('.m5-diff-details', '이 파일 편집하기');
  if (!open || open.disabled) throw new Error('Diff editor navigation unavailable');
  open.click();
  await wait(() => document.querySelector('.m5-tab-list [aria-selected=true]')?.title.endsWith('file-000.txt')
    && document.querySelector('.view-lines')?.textContent.includes('after--'), 'open reviewed file');
  const review = button('.m5-editor-head', '저장된 변경 보기');
  if (!review || review.disabled) throw new Error('Editor diff navigation unavailable');
  if (document.querySelector('.m5-changes .m5-panel-title')?.getAttribute('aria-pressed') !== 'false') throw new Error('Opening editor did not leave diff focus');
  review.click();
  await wait(() => document.querySelector('.m5-changes .m5-panel-title')?.getAttribute('aria-pressed') === 'true'
    && document.querySelector('.m5-diff-details')?.textContent.includes('after--'), 'return to diff');
  return { filesVisited: paths.size, pageSize: 100, filter: true, empty: true, escape: true, preservedDiff: true, editorRoundTrip: true };
};
