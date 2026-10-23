/** Read-only durable output extension. Does not authorize a new run format. */
import { M5_BRIDGE_V7_METHODS, type M5BridgeV7 } from "./bridge-v7.ts";
import type { AgentRunBindingV1 } from "./agent-adapter-v1.ts";
import type { OutputHead, OutputPage, OutputPageRequest } from "./agent-output-pages.ts";

export const M5_BRIDGE_V8_METHODS = [...M5_BRIDGE_V7_METHODS, "getAgentOutputHead", "getAgentOutputPage"] as const;
export type M5BridgeV8 = Omit<M5BridgeV7, "version"> & {
  readonly version: 8;
  getAgentOutputHead(input: AgentRunBindingV1): Promise<OutputHead | null>;
  getAgentOutputPage(input: OutputPageRequest): Promise<OutputPage>;
};
