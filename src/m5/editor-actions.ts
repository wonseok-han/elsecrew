import type { EditorTab } from "./editor-state.ts";

/** Keyboard and context-menu entry only; the controller still owns save admission. */
export function requestEditorSave(tab: Pick<EditorTab, "path" | "mode" | "saveState">, readOnly: boolean, save: (path: string) => void): void {
  if (readOnly || tab.mode !== "text" || tab.saveState !== "dirty") return;
  save(tab.path);
}
