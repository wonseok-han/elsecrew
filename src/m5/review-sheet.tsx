import type { ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import type { DiffEntryV1 } from "./contracts/diff-v1.ts";
import { ChangeKindLetter, LineCounts } from "./changes-view.tsx";
import type { SheetMode } from "./task-layout-model.ts";

/**
 * The wide review sheet's frame (ADE 1차-나 L2): changes or the editor,
 * and Esc to close (a single file's comparison keeps its own side-by-side
 * choice). In changes mode a file list
 * sits on the left. The editor itself is the workbench's own section,
 * passed in as children, so it is never remounted.
 */
export function ReviewSheet({ mode, onMode, entries, selectedPath, onSelectFile, dirty, onClose, children }: {
  mode: SheetMode;
  onMode: (mode: SheetMode) => void;
  /** Changed files of the open task; null while unread. */
  entries: readonly DiffEntryV1[] | null;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  dirty: number;
  onClose: () => void;
  children: ReactNode;
}) {
  const { t } = useI18n();
  const count = entries?.length ?? 0;
  return <>
    <header className="m5-sheet-head">
      <h2>{t(mode === "changes" ? "m5.sheet.changes" : "m5.sheet.editor")}</h2>
      {mode === "changes" && entries && <span className="m5-sheet-sub">{t("m5.result.files", { count })}</span>}
      <span className="m5-seg" role="group" aria-label={t("m5.sheet.mode")}>
        <button type="button" aria-pressed={mode === "changes"} data-testid="sheet-mode-changes" disabled={count === 0 && mode !== "changes"} onClick={() => onMode("changes")}>{t("m5.sheet.changes")}</button>
        <button type="button" aria-pressed={mode === "editor"} data-testid="sheet-mode-editor" onClick={() => onMode("editor")}>{t("m5.sheet.editor")}</button>
      </span>
      <span className="m5-sheet-spacer" />
      <button type="button" className="m5-secondary m5-sheet-close" data-testid="sheet-close" onClick={onClose}>
        {t("m5.sheet.close")}{dirty > 0 && <span className="m5-sheet-dirty" title={t("m5.sheet.unsaved", { count: dirty })} aria-label={t("m5.sheet.unsaved", { count: dirty })}>●</span>}
      </button>
    </header>
    <div className={`m5-sheet-body${mode === "changes" && count > 0 ? " m5-sheet-with-files" : ""}`}>
      {mode === "changes" && count > 0 && <nav className="m5-sheet-files" aria-label={t("m5.sheet.files")}>
        {entries!.map(entry => <button key={entry.path} type="button" className="m5-sheet-file" title={entry.path} aria-current={entry.path === selectedPath ? "true" : undefined}
          data-sheet-focus={entry.path === selectedPath ? "" : undefined} onClick={() => onSelectFile(entry.path)}>
          <span className="m5-sheet-file-name">{entry.path.split("/").pop()}</span>
          <LineCounts entry={entry} />
          <ChangeKindLetter entry={entry} />
        </button>)}
      </nav>}
      <div className="m5-sheet-main">{children}</div>
    </div>
  </>;
}
