import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";
import ts from "typescript";

test("renderer navigation unmounts resources but bfcache suspension preserves them", () => {
  const source = readFileSync(new URL("../src/m5/bootstrap.tsx", import.meta.url), "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React, module: ts.ModuleKind.ESNext },
    transformers: { before: [context => root => ts.visitNode(root, function visit(node): ts.Node | undefined {
      if (ts.isImportDeclaration(node)) return undefined;
      return ts.visitEachChild(node, visit, context);
    }) as ts.SourceFile] },
  }).outputText.replace(/export \{\};?/g, "");
  let renders = 0, unmounts = 0;
  let hide!: (event: { persisted: boolean }) => void;
  runInNewContext(compiled, {
    exports: {},
    React: { createElement: () => ({}), StrictMode: {} }, M5Workbench: {},
    ReactDOM: { createRoot: () => ({ render() { renders++; }, unmount() { unmounts++; } }) },
    document: { getElementById: () => ({}) },
    window: { addEventListener(name: string, listener: typeof hide) { assert.equal(name, "pagehide"); hide = listener; } },
  });
  assert.equal(renders, 1);
  hide({ persisted: true });
  assert.equal(unmounts, 0);
  hide({ persisted: false });
  assert.equal(unmounts, 1);
});
