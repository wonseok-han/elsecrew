// Which projects have a team at work, and how many of their requests wait
// for the person (ADE multi-project PR-A). A project's tasks keep working
// after the person switches to another project; the project list shows that
// here, and its requests reach the bell as before.

import { needsAttention } from "./notification-groups.ts";
import type { AppNotification } from "./notifications.ts";

interface RunNotice { run: { runId: string; chatId: string; projectId: string; state: string } }
interface ChatNotice { chatId: string; kind: string }
export interface ProjectActivityBridge {
  onAgentRunStateChanged?(listener: (event: RunNotice) => void): () => void;
  onChatEvent?(listener: (event: ChatNotice) => void): () => void;
}

export interface ProjectActivity { working: boolean; attention: number }

const LIVE = new Set(["preparing", "running", "cancelling"]);
/** A task in a project's list that is at work. Unlike the task view, a draft
 * that never started is not: it would show "working" until it is opened. */
const LISTED_WORKING = new Set(["preparing", "running", "stopping", "applying"]);
export function listedWorking(state: string | null | undefined): boolean {
  return state !== null && state !== undefined && LISTED_WORKING.has(state);
}
/** Between two team steps no run is live; the next one starts within seconds. */
export const BETWEEN_STEPS_MS = 20_000;

/**
 * Tasks at work, by project. A task counts as working while one of its runs
 * is live, and for a short while after one ends (the coordinator picks the
 * next member). Opening a project sets its tasks from its own chat list.
 */
export class ProjectActivityTracker {
  /** runId → its task and project, while live. */
  readonly #live = new Map<string, { chatId: string; projectId: string }>();
  /** chatId → project and the time the wait after its last run ends. */
  readonly #between = new Map<string, { projectId: string; until: number }>();
  /** chatId → project, for tasks seen working in a project's own list. */
  readonly #listed = new Map<string, string>();
  readonly #listeners = new Set<() => void>();
  readonly #now: () => number;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #version = 0;

  constructor(now: () => number = Date.now) { this.#now = now; }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => { this.#listeners.delete(listener); };
  };

  observeRun(run: RunNotice["run"]): void {
    if (LIVE.has(run.state)) {
      this.#live.set(run.runId, { chatId: run.chatId, projectId: run.projectId });
      this.#between.delete(run.chatId);
    } else {
      if (!this.#live.delete(run.runId) && !this.#listed.has(run.chatId)) return;
      this.#listed.delete(run.chatId);
      if (![...this.#live.values()].some(other => other.chatId === run.chatId)) {
        this.#between.set(run.chatId, { projectId: run.projectId, until: this.#now() + BETWEEN_STEPS_MS });
        this.#schedule();
      }
    }
    this.#emit();
  }

  /** A team status line for a task known only from its list: what it said
   * is read again when the project opens. (A step's end keeps its short wait:
   * "continue" is a status line too.) */
  observeStatus(chatId: string): void {
    if (this.#listed.delete(chatId)) this.#emit();
  }

  /** The engine's list of every project's tasks at work (the task inbox):
   * it replaces what the project lists said, for every project. */
  reconcileFromEngine(working: ReadonlyMap<string, readonly string[]>): void {
    this.#listed.clear();
    for (const [projectId, chats] of working) for (const chatId of chats) this.#listed.set(chatId, projectId);
    this.#emit();
  }

  /** A project's own list, just read: these of its tasks are at work. */
  reconcileProject(projectId: string, workingChatIds: readonly string[]): void {
    const working = new Set(workingChatIds);
    for (const [chatId, owner] of [...this.#listed]) if (owner === projectId && !working.has(chatId)) this.#listed.delete(chatId);
    for (const chatId of working) this.#listed.set(chatId, projectId);
    this.#emit();
  }

  /** Changes with every notice that may change what is shown. */
  version = (): number => this.#version;

  /** Projects with a task at work. */
  workingProjects(): string[] {
    const now = this.#now();
    return [...new Set([
      ...[...this.#live.values()].map(run => run.projectId),
      ...[...this.#between.values()].filter(wait => wait.until > now).map(wait => wait.projectId),
      ...this.#listed.values(),
    ])];
  }

  working(projectId: string): boolean {
    const now = this.#now();
    return [...this.#live.values()].some(run => run.projectId === projectId)
      || [...this.#between.values()].some(wait => wait.projectId === projectId && wait.until > now)
      || [...this.#listed.values()].includes(projectId);
  }

  dispose(): void {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = null;
    this.#listeners.clear();
  }

  #schedule(): void {
    if (this.#timer) clearTimeout(this.#timer);
    const now = this.#now();
    const next = Math.min(...[...this.#between.values()].map(wait => wait.until).filter(until => until > now));
    if (!Number.isFinite(next)) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      const at = this.#now();
      for (const [chatId, wait] of [...this.#between]) if (wait.until <= at) this.#between.delete(chatId);
      this.#emit();
      this.#schedule();
    }, next - now + 5);
  }

  #emit(): void {
    this.#version += 1;
    for (const listener of this.#listeners) listener();
  }
}

/** Starts following the engine's run and team notices. Returns a stop function. */
export function followProjectActivity(bridge: ProjectActivityBridge, tracker: ProjectActivityTracker): () => void {
  const stops = [
    bridge.onAgentRunStateChanged?.(({ run }) => tracker.observeRun(run)),
    bridge.onChatEvent?.(event => { if (event.kind === "status") tracker.observeStatus(event.chatId); }),
  ];
  return () => { for (const stop of stops) stop?.(); };
}

/** What the project list shows for a project. */
export function projectActivity(tracker: ProjectActivityTracker, notifications: readonly AppNotification[], projectId: string): ProjectActivity {
  return { working: tracker.working(projectId), attention: notifications.filter(item => item.projectId === projectId && needsAttention(item)).length };
}

/** Other projects than the open one with work going on or waiting (the switcher's mark). */
export function otherProjectsActivity(tracker: ProjectActivityTracker, notifications: readonly AppNotification[], openProjectId: string): ProjectActivity {
  return {
    working: tracker.workingProjects().some(projectId => projectId !== openProjectId),
    attention: notifications.filter(item => item.projectId !== openProjectId && needsAttention(item)).length,
  };
}
