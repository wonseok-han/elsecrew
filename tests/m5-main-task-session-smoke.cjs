// Opt-in collector lifecycle check, not product performance or G6 evidence.
const {app, BrowserWindow} = require('electron');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');
const dense = process.argv.includes('--dense-trace');
assert.deepEqual(process.argv.slice(2), dense ? ['--dense-trace'] : []);
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hello-agent-main-trace-session-'));
app.setPath('userData', path.join(root, 'userdata'));
const report = {dense, markers:[], root, pid:process.pid, diagnostic:true, gateEligible:false, status:'RUNNING'};
const persist = () => fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(report,null,2)+'\n');
persist(); console.log(JSON.stringify({root, pid:process.pid}));
app.on('will-quit', () => { report.normalQuit = true; persist(); app.exit(report.status === 'PASS' ? 0 : 1); });
app.whenReady().then(async () => {
  const window = new BrowserWindow({show:false,webPreferences:{sandbox:true,contextIsolation:true,nodeIntegration:false}});
  let session, timer, loadTimeout;
  const markerListener=(_event,method,params)=>{
    if(method !== 'Tracing.dataCollected') return;
    for(const event of params.value ?? []) {
      if(['m5-blank-start','m5-blank-end'].includes(event.args?.sync_id) && report.markers.length<8) {
        report.markers.push({syncId:event.args.sync_id,pid:event.pid,tid:event.tid,ts:event.ts,name:event.name,ph:event.ph});
      }
    }
  };
  try {
    await window.loadURL('about:blank');
    const {startMainTaskSession} = await import('../scripts/m5-main-task-session.mjs');
    const {mainTaskMaximum} = await import('../scripts/m5-main-task-trace.mjs');
    session = await startMainTaskSession(window.webContents.debugger,process.pid);
    if(dense) {
      window.webContents.debugger.on('message',markerListener);
      await window.webContents.debugger.sendCommand('Tracing.recordClockSyncMarker',{syncId:'m5-blank-start'});
    }
    const rendererLoad = dense ? Promise.race([window.webContents.executeJavaScript(`new Promise(resolve => {
      const channel=new MessageChannel();let count=0;
      channel.port1.onmessage=()=>{
        if(++count===100000) {channel.port1.close();channel.port2.close();resolve(count);}
        else channel.port2.postMessage(0);
      };channel.port2.postMessage(0);
    })`), new Promise((_,reject)=>{loadTimeout=setTimeout(()=>reject(new Error('Renderer trace volume timeout')),35000);})]) : Promise.resolve(0);
    let ticks=0;
    timer = setInterval(() => {ticks++;},dense ? 1 : 10);
    const [,messages]=await Promise.all([new Promise(resolve => setTimeout(resolve,dense ? 25000 : 1000)),rendererLoad]);
    report.rendererMessages=messages;
    clearTimeout(loadTimeout);
    report.ticks=ticks;
    if(dense) await window.webContents.debugger.sendCommand('Tracing.recordClockSyncMarker',{syncId:'m5-blank-end'});
    clearInterval(timer);
    report.trace = await session.stop(true); session = undefined;
    report.maximumMs = mainTaskMaximum(report.trace.observation);
    assert.equal(report.trace.error,undefined);
    window.webContents.debugger.removeListener('message',markerListener);
    if(dense) {
      assert.equal(report.markers.length,2,'Clock markers absent or ambiguous');
      report.maximumStartUs=report.trace.observation.tasks[0].startUs;
      report.maximumAfterEndMarker=report.maximumStartUs >= report.markers.find(x=>x.syncId==='m5-blank-end').ts;
    }
    assert.equal(window.webContents.debugger.isAttached(),false);
    assert.equal(window.webContents.debugger.listenerCount('message'),0);
    report.status = 'PASS';
  } catch (error) { report.status='FAIL'; report.error=String(error); }
  finally {
    clearTimeout(loadTimeout);
    clearInterval(timer);
    if (session) report.cleanupTrace=await session.stop(false);
    window.webContents.debugger.removeListener('message',markerListener);
    report.processes=app.getAppMetrics().map(({pid,type})=>({pid,type}));
    persist(); window.destroy(); app.quit();
  }
});
