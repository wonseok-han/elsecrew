import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { recoveryConfirmation } from "./chat-notice.ts";
import { MessageBody } from "./message-body.tsx";
import { isAgentChatDetailPageV1, type AgentChatGetResultV1 } from "./contracts/agent-adapter-v1.ts";
import type { ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { recoveryChatPage, recoveryHistoryPage, resolveHistoryRecovery, type RecoveryHistoryBridge } from "./project-recovery-history.ts";

export function ProjectRecoveryHistory({ bridge, projectId, disabled }: { bridge: RecoveryHistoryBridge; projectId: string; disabled: boolean }) {
  const { t, lang } = useI18n();
  const [chats, setChats] = useState<readonly ChatRecordV1[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [detail, setDetail] = useState<AgentChatGetResultV1 | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<"error" | "resolved" | "blocked" | null>(null);
  const generation = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    generation.current++; setChats([]); setCursor(null); setLoaded(false); setDetail(null); setStatus(null);
    return () => { generation.current++; };
  }, [projectId, disabled]);
  const perform = async (operation: (current: () => boolean) => Promise<void>) => {
    if (disabled || pending.current) return;
    const epoch = generation.current;
    const current = () => generation.current === epoch;
    pending.current = true; setBusy(true); setStatus(null);
    try { await operation(current); }
    catch { if (current()) setStatus("error"); }
    finally { pending.current = false; setBusy(false); }
  };
  const list = (next: string | null = null) => perform(async current => {
    const page = await recoveryChatPage(bridge, projectId, next);
    if (current()) { setChats(previous => next ? [...previous, ...page.items.filter(item => !previous.some(old => old.chatId === item.chatId))] : page.items); setCursor(page.nextCursor); setLoaded(true); }
  });
  const read = (chat: ChatRecordV1, after = 0) => perform(async current => {
    const page = await recoveryHistoryPage(bridge, chat, after);
    if (current()) setDetail(page);
  });
  const recovery = detail && isAgentChatDetailPageV1(detail) ? detail.agentRecovery : null;
  const resolve = () => {
    if (!recovery || !detail || disabled || pending.current || !globalThis.confirm(recoveryConfirmation(true, lang))) return;
    const selected = detail.chat;
    void perform(async current => {
      const result = await resolveHistoryRecovery(bridge, recovery, current);
      if (!current()) return;
      const refreshed = await recoveryHistoryPage(bridge, selected);
      if (!current()) return;
      setDetail(refreshed);
      setStatus(result.outcome === "resolvedAbandoned" ? "resolved" : "blocked");
    });
  };
  return <section className="m5-maintenance-group" aria-busy={busy}>
    <h4>{t("m5.historyRecovery.title")}</h4>
    <p>{t("m5.historyRecovery.help")}</p>
    <button type="button" disabled={disabled || busy} onClick={() => void list()}>{t("m5.historyRecovery.open")}</button>
    {loaded && chats.length === 0 && <p>{t("m5.historyRecovery.empty")}</p>}
    <ul>{chats.map(chat => <li key={chat.chatId}><button type="button" disabled={disabled || busy} aria-current={detail?.chat.chatId === chat.chatId ? "true" : undefined} onClick={() => void read(chat)}>{chat.goal}</button></li>)}</ul>
    {cursor && <button type="button" disabled={disabled || busy} onClick={() => void list(cursor)}>{t("m5.historyRecovery.more")}</button>}
    {status && <p role="status">{t(`m5.historyRecovery.${status}`)}</p>}
    {detail && <section aria-label={t("m5.historyRecovery.title")}>
      <h4>{detail.chat.goal}</h4>
      {recovery?.state === "unresolved" && <div className="m5-notice">
        <p>{t("m5.chat.terminationHelp")}</p>
        {recovery.reason === "processIdentityUnavailable" ? <p>{t("m5.chat.identityUnavailable")}</p> : <button type="button" disabled={disabled || busy} onClick={resolve}>{t("m5.chat.discardAfterTermination")}</button>}
        <details><summary>{t("m5.feedback.details")}</summary><code>{recovery.runId}</code></details>
      </div>}
      <div className="m5-recovery-history-messages">{detail.messages.map(message => <article key={message.messageId}><small>{message.roleId ?? message.kind}</small><MessageBody text={message.text} /></article>)}</div>
      {detail.afterEventSequence > 0 && <button type="button" disabled={disabled || busy} onClick={() => void read(detail.chat)}>{t("m5.historyRecovery.first")}</button>}
      {detail.nextEventSequence !== null && <button type="button" disabled={disabled || busy} onClick={() => void read(detail.chat, detail.nextEventSequence!)}>{t("m5.historyRecovery.more")}</button>}
    </section>}
  </section>;
}
