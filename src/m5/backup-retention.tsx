import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { BackupCleanup } from "./backup-cleanup.tsx";
import { validateProjectCatalogResult, type BackupRetention } from "./contracts/project-catalog-v1.ts";

export function BackupRetentionView({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [entries, setEntries] = useState<BackupRetention[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => { epoch.current++; setEntries(null); setSelected(null); setFailed(false); return () => { epoch.current++; }; }, [projectId, disabled]);
  async function inspect() {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setEntries(null); setSelected(null); setFailed(false);
    const request = {action:"backupRetention" as const,projectId};
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation === epoch.current && result.action === "backupRetention") setEntries(result.entries);
    } catch { if (generation === epoch.current) setFailed(true); }
    finally { pending.current = false; setBusy(false); }
  }
  return <section aria-label={t("m5.backup.retentionTitle")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => void inspect()}>{t("m5.backup.retentionTitle")}</button>
    {entries && <><p>{t("m5.backup.retentionHelp")}</p>{entries.length === 0 ? <p>{t("m5.backup.retentionEmpty")}</p> : <ul>{entries.map(entry => <li key={entry.reviewId}><code>{entry.reviewId}</code> — {t(`m5.backup.${entry.retainedFor}`)}
      {entry.retainedFor === "abandonedPreparation" && <button type="button" disabled={disabled || busy} onClick={() => setSelected(entry.reviewId)}>{t("m5.backup.cleanupTitle")}</button>}
    </li>)}</ul>}</>}
    {selected && <BackupCleanup projectId={projectId} reviewId={selected} disabled={disabled || busy} />}
    {failed && <p role="status">{t("m5.backup.error")}</p>}
  </section>;
}
