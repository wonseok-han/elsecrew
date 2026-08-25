import type { M5BridgeV2 } from "./contracts/bridge-v2.ts";

declare global {
  interface Window {
    readonly m5: Readonly<M5BridgeV2>;
  }
}

export {};
