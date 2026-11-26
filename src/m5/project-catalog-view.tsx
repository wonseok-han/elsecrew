import { useI18n } from "../i18n.tsx";
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { ProjectEditorController } from "./editor-state.ts";
import { validateProjectCatalogResult, type ProjectCatalogEntry } from "./contracts/project-catalog-v1.ts";
import { byRecency, openedAgo, projectMark, projectName, type OpenedAt } from "./project-recency.ts";
import type { ProjectActivity } from "./project-activity.ts";

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
      // Open beside the trigger, crossing the rail edge instead of covering its rows.
      const right = anchor.right + 10;
      const left = right + bounds.width <= window.innerWidth - 8 ? right : anchor.left - bounds.width - 10;
      panel.style.left = `${Math.max(8, Math.min(left, window.innerWidth - bounds.width - 8))}px`;
      panel.style.top = `${Math.max(8, Math.min(anchor.top, window.innerHeight - bounds.height - 8))}px`;
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


const icon = {
  manage: "M4 7h16M4 17h16M8 4v6m8 4v6",
  roles: "M8 10a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM2 20v-2a6 6 0 0 1 12 0v2M16 4a3 3 0 0 1 0 6M17 14a5 5 0 0 1 5 4v2",
  close: "M6 6l12 12M18 6 6 18",
  remove: "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 10v7m4-7v7",
  add: "M12 5v14 M5 12h14",
};
const Icon = ({ path }: { path: string }) => <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d={path} /></svg>;

/** The project's mark: one letter on its own color, so cards are told apart (D5). */
export function ProjectMark({ name, large = false }: { name: string; large?: boolean }) {
  const mark = projectMark(name);
  return <i className={`m5-project-monogram${large ? " m5-project-monogram-large" : ""}`} aria-hidden="true" style={{ "--role-color": `var(--role-${mark.color})` } as CSSProperties}>{mark.letters}</i>;
}

/**
 * The saved projects (ADE U2): `home` — the one list on the home screen, the
 * most recently opened first with "continue"; `switcher` — the popover over
 * the task list, switching closes this project and opens the other in one
 * step (Q6); `management` — the list in settings. Paths are not shown; they
 * are the row's tooltip and `data-path`.
 */
export type CatalogListState = "projects" | "empty" | "failed";
/** Pages the home and the switcher read before ordering by recency (50 projects each). */
const RECENCY_PAGES = 10;

