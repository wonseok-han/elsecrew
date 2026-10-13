export async function probeLargeDiff() {
  const wait = async ready => {
    const deadline = performance.now() + 8000;
    while (!ready()) {
      if (performance.now() > deadline) throw new Error('Packaged diff deadline exceeded');
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  };
  const tab = [...document.querySelectorAll('.m5-dock-group-tab')].find(button => button.textContent.includes('변경'));
  if (tab) tab.click();
  await wait(() => [...document.querySelectorAll('.m5-changes button')].some(button => button.textContent === '변경 새로 고침' && !button.disabled));
  const start = performance.now();
  [...document.querySelectorAll('.m5-changes button')].find(button => button.textContent === '변경 새로 고침').click();
  await wait(() => document.querySelector('.m5-changes')?.textContent.includes('변경 (500)'));
  const diffSummaryMs = performance.now() - start;
  const file = [...document.querySelectorAll('.m5-change-row')].find(button => button.title === 'native-diff-fixture/file-000.txt');
  if (!file || file.disabled) throw new Error('First diff file unavailable');
  const opened = performance.now(); file.click();
  await wait(() => document.querySelector('.m5-diff-details')?.textContent.includes('after--'));
  const values = {diffSummaryMs, diffFirstFileMs:performance.now() - opened};
  const limits = {diffSummaryMs:2000, diffFirstFileMs:1500};
  return {diagnostic:true, gateEligible:false, values,
    failedMetrics:Object.keys(limits).filter(key => values[key] > limits[key])};
}
