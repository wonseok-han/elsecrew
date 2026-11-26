import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import type { AppNotification, NotificationCenter } from "./notifications.ts";
import { attentionCount, groupNotifications, type NotificationTab } from "./notification-groups.ts";
import { ProjectMark } from "./project-catalog-view.tsx";
import { projectName } from "./project-recency.ts";
import { validateProjectCatalogResult } from "./contracts/project-catalog-v1.ts";

/** Project names for the panel's groups, read from the saved list when the panel opens (ADE U6). */
function useProjectNames(open: boolean, ids: readonly string[]): Readonly<Record<string, string>> {
  const [names, setNames] = useState<Record<string, string>>({});
  const missing = ids.filter(id => !(id in names)).join(",");
  useEffect(() => {
    if (!open || !missing || !window.m5Projects) return;
    let alive = true;
    void (async () => {
      const found: Record<string, string> = {};
      let cursor: string | null = null;
      for (let page = 0; page < 10; page++) {
        const input = { action: "list" as const, cursor };
        const result = validateProjectCatalogResult(await window.m5Projects!.request(input), input);
        if (result.action !== "list") break;
        for (const entry of result.projects) found[entry.projectId] = projectName(entry.canonicalPath, entry.projectId);
        cursor = result.nextCursor;
        if (!cursor) break;
      }
      if (alive) setNames(current => ({ ...current, ...found }));
    })().catch(() => undefined);
    return () => { alive = false; };
  }, [open, missing]);
  return names;
}

function relativeTime(at: string, lang: string, now: number): string {
  const seconds = Math.round((Date.parse(at) - now) / 1000);
  const format = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  const abs = Math.abs(seconds);
  if (abs < 60) return format.format(Math.round(seconds), "second");
  if (abs < 3600) return format.format(Math.round(seconds / 60), "minute");
  if (abs < 86400) return format.format(Math.round(seconds / 3600), "hour");
  return format.format(Math.round(seconds / 86400), "day");
}

/** How the notice reads at a glance: still working, done, or stopped short. */
export type BellNoticeTone = "info" | "progress" | "success" | "warning";
export interface BellNotice { text: string; action?: ReactNode; tone?: BellNoticeTone }

function NoticeIcon({ tone }: { tone: BellNoticeTone }) {
  if (tone === "progress") return <span className="m5-notice-toast-spinner" aria-hidden="true" />;
  const path = tone === "success" ? "M5 12.5l4.5 4.5L19 7.5"
    : tone === "warning" ? "M12 4l9 16H3L12 4zM12 10v4.5M12 17.5v.01"
    : "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8v.01";
  return <svg className="m5-notice-toast-icon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d={path} /></svg>;
}
/** An error from what the user just did: stays until dismissed, details fold. */
export interface BellError { title: string; body?: ReactNode; /** What makes a later, different error show again after dismissing. */ key?: string }

/**
 * A short notice about what the app is doing right now ("checking…", "done").
 * It shows under the bell instead of the problem strip, fades on its own, and
 * stays only while it carries an action. It is not kept in the bell's list.
 */
function useDismissed(key: string | null, sticky: boolean): [boolean, () => void] {
  const [hidden, setHidden] = useState<string | null>(null);
  useEffect(() => {
    setHidden(null);
    if (!key || sticky) return;
    const timer = window.setTimeout(() => setHidden(key), 6000);
    return () => window.clearTimeout(timer);
  }, [key, sticky]);
  return [key !== null && hidden === key, () => setHidden(key)];
}

function ErrorToast({ error }: { error: BellError }) {
  const { t } = useI18n();
  const [dismissed, dismiss] = useDismissed(error.key ?? error.title, true);
  if (dismissed) return null;
  return <div className="m5-notice-toast m5-notice-toast-error" role="alert">
    <div><strong dir="auto">{error.title}</strong>{error.body}</div>
    <button type="button" className="m5-notice-toast-close" aria-label={t("m5.notifications.dismiss")} title={t("m5.notifications.dismiss")} onClick={dismiss}>×</button>
  </div>;
}

function NoticeToast({ notice }: { notice: BellNotice }) {
  const { t } = useI18n();
  const [dismissed, dismiss] = useDismissed(notice.text, Boolean(notice.action));
  if (dismissed) return null;
  const tone = notice.tone ?? "info";
  return <div className={`m5-notice-toast m5-notice-toast-${tone}`} role="status" aria-live="polite">
    <NoticeIcon tone={tone} />
    <span dir="auto">{notice.text}</span>
    {notice.action}
    <button type="button" className="m5-notice-toast-close" aria-label={t("m5.notifications.dismiss")} title={t("m5.notifications.dismiss")} onClick={dismiss}>×</button>
  </div>;
}

