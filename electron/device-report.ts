import { statfs } from "node:fs/promises";
import { release, totalmem } from "node:os";
import { validateDeviceReport, type DeviceReport } from "../src/m5/contracts/device-report-v1.ts";

// Main owns the path. No renderer-provided paths, shell probes, or provider calls.
export async function readDeviceReport(appDataDirectory: string): Promise<DeviceReport> {
  let availableBytes: number | null = null;
  try {
    const space = await statfs(appDataDirectory, { bigint: true });
    const available = space.bavail * space.bsize;
    if (available >= 0n && available <= BigInt(Number.MAX_SAFE_INTEGER)) availableBytes = Number(available);
  } catch { /* Unknown space is not zero and is never installation admission. */ }
  return validateDeviceReport({
    platform: process.platform === "darwin" || process.platform === "win32" ? process.platform : "other",
    arch: process.arch === "arm64" || process.arch === "x64" ? process.arch : "other",
    osVersion: release(),
    memoryBytes: totalmem(),
    availableBytes,
  });
}
