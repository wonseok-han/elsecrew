import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { requestEditorSave } from "../src/m5/editor-actions.ts";

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
  const saves: string[] = [];
  let saveAction: { run(): void; keybindings: number[] } | undefined;
  let resolveLoader!: (value: unknown) => void;
  const loader = new Promise(resolve => { resolveLoader = resolve; });
  const model = { getValue: () => text, setValue: (value: string) => { text = value; }, onDidChangeContent: () => ({ dispose() {} }), dispose() {} };
  const editor = { updateOptions: (options: { readOnly: boolean }) => { readOnly = options.readOnly; }, addAction: (action: typeof saveAction) => { saveAction = action; return { dispose() {} }; }, dispose() {} };
  const context: Record<string, any> = {
    useRef: (initial: unknown) => { const index = cursor++; return refs[index] ??= { current: initial }; },
    useEffect: (effect: () => unknown, deps: unknown[]) => {
      const index = cursor++;
      if (!dependencies[index] || deps.some((value, i) => value !== dependencies[index][i])) pending.push(effect);
      dependencies[index] = deps;
    },
    React: { createElement: (_type: unknown, props: { ref: { current: unknown } }) => { props.ref.current = {}; return null; } },
    languageFor: () => "plaintext",
    requestEditorSave,
    loadMonaco: () => loader,
  };
  context.useLayoutEffect = context.useEffect;
  const resolveMonaco = () => resolveLoader({ KeyMod: { CtrlCmd: 2048 }, KeyCode: { KeyS: 49 }, editor: { createModel: (value: string) => { text = value; return model; }, create: (_host: unknown, options: { readOnly: boolean }) => { readOnly = options.readOnly; return editor; } } });
  runInNewContext(compiled, context);
  const render = async (locked: boolean, saveState: string, value = "original") => {
    cursor = 0; pending = [];
    context.Component({ tab: { path: "test.txt", mode: "text", text: value, saveState }, readOnly: locked, onChange() {}, onSave: (path: string) => saves.push(path) });
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
  assert.ok(saveAction, "the lazy editor must finish registering its save action");
  assert.equal(saveAction.keybindings[0], 2048 | 49);
  await render(true, "clean");
  assert.equal(readOnly, true, "admission lock must survive updateOptions");
  await render(true, "dirty", "draft");
  assert.equal(readOnly, true);
  assert.equal(text, "draft");
  saveAction.run();
  assert.deepEqual(saves, [], "the current admission lock must also block the shortcut");
  await render(false, "saving");
  assert.equal(readOnly, true);
  await render(true, "saving");
  await render(true, "clean");
  assert.equal(readOnly, true, "finishing a save must not clear admission lock");
  await render(false, "clean");
  assert.equal(readOnly, false);
  await render(false, "dirty", "latest editable draft");
  saveAction.run();
  assert.deepEqual(saves, ["test.txt"]);
  await render(false, "saving");
  saveAction.run();
  assert.deepEqual(saves, ["test.txt"], "saving must block a duplicate shortcut");
});
