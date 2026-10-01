import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type BackupStorageUsage } from "./contracts/project-catalog-v1.ts";

export function BackupStorageUsageView({ disabled }: { disabled: boolean }) {
  const { t } = useI18n();
  const [usage, setUsage] = useState<BackupStorageUsage | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [limit, setLimit] = useState("5");
  const [agreed, setAgreed] = useState(false);
  const [changeUnknown, setChangeUnknown] = useState(false);
  const pending = useRef(false);
  const epoch = useRef(0);
  useEffect(() => {
    epoch.current++; setUsage(null); setFailed(false); setAgreed(false); setChangeUnknown(false);
    return () => { epoch.current++; };
  }, [disabled]);
  async function inspect() {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setUsage(null); setFailed(false);
    const request = { action: "backupStorageUsage" as const };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation === epoch.current && result.action === "backupStorageUsage") {
        setUsage(result.usage); setLimit(String(result.usage.limitBytes / 1024 ** 3)); setAgreed(false); setChangeUnknown(false);
      }
    } catch { if (generation === epoch.current) setFailed(true); }
    finally { pending.current = false; setBusy(false); }
  }
  async function changeLimit() {
    if (disabled || pending.current || !window.m5Projects || !usage || !agreed || !/^\d{1,4}$/.test(limit)) return;
    const amount = Number(limit);
    if (amount < 1 || amount > 1024) return;
    const generation = epoch.current;
    const request = {action:"setBackupStorageLimit" as const,expectedLimitBytes:usage.limitBytes,limitBytes:amount * 1024 ** 3};
    pending.current = true; setBusy(true); setUsage(null); setAgreed(false); setChangeUnknown(false);
    try {
      validateProjectCatalogResult(await window.m5Projects.request(request), request);
    } catch { if (generation === epoch.current) setChangeUnknown(true); }
    finally { pending.current = false; setBusy(false); }
    // No automatic replay after success or failure; inspect the persisted value.
  }
  return <section aria-label={t("m5.backup.storageTitle")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => void inspect()}>{t("m5.backup.storageTitle")}</button>
    {usage && <div role="status">
      <p>{t("m5.backup.storageAmounts", { used: usage.usedBytes.toLocaleString(), limit: usage.limitBytes.toLocaleString(), remaining: usage.remainingBytes.toLocaleString() })}</p>
      <p>{t("m5.backup.storageHelp")}</p>
      {usage.remainingBytes === 0 && <p>{t("m5.backup.storageFull")}</p>}
      <label>{t("m5.backup.limitLabel")} <input type="number" min="1" max="1024" step="1" value={limit} disabled={disabled || busy} onChange={event => { setLimit(event.target.value); setAgreed(false); }} /></label>
      <label><input type="checkbox" checked={agreed} disabled={disabled || busy} onChange={event => setAgreed(event.target.checked)} />{t("m5.backup.limitConsent")}</label>
      <button type="button" disabled={disabled || busy || !agreed || !/^\d{1,4}$/.test(limit) || Number(limit) < 1 || Number(limit) > 1024} onClick={() => void changeLimit()}>{t("m5.backup.limitChange")}</button>
    </div>}
    {!usage && !busy && agreed === false && <p>{t("m5.backup.limitInspect")}</p>}
    {changeUnknown && <p role="status">{t("m5.backup.limitUnknown")}</p>}
    {failed && <p role="status">{t("m5.backup.storageError")}</p>}
  </section>;
}
