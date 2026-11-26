import type { ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import type { ResultBarView, TaskPhase } from "./task-status.ts";
import type { ChangesLine, ResultBoxMode } from "./task-view-model.ts";

/**
 * The result box (ADE 1차-나 L5): at the bottom of the results panel,
 * whatever tab is open, and the only place that puts a task's work into the
 * user's folder (merge for a Git task, apply for a copy task) or throws it
 * away. Its words come from the single task status (resultBarView).
 * `waiting`: the team still works — the box says the work can be put in
 * here once it is done, with the button shown but off.
 */
export function ResultBox({ view, mode, detail, actions, extra }: { view: ResultBarView | null; mode: ResultBoxMode; detail: ReactNode; actions: ReactNode; extra?: ReactNode }) {
  const { t } = useI18n();
  if (mode === "waiting") return <section className="m5-result-box m5-result-waiting" data-testid="result-box" data-task-phase="working" aria-label={t("m5.result.title")}>
    <span className="m5-result-text"><strong>{t("m5.resultBox.notYet")}</strong><small>{t("m5.resultBox.whenDone")}</small></span>
    <span className="m5-result-actions"><button type="button" disabled>{t("m5.resultBox.put")}</button></span>
  </section>;
  // A task stopped for a check (recovery, needs attention) is not waiting for the team (review #3).
  if (mode === "attention") return <section className="m5-result-box m5-result-attention" data-testid="result-box" data-task-phase="attention" aria-label={t("m5.result.title")}>
    <span className="m5-result-text"><strong>{t("m5.resultBox.attention")}</strong><small>{t("m5.resultBox.attentionHelp")}</small></span>
  </section>;
  if (mode === "hidden" || !view) return null;
  return <section className={`m5-result-box m5-result-bar${view.loading ? " m5-result-loading" : ""}`} data-testid="result-box" data-task-phase={view.loading ? "loading" : view.phase} aria-label={t("m5.result.title")} aria-busy={view.loading}>
    <span className="m5-result-text"><strong>{t("m5.result.title")}</strong><small>{detail}</small></span>
    <span className="m5-result-actions">{actions}</span>
    {extra && <div className="m5-result-extra">{extra}</div>}
  </section>;
}

/** The changes tab's one line for a copy task with a known standing (git tasks: ChangesMergeStatus); putting it in is in the result box. */
export function ChangesStandingLine({ line, pending, onGoToResult }: { line: ChangesLine; pending: number; onGoToResult: () => void }) {
  const { t } = useI18n();
  if (line !== "notYet" && line !== "done" && line !== "changedSince") return null;
  return <section className={`m5-apply-latest${line === "done" ? " m5-apply-latest-applied" : ""}`} data-testid="changes-standing" data-line={line}>
    <div className="m5-apply-latest-head">
      <strong>{line === "done" ? t("m5.task.result.applied") : line === "changedSince" ? t("m5.task.result.changedSinceApply", { count: pending }) : t("m5.task.result.review")}</strong>
      {line !== "done" && <button type="button" className="m5-link-button" onClick={onGoToResult}>{t("m5.resultBox.goThere")}</button>}
    </div>
  </section>;
}

/**
 * The changes tab's line while nothing about the folder can be said (review
 * #3, #4): the team works, the task needs a check, the standing is being
 * read, or the read failed (then "look again", as in the result box).
 */
export function ChangesStatusLine({ line, count, phase, onRetry, retryDisabled }: { line: ChangesLine; count: number; phase: TaskPhase; onRetry: () => void; retryDisabled: boolean }) {
  const { t } = useI18n();
  if (line === "working") return <p className="m5-task-changes-working" data-task-phase={phase} data-line={line}>{t("m5.task.changes.working", { count })}</p>;
  if (line === "attention") return <p className="m5-task-changes-working" data-task-phase={phase} data-line={line}>{t("m5.task.changes.attention", { count })}</p>;
  if (line === "loading") return <p className="m5-task-changes-working" data-line={line} aria-busy="true"><span className="m5-skeleton-line" aria-label={t("m5.task.loading")} /></p>;
  if (line === "failed") return <p className="m5-task-changes-working" role="status" data-line={line}>{t("m5.task.result.readFailed")} <button type="button" className="m5-link-button" disabled={retryDisabled} onClick={onRetry}>{t("m5.merge.inspectAgain")}</button></p>;
  return null;
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
