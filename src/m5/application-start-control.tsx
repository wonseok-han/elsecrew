import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type ProjectCatalogRequest, type ApplicationStatus } from "./contracts/project-catalog-v1.ts";

type StartRequest = Extract<ProjectCatalogRequest, { action: "startApplication" | "restoreApplication" }>;

/** Start once per reviewed target. A lost response requires status inspection, not replay. */
export function ApplicationStartControl({ request, disabled }: { request: StartRequest; disabled: boolean }) {
  const { t } = useI18n();
  const [enabled, setEnabled] = useState(false);
  const [agreed, setAgreed] = useState(false);
  const [attempted, setAttempted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [unknown, setUnknown] = useState(false);
  const [cancelPending, setCancelPending] = useState(false);
  const [status, setStatus] = useState<ApplicationStatus | null>(null);
  const pending = useRef(false);
  const expectedId = useRef<string | null>(request.action === "restoreApplication" ? request.applicationId : null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    const query = { action: "applicationCapabilities" as const };
    if (window.m5Projects) void window.m5Projects.request(query).then(raw => {
      const result = validateProjectCatalogResult(raw, query);
      if (alive.current && result.action === "applicationCapabilities") setEnabled(result.enabled);
    }).catch(() => { if (alive.current) setEnabled(false); });
    return () => { alive.current = false; };
  }, []);

  async function refresh() {
    if (!window.m5Projects || pending.current || disabled) return;
    pending.current = true;
    const query = { action: "applicationStatus" as const, projectId: request.projectId };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(query), query);
      if (alive.current && result.action === "applicationStatus") {
        if (!expectedId.current || result.status.applicationId !== expectedId.current) { setStatus(null); setUnknown(true); return; }
        setStatus(result.status); setUnknown(false);
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
    pending.current = true; setBusy(true); setUnknown(false);
    if (!stop) setAttempted(true);
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(command), command);
      if (result.action === "startApplication") expectedId.current = result.applicationId;
    }
    catch { if (alive.current) setUnknown(true); }
    finally { pending.current = false; if (alive.current) { setBusy(false); void refresh(); } }
  }
  async function stopPreparation() {
    if (!window.m5Projects || request.action !== "startApplication" || cancelPending) return;
    setCancelPending(true);
    const command = {action:"cancelApplicationConfirmation" as const,confirmationId:request.confirmationId};
    try { validateProjectCatalogResult(await window.m5Projects.request(command), command); }
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
    {busy && request.action === "startApplication" && <button type="button" disabled={cancelPending} onClick={() => void stopPreparation()}>{t("m5.application.stopPreparation")}</button>}
    {attempted && <button type="button" disabled={disabled || busy} onClick={() => void refresh()}>{t("m5.application.inspect")}</button>}
    {status && <div role="status">
      <p>{t(status.running ? "m5.application.running" : status.state === "prepared" || status.state === "applying" ? "m5.application.interrupted" : `m5.application.${status.state}`)}</p>
      {status.currentIndex !== null && <p>{t("m5.application.item")} {status.currentIndex + 1} / {status.total}</p>}
    </div>}
    {status?.running && <button type="button" disabled={disabled || busy} onClick={() => void submit(true)}>{t("m5.application.stop")}</button>}
  </div>;
}
