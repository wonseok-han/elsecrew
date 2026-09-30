import assert from "node:assert/strict";
import test from "node:test";
import { requestEditorSave } from "../src/m5/editor-actions.ts";

test("editor save action admits only editable dirty text and binds its path", () => {
  for (const mode of ["text", "binary", "tooLarge"] as const) {
    for (const saveState of ["clean", "dirty", "saving", "conflicted"] as const) {
      for (const readOnly of [false, true]) {
        const paths: string[] = [];
        requestEditorSave({ path: "src/a.ts", mode, saveState }, readOnly, path => paths.push(path));
        assert.deepEqual(paths, mode === "text" && saveState === "dirty" && !readOnly ? ["src/a.ts"] : []);
      }
    }
  }
});
