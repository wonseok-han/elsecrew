/**
 * The order in which the preview view is placed, hidden and shown (PR #9
 * re-review B1·I1). No DOM and no Electron here: the tab gives it how to
 * measure, how to send a layout to main and how to detach the preview, and
 * tests drive it with answers that come late or never
 * (`tests/m5-preview-layout-loop.test.mts`).
 *
 * The rules:
 * - A hide is sent at once, even while another layout is still on its way;
 *   showing waits for every earlier answer.
 * - What main ACCEPTED (the answer to the newest request) is the truth; a
 *   failed request is sent again.
 * - Held app elements (a dialog, the review sheet, a panel over the stage)
 *   and queued layout changes (a band that pushes the stage) are let go only
 *   when main has confirmed the view hidden and no show is on its way.
 * - If that confirmation does not come within the limit, the preview is
 *   detached (closed) and only once main answers the close is anything let go.
 */
import type { PreviewBoundsV1, PreviewPageSizeV1, PreviewViewportV1 } from "./contracts/preview-v1.ts";
import { sameLayout, type LayoutState } from "./preview-tab-model.ts";

export interface LoopLayout extends LayoutState { pixelRatio: number; page: PreviewPageSizeV1 }

export interface LoopMeasure {
  /** The stage in CSS pixels, or null when it is not on screen. */
  bounds: PreviewBoundsV1 | null;
  pixelRatio: number;
  page: PreviewPageSizeV1;
}

export interface PreviewLayoutLoopOptions {
  measure(): LoopMeasure;
  viewport(): PreviewViewportV1;
  /** Covers other than holds: explicit covers, points, another tab, resizing. */
  obscured(): boolean;
  /** Sends one layout; `closed` when main no longer has this preview. */
  send(layout: LoopLayout): Promise<{ closed: boolean; snapshot: string | null }>;
  /** Detaches the preview (main closes it). Settles once main answered. */
  close(): Promise<unknown>;
  /** The still image to show while hidden, or null when shown again. */
  still(image: string | null): void;
  /** Everything held was let go (all at once): look again at what covers the
   * stage before the view may be shown. */
  released?(): void;
  holdLimitMs?: number;
  retryMs?: number;
  /** A request main never answers counts as failed after this long and is sent again. */
  answerLimitMs?: number;
  setTimer?(callback: () => void, ms: number): unknown;
  clearTimer?(timer: unknown): void;
}

export const PREVIEW_HOLD_LIMIT_MS = 1500;
export const PREVIEW_ANSWER_LIMIT_MS = 3000;

export class PreviewLayoutLoop {
  /** The newest layout main accepted. */
  accepted: LoopLayout | null = null;
  #options: PreviewLayoutLoopOptions;
  #lastSent: LoopLayout | null = null;
  #seq = 0;
  #acceptedSeq = 0;
  #inFlight = 0;
  #showsInFlight = 0;
  #releases: (() => void)[] = [];
  #queued: (() => void)[] = [];
  /** A queued change ran; hidden until the page says it is laid out (`settled`). */
  #shifting = false;
  #holdTimer: unknown = null;
  #retryTimer: unknown = null;
  #detaching = false;
  #closed = false;
  #disposed = false;

