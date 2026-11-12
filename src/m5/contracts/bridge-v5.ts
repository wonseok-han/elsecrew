import type { AgentRunStateChangedV1 } from "./agent-adapter-v1.ts";
import { M5_BRIDGE_V4_METHODS, type M5BridgeV4 } from "./bridge-v4.ts";

/** Planned G5B surface. V3/V4 remain valid until G5B is negotiated end to end. */
export const M5_BRIDGE_V5_METHODS = [
  ...M5_BRIDGE_V4_METHODS,
  "onAgentRunStateChanged",
] as const;

export type M5BridgeV5 = Omit<M5BridgeV4, "version"> & {
  readonly version: 5;
  /** Notification only: renderer must hydrate the durable chat/run ledger before rendering state. */
  onAgentRunStateChanged(listener: (event: AgentRunStateChangedV1) => void): () => void;
};
