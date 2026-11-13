import type { PrepareQuitResultV1 } from "../src/m5/contracts/protocol-v1.ts";

export type ExitKindV1 = "clean" | "uncleanStartupFailure" | "uncleanUserConfirmed";

/** Recovery the ledger already keeps (`run:`/`lock:` from storage). Unlike a
 * live run, login or install, it survives a restart, so it never needs to
 * keep the app open: the next start simply shows it again. */
export function onlyStoredRecovery(blockers: readonly string[]): boolean {
  return blockers.length > 0 && blockers.every((blocker) => blocker.startsWith("run:") || blocker.startsWith("lock:"));
}

export interface QuitCoordinatorOptionsV1 {
  prepareSidecarQuit(): Promise<PrepareQuitResultV1>;
  failedBeforeReady(): boolean;
  allowProcessExit(kind: ExitKindV1): void;
  notifyQuitBlocked?(reason: "recoveryRequired" | "unavailable"): void | Promise<void>;
  /** Asked when nothing is live any more (only stored recovery, or the task
   * service already stopped). True lets the app close without a clean mark,
   * so the next start runs recovery. */
  confirmUncleanQuit?(reason: "storedRecovery" | "unavailable"): Promise<boolean>;
}

/** Main-owned only: renderer calls cannot turn an unavailable startup into clean exit. */
export class QuitCoordinatorV1 {
  readonly #options: QuitCoordinatorOptionsV1;
  #exitAllowed = false;
  #inFlight: Promise<PrepareQuitResultV1> | undefined;
  #handlingBeforeQuit = false;
  #unavailableNotified = false;

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
          || blocker === "provider-login:claude" || blocker === "provider-install")) return result;
      await new Promise<void>((resolve) => setTimeout(resolve, 250));
    }
  }

  #allowUnclean(): void {
    this.#exitAllowed = true;
    this.#options.allowProcessExit("uncleanUserConfirmed");
  }

  handleBeforeQuit(): void {
    if (this.#exitAllowed) return;
    if (this.#options.failedBeforeReady()) {
      // Do not call the sidecar or write a clean marker. Its next start must recover.
      this.#exitAllowed = true;
      this.#options.allowProcessExit("uncleanStartupFailure");
      return;
    }
    if (this.#handlingBeforeQuit) return;
    this.#handlingBeforeQuit = true;
    void this.prepareQuit().then(
      async (result) => {
        if (result.safeToQuit) return;
        if (onlyStoredRecovery(result.blockers) && await this.#options.confirmUncleanQuit?.("storedRecovery")) {
          this.#allowUnclean();
          return;
        }
        if (!onlyStoredRecovery(result.blockers)) await this.#options.notifyQuitBlocked?.("recoveryRequired");
      },
      async () => {
        // No raw provider error or credential-bearing stderr reaches the dialog.
        // A disconnected sidecar is not restarted in this session; nothing of
        // it is left to wait for, so the user may still close the app.
        if (this.#options.confirmUncleanQuit) {
          if (await this.#options.confirmUncleanQuit("unavailable")) this.#allowUnclean();
          return;
        }
        if (!this.#unavailableNotified) {
          await this.#options.notifyQuitBlocked?.("unavailable");
          this.#unavailableNotified = true;
        }
      },
    ).catch(() => {
      // A failed notification never grants permission to exit.
    }).finally(() => { this.#handlingBeforeQuit = false; });
  }
}
