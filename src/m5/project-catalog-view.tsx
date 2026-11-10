import { useI18n } from "../i18n.tsx";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { ProjectEditorController } from "./editor-state.ts";
import { validateProjectCatalogResult, type ProjectCatalogEntry } from "./contracts/project-catalog-v1.ts";

export function ProjectCatalog({ controller, disabled, currentId, ready = true, rail = false, home = false, settingsAction, onManage, onRemove, removeDisabled, refreshKey = 0, onHasProjects }: {
  onRemove?: (entry: ProjectCatalogEntry) => void; removeDisabled?: boolean; refreshKey?: number;
  settingsAction?: ReactNode; onManage?: (entry: ProjectCatalogEntry) => void; controller: ProjectEditorController; disabled: boolean; currentId?: string; ready?: boolean; rail?: boolean; home?: boolean; onHasProjects?: (value: boolean) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState(true);
  const [entries, setEntries] = useState<ProjectCatalogEntry[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const compact = rail && !expanded && !error;
  const generation = useRef(0);
  const inFlight = useRef(false);
  useEffect(() => () => { generation.current++; }, []);
  useEffect(() => { generation.current++; if (!rail && !home) setOpen(false); }, [disabled, currentId, rail, home]);
  const service = window.m5Projects;
  useEffect(() => {
    if ((!rail && !home) || !ready || !service) return;
    const epoch = ++generation.current;
    let cancelled = false;
    setOpen(true); setBusy(true); setError(false);
    const input = { action: "list" as const, cursor: null };
    void service.request(input).then(raw => {
      if (cancelled || epoch !== generation.current) return;
      const result = validateProjectCatalogResult(raw, input);
      if (result.action !== "list") throw new Error("unexpected project catalog response");
      setEntries(result.projects); setNext(result.nextCursor);
    }).catch(() => { if (!cancelled && epoch === generation.current) setError(true); })
      .finally(() => { if (!cancelled && epoch === generation.current) setBusy(false); });
    return () => { cancelled = true; };
  }, [rail, home, ready, service, currentId, disabled, refreshKey]);
  useEffect(() => { if (!busy && !error) onHasProjects?.(entries.length > 0); }, [entries, busy, error, onHasProjects]);
  if (!service) return null;
  const load = async (cursor: string | null) => {
    if (disabled || inFlight.current) return;
    const epoch = ++generation.current;
    inFlight.current = true; setBusy(true); setError(false); setOpen(true);
    try {
      const input = { action: "list" as const, cursor };
      const result = validateProjectCatalogResult(await service.request(input), input);
      if (epoch !== generation.current) return;
      if (result.action !== "list") throw new Error("unexpected project catalog response");
      setEntries(result.projects); setNext(result.nextCursor);
    } catch {
      if (epoch === generation.current) setError(true);
    } finally { inFlight.current = false; setBusy(false); }
  };
  const select = async (projectId: string) => {
    if (disabled || inFlight.current) return;
    inFlight.current = true; setBusy(true); generation.current++;
    try { await controller.reopenProject(projectId, input => service.request(input)); }
    finally { inFlight.current = false; setBusy(false); if (!rail && !home) setOpen(false); }
  };
  return <section className={`m5-project-catalog${home ? " m5-home-projects" : ""}${rail ? ` m5-project-rail${expanded || error ? " m5-project-rail-expanded" : ""}` : ""}`} aria-label={t("m5.catalog.title")}>
    {rail && <div className="m5-rail-brand" title={t("m5.shell.eyebrow")} aria-label={t("m5.shell.eyebrow")}>{compact ? "E" : t("m5.shell.eyebrow")}</div>}
    {home ? <header className="m5-home-projects-heading"><h2>{t("m5.home.recent")}</h2><span>{t("m5.catalog.order")}</span></header> : rail ? <header className="m5-project-rail-header"><h2 title={t("m5.catalog.title")}>{t(compact ? "m5.catalog.shortTitle" : "m5.catalog.title")}</h2><button type="button" className="m5-project-refresh m5-rail-action" disabled={disabled || busy} title={t("m5.catalog.refresh")} aria-label={t("m5.catalog.refresh")} onClick={() => void load(null)}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M20 4v5h-5 M20 9a8 8 0 1 0 0 6" /></svg></button><button type="button" className="m5-project-add m5-rail-action" disabled={disabled || busy} title={t("m5.catalog.add")} aria-label={t("m5.catalog.add")} onClick={() => void controller.chooseProject()}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M12 5v14 M5 12h14" /></svg></button></header> : <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => open ? setOpen(false) : void load(null)} aria-expanded={open}>{t("m5.catalog.title")}</button>}
    {rail && <button type="button" className="m5-rail-bookmark" aria-expanded={expanded || error} disabled={error} title={t(expanded || error ? "m5.catalog.collapse" : "m5.catalog.expand")} aria-label={t(expanded || error ? "m5.catalog.collapse" : "m5.catalog.expand")} onClick={() => setExpanded(value => !value)}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d={expanded || error ? "m14 7-5 5 5 5" : "m10 7 5 5-5 5"} /></svg></button>}
    {open && <div className="m5-project-catalog-list" aria-busy={busy}>
      {!rail && !home && <p>{t("m5.catalog.order")}</p>}
      {rail && ready && disabled && <details className="m5-project-switch-help" onToggle={event => { if (event.currentTarget.open) setExpanded(true); }}><summary title={t("m5.catalog.switchTitle")}>{t("m5.catalog.switchTitle")}</summary><p>{t("m5.catalog.switchHelp")}</p></details>}
      {busy && <p role="status" title={t("m5.catalog.busy")}>{t(compact ? "m5.catalog.shortBusy" : "m5.catalog.busy")}</p>}
      {error ? <p role="alert">{t("m5.catalog.error")}</p> : !busy && entries.length === 0 ? <p title={t("m5.catalog.empty")}>{t(compact ? "m5.catalog.shortEmpty" : "m5.catalog.empty")}</p> :
        <ul>{entries.map(entry => <li key={entry.projectId}><button type="button" aria-current={entry.projectId === currentId ? "true" : undefined} title={entry.canonicalPath} disabled={disabled || busy || entry.projectId === currentId} onClick={() => void select(entry.projectId)}>
          {rail && <i className="m5-project-monogram" aria-hidden="true">{Array.from(entry.canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? entry.projectId).slice(0, 2).join("").toUpperCase()}</i>}
          <strong>{entry.canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? entry.projectId}</strong>
          {entry.projectId === currentId && <small className="m5-project-current">{t("m5.catalog.active")}</small>}
          <span dir="auto">{entry.canonicalPath}</span>
        </button><details className="m5-project-overflow" onKeyDown={event => { if (event.key === "Escape") { event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); } }}><summary aria-label={t("m5.catalog.manage")} title={t("m5.catalog.manage")}>⋯</summary><div><button type="button" className="m5-project-manage" disabled={disabled || busy} onClick={event => { event.currentTarget.closest("details")?.removeAttribute("open"); onManage?.(entry); }} title={t("m5.catalog.manage")}>{t("m5.preferences.project")}</button>{onRemove && <button type="button" disabled={removeDisabled || busy} onClick={event => { event.currentTarget.closest("details")?.removeAttribute("open"); onRemove(entry); }}>{t("m5.catalog.remove")}</button>}</div></details></li>)}</ul>}
      {(!rail || !disabled || error) && <div className="m5-project-pagination">{!rail && <button type="button" title={t("m5.catalog.first")} aria-label={t("m5.catalog.first")} disabled={disabled || busy} onClick={() => void load(null)}>{t("m5.catalog.first")}</button>}
        {next && <button type="button" disabled={disabled || busy || error} onClick={() => void load(next)}>{t("m5.catalog.next")}</button>}</div>}
    </div>}

    {rail && <footer className="m5-rail-footer">{settingsAction}</footer>}
  </section>;
}
