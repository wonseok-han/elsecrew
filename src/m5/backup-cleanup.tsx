import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type BackupCleanupInspection } from "./contracts/project-catalog-v1.ts";

type Props = { projectId: string; reviewId: string; disabled: boolean };
export function BackupCleanup(props: Props) {
  return <CleanupSession key={JSON.stringify([props.projectId, props.reviewId])} {...props} />;
}

function CleanupSession({ projectId, reviewId, disabled }: Props) {
  const { t } = useI18n();
  const [inspection, setInspection] = useState<BackupCleanupInspection | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [agreed, setAgreed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"" | "error" | "unknown" | "done">("");
  const pending = useRef(false);
  const epoch = useRef(0);
  useEffect(() => {
    epoch.current++; setInspection(null); setEnabled(false); setAgreed(false); setMessage("");
    return () => { epoch.current++; };
  }, [disabled]);
  async function run(remove = false) {
    if (!window.m5Projects || disabled || pending.current || (remove && (!enabled || !agreed || !inspection || inspection.state === "deleted"))) return;
    const generation = epoch.current;
    const request = remove
      ? {action:"cleanupBackup" as const,projectId,reviewId,fingerprint:inspection!.fingerprint}
      : {action:"inspectBackupCleanup" as const,projectId,reviewId};
    pending.current = true; setBusy(true); setInspection(null); setEnabled(false); setAgreed(false); setMessage("");
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation !== epoch.current) return;
      if (result.action === "inspectBackupCleanup") { setInspection(result.inspection); setEnabled(result.enabled); }
      else if (result.action === "cleanupBackup") setMessage("done");
    } catch { if (generation === epoch.current) setMessage(remove ? "unknown" : "error"); }
    finally { pending.current = false; setBusy(false); }
  }
  return <section aria-label={t("m5.backup.cleanupTitle")} aria-busy={busy}>
    <p><code>{reviewId}</code></p>
    <button type="button" disabled={disabled || busy} onClick={() => void run()}>{t("m5.backup.cleanupInspect")}</button>
    {inspection && <>
      <p>{t(`m5.backup.cleanup.${inspection.state}`)}</p>
      <p>{t(inspection.filePresent ? "m5.backup.cleanup.present" : "m5.backup.cleanup.absent")}</p>
      {!enabled && inspection.state !== "deleted" && <p>{t("m5.backup.cleanup.disabled")}</p>}
      {enabled && inspection.state !== "deleted" && <>
        <label><input type="checkbox" checked={agreed} disabled={disabled || busy} onChange={event => setAgreed(event.target.checked)} />{t("m5.backup.cleanupConsent")}</label>
        <button type="button" disabled={disabled || busy || !agreed} onClick={() => void run(true)}>{t("m5.backup.cleanupConfirm")}</button>
      </>}
    </>}
    {message && <p role="status">{t(`m5.backup.cleanup.${message}`)}</p>}
  </section>;
}
