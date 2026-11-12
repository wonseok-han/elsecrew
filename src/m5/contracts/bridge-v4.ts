import type {
  AgentCapabilitiesRequestV1,
  AgentCapabilitySnapshotV1,
  AgentChatGetResultV1,
  AgentRecoveryInspectRequestV1,
  AgentRecoveryRecordV1,
  AgentRecoveryResolveRequestV1,
  AgentRecoveryResolveResultV1,
  AgentRunAcceptedV1,
  AgentRunCancelRequestV1,
  AgentRunCancelResultV1,
  AgentPublicEventV1,
  AgentRunStartRequestV1,
} from "./agent-adapter-v1.ts";
import type { M5BridgeV3 } from "./bridge-v3.ts";
import type { ChatGetRequestV1 } from "./run-recovery-v1.ts";

/** Planned G5A surface. It is not advertised by the current G5 ready handshake. */
export const M5_BRIDGE_V4_METHODS = [
  "getSystemStatus",
  "prepareQuit",
  "onSidecarEvent",
  "openProject",
  "getProjectTree",
  "createProjectWorktree",
  "closeProject",
  "readFile",
  "beginFileSave",
  "appendFileSaveChunk",
  "commitFileSave",
  "watchProjectFiles",
  "getDiffSummary",
  "getDiffFile",
  "onProjectEvent",
  "getWorkspaceTree",
  "closeWorkspace",
  "readWorkspaceFile",
  "beginWorkspaceFileSave",
  "appendWorkspaceFileSaveChunk",
  "commitWorkspaceFileSave",
  "watchWorkspaceFiles",
  "getWorkspaceDiffSummary",
  "getWorkspaceDiffFile",
  "onWorkspaceEvent",
  "prepareChatWorkspace",
  "abandonPreparedWorkspace",
  "getProjectRoleSettings",
  "updateProjectRoleSettings",
  "createChat",
  "listChats",
  "getChat",
  "startFixtureRun",
  "cancelRun",
  "listRecoveries",
  "inspectRecovery",
  "markRecoveryResolved",
  "onChatEvent",
  "getAgentCapabilities",
  "startAgentRun",
  "cancelAgentRun",
  "inspectAgentRecovery",
  "resolveAgentRecovery",
  "onAgentPublicEvent",
] as const;

export type M5BridgeV4 = Omit<M5BridgeV3, "version" | "getChat"> & {
  readonly version: 4;
  getAgentCapabilities(input: AgentCapabilitiesRequestV1): Promise<AgentCapabilitySnapshotV1>;
  startAgentRun(input: AgentRunStartRequestV1): Promise<AgentRunAcceptedV1>;
  cancelAgentRun(input: AgentRunCancelRequestV1): Promise<AgentRunCancelResultV1>;
  inspectAgentRecovery(input: AgentRecoveryInspectRequestV1): Promise<AgentRecoveryRecordV1>;
  resolveAgentRecovery(input: AgentRecoveryResolveRequestV1): Promise<AgentRecoveryResolveResultV1>;
  onAgentPublicEvent(listener: (event: AgentPublicEventV1) => void): () => void;
  /** Current G5 has no agentRuns member; planned G5A adds it after capability negotiation. */
  getChat(input: ChatGetRequestV1): Promise<AgentChatGetResultV1>;
};
