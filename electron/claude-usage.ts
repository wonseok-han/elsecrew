//! Privileged, read-only quota lookup. Never returns or logs credentials.
import { constants } from "node:fs";
import { lstat, open, realpath } from "node:fs/promises";
import { join } from "node:path";
import type { ClaudeUsageResult, ProviderUsage } from "../src/m5/contracts/provider-usage-v1.ts";

export const CLAUDE_USAGE_URL = "https://api.anthropic.com/api/oauth/usage";
const MAX_BYTES = 64 * 1024;
export function validCredentialFile(stat: {isFile(): boolean; uid: number; mode: number; size: number; nlink: number}, uid: number | undefined): boolean {
  return stat.isFile() && stat.uid === uid && (stat.mode & 0o777) === 0o600 && stat.nlink === 1 && stat.size <= MAX_BYTES;
}
export function parseClaudeQuota(value: unknown): ProviderUsage {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid quota");
  const record = value as Record<string, unknown>;
  const windows: ProviderUsage["windows"] = [];
  for (const [key, id, minutes] of [["five_hour","primary",300],["seven_day","secondary",10080]] as const) {
    const item = record[key];
    if (item === null || item === undefined) continue;
    if (typeof item !== "object" || Array.isArray(item)) throw new Error("invalid quota");
    const window = item as Record<string, unknown>;
    const percent = window.utilization;
    if (typeof percent !== "number" || !Number.isFinite(percent) || percent < 0 || percent > 100) throw new Error("invalid quota");
    const reset = window.resets_at === null || window.resets_at === undefined ? null : typeof window.resets_at === "string" ? Math.floor(Date.parse(window.resets_at) / 1000) : NaN;
    if (reset !== null && (!Number.isSafeInteger(reset) || reset < 0 || reset > 253402300799)) throw new Error("invalid quota");
    windows.push({id,usedPercent:percent,windowMinutes:minutes,resetsAt:reset});
  }
  return { windows };
}
function parseCredential(bytes: Buffer): { accessToken: string; expiresAt: number } | null {
  try {
    const value = JSON.parse(bytes.toString("utf8"));
    const auth = value?.claudeAiOauth;
    if (!auth || typeof auth.accessToken !== "string" || !/^[A-Za-z0-9._~+\/-]{16,8192}$/.test(auth.accessToken) || !Number.isSafeInteger(auth.expiresAt)) return null;
    return { accessToken: auth.accessToken, expiresAt: auth.expiresAt };
  } catch { return null; }
  finally { bytes.fill(0); }
}
async function profileIdentity(profile: string): Promise<string> {
  const stat = await lstat(profile);
  if (!stat.isDirectory() || stat.isSymbolicLink() || stat.uid !== process.getuid?.() || (stat.mode & 0o777) !== 0o700 || await realpath(profile) !== profile) throw new Error("profile unavailable");
  return `${stat.dev}:${stat.ino}`;
}
async function readScopedCredential(profile: string): Promise<{ accessToken: string; expiresAt: number } | null> {
  const identity = await profileIdentity(profile);
  let credential: { accessToken: string; expiresAt: number } | null = null;
  // Read only the dedicated profile, never a global or inferred Keychain item.
  {
    let file;
    try {
      file = await open(join(profile,".credentials.json"), constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
      const stat = await file.stat();
      if (!validCredentialFile(stat, process.getuid?.())) return null;
      const buffer = Buffer.alloc(MAX_BYTES + 1);
      try {
        const read = await file.read(buffer,0,buffer.length,0);
        if (read.bytesRead > MAX_BYTES) return null;
        credential = parseCredential(buffer.subarray(0,read.bytesRead));
      } finally { buffer.fill(0); }
    } catch { return null; }
    finally { await file?.close(); }
  }
  if (identity !== await profileIdentity(profile)) throw new Error("profile changed");
  return credential;
}
export async function queryClaudeQuota(token: string, fetcher: typeof fetch, now = Date.now()): Promise<ClaudeUsageResult> {
  const failed = (state: ClaudeUsageResult["state"]): ClaudeUsageResult => ({state,usage:null,checkedAt:now});
  try {
    const response = await fetcher(CLAUDE_USAGE_URL, {
      method:"GET", redirect:"error", credentials:"omit", cache:"no-store", referrerPolicy:"no-referrer",
      signal:AbortSignal.timeout(8000), headers:{Authorization:`Bearer ${token}`,"anthropic-beta":"oauth-2025-04-20","User-Agent":"Elsecrew/0.1.0"},
    });
    if (!response.ok) {
      await response.body?.cancel();
      return failed(response.status === 401 ? "expired" : response.status === 429 ? "retryLater" : "unavailable");
    }
    const reader = response.body?.getReader();
    if (!reader) return failed("unavailable");
    const chunks: Uint8Array[] = []; let length = 0;
    try {
      for (;;) { const part = await reader.read(); if (part.done) break; length += part.value.length; if (length > MAX_BYTES) return failed("unavailable"); chunks.push(part.value); }
      const value = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      return {state:"available",usage:parseClaudeQuota(value),checkedAt:now};
    } finally { await reader.cancel(); }
  } catch { return failed("unavailable"); }
}
export function createClaudeUsageReader(dataDirectory: string, fetcher: typeof fetch) {
  let pending: Promise<ClaudeUsageResult> | undefined;
  let cached: ClaudeUsageResult | undefined;
  let epoch = 0;
  return {
    invalidate() { cached = undefined; epoch++; },
    read(): Promise<ClaudeUsageResult> {
      if (pending) return pending;
      if (cached && Date.now() - cached.checkedAt < 60_000) return Promise.resolve(cached);
      const startedEpoch = epoch;
      pending = (async (): Promise<ClaudeUsageResult> => {
        const unavailable: ClaudeUsageResult = {state:"unavailable",usage:null,checkedAt:Date.now()};
        if (process.platform !== "darwin") return unavailable;
        try {
          const profile = join(dataDirectory,"claude-profile-v1");
          const identity = await profileIdentity(profile);
          const auth = await readScopedCredential(profile);
          if (!auth) return {...unavailable,state:"notConnected"};
          if (auth.expiresAt <= Date.now()) return {...unavailable,state:"expired"};
          const result = await queryClaudeQuota(auth.accessToken,fetcher);
          if (identity !== await profileIdentity(profile) || startedEpoch !== epoch) return unavailable;
          return result;
        } catch { return unavailable; }
      })().then(result => { if (startedEpoch === epoch) cached = result; return result; }).finally(() => { pending = undefined; });
      return pending;
    },
  };
}
