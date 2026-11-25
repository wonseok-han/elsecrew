import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type ChatMergeState } from "./contracts/project-catalog-v1.ts";
import { changesStatus, type TaskPhase } from "./task-status.ts";

/** Where a task's changes stand against the user's folder (both kinds of task). */
export type ChangesStanding = "notYet" | "done" | "changedSince";

export interface ApplyStanding { applied: boolean; pending: number; total: number }

export function applyStanding(state: ApplyStanding | null): ChangesStanding | null {
  if (!state || state.total === 0) return null;
  if (!state.applied) return "notYet";
  return state.pending === 0 ? "done" : "changedSince";
}

/** A task worktree: merged into its base before, and anything new since. */
export function mergeStanding(state: ChatMergeState | null, changeCount: number): ChangesStanding | null {
  if (state?.kind !== "gitWorktree" || !state.baseTip || changeCount === 0) return null;
  const pending = state.ahead > 0 || state.uncommitted;
  if (!state.mergedBefore) return "notYet";
  return pending ? "changedSince" : "done";
}

/**
 * One button: the task's workspace, as it is now, is written over the user's
 * own folder (latest wins, user decision 2026-11-17). One confirmation names
 * what that means; there is no separate review, token or backup step.
 *
 * Three standings (user decision 2026-11-20), kept across restarts by the
 * engine's apply receipt: not applied yet (apply), applied (no button), and
 * changed again since (apply again, counting only what changed since).
 */
