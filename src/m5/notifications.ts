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
  /** When it was settled: settled lines are tidied away a day later (if the person keeps that on). */
  resolvedAt?: string;
}

const KEY = "elsecrew.m5.notifications.v1";
const OS_KEY = "elsecrew.m5.notifications.os.v1";
const PREFS_KEY = "elsecrew.m5.notifications.prefs.v1";
/** Settled lines stay this long before they are tidied away. */
export const RESOLVED_KEEP_MS = 24 * 60 * 60 * 1000;

/** What the person wants to hear about (settings › notifications, ADE U4). */
export interface NotificationPreferences { kinds: Record<NotificationKind, boolean>; pruneResolved: boolean }
export const DEFAULT_NOTIFICATION_PREFERENCES: NotificationPreferences = { kinds: { attention: true, done: true, stopped: true }, pruneResolved: true };

export function readNotificationPreferences(storage: StorageLike | null = safeStorage()): NotificationPreferences {
  try {
    const raw: unknown = JSON.parse(storage?.getItem(PREFS_KEY) ?? "null");
    if (!raw || typeof raw !== "object") return DEFAULT_NOTIFICATION_PREFERENCES;
    const value = raw as { kinds?: Record<string, unknown>; pruneResolved?: unknown };
    const kind = (name: NotificationKind) => typeof value.kinds?.[name] === "boolean" ? value.kinds[name] as boolean : true;
    return { kinds: { attention: kind("attention"), done: kind("done"), stopped: kind("stopped") }, pruneResolved: typeof value.pruneResolved === "boolean" ? value.pruneResolved : true };
  } catch { return DEFAULT_NOTIFICATION_PREFERENCES; }
}

/** Said on the window when the person changes these preferences, so tidying applies at once (PR #12 review minor 4). */
export const NOTIFICATION_PREFERENCES_CHANGED = "m5-notification-preferences-changed";
/** How often settled lines are looked at while the app stays open. */
export const PRUNE_INTERVAL_MS = 60 * 60 * 1000;

export function saveNotificationPreferences(preferences: NotificationPreferences, storage: StorageLike | null = safeStorage()): void {
  try { storage?.setItem(PREFS_KEY, JSON.stringify(preferences)); } catch { /* per-viewer preference only */ }
  if (typeof window !== "undefined" && typeof window.dispatchEvent === "function") window.dispatchEvent(new Event(NOTIFICATION_PREFERENCES_CHANGED));
}
const LIMIT = 50;
/** Open requests are never trimmed for room (PR #14 review I2); this bounds a runaway list of them. */
const OPEN_LIMIT = 200;
const isOpenRequest = (item: AppNotification) => item.kind === "attention" && !item.resolved;
/** Newest first; open requests stay, the rest fills what LIMIT leaves. */
function trimmed(items: readonly AppNotification[]): AppNotification[] {
  const open = new Set(items.filter(isOpenRequest).slice(0, OPEN_LIMIT));
  let room = Math.max(0, LIMIT - open.size);
  return items.filter(item => open.has(item) || (!isOpenRequest(item) && room-- > 0));
}

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
    && Number.isFinite(Date.parse(value.at as string))
    && ["done", "attention", "stopped"].includes(value.kind as string) && typeof value.read === "boolean"
    && (value.resolved === undefined || typeof value.resolved === "boolean")
    && (value.resolvedAt === undefined || typeof value.resolvedAt === "string");
}

export class NotificationCenter {
  #items: AppNotification[];
  #listeners = new Set<() => void>();
  #storage: StorageLike | null;

  #preferences: () => NotificationPreferences;
  #now: () => number;

  constructor(storage: StorageLike | null = safeStorage(), preferences: () => NotificationPreferences = () => readNotificationPreferences(storage), now: () => number = Date.now) {
    this.#storage = storage;
    this.#preferences = preferences;
    this.#now = now;
    let restored: AppNotification[] = [];
    try {
      const parsed: unknown = JSON.parse(storage?.getItem(KEY) ?? "[]");
      if (Array.isArray(parsed)) restored = trimmed(parsed.filter(valid));
    } catch { restored = []; }
    this.#items = restored;
    this.prune();
  }

  /** Tidies away lines settled more than a day ago, when the person keeps that on. */
  prune(): void {
    if (!this.#preferences().pruneResolved) return;
    const limit = this.#now() - RESOLVED_KEEP_MS;
    const kept = this.#items.filter(item => !item.resolved || Date.parse(item.resolvedAt ?? item.at) > limit);
    if (kept.length === this.#items.length) return;
    this.#items = kept;
    this.#changed();
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
    // A kind the person turned off is not kept (nor shown as a banner).
    if (!this.#preferences().kinds[item.kind]) return false;
    this.#items = trimmed([{ ...item, read: false }, ...this.#items]);
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
    const at = new Date(this.#now()).toISOString();
    this.#items = this.#items.map(item => item.id === id ? { ...item, read: true, resolved: true, resolvedAt: at } : item);
    this.#changed();
  }

  /** A task's permission requests all ended with its run (or the task is no
   * longer working): none of them waits for the person any more. */
  resolveApprovalsFor(chatId: string): void {
    const open = (item: AppNotification) => item.chatId === chatId && item.id.startsWith("approval:") && !item.resolved;
    if (!this.#items.some(open)) return;
    const at = new Date(this.#now()).toISOString();
    this.#items = this.#items.map(item => open(item) ? { ...item, read: true, resolved: true, resolvedAt: at } : item);
    this.#changed();
  }

  /** A new run of the task started: the person answered the questions it asked before (PR #14 review I2). */
  resolveQuestionsFor(chatId: string, before: string = new Date(this.#now()).toISOString()): void {
    const open = (item: AppNotification) => item.chatId === chatId && item.id.startsWith("question:") && !item.resolved && Date.parse(item.at) <= Date.parse(before);
    if (!this.#items.some(open)) return;
    const at = new Date(this.#now()).toISOString();
    this.#items = this.#items.map(item => open(item) ? { ...item, read: true, resolved: true, resolvedAt: at } : item);
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
