import type { M5BridgeV4 } from "./contracts/bridge-v4.ts";
import { validateChatListPageV1, type ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { isAgentChatDetailPageV1, validateAgentChatGetResultV1, validateAgentRecoveryRecordV1, validateAgentRecoveryResolveResultV1, type AgentRecoveryRecordV1 } from "./contracts/agent-adapter-v1.ts";

// This surface deliberately has no file, snapshot, run-start or source-write API.
export type RecoveryHistoryBridge = Pick<M5BridgeV4, "listChats" | "getChat" | "inspectAgentRecovery" | "resolveAgentRecovery">;
export async function recoveryChatPage(bridge: RecoveryHistoryBridge, projectId: string, cursor: string | null) {
  const page = validateChatListPageV1(await bridge.listChats({ projectId, cursor, limit: 50 }));
  if (page.projectId !== projectId || page.items.some(chat => chat.projectId !== projectId)) throw new Error("recovery history project mismatch");
  return page;
}
export async function recoveryHistoryPage(bridge: RecoveryHistoryBridge, chat: ChatRecordV1, afterEventSequence = 0) {
  const page = validateAgentChatGetResultV1(await bridge.getChat({ chatId: chat.chatId, afterEventSequence, cursor: null, limit: 50 }));
  for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"] as const) {
    if (page.chat[key] !== chat[key]) throw new Error("recovery history chat mismatch");
  }
  if (page.afterEventSequence !== afterEventSequence) throw new Error("recovery history cursor mismatch");
  const recovery = isAgentChatDetailPageV1(page) ? page.agentRecovery : null;
  if (recovery) for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"] as const) {
    if (recovery[key] !== chat[key]) throw new Error("recovery history recovery mismatch");
  }
  return page;
}
export function recoveryAddress(record: AgentRecoveryRecordV1) {
  return { chatId: record.chatId, taskId: record.taskId, projectId: record.projectId, worktreeId: record.worktreeId,
    workspaceIdentity: record.workspaceIdentity, runId: record.runId, recoveryId: record.recoveryId };
}
export async function resolveHistoryRecovery(bridge: RecoveryHistoryBridge, record: AgentRecoveryRecordV1, stillCurrent: () => boolean) {
  if (!stillCurrent() || record.state !== "unresolved" || !record.poisoned || record.reason === "processIdentityUnavailable") throw new Error("recovery is not available");
  const address = recoveryAddress(record);
  const inspected = validateAgentRecoveryRecordV1(await bridge.inspectAgentRecovery(address));
  for (const key of Object.keys(address) as (keyof typeof address)[]) {
    if (address[key] !== inspected[key]) throw new Error("recovery identity changed");
  }
  if (!stillCurrent() || inspected.state !== "unresolved" || !inspected.poisoned || inspected.reason === "processIdentityUnavailable") throw new Error("recovery changed before confirmation");
  const request = { ...address, expectedRevision: inspected.revision, resolution: "abandon" as const };
  const result = validateAgentRecoveryResolveResultV1(await bridge.resolveAgentRecovery(request));
  for (const key of Object.keys(request) as (keyof typeof request)[]) {
    if (result.request[key] !== request[key]) throw new Error("recovery response identity changed");
  }
  return result;
}
