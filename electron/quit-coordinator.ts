import type { PrepareQuitResultV1 } from "../src/m5/contracts/protocol-v1.ts";

export type ExitKindV1 = "clean" | "uncleanStartupFailure" | "uncleanQuit";

/** Recovery the ledger already keeps (`run:`/`lock:` from storage). Unlike a
 * live run, login or install, waiting does not change it: the next start
 * shows it again. */
export function onlyStoredRecovery(blockers: readonly string[]): boolean {
  return blockers.length > 0 && blockers.every((blocker) => blocker.startsWith("run:") || blocker.startsWith("lock:"));
}

/** How long a quit waits for running work to stop before closing anyway. */
export const QUIT_WAIT_ATTEMPTS = 40;
export const QUIT_WAIT_INTERVAL_MS = 250;
/** A task service that stops answering never holds the app open. */
export const QUIT_HARD_DEADLINE_MS = 15_000;

export interface QuitCoordinatorOptionsV1 {
  prepareSidecarQuit(): Promise<PrepareQuitResultV1>;
  failedBeforeReady(): boolean;
  allowProcessExit(kind: ExitKindV1): void;
  /** The person asked to quit: the app may look closed right away while
   * running work stops in the background. */
  onQuitStarted?(): void;
}

/** Main-owned only: renderer calls cannot turn an unavailable startup into clean exit.
 * Quitting is never refused: running work is asked to stop and given a short
 * time to do so; whatever is still open is left for the next start to
 * recover (the engine also ends its agent processes when the app closes). */
export class QuitCoordinatorV1 {
  readonly #options: QuitCoordinatorOptionsV1;
  #exitAllowed = false;
  #inFlight: Promise<PrepareQuitResultV1> | undefined;
  #quitting = false;

  constructor(options: QuitCoordinatorOptionsV1) {
    this.#options = options;
  }

  get exitAllowed(): boolean {
    return this.#exitAllowed;
  }

  /** Stops running work and reports whether the app can close cleanly.
   * Only a clean result allows exit here (an update installs only then). */
  prepareQuit(): Promise<PrepareQuitResultV1> {
    if (this.#exitAllowed) return Promise.resolve({ safeToQuit: true, blockers: [] });
    if (this.#inFlight) return this.#inFlight;
    this.#inFlight = this.#prepareUntilStopped().then((result) => {
      if (!result.safeToQuit || this.#exitAllowed) return result;
      this.#exitAllowed = true;
      this.#options.allowProcessExit("clean");
      return result;
    });
    void this.#inFlight.catch(() => {
      // An error is never a clean result.
    }).finally(() => {
      if (!this.#exitAllowed) this.#inFlight = undefined;
    });
    return this.#inFlight;
  }

  async #prepareUntilStopped(): Promise<PrepareQuitResultV1> {
    // Stopping is asynchronous: ask again while live work (a run, start,
    // discovery, login or install) is still ending. Stored records never change by waiting.
    for (let attempt = 0; ; attempt += 1) {
      const result = await this.#options.prepareSidecarQuit();
      if (result.safeToQuit || attempt >= QUIT_WAIT_ATTEMPTS || result.blockers.length === 0
        || onlyStoredRecovery(result.blockers)) return result;
      await new Promise<void>((resolve) => setTimeout(resolve, QUIT_WAIT_INTERVAL_MS));
    }
  }

  #exitUnclean(kind: ExitKindV1): void {
    if (this.#exitAllowed) return;
    this.#exitAllowed = true;
    this.#options.allowProcessExit(kind);
  }

  handleBeforeQuit(): void {
    if (this.#exitAllowed) return;
    if (this.#options.failedBeforeReady()) {
      // Do not call the sidecar or write a clean marker. Its next start must recover.
      this.#exitUnclean("uncleanStartupFailure");
      return;
    }
    if (this.#quitting) return;
    this.#quitting = true;
    try { this.#options.onQuitStarted?.(); } catch { /* closing the window is cosmetic */ }
    let timer: ReturnType<typeof setTimeout> | undefined;
    const deadline = new Promise<"deadline">((resolve) => { timer = setTimeout(() => resolve("deadline"), QUIT_HARD_DEADLINE_MS); });
    void Promise.race([this.prepareQuit(), deadline]).then(
      (result) => { if (result === "deadline" || !result.safeToQuit) this.#exitUnclean("uncleanQuit"); },
      () => this.#exitUnclean("uncleanQuit"),
    ).finally(() => clearTimeout(timer));
  }
}
