import { M5_BRIDGE_V6_METHODS, type M5BridgeV6 } from "./bridge-v6.ts";
import type { ChatAppendMessageRequestV1, TeamMessageV1 } from "./run-recovery-v1.ts";

export const M5_BRIDGE_V7_METHODS = [...M5_BRIDGE_V6_METHODS, "appendChatMessage"] as const;
export type M5BridgeV7 = Omit<M5BridgeV6, "version"> & {
  readonly version: 7;
  appendChatMessage(input: ChatAppendMessageRequestV1): Promise<TeamMessageV1>;
};