export function ApplyLatest({ projectId, chatId, changeCount, disabled, confirmRequest = 0, onApplied, onStanding, onInspected, revision = "" }: {
  projectId: string; chatId: string; changeCount: number; disabled: boolean;
  /** Bumped by "check changes and apply" elsewhere: opens the confirmation. */
  confirmRequest?: number;
  onApplied?: () => void;
  onStanding?: (standing: ChangesStanding | null) => void;
  /** Each answer of the engine's apply receipt (null: could not read it). */
  onInspected?: (state: ApplyStanding | null) => void;
  /** Changes whenever the workspace's changes do (a save, a refresh): look again. */
  revision?: string;
}) {
  const { t } = useI18n();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ written: number; removed: number; skipped: number; failed: string | null } | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [state, setState] = useState<ApplyStanding | null>(null);
  /** The receipt could not be read: show the plain panel rather than wait. */
  const [unreadable, setUnreadable] = useState(false);
  const generation = useRef(0);
  const onInspectedRef = useRef(onInspected);
  onInspectedRef.current = onInspected;
  const inspect = useCallback(async () => {
    if (!window.m5Projects) return;
    const epoch = ++generation.current;
    const request = { action: "inspectChatApply" as const, projectId, chatId };
    try {
      const reply = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (epoch === generation.current && reply.action === "inspectChatApply") {
        const next = { applied: reply.applied, pending: reply.pending, total: reply.total };
        setState(next); setUnreadable(false); onInspectedRef.current?.(next);
      }
    } catch { if (epoch === generation.current) { setState(null); setUnreadable(true); onInspectedRef.current?.(null); } }
  }, [projectId, chatId]);
  useEffect(() => { setConfirming(false); setResult(null); setFailed(null); setState(null); setUnreadable(false); }, [projectId, chatId]);
  // The workspace changed (a new step, a save): look again.
  useEffect(() => { void inspect(); return () => { generation.current++; }; }, [inspect, changeCount, revision]);
  const standing = applyStanding(state);
  useEffect(() => { onStanding?.(standing); }, [standing, onStanding]);
  // A request to confirm waits for the change count: asking refreshes the
  // changes list, which reads as zero for a moment (seen in the real app).
  // Only requests made while this task is shown (never one left from another task).
  const handledRequest = useRef(confirmRequest);
  useEffect(() => {
    if (confirmRequest > handledRequest.current && changeCount > 0) { handledRequest.current = confirmRequest; setConfirming(true); }
  }, [confirmRequest, changeCount]);
  const apply = async () => {
    if (!window.m5Projects || busy) return;
    setBusy(true); setFailed(null); setResult(null);
    const request = { action: "applyChatLatest" as const, projectId, chatId };
    try {
      const reply = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (reply.action === "applyChatLatest") {
        setResult({ written: reply.written, removed: reply.removed, skipped: reply.skipped.length, failed: reply.failed });
        onApplied?.();
      }
      setConfirming(false);
      void inspect();
    } catch (error) {
      setFailed(error instanceof Error ? error.message : String(error));
    } finally { setBusy(false); }
  };
  const done = standing === "done";
  // Until the receipt is read, a placeholder: never "not applied yet" and a
  // button for a task whose standing is not known (seen in the real app).
  if (!state && !unreadable && !failed && !result && window.m5Projects) return <section className="m5-apply-latest m5-apply-latest-loading" aria-label={t("m5.apply.title")} aria-busy="true"><span className="m5-skeleton-line" /></section>;
  return <section className={`m5-apply-latest${done ? " m5-apply-latest-applied" : ""}`} aria-label={t("m5.apply.title")}>
    <div className="m5-apply-latest-head">
      <strong>{done ? t("m5.apply.applied", { count: state?.total ?? changeCount })
        : standing === "changedSince" ? t("m5.apply.changedSince", { count: state?.pending ?? 0 })
        : t("m5.apply.count", { count: changeCount })}</strong>
      {!confirming && !done && <button type="button" disabled={disabled || busy || changeCount === 0} onClick={() => { setConfirming(true); setResult(null); setFailed(null); }}>{t(standing === "changedSince" ? "m5.apply.again" : "m5.apply.action")}</button>}
    </div>
    {confirming && <div className="m5-apply-latest-confirm" role="alertdialog" aria-label={t("m5.apply.confirmTitle")}>
      <p>{t("m5.apply.confirm")}</p>
      <div className="m5-apply-latest-actions">
        <button type="button" disabled={disabled || busy} onClick={() => void apply()}>{busy ? t("m5.apply.applying") : t("m5.apply.confirmAction")}</button>
        <button type="button" className="m5-secondary" disabled={busy} onClick={() => setConfirming(false)}>{t("m5.editor.cancel")}</button>
      </div>
    </div>}
    {result && <p role={result.failed ? "alert" : "status"} className="m5-apply-latest-done">{t(result.failed ? "m5.apply.partial" : "m5.apply.done", { written: result.written, removed: result.removed, path: result.failed ?? "" })}{result.skipped ? ` ${t("m5.apply.skipped", { count: result.skipped })}` : ""}</p>}
    {unreadable && !failed && <p role="status" className="m5-apply-latest-unread">{t("m5.apply.unreadable")} <button type="button" className="m5-link-button" onClick={() => void inspect()}>{t("m5.merge.inspectAgain")}</button></p>}
    {failed && <div role="alert" className="m5-apply-latest-failed"><p>{t("m5.apply.failed")}</p><details><summary>{t("m5.chat.details")}</summary><pre>{failed}</pre></details></div>}
  </section>;
}

/**
 * A task worktree's changes panel says where the task stands against its base
 * branch, the same three ways; merging itself stays in the result bar (one
 * place, one confirmation), which this button opens.
 */
export function ChangesMergeStatus({ state, changeCount, canMerge, onMerge, phase }: {
  state: ChatMergeState | null; changeCount: number;
  /** The result bar's merge is there (the task is ready for review). */
  canMerge: boolean;
  onMerge: () => void;
  /** The task's single status (task-status.ts); the line never says more than it. */
  phase: TaskPhase;
}) {
  const { t } = useI18n();
  const status = changesStatus(phase, changeCount);
  if (!status || status === "working" || state?.kind !== "gitWorktree") return null;
  const base = state.baseRef ?? "main";
  const done = status === "done";
  return <section className={`m5-apply-latest${done ? " m5-apply-latest-applied" : ""}`} aria-label={t("m5.merge.open", { base })}>
    <div className="m5-apply-latest-head">
      <strong>{done ? t("m5.mergeStatus.merged", { base, count: changeCount })
        : status === "changedSince" ? t("m5.mergeStatus.newSince", { base })
        : t("m5.mergeStatus.notYet", { base, count: changeCount })}</strong>
      {!done && canMerge && <button type="button" onClick={onMerge}>{t(status === "changedSince" ? "m5.mergeStatus.again" : "m5.merge.open", { base })}</button>}
    </div>
  </section>;
}
