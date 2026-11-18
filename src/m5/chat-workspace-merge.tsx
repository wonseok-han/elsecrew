import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { validateProjectCatalogResult, type ChatMergeBlockReason, type ChatMergeOutcome, type ChatMergeState } from "./contracts/project-catalog-v1.ts";

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
export function ChatWorkspaceMerge({ chat, disabled, fallback, onMerged, onAskTeam, onState }: {
  chat: ChatRecordV1; disabled: boolean; fallback: React.ReactNode;
  /** The user's folder and the changes list moved; the host refreshes them. */
  onMerged: () => void;
  /** Sends the coordinator a follow-up asking the team to resolve conflicts. */
  onAskTeam: (text: string) => void;
  /** Where the task stands, for the host's summary line. */
  onState?: (state: ChatMergeState | null) => void;
}) {
  const { t } = useI18n();
  const [state, setState] = useState<ChatMergeState | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const generation = useRef(0);
  const address = { chatId: chat.chatId, projectId: chat.projectId, worktreeId: chat.worktreeId, workspaceIdentity: chat.workspaceIdentity };

  const inspect = useCallback(async (): Promise<ChatMergeState | null> => {
    if (!window.m5Projects) return null;
    const epoch = ++generation.current;
    const request = { action: "inspectChatMerge" as const, ...address };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (epoch === generation.current && result.action === "inspectChatMerge") { setState(result.merge); return result.merge; }
    } catch { if (epoch === generation.current) setState({ kind: "snapshot" }); }
    return null;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chat.chatId, chat.worktreeId, chat.workspaceIdentity]);
  useEffect(() => { void inspect(); return () => { generation.current++; }; }, [inspect]);
  useEffect(() => { onState?.(state); }, [state, onState]);

  if (!state) return null;
  if (state.kind === "snapshot") return <>{fallback}</>;

  const base = state.baseRef ?? "main";
  async function merge(keepConflicts = false, current: ChatMergeState | null = state) {
    if (!window.m5Projects || current?.kind !== "gitWorktree" || !current.baseTip || busy) return;
    setBusy(true); setNotice(null);
    const request = { action: "mergeChatWorkspace" as const, ...address, expectedHead: current.head, expectedBase: current.baseTip, keepConflicts, confirmed: true as const };
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
    // Fresh head and base, then merge again keeping the conflict marks.
    setNotice(null);
    await merge(true, await inspect());
  }

  const blocked = state.blocked;
  return <span className="m5-merge">
    <small className="m5-merge-branch" title={state.branch}>
      {t("m5.merge.branch", { branch: state.branch })}
      {state.ahead > 0 && <> · {t("m5.merge.ahead", { count: state.ahead, base })}</>}
      {state.resolving && <> · {state.unresolved > 0 ? t("m5.merge.resolving", { count: state.unresolved }) : t("m5.merge.resolved")}</>}
    </small>
    {confirming
      ? <span className="m5-merge-confirm" role="group" aria-label={t("m5.merge.confirmTitle", { base })}>
          <small>{t(state.baseCheckedOut ? "m5.merge.confirmCheckedOut" : "m5.merge.confirmOther", { base })}</small>
          <button type="button" disabled={disabled || busy} onClick={() => void merge()}>{t(busy ? "m5.merge.merging" : "m5.merge.confirm")}</button>
          <button type="button" className="m5-secondary" disabled={busy} onClick={() => setConfirming(false)}>{t("m5.merge.cancel")}</button>
        </span>
      : <button type="button" disabled={disabled || busy || blocked !== null} title={blocked ? t(`m5.merge.blocked.${blocked}`, { base }) : undefined}
          onClick={() => { setNotice(null); void inspect().then(() => setConfirming(true)); }}>
          {blocked === "nothingToMerge" && notice?.kind === "merged" ? t("m5.merge.done", { base }) : t("m5.merge.open", { base })}
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
