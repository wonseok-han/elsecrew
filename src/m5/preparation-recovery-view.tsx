import { useI18n } from "../i18n.tsx";
import { RestorationPreview } from "./restoration-preview.tsx";
import { BackupPreparationRecovery } from "./backup-preparation-recovery.tsx";
import { ApplicationFailure } from "./application-failure.tsx";
import { BackupStorageUsageView } from "./backup-storage-usage.tsx";
import { useEffect, useRef, useState } from "react";
import { validateProjectCatalogResult, type ApplicationPreview, type ApplicationHistoryEntry, type ApplicationStatus, type PreparationInspection, type ProjectCatalogRequest } from "./contracts/project-catalog-v1.ts";

export function PreparationRecovery({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [inspection, setInspection] = useState<PreparationInspection | null>(null);
  const [application, setApplication] = useState<ApplicationStatus | null>(null);
  const [history, setHistory] = useState<ApplicationHistoryEntry[] | null>(null);
  const [preview, setPreview] = useState<ApplicationPreview | null>(null);
  const [confirmWorkspace, setConfirmWorkspace] = useState(false);
  const [createdWorkspace, setCreatedWorkspace] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"" | "done" | "unknown" | "error" | "applicationError" | "previewError" | "prepareError">("");
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    epoch.current++; setInspection(null); setApplication(null); setHistory(null); setPreview(null); setMessage("");
    setConfirmWorkspace(false); setCreatedWorkspace(null);
    return () => { epoch.current++; };
  }, [projectId, disabled]);
  const request = async (input: ProjectCatalogRequest) => {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setMessage("");
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(input), input);
      if (epoch.current !== generation) return;
      if (result.action === "inspectPreparation") setInspection(result.inspection);
      else if (result.action === "applicationStatus") setApplication(result.status);
      else if (result.action === "cancelApplication") {
        setApplication(null);
        const query = { action: "applicationStatus" as const, projectId };
        const refreshed = validateProjectCatalogResult(await window.m5Projects.request(query), query);
        if (epoch.current === generation && refreshed.action === "applicationStatus") setApplication(refreshed.status);
      }
      else if (result.action === "applicationHistory") setHistory(result.entries);
      else if (result.action === "applicationPreview") setPreview(result.preview);
      else if (result.action === "prepareApplicationWorkspace") {
        setCreatedWorkspace(result.worktreeId); setPreview(null); setInspection(null);
      }
      else if (result.action === "abandonPreparation") {
        setInspection(null);
        setMessage("done");
      }
    } catch {
      if (epoch.current === generation && (input.action === "applicationStatus" || input.action === "cancelApplication")) setApplication(null);
      if (epoch.current === generation) setMessage(input.action === "abandonPreparation"
        ? "unknown"
        : input.action === "prepareApplicationWorkspace" ? "prepareError"
        : input.action === "applicationPreview" ? "previewError"
        : input.action === "applicationStatus" || input.action === "applicationHistory" || input.action === "cancelApplication" ? "applicationError" : "error");
    } finally { pending.current = false; setBusy(false); }
  };
  useEffect(() => {
    if (!application?.running || disabled || busy) return;
    const timer = window.setTimeout(() => { void request({ action: "applicationStatus", projectId }); }, 1000);
    return () => window.clearTimeout(timer);
  }, [application, disabled, busy, projectId]);
  return <div aria-label={t("m5.prep.title")} aria-busy={busy}>
    <BackupStorageUsageView disabled={disabled || busy} />
    <BackupPreparationRecovery projectId={projectId} disabled={disabled || busy} />
    <button type="button" disabled={disabled || busy} onClick={() => setConfirmWorkspace(true)}>{t("m5.application.prepare")}</button>
    {confirmWorkspace && <div role="group" aria-label={t("m5.application.prepare")}>
      <p>{t("m5.application.prepareHelp")}</p>
      <button type="button" disabled={disabled || busy} onClick={() => { setConfirmWorkspace(false); setCreatedWorkspace(null); setPreview(null); void request({ action: "prepareApplicationWorkspace", projectId }); }}>{t("m5.application.prepareConfirm")}</button>
      <button type="button" disabled={busy} onClick={() => setConfirmWorkspace(false)}>{t("m5.prep.back")}</button>
    </div>}
    {createdWorkspace && <p role="status">{t("m5.application.preparedWorkspace")} <code>{createdWorkspace}</code></p>}
    <button type="button" disabled={disabled || busy} onClick={() => { setPreview(null); void request({ action: "applicationPreview", projectId }); }}>{t("m5.application.preview")}</button>
    {preview && <section aria-label={t("m5.application.preview")}>
      <p>{t("m5.application.previewHelp")}</p>
      <p>{t("m5.application.workspace")} <code>{preview.worktreeId}</code></p>
      <p>{t("m5.application.total")} {preview.total}</p>
      {preview.total === 0 && <p>{t("m5.application.noChanges")}</p>}
      {preview.total > preview.changes.length && <p>{t("m5.application.previewPartial")}</p>}
      <ul>{preview.changes.map(change => <li key={change.path}>
        {t(`m5.application.${change.kind}`)} <code>{change.path.replace(/[\p{Cc}\p{Cf}]/gu, character => `\\u{${character.codePointAt(0)!.toString(16)}}`)}</code>
      </li>)}</ul>
    </section>}
    <button type="button" disabled={disabled || busy} onClick={() => { setInspection(null); void request({ action: "inspectPreparation", projectId }); }}>{t("m5.prep.inspect")}</button>
    <button type="button" disabled={disabled || busy} onClick={() => { setApplication(null); void request({action:"applicationStatus",projectId}); }}>{t("m5.application.inspect")}</button>
    {application && <section aria-label={t("m5.application.inspect")}>
      <div role="status">
        <p>{t(application.running ? "m5.application.running" : application.state === "prepared" || application.state === "applying" ? "m5.application.interrupted" : `m5.application.${application.state}`)}</p>
        {application.currentIndex !== null && <p>{t("m5.application.item")} {application.currentIndex + 1} / {application.total}</p>}
      </div>
      {application.running && application.applicationId && <button type="button" disabled={disabled || busy} onClick={() => void request({ action: "cancelApplication", projectId, applicationId: application.applicationId! })}>{t("m5.application.stop")}</button>}
      <ApplicationFailure failure={application.failure} />
      {!application.running && application.applicationId && ["prepared", "applying", "applied"].includes(application.state) && <RestorationPreview key={application.applicationId} projectId={projectId} applicationId={application.applicationId} disabled={disabled || busy} />}
    </section>}
    <button type="button" disabled={disabled || busy} onClick={() => { setHistory(null); void request({ action: "applicationHistory", projectId }); }}>{t("m5.application.history")}</button>
    {history && <section aria-label={t("m5.application.history")}>
      <p>{t("m5.application.historyHelp")}</p>
      {history.length === 0 ? <p role="status">{t("m5.application.historyEmpty")}</p> : <ul>
        {history.map(entry => <li key={entry.applicationId}><details>
          <summary>{t(`m5.application.${entry.state}`)} — {t("m5.application.total")} {entry.total}</summary>
          <code>{entry.applicationId}</code>
        </details></li>)}
      </ul>}
    </section>}
    {inspection && <div role="group" aria-label={t("m5.prep.confirm")}>
      <p>{t("m5.prep.help")}</p>
      <code>{inspection.worktreeId}</code>
      <button type="button" disabled={disabled || busy} onClick={() => void request({ action: "abandonPreparation", inspection })}>{t("m5.prep.abandon")}</button>
      <button type="button" disabled={busy} onClick={() => setInspection(null)}>{t("m5.prep.back")}</button>
    </div>}
    {message && <p role="status">{t(message === "prepareError" ? "m5.application.prepareError" : message === "previewError" ? "m5.application.previewError" : message === "applicationError" ? "m5.application.error" : `m5.prep.${message}`)}</p>}
  </div>;
}
