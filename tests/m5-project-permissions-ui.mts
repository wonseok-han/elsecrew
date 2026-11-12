import assert from "node:assert/strict";
import {createRequire} from "node:module";
import {mkdtempSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {resolve} from "node:path";
import {spawnSync} from "node:child_process";
import test from "node:test";
const require=createRequire(import.meta.url);
test("project open asks once, saves and revokes permissions without model calls", () => {
 const temporary=mkdtempSync(resolve(tmpdir(),"elsecrew-permissions-ui-"));
 const result=spawnSync(require("electron"),["tests/m5-renderer-smoke-runner.cjs"],{encoding:"utf8",timeout:30000,env:{...process.env,M5_SMOKE_TEMP_ROOT:temporary,M5_RENDERER_DIR:resolve(".vite/renderer/main_window"),M5_PERMISSIONS_ONLY:"1",M5_MEASURE_PARALLEL_PAINT:"0",M5_MEASURE_MESSAGE_PAINT:"0"}});
 if(result.signal===null && [0,1].includes(result.status??-1)) rmSync(temporary,{recursive:true});
 assert.equal(result.status,0, result.stdout+result.stderr);
 assert.match(result.stdout,/"permissionsVerified":true/);
});
