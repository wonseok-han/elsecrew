import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type BackupPreparation } from "./contracts/project-catalog-v1.ts";

export function BackupPreparationRecovery({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [entries, setEntries] = useState<BackupPreparation[] | null>(null);
  const [selected, setSelected] = useState<BackupPreparation | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"" | "done" | "error">("");
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    epoch.current++; setEntries(null); setSelected(null); setMessage("");
    return () => { epoch.current++; };
  }, [projectId, disabled]);
  async function run(entry?: BackupPreparation) {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    const request = entry ? {action:"abandonBackupPreparation" as const,projectId,...entry} : {action:"backupPreparations" as const,projectId};
    pending.current = true; setBusy(true); setMessage(""); setSelected(null); setEntries(null);
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (epoch.current !== generation) return;
      if (result.action === "backupPreparations") setEntries(result.entries);
      else if (result.action === "abandonBackupPreparation") setMessage("done");
    } catch { if (epoch.current === generation) setMessage("error"); }
    finally { pending.current = false; setBusy(false); }
  }
  return <section aria-label={t("m5.backup.title")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => void run()}>{t("m5.backup.title")}</button>
    {entries && (entries.length === 0 ? <p role="status">{t("m5.backup.empty")}</p> : <ul>{entries.map(entry => <li key={entry.reviewId}>
      <code>{entry.reviewId}</code> <button type="button" disabled={disabled || busy} onClick={() => setSelected(entry)}>{t("m5.backup.finish")}</button>
    </li>)}</ul>)}
    {selected && <div role="group" aria-label={t("m5.backup.finish")}>
      <p>{t("m5.backup.help")}</p><code>{selected.reviewId}</code>
      <button type="button" disabled={disabled || busy} onClick={() => void run(selected)}>{t("m5.backup.confirm")}</button>
      <button type="button" disabled={busy} onClick={() => setSelected(null)}>{t("m5.prep.back")}</button>
    </div>}
    {message && <p role="status">{t(`m5.backup.${message}`)}</p>}
  </section>;
}
