import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";

// Execute the actual component's effects, without exporting a product test API
// or loading browser-only Monaco in Node. Chromium mounting is tested separately.
const source = ts.createSourceFile("workbench.tsx", readFileSync(new URL("../src/m5/workbench.tsx", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const component = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "MonacoEditor")!;
const compiled = ts.transpileModule(`${component.getText(source)}\nglobalThis.Component = MonacoEditor;`, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React },
}).outputText;

for (const delayedGate of [null, "admission", "saving"]) test(`Monaco keeps current read-only state (deferred gate: ${delayedGate ?? "none"})`, async () => {
  const refs: { current: any }[] = [];
  const dependencies: unknown[][] = [];
  let cursor = 0;
  let pending: (() => unknown)[] = [];
  let text = "original";
  let readOnly: boolean | undefined;
  let resolveLoader!: (value: unknown) => void;
  const loader = new Promise(resolve => { resolveLoader = resolve; });
  const model = { getValue: () => text, setValue: (value: string) => { text = value; }, onDidChangeContent: () => ({ dispose() {} }), dispose() {} };
  const editor = { updateOptions: (options: { readOnly: boolean }) => { readOnly = options.readOnly; }, dispose() {} };
  const context: Record<string, any> = {
    useRef: (initial: unknown) => { const index = cursor++; return refs[index] ??= { current: initial }; },
    useEffect: (effect: () => unknown, deps: unknown[]) => {
      const index = cursor++;
      if (!dependencies[index] || deps.some((value, i) => value !== dependencies[index][i])) pending.push(effect);
      dependencies[index] = deps;
    },
    React: { createElement: (_type: unknown, props: { ref: { current: unknown } }) => { props.ref.current = {}; return null; } },
    languageFor: () => "plaintext",
    loadMonaco: () => loader,
  };
  context.useLayoutEffect = context.useEffect;
  const resolveMonaco = () => resolveLoader({ editor: { createModel: (value: string) => { text = value; return model; }, create: (_host: unknown, options: { readOnly: boolean }) => { readOnly = options.readOnly; return editor; } } });
  runInNewContext(compiled, context);
  const render = async (locked: boolean, saveState: string, value = "original") => {
    cursor = 0; pending = [];
    context.Component({ tab: { path: "test.txt", text: value, saveState }, readOnly: locked, onChange() {} });
    for (const effect of pending) effect();
    await Promise.resolve();
  };
  if (!delayedGate) resolveMonaco();
  await render(false, "clean");
  if (delayedGate) {
    assert.equal(readOnly, undefined);
    await render(delayedGate === "admission", delayedGate === "saving" ? "saving" : "clean", "latest draft");
    resolveMonaco();
    await Promise.resolve();
    assert.equal(readOnly, true, "creation must use the current gate after loading");
    assert.equal(text, "latest draft");
    await render(false, "clean");
  }
  assert.equal(readOnly, false);
  await render(true, "clean");
  assert.equal(readOnly, true, "admission lock must survive updateOptions");
  await render(true, "dirty", "draft");
  assert.equal(readOnly, true);
  assert.equal(text, "draft");
  await render(false, "saving");
  assert.equal(readOnly, true);
  await render(true, "saving");
  await render(true, "clean");
  assert.equal(readOnly, true, "finishing a save must not clear admission lock");
  await render(false, "clean");
  assert.equal(readOnly, false);
});