  constructor(options: PreviewLayoutLoopOptions) { this.#options = options; }

  /** Whether the view may be on screen now or soon (a show on its way). */
  get mayBeShown(): boolean {
    return !this.#closed && (this.#showsInFlight > 0 || this.accepted?.obscured === false);
  }
  get holding(): boolean { return this.#releases.length > 0 || this.#queued.length > 0 || this.#shifting; }
  get detached(): boolean { return this.#closed; }

  /**
   * Keeps something of the app from showing until the view is hidden. If
   * the view cannot be on screen, it is let go at once.
   */
  hold(release: () => void): void {
    if (this.#disposed || this.#closed) { release(); return; }
    this.#releases.push(release);
    this.#startLimit();
    this.sync();
  }

  /** Runs a change that moves the stage only once the view is hidden. */
  afterHidden(change: () => void): void {
    if (this.#disposed || this.#closed) { change(); return; }
    this.#queued.push(change);
    this.#startLimit();
    this.sync();
  }

  /** The page has laid out the queued changes: place the view again. */
  settled(): void {
    if (!this.#shifting) return;
    this.#shifting = false;
    this.sync();
  }

  sync(): void {
    if (this.#disposed || this.#closed) return;
    const measured = this.#options.measure();
    const bounds = measured.bounds ?? this.accepted?.bounds ?? this.#lastSent?.bounds ?? null;
    // Never placed and nowhere to place it: main keeps it hidden.
    if (!bounds) { this.#tryRelease(); return; }
    const obscured = this.#options.obscured() || this.holding || measured.bounds === null;
    const next: LoopLayout = { bounds, viewport: this.#options.viewport(), obscured, pixelRatio: measured.pixelRatio, page: measured.page };
    if (obscured) {
      // Hidden already, or a hide on its way: where the hidden view would
      // sit does not matter (the next show carries a fresh measurement), so
      // nothing more is sent — a stage measured on and off while hidden
      // (a closed drawer) never turns into a stream of requests.
      if (this.#lastSent?.obscured === true && (this.#inFlight > 0 || this.accepted?.obscured === true)) {
        if (this.#inFlight === 0) this.#tryRelease();
        return;
      }
      // A hide never waits behind another request.
      this.#send(next);
      return;
    }
    // Showing waits for every earlier answer.
    if (this.#inFlight > 0) return;
    if (sameLoopLayout(this.accepted, next)) return;
    this.#send(next);
  }

  /** The preview is going away: what is held is let go only once main has detached it. */
  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#clear(this.#retryTimer);
    if (this.#releases.length === 0 && this.#queued.length === 0) { this.#finish(); return; }
    if (this.#closed) { this.#finish(); return; }
    void this.#options.close().then(() => this.#finish(), () => this.#finish());
  }

  #send(next: LoopLayout): void {
    const seq = ++this.#seq;
    this.#inFlight++;
    if (!next.obscured) this.#showsInFlight++;
    const wasObscured = this.accepted?.obscured ?? true;
    this.#lastSent = next;
    let limit: unknown = null;
    const unanswered = new Promise<never>((_resolve, reject) => {
      limit = this.#set(() => reject(new Error("no answer")), this.#options.answerLimitMs ?? PREVIEW_ANSWER_LIMIT_MS);
    });
    Promise.race([this.#options.send(next), unanswered]).finally(() => this.#clear(limit)).then(answer => {
      this.#settle(next);
      if (this.#disposed) return;
      if (answer.closed) { this.#closed = true; this.#finish(); return; }
      if (seq > this.#acceptedSeq) {
        this.#acceptedSeq = seq;
        this.accepted = next;
        if (next.obscured && !wasObscured && answer.snapshot) this.#options.still(answer.snapshot);
        if (!next.obscured) this.#options.still(null);
      }
      this.sync();
      this.#tryRelease();
    }, () => {
      this.#settle(next);
      if (this.#disposed) return;
      this.#clear(this.#retryTimer);
      this.#retryTimer = this.#set(() => { this.#retryTimer = null; this.sync(); }, this.#options.retryMs ?? 250);
    });
  }

  #settle(sent: LoopLayout): void {
    this.#inFlight--;
    if (!sent.obscured) this.#showsInFlight--;
  }

  /** Let go only when main has confirmed the view hidden and no show is on its way. */
  #tryRelease(): void {
    if (this.#releases.length === 0 && this.#queued.length === 0) return;
    if (this.#showsInFlight > 0 || this.#detaching) return;
    const hidden = this.accepted === null || this.accepted.obscured;
    if (!hidden) return;
    // A hide that is still on its way is not a confirmation yet.
    if (this.accepted !== null && this.#lastSent !== null && this.#inFlight > 0) return;
    this.#letGo();
  }

  #letGo(): void {
    this.#clear(this.#holdTimer);
    this.#holdTimer = null;
    const releases = this.#releases.splice(0);
    const queued = this.#queued.splice(0);
    if (queued.length > 0 && !this.#disposed && !this.#closed) this.#shifting = true;
    for (const release of releases) release();
    for (const change of queued) change();
    // Only once ALL of them are back: one held element must not be judged
    // while another is still invisible (the two would show the view in turn).
    if (releases.length > 0) this.#options.released?.();
    // Shown again if nothing else covers it (a band waits for `settled`).
    this.sync();
  }

  #startLimit(): void {
    if (this.#holdTimer !== null) return;
    this.#holdTimer = this.#set(() => {
      this.#holdTimer = null;
      if (!this.holding || this.#detaching) return;
      // No confirmation in time: detach the preview, and let go only once
      // main has answered the close — never on a guess.
      this.#detaching = true;
      void this.#options.close().then(() => this.#detached(), () => this.#detached());
    }, this.#options.holdLimitMs ?? PREVIEW_HOLD_LIMIT_MS);
  }

  #detached(): void {
    this.#closed = true;
    this.#detaching = false;
    this.#finish();
  }

  #finish(): void {
    this.#shifting = false;
    this.#letGo();
    this.#shifting = false;
  }

  #set(callback: () => void, ms: number): unknown {
    return this.#options.setTimer ? this.#options.setTimer(callback, ms) : setTimeout(callback, ms);
  }
  #clear(timer: unknown): void {
    if (timer === null) return;
    if (this.#options.clearTimer) this.#options.clearTimer(timer);
    else clearTimeout(timer as ReturnType<typeof setTimeout>);
  }
}

/** Same place, width, cover state, page size and ratio. */
export function sameLoopLayout(a: LoopLayout | null, b: LoopLayout): boolean {
  return a !== null && sameLayout(a, b) && a.pixelRatio === b.pixelRatio && a.page.width === b.page.width && a.page.height === b.page.height;
}
