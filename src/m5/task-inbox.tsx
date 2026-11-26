import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { TaskInboxItem } from "./contracts/project-catalog-v1.ts";
import { ProjectMark } from "./project-catalog-view.tsx";
import { groupInbox, inboxGroup, type InboxGroup } from "./task-inbox-model.ts";

/**
 * Every project's tasks in one place (ADE multi-project P4): what waits for
 * the person, what works (or waits its turn for an agent), what is ready for
 * review. One click opens the task, switching projects if needed. The list
 * comes from the engine, so tasks started before the app was reloaded are
 * here too.
 */
export function TaskInbox({ items, names, onOpen }: {
  items: readonly TaskInboxItem[];
  /** Project names by id (from the saved list). */
  names: ReadonlyMap<string, string>;
  onOpen: (item: TaskInboxItem) => void;
}) {
  const { t } = useI18n();
  const details = useRef<HTMLDetailsElement>(null);
  const [, setOpen] = useState(false);
  useEffect(() => {
    const close = (event: PointerEvent) => { if (details.current?.open && !details.current.contains(event.target as Node)) details.current.open = false; };
    document.addEventListener("pointerdown", close, true);
    return () => document.removeEventListener("pointerdown", close, true);
  }, []);
  const groups = groupInbox(items);
  const label = t("m5.inbox.label", { attention: groups.attention.length, working: groups.working.length });
  const line = (item: TaskInboxItem) => {
    const name = names.get(item.projectId) ?? t("m5.notifications.unknownProject");
    return <li key={item.chatId}>
      <button type="button" className={`m5-inbox-item m5-inbox-${inboxGroup(item)}`} onClick={() => { if (details.current) details.current.open = false; onOpen(item); }}>
        <ProjectMark name={name} />
        <span className="m5-inbox-text"><span className="m5-inbox-goal" dir="auto">{item.goal}</span>
          <span className="m5-inbox-meta"><bdi>{name}</bdi> · {item.waiting ? t("m5.inbox.waiting") : t(`m5.inbox.state.${inboxGroup(item)}`)}</span></span>
      </button>
    </li>;
  };
  const section = (group: InboxGroup) => groups[group].length > 0 && <section key={group} className="m5-inbox-group" aria-label={t(`m5.inbox.group.${group}`)}>
    <h4>{t(`m5.inbox.group.${group}`)} <span className="m5-inbox-count">{groups[group].length}</span></h4>
    <ul>{groups[group].map(line)}</ul>
  </section>;
  return <details ref={details} className="m5-task-inbox" data-testid="task-inbox" onToggle={event => setOpen(event.currentTarget.open)} onKeyDown={event => { if (event.key === "Escape" && details.current) details.current.open = false; }}>
    <summary title={t("m5.inbox.title")} aria-label={label}>
      <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M4 13l2.5-7h11L20 13M4 13v5h16v-5M4 13h4.5l1 2h5l1-2H20" /></svg>
      {groups.attention.length > 0 ? <span className="m5-notification-count" aria-hidden="true">{groups.attention.length > 9 ? "9+" : groups.attention.length}</span>
        : groups.working.length > 0 && <span className="m5-inbox-live" aria-hidden="true" />}
    </summary>
    <div className="m5-notification-panel m5-inbox-panel" role="region" aria-label={t("m5.inbox.title")}>
      <header><h3>{t("m5.inbox.title")}</h3></header>
      {items.length === 0 ? <div className="m5-empty-state m5-notification-empty"><h3>{t("m5.inbox.empty")}</h3><p>{t("m5.inbox.emptyHelp")}</p></div>
        : (["attention", "working", "review"] as const).map(section)}
    </div>
  </details>;
}
