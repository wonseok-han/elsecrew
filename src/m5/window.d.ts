import type { M5BridgeV7 } from "./contracts/bridge-v7.ts";

declare global {
  interface Window {
    readonly m5Device?: { inspect(): Promise<import("./contracts/device-report-v1.ts").DeviceReport> };
    readonly m5Approval?: { request(input: import("./contracts/approval-v1.ts").ApprovalRequest): Promise<import("./contracts/approval-v1.ts").ApprovalResult> };
    readonly m5Projects?: { request(input: import("./contracts/project-catalog-v1.ts").ProjectCatalogRequest): Promise<import("./contracts/project-catalog-v1.ts").ProjectCatalogResult> };
    readonly m5Login?: { request(input: import("./contracts/provider-login-v1.ts").LoginRequest): Promise<import("./contracts/provider-login-v1.ts").LoginStatus> };
    readonly m5: Readonly<M5BridgeV7>;
  }
}

export {};
