// Every project's tasks in one place (ADE multi-project P4): the pure rules.
import type { TaskInboxItem } from "./contracts/project-catalog-v1.ts";

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

/** Tasks of a project the engine reports at work (for the project badges). */
export function workingByProject(items: readonly TaskInboxItem[]): Map<string, string[]> {
  const projects = new Map<string, string[]>();
  for (const item of items) {
    if (inboxGroup(item) !== "working") continue;
    projects.set(item.projectId, [...(projects.get(item.projectId) ?? []), item.chatId]);
  }
  return projects;
}

