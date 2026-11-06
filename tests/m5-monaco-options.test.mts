import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { requestEditorSave } from "../src/m5/editor-actions.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

// Execute the actual component's effects, without exporting a product test API
// or loading browser-only Monaco in Node. Chromium mounting is tested separately.
const source = ts.createSourceFile("workbench.tsx", readFileSync(new URL("../src/m5/workbench.tsx", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const component = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "MonacoEditor")!;
const compiled = ts.transpileModule(`${component.getText(source)}\nglobalThis.Component = MonacoEditor;`, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React },
}).outputText;

test("Monaco loader shares an attempt but clears a rejected cache for explicit retry", async () => {
  const loader = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "loadMonaco")!;
  const loaderCode = ts.transpileModule(`let monacoLoader;\n${loader.getText(source)}\nglobalThis.load = loadMonaco;`, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
    transformers: { before: [context => root => ts.visitNode(root, function visit(node): ts.Node {
      if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
        return ts.factory.createCallExpression(ts.factory.createIdentifier("fakeImport"), undefined, node.arguments);
      }
      return ts.visitEachChild(node, visit, context);
    }) as ts.SourceFile] },
  }).outputText;
  let fail = true;
  let attempts = 0;
  const api = {};
  const context: Record<string, any> = { fakeImport: async (path: string) => {
    if (path.endsWith("editor.api")) {
      attempts++;
      if (fail) throw new Error("missing local editor chunk");
      return api;
    }
    return { default: class Worker {} };
  } };
  runInNewContext(loaderCode, context);
  const first = context.load();
  assert.equal(context.load(), first);
  await assert.rejects(first, /missing local editor chunk/);
  fail = false;
  assert.equal(await context.load(), api);
  assert.equal(await context.load(), api);
  assert.equal(attempts, 2);
});

for (const delayedGate of [null, "admission", "saving"]) test(`Monaco keeps current read-only state (deferred gate: ${delayedGate ?? "none"})`, async () => {
  const refs: { current: any }[] = [];
  const dependencies: unknown[][] = [];
  let cursor = 0;
  let pending: (() => unknown)[] = [];
  let text = "original";
  let valueReads = 0;
  let changeContent: (() => void) | undefined;
  let readOnly: boolean | undefined;
  let lang: "ko" | "en" = "ko";
  let editorCreates = 0;
  let actionDisposals = 0;
  let optionUpdates = 0;
  let accessibleName = "";
  const saves: string[] = [];
  let saveAction: { run(): void; keybindings: number[]; label: string } | undefined;
  let resolveLoader!: (value: unknown) => void;
  const loader = new Promise(resolve => { resolveLoader = resolve; });
  const model = { getValue: () => { valueReads++; return text; }, setValue: (value: string) => { text = value; changeContent?.(); }, onDidChangeContent: (listener: () => void) => { changeContent = listener; return { dispose() { changeContent = undefined; } }; }, dispose() {} };
  const editor = { setModel() {}, updateOptions: (options: { readOnly?: boolean; ariaLabel?: string }) => { optionUpdates++; if (options.readOnly !== undefined) readOnly = options.readOnly; if (options.ariaLabel !== undefined) accessibleName = options.ariaLabel; }, addAction: (action: typeof saveAction) => { saveAction = action; return { dispose() { actionDisposals++; } }; }, dispose() {} };
  const context: Record<string, any> = {
    useState: (initial: unknown) => [initial, () => {}],
    useI18n: () => ({ t: (key: keyof typeof ko, params: { path?: string } = {}) => (lang === "ko" ? ko : en)[key].replace("{path}", params.path ?? "") }),
    useRef: (initial: unknown) => { const index = cursor++; return refs[index] ??= { current: initial }; },
    useEffect: (effect: () => unknown, deps: unknown[]) => {
      const index = cursor++;
      if (!dependencies[index] || deps.some((value, i) => value !== dependencies[index][i])) pending.push(effect);
      dependencies[index] = deps;
    },
    React: { createElement: (_type: unknown, props: { ref?: { current: unknown } } | null) => { if (props?.ref) props.ref.current = {}; return null; } },
    languageFor: () => "plaintext",
    requestEditorSave,
    loadMonaco: () => loader,
  };
  context.useLayoutEffect = context.useEffect;
  const resolveMonaco = () => resolveLoader({ KeyMod: { CtrlCmd: 2048 }, KeyCode: { KeyS: 49 }, editor: { createModel: (value: string) => { text = value; return model; }, create: (_host: unknown, options: { readOnly: boolean }) => { editorCreates++; readOnly = options.readOnly; return editor; } } });
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
  const beforeDraftUpdate = optionUpdates;
  await render(false, "dirty", "next editable draft");
  assert.equal(text, "next editable draft");
  assert.equal(optionUpdates, beforeDraftUpdate, "text-only changes must not reconfigure the editor");
  await render(false, "dirty", "latest editable draft");
  lang = "en";
  await render(false, "dirty", "latest editable draft");
  assert.equal(editorCreates, 1);
  assert.equal(actionDisposals, 1);
  assert.equal(saveAction.label, "Save to working copy");
  assert.equal(accessibleName, "test.txt code editor");
  assert.equal(text, "latest editable draft");
  saveAction.run();
  assert.deepEqual(saves, ["test.txt"]);
  await render(false, "saving");
  saveAction.run();
  assert.deepEqual(saves, ["test.txt"], "saving must block a duplicate shortcut");
  const beforeInput = valueReads;
  text = "typed locally";
  changeContent!();
  await render(false, "dirty", text);
  await render(true, "dirty", text);
  assert.equal(valueReads, beforeInput + 1, "typing copies model text once; state and gate renders do not read it again");
  await render(false, "clean", "reloaded externally");
  assert.equal(text, "reloaded externally", "external reload still updates the model");
});

