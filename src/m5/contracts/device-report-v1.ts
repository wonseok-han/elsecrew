export const DEVICE_REPORT_CHANNEL = "m5:device-report:v1";
export type DeviceReport = {
  platform: "darwin" | "win32" | "other";
  arch: "arm64" | "x64" | "other";
  osVersion: string;
  memoryBytes: number;
  availableBytes: number | null;
};

export function validateDeviceReport(input: unknown): DeviceReport {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("invalid device report");
  const v = input as Record<string, unknown>;
  if (Object.keys(v).sort().join() !== "arch,availableBytes,memoryBytes,osVersion,platform"
    || !["darwin", "win32", "other"].includes(String(v.platform))
    || !["arm64", "x64", "other"].includes(String(v.arch))
    || typeof v.osVersion !== "string" || !/^[a-zA-Z0-9._+-]{1,128}$/.test(v.osVersion)
    || !Number.isSafeInteger(v.memoryBytes) || (v.memoryBytes as number) <= 0
    || (v.availableBytes !== null && (!Number.isSafeInteger(v.availableBytes) || (v.availableBytes as number) < 0))) {
    throw new Error("invalid device report");
  }
  return v as DeviceReport;
}
