import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type ApplicationPreviewPage } from "./contracts/project-catalog-v1.ts";

export function ChatApplicationPreview({ projectId, chatId, worktreeId, disabled }: { projectId: string; chatId: string; worktreeId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<ApplicationPreviewPage | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [confirmation, setConfirmation] = useState<string | null>(null);
  const generation = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    generation.current++; setPreview(null); setError(false); setConfirmation(null);
    return () => { generation.current++; };
  }, [projectId, chatId, worktreeId, disabled]);
  useEffect(() => {
    if (!confirmation) return;
    const timer = setTimeout(() => setConfirmation(null), 300_000);
    return () => clearTimeout(timer);
  }, [confirmation]);
  async function confirmReview(cancel = false) {
    if (disabled || pending.current || !window.m5Projects || (!cancel && !preview) || (cancel && !confirmation)) return;
    const epoch = generation.current;
    pending.current = true; setBusy(true); setError(false);
    const request = cancel
      ? { action: "cancelApplicationConfirmation" as const, confirmationId: confirmation! }
      : { action: "openApplicationConfirmation" as const, projectId, chatId, fingerprint: preview!.fingerprint };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation.current !== epoch) return;
      if (result.action === "openApplicationConfirmation") {
        if (result.worktreeId !== worktreeId) throw new Error("confirmation workspace mismatch");
        setConfirmation(result.confirmationId);
      } else setConfirmation(null);
    } catch { if (generation.current === epoch) { setError(true); setConfirmation(null); } }
    finally { pending.current = false; setBusy(false); }
  }
  async function inspect(offset = 0, fingerprint: string | null = null) {
    if (disabled || pending.current || !window.m5Projects) return;
    const epoch = generation.current;
    pending.current = true; setBusy(true); setError(false); setPreview(null); setConfirmation(null);
    const request = { action: "applicationPreviewPage" as const, projectId, chatId, offset, fingerprint };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation.current !== epoch) return;
      if (result.action !== "applicationPreviewPage" || result.page.worktreeId !== worktreeId) throw new Error("selected chat workspace changed");
      setPreview(result.page);
    } catch { if (generation.current === epoch) setError(true); }
    finally { pending.current = false; setBusy(false); }
  }
  return <section aria-label={t("m5.application.chatPreview")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => void inspect()}>{t("m5.application.chatPreview")}</button>
    {error && <p role="status">{t("m5.application.previewError")}</p>}
    {preview && <>
      <p>{t("m5.application.previewHelp")}</p>
      <p>{t("m5.application.workspace")} <code>{preview.worktreeId}</code></p>
      <p>{t("m5.application.total")} {preview.total}</p>
      {preview.total === 0 && <p>{t("m5.application.noChanges")}</p>}
      <p>{preview.total === 0 ? 0 : preview.offset + 1}–{preview.offset + preview.changes.length} / {preview.total}</p>
      <button type="button" disabled={disabled || busy || preview.offset === 0} onClick={() => void inspect(Math.max(0, preview.offset - 100), preview.fingerprint)}>{t("m5.application.previousPage")}</button>
      <button type="button" disabled={disabled || busy || preview.offset + preview.changes.length >= preview.total} onClick={() => void inspect(preview.offset + 100, preview.fingerprint)}>{t("m5.application.nextPage")}</button>
      <ul>{preview.changes.map(change => <li key={change.path}>{t(`m5.application.${change.kind}`)} <code>{change.path.replace(/[\p{Cc}\p{Cf}]/gu, character => `\\u{${character.codePointAt(0)!.toString(16)}}`)}</code></li>)}</ul>
      <button type="button" disabled={disabled || busy || preview.total === 0 || confirmation !== null} onClick={() => void confirmReview()}>{t("m5.application.openConfirmation")}</button>
    </>}
    {confirmation && <div role="status"><p>{t("m5.application.confirmationReady")}</p><button type="button" disabled={disabled || busy} onClick={() => void confirmReview(true)}>{t("m5.application.cancelConfirmation")}</button></div>}
  </section>;
}
