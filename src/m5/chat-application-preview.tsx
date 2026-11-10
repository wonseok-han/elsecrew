import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { ApplicationStartControl } from "./application-start-control.tsx";
import { RestorationPreview } from "./restoration-preview.tsx";
import { validateProjectCatalogResult, type ApplicationPreviewPage, type ApplicationStatus } from "./contracts/project-catalog-v1.ts";

type PreviewProps = { projectId: string; chatId: string; worktreeId: string; disabled: boolean; onReviewDiff?: () => void };
export function ChatApplicationPreview(props: PreviewProps) {
  return <ChatApplicationPreviewSession key={JSON.stringify([props.projectId, props.chatId, props.worktreeId])} {...props} />;
}

function ChatApplicationPreviewSession({ projectId, chatId, worktreeId, disabled, onReviewDiff }: PreviewProps) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<ApplicationPreviewPage | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [confirmation, setConfirmation] = useState<string | null>(null);
  const [started, setStarted] = useState(false);
  const [application, setApplication] = useState<ApplicationStatus | null>(null);
  const [restorationOffered, setRestorationOffered] = useState(false);
  const startedRef = useRef(false);
  const generation = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    generation.current++; setPreview(null); setError(false); setConfirmation(null);
    startedRef.current = false; setStarted(false);
    setApplication(null);
    setRestorationOffered(false);
    return () => { generation.current++; };
  }, [projectId, chatId, worktreeId]);
  useEffect(() => {
    if (disabled && !startedRef.current) {
      generation.current++; setPreview(null); setConfirmation(null);
    }
  }, [disabled]);
  useEffect(() => {
    if (!confirmation) return;
    // Expiry limits admission, not the lifetime of an already submitted operation.
    const timer = setTimeout(() => { if (!startedRef.current) setConfirmation(null); }, 300_000);
    return () => clearTimeout(timer);
  }, [confirmation]);
  useEffect(() => {
    if (disabled || !window.m5Projects) return;
    let current = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const request = { action: "chatApplicationStatus" as const, projectId, chatId };
    const refresh = () => { void window.m5Projects!.request(request).then(raw => {
      const result = validateProjectCatalogResult(raw, request);
      if (!current || startedRef.current || result.action !== "chatApplicationStatus") return;
      setApplication(result.status);
      setRestorationOffered(!result.status.running && ["prepared", "applying", "applied"].includes(result.status.state));
      if (result.status.running) timer = setTimeout(refresh, 1000);
    }).catch(() => { if (current && !startedRef.current) setError(true); }); };
    refresh();
    return () => { current = false; clearTimeout(timer); };
  }, [projectId, chatId, worktreeId, disabled]);
  async function confirmReview(cancel = false) {
    if (disabled || startedRef.current || pending.current || !window.m5Projects || (!cancel && !preview) || (cancel && !confirmation)) return;
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
    if (disabled || startedRef.current || pending.current || !window.m5Projects) return;
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
  return <section className="m5-chat-review" aria-label={t("m5.application.chatPreview")} aria-busy={busy}>
    <header><h3>{t("m5.application.reviewTitle")}</h3>{onReviewDiff && <button type="button" className="m5-secondary" disabled={disabled || busy || started} onClick={onReviewDiff}>{t("m5.application.reviewDiff")}</button>}</header>
    <p className="m5-review-help">{t("m5.application.reviewFlow")}</p>
    {disabled && <p role="status" className="m5-review-help">{t("m5.application.reviewBlocked")}</p>}
    <button type="button" disabled={disabled || busy || started} onClick={() => void inspect()}>{t("m5.application.chatPreview")}</button>
    {error && <p role="status">{t("m5.application.previewError")}</p>}
    {preview && <>
      <p>{t("m5.application.previewHelp")}</p>
      <details className="m5-review-identity"><summary>{t("m5.application.workspace")}</summary><code>{preview.worktreeId}</code></details>
      <p className="m5-review-count">{t("m5.application.total")} <strong>{preview.total}</strong></p>
      {preview.total === 0 && <p>{t("m5.application.noChanges")}</p>}
      <ul className="m5-review-files">{preview.changes.map(change => <li key={change.path}><span className={`m5-review-kind m5-kind-${change.kind}`}>{t(`m5.application.${change.kind}`)}</span><code>{change.path.replace(/[\p{Cc}\p{Cf}]/gu, character => `\\u{${character.codePointAt(0)!.toString(16)}}`)}</code></li>)}</ul>
      {preview.total > 100 && <nav className="m5-review-pagination">
      <p>{preview.total === 0 ? 0 : preview.offset + 1}–{preview.offset + preview.changes.length} / {preview.total}</p>
      <button type="button" disabled={disabled || busy || started || preview.offset === 0} onClick={() => void inspect(Math.max(0, preview.offset - 100), preview.fingerprint)}>{t("m5.application.previousPage")}</button>
      <button type="button" disabled={disabled || busy || started || preview.offset + preview.changes.length >= preview.total} onClick={() => void inspect(preview.offset + 100, preview.fingerprint)}>{t("m5.application.nextPage")}</button>
      </nav>}
      <button type="button" disabled={disabled || busy || preview.total === 0 || confirmation !== null} onClick={() => void confirmReview()}>{t("m5.application.openConfirmation")}</button>
    </>}
    {confirmation && !started && <div role="status"><p>{t("m5.application.confirmationReady")}</p><button type="button" disabled={disabled || busy} onClick={() => void confirmReview(true)}>{t("m5.application.cancelConfirmation")}</button></div>}
    {confirmation && application?.state !== "restored" && <ApplicationStartControl key={confirmation} request={{action:"startApplication",projectId,chatId,confirmationId:confirmation}} disabled={disabled || busy} onAttempt={() => { startedRef.current = true; setStarted(true); }} onPreparationCanceled={() => {
      setConfirmation(null); setPreview(null); startedRef.current = false; setStarted(false);
    }} onStatus={status => {
      setApplication(status);
      if (!status.running && status.applicationId && ["prepared", "applying", "applied"].includes(status.state)) setRestorationOffered(true);
    }} />}
    {application?.state === "restored" && <p role="status">{t("m5.application.restored")}</p>}
    {restorationOffered && application?.applicationId && ["prepared", "applying", "applied"].includes(application.state) && <RestorationPreview projectId={projectId} applicationId={application.applicationId} disabled={disabled || busy} onStatus={status => setApplication(current => current?.applicationId === status.applicationId ? status : current)} />}
  </section>;
}
