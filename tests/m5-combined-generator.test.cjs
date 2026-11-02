const test=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const os=require('node:os');
const path=require('node:path');
const {execFileSync}=require('node:child_process');
const {mount}=require('../scripts/m5-combined-output-workload.cjs');
test('measured diff generation yields main timers and completes before probing',async()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'m5-generator-test-'));
  const snapshot=path.join(root,'com.wonseokhan.helloagent/m5/worktrees-v1/worktree_test');
  let timer,ticks=0,calls=0;
  try {
    fs.mkdirSync(snapshot,{recursive:true});
    require('./m5-native-diff-fixture.cjs').prepare(snapshot);
    for(const args of [['init','-q','--template='],['add','.'],['-c','core.hooksPath=/dev/null','-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-qm','fixture']])
      execFileSync('/usr/bin/git',args,{cwd:snapshot,env:{...process.env,GIT_CONFIG_NOSYSTEM:'1',GIT_CONFIG_GLOBAL:'/dev/null',GIT_CONFIG_COUNT:'0'}});
    const result=await mount(async()=>{
      if(calls===0) timer=setInterval(()=>ticks++,1);
      if(calls===2) assert.ok(ticks>0,'Child generation blocked the main timer');
      calls++;return true;
    },root,{worktreeId:'worktree_test',projectId:'project_test'});
    assert.equal(result.largeDiff.diffFiles,500);
    assert.equal(result.largeDiff.diffChangedLines,20000);
    assert.equal(result.largeDiff.diffBytes,10*1024*1024);
  } finally {clearInterval(timer);fs.rmSync(root,{recursive:true,force:true});}
});