for (const failure of ["load", "editor", "action"]) test(`Monaco preserves drafts and explicitly retries ${failure} failure`, async () => {
  const values: any[] = [];
  const dependencies: unknown[][] = [];
  const cleanups: (() => void)[] = [];
  let cursor = 0;
  let pending: { index: number; effect: () => any }[] = [];
  let buttons: any[] = [];
  let drafts: any[] = [];
  let fail = true;
  let loads = 0;
  let text = "";
  let locked = false;
  let lang: "ko" | "en" = "ko";
  const disposed: string[] = [];
  const changes: string[] = [];
  const context: Record<string, any> = {
    useI18n: () => ({ t: (key: keyof typeof ko) => (lang === "ko" ? ko : en)[key] }),
    useState: (initial: unknown) => {
      const index = cursor++;
      if (!(index in values)) values[index] = initial;
      return [values[index], (next: any) => { values[index] = typeof next === "function" ? next(values[index]) : next; }];
    },
    useRef: (initial: unknown) => { const index = cursor++; return values[index] ??= { current: initial }; },
    useEffect: (effect: () => any, deps: unknown[]) => {
      const index = cursor++;
      if (!dependencies[index] || deps.some((value, i) => value !== dependencies[index][i])) {
        dependencies[index] = deps;
        pending.push({ index, effect });
      }
    },
    React: { createElement: (type: string, props: any) => {
      if (props?.ref) props.ref.current ??= {};
      if (type === "button") buttons.push(props);
      if (type === "textarea") drafts.push(props);
      return null;
    } },
    languageFor: () => "plaintext", requestEditorSave,
    loadMonaco: async () => {
      loads++;
      if (fail && failure === "load") throw new Error("chunk unavailable");
      return { KeyMod: { CtrlCmd: 2048 }, KeyCode: { KeyS: 49 }, editor: {
        createModel: (value: string) => {
          text = value;
          return { getValue: () => text, setValue: (next: string) => { text = next; },
            onDidChangeContent: () => ({ dispose() { disposed.push("subscription"); } }),
            dispose() { disposed.push("model"); } };
        },
        create: (_host: unknown, options: { readOnly: boolean }) => {
          if (fail && failure === "editor") throw new Error("editor unavailable");
          locked = options.readOnly;
          return { setModel() {}, updateOptions: (next: { readOnly?: boolean }) => { if (next.readOnly !== undefined) locked = next.readOnly; },
            addAction: () => {
              if (fail && failure === "action") throw new Error("action unavailable");
              return { dispose() { disposed.push("action"); } };
            }, dispose() { disposed.push("editor"); } };
        },
      } };
    },
  };
  context.useLayoutEffect = context.useEffect;
  runInNewContext(compiled, context);
  const render = async () => {
    cursor = 0; pending = []; buttons = []; drafts = [];
    context.Component({ tab: { path: "draft.txt", mode: "text", text: "unsaved draft", saveState: "dirty" },
      readOnly: true, onChange: (value: string) => changes.push(value), onSave() { assert.fail("must not save during recovery"); } });
    for (const { index, effect } of pending) { cleanups[index]?.(); cleanups[index] = effect(); }
    await Promise.resolve(); await Promise.resolve();
  };
  await render(); await render();
  assert.equal(loads, 1, "failure must not trigger an automatic retry");
  assert.equal(drafts[0].value, "unsaved draft");
  assert.equal(drafts[0].readOnly, true);
  assert.equal(drafts[0]["aria-label"], "보존된 편집 내용");
  lang = "en";
  await render();
  assert.equal(drafts[0]["aria-label"], "Preserved edits");
  assert.equal(drafts[0].value, "unsaved draft");
  assert.equal(drafts[0].readOnly, true);
  assert.equal(loads, 1, "language change must not retry initialization");
  assert.deepEqual(disposed, failure === "action" ? ["editor"] : []);
  fail = false;
  buttons[0].onClick();
  await render(); await render();
  assert.equal(loads, 2);
  assert.equal(drafts.length, 0);
  assert.equal(text, "unsaved draft");
  assert.equal(locked, true, "retry must retain the current admission lock");
  assert.deepEqual(changes, []);
  const before = disposed.length;
  for (const cleanup of cleanups) cleanup?.();
  assert.deepEqual(disposed.slice(before), ["subscription", "model", "action", "editor"]);
});
