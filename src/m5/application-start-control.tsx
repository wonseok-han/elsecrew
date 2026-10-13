import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { ApplicationFailure } from "./application-failure.tsx";
import { reconcileApplicationStatus } from "./application-status-reconcile.ts";
import { validateProjectCatalogResult, type ProjectCatalogRequest, type ApplicationStatus } from "./contracts/project-catalog-v1.ts";

type StartRequest = Extract<ProjectCatalogRequest, { action: "startApplication" | "restoreApplication" }>;
type StartProps = { request: StartRequest; disabled: boolean; onAttempt?: () => void; onStatus?: (status: ApplicationStatus) => void; onPreparationCanceled?: () => void };

/** Start once per reviewed target. A lost response requires status inspection, not replay. */
export function ApplicationStartControl(props: StartProps) {
  const request = props.request;
  const key = JSON.stringify(request.action === "startApplication"
    ? [request.action, request.projectId, request.chatId, request.confirmationId]
    : [request.action, request.projectId, request.applicationId, request.fingerprint]);
  return <ApplicationStartSession key={key} {...props} />;
}

function ApplicationStartSession({ request, disabled, onAttempt, onStatus, onPreparationCanceled }: StartProps) {
  const { t } = useI18n();
  const [enabled, setEnabled] = useState(false);
  const [agreed, setAgreed] = useState(false);
  const [attempted, setAttempted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [unknown, setUnknown] = useState(false);
  const [preflightFailed, setPreflightFailed] = useState(false);
  const [cancelPending, setCancelPending] = useState(false);
  const [status, setStatus] = useState<ApplicationStatus | null>(null);
  const pending = useRef(false);
  const expectedId = useRef<string | null>(request.action === "restoreApplication" ? request.applicationId : null);
  const previousId = useRef<string | null | undefined>(undefined);
  const stopRequested = useRef(false);
  const stopConfirmed = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    let current = true;
    const query = { action: "applicationCapabilities" as const };
    if (window.m5Projects) void window.m5Projects.request(query).then(raw => {
      const result = validateProjectCatalogResult(raw, query);
      if (current && result.action === "applicationCapabilities") setEnabled(result.enabled);
    }).catch(() => { if (current) setEnabled(false); });
    return () => { current = false; alive.current = false; };
  }, []);

  async function refresh() {
    if (!window.m5Projects || pending.current || disabled) return;
    pending.current = true;
    const query = request.action === "startApplication"
      ? { action: "chatApplicationStatus" as const, projectId: request.projectId, chatId: request.chatId }
      : { action: "applicationStatus" as const, projectId: request.projectId };
    try {
      const raw = await window.m5Projects.request(query);
      const inspected = validateProjectCatalogResult(raw, query);
      // Both requests have settled (pending guards this read). A confirmed
      // preparation cancellation with no new intent may return to fresh review.
      if (alive.current && request.action === "startApplication" && stopConfirmed.current &&
          !expectedId.current && previousId.current !== undefined &&
          inspected.action === "chatApplicationStatus" &&
          inspected.status.applicationId === previousId.current && !inspected.status.running) {
        onPreparationCanceled?.();
        return;
      }
      const next = reconcileApplicationStatus(raw, query, expectedId.current, previousId.current);
      if (alive.current) {
        expectedId.current = next.applicationId;
        setStatus(next); setUnknown(false);
        onStatus?.(next);
      }
    } catch { if (alive.current) { setStatus(null); setUnknown(true); } }
    finally { pending.current = false; }
  }
  useEffect(() => {
    if (!status?.running || disabled) return;
    const timer = window.setTimeout(() => { void refresh(); }, 1000);
    return () => window.clearTimeout(timer);
  }, [status, disabled]);

  async function submit(stop = false) {
    if (!window.m5Projects || pending.current || disabled || (!stop && (!enabled || !agreed || attempted))) return;
    if (stop && !status?.applicationId) return;
    const command = stop ? { action: "cancelApplication" as const, projectId: request.projectId, applicationId: status!.applicationId! } : request;
    pending.current = true; setBusy(true); setUnknown(false); setPreflightFailed(false);
    let commandSent = false;
    try {
      if (!stop && request.action === "startApplication") {
        // Remember any older record before starting. A lost response may only
        // adopt a new record from this exact chat, never another chat or old run.
        const query = { action: "chatApplicationStatus" as const, projectId: request.projectId, chatId: request.chatId };
        const before = validateProjectCatalogResult(await window.m5Projects.request(query), query);
        if (before.action !== "chatApplicationStatus" || !alive.current) return;
        previousId.current = before.status.applicationId;
        if (stopRequested.current) return;
      }
      if (!stop) { setAttempted(true); onAttempt?.(); }
      commandSent = true;
      const result = validateProjectCatalogResult(await window.m5Projects.request(command), command);
      if (alive.current && result.action === "startApplication") expectedId.current = result.applicationId;
    }
    catch { if (alive.current) { if (commandSent) setUnknown(true); else setPreflightFailed(true); } }
    finally { pending.current = false; if (alive.current) { setBusy(false); if (commandSent || stopConfirmed.current) void refresh(); } }
  }
  async function stopPreparation() {
    if (!window.m5Projects || request.action !== "startApplication" || cancelPending) return;
    stopRequested.current = true;
    setCancelPending(true);
    const command = {action:"cancelApplicationConfirmation" as const,confirmationId:request.confirmationId};
    try {
      validateProjectCatalogResult(await window.m5Projects.request(command), command);
      stopConfirmed.current = true;
      if (alive.current) void refresh();
    }
    catch { if (alive.current) setUnknown(true); }
    finally { if (alive.current) setCancelPending(false); }
  }
  return <div aria-busy={busy}>
    {!enabled && <p>{t("m5.application.notEnabled")}</p>}
    {enabled && !attempted && <>
      <label><input type="checkbox" checked={agreed} disabled={disabled || busy} onChange={event => setAgreed(event.target.checked)} />{t("m5.application.sourceConsent")}</label>
      <button type="button" disabled={disabled || busy || !agreed} onClick={() => void submit()}>{t(request.action === "startApplication" ? "m5.application.start" : "m5.restore.start")}</button>
    </>}
    {unknown && <p role="status">{t("m5.application.startUnknown")}</p>}
    {preflightFailed && <p role="status">{t("m5.application.previewError")}</p>}
    {busy && request.action === "startApplication" && <button type="button" disabled={cancelPending} onClick={() => void stopPreparation()}>{t("m5.application.stopPreparation")}</button>}
    {attempted && <button type="button" disabled={disabled || busy} onClick={() => void refresh()}>{t("m5.application.inspect")}</button>}
    {status && <div role="status">
      <p>{t(status.running ? "m5.application.running" : status.state === "prepared" || status.state === "applying" ? "m5.application.interrupted" : `m5.application.${status.state}`)}</p>
      {status.currentIndex !== null && <p>{t("m5.application.item")} {status.currentIndex + 1} / {status.total}</p>}
    </div>}
    {status?.running && <button type="button" disabled={disabled || busy} onClick={() => void submit(true)}>{t("m5.application.stop")}</button>}
    {status && <ApplicationFailure failure={status.failure} />}
  </div>;
}