export function ProjectCatalog({ onCloseProject, closeDisabled, onRoles, rolesDisabled, controller, disabled, currentId, ready = true, home = false, switcher = false, management = false, onManage, onRemove, removeDisabled, refreshKey = 0, onListState, onAdd, onSwitch, switchBlockedReason, addBlockedReason, openedAt = {}, activity }: {
  onCloseProject?: () => void; closeDisabled?: boolean; onRoles?: () => void; rolesDisabled?: boolean;
  onRemove?: (entry: ProjectCatalogEntry) => void; removeDisabled?: boolean; refreshKey?: number;
  onManage?: (entry: ProjectCatalogEntry) => void; controller: ProjectEditorController; disabled: boolean; currentId?: string; ready?: boolean; home?: boolean; switcher?: boolean; management?: boolean;
  /** What the first read found: projects, none (the first run), or a failure (the home keeps its retry, PR #12 review I3). */
  onListState?: (state: CatalogListState) => void; onAdd?: () => Promise<void>;
  /** switcher: open another project (the open one is closed first). */
  onSwitch?: (entry: ProjectCatalogEntry) => void;
  /** switcher: why switching is not possible now, or null. */
  switchBlockedReason?: string | null;
  /** switcher: why a folder cannot be added from here now (an open task, unsaved edits), or null (PR #12 review I2). */
  addBlockedReason?: string | null;
  openedAt?: OpenedAt;
  /** home, switcher: a project's team at work and its requests waiting (ADE multi-project). */
  activity?: (projectId: string) => ProjectActivity;
}) {
  const { t, lang } = useI18n();
  const [entries, setEntries] = useState<ProjectCatalogEntry[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [paged, setPaged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [readOnce, setReadOnce] = useState(false);
  const generation = useRef(0);
  const inFlight = useRef(false);
  useEffect(() => () => { generation.current++; }, []);
  useEffect(() => { generation.current++; }, [disabled, currentId]);
  const service = window.m5Projects;
  useEffect(() => {
    if (!ready || !service) return;
    const epoch = ++generation.current;
    let cancelled = false;
    setBusy(true); setError(false);
    void (async () => {
      // The engine pages by project id; the home and the switcher order by when each was opened here,
      // so they read several pages first and the most recent project leads wherever it is (PR #12 review minor 3).
      let cursor: string | null = null;
      const projects: ProjectCatalogEntry[] = [];
      for (let page = 0; page < (home || switcher ? RECENCY_PAGES : 1); page++) {
        const input = { action: "list" as const, cursor };
        const result = validateProjectCatalogResult(await service.request(input), input);
        if (cancelled || epoch !== generation.current) return;
        if (result.action !== "list") throw new Error("unexpected project catalog response");
        for (const project of result.projects) if (!projects.some(seen => seen.projectId === project.projectId)) projects.push(project);
        cursor = result.nextCursor;
        if (!cursor) break;
      }
      setEntries(projects); setNext(cursor); setPaged(false);
    })().catch(() => { if (!cancelled && epoch === generation.current) setError(true); })
      .finally(() => { if (!cancelled && epoch === generation.current) { setBusy(false); setReadOnce(true); } });
    return () => { cancelled = true; };
  }, [ready, service, currentId, disabled, refreshKey]);
  // Said only once the list was read: an empty list before reading is not "no projects".
  useEffect(() => { if (readOnce && !busy) onListState?.(error ? "failed" : entries.length > 0 ? "projects" : "empty"); }, [readOnce, entries, busy, error, onListState]);
  if (!service) return null;
  const load = async (cursor: string | null) => {
    if (disabled || inFlight.current) return;
    const epoch = ++generation.current;
    inFlight.current = true; setBusy(true); setError(false);
    try {
      const input = { action: "list" as const, cursor };
      const result = validateProjectCatalogResult(await service.request(input), input);
      if (epoch !== generation.current) return;
      if (result.action !== "list") throw new Error("unexpected project catalog response");
      setEntries(result.projects); setNext(result.nextCursor); setPaged(cursor !== null);
    } catch {
      if (epoch === generation.current) setError(true);
    } finally { inFlight.current = false; setBusy(false); }
  };
  const select = async (projectId: string) => {
    if (disabled || inFlight.current) return;
    inFlight.current = true; setBusy(true); generation.current++;
    try { await controller.reopenProject(projectId, input => service.request(input)); }
    finally { inFlight.current = false; setBusy(false); }
  };
  const closeMenu = (event: { currentTarget: Element }) => event.currentTarget.closest("details")?.removeAttribute("open");
  const menu = (entry: ProjectCatalogEntry, name: string) => <ProjectMenu label={t("m5.catalog.manageNamed", { name })}>
    <p className="m5-context-heading" title={entry.canonicalPath}>{name}</p>
    <button type="button" className="m5-project-manage" disabled={!ready || busy} onClick={event => { closeMenu(event); onManage?.(entry); }} title={t("m5.catalog.manage")}><Icon path={icon.manage} />{t("m5.preferences.project")}</button>
    {entry.projectId === currentId && onRoles && <button type="button" disabled={rolesDisabled || busy} onClick={event => { closeMenu(event); onRoles(); }}><Icon path={icon.roles} />{t("m5.shell.roles")}</button>}
    {entry.projectId === currentId && onCloseProject && <button type="button" disabled={closeDisabled || busy} onClick={event => { closeMenu(event); onCloseProject(); }}><Icon path={icon.close} />{t("m5.shell.close")}</button>}
    {onRemove && <button className="m5-danger" type="button" disabled={removeDisabled || busy} onClick={event => { closeMenu(event); onRemove(entry); }}><Icon path={icon.remove} />{t("m5.catalog.remove")}</button>}
  </ProjectMenu>;
  const ordered = home || switcher ? byRecency(entries, openedAt) : entries;
  const sub = (entry: ProjectCatalogEntry) => {
    if (entry.projectId === currentId) return t("m5.catalog.active");
    const ago = openedAgo(openedAt[entry.projectId], lang);
    return ago ? t("m5.home.openedAgo", { when: ago }) : t("m5.home.notOpenedHere");
  };
  const add = <button type="button" className="m5-secondary m5-project-add" disabled={disabled || busy || Boolean(addBlockedReason)} onClick={() => void (onAdd ? onAdd() : controller.chooseProject())}><Icon path={icon.add} />{t("m5.catalog.add")}</button>;
  return <section className={`m5-project-catalog${management ? " m5-management-projects" : ""}${home ? " m5-home-projects" : ""}${switcher ? " m5-switcher-projects" : ""}`} aria-label={t("m5.catalog.title")}>
    {management && <p className="m5-settings-intro">{t("m5.preferences.chooseProject")}</p>}
    {home && <header className="m5-home-projects-heading"><div><h1>{t("m5.home.title")}</h1><p>{t("m5.home.subtitle")}</p></div>{add}</header>}
    <div className="m5-project-catalog-list" aria-busy={busy}>
      {busy && <p role="status" className="m5-settings-intro">{t("m5.catalog.busy")}</p>}
      {error ? <p role="alert">{t("m5.catalog.error")}</p> : !busy && entries.length === 0 ? <p className="m5-settings-intro">{t("m5.catalog.empty")}</p> :
        <ul className={home ? "m5-home-cards" : management ? "m5-group" : undefined}>{ordered.map((entry, index) => {
          const name = projectName(entry.canonicalPath, entry.projectId);
          const hero = home && index === 0 && !paged;
          const current = entry.projectId === currentId;
          const blocked = switcher && !current && Boolean(switchBlockedReason);
          const open = () => management ? onManage?.(entry) : switcher ? onSwitch?.(entry) : void select(entry.projectId);
          return <li key={entry.projectId} data-path={entry.canonicalPath} className={`${hero ? "m5-home-hero" : ""}${current ? " m5-project-open" : ""}`.trim() || undefined}>
            <button type="button" className="m5-project-row" aria-current={current ? "true" : undefined} title={entry.canonicalPath} disabled={disabled || busy || blocked || (!management && current)} onClick={open}>
              <ProjectMark name={name} large={home} />
              <span className="m5-project-row-text"><strong>{name}</strong><small className={current ? "m5-project-current" : undefined}>{sub(entry)}</small>{activity && <ActivityBadges activity={activity(entry.projectId)} />}</span>
            </button>
            {hero && <button type="button" className="m5-btn-primary m5-home-continue" aria-label={t("m5.home.continueNamed", { name })} disabled={disabled || busy} onClick={open}>{t("m5.home.continueAction")}</button>}
            {!management && menu(entry, name)}
          </li>;
        })}</ul>}
      {switcher && (switchBlockedReason || addBlockedReason) && <p className="m5-settings-intro m5-switch-blocked" role="status">{switchBlockedReason ?? addBlockedReason}</p>}
      {(next || paged || error) && <div className="m5-project-pagination">{(paged || error) && <button type="button" className={error ? "m5-secondary" : "m5-btn-quiet"} title={t("m5.catalog.first")} aria-label={t("m5.catalog.first")} disabled={disabled || busy} onClick={() => void load(null)}>{t("m5.catalog.first")}</button>}
        {next && <button type="button" className="m5-btn-quiet" disabled={disabled || busy || error} onClick={() => void load(next)}>{t("m5.catalog.next")}</button>}</div>}
      {switcher && <div className="m5-switcher-footer">{add}</div>}
    </div>
  </section>;
}

/** A project's team at work and its requests waiting (ADE multi-project). */
function ActivityBadges({ activity }: { activity: ProjectActivity }) {
  const { t } = useI18n();
  if (!activity.working && activity.attention === 0) return null;
  return <span className="m5-project-activity">
    {activity.attention > 0 && <span className="m5-badge m5-badge-attention">{t("m5.catalog.attention", { count: activity.attention })}</span>}
    {activity.working && <span className="m5-badge m5-badge-running">{t("m5.catalog.working")}</span>}
  </span>;
}
