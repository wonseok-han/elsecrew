import type { M5BridgeV1 } from "./contracts/bridge-v1.ts";

declare global {
  interface Window {
    readonly m5: Readonly<M5BridgeV1>;
  }
}

export {};
