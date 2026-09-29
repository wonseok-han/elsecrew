import { validateAgentRunCancelRequestV1, type AgentRunCancelRequestV1 } from "./agent-adapter-v1.ts";

export const APPROVAL_CHANNEL = "m5:approval:v1";
export type ApprovalRequest = { action: "list"; identity: AgentRunCancelRequestV1 } | { action: "decline"; identity: AgentRunCancelRequestV1; approvalId: string };
export type ApprovalKind = "command" | "writeStdin" | "network" | "fileChange";
export type ApprovalSummary = { approvalId: string; kind: ApprovalKind };
export type ApprovalResult = { identity: AgentRunCancelRequestV1; pending: ApprovalSummary[]; queued: string | null };
export const APPROVAL_LABELS: Record<ApprovalKind, string> = {
  command: "명령 실행", writeStdin: "실행 중인 프로그램에 입력 전달", network: "네트워크 연결", fileChange: "파일 변경",
};
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid approval object");
  return value as Record<string, unknown>;
}
function id(value: unknown): string {
  if (typeof value !== "string" || !/^approval_[a-f0-9]{32}$/.test(value)) throw new Error("invalid approval id");
  return value;
}
export function validateApprovalRequest(value: unknown): ApprovalRequest {
  const v = object(value);
  const identity = validateAgentRunCancelRequestV1(v.identity);
  if (v.action === "list" && Object.keys(v).sort().join() === "action,identity") return { action: "list", identity };
  if (v.action === "decline" && Object.keys(v).sort().join() === "action,approvalId,identity") return { action: "decline", identity, approvalId: id(v.approvalId) };
  throw new Error("invalid approval action");
}
export function validateApprovalResult(value: unknown, request: ApprovalRequest): ApprovalResult {
  const v = object(value);
  if (Object.keys(v).sort().join() !== "identity,pending,queued") throw new Error("invalid approval result");
  const identity = validateAgentRunCancelRequestV1(v.identity);
  for (const key of Object.keys(identity) as (keyof typeof identity)[]) if (identity[key] !== request.identity[key]) throw new Error("approval run differs");
  if (!Array.isArray(v.pending) || v.pending.length > 128) throw new Error("invalid approval list");
  const pending = v.pending.map(value => {
    const item = object(value);
    if (Object.keys(item).sort().join() !== "approvalId,kind" || typeof item.kind !== "string" || !Object.prototype.hasOwnProperty.call(APPROVAL_LABELS,item.kind)) throw new Error("invalid approval summary");
    return {approvalId:id(item.approvalId),kind:item.kind as ApprovalKind};
  });
  const ids = pending.map(item=>item.approvalId);
  if (new Set(ids).size !== ids.length) throw new Error("duplicate approval");
  const queued = v.queued === null ? null : id(v.queued);
  if (queued !== (request.action === "decline" ? request.approvalId : null) || (queued !== null && ids.includes(queued))) throw new Error("approval receipt differs");
  return { identity, pending, queued };
}
