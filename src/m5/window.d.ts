import type { M5BridgeV7 } from "./contracts/bridge-v7.ts";

declare global {
  interface Window {
    readonly m5: Readonly<M5BridgeV7>;
  }
}

export {};
