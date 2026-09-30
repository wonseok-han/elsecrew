import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import test from "node:test";
import ts from "typescript";
import { renderToStaticMarkup } from "react-dom/server";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";
import * as contract from "../src/m5/contracts/approval-v1.ts";

const require = createRequire(import.meta.url);
const compiled = ts.transpileModule(readFileSync(new URL("../src/m5/approval-card.tsx", import.meta.url), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020 },
}).outputText;

test("approval translations preserve warnings, raw detail and decline-only controls", () => {
  for (const dictionary of [ko, en]) {
    let index = 0;
    const states = [[{approvalId:"approval_test",kind:"command"}], true, true, true,
      {approvalId:"approval_test",kind:"command",command:"echo PRIVATE <tag>",cwd:"/snapshot",reason:null,grantRoot:null,host:null,protocol:null}];
    const context: any = { exports: {}, require(name: string) {
      if (name === "react") return { useEffect() {}, useRef: (value: unknown) => ({current:value}), useState: () => [states[index++], () => {}] };
      if (name === "../i18n.tsx") return { useI18n: () => ({ t(key: keyof typeof ko, params: Record<string, unknown> = {}) {
        return dictionary[key].replace(/\{(\w+)\}/g, (_, key) => String(params[key]));
      } }) };
      if (name === "./contracts/approval-v1.ts") return contract;
      return require(name);
    } };
    runInNewContext(compiled, context);
    const html = renderToStaticMarkup(context.exports.ApprovalCard({identity:{runId:"run_test"},onCancel() {}}));
    for (const key of ["help","warning","queued","error","cancel","decline","close"] as const) {
      const expected = renderToStaticMarkup(dictionary[`m5.approval.${key}`]);
      assert.ok(html.includes(expected), key);
    }
    assert.ok(html.includes("echo PRIVATE &lt;tag&gt;"));
    assert.equal((html.match(/<button/g) ?? []).length, 3);
    assert.equal((html.match(/disabled=""/g) ?? []).length, 2);
    assert.ok(html.includes('aria-expanded="true"'));
  }
});
