/** Native updater policy. No renderer-supplied feed, path, or executable. */
export type UpdateState = "unavailable" | "idle" | "current" | "checking" | "downloading" | "ready" | "restarting" | "error";
export interface NativeUpdater {
  setFeedURL(options: { url: string }): void;
  checkForUpdates(): void;
  quitAndInstall(): void;
  on(event: string, listener: (...args: unknown[]) => void): unknown;
}
export class AppUpdateController {
  state: UpdateState = "unavailable";
  readonly #native: NativeUpdater;
  readonly #restart: (install: () => void) => Promise<boolean>;
  readonly #changed: () => void;
  constructor(native: NativeUpdater, restart: (install: () => void) => Promise<boolean>, changed: () => void) {
    this.#native = native; this.#restart = restart; this.#changed = changed;
    native.on("error", () => { if (this.state !== "unavailable") this.#set("error"); });
    native.on("update-available", () => { if (this.state === "checking") this.#set("downloading"); });
    native.on("update-not-available", () => { if (this.state === "checking") this.#set("current"); });
    native.on("update-downloaded", () => {
      if (this.state === "checking" || this.state === "downloading") this.#set("ready");
    });
  }
  configure(feed: string | null, supportedSignedPackage: boolean): void {
    if (this.state !== "unavailable" || !supportedSignedPackage || !feed) return;
    try {
      const url = new URL(feed);
      if (url.protocol !== "https:" || url.username || url.password || url.hash) return;
      this.#native.setFeedURL({ url: url.href });
      this.#set("idle");
    } catch { /* Invalid release configuration never initiates a network request. */ }
  }
  check(): void {
    if (this.state !== "idle" && this.state !== "current" && this.state !== "error") return;
    this.#set("checking");
    try { this.#native.checkForUpdates(); } catch { this.#set("error"); }
  }
  async restart(): Promise<void> {
    if (this.state !== "ready") return;
    this.#set("restarting");
    try {
      const safe = await this.#restart(() => this.#native.quitAndInstall());
      if (!safe) this.#set("ready");
    } catch { this.#set("ready"); }
  }
  #set(state: UpdateState): void { this.state = state; this.#changed(); }
}
