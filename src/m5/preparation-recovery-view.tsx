import { useI18n } from "../i18n.tsx";
import { RestorationPreview } from "./restoration-preview.tsx";
import { BackupPreparationRecovery } from "./backup-preparation-recovery.tsx";
import { ApplicationFailure } from "./application-failure.tsx";
import { ApplicationRetirement } from "./application-retirement.tsx";
import { BackupRetentionView } from "./backup-retention.tsx";
import { useEffect, useRef, useState } from "react";
import { validateProjectCatalogResult, type ApplicationHistoryEntry, type ApplicationStatus, type PreparationInspection, type ProjectCatalogRequest } from "./contracts/project-catalog-v1.ts";

export function PreparationRecovery({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  return <PreparationRecoverySession key={projectId} projectId={projectId} disabled={disabled} />;
}

function PreparationRecoverySession({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [inspection, setInspection] = useState<PreparationInspection | null>(null);
  const [application, setApplication] = useState<ApplicationStatus | null>(null);
  const [history, setHistory] = useState<ApplicationHistoryEntry[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"" | "done" | "unknown" | "error" | "applicationError" | "retired">("");
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    epoch.current++; setInspection(null); setApplication(null); setHistory(null); setMessage("");
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
      else if (result.action === "abandonPreparation") {
        setInspection(null);
        setMessage("done");
      }
    } catch {
      if (epoch.current === generation && (input.action === "applicationStatus" || input.action === "cancelApplication")) setApplication(null);
      if (epoch.current === generation) setMessage(input.action === "abandonPreparation"
        ? "unknown"
        : input.action === "applicationStatus" || input.action === "applicationHistory" || input.action === "cancelApplication" ? "applicationError" : "error");
    } finally { pending.current = false; setBusy(false); }
  };
  useEffect(() => {
    if (!application?.running || disabled || busy) return;
    const timer = window.setTimeout(() => { void request({ action: "applicationStatus", projectId }); }, 1000);
    return () => window.clearTimeout(timer);
  }, [application, disabled, busy, projectId]);
  return <div aria-label={t("m5.prep.title")} aria-busy={busy}>
    <section className="m5-maintenance-group"><h4>{t("m5.preferences.backups")}</h4>
    <BackupRetentionView projectId={projectId} disabled={disabled || busy} />
    </section>
    <section className="m5-maintenance-group"><h4>{t("m5.preferences.recovery")}</h4>
    <BackupPreparationRecovery projectId={projectId} disabled={disabled || busy} />
    </section>
    <section className="m5-maintenance-group"><h4>{t("m5.preferences.records")}</h4>
    <button type="button" disabled={disabled || busy} onClick={() => { setInspection(null); void request({ action: "inspectPreparation", projectId }); }}>{t("m5.prep.inspect")}</button>
    <button type="button" disabled={disabled || busy} onClick={() => { setApplication(null); void request({action:"applicationStatus",projectId}); }}>{t("m5.application.inspect")}</button>
    {application && <section aria-label={t("m5.application.inspect")}>
      <div role="status">
        <p>{t(application.running ? "m5.application.running" : application.state === "prepared" || application.state === "applying" ? "m5.application.interrupted" : `m5.application.${application.state}`)}</p>
        {application.currentIndex !== null && <p>{t("m5.application.item")} {application.currentIndex + 1} / {application.total}</p>}
      </div>
      {application.running && application.applicationId && <button type="button" disabled={disabled || busy} onClick={() => void request({ action: "cancelApplication", projectId, applicationId: application.applicationId! })}>{t("m5.application.stop")}</button>}
      <ApplicationFailure failure={application.failure} />
      {!application.running && application.applicationId && ["applied", "restored", "abandoned"].includes(application.state) && <ApplicationRetirement projectId={projectId} applicationId={application.applicationId} disabled={disabled || busy} onRetired={() => { setApplication(null); setHistory(null); setMessage("retired"); }} />}
      {!application.running && application.applicationId && ["prepared", "applying", "applied"].includes(application.state) && <RestorationPreview key={application.applicationId} projectId={projectId} applicationId={application.applicationId} disabled={disabled || busy} onStatus={status => {
        // The child has validated the project and application response. Do not
        // replace a newer selection or a status cleared by another action.
        setApplication(current => current?.applicationId === status.applicationId ? status : current);
      }} />}
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
    </section>
    {message && <p role="status">{t(message === "applicationError" ? "m5.application.error" : `m5.prep.${message}`)}</p>}
  </div>;
}
