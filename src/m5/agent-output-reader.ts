import { validateAgentRunBindingV1, type AgentRunBindingV1 } from "./contracts/agent-adapter-v1.ts";
import {
  decodeOutputPage, validateOutputHead, OUTPUT_PAGE_MAX_ITEMS,
  type OutputHead, type OutputPageRequest, type OutputRow,
} from "./contracts/agent-output-pages.ts";

/** Subscription resolves only after registration. Reconnection must create a
 * new reader; the caller invokes refresh on a bounded reconciliation timer.
 * All operations must settle on abort. No execution authority is conveyed. */
export interface AgentOutputSource {
  subscribe(binding: AgentRunBindingV1, notify: () => void, signal: AbortSignal): Promise<() => void>;
  head(binding: AgentRunBindingV1, signal: AbortSignal): Promise<unknown>;
  page(request: OutputPageRequest, signal: AbortSignal): Promise<Uint8Array>;
}

/** One request in flight and one page in memory. The consumer must keep its
 * own bounded display window. Cursor advances only after consumer acceptance. */
export class AgentOutputReader {
  readonly #binding: AgentRunBindingV1;
  readonly #source: AgentOutputSource;
  readonly #accept: (rows: readonly OutputRow[], signal: AbortSignal) => Promise<void>;
  readonly #onError: (error: unknown) => void;
  readonly #abort = new AbortController();
  #unsubscribe: (() => void) | undefined;
  #started = false;
  #ready = false;
  #dirty = false;
  #pending: Promise<void> | undefined;
  #cursor = 0;
  #head: OutputHead | undefined;

  constructor(source: AgentOutputSource, binding: AgentRunBindingV1,
    accept: (rows: readonly OutputRow[], signal: AbortSignal) => Promise<void>,
    onError: (error: unknown) => void) {
    this.#source = source;
    this.#binding = { ...validateAgentRunBindingV1(binding) };
    this.#accept = accept;
    this.#onError = onError;
  }

  get cursor(): number { return this.#cursor; }

  async start(): Promise<void> {
    if (this.#started || this.#abort.signal.aborted) throw new Error("output reader already started or stopped");
    this.#started = true;
    try {
      const unsubscribe = await this.#source.subscribe(this.#binding, () => {
        void this.refresh().catch(() => { /* refresh reports once and closes */ });
      }, this.#abort.signal);
      if (this.#abort.signal.aborted) { unsubscribe(); return; }
      this.#unsubscribe = unsubscribe;
      this.#ready = true;
      await this.refresh();
    } catch (error) {
      if (!this.#abort.signal.aborted) { this.stop(); this.#onError(error); }
      throw error;
    }
  }

  stop(): void {
    this.#abort.abort();
    this.#ready = false;
    this.#dirty = false;
    this.#unsubscribe?.();
    this.#unsubscribe = undefined;
  }

  refresh(): Promise<void> {
    if (this.#abort.signal.aborted) return Promise.resolve();
    this.#dirty = true;
    if (!this.#ready) return Promise.resolve();
    if (!this.#pending) {
      this.#pending = Promise.resolve().then(() => this.#drain()).catch(error => {
        if (!this.#abort.signal.aborted) { this.stop(); this.#onError(error); }
        throw error;
      }).finally(() => {
        this.#pending = undefined;
        // A notice may arrive after drain resolves but before this finalizer.
        if (this.#dirty && !this.#abort.signal.aborted) void this.refresh().catch(() => {});
      });
    }
    return this.#pending;
  }

  async #drain(): Promise<void> {
    const signal = this.#abort.signal;
    while (this.#dirty && !signal.aborted) {
      this.#dirty = false;
      const raw = await this.#source.head(this.#binding, signal);
      if (signal.aborted) return;
      const head = validateOutputHead(raw, this.#binding);
      if (head.committedSequence < this.#cursor
        || (this.#head && (head.committedSequence < this.#head.committedSequence
          || (this.#head.sealed && (!head.sealed || head.committedSequence !== this.#head.committedSequence))))) {
        throw new Error("output head regressed or changed after sealing");
      }
      this.#head = head;
      let progressed = false;
      while (this.#cursor < head.committedSequence && !signal.aborted) {
        const request: OutputPageRequest = {
          formatVersion: 1, binding: this.#binding, afterSequence: this.#cursor,
          throughSequence: head.committedSequence, limit: OUTPUT_PAGE_MAX_ITEMS,
        };
        const bytes = await this.#source.page(request, signal);
        if (signal.aborted) return;
        const page = decodeOutputPage(bytes, request);
        await this.#accept(page.rows, signal);
        if (signal.aborted) return;
        this.#cursor = page.nextAfterSequence;
        progressed = true;
      }
      // Re-read after catch-up even when the last commit produced no notice.
      if (progressed) this.#dirty = true;
    }
  }
}
