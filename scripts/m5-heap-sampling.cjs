// Synthetic diagnostic only. Sampled live allocations are not RSS or a leak verdict.
function summarizeHeapSampling(profile) {
  const pending = [profile.head];
  const rows = [];
  let nodes = 0;
  let sampledBytes = 0;
  while (pending.length) {
    const node = pending.pop();
    if (++nodes > 100000) throw new Error('Heap sampling node budget exceeded');
    if (!node || !Number.isFinite(node.selfSize) || node.selfSize < 0 || !Array.isArray(node.children)) throw new Error('Invalid heap sampling node');
    sampledBytes += node.selfSize;
    if (node.selfSize > 0) rows.push({
      sampledBytes:node.selfSize,
      functionName:String(node.callFrame?.functionName ?? '').slice(0,160),
      url:String(node.callFrame?.url ?? '').slice(0,240),
      lineNumber:node.callFrame?.lineNumber,
      columnNumber:node.callFrame?.columnNumber,
    });
    for (const child of node.children) pending.push(child);
  }
  rows.sort((a,b) => b.sampledBytes-a.sampledBytes);
  return {nodes,sampledBytes,omitted:Math.max(0,rows.length-50),top:rows.slice(0,50)};
}
module.exports = {summarizeHeapSampling};
