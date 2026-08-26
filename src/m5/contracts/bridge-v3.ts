import type { UnsubscribeV1 } from "./bridge-v1.ts";
import type { M5BridgeV2 } from "./bridge-v2.ts";
import type {
  ChatDetailPageV1,
  ChatGetRequestV1,
  ChatListPageV1,
  ChatListRequestV1,
  ChatRecordV1,
  ChatCreateRequestV1,
  FixtureRunStartRequestV1,
  PageRequestV1,
  ProjectRoleSettingsV1,
  ProjectRoleSettingsUpdateRequestV1,
  RecoveryMarkResolvedRequestV1,
  RecoveryPageV1,
  RecoveryRecordV1,
  RunRecordV1,
  RunCancelRequestV1,
  TeamMessageV1,
} from "./run-recovery-v1.ts";

export const M5_BRIDGE_V3_METHODS = [
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
] as const;

export type M5BridgeV3 = Omit<M5BridgeV2, "version"> & {
  readonly version: 3;
  getProjectRoleSettings(input: { projectId: string }): Promise<ProjectRoleSettingsV1>;
  updateProjectRoleSettings(input: ProjectRoleSettingsUpdateRequestV1): Promise<ProjectRoleSettingsV1>;
  createChat(input: ChatCreateRequestV1): Promise<ChatRecordV1>;
  listChats(input: ChatListRequestV1): Promise<ChatListPageV1>;
  getChat(input: ChatGetRequestV1): Promise<ChatDetailPageV1>;
  startFixtureRun(input: FixtureRunStartRequestV1): Promise<RunRecordV1>;
  cancelRun(input: RunCancelRequestV1): Promise<RunRecordV1>;
  listRecoveries(input: PageRequestV1): Promise<RecoveryPageV1>;
  inspectRecovery(input: { recoveryId: string }): Promise<RecoveryRecordV1>;
  markRecoveryResolved(input: RecoveryMarkResolvedRequestV1): Promise<RecoveryRecordV1>;
  onChatEvent(listener: (event: TeamMessageV1) => void): UnsubscribeV1;
};
