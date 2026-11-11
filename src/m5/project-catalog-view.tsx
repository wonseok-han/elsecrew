import { useI18n } from "../i18n.tsx";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { ProjectEditorController } from "./editor-state.ts";
import { validateProjectCatalogResult, type ProjectCatalogEntry } from "./contracts/project-catalog-v1.ts";

// Keep menus in the browser top layer so scrollable project lists cannot clip them.
function ProjectMenu({ label, children }: { label: string; children: ReactNode }) {
  const details = useRef<HTMLDetailsElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const host = details.current!;
    const panel = popup.current!;
    const close = () => { host.open = false; panel.hidePopover(); };
    const toggle = () => {
      if (!host.open) { panel.hidePopover(); return; }
      panel.showPopover();
      const anchor = host.querySelector("summary")!.getBoundingClientRect();
      const bounds = panel.getBoundingClientRect();
      panel.style.left = `${Math.max(8, Math.min(anchor.right - bounds.width, window.innerWidth - bounds.width - 8))}px`;
      panel.style.top = `${Math.max(8, Math.min(anchor.bottom + 5, window.innerHeight - bounds.height - 8))}px`;
    };
    const outside = (event: PointerEvent) => { if (!host.contains(event.target as Node)) close(); };
    const scroll = (event: Event) => { if (!panel.contains(event.target as Node)) close(); };
    host.addEventListener("toggle", toggle);
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("scroll", scroll, true);
    window.addEventListener("resize", close);
    return () => {
      host.removeEventListener("toggle", toggle);
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", close);
    };
  }, []);
  return <details ref={details} className="m5-project-overflow" onKeyDown={event => { if (event.key === "Escape") { event.currentTarget.open = false; popup.current?.hidePopover(); event.currentTarget.querySelector("summary")?.focus(); } }}>
    <summary aria-label={label} title={label}>⋯</summary>
    <div ref={popup} popover="manual" className="m5-project-context-popup">{children}</div>
  </details>;
}

