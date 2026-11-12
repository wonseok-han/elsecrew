import { validateAgentCapabilitiesRequestV1, type AgentCapabilitiesRequestV1 } from "./agent-adapter-v1.ts";

export const MODEL_CHOICES_CHANNEL = "m5:models:v1";
export type ModelChoicesRequest = AgentCapabilitiesRequestV1 & { runtime: "codex" | "claude-code"; chatId?: string; roleId?: "coordinator" | "design" | "planning" | "coding" | "review" };
export interface ModelChoicesResult {
  schemaVersion: 1;
  runtime: "codex" | "claude-code";
  scope: { chatId: string; roleId: string; worktreeId: string; workspaceIdentity: string } | null;
  usage?: { windows: { id: "primary" | "secondary"; usedPercent: number; windowMinutes: number | null; resetsAt: number | null }[] } | null;
  models: { modelSelector: string; reasoningEfforts: string[]; isDefault?: boolean }[];
}
function fail(): never { throw new Error("invalid model choices"); }
function object(value: unknown, keys: string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return fail();
  const result = value as Record<string, unknown>;
  if (Object.keys(result).length !== keys.length || keys.some(key => !Object.prototype.hasOwnProperty.call(result, key))) return fail();
  return result;
}
function text(value: unknown, max: number): string {
  if (typeof value !== "string" || !value || new TextEncoder().encode(value).length > max) return fail();
  return value;
}
export function validateModelChoicesRequest(value: unknown): ModelChoicesRequest {
  const request = validateAgentCapabilitiesRequestV1(value);
  if (!["codex", "claude-code"].includes(request.runtime) || Boolean(request.chatId) !== Boolean(request.roleId)) return fail();
  return request as ModelChoicesRequest;
}
/** Display data only: no canStart, capability fingerprint, or permission grant. */
export function validateModelChoicesResult(value: unknown, request: ModelChoicesRequest): ModelChoicesResult {
  const hasUsage = !!value && typeof value === "object" && "usage" in value;
  const result = object(value, ["schemaVersion", "runtime", "scope", "models", ...(hasUsage ? ["usage"] : [])]);
  if (result.usage !== undefined && result.usage !== null) {
    const usage = object(result.usage, ["windows"]);
    if (!Array.isArray(usage.windows) || usage.windows.length > 2) return fail();
    const ids = new Set<string>();
    for (const item of usage.windows) {
      const window = object(item, ["id", "usedPercent", "windowMinutes", "resetsAt"]);
      if (window.id !== "primary" && window.id !== "secondary" || ids.has(window.id as string)) return fail();
      ids.add(window.id as string);
      if (typeof window.usedPercent !== "number" || !Number.isFinite(window.usedPercent) || window.usedPercent < 0 || window.usedPercent > 100) return fail();
      if (window.windowMinutes !== null && (!Number.isSafeInteger(window.windowMinutes) || (window.windowMinutes as number) <= 0 || (window.windowMinutes as number) > 525600)) return fail();
      if (window.resetsAt !== null && (!Number.isSafeInteger(window.resetsAt) || (window.resetsAt as number) < 0 || (window.resetsAt as number) > 253402300799)) return fail();
    }
  }
  if (result.schemaVersion !== 1 || result.runtime !== request.runtime) return fail();
  if (request.chatId) {
    const scope = object(result.scope, ["chatId", "roleId", "worktreeId", "workspaceIdentity"]);
    for (const key of Object.keys(scope)) text(scope[key], 256);
    if (scope.chatId !== request.chatId || scope.roleId !== request.roleId) return fail();
  } else if (result.scope !== null) return fail();
  if (!Array.isArray(result.models) || result.models.length > 256) return fail();
  const names = new Set<string>();
  for (const item of result.models) {
    const model = object(item, ["modelSelector", "reasoningEfforts", ...(item && typeof item === "object" && "isDefault" in item ? ["isDefault"] : [])]);
    if (model.isDefault !== undefined && typeof model.isDefault !== "boolean") return fail();
    const name = text(model.modelSelector, 128);
    if (names.has(name)) return fail();
    names.add(name);
    if (!Array.isArray(model.reasoningEfforts) || model.reasoningEfforts.length > 32) return fail();
    const efforts = model.reasoningEfforts.map(value => text(value, 128));
    if (new Set(efforts).size !== efforts.length) return fail();
  }
  return value as ModelChoicesResult;
}
declare global {
  interface Window { m5Models?: { request(input: ModelChoicesRequest): Promise<ModelChoicesResult> } }
}
