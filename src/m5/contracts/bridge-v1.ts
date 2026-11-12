import type {
  EventV1,
  PrepareQuitResultV1,
} from "./protocol-v1.ts";
import type { SystemStatusV1 } from "./lifecycle-v1.ts";

export type { SystemStatusV1 } from "./lifecycle-v1.ts";

export type UnsubscribeV1 = () => void;

export const M5_BRIDGE_V1_METHODS = [
  "getSystemStatus",
  "prepareQuit",
  "onSidecarEvent",
] as const;

export interface M5BridgeV1 {
  readonly version: 1;
  getSystemStatus(): Promise<SystemStatusV1>;
  /** Starts the main-owned quit coordination; this is not a readiness probe. */
  prepareQuit(): Promise<PrepareQuitResultV1>;
  onSidecarEvent(listener: (event: EventV1) => void): UnsubscribeV1;
}
