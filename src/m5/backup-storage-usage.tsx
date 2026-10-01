import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type BackupStorageUsage } from "./contracts/project-catalog-v1.ts";

export function BackupStorageUsageView({ disabled }: { disabled: boolean }) {
  const { t } = useI18n();
  const [usage, setUsage] = useState<BackupStorageUsage | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const pending = useRef(false);
  const epoch = useRef(0);
  useEffect(() => {
    epoch.current++; setUsage(null); setFailed(false);
    return () => { epoch.current++; };
  }, [disabled]);
  async function inspect() {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setUsage(null); setFailed(false);
    const request = { action: "backupStorageUsage" as const };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation === epoch.current && result.action === "backupStorageUsage") setUsage(result.usage);
    } catch { if (generation === epoch.current) setFailed(true); }
    finally { pending.current = false; setBusy(false); }
  }
  return <section aria-label={t("m5.backup.storageTitle")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => void inspect()}>{t("m5.backup.storageTitle")}</button>
    {usage && <div role="status">
      <p>{t("m5.backup.storageAmounts", { used: usage.usedBytes.toLocaleString(), limit: usage.limitBytes.toLocaleString(), remaining: usage.remainingBytes.toLocaleString() })}</p>
      <p>{t("m5.backup.storageHelp")}</p>
      {usage.remainingBytes === 0 && <p>{t("m5.backup.storageFull")}</p>}
    </div>}
    {failed && <p role="status">{t("m5.backup.storageError")}</p>}
  </section>;
}