export function ProjectCatalog({ controller, disabled, currentId, ready = true, rail = false, home = false, management = false, settingsAction, onManage, onRemove, removeDisabled, refreshKey = 0, onHasProjects }: {
  onRemove?: (entry: ProjectCatalogEntry) => void; removeDisabled?: boolean; refreshKey?: number;
  settingsAction?: ReactNode; onManage?: (entry: ProjectCatalogEntry) => void; controller: ProjectEditorController; disabled: boolean; currentId?: string; ready?: boolean; rail?: boolean; home?: boolean; management?: boolean; onHasProjects?: (value: boolean) => void;
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
  useEffect(() => { generation.current++; if (!rail && !home && !management) setOpen(false); }, [disabled, currentId, rail, home, management]);
  const service = window.m5Projects;
  useEffect(() => {
    if ((!rail && !home && !management) || !ready || !service) return;
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
  }, [rail, home, management, ready, service, currentId, disabled, refreshKey]);
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
    finally { inFlight.current = false; setBusy(false); if (!rail && !home && !management) setOpen(false); }
  };
  return <section className={`m5-project-catalog${management ? " m5-management-projects" : ""}${home ? " m5-home-projects" : ""}${rail ? ` m5-project-rail${expanded || error ? " m5-project-rail-expanded" : ""}` : ""}`} aria-label={t("m5.catalog.title")}>
    {rail && <div className="m5-rail-brand" title={t("m5.shell.eyebrow")} aria-label={t("m5.shell.eyebrow")}>{compact ? "E" : t("m5.shell.eyebrow")}</div>}
    {management ? <p>{t("m5.preferences.chooseProject")}</p> : home ? <header className="m5-home-projects-heading"><h2>{t("m5.home.recent")}</h2><span>{t("m5.catalog.order")}</span></header> : rail ? <header className="m5-project-rail-header"><h2 title={t("m5.catalog.title")}>{t(compact ? "m5.catalog.shortTitle" : "m5.catalog.title")}</h2><button type="button" className="m5-project-refresh m5-rail-action" disabled={disabled || busy} title={t("m5.catalog.refresh")} aria-label={t("m5.catalog.refresh")} onClick={() => void load(null)}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M20 4v5h-5 M20 9a8 8 0 1 0 0 6" /></svg></button><button type="button" className="m5-project-add m5-rail-action" disabled={disabled || busy} title={t("m5.catalog.add")} aria-label={t("m5.catalog.add")} onClick={() => void controller.chooseProject()}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M12 5v14 M5 12h14" /></svg></button></header> : <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => open ? setOpen(false) : void load(null)} aria-expanded={open}>{t("m5.catalog.title")}</button>}
    {rail && <button type="button" className="m5-rail-bookmark" aria-expanded={expanded || error} disabled={error} title={t(expanded || error ? "m5.catalog.collapse" : "m5.catalog.expand")} aria-label={t(expanded || error ? "m5.catalog.collapse" : "m5.catalog.expand")} onClick={event => { event.currentTarget.closest("section")?.querySelectorAll<HTMLDetailsElement>("details[open]").forEach(menu => { menu.open = false; }); setExpanded(value => !value); }}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d={expanded || error ? "m14 7-5 5 5 5" : "m10 7 5 5-5 5"} /></svg></button>}
    {open && <div className="m5-project-catalog-list" aria-busy={busy}>
      {!rail && !home && !management && <p>{t("m5.catalog.order")}</p>}
      {rail && ready && disabled && <details className="m5-project-switch-help" onToggle={event => { if (event.currentTarget.open) setExpanded(true); }}><summary title={t("m5.catalog.switchTitle")}>{t("m5.catalog.switchTitle")}</summary><p>{t("m5.catalog.switchHelp")}</p></details>}
      {busy && <p role="status" title={t("m5.catalog.busy")}>{t(compact ? "m5.catalog.shortBusy" : "m5.catalog.busy")}</p>}
      {error ? <p role="alert">{t("m5.catalog.error")}</p> : !busy && entries.length === 0 ? <p title={t("m5.catalog.empty")}>{t(compact ? "m5.catalog.shortEmpty" : "m5.catalog.empty")}</p> :
        <ul>{entries.map(entry => <li key={entry.projectId}><button type="button" aria-current={entry.projectId === currentId ? "true" : undefined} title={entry.canonicalPath} disabled={disabled || busy || (!management && entry.projectId === currentId)} onClick={() => management ? onManage?.(entry) : void select(entry.projectId)}>
          {rail && <i className="m5-project-monogram" aria-hidden="true">{Array.from(entry.canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? entry.projectId).slice(0, 2).join("").toUpperCase()}</i>}
          <strong>{entry.canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? entry.projectId}</strong>
          {entry.projectId === currentId && <small className="m5-project-current">{t("m5.catalog.active")}</small>}
          <span dir="auto">{entry.canonicalPath}</span>
        </button>{!management && <ProjectMenu label={t("m5.catalog.manage")}> <p className="m5-context-heading" title={entry.canonicalPath}>{entry.canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? entry.projectId}</p><button type="button" className="m5-project-manage" disabled={disabled || busy} onClick={event => { event.currentTarget.closest("details")?.removeAttribute("open"); onManage?.(entry); }} title={t("m5.catalog.manage")}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M4 17h16M8 4v6m8 4v6"/></svg>{t("m5.preferences.project")}</button>{onRemove && <button className="m5-danger" type="button" disabled={removeDisabled || busy} onClick={event => { event.currentTarget.closest("details")?.removeAttribute("open"); onRemove(entry); }}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 10v7m4-7v7"/></svg>{t("m5.catalog.remove")}</button>}</ProjectMenu>}</li>)}</ul>}
      {(!rail || !disabled || error) && <div className="m5-project-pagination">{!rail && <button type="button" title={t("m5.catalog.first")} aria-label={t("m5.catalog.first")} disabled={disabled || busy} onClick={() => void load(null)}>{t("m5.catalog.first")}</button>}
        {next && <button type="button" disabled={disabled || busy || error} onClick={() => void load(next)}>{t("m5.catalog.next")}</button>}</div>}
    </div>}

    {rail && <footer className="m5-rail-footer">{settingsAction}</footer>}
  </section>;
}
