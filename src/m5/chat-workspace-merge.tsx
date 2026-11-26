import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { validateProjectCatalogResult, type ChatMergeBlockReason, type ChatMergeOutcome, type ChatMergeState } from "./contracts/project-catalog-v1.ts";
import { mergeConfirmationFor, mergeConfirmationStillValid, type MergeConfirmation } from "./merge-confirmation.ts";

type Notice =
  | { kind: "merged" }
  | { kind: "conflict"; count: number; paths: string[]; teamResolvable: boolean }
  | { kind: "resolving"; count: number }
  | { kind: "blocked"; reason: ChatMergeBlockReason }
  | { kind: "busy" }
  | { kind: "error" };

/**
 * Task = PR (docs/proposals/m5-real-worktrees.md §1.1): a task worktree is
 * merged into its base branch instead of copied into the user's folder. A
 * v1 copy keeps the old apply button (`fallback`).
 */
export function ChatWorkspaceMerge({ chat, disabled, fallback, merged = false, onMerged, onAskTeam, onState, revision = "" }: {
  chat: ChatRecordV1; disabled: boolean; fallback: React.ReactNode;
  /** The host knows the task's changes are already in the base (after a reload). */
  merged?: boolean;
  /** The user's folder and the changes list moved; the host refreshes them. */
  onMerged: () => void;
  /** Sends the coordinator a follow-up asking the team to resolve conflicts. */
  onAskTeam: (text: string) => void;
  /** Where the task stands, for the host's summary line. */
  onState?: (state: ChatMergeState | null) => void;
  /** Changes whenever the workspace's changes do (a save, a refresh): look again. */
  revision?: string;
}) {
  const { t } = useI18n();
  const [state, setState] = useState<ChatMergeState | null>(null);
  /** The head and base the person is confirming (N1): the merge is sent with these. */
  const [confirmation, setConfirmation] = useState<MergeConfirmation | null>(null);
  const confirming = confirmation !== null;
  const setConfirming = (open: false) => { if (!open) setConfirmation(null); };
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  /** The state could not be read: say so and offer to look again. */
  const [unavailable, setUnavailable] = useState(false);
  const generation = useRef(0);
  const address = { chatId: chat.chatId, projectId: chat.projectId, worktreeId: chat.worktreeId, workspaceIdentity: chat.workspaceIdentity };

  const inspect = useCallback(async (): Promise<ChatMergeState | null> => {
    if (!window.m5Projects) return null;
    const epoch = ++generation.current;
    const request = { action: "inspectChatMerge" as const, ...address };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (epoch === generation.current && result.action === "inspectChatMerge") { setUnavailable(false); setState(result.merge); return result.merge; }
    // A v1 copy answers `snapshot` itself. A task that cannot be inspected
    // (one of a folder's earlier record, or the engine busy) offers neither
    // merge nor apply, which would fail; it says so and can look again.
    } catch { if (epoch === generation.current) { setState(null); setUnavailable(true); } }
    return null;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chat.chatId, chat.worktreeId, chat.workspaceIdentity]);
  useEffect(() => { void inspect(); return () => { generation.current++; }; }, [inspect, revision]);
  // Reported once known: null then means the state could not be read.
  useEffect(() => { if (state || unavailable) onState?.(state); }, [state, unavailable, onState]);
  // A later read that moved the task or the base ends the open confirmation:
  // the person confirms again what will actually be merged.
  useEffect(() => {
    if (confirmation && !busy && !mergeConfirmationStillValid(confirmation, state)) {
      setConfirmation(null);
      setNotice({ kind: "blocked", reason: "changedSinceReview" });
    }
  }, [state, confirmation, busy]);

  if (!state) {
    return unavailable
      ? <span className="m5-merge"><small className="m5-merge-notice" role="status">{t("m5.merge.unavailable")}</small>{" "}
          <button type="button" className="m5-link-button" onClick={() => void inspect()}>{t("m5.merge.inspectAgain")}</button></span>
      : null;
  }
  if (state.kind === "snapshot") return <>{fallback}</>;

  const base = state.baseRef ?? "main";
  /** Sends the merge for exactly what was confirmed (`target`), never a later read. */
  async function merge(target: MergeConfirmation | null, keepConflicts = false) {
    if (!window.m5Projects || !target || busy) return;
    if (!mergeConfirmationStillValid(target, state)) { setConfirmation(null); setNotice({ kind: "blocked", reason: "changedSinceReview" }); return; }
    setBusy(true); setNotice(null);
    const request = { action: "mergeChatWorkspace" as const, ...address, expectedHead: target.head, expectedBase: target.baseTip, keepConflicts, confirmed: true as const };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (result.action !== "mergeChatWorkspace") return;
      const outcome = result as unknown as ChatMergeOutcome;
      setState(outcome.merge);
      if (outcome.outcome === "merged") { setNotice({ kind: "merged" }); onMerged(); }
      else if (outcome.outcome === "conflict") setNotice({ kind: "conflict", count: outcome.conflictCount, paths: outcome.conflicts, teamResolvable: outcome.teamResolvable });
      else if (outcome.outcome === "resolving") {
        // The marks are in the worktree now; the team resolves them.
        setNotice({ kind: "resolving", count: outcome.conflictCount });
        const shown = outcome.conflicts.slice(0, 20).join(", ") + (outcome.conflictCount > 20 ? " …" : "");
        onAskTeam(t("m5.merge.askTeamMessage", { base, count: outcome.conflictCount, files: shown }));
      }
      else setNotice({ kind: "blocked", reason: outcome.reason });
    } catch (error) {
      // An agent still running somewhere: wait, not a failure.
      setNotice(String(error instanceof Error ? error.message : error).includes("requestBlocked") ? { kind: "busy" } : { kind: "error" });
    }
    finally { setBusy(false); setConfirming(false); }
  }

  async function askTeam() {
    // Fresh head and base (the person asked the team to resolve this exact
    // conflict), then merge again keeping the conflict marks.
    setNotice(null);
    await merge(mergeConfirmationFor(await inspect()), true);
  }

  const blocked = state.blocked;
  return <span className="m5-merge">
    {/* Branches and commits are for those who ask: under "details". */}
    {state.resolving && <small className="m5-merge-resolving">{state.unresolved > 0 ? t("m5.merge.resolving", { count: state.unresolved }) : t("m5.merge.resolved")}</small>}
    <details className="m5-merge-more"><summary>{t("m5.feedback.details")}</summary>
      <small className="m5-merge-branch" title={state.branch}>
        {t("m5.merge.branch", { branch: state.branch })}
        {state.ahead > 0 && <> · {t("m5.merge.ahead", { count: state.ahead, base })}</>}
      </small>
    </details>
    {confirming
      ? <span className="m5-merge-confirm" role="group" aria-label={t("m5.merge.confirmTitle", { base })}>
          <small>{t(state.baseCheckedOut ? "m5.merge.confirmCheckedOut" : "m5.merge.confirmOther", { base })}</small>
          <button type="button" disabled={disabled || busy} onClick={() => void merge(confirmation)}>{t(busy ? "m5.merge.merging" : "m5.merge.confirm")}</button>
          <button type="button" className="m5-secondary" disabled={busy} onClick={() => setConfirming(false)}>{t("m5.merge.cancel")}</button>
        </span>
      : <button type="button" disabled={disabled || busy || blocked !== null} title={blocked ? t(`m5.merge.blocked.${blocked}`, { base }) : undefined}
          onClick={() => { setNotice(null); void inspect().then(fresh => { const target = mergeConfirmationFor(fresh); if (target) setConfirmation(target); }); }}>
          {blocked === "nothingToMerge" && (merged || notice?.kind === "merged") ? t("m5.merge.done", { base }) : t("m5.merge.open", { base })}
        </button>}
    {notice && <span className="m5-merge-notice" role="status">
      {notice.kind === "merged" && t("m5.merge.merged", { base })}
      {notice.kind === "conflict" && <>{t("m5.merge.conflict", { count: notice.count, base })} <code>{notice.paths.slice(0, 5).join(", ")}{notice.count > 5 ? " …" : ""}</code>{" "}
        {notice.teamResolvable
          ? <button type="button" className="m5-link-button" disabled={disabled || busy} onClick={() => void askTeam()}>{t("m5.merge.askTeam")}</button>
          : <small>{t("m5.merge.notTeamResolvable")}</small>}</>}
      {notice.kind === "resolving" && t("m5.merge.askedTeam", { count: notice.count })}
      {notice.kind === "blocked" && t(`m5.merge.blocked.${notice.reason}`, { base })}
      {notice.kind === "busy" && t("m5.merge.busy")}
      {notice.kind === "error" && t("m5.merge.error")}
    </span>}
  </span>;
}
