import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { validateProjectCatalogResult, type ChatDiscardInspection } from "./contracts/project-catalog-v1.ts";

/** The host must prevent unsaved editor loss and clear its view after success. */
export function ChatWorkspaceDiscard({ chat, disabled, onCompleted }: {
  chat: ChatRecordV1; disabled: boolean; onCompleted: (chat: ChatRecordV1) => Promise<void>;
}) {
  const { t } = useI18n();
  const dialog = useRef<HTMLDialogElement>(null);
  const generation = useRef(0);
  const pending = useRef(false);
  const committed = useRef<ChatRecordV1 | null>(null);
  const [inspection, setInspection] = useState<ChatDiscardInspection | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [error, setError] = useState(false);
  useEffect(() => () => { generation.current++; }, []);
  function close() {
    if (pending.current) return;
    generation.current++;
    dialog.current?.close();
    setInspection(null); setConfirmed(false); setError(false);
  }
  async function inspect() {
    if (disabled || pending.current || chat.state !== "reviewReady" || !window.m5Projects) return;
    if (!dialog.current?.open) dialog.current?.showModal();
    const epoch = ++generation.current;
    pending.current = true; setBusy(true); setError(false); setConfirmed(false); setInspection(null);
    const request = { action: "inspectChatDiscard" as const, chatId: chat.chatId, taskId: chat.taskId,
      projectId: chat.projectId, worktreeId: chat.worktreeId, workspaceIdentity: chat.workspaceIdentity };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (epoch === generation.current && result.action === "inspectChatDiscard") setInspection(result.inspection);
    } catch { if (epoch === generation.current) setError(true); }
    finally { if (epoch === generation.current) { pending.current = false; setBusy(false); } }
  }
  async function discard() {
    if (disabled || pending.current || !confirmed || !inspection || !window.m5Projects) return;
    const epoch = generation.current;
    pending.current = true; setBusy(true); setError(false);
    const request = { action: "discardChatWorkspace" as const, inspection, confirmed: true as const };
    try {
      if (!committed.current) {
        const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
        if (epoch !== generation.current || result.action !== "discardChatWorkspace") return;
        committed.current = result.chat;
      }
      await onCompleted(committed.current);
      dialog.current?.close();
    } catch { if (epoch === generation.current) setError(true); }
    finally { if (epoch === generation.current) { pending.current = false; setBusy(false); } }
  }
  return <section className="m5-chat-discard">
    <button type="button" disabled={disabled || busy || chat.state !== "reviewReady"} onClick={() => void inspect()}>{t("m5.discard.open")}</button>
    <dialog ref={dialog} className="m5-discard-dialog" aria-label={t("m5.discard.title")} onCancel={event => { event.preventDefault(); close(); }}>
      <h2>{t("m5.discard.title")}</h2>
      <p><strong>{chat.goal}</strong></p>
      <p>{t("m5.discard.explanation")}</p>
      {busy && <p role="status">{t("m5.discard.busy")}</p>}
      {error && <p role="alert">{t("m5.discard.error")}</p>}
      {inspection && <label><input type="checkbox" checked={confirmed} disabled={busy || disabled} onChange={event => setConfirmed(event.target.checked)} />{t("m5.discard.consent")}</label>}
      <div className="m5-discard-actions">
        <button type="button" disabled={busy} onClick={close}>{t("m5.discard.cancel")}</button>
        {!inspection && error && <button type="button" disabled={busy || disabled} onClick={() => void inspect()}>{t("m5.discard.retry")}</button>}
        {inspection && <button type="button" disabled={busy || disabled || !confirmed} onClick={() => void discard()}>{t(error ? "m5.discard.retry" : "m5.discard.open")}</button>}
      </div>
    </dialog>
  </section>;
}
