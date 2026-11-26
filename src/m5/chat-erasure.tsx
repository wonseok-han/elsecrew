import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type ChatErasureEntry } from "./contracts/project-catalog-v1.ts";

/** Settings → storage: explicit, selected chat content erasure (docs/proposals/m5-chat-erasure.md).
 *  An item is shown as done only after the sidecar returns the durable `erased:true` receipt. */
export function ChatErasure({ disabled, loadKey, onBusy }: { disabled: boolean; loadKey: number; onBusy?: (busy: boolean) => void }) {
  const { t } = useI18n();
  const [entries, setEntries] = useState<ChatErasureEntry[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => { epoch.current++; setEntries([]); setSelected([]); setConfirm(false); setLoaded(false); setProgress(null); return () => { epoch.current++; }; }, [disabled]);
  useEffect(() => { if (loadKey > 0) void load(); }, [loadKey]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { onBusy?.(busy); }, [busy, onBusy]);
  async function load() {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setError(false); setConfirm(false);
    try {
      const request = { action: "chatErasureList" as const };
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation === epoch.current && result.action === request.action) { setEntries(result.entries); setSelected([]); setLoaded(true); setProgress(null); }
    } catch { if (generation === epoch.current) setError(true); }
    finally { pending.current = false; setBusy(false); }
  }
  async function erase() {
    if (disabled || pending.current || !confirm || !selected.length || !window.m5Projects) return;
    const generation = epoch.current;
    const queue = [...selected];
    pending.current = true; setBusy(true); setError(false); setProgress({ done: 0, total: queue.length });
    try {
      // One chat per request, in order; each request is atomic and resumable in the sidecar.
      for (const [index, chatId] of queue.entries()) {
        if (generation !== epoch.current) break;
        const request = { action: "eraseChatContent" as const, chatId, confirmed: true as const };
        validateProjectCatalogResult(await window.m5Projects.request(request), request);
        if (generation === epoch.current) {
          setEntries(items => items.filter(item => item.chatId !== chatId));
          setSelected(ids => ids.filter(id => id !== chatId));
          setProgress({ done: index + 1, total: queue.length });
        }
      }
    } catch { if (generation === epoch.current) setError(true); }
    finally { pending.current = false; setBusy(false); setConfirm(false); }
  }
  const status = (entry: ChatErasureEntry) => t(entry.state === "pending" ? "m5.erasure.pending" : entry.state === "redacted" ? "m5.erasure.redacted" : entry.eligible ? "m5.erasure.ready" : "m5.erasure.blocked");
  if (!loaded && !error) return null;
  return <div className="m5-cleanup-part m5-erasure-settings" aria-busy={busy}>
    <h4>{t("m5.erasure.title")}</h4><p className="m5-settings-intro">{t("m5.erasure.help")}</p>
    {loaded && !entries.length && <p role="status">{t("m5.erasure.empty")}</p>}
    {!!entries.length && <div className="m5-settings-group">{entries.map(entry => <label className="m5-preferences-row" key={entry.chatId}>
      <span><strong dir="auto">{entry.preview || t("m5.erasure.untitled")}</strong><small>{status(entry)}</small></span>
      <input type="checkbox" checked={selected.includes(entry.chatId)} disabled={disabled || busy || !entry.eligible} onChange={event => { setConfirm(false); setSelected(ids => event.target.checked ? [...ids, entry.chatId] : ids.filter(id => id !== entry.chatId)); }} />
    </label>)}</div>}
    {!!selected.length && !confirm && <button className="m5-danger" type="button" disabled={disabled || busy} onClick={() => setConfirm(true)}>{t("m5.erasure.selected", { count: selected.length })}</button>}
    {confirm && <div className="m5-cleanup-confirm" role="alertdialog" aria-labelledby="m5-erasure-confirm-title">
      <p id="m5-erasure-confirm-title"><strong>{t("m5.erasure.confirmTitle")}</strong></p>
      <ul><li>{t("m5.erasure.confirmErased")}</li><li>{t("m5.erasure.confirmKept")}</li><li>{t("m5.erasure.confirmOutside")}</li></ul>
      <div className="m5-storage-actions"><button type="button" disabled={busy} onClick={() => setConfirm(false)}>{t("m5.erasure.cancel")}</button><button className="m5-danger m5-btn-danger-solid" type="button" disabled={disabled || busy} onClick={() => void erase()}>{t("m5.erasure.erase")}</button></div>
    </div>}
    {progress && <p role="status">{t("m5.erasure.progress", { done: progress.done, total: progress.total })}</p>}
    {error && <p role="alert">{t("m5.erasure.error")}</p>}
  </div>;
}
