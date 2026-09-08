import type { SystemStatusG5DV1 } from "./contracts/lifecycle-g5d-v1.ts";

export const STORAGE_ADMISSION_POLL_MS = 500;
export type AdmissionView = "checking" | "verifying" | "ready" | "failed";

interface TimerV1 {
  set(callback: () => void, delayMs: number): ReturnType<typeof setTimeout>;
  clear(timer: ReturnType<typeof setTimeout>): void;
}

/** One-at-a-time, cancellable polling for G5D's deliberately event-free admission state. */
export class StorageAdmissionPoller {
  readonly #getStatus: () => Promise<SystemStatusG5DV1>;
  readonly #onState: (state: AdmissionView) => void;
  readonly #timer: TimerV1;
  #scheduled: ReturnType<typeof setTimeout> | undefined;
  #running = false;
  #active = false;
  #generation = 0;

  constructor(
    getStatus: () => Promise<SystemStatusG5DV1>,
    onState: (state: AdmissionView) => void,
    timer: TimerV1 = { set: setTimeout, clear: clearTimeout },
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
      this.#onState(state);
      if (state === "verifying") {
        this.#scheduled = this.#timer.set(() => {
          this.#scheduled = undefined;
          void this.#poll(generation);
        }, STORAGE_ADMISSION_POLL_MS);
      }
    } catch {
      if (this.#active && generation === this.#generation) this.#onState("failed");
    } finally {
      this.#running = false;
    }
  }
}
