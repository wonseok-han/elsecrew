// Notifications are a per-viewer convenience: what happened to the user's
// tasks (an answer arrived, something needs a decision, work stopped). They
// never drive app state; the sidecar remains the source of truth.

export type NotificationKind = "done" | "attention" | "stopped";

export interface AppNotification {
  id: string;
  kind: NotificationKind;
  projectId: string;
  chatId: string;
  goal: string;
  /** Provider or team text to show as-is (for example a stop reason). */
  text: string;
  at: string;
  read: boolean;
  /** The request it was about went away (answered or taken back by the agent). */
  resolved?: boolean;
}

const KEY = "elsecrew.m5.notifications.v1";
const OS_KEY = "elsecrew.m5.notifications.os.v1";
const LIMIT = 50;

/** Chat states that mean the team finished its turn and the user can read the result. */
const DONE_STATES = new Set(["waitingForUser", "completed", "reviewReady"]);
/** Chat states that mean work stopped without finishing. */
const STOPPED_STATES = new Set(["needsAttention", "recoveryRequired"]);

export function outcomeOfChatState(state: string): NotificationKind | null {
  return DONE_STATES.has(state) ? "done" : STOPPED_STATES.has(state) ? "stopped" : null;
}

interface StorageLike { getItem(key: string): string | null; setItem(key: string, value: string): void }

function safeStorage(): StorageLike | null {
  try { return typeof localStorage === "undefined" ? null : localStorage; } catch { return null; }
}

function valid(item: unknown): item is AppNotification {
  if (!item || typeof item !== "object") return false;
  const value = item as Record<string, unknown>;
  return ["id", "projectId", "chatId", "goal", "text", "at"].every(key => typeof value[key] === "string")
    && ["done", "attention", "stopped"].includes(value.kind as string) && typeof value.read === "boolean"
    && (value.resolved === undefined || typeof value.resolved === "boolean");
}

export class NotificationCenter {
  #items: AppNotification[];
  #listeners = new Set<() => void>();
  #storage: StorageLike | null;

  constructor(storage: StorageLike | null = safeStorage()) {
    this.#storage = storage;
    let restored: AppNotification[] = [];
    try {
      const parsed: unknown = JSON.parse(storage?.getItem(KEY) ?? "[]");
      if (Array.isArray(parsed)) restored = parsed.filter(valid).slice(0, LIMIT);
    } catch { restored = []; }
    this.#items = restored;
  }

  snapshot(): readonly AppNotification[] { return this.#items; }
  unread(): number { return this.#items.filter(item => !item.read).length; }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => { this.#listeners.delete(listener); };
  };

  /** Adds a notification once per id; returns whether it was new. */
  add(item: Omit<AppNotification, "read">): boolean {
    if (this.#items.some(existing => existing.id === item.id)) return false;
    this.#items = [{ ...item, read: false }, ...this.#items].slice(0, LIMIT);
    this.#changed();
    return true;
  }

  /** Drops notifications for a task (or a whole project) the user removed. */
  removeFor(projectId: string, chatId?: string): void {
    const kept = this.#items.filter(item => item.projectId !== projectId || (chatId !== undefined && item.chatId !== chatId));
    if (kept.length === this.#items.length) return;
    this.#items = kept;
    this.#changed();
  }

  /** The thing to check is gone: keep the line, read and marked as settled. */
  resolve(id: string): void {
    if (!this.#items.some(item => item.id === id && !item.resolved)) return;
    this.#items = this.#items.map(item => item.id === id ? { ...item, read: true, resolved: true } : item);
    this.#changed();
  }

  /** A task's permission requests all ended with its run (or the task is no
   * longer working): none of them waits for the person any more. */
  resolveApprovalsFor(chatId: string): void {
    const open = (item: AppNotification) => item.chatId === chatId && item.id.startsWith("approval:") && !item.resolved;
    if (!this.#items.some(open)) return;
    this.#items = this.#items.map(item => open(item) ? { ...item, read: true, resolved: true } : item);
    this.#changed();
  }

  markRead(id: string): void {
    if (!this.#items.some(item => item.id === id && !item.read)) return;
    this.#items = this.#items.map(item => item.id === id ? { ...item, read: true } : item);
    this.#changed();
  }

  markAllRead(): void {
    if (this.#items.every(item => item.read)) return;
    this.#items = this.#items.map(item => item.read ? item : { ...item, read: true });
    this.#changed();
  }

  #changed(): void {
    try { this.#storage?.setItem(KEY, JSON.stringify(this.#items)); } catch { /* a full or blocked store only loses history */ }
    for (const listener of this.#listeners) listener();
  }
}

/** macOS banners while the app is in the background; on unless turned off. */
export function osNotificationsEnabled(storage: StorageLike | null = safeStorage()): boolean {
  try { return storage?.getItem(OS_KEY) !== "off"; } catch { return true; }
}

export function setOsNotificationsEnabled(enabled: boolean, storage: StorageLike | null = safeStorage()): void {
  try { storage?.setItem(OS_KEY, enabled ? "on" : "off"); } catch { /* per-viewer preference only */ }
}
