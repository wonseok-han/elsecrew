import type { TaskDisposition } from "./task-status.ts";

/**
 * Remembers where each finished task stands against the user's folder, so the
 * task list says "merged" or "applied" for tasks that are not open. Display
 * only, per viewer; the sidecar answers again whenever a task changes.
 *
 * An entry belongs to one version of the task (`updatedAt`): a task that ran
 * again is looked at afresh, never shown with its old standing.
 */
export interface DispositionTask { chatId: string; updatedAt: string; state: string }
type Entry = { updatedAt: string; disposition: TaskDisposition };
interface StorageLike { getItem(key: string): string | null; setItem(key: string, value: string): void }

const KEY = "elsecrew.m5.taskDisposition.v1";
const LIMIT = 300;
/** Only a finished task has a standing worth asking about. */
const FINISHED = new Set(["reviewReady", "completed"]);

function validDisposition(value: unknown): value is TaskDisposition {
  if (!value || typeof value !== "object") return false;
  const item = value as Record<string, unknown>;
  return (item.kind === "git" || item.kind === "copy")
    && ["none", "notYet", "done", "changedSince"].includes(item.standing as string)
    && (item.pending === undefined || (Number.isSafeInteger(item.pending) && (item.pending as number) >= 0));
}

export class TaskDispositionStore {
  #entries = new Map<string, Entry>();
  #listeners = new Set<() => void>();
  #storage: StorageLike | null;
  #inspect: (chatId: string) => Promise<TaskDisposition | null>;
  /** Tasks asked about (or failed) at this version, so a list redraw never re-asks. */
  #asked = new Map<string, string>();
  #queue: Promise<void> = Promise.resolve();
  #version = 0;

  constructor(inspect: (chatId: string) => Promise<TaskDisposition | null>, storage: StorageLike | null = null) {
    this.#inspect = inspect;
    this.#storage = storage;
    try {
      const parsed: unknown = JSON.parse(storage?.getItem(KEY) ?? "{}");
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        for (const [chatId, entry] of Object.entries(parsed as Record<string, unknown>).slice(0, LIMIT)) {
          const value = entry as Record<string, unknown> | null;
          if (value && typeof value.updatedAt === "string" && validDisposition(value.disposition)) this.#entries.set(chatId, { updatedAt: value.updatedAt, disposition: value.disposition });
        }
      }
    } catch { /* a damaged record only means asking again */ }
  }

  subscribe = (listener: () => void): (() => void) => { this.#listeners.add(listener); return () => { this.#listeners.delete(listener); }; };
  /** Changes whenever any answer arrives (for useSyncExternalStore). */
  version(): number { return this.#version; }

  /** The standing for this exact version of the task, or null if unknown. */
  get(task: DispositionTask): TaskDisposition | null {
    const entry = this.#entries.get(task.chatId);
    return entry && entry.updatedAt === task.updatedAt ? entry.disposition : null;
  }

  /** What the open task's own panels just read. */
  record(task: Pick<DispositionTask, "chatId" | "updatedAt">, disposition: TaskDisposition | null): void {
    if (!disposition) return;
    const current = this.#entries.get(task.chatId);
    if (current && current.updatedAt === task.updatedAt && JSON.stringify(current.disposition) === JSON.stringify(disposition)) return;
    this.#entries.delete(task.chatId);
    this.#entries.set(task.chatId, { updatedAt: task.updatedAt, disposition });
    while (this.#entries.size > LIMIT) this.#entries.delete(this.#entries.keys().next().value!);
    this.#changed();
  }

  /** Forget a task (deleted, or its standing changed through a merge or apply). */
  forget(chatId: string): void {
    this.#asked.delete(chatId);
    if (this.#entries.delete(chatId)) this.#changed();
  }

  /** Ask the engine, one task at a time, about finished tasks not known yet. */
  request(tasks: readonly DispositionTask[]): void {
    for (const task of tasks) {
      if (!FINISHED.has(task.state) || this.get(task) || this.#asked.get(task.chatId) === task.updatedAt) continue;
      this.#asked.set(task.chatId, task.updatedAt);
      this.#queue = this.#queue.then(async () => {
        try {
          const disposition = await this.#inspect(task.chatId);
          if (disposition && this.#asked.get(task.chatId) === task.updatedAt) this.record(task, disposition);
        } catch { /* unknown stays "waiting for review" */ }
      });
    }
  }

  #changed(): void {
    this.#version++;
    try { this.#storage?.setItem(KEY, JSON.stringify(Object.fromEntries(this.#entries))); } catch { /* per-viewer cache only */ }
    for (const listener of this.#listeners) listener();
  }
}
