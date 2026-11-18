import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { validateProjectCatalogResult, type ChatMergeOutcome, type ChatMergeState } from "./contracts/project-catalog-v1.ts";

type Notice =
  | { kind: "merged" }
  | { kind: "conflict"; count: number; paths: string[] }
  | { kind: "blocked"; reason: "noBaseBranch" | "changedSinceReview" | "userChangesOverlap" }
  | { kind: "error" };

/**
 * Task = PR (docs/proposals/m5-real-worktrees.md §1.1): a task worktree is
 * merged into its base branch instead of copied into the user's folder. A
 * v1 copy keeps the old apply button (`fallback`).
 */
export function ChatWorkspaceMerge({ chat, disabled, fallback, onMerged }: {
  chat: ChatRecordV1; disabled: boolean; fallback: React.ReactNode;
  /** The user's folder and the changes list moved; the host refreshes them. */
  onMerged: () => void;
}) {
  const { t } = useI18n();
  const [state, setState] = useState<ChatMergeState | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const generation = useRef(0);
  const address = { chatId: chat.chatId, projectId: chat.projectId, worktreeId: chat.worktreeId, workspaceIdentity: chat.workspaceIdentity };

  const inspect = useCallback(async () => {
    if (!window.m5Projects) return;
    const epoch = ++generation.current;
    const request = { action: "inspectChatMerge" as const, ...address };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (epoch === generation.current && result.action === "inspectChatMerge") setState(result.merge);
    } catch { if (epoch === generation.current) setState({ kind: "snapshot" }); }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chat.chatId, chat.worktreeId, chat.workspaceIdentity]);
  useEffect(() => { void inspect(); return () => { generation.current++; }; }, [inspect]);

  if (!state) return null;
  if (state.kind === "snapshot") return <>{fallback}</>;

  const base = state.baseRef ?? "main";
  async function merge() {
    if (!window.m5Projects || state?.kind !== "gitWorktree" || !state.baseTip || busy) return;
    setBusy(true); setNotice(null);
    const request = { action: "mergeChatWorkspace" as const, ...address, expectedHead: state.head, expectedBase: state.baseTip, confirmed: true as const };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (result.action !== "mergeChatWorkspace") return;
      const outcome = result as unknown as ChatMergeOutcome;
      setState(outcome.merge);
      if (outcome.outcome === "merged") { setNotice({ kind: "merged" }); onMerged(); }
      else if (outcome.outcome === "conflict") setNotice({ kind: "conflict", count: outcome.conflictCount, paths: outcome.conflicts });
      else setNotice({ kind: "blocked", reason: outcome.reason });
    } catch { setNotice({ kind: "error" }); }
    finally { setBusy(false); setConfirming(false); }
  }

  const blocked = state.blocked;
  return <span className="m5-merge">
    <small className="m5-merge-branch" title={state.branch}>
      {t("m5.merge.branch", { branch: state.branch })}
      {state.ahead > 0 && <> · {t("m5.merge.ahead", { count: state.ahead, base })}</>}
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
      {notice.kind === "conflict" && <>{t("m5.merge.conflict", { count: notice.count, base })} <code>{notice.paths.slice(0, 5).join(", ")}{notice.count > 5 ? " …" : ""}</code></>}
      {notice.kind === "blocked" && t(`m5.merge.blocked.${notice.reason}`, { base })}
      {notice.kind === "error" && t("m5.merge.error")}
    </span>}
  </span>;
}
