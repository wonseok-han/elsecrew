import type { M5BridgeV4 } from "./contracts/bridge-v4.ts";

declare global {
  interface Window {
    readonly m5: Readonly<M5BridgeV4>;
  }
}

export {};
