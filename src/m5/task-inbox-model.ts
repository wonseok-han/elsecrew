// Every project's tasks in one place (ADE multi-project P4): the pure rules.
import type { TaskInboxItem, TaskInboxWorking } from "./contracts/project-catalog-v1.ts";

export type InboxGroup = "attention" | "working" | "review";

export function inboxGroup(item: Pick<TaskInboxItem, "state" | "waiting" | "working">): InboxGroup {
  if (item.state === "needsAttention" || item.state === "recoveryRequired" || item.state === "waitingForUser") return "attention";
  if (item.state === "reviewReady" && !item.working && !item.waiting) return "review";
  return "working";
}

export function groupInbox(items: readonly TaskInboxItem[]): Record<InboxGroup, TaskInboxItem[]> {
  const groups: Record<InboxGroup, TaskInboxItem[]> = { attention: [], working: [], review: [] };
  for (const item of items) groups[inboxGroup(item)].push(item);
  return groups;
}

/** Tasks of a project the engine reports at work (for the project badges):
 * from the engine's full list of work, not the shortened inbox list (PR #20
 * review I4). */
export function workingByProject(tasks: readonly TaskInboxWorking[]): Map<string, string[]> {
  const projects = new Map<string, string[]>();
  for (const task of tasks) projects.set(task.projectId, [...(projects.get(task.projectId) ?? []), task.chatId]);
  return projects;
}
