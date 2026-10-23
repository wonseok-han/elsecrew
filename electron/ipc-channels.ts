export const M5_BRIDGE_V7_CHANNELS = { chatAppendMessage: "m5:v7:chat:append-message" } as const;
export const M5_BRIDGE_V8_CHANNELS = { agentOutputHead: "m5:v8:agent:output-head", agentOutputPage: "m5:v8:agent:output-page" } as const;

export const M5_BRIDGE_V2_CHANNELS = {
  projectOpen: "m5:v2:project:open",
  projectGetTree: "m5:v2:project:get-tree",
  projectCreateWorktree: "m5:v2:project:create-worktree",
  projectClose: "m5:v2:project:close",
  fileRead: "m5:v2:file:read",
  fileSaveBegin: "m5:v2:file:save-begin",
  fileSaveChunk: "m5:v2:file:save-chunk",
  fileSaveCommit: "m5:v2:file:save-commit",
  fileWatch: "m5:v2:file:watch",
  diffGetSummary: "m5:v2:diff:get-summary",
  diffGetFile: "m5:v2:diff:get-file",
  projectEvent: "m5:v2:project:event",
  eventAck: "m5:v2:internal:event-ack",
} as const;

/** V3 is additive: every renderer capability has one fixed, named channel. */
export const M5_BRIDGE_V3_CHANNELS = {
  workspaceGetTree: "m5:v3:workspace:get-tree",
  workspaceClose: "m5:v3:workspace:close",
  workspaceFileRead: "m5:v3:workspace:file:read",
  workspaceFileSaveBegin: "m5:v3:workspace:file:save-begin",
  workspaceFileSaveChunk: "m5:v3:workspace:file:save-chunk",
  workspaceFileSaveCommit: "m5:v3:workspace:file:save-commit",
  workspaceFileWatch: "m5:v3:workspace:file:watch",
  workspaceDiffGetSummary: "m5:v3:workspace:diff:get-summary",
  workspaceDiffGetFile: "m5:v3:workspace:diff:get-file",
  workspaceEvent: "m5:v3:workspace:event",
  workspacePrepareChat: "m5:v3:workspace:prepare-chat",
  workspaceAbandonPrepared: "m5:v3:workspace:abandon-prepared",
  projectGetRoleSettings: "m5:v3:project:get-role-settings",
  projectUpdateRoleSettings: "m5:v3:project:update-role-settings",
  chatCreate: "m5:v3:chat:create",
  chatList: "m5:v3:chat:list",
  chatGet: "m5:v3:chat:get",
  runStartFixture: "m5:v3:run:start-fixture",
  runCancel: "m5:v3:run:cancel",
  recoveryList: "m5:v3:recovery:list",
  recoveryInspect: "m5:v3:recovery:inspect",
  recoveryMarkResolved: "m5:v3:recovery:mark-resolved",
  chatEvent: "m5:v3:chat:event",
} as const;

/** G5A is additive and remains unavailable until the exact planned ready set is negotiated. */
export const M5_BRIDGE_V4_CHANNELS = {
  agentGetCapabilities: "m5:v4:agent:get-capabilities",
  agentStartRun: "m5:v4:agent:start-run",
  agentCancelRun: "m5:v4:agent:cancel-run",
  agentRecoveryInspect: "m5:v4:agent:recovery:inspect",
  agentRecoveryResolve: "m5:v4:agent:recovery:resolve",
  agentPublicEvent: "m5:v4:agent:public-event",
} as const;

/** G5B adds a notification-only hydration trigger; V3/V4 channels stay stable. */
export const M5_BRIDGE_V5_CHANNELS = {
  agentRunStateChanged: "m5:v5:agent:run-state-changed",
} as const;

/** V6 exposes the already-negotiated, privacy-safe storage status through one fixed channel. */
export const M5_BRIDGE_V6_CHANNELS = {
  storageGetStatus: "m5:v6:storage:get-status",
} as const;
