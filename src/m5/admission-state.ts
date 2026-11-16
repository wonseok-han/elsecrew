import type { SystemStatusG5DV1 } from "./contracts/lifecycle-g5d-v1.ts";

export const STORAGE_ADMISSION_POLL_MS = 500;
/** After admission, a light liveness check: if the app's engine (sidecar)
 * stops, the status read fails and the screen says so instead of looking busy. */
export const STORAGE_LIVENESS_POLL_MS = 15000;
/** While stopped, how often to look for the engine the app starts again. */
export const STORAGE_RECONNECT_POLL_MS = 3000;
export type AdmissionView = "checking" | "verifying" | "ready" | "failed" | "stopped";

interface TimerV1 {
  set(callback: () => void, delayMs: number): ReturnType<typeof setTimeout>;
  clear(timer: ReturnType<typeof setTimeout>): void;
}

/** One-at-a-time, cancellable polling for G5D's deliberately event-free admission state,
 * then a slow liveness check while ready. After the engine stopped mid-use it
 * keeps looking, because the app starts a new engine by itself; when that one
 * is ready, `onRecovered` runs once. A failure before the first ready is final. */
export class StorageAdmissionPoller {
  readonly #getStatus: () => Promise<SystemStatusG5DV1>;
  readonly #onState: (state: AdmissionView) => void;
  readonly #timer: TimerV1;
  #scheduled: ReturnType<typeof setTimeout> | undefined;
  #running = false;
  #active = false;
  #generation = 0;
  #wasReady = false;
  #stopped = false;
  readonly #onRecovered: (() => void) | undefined;

  constructor(
    getStatus: () => Promise<SystemStatusG5DV1>,
    onState: (state: AdmissionView) => void,
    timer: TimerV1 = {
      set: (callback, delayMs) => setTimeout(callback, delayMs),
      clear: (timer) => clearTimeout(timer),
    },
    onRecovered?: () => void,
  ) {
    this.#getStatus = getStatus;
    this.#onState = onState;
    this.#timer = timer;
    this.#onRecovered = onRecovered;
  }

  #schedule(generation: number, delayMs: number): void {
    this.#scheduled = this.#timer.set(() => {
      this.#scheduled = undefined;
      void this.#poll(generation);
    }, delayMs);
  }

  start(): void {
    if (this.#active) return;
    this.#active = true;
    this.#onState("checking");
    void this.#poll(this.#generation);
  }

  stop(): void {
    this.#active = false;
    this.#generation += 1;
    if (this.#scheduled) this.#timer.clear(this.#scheduled);
    this.#scheduled = undefined;
  }

  async #poll(generation: number): Promise<void> {
    if (!this.#active || this.#running || generation !== this.#generation) return;
    this.#running = true;
    try {
      const status = await this.#getStatus();
      if (!this.#active || generation !== this.#generation) return;
      const state = status.storageAdmission.state;
      const stopped = state === "failed" && this.#wasReady;
      if (state === "ready") {
        this.#wasReady = true;
        if (this.#stopped) { this.#stopped = false; this.#onRecovered?.(); }
      }
      if (stopped) this.#stopped = true;
      this.#onState(stopped ? "stopped" : state);
      if (state === "verifying" || state === "ready") {
        this.#schedule(generation, state === "verifying" ? STORAGE_ADMISSION_POLL_MS : STORAGE_LIVENESS_POLL_MS);
      } else if (stopped) {
        this.#schedule(generation, STORAGE_RECONNECT_POLL_MS);
      }
    } catch {
      // Failing after it once worked means the engine stopped mid-use; the
      // app starts it again, so keep looking.
      if (this.#active && generation === this.#generation) {
        if (this.#wasReady) {
          this.#stopped = true;
          this.#onState("stopped");
          this.#schedule(generation, STORAGE_RECONNECT_POLL_MS);
        } else {
          this.#onState("failed");
        }
      }
    } finally {
      this.#running = false;
    }
  }
}
