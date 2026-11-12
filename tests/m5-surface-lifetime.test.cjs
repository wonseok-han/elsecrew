const test=require('node:test');const assert=require('node:assert/strict');
const {summarizeSurfaces}=require('../scripts/m5-surface-lifetime.cjs');
const header='[ VSIZE RSDNT DIRTY SWAP]\n';
const row='IOSurface 12340000-12350000 [ 4160K 4160K 4160K 0K] rw-/rw- SurfaceID: 0x123 2560x416 (BGRA)';
test('surface groups count resident, not virtual, and omit addresses',()=>{
 const result=summarizeSurfaces(header+row+'\n'+row.replace('4160K 4160K 4160K','15.0M 0K 0K'));
 assert.equal(result.groups[0].count,2);assert.equal(result.groups[0].residentBytes,4160*1024);
 assert.ok(!JSON.stringify(result).includes('12340000'));
});
test('unknown columns, malformed sizes and missing dimensioned surfaces fail',()=>{
 for(const text of [row,header,header+row.replace('4160K 4160K','4160K unknown'),header+row.replace('2560x416','unknown')]) assert.throws(()=>summarizeSurfaces(text));
});
