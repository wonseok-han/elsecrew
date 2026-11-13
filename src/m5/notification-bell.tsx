import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import type { AppNotification, NotificationCenter } from "./notifications.ts";

function relativeTime(at: string, lang: string, now: number): string {
  const seconds = Math.round((Date.parse(at) - now) / 1000);
  const format = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  const abs = Math.abs(seconds);
  if (abs < 60) return format.format(Math.round(seconds), "second");
  if (abs < 3600) return format.format(Math.round(seconds / 60), "minute");
  if (abs < 86400) return format.format(Math.round(seconds / 3600), "hour");
  return format.format(Math.round(seconds / 86400), "day");
}

export interface BellNotice { text: string; action?: ReactNode }

/**
 * A short notice about what the app is doing right now ("checking…", "done").
 * It shows under the bell instead of the problem strip, fades on its own, and
 * stays only while it carries an action. It is not kept in the bell's list.
 */
function NoticeToast({ notice }: { notice: BellNotice | null }) {
  const { t } = useI18n();
  const [hidden, setHidden] = useState<string | null>(null);
  const text = notice?.text ?? null;
  const sticky = Boolean(notice?.action);
  useEffect(() => {
    setHidden(null);
    if (!text || sticky) return;
    const timer = window.setTimeout(() => setHidden(text), 6000);
    return () => window.clearTimeout(timer);
  }, [text, sticky]);
  if (!notice || hidden === notice.text) return null;
  return <div className="m5-notice-toast" role="status" aria-live="polite">
    <span dir="auto">{notice.text}</span>
    {notice.action}
    <button type="button" className="m5-notice-toast-close" aria-label={t("m5.notifications.dismiss")} title={t("m5.notifications.dismiss")} onClick={() => setHidden(notice.text)}>×</button>
  </div>;
}

/** The bell: newest first, an unread count, and a click that opens the task. */
export function NotificationBell({ center, onOpen, notice = null }: { center: NotificationCenter; onOpen: (item: AppNotification) => void; notice?: BellNotice | null }) {
  const { t, lang } = useI18n();
  const items = useSyncExternalStore(center.subscribe, () => center.snapshot(), () => center.snapshot());
  const unread = items.filter(item => !item.read).length;
  const details = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    const close = (event: PointerEvent) => { if (details.current?.open && !details.current.contains(event.target as Node)) details.current.open = false; };
    document.addEventListener("pointerdown", close, true);
    return () => document.removeEventListener("pointerdown", close, true);
  }, []);
  const now = Date.now();
  return <div className="m5-bell-area"><details ref={details} className="m5-notification-bell" onKeyDown={event => { if (event.key === "Escape" && details.current) details.current.open = false; }}>
    <summary title={t("m5.notifications.title")} aria-label={unread ? t("m5.notifications.unread", { count: unread }) : t("m5.notifications.title")}>
      <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4"/></svg>
      {unread > 0 && <span className="m5-notification-count" aria-hidden="true">{unread > 9 ? "9+" : unread}</span>}
    </summary>
    <div className="m5-notification-panel" role="region" aria-label={t("m5.notifications.title")}>
      <header><h3>{t("m5.notifications.title")}</h3>{unread > 0 && <button type="button" className="m5-secondary" onClick={() => center.markAllRead()}>{t("m5.notifications.markAllRead")}</button>}</header>
      {items.length === 0 ? <p className="m5-notification-empty">{t("m5.notifications.empty")}</p> : <ul>
        {items.map(item => <li key={item.id}>
          <button type="button" className={`m5-notification-item m5-notification-${item.kind}${item.read ? "" : " m5-notification-unread"}`} onClick={() => {
            center.markRead(item.id);
            if (details.current) details.current.open = false;
            onOpen(item);
          }}>
            <span className="m5-notification-kind">{t(`m5.notifications.kind.${item.kind}`)}</span>
            <strong dir="auto">{item.goal}</strong>
            {item.text && <span className="m5-notification-text" dir="auto">{item.text}</span>}
            <time dateTime={item.at}>{relativeTime(item.at, lang, now)}</time>
          </button>
        </li>)}
      </ul>}
    </div>
  </details><NoticeToast notice={notice} /></div>;
}