/** The bell: newest first, an unread count, and a click that opens the task. */
export function NotificationBell({ center, onOpen, notice = null, errors = [], problem, hasProblem = false }: {
  center: NotificationCenter; onOpen: (item: AppNotification) => void;
  notice?: BellNotice | null; errors?: readonly BellError[];
  /** Something that still needs attention (e.g. a stopped run); shown first in the panel. */
  problem?: ReactNode; hasProblem?: boolean;
}) {
  const { t, lang } = useI18n();
  const items = useSyncExternalStore(center.subscribe, () => center.snapshot(), () => center.snapshot());
  const unread = items.filter(item => !item.read).length;
  // The badge counts only what still needs the person (ADE U6); new done/stopped lines show in bold inside.
  const waiting = attentionCount(items);
  const details = useRef<HTMLDetailsElement>(null);
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<NotificationTab>("all");
  useEffect(() => {
    const close = (event: PointerEvent) => { if (details.current?.open && !details.current.contains(event.target as Node)) details.current.open = false; };
    document.addEventListener("pointerdown", close, true);
    return () => document.removeEventListener("pointerdown", close, true);
  }, []);
  const groups = groupNotifications(items, tab);
  const names = useProjectNames(open, [...new Set(items.map(item => item.projectId))]);
  const now = Date.now();
  // A new problem announces itself once under the bell (and to screen readers).
  const [problemNotice, setProblemNotice] = useState(0);
  const hadProblem = useRef(false);
  useEffect(() => {
    if (hasProblem && !hadProblem.current) setProblemNotice(value => value + 1);
    hadProblem.current = hasProblem;
  }, [hasProblem]);
  const shownNotice: BellNotice | null = notice ?? (hasProblem && problemNotice > 0 ? { text: t("m5.notifications.problemNotice"), tone: "warning" } : null);
  const openItem = (item: AppNotification) => {
    center.markRead(item.id);
    if (details.current) details.current.open = false;
    onOpen(item);
  };
  const line = (item: AppNotification) => <li key={item.id}>
    <button type="button" className={`m5-notification-item m5-notification-${item.kind}${item.read ? "" : " m5-notification-unread"}${item.resolved ? " m5-notification-resolved" : ""}`} onClick={() => openItem(item)}>
      <span className="m5-notification-dot" aria-hidden="true" />
      <span className="m5-notification-line" dir="auto">{item.text || t(`m5.notifications.kind.${item.kind}`)}</span>
      <span className="m5-notification-meta"><span className="m5-notification-kind">{t(`m5.notifications.kind.${item.kind}`)}</span> · <time dateTime={item.at}>{relativeTime(item.at, lang, now)}</time>{item.resolved && <> · {t("m5.notifications.resolved")}</>}</span>
    </button>
  </li>;
  return <div className="m5-bell-area"><details ref={details} className="m5-notification-bell" onToggle={event => {
    const opened = event.currentTarget.open;
    setOpen(opened);
    // Open on what needs the person when there is any; otherwise everything.
    if (opened) setTab(attentionCount(center.snapshot()) > 0 ? "attention" : "all");
  }} onKeyDown={event => { if (event.key === "Escape" && details.current) details.current.open = false; }}>
    <summary title={hasProblem ? t("m5.notifications.problem") : t("m5.notifications.title")} aria-label={[hasProblem ? t("m5.notifications.problem") : null, waiting ? t("m5.notifications.waiting", { count: waiting }) : t("m5.notifications.title")].filter(Boolean).join(" · ")}>
      <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4"/></svg>
      {hasProblem && <span className="m5-notification-count m5-notification-alert" aria-hidden="true">!</span>}
      {waiting > 0 && <span className={`m5-notification-count${hasProblem ? " m5-notification-count-shifted" : ""}`} aria-hidden="true">{waiting > 9 ? "9+" : waiting}</span>}
    </summary>
    <div className="m5-notification-panel" role="region" aria-label={t("m5.notifications.title")}>
      <header><h3>{t("m5.notifications.title")}</h3>{unread > 0 && <button type="button" className="m5-btn-quiet" onClick={() => center.markAllRead()}>{t("m5.notifications.markAllRead")}</button>}</header>
      <div className="m5-notification-tabs" role="group" aria-label={t("m5.notifications.show")}>
        <button type="button" aria-pressed={tab === "attention"} onClick={() => setTab("attention")}>{t("m5.notifications.attention")}{waiting > 0 && <span className="m5-notification-tab-count">{waiting}</span>}</button>
        <button type="button" aria-pressed={tab === "all"} onClick={() => setTab("all")}>{t("m5.notifications.all")}</button>
      </div>
      {problem}
      {groups.length === 0 ? <div className="m5-empty-state m5-notification-empty"><h3>{t(tab === "attention" ? "m5.notifications.emptyAttention" : "m5.notifications.empty")}</h3><p>{t(tab === "attention" ? "m5.notifications.emptyAttentionHelp" : "m5.notifications.emptyHelp")}</p></div>
        : groups.map(project => {
          const name = names[project.projectId] ?? t("m5.notifications.unknownProject");
          return <section key={project.projectId} className="m5-notification-project" aria-label={name}>
            <h4><ProjectMark name={name} /><span>{name}</span></h4>
            {project.tasks.map(task => <div key={task.chatId} className="m5-notification-task">
              <p className="m5-notification-goal" dir="auto">{task.goal}</p>
              {task.open.length > 0 && <ul>{task.open.map(line)}</ul>}
              {task.settled.length > 0 && <details className="m5-notification-settled"><summary>{t("m5.notifications.settledCount", { count: task.settled.length })}</summary><ul>{task.settled.map(line)}</ul></details>}
            </div>)}
          </section>;
        })}
    </div>
  </details>{(errors.length > 0 || shownNotice) && <div className="m5-toast-stack">
    {errors.map(error => <ErrorToast key={`error:${error.key ?? error.title}`} error={error} />)}
    {shownNotice && <NoticeToast key={`notice:${shownNotice.text}:${problemNotice}`} notice={shownNotice} />}
  </div>}</div>;
}
