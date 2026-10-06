import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import test from "node:test";
import ts from "typescript";

const require = createRequire(import.meta.url);

// Tests the outer identity boundary only; async React lifecycle needs UI QA.
function load(file: string, name: string): (props: any) => { key: string } {
  const compiled = ts.transpileModule(readFileSync(new URL(`../src/m5/${file}.tsx`, import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020 },
  }).outputText;
  const context: any = { exports: {}, require(name: string) {
    return name.startsWith(".") ? {} : require(name);
  } };
  runInNewContext(compiled, context);
  return context.exports[name];
}

test("application sessions bind every reviewed target field, not transient disabled state", () => {
  const render = load("application-start-control", "ApplicationStartControl");
  for (const request of [
    { action: "startApplication", projectId: "project_a", chatId: "chat_a", confirmationId: "confirmation_a" },
    { action: "restoreApplication", projectId: "project_a", applicationId: "application_a", fingerprint: "fingerprint_a" },
  ]) {
    const key = render({ request, disabled: false }).key;
    assert.equal(render({ request: { ...request }, disabled: true }).key, key);
    for (const field of Object.keys(request).filter(field => field !== "action")) {
      assert.notEqual(render({ request: { ...request, [field]: "other" }, disabled: false }).key, key, field);
    }
  }
});

test("preview sessions separate projects, chats, workspaces and restoration records", () => {
  for (const [file, name, props] of [
    ["chat-application-preview", "ChatApplicationPreview", { projectId: "p", chatId: "c", worktreeId: "w" }],
    ["restoration-preview", "RestorationPreview", { projectId: "p", applicationId: "a" }],
    ["preparation-recovery-view", "PreparationRecovery", { projectId: "p" }],
  ] as const) {
    const render = load(file, name);
    const key = render({ ...props, disabled: false }).key;
    assert.equal(render({ ...props, disabled: true }).key, key);
    for (const field of Object.keys(props)) {
      assert.notEqual(render({ ...props, [field]: "other", disabled: false }).key, key, field);
    }
  }
});
