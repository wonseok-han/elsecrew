import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";

declare global {
  interface Window {
    readonly m5: Readonly<M5BridgeV6>;
  }
}

export {};
