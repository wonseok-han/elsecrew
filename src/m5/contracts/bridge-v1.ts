import type {
  EventV1,
  PrepareQuitResultV1,
  ReadyPayloadV1,
} from "./protocol-v1.ts";

export interface SystemStatusV1 {
  ready: boolean;
  sidecar: ReadyPayloadV1;
}

export type UnsubscribeV1 = () => void;

export const M5_BRIDGE_V1_METHODS = [
  "getSystemStatus",
  "prepareQuit",
  "onSidecarEvent",
] as const;

export interface M5BridgeV1 {
  readonly version: 1;
  getSystemStatus(): Promise<SystemStatusV1>;
  prepareQuit(): Promise<PrepareQuitResultV1>;
  onSidecarEvent(listener: (event: EventV1) => void): UnsubscribeV1;
}
