// Runs inside the actual packaged renderer via the existing CDP connection.
export async function probeLargeProject(projectId) {
  const visible = label => [...document.querySelectorAll('.m5-files button')].some(button => button.textContent.includes(label));
  const wait = async ready => {
    const deadline = performance.now() + 8000;
    while (!ready()) {
      if (performance.now() > deadline) throw new Error('Large project UI deadline exceeded');
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  };
  const open = async (label, ready) => {
    const button = [...document.querySelectorAll('.m5-files button')].find(item => item.textContent.includes(label) && !item.disabled);
    if (!button) throw new Error(`Large project target missing: ${label}`);
    const start = performance.now(); button.click(); await wait(ready);
    return {ms: performance.now() - start, domNodes: document.querySelectorAll('.m5-file-list *').length};
  };
  const root = await open('aaa-tree', () => visible('folder-0001'));
  const child = await open('folder-0001', () => visible('file-0.txt') && !visible('folder-0002'));
  document.querySelector('.m5-breadcrumbs .m5-root').click();
  await wait(() => visible('large.txt'));
  const file = await open('large.txt', () => document.querySelector('.monaco-editor .view-line')?.textContent.includes('가') && document.querySelector('.monaco-editor textarea'));
  // Keep the original cold observations above unchanged. These subsequent
  // same-process diagnostics must not replace cold timing or become gate p95.
  const repeatedRootMs = [], treeRequestMs = [];
  for (let index = 0; index < 3; index++) {
    repeatedRootMs.push((await open('aaa-tree', () => visible('folder-0001'))).ms);
    document.querySelector('.m5-breadcrumbs .m5-root').click();
    await wait(() => visible('large.txt'));
    const start = performance.now();
    const page = await window.m5.getProjectTree({projectId, parentPath:'aaa-tree', cursor:null});
    treeRequestMs.push(performance.now() - start);
    if (page.projectId !== projectId || page.parentPath !== 'aaa-tree' || page.entries.length !== 500 || !page.nextCursor) {
      throw new Error('Diagnostic tree request returned unexpected page');
    }
  }
  const values = {treeRootMs: root.ms, treeExpandMs: child.ms,
    treeDomNodes: Math.max(root.domNodes, child.domNodes), fileEditableMs: file.ms};
  const limits = {treeRootMs:1000, treeExpandMs:250, treeDomNodes:500, fileEditableMs:1500};
  return {diagnostic:true, gateEligible:false, values, subsequent:{repeatedRootMs, treeRequestMs},
    failedMetrics:Object.keys(limits).filter(key => values[key] > limits[key]),
    fixture:{treeFiles:10000, treeDirectories:1000, controlFiles:2, fileBytes:5242880, fileLines:100000}};
}
