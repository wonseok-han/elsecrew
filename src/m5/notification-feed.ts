import { outcomeOfChatState, type NotificationCenter, type NotificationKind } from "./notifications.ts";

// Structural views of the bridge pieces this feed listens to; every one is
// optional so older bridges simply produce fewer notifications.
interface RunIdentity { runId: string; chatId: string; taskId: string; projectId: string; worktreeId: string; workspaceIdentity: string }
interface RunEvent { run: RunIdentity & { state: string; selectedProfile: { roleId: string } } }
interface ChatEvent { messageId: string; chatId: string; kind: string; text: string }
interface PublicEvent { eventId: string; chatId: string; kind: string; text: string }
interface ChatDetail { chat: { chatId: string; projectId: string; goal: string; state: string } }
export interface NotificationFeedBridge {
  onAgentRunStateChanged?(listener: (event: RunEvent) => void): () => void;
  onChatEvent?(listener: (event: ChatEvent) => void): () => void;
  onAgentPublicEvent?(listener: (event: PublicEvent) => void): () => void;
  getChat?(input: { chatId: string; afterEventSequence: number; cursor: null; limit: number }): Promise<unknown>;
}
interface ApprovalBridge { request(input: unknown): Promise<unknown> }

const APPROVAL_POLL_MS = 2000;

/**
 * Turns task events into notifications: the coordinator finishing a turn
 * (done), a question or pending approval (attention), and failed or paused
 * work (stopped). Returns a stop function.
 */
export function startNotificationFeed(bridge: NotificationFeedBridge, center: NotificationCenter, approvals: ApprovalBridge | undefined, onAdded: (id: string) => void): () => void {
  const running = new Map<string, RunIdentity>();
  const seenApprovals = new Set<string>();
  /** Requests still open per run, so the bell settles them once they go away. */
  const openApprovals = new Map<string, Set<string>>();
  const settle = (runId: string, still: ReadonlySet<string> = new Set()) => {
    const open = openApprovals.get(runId);
    if (!open) return;
    for (const approvalId of [...open]) if (!still.has(approvalId)) { open.delete(approvalId); center.resolve(`approval:${approvalId}`); }
    if (!open.size) openApprovals.delete(runId);
  };
  let disposed = false;
  const chatOf = async (chatId: string): Promise<ChatDetail["chat"] | null> => {
    try {
      const detail = await bridge.getChat?.({ chatId, afterEventSequence: 0, cursor: null, limit: 1 }) as ChatDetail | undefined;
      return detail?.chat ?? null;
    } catch { return null; }
  };
  const add = (id: string, kind: NotificationKind, chat: ChatDetail["chat"], text = "") => {
    if (!disposed && center.add({ id, kind, projectId: chat.projectId, chatId: chat.chatId, goal: chat.goal, text, at: new Date().toISOString() })) onAdded(id);
  };
  /** Adds when the chat's settled state matches `want`. */
  const whenState = async (chatId: string, id: string, want: NotificationKind, text = "") => {
    const chat = await chatOf(chatId);
    if (chat && outcomeOfChatState(chat.state) === want) add(id, want, chat, text);
  };
  const unsubscribers = [
    bridge.onAgentRunStateChanged?.(({ run }) => {
      if (run.state === "running") { running.set(run.runId, run); return; }
      running.delete(run.runId);
      latest.delete(run.runId);
      // A run that stopped asks nothing any more; nor does its task once no
      // other run of it is working (requests seen before a reload included).
      settle(run.runId);
      if (![...running.values()].some(other => other.chatId === run.chatId)) center.resolveApprovalsFor(run.chatId);
      if (run.state === "failed" || run.state === "recoveryRequired") {
        void chatOf(run.chatId).then(chat => { if (chat) add(`run:${run.runId}`, "stopped", chat); });
      } else if (run.state === "succeeded" && run.selectedProfile.roleId === "coordinator") {
        // A coordinator run either delegates (the team continues) or answers.
        void whenState(run.chatId, `done:${run.runId}`, "done");
      }
    }),
    // A team pause has no run of its own; its status message carries the reason.
    bridge.onChatEvent?.(event => {
      if (event.kind === "status") void whenState(event.chatId, `msg:${event.messageId}`, "stopped", event.text);
    }),
    bridge.onAgentPublicEvent?.(event => {
      if (event.kind === "question") void chatOf(event.chatId).then(chat => { if (chat) add(`question:${event.eventId}`, "attention", chat, event.text); });
    }),
  ];
  // Approvals are listed per run; only runs this session saw start are polled.
  // One list at a time per run (answers can take seconds), and a request is
  // only announced if it is still waiting in that run's latest list when the
  // task's details come back.
  const listing = new Set<string>();
  const latest = new Map<string, Set<string>>();
  const stillWaiting = (runId: string, approvalId: string) => running.has(runId) && Boolean(latest.get(runId)?.has(approvalId));
  const timer = approvals ? setInterval(() => {
    for (const identity of running.values()) {
      const { runId, chatId, taskId, projectId, worktreeId, workspaceIdentity } = identity;
      if (listing.has(runId)) continue;
      listing.add(runId);
      void approvals.request({ action: "list", identity: { chatId, taskId, projectId, worktreeId, workspaceIdentity, runId } }).then(result => {
        const pending = (result as { pending?: { approvalId: string }[] } | null)?.pending;
        // A slow answer that arrives after its run ended is out of date.
        if (!Array.isArray(pending) || !running.has(runId)) return;
        const ids = new Set(pending.map(item => item.approvalId));
        latest.set(runId, ids);
        settle(runId, ids);
        if (pending.length) openApprovals.set(runId, new Set([...(openApprovals.get(runId) ?? []), ...ids]));
        for (const approvalId of ids) {
          if (seenApprovals.has(approvalId)) continue;
          seenApprovals.add(approvalId);
          void chatOf(chatId).then(chat => { if (chat && stillWaiting(runId, approvalId)) add(`approval:${approvalId}`, "attention", chat); });
        }
      }).catch(() => undefined).finally(() => { listing.delete(runId); });
    }
  }, APPROVAL_POLL_MS) : null;
  return () => {
    disposed = true;
    if (timer) clearInterval(timer);
    for (const unsubscribe of unsubscribers) unsubscribe?.();
  };
}
