// Opt-in collector lifecycle check, not product performance or G6 evidence.
const {app, BrowserWindow} = require('electron');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hello-agent-main-trace-session-'));
app.setPath('userData', path.join(root, 'userdata'));
const report = {root, pid:process.pid, diagnostic:true, gateEligible:false, status:'RUNNING'};
const persist = () => fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(report,null,2)+'\n');
persist(); console.log(JSON.stringify({root, pid:process.pid}));
app.on('will-quit', () => { report.normalQuit = true; persist(); app.exit(report.status === 'PASS' ? 0 : 1); });
app.whenReady().then(async () => {
  const window = new BrowserWindow({show:false,webPreferences:{sandbox:true,contextIsolation:true,nodeIntegration:false}});
  let session, timer;
  try {
    await window.loadURL('about:blank');
    const {startMainTaskSession} = await import('../scripts/m5-main-task-session.mjs');
    const {mainTaskMaximum} = await import('../scripts/m5-main-task-trace.mjs');
    session = await startMainTaskSession(window.webContents.debugger,process.pid);
    timer = setInterval(() => {},10);
    await new Promise(resolve => setTimeout(resolve,1000));
    clearInterval(timer);
    report.trace = await session.stop(true); session = undefined;
    report.maximumMs = mainTaskMaximum(report.trace.observation);
    assert.equal(report.trace.error,undefined);
    assert.equal(window.webContents.debugger.isAttached(),false);
    assert.equal(window.webContents.debugger.listenerCount('message'),0);
    report.status = 'PASS';
  } catch (error) { report.status='FAIL'; report.error=String(error); }
  finally {
    clearInterval(timer);
    if (session) report.cleanupTrace=await session.stop(false);
    report.processes=app.getAppMetrics().map(({pid,type})=>({pid,type}));
    persist(); window.destroy(); app.quit();
  }
});
