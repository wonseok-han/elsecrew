import { useEffect, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult } from "./contracts/project-catalog-v1.ts";

/**
 * One button: the task's workspace, as it is now, is written over the user's
 * own folder (latest wins, user decision 2026-11-17). One confirmation names
 * what that means; there is no separate review, token or backup step.
 */
export function ApplyLatest({ projectId, chatId, changeCount, disabled, confirmRequest = 0, onApplied }: {
  projectId: string; chatId: string; changeCount: number; disabled: boolean;
  /** Bumped by "check changes and apply" elsewhere: opens the confirmation. */
  confirmRequest?: number;
  onApplied?: () => void;
}) {
  const { t } = useI18n();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ written: number; removed: number; skipped: number; failed: string | null } | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  useEffect(() => { setConfirming(false); setResult(null); setFailed(null); }, [projectId, chatId]);
  useEffect(() => { if (confirmRequest > 0 && changeCount > 0) setConfirming(true); }, [confirmRequest]);
  const apply = async () => {
    if (!window.m5Projects || busy) return;
    setBusy(true); setFailed(null); setResult(null);
    const request = { action: "applyChatLatest" as const, projectId, chatId };
    try {
      const reply = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (reply.action === "applyChatLatest") {
        setResult({ written: reply.written, removed: reply.removed, skipped: reply.skipped.length, failed: reply.failed });
        onApplied?.();
      }
      setConfirming(false);
    } catch (error) {
      setFailed(error instanceof Error ? error.message : String(error));
    } finally { setBusy(false); }
  };
  return <section className="m5-apply-latest" aria-label={t("m5.apply.title")}>
    <div className="m5-apply-latest-head">
      <strong>{t("m5.apply.count", { count: changeCount })}</strong>
      {!confirming && <button type="button" disabled={disabled || busy || changeCount === 0} onClick={() => { setConfirming(true); setResult(null); setFailed(null); }}>{t("m5.apply.action")}</button>}
    </div>
    {confirming && <div className="m5-apply-latest-confirm" role="alertdialog" aria-label={t("m5.apply.confirmTitle")}>
      <p>{t("m5.apply.confirm")}</p>
      <div className="m5-apply-latest-actions">
        <button type="button" disabled={disabled || busy} onClick={() => void apply()}>{busy ? t("m5.apply.applying") : t("m5.apply.confirmAction")}</button>
        <button type="button" className="m5-secondary" disabled={busy} onClick={() => setConfirming(false)}>{t("m5.editor.cancel")}</button>
      </div>
    </div>}
    {result && <p role={result.failed ? "alert" : "status"} className="m5-apply-latest-done">{t(result.failed ? "m5.apply.partial" : "m5.apply.done", { written: result.written, removed: result.removed, path: result.failed ?? "" })}{result.skipped ? ` ${t("m5.apply.skipped", { count: result.skipped })}` : ""}</p>}
    {failed && <div role="alert" className="m5-apply-latest-failed"><p>{t("m5.apply.failed")}</p><details><summary>{t("m5.chat.details")}</summary><pre>{failed}</pre></details></div>}
  </section>;
}
