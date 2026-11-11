import { validateAgentCapabilitiesRequestV1, type AgentCapabilitiesRequestV1 } from "./agent-adapter-v1.ts";

export const MODEL_CHOICES_CHANNEL = "m5:models:v1";
export type ModelChoicesRequest = AgentCapabilitiesRequestV1 & { runtime: "codex" | "claude-code"; chatId?: string; roleId?: "coordinator" | "design" | "planning" | "coding" | "review" };
export interface ModelChoicesResult {
  schemaVersion: 1;
  runtime: "codex" | "claude-code";
  scope: { chatId: string; roleId: string; worktreeId: string; workspaceIdentity: string } | null;
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
  const result = object(value, ["schemaVersion", "runtime", "scope", "models"]);
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
