import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";

type EventCreditIdentity = Pick<EventV1, "streamId" | "seq">;

export const MAX_IN_FLIGHT_EVENTS = 32;
export const MAX_GLOBAL_IN_FLIGHT_EVENTS = 128;

export class EventCreditV1 {
  readonly #pending = new Map<string, EventCreditIdentity[]>();
  readonly #fail: (error: Error) => void;
  #total = 0;
  #paused = false;
  constructor(fail: (error: Error) => void) { this.#fail = fail; }
  /** While the page (re)loads nothing can acknowledge: drop deliveries, the
   * new page reads its state afresh. */
  pause(): void { this.reset(); this.#paused = true; }
  resume(): void { this.reset(); this.#paused = false; }
  deliver(event: EventCreditIdentity, send: () => void): void {
    if (this.#paused) return;
    if (this.#total >= MAX_GLOBAL_IN_FLIGHT_EVENTS || (this.#pending.get(event.streamId)?.length ?? 0) >= MAX_IN_FLIGHT_EVENTS) {
      this.#fail(new Error("consumerTooSlow: renderer event credit exhausted")); return;
    }
    const queue = this.#pending.get(event.streamId) ?? []; queue.push(event); this.#pending.set(event.streamId, queue); this.#total += 1; send();
  }
  acknowledge(value: unknown): void {
    if (typeof value !== "object" || value === null || Array.isArray(value)) return this.#fail(new Error("consumerTooSlow: invalid renderer event acknowledgement"));
    const ack = value as Record<string, unknown>;
    if (Object.keys(ack).length !== 2 || typeof ack.streamId !== "string" || !Number.isSafeInteger(ack.seq)) return this.#fail(new Error("consumerTooSlow: invalid renderer event acknowledgement"));
    if (this.#paused) return;
    const queue = this.#pending.get(ack.streamId); const event = queue?.[0];
    // A stream with nothing outstanding here belongs to an engine or page that
    // was replaced (or was already settled): its late acknowledgement is ignored,
    // as is one for an event older than the oldest outstanding one.
    if (!queue || (event && (ack.seq as number) < event.seq)) return;
    if (!event || event.seq !== ack.seq) return this.#fail(new Error("consumerTooSlow: unknown, duplicate, or out-of-order acknowledgement"));
    queue!.shift(); this.#total -= 1; if (queue!.length === 0) this.#pending.delete(ack.streamId);
  }
  get outstanding(): number { return this.#total; }
  resetIfEmpty(): boolean { if (this.#total !== 0) return false; this.#pending.clear(); return true; }
  /** A new engine starts with a clean slate. */
  reset(): void { this.#pending.clear(); this.#total = 0; }
}
