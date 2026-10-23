import type { M5BridgeV8 } from "./contracts/bridge-v8.ts";

declare global {
  interface Window {
    readonly m5Diagnostics?: { inspect(): Promise<import("./contracts/session-diagnostics-v1.ts").SessionDiagnostics> };
    readonly m5Install?: { request(input: import("./contracts/provider-install-v1.ts").InstallRequest): Promise<import("./contracts/provider-install-v1.ts").InstallStatus> };
    readonly m5Device?: { inspect(): Promise<import("./contracts/device-report-v1.ts").DeviceReport> };
    readonly m5Approval?: { request(input: import("./contracts/approval-v1.ts").ApprovalRequest): Promise<import("./contracts/approval-v1.ts").ApprovalResult> };
    readonly m5Projects?: { request(input: import("./contracts/project-catalog-v1.ts").ProjectCatalogRequest): Promise<import("./contracts/project-catalog-v1.ts").ProjectCatalogResult> };
    readonly m5Login?: { request(input: import("./contracts/provider-login-v1.ts").LoginRequest): Promise<import("./contracts/provider-login-v1.ts").LoginStatus> };
    readonly m5: Readonly<M5BridgeV8>;
  }
}

export {};
