// The notification panel's shape (ADE U6, DESIGN D10, mockup 12): by project,
// then by task (its goal said once), open lines first and settled ones folded.
// The bell's badge counts only what still needs the person.
import type { AppNotification } from "./notifications.ts";

export type NotificationTab = "attention" | "all";

/** Still waiting for the person: a request or a decision not yet settled. */
export function needsAttention(item: AppNotification): boolean {
  return item.kind === "attention" && !item.resolved;
}

export function attentionCount(items: readonly AppNotification[]): number {
  return items.filter(needsAttention).length;
}

export interface TaskGroup {
  chatId: string;
  goal: string;
  /** Lines still open (unsettled), newest first. */
  open: AppNotification[];
  /** Settled lines, folded under one row; only in the "all" tab. */
  settled: AppNotification[];
  latestAt: number;
}

export interface ProjectGroup {
  projectId: string;
  tasks: TaskGroup[];
  latestAt: number;
}

const time = (item: AppNotification) => { const at = Date.parse(item.at); return Number.isFinite(at) ? at : 0; };

export function groupNotifications(items: readonly AppNotification[], tab: NotificationTab): ProjectGroup[] {
  const shown = tab === "attention" ? items.filter(needsAttention) : items;
  const projects = new Map<string, Map<string, TaskGroup>>();
  for (const item of shown) {
    let tasks = projects.get(item.projectId);
    if (!tasks) { tasks = new Map(); projects.set(item.projectId, tasks); }
    let task = tasks.get(item.chatId);
    if (!task) { task = { chatId: item.chatId, goal: item.goal, open: [], settled: [], latestAt: 0 }; tasks.set(item.chatId, task); }
    (item.resolved ? task.settled : task.open).push(item);
    if (time(item) >= task.latestAt) { task.latestAt = time(item); task.goal = item.goal || task.goal; }
  }
  const newest = (a: AppNotification, b: AppNotification) => time(b) - time(a);
  return [...projects.entries()].map(([projectId, tasks]) => {
    const list = [...tasks.values()].map(task => ({ ...task, open: task.open.sort(newest), settled: task.settled.sort(newest) }))
      .sort((a, b) => b.latestAt - a.latestAt);
    return { projectId, tasks: list, latestAt: Math.max(...list.map(task => task.latestAt)) };
  }).sort((a, b) => b.latestAt - a.latestAt);
}
