import type { M5BridgeV7 } from "./contracts/bridge-v7.ts";

declare global {
  interface Window {
    readonly m5Login?: { request(input: import("./contracts/provider-login-v1.ts").LoginRequest): Promise<import("./contracts/provider-login-v1.ts").LoginStatus> };
    readonly m5: Readonly<M5BridgeV7>;
  }
}

export {};
