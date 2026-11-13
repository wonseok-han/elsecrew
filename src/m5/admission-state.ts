import type { SystemStatusG5DV1 } from "./contracts/lifecycle-g5d-v1.ts";

export const STORAGE_ADMISSION_POLL_MS = 500;
/** After admission, a light liveness check: if the app's engine (sidecar)
 * stops, the status read fails and the screen says so instead of looking busy. */
export const STORAGE_LIVENESS_POLL_MS = 15000;
export type AdmissionView = "checking" | "verifying" | "ready" | "failed" | "stopped";

interface TimerV1 {
  set(callback: () => void, delayMs: number): ReturnType<typeof setTimeout>;
  clear(timer: ReturnType<typeof setTimeout>): void;
}

/** One-at-a-time, cancellable polling for G5D's deliberately event-free admission state,
 * then a slow liveness check while ready. A failure is final until the app restarts. */
export class StorageAdmissionPoller {
  readonly #getStatus: () => Promise<SystemStatusG5DV1>;
  readonly #onState: (state: AdmissionView) => void;
  readonly #timer: TimerV1;
  #scheduled: ReturnType<typeof setTimeout> | undefined;
  #running = false;
  #active = false;
  #generation = 0;
  #wasReady = false;

  constructor(
    getStatus: () => Promise<SystemStatusG5DV1>,
    onState: (state: AdmissionView) => void,
    timer: TimerV1 = {
      set: (callback, delayMs) => setTimeout(callback, delayMs),
      clear: (timer) => clearTimeout(timer),
    },
  ) {
    this.#getStatus = getStatus;
    this.#onState = onState;
    this.#timer = timer;
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
      if (state === "ready") this.#wasReady = true;
      this.#onState(state === "failed" && this.#wasReady ? "stopped" : state);
      if (state === "verifying" || state === "ready") {
        this.#scheduled = this.#timer.set(() => {
          this.#scheduled = undefined;
          void this.#poll(generation);
        }, state === "verifying" ? STORAGE_ADMISSION_POLL_MS : STORAGE_LIVENESS_POLL_MS);
      }
    } catch {
      // Failing after it once worked means the engine stopped mid-use.
      if (this.#active && generation === this.#generation) this.#onState(this.#wasReady ? "stopped" : "failed");
    } finally {
      this.#running = false;
    }
  }
}
