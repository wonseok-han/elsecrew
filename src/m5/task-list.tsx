import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import type { TaskGroup } from "./task-status.ts";
import { elapsedParts, groupStartsFolded, groupTaskRows, relativeTime, type TaskListRow } from "./task-list-model.ts";

/** One row of the task list, prepared by the workbench from the single task status. */
export interface TaskListItem extends TaskListRow {
  goal: string;
  /** The status words (task-status.ts labels, "new state", "check failed"). */
  status: string;
  /** Status colour classes and phase marker, as the old list drew them. */
  statusClass: string;
  statusPhase: string;
  selected: boolean;
  /** What the open task waits for or who works on it now. */
  detail?: { kind: "approval"; role: string } | { kind: "working"; role: string; startedAt: string };
  /** Changed files of the open task (review rows). */
  fileCount?: number;
}

/**
 * The task list (ADE 1차-나 L3): rows grouped by status — needs me, working,
 * waiting for review, done (folded) — with empty groups hidden. Deleting a
 * task sits in each row's ⋯ menu. In the rail (below 1100px) only the group
 * dots and counts show; a dot opens the list over the story.
 */
export function TaskList({ items, disabled, removeDisabled, onOpen, onRemove, rail, header, footer }: {
  items: readonly TaskListItem[];
  disabled: boolean;
  removeDisabled: boolean;
  onOpen: (chatId: string) => void;
  onRemove: (chatId: string) => void;
  rail: boolean;
  /** Project switcher and the new-task button, above the groups. */
  header: ReactNode;
  /** Check again, more tasks and errors, below the groups. */
  footer: ReactNode;
}) {
  const { lang, t } = useI18n();
  const groups = groupTaskRows(items);
  const openGroup = groups.find(entry => entry.rows.some(row => row.selected))?.group ?? null;
  // The person's own folding wins; otherwise "done" starts folded unless it holds the open task.
  const [foldChoice, setFoldChoice] = useState<ReadonlyMap<TaskGroup, boolean>>(() => new Map());
  const isFoldedGroup = (group: TaskGroup) => foldChoice.get(group) ?? groupStartsFolded(group, group === openGroup);
  // In the rail each status dot opens its own group's list (review m2): the
  // dot it belongs to is the only one marked open, focus goes to its first
  // row, and Escape or a click outside closes it and returns focus.
  const [railOpen, setRailOpenState] = useState<TaskGroup | null>(null);
  const railButtons = useRef(new Map<TaskGroup, HTMLButtonElement>());
  const popoverRef = useRef<HTMLDivElement>(null);
  const popoverId = useId();
  const setRailOpen = (group: TaskGroup | null, returnFocus = false) => {
    const previous = railOpen;
    setRailOpenState(group);
    if (group === null && previous && returnFocus) requestAnimationFrame(() => railButtons.current.get(previous)?.focus());
  };
  useEffect(() => { if (!rail) setRailOpenState(null); }, [rail]);
  useEffect(() => {
    if (!railOpen) return;
    requestAnimationFrame(() => popoverRef.current?.querySelector<HTMLElement>(".m5-chat-chip:not(:disabled), button:not(:disabled)")?.focus());
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape" && !event.defaultPrevented) { event.preventDefault(); setRailOpen(null, true); } };
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (popoverRef.current?.contains(target) || [...railButtons.current.values()].some(button => button.contains(target))) return;
      setRailOpenState(null);
    };
    document.addEventListener("keydown", escape);
    document.addEventListener("pointerdown", outside, true);
    return () => { document.removeEventListener("keydown", escape); document.removeEventListener("pointerdown", outside, true); };
  }, [railOpen]);
  const working = items.some(item => item.detail?.kind === "working");
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), working ? 1000 : 30_000);
    return () => window.clearInterval(timer);
  }, [working]);
  const dateFormat = new Intl.DateTimeFormat(lang === "ko" ? "ko-KR" : "en-US", { month: "short", day: "numeric" });
  const when = (iso: string) => {
    const value = relativeTime(iso, now);
    if (!value) return "";
    if (value.key === "m5.when.date") return dateFormat.format(value.date);
    return "n" in value ? t(value.key, { n: value.n }) : t(value.key);
  };
  const sub = (item: TaskListItem) => {
    if (item.detail?.kind === "approval") return t("m5.taskList.waitingApproval", { role: `\u2068${item.detail.role}\u2069` });
    if (item.detail?.kind === "working") {
      const started = Date.parse(item.detail.startedAt);
      const parts = elapsedParts(Number.isFinite(started) ? (now - started) / 1000 : 0);
      // The role name is isolated so a right-to-left name cannot reorder the rest (review m1).
      return `\u2068${item.detail.role}\u2069 · ${t(parts.key, { n: parts.n })}`;
    }
    const pieces = [item.status];
    if (item.fileCount) pieces.push(t("m5.taskList.files", { count: item.fileCount }));
    const time = when(item.updatedAt);
    if (time) pieces.push(time);
    return pieces.join(" · ");
  };
  const toggle = (group: TaskGroup) => setFoldChoice(current => new Map(current).set(group, !isFoldedGroup(group)));
  const shownGroups = rail && railOpen ? groups.filter(entry => entry.group === railOpen) : groups;
  const list = <div className="m5-task-groups" data-testid="task-groups">
    {!rail && groups.length === 0 && <p className="m5-task-list-empty">{t("m5.taskList.empty")}</p>}
    {shownGroups.map(({ group, rows }) => {
      const isFolded = rail ? false : isFoldedGroup(group);
      return <section key={group} className="m5-task-group" data-group={group} aria-label={t(`m5.taskList.group.${group}`)}>
        <h3 className="m5-task-group-head">
          <button type="button" aria-expanded={!isFolded} onClick={() => toggle(group)}>
            <span className={`m5-group-dot m5-group-dot-${group}`} aria-hidden="true" />
            <span className="m5-task-group-name">{t(`m5.taskList.group.${group}`)}</span>
            <span className="m5-task-group-count">{rows.length}{isFolded ? " ▸" : ""}</span>
          </button>
        </h3>
        {!isFolded && rows.map(item => <div key={item.chatId} className={`m5-chat-list-row m5-task-row${item.selected ? " selected" : ""}`}>
          <button type="button" disabled={disabled} aria-current={item.selected ? "true" : undefined} title={`${item.goal} · ${item.status}`}
            className={item.selected ? "m5-chat-chip selected" : "m5-chat-chip"} onClick={() => { setRailOpenState(null); onOpen(item.chatId); }}>
            <span className="m5-chat-goal">{item.goal}</span>
            <span data-task-phase={item.statusPhase} className={`${item.statusClass} m5-task-sub${item.detail?.kind === "approval" ? " m5-task-sub-attention" : ""}`}>{sub(item)}</span>
          </button>
          <details className="m5-task-menu">
            <summary aria-label={t("m5.taskList.menu", { goal: item.goal })} title={t("m5.taskList.menuShort")}>⋯</summary>
            <div className="m5-task-menu-panel">
              <button type="button" className="m5-chat-remove m5-danger" disabled={removeDisabled} aria-label={t("m5.catalog.removeChat", { goal: item.goal })}
                onClick={event => { event.currentTarget.closest("details")?.removeAttribute("open"); onRemove(item.chatId); }}>{t("m5.catalog.remove")}</button>
            </div>
          </details>
        </div>)}
      </section>;
    })}
  </div>;
  if (rail) return <div className="m5-task-list m5-task-rail" data-testid="task-list">
    {header}
    <div className="m5-rail-groups" role="group" aria-label={t("m5.layout.list")}>
      {groups.map(({ group, rows }) => <button key={group} ref={node => { if (node) railButtons.current.set(group, node); else railButtons.current.delete(group); }} type="button" className="m5-rail-group" data-group={group}
        aria-expanded={railOpen === group} aria-controls={railOpen === group ? popoverId : undefined} title={`${t(`m5.taskList.group.${group}`)} ${rows.length}`}
        aria-label={`${t(`m5.taskList.group.${group}`)} ${rows.length}`} onClick={() => setRailOpen(railOpen === group ? null : group)}>
        <span className={`m5-group-dot m5-group-dot-${group}`} aria-hidden="true" />
        {group !== "done" && <span className={`m5-rail-badge m5-rail-badge-${group}`} aria-hidden="true">{rows.length}</span>}
      </button>)}
    </div>
    {railOpen && <div ref={popoverRef} id={popoverId} className="m5-task-rail-popover" role="region" aria-label={t(`m5.taskList.group.${railOpen}`)}>{list}{footer}</div>}
  </div>;
  return <div className="m5-task-list" data-testid="task-list">
    {header}
    {list}
    {footer}
  </div>;
}
