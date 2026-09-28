import type { PrepareQuitResultV1 } from "../src/m5/contracts/protocol-v1.ts";

export type ExitKindV1 = "clean" | "uncleanStartupFailure";

export interface QuitCoordinatorOptionsV1 {
  prepareSidecarQuit(): Promise<PrepareQuitResultV1>;
  failedBeforeReady(): boolean;
  allowProcessExit(kind: ExitKindV1): void;
}

/** Main-owned only: renderer calls cannot turn an unavailable startup into clean exit. */
export class QuitCoordinatorV1 {
  readonly #options: QuitCoordinatorOptionsV1;
  #exitAllowed = false;
  #inFlight: Promise<PrepareQuitResultV1> | undefined;

  constructor(options: QuitCoordinatorOptionsV1) {
    this.#options = options;
  }

  get exitAllowed(): boolean {
    return this.#exitAllowed;
  }

  prepareQuit(): Promise<PrepareQuitResultV1> {
    if (this.#exitAllowed) return Promise.resolve({ safeToQuit: true, blockers: [] });
    if (this.#inFlight) return this.#inFlight;
    this.#inFlight = this.#prepareWithLoginCleanup().then((result) => {
      if (!result.safeToQuit) return result;
      this.#exitAllowed = true;
      this.#options.allowProcessExit("clean");
      return result;
    });
    void this.#inFlight.catch(() => {
      // An error is unsafe and is deliberately not retried.
    }).finally(() => {
      if (!this.#exitAllowed) this.#inFlight = undefined;
    });
    return this.#inFlight;
  }

  async #prepareWithLoginCleanup(): Promise<PrepareQuitResultV1> {
    // Cancellation is asynchronous. Recheck only login blockers, with a finite
    // budget; neither a timeout nor a recovery fence grants permission to exit.
    for (let attempt = 0; ; attempt += 1) {
      const result = await this.#options.prepareSidecarQuit();
      if (result.safeToQuit || attempt >= 40 || result.blockers.length === 0
        || !result.blockers.every((blocker) => blocker === "provider-login:codex"
          || blocker === "provider-login:claude")) return result;
      await new Promise<void>((resolve) => setTimeout(resolve, 250));
    }
  }

  handleBeforeQuit(): void {
    if (this.#exitAllowed) return;
    if (this.#options.failedBeforeReady()) {
      // Do not call the sidecar or write a clean marker. Its next start must recover.
      this.#exitAllowed = true;
      this.#options.allowProcessExit("uncleanStartupFailure");
      return;
    }
    void this.prepareQuit().catch(() => {
      // The before-quit handler has prevented exit; an unsafe error keeps the app alive.
    });
  }
}
