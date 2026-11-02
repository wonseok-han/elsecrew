// Separate, explicit QA artifact. Never writes the product out/ directory.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { builtinModules, createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { spawnSync, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { build } from 'vite';
import { packager } from '@electron/packager';

assert.equal(process.platform, 'darwin');
assert.equal(process.arch, 'arm64');
assert.equal(process.argv.length, 2, 'No runtime overrides accepted');
const repo = fileURLToPath(new URL('..', import.meta.url));
const require = createRequire(import.meta.url);
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hello-agent-output-qa-'));
const source = path.join(root, 'source');
fs.mkdirSync(source);
const git = (...args) => execFileSync('/usr/bin/git', args, {cwd:repo, encoding:'utf8'}).trim();
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const report = {diagnostic:true, gateEligible:false, status:'BUILDING', root,
  head:git('rev-parse','HEAD'), gitStatus:git('status','--porcelain'),
  differences:['QA main entrypoint installs the fixed synthetic launch harness.',
    'Sidecar enables qa-output-fixtures; production staging rejects this feature.',
    'QA artifact does not establish production signing, fuses, or full G6 readiness.']};
const persist = () => fs.writeFileSync(path.join(root,'build.json'), JSON.stringify(report,null,2));
persist(); console.log(JSON.stringify({stage:'QA_BUILD_STARTED', root}));
try {
  const cargo = spawnSync('cargo', ['build','--locked','--release','-p','hello-agent-sidecar',
    '--bin','hello-agent-sidecar','--features','qa-output-fixtures','--message-format=json-render-diagnostics'],
  {cwd:repo, encoding:'utf8', maxBuffer:16*1024*1024,
    env:{...process.env,CARGO_BUILD_JOBS:'4',CARGO_INCREMENTAL:'0'}});
  fs.writeFileSync(path.join(root,'cargo.log'), cargo.stderr ?? '');
  assert.ok(!cargo.error && cargo.status === 0, 'QA sidecar build failed');
  const artifacts = cargo.stdout.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line)).filter(value =>
    value.reason === 'compiler-artifact' && value.manifest_path === path.join(repo,'crates/hello-agent-sidecar/Cargo.toml')
    && value.target?.name === 'hello-agent-sidecar' && value.target.kind.includes('bin')
    && value.profile?.test === false && value.profile.debug_assertions === false);
  assert.equal(artifacts.length,1);
  const artifact = artifacts[0];
  assert.ok(artifact.features?.includes('qa-output-fixtures') && path.isAbsolute(artifact.executable));
  // Copy before any later Cargo invocation can replace the resolved binary.
  const sidecarDirectory = path.join(root,'sidecar'); fs.mkdirSync(sidecarDirectory);
  const sidecar = path.join(sidecarDirectory,'hello-agent-sidecar');
  fs.copyFileSync(artifact.executable,sidecar); fs.chmodSync(sidecar,0o755);
  report.sidecarSha256 = hash(sidecar);
  const external = ['electron', ...builtinModules, ...builtinModules.map(name => `node:${name}`)];
  for (const name of ['main','preload']) await build({configFile:false, root:repo,
    define:{MAIN_WINDOW_VITE_DEV_SERVER_URL:'undefined',MAIN_WINDOW_VITE_NAME:JSON.stringify('main_window')},
    build:{outDir:path.join(source,'.vite/build'),emptyOutDir:false,
      lib:{entry:path.join(repo,`electron/${name}.ts`),formats:['cjs'],fileName:() => `${name}.cjs`},
      rollupOptions:{external}}});
  await build({configFile:path.join(repo,'electron/vite.renderer.config.ts'),
    build:{outDir:path.join(source,'.vite/renderer/main_window')}});
  fs.mkdirSync(path.join(source,'scripts'));
  for (const name of ['m5-durable-output-ui-smoke.cjs','m5-durable-output-observer.cjs',
    'm5-live-output-latency.cjs','m5-stdio-observer.cjs','m5-output-generation.cjs',
    'm5-combined-output-workload.cjs','m5-process-memory.cjs','m5-packaged-large-project-probe.mjs','m5-packaged-diff-probe.mjs']) fs.copyFileSync(path.join(repo,'scripts',name),path.join(source,'scripts',name));
  fs.mkdirSync(path.join(source,'tests'));
  for (const name of ['m5-native-diff-fixture.cjs','m5-renderer-performance-fixture.cjs'])
    fs.copyFileSync(path.join(repo,'tests',name),path.join(source,'tests',name));
  fs.writeFileSync(path.join(source,'.vite/build/qa-main.cjs'),
    "require('../../scripts/m5-durable-output-ui-smoke.cjs').runOutputSmoke(true);\n");
  fs.writeFileSync(path.join(source,'package.json'),JSON.stringify({name:'hello-agent-output-qa',
    version:'0.0.0',main:'.vite/build/qa-main.cjs',private:true}));
  const apps = await packager({dir:source,out:path.join(root,'app'),name:'Hello Agent Output QA',
    platform:'darwin',arch:'arm64',electronVersion:require('electron/package.json').version,
    asar:true,overwrite:false,prune:false,extraResource:[sidecarDirectory]});
  assert.equal(apps.length,1);
  assert.equal(git('rev-parse','HEAD'),report.head);
  assert.equal(git('status','--porcelain'),report.gitStatus);
  report.appDirectory=apps[0]; report.status='BUILT';
  // No app has been launched. Remove only the build inputs owned by this run.
  fs.rmSync(source,{recursive:true}); fs.rmSync(sidecarDirectory,{recursive:true});
} catch(error) {report.status='FAIL'; report.error=String(error); process.exitCode=1;}
finally {persist(); console.log(JSON.stringify(report));}
