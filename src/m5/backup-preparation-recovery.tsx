import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type BackupPreparation } from "./contracts/project-catalog-v1.ts";

/** Lists unfinished backup preparations on its own and shows only what it finds. */
export function BackupPreparationRecovery({ projectId, disabled, onFound, onUnverified, onChecked }: { projectId: string; disabled: boolean; onFound?: (found: boolean) => void; onUnverified?: (failed: boolean) => void; onChecked?: () => void }) {
  const { t } = useI18n();
  const [entries, setEntries] = useState<BackupPreparation[] | null>(null);
  const [selected, setSelected] = useState<BackupPreparation | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"" | "done" | "error">("");
  const epoch = useRef(0);
  const pending = useRef(false);
  const loaded = useRef(false);
  // Look once per mount; the parent remounts this view to check again. A busy
  // parent only disables the buttons and must not discard the answer.
  useEffect(() => () => { epoch.current++; loaded.current = false; pending.current = false; }, []);
  useEffect(() => {
    if (!disabled && !loaded.current) { loaded.current = true; void run(); }
  }, [disabled]);
  // A failed look is not "nothing found": the parent says it could not check.
  useEffect(() => { onFound?.(Boolean(entries && entries.length > 0)); onUnverified?.(message === "error"); }, [entries, message]);
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
    finally {
      pending.current = false; setBusy(false);
      // The first look has an answer (or failed): the parent may sum up now.
      if (!entry && epoch.current === generation) onChecked?.();
    }
  }
  if (!message && !selected && (!entries || entries.length === 0)) return null;
  return <section className="m5-recovery-item" aria-label={t("m5.backup.title")} aria-busy={busy}>
    <h5>{t("m5.backup.title")}</h5>
    {entries && entries.length > 0 && <ul>{entries.map(entry => <li key={entry.reviewId}>
      <code>{entry.reviewId}</code> <button type="button" disabled={disabled || busy} onClick={() => setSelected(entry)}>{t("m5.backup.finish")}</button>
    </li>)}</ul>}
    {selected && <div role="group" aria-label={t("m5.backup.finish")}>
      <p>{t("m5.backup.help")}</p><code>{selected.reviewId}</code>
      <button type="button" disabled={disabled || busy} onClick={() => void run(selected)}>{t("m5.backup.confirm")}</button>
      <button type="button" disabled={busy} onClick={() => setSelected(null)}>{t("m5.prep.back")}</button>
    </div>}
    {message && <p role="status">{t(`m5.backup.${message}`)}</p>}
  </section>;
}
