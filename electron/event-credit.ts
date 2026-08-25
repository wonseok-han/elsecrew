import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";

export const MAX_IN_FLIGHT_EVENTS = 32;
export const MAX_GLOBAL_IN_FLIGHT_EVENTS = 128;

export class EventCreditV1 {
  readonly #pending = new Map<string, EventV1[]>();
  readonly #fail: (error: Error) => void;
  #total = 0;
  constructor(fail: (error: Error) => void) { this.#fail = fail; }
  deliver(event: EventV1, send: () => void): void {
    if (this.#total >= MAX_GLOBAL_IN_FLIGHT_EVENTS || (this.#pending.get(event.streamId)?.length ?? 0) >= MAX_IN_FLIGHT_EVENTS) {
      this.#fail(new Error("consumerTooSlow: renderer event credit exhausted")); return;
    }
    const queue = this.#pending.get(event.streamId) ?? []; queue.push(event); this.#pending.set(event.streamId, queue); this.#total += 1; send();
  }
  acknowledge(value: unknown): void {
    if (typeof value !== "object" || value === null || Array.isArray(value)) return this.#fail(new Error("consumerTooSlow: invalid renderer event acknowledgement"));
    const ack = value as Record<string, unknown>;
    if (Object.keys(ack).length !== 2 || typeof ack.streamId !== "string" || !Number.isSafeInteger(ack.seq)) return this.#fail(new Error("consumerTooSlow: invalid renderer event acknowledgement"));
    const queue = this.#pending.get(ack.streamId); const event = queue?.[0];
    if (!event || event.seq !== ack.seq) return this.#fail(new Error("consumerTooSlow: unknown, duplicate, or out-of-order acknowledgement"));
    queue!.shift(); this.#total -= 1; if (queue!.length === 0) this.#pending.delete(ack.streamId);
  }
  get outstanding(): number { return this.#total; }
  resetIfEmpty(): boolean { if (this.#total !== 0) return false; this.#pending.clear(); return true; }
}
