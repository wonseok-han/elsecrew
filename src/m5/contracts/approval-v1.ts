import { validateAgentRunCancelRequestV1, type AgentRunCancelRequestV1 } from "./agent-adapter-v1.ts";

export const APPROVAL_CHANNEL = "m5:approval:v1";
export type ApprovalRequest = { action: "list"; identity: AgentRunCancelRequestV1 } | { action: "decline" | "inspect" | "acceptOnce"; identity: AgentRunCancelRequestV1; approvalId: string };
export type ApprovalKind = "command" | "writeStdin" | "network" | "fileChange" | "tool";
export type ApprovalSummary = { approvalId: string; kind: ApprovalKind };
export type ApprovalInspection = ApprovalSummary & { command: string | null; cwd: string | null; reason: string | null; grantRoot: string | null; host: string | null; protocol: string | null; toolInput?: string };
export type ApprovalResult = { identity: AgentRunCancelRequestV1; pending: ApprovalSummary[]; queued: string | null; inspection?: ApprovalInspection };
export const APPROVAL_LABELS: Record<ApprovalKind, string> = {
  command: "명령 실행", writeStdin: "실행 중인 프로그램에 입력 전달", network: "네트워크 연결", fileChange: "파일 변경", tool: "에이전트 도구 사용",
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
  if ((v.action === "decline" || v.action === "inspect" || v.action === "acceptOnce") && Object.keys(v).sort().join() === "action,approvalId,identity") return { action: v.action, identity, approvalId: id(v.approvalId) };
  throw new Error("invalid approval action");
}
export function validateApprovalResult(value: unknown, request: ApprovalRequest): ApprovalResult {
  const v = object(value);
  if (Object.keys(v).sort().join() !== (request.action === "inspect" ? "identity,inspection,pending,queued" : "identity,pending,queued")) throw new Error("invalid approval result");
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
  if (queued !== (request.action === "decline" || request.action === "acceptOnce" ? request.approvalId : null) || (queued !== null && ids.includes(queued))) throw new Error("approval receipt differs");
  if (request.action === "inspect") {
    const detail = object(v.inspection);
    const keys = "approvalId,command,cwd,grantRoot,host,kind,protocol,reason" + (detail.kind === "tool" ? ",toolInput" : "");
    if (Object.keys(detail).sort().join() !== keys || detail.approvalId !== request.approvalId || !pending.some(item=>item.approvalId === detail.approvalId && item.kind === detail.kind)) throw new Error("approval inspection differs");
    if (detail.kind === "tool" && (typeof detail.toolInput !== "string" || new TextEncoder().encode(detail.toolInput).length > 128 * 1024)) throw new Error("invalid tool inspection input");
    const limits = {command:65536,cwd:16384,reason:16384,grantRoot:16384,host:1024,protocol:64};
    for (const [key, maximum] of Object.entries(limits)) {
      const text = detail[key];
      if (text !== null && (typeof text !== "string" || new TextEncoder().encode(text).length > maximum)) throw new Error("invalid approval inspection text");
    }
    if (detail.kind === "fileChange" ? detail.command !== null || detail.cwd !== null || detail.host !== null || detail.protocol !== null : detail.grantRoot !== null) throw new Error("approval action fields differ");
    if (detail.kind === "network" ? typeof detail.host !== "string" || !detail.host || typeof detail.protocol !== "string" || !detail.protocol : detail.host !== null || detail.protocol !== null) throw new Error("approval network fields differ");
    return {identity,pending,queued,inspection:detail as ApprovalInspection};
  }
  return { identity, pending, queued };
}

/** Make invisible control/direction characters visible; never interpret as markup. */
export function visibleApprovalText(value: string): string {
  return value.replace(/[\u0000-\u001f\u007f-\u009f\p{Cf}]/gu, character => {
    const point = character.codePointAt(0)!;
    return point <= 0xffff ? `\\u${point.toString(16).padStart(4,"0")}` : `\\u{${point.toString(16)}}`;
  });
}
