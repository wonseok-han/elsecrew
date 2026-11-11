import { validateModelChoicesResult, type ModelChoicesResult } from "./model-choices-v1.ts";
export const CLAUDE_USAGE_CHANNEL = "m5:claude-usage:v1";
export type ProviderUsage = NonNullable<ModelChoicesResult["usage"]>;
export type ClaudeUsageResult = { state: "available" | "unavailable" | "notConnected" | "expired" | "retryLater"; usage: ProviderUsage | null; checkedAt: number };
export function validateClaudeUsageResult(value: unknown): ClaudeUsageResult {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid usage response");
  const result = value as Record<string, unknown>;
  if (Object.keys(result).sort().join() !== "checkedAt,state,usage" || !["available","unavailable","notConnected","expired","retryLater"].includes(String(result.state)) || !Number.isSafeInteger(result.checkedAt) || (result.checkedAt as number) < 0) throw new Error("invalid usage response");
  validateModelChoicesResult({schemaVersion:1,runtime:"claude-code",scope:null,models:[],usage:result.usage},{runtime:"claude-code",refresh:false});
  if ((result.state === "available") !== (result.usage !== null)) throw new Error("invalid usage state");
  return value as ClaudeUsageResult;
}
declare global { interface Window { m5ClaudeUsage?: { read(): Promise<ClaudeUsageResult> } } }
