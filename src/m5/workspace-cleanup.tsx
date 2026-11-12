import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type WorkspaceCleanupEntry } from "./contracts/project-catalog-v1.ts";

export function WorkspaceCleanup({ disabled }: { disabled: boolean }) {
  const { t } = useI18n();
  const [entries, setEntries] = useState<WorkspaceCleanupEntry[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => { epoch.current++; setEntries([]); setSelected([]); setConfirm(false); setLoaded(false); return () => { epoch.current++; }; },[disabled]);
  async function load() {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setError(false); setConfirm(false);
    try {
      const request = {action:"workspaceCleanupList" as const};
      const result = validateProjectCatalogResult(await window.m5Projects.request(request),request);
      if (generation === epoch.current && result.action === request.action) { setEntries(result.entries); setSelected([]); setLoaded(true); }
    } catch { if (generation === epoch.current) setError(true); }
    finally { pending.current = false; setBusy(false); }
  }
  async function erase() {
    if (disabled || pending.current || !confirm || !selected.length || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setError(false);
    try {
      for (const chatId of selected) {
        if (generation !== epoch.current) break;
        const request = {action:"cleanupWorkspace" as const,chatId,confirmed:true as const};
        validateProjectCatalogResult(await window.m5Projects.request(request),request);
        if (generation === epoch.current) { setEntries(items => items.filter(item => item.chatId !== chatId)); setSelected(ids => ids.filter(id => id !== chatId)); }
      }
    } catch { if (generation === epoch.current) setError(true); }
    finally { pending.current = false; setBusy(false); setConfirm(false); }
  }
  return <section className="m5-cleanup-settings" aria-busy={busy}>
    <h3>{t("m5.cleanup.title")}</h3><p className="m5-settings-intro">{t("m5.cleanup.help")}</p>
    <button type="button" disabled={disabled || busy} onClick={() => void load()}>{t("m5.cleanup.inspect")}</button>
    {loaded && !entries.length && <p role="status">{t("m5.cleanup.empty")}</p>}
    {!!entries.length && <div className="m5-settings-group">{entries.map(entry => <label className="m5-preferences-row" key={entry.chatId}>
      <span><strong>{entry.title}</strong><small>{t(entry.pending ? "m5.cleanup.pending" : entry.eligible ? "m5.cleanup.ready" : "m5.cleanup.blocked")}</small></span>
      <input type="checkbox" checked={selected.includes(entry.chatId)} disabled={disabled || busy || !entry.eligible} onChange={event => { setConfirm(false); setSelected(ids => event.target.checked ? [...ids,entry.chatId] : ids.filter(id => id !== entry.chatId)); }} />
    </label>)}</div>}
    {!!selected.length && !confirm && <button className="m5-danger" type="button" disabled={disabled || busy} onClick={() => setConfirm(true)}>{t("m5.cleanup.selected",{count:selected.length})}</button>}
    {confirm && <div className="m5-cleanup-confirm"><p>{t("m5.cleanup.confirm")}</p><div className="m5-storage-actions"><button type="button" disabled={busy} onClick={() => setConfirm(false)}>{t("m5.cleanup.cancel")}</button><button className="m5-danger" type="button" disabled={disabled || busy} onClick={() => void erase()}>{t("m5.cleanup.erase")}</button></div></div>}
    {error && <p role="alert">{t("m5.cleanup.error")}</p>}
  </section>;
}
