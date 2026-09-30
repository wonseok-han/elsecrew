import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";
import ts from "typescript";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

const source = ts.createSourceFile("i18n.tsx", readFileSync(new URL("../src/i18n.tsx", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const fn = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "detectInitialLang")!;
const compiled = ts.transpileModule(`const STORAGE_KEY = 'hello-agent.lang';\n${fn.getText(source)}\nglobalThis.detect = detectInitialLang;`, {}).outputText;

test("language choice survives restart and unavailable storage falls back to system locale", () => {
  for (const [saved, locale, expected] of [["en", "ko-KR", "en"], ["ko", "en-US", "ko"], [null, "ko-KR", "ko"], ["unknown", "fr-FR", "en"]]) {
    const context: Record<string, any> = { localStorage: { getItem: () => saved }, navigator: { language: locale } };
    runInNewContext(compiled, context);
    assert.equal(context.detect(), expected);
  }
  const context: Record<string, any> = { localStorage: { getItem() { throw new Error("storage denied"); } }, navigator: { language: "ko-KR" } };
  runInNewContext(compiled, context);
  assert.equal(context.detect(), "ko");
});

test("both dictionaries cover the workbench shell and admission keys", () => {
  const keys = Object.keys(ko).filter(key => key.startsWith("m5.")) as (keyof typeof ko)[];
  assert.equal(keys.length, 16);
  for (const key of keys) {
    assert.ok(ko[key].length > 0);
    assert.ok(en[key].length > 0);
    assert.notEqual(en[key], ko[key]);
  }
});
