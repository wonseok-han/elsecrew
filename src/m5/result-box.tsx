import type { ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import { changesStatus, type ResultBarView, type TaskPhase } from "./task-status.ts";

/**
 * The result box (ADE 1차-나 L5): at the bottom of the results panel,
 * whatever tab is open, and the only place that puts a task's work into the
 * user's folder (merge for a Git task, apply for a copy task) or throws it
 * away. Its words come from the single task status (resultBarView).
 * `waiting`: the team still works — the box says the work can be put in
 * here once it is done, with the button shown but off.
 */
export function ResultBox({ view, waiting, detail, actions, extra }: { view: ResultBarView | null; waiting: boolean; detail: ReactNode; actions: ReactNode; extra?: ReactNode }) {
  const { t } = useI18n();
  if (waiting) return <section className="m5-result-box m5-result-waiting" data-testid="result-box" data-task-phase="working" aria-label={t("m5.result.title")}>
    <span className="m5-result-text"><strong>{t("m5.resultBox.notYet")}</strong><small>{t("m5.resultBox.whenDone")}</small></span>
    <span className="m5-result-actions"><button type="button" disabled>{t("m5.resultBox.put")}</button></span>
  </section>;
  if (!view) return null;
  return <section className={`m5-result-box m5-result-bar${view.loading ? " m5-result-loading" : ""}`} data-testid="result-box" data-task-phase={view.loading ? "loading" : view.phase} aria-label={t("m5.result.title")} aria-busy={view.loading}>
    <span className="m5-result-text"><strong>{t("m5.result.title")}</strong><small>{detail}</small></span>
    <span className="m5-result-actions">{actions}</span>
    {extra && <div className="m5-result-extra">{extra}</div>}
  </section>;
}

/** The changes tab's one line for a copy task (git tasks: ChangesMergeStatus); putting it in is in the result box. */
export function ChangesStandingLine({ phase, changeCount, pending, onGoToResult }: { phase: TaskPhase; changeCount: number; pending: number; onGoToResult: () => void }) {
  const { t } = useI18n();
  const status = changesStatus(phase, changeCount);
  if (!status || status === "working") return null;
  return <section className={`m5-apply-latest${status === "done" ? " m5-apply-latest-applied" : ""}`} data-testid="changes-standing">
    <div className="m5-apply-latest-head">
      <strong>{status === "done" ? t("m5.task.result.applied") : status === "changedSince" ? t("m5.task.result.changedSinceApply", { count: pending }) : t("m5.task.result.review")}</strong>
      {status !== "done" && <button type="button" className="m5-link-button" onClick={onGoToResult}>{t("m5.resultBox.goThere")}</button>}
    </div>
  </section>;
}

/**
 * The preview tab's place (ADE 1차-나 L5). The isolated preview itself
 * (P3) comes later; until then the tab says so and points at the changes.
 */
export function PreviewPlaceholder({ onChanges }: { onChanges: () => void }) {
  const { t } = useI18n();
  return <section className="m5-preview-empty" data-testid="preview-placeholder">
    <strong>{t("m5.preview.notYet")}</strong>
    <p>{t("m5.preview.notYetHelp")}</p>
    <button type="button" className="m5-secondary" onClick={onChanges}>{t("m5.preview.toChanges")}</button>
  </section>;
}
