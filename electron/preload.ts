import { PREVIEW_CHANNEL_V1 as M5_PREVIEW_CHANNEL, PREVIEW_EVENT_CHANNEL_V1 as M5_PREVIEW_EVENT_CHANNEL, validatePreviewEventV1, validatePreviewIpcRequestV1, validatePreviewIpcResultV1, type M5PreviewBridgeV1, type PreviewEventV1 } from "../src/m5/contracts/preview-v1.ts";
import { CLAUDE_USAGE_CHANNEL as M5_CLAUDE_USAGE_CHANNEL, validateClaudeUsageResult } from "../src/m5/contracts/provider-usage-v1.ts";
import { GIT_SELECTION_CHANNEL as M5_GIT_SELECTION_CHANNEL, validateGitSelectionAction, validateGitSelection, type GitSelectionAction } from "../src/m5/contracts/git-selection-v1.ts";
import { SAVE_RECOVERY_RESOLVE_CHANNEL as M5_SAVE_RECOVERY_RESOLVE_CHANNEL, validateSaveRecoveryResolveRequest, validateSaveRecoveryResolveResult } from "../src/m5/contracts/save-recovery-v1.ts";
import { SAVE_RECOVERY_CHANNEL as M5_SAVE_RECOVERY_CHANNEL, validateSaveRecoveryRequest, validateSaveRecoveryResult } from "../src/m5/contracts/save-recovery-v1.ts";
import { contextBridge, ipcRenderer } from "electron";
import { SESSION_DIAGNOSTICS_CHANNEL as M5_SESSION_DIAGNOSTICS_CHANNEL, validateSessionDiagnostics } from "../src/m5/contracts/session-diagnostics-v1.ts";
import { DEVICE_REPORT_CHANNEL as M5_DEVICE_REPORT_CHANNEL, validateDeviceReport } from "../src/m5/contracts/device-report-v1.ts";
import { MODEL_CHOICES_CHANNEL, validateModelChoicesRequest, validateModelChoicesResult, type ModelChoicesRequest } from "../src/m5/contracts/model-choices-v1.ts";
import { APPROVAL_CHANNEL as M5_APPROVAL_CHANNEL, validateApprovalRequest, validateApprovalResult, type ApprovalRequest } from "../src/m5/contracts/approval-v1.ts";
import { PROJECT_CATALOG_CHANNEL as M5_PROJECT_CATALOG_CHANNEL, validateProjectCatalogRequest, validateProjectCatalogResult, type ProjectCatalogRequest } from "../src/m5/contracts/project-catalog-v1.ts";
import { LOGIN_CHANNEL as M5_LOGIN_CHANNEL, validateLoginRequest, validateLoginStatus, type LoginRequest } from "../src/m5/contracts/provider-login-v1.ts";
import { INSTALL_CHANNEL as M5_INSTALL_CHANNEL, validateInstallRequest, validateInstallStatus, type InstallRequest } from "../src/m5/contracts/provider-install-v1.ts";

import { M5_INTERNAL_CHANNELS_V1 } from "../src/m5/contracts/lifecycle-v1.ts";
import type { M5BridgeV6 } from "../src/m5/contracts/bridge-v6.ts";
import type { M5BridgeV8 } from "../src/m5/contracts/bridge-v8.ts";
import { validateAgentRunBindingV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import { validateOutputHead, validateOutputPageRequest, type OutputHead } from "../src/m5/contracts/agent-output-pages.ts";
import { M5_BRIDGE_V8_CHANNELS } from "./ipc-channels.ts";
import { validateChatAppendMessageRequestV1 } from "../src/m5/contracts/run-recovery-v1.ts";
import { M5_BRIDGE_V7_CHANNELS } from "./ipc-channels.ts";
import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";
import {
  validateAgentCapabilitiesRequestV1,
  validateAgentPublicEventV1,
  validateAgentRunStateChangedV1,
  validateAgentRecoveryInspectRequestV1,
  validateAgentRecoveryResolveRequestV1,
  validateAgentRunCancelRequestV1,
  validateAgentRunStartRequestV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";
import { validateProjectIdentityV1 } from "../src/m5/contracts/project-v1.ts";
import { validateTeamMessageV1 } from "../src/m5/contracts/run-recovery-v1.ts";
import { validateWorkspaceEventV1 } from "../src/m5/contracts/workspace-addressing-v1.ts";
import { validateStorageStatusRequestV1 } from "../src/m5/contracts/storage-lifetime-v1.ts";
import { EXTERNAL_LINK_CHANNEL as M5_EXTERNAL_LINK_CHANNEL, validateExternalLink, validateExternalLinkResult } from "../src/m5/contracts/external-link-v1.ts";
import { M5_BRIDGE_V2_CHANNELS, M5_BRIDGE_V3_CHANNELS, M5_BRIDGE_V4_CHANNELS, M5_BRIDGE_V5_CHANNELS, M5_BRIDGE_V6_CHANNELS } from "./ipc-channels.ts";
import {
  validateChatCreateInput, validateChatGetInput, validateChatListInput, validateFixtureRunStartInput,
  validateAbandonPreparedWorkspaceInput, validatePrepareChatWorkspaceInput,
  validatePathInput, validateProjectInput, validateRecoveryInspectInput, validateRecoveryListInput,
  validateRecoveryMarkResolvedInput, validateRoleSettingsGetInput, validateRoleSettingsUpdateInput,
  validateRunCancelInput, validateSaveBeginInput, validateSaveChunkInput, validateSaveCommitInput,
  validateTreeInput, validateWorkspaceDiffFileInput, validateWorkspaceFileReadInput,
  validateWorkspaceRefInput, validateWorkspaceSaveBeginInput, validateWorkspaceSaveChunkInput,
  validateWorkspaceSaveCommitInput, validateWorkspaceTreeInput,
} from "./bridge-input.ts";

const bridge: M5BridgeV8 = Object.freeze({
  version: 8,
  getAgentOutputHead: (input) => ipcRenderer.invoke(M5_BRIDGE_V8_CHANNELS.agentOutputHead, validateAgentRunBindingV1(input)),
  onAgentOutputAvailable: (listener) => subscribe(agentOutputListeners, listener),
  getAgentOutputPage: (input) => ipcRenderer.invoke(M5_BRIDGE_V8_CHANNELS.agentOutputPage, validateOutputPageRequest(input)),
  appendChatMessage: (input) => ipcRenderer.invoke(M5_BRIDGE_V7_CHANNELS.chatAppendMessage, validateChatAppendMessageRequestV1(input)),
  getSystemStatus: () => ipcRenderer.invoke(M5_INTERNAL_CHANNELS_V1.systemGetStatus),
  getStorageStatus: (input) => ipcRenderer.invoke(M5_BRIDGE_V6_CHANNELS.storageGetStatus, validateStorageStatusRequestV1(input)),
  prepareQuit: () => ipcRenderer.invoke(M5_INTERNAL_CHANNELS_V1.appPrepareQuit),
  onSidecarEvent: (listener) => subscribe(sidecarListeners, listener),
  openProject: async () => {
    const result: unknown = await ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.projectOpen);
    if (isProjectPickerCancelled(result)) return null;
    return validateProjectIdentityV1(result);
  },
  getProjectTree: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.projectGetTree, validateTreeInput(input)),
  createProjectWorktree: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.projectCreateWorktree, validateProjectInput(input)),
  closeProject: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.projectClose, validateProjectInput(input)),
  readFile: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.fileRead, validatePathInput(input)),
  beginFileSave: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.fileSaveBegin, validateSaveBeginInput(input)),
  appendFileSaveChunk: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.fileSaveChunk, validateSaveChunkInput(input)),
  commitFileSave: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.fileSaveCommit, validateSaveCommitInput(input)),
  watchProjectFiles: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.fileWatch, validateProjectInput(input)),
  getDiffSummary: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.diffGetSummary, validateProjectInput(input)),
  getDiffFile: (input) => ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.diffGetFile, validatePathInput(input)),
  onProjectEvent: (listener) => subscribe(projectListeners, listener),
  getWorkspaceTree: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceGetTree, validateWorkspaceTreeInput(input)),
  closeWorkspace: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceClose, validateWorkspaceRefInput(input)),
  readWorkspaceFile: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceFileRead, validateWorkspaceFileReadInput(input)),
  beginWorkspaceFileSave: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceFileSaveBegin, validateWorkspaceSaveBeginInput(input)),
  appendWorkspaceFileSaveChunk: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceFileSaveChunk, validateWorkspaceSaveChunkInput(input)),
  commitWorkspaceFileSave: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceFileSaveCommit, validateWorkspaceSaveCommitInput(input)),
  watchWorkspaceFiles: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceFileWatch, validateWorkspaceRefInput(input)),
  getWorkspaceDiffSummary: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceDiffGetSummary, validateWorkspaceRefInput(input)),
  getWorkspaceDiffFile: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceDiffGetFile, validateWorkspaceDiffFileInput(input)),
  onWorkspaceEvent: (listener) => subscribe(workspaceListeners, listener),
  prepareChatWorkspace: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspacePrepareChat, validatePrepareChatWorkspaceInput(input)),
  abandonPreparedWorkspace: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.workspaceAbandonPrepared, validateAbandonPreparedWorkspaceInput(input)),
  getProjectRoleSettings: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.projectGetRoleSettings, validateRoleSettingsGetInput(input)),
  updateProjectRoleSettings: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.projectUpdateRoleSettings, validateRoleSettingsUpdateInput(input)),
  createChat: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.chatCreate, validateChatCreateInput(input)),
  listChats: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.chatList, validateChatListInput(input)),
  getChat: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.chatGet, validateChatGetInput(input)),
  startFixtureRun: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.runStartFixture, validateFixtureRunStartInput(input)),
  cancelRun: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.runCancel, validateRunCancelInput(input)),
  listRecoveries: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.recoveryList, validateRecoveryListInput(input)),
  inspectRecovery: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.recoveryInspect, validateRecoveryInspectInput(input)),
  markRecoveryResolved: (input) => ipcRenderer.invoke(M5_BRIDGE_V3_CHANNELS.recoveryMarkResolved, validateRecoveryMarkResolvedInput(input)),
  onChatEvent: (listener) => subscribe(chatListeners, listener),
  getAgentCapabilities: (input) => ipcRenderer.invoke(M5_BRIDGE_V4_CHANNELS.agentGetCapabilities, validateAgentCapabilitiesRequestV1(input)),
  startAgentRun: (input) => ipcRenderer.invoke(M5_BRIDGE_V4_CHANNELS.agentStartRun, validateAgentRunStartRequestV1(input)),
  cancelAgentRun: (input) => ipcRenderer.invoke(M5_BRIDGE_V4_CHANNELS.agentCancelRun, validateAgentRunCancelRequestV1(input)),
  inspectAgentRecovery: (input) => ipcRenderer.invoke(M5_BRIDGE_V4_CHANNELS.agentRecoveryInspect, validateAgentRecoveryInspectRequestV1(input)),
  resolveAgentRecovery: (input) => ipcRenderer.invoke(M5_BRIDGE_V4_CHANNELS.agentRecoveryResolve, validateAgentRecoveryResolveRequestV1(input)),
  onAgentPublicEvent: (listener) => subscribe(agentPublicEventListeners, listener),
  onAgentRunStateChanged: (listener) => subscribe(agentRunStateChangedListeners, listener),
});

contextBridge.exposeInMainWorld("m5", bridge);
contextBridge.exposeInMainWorld("m5Models", Object.freeze({
  request: async (input: ModelChoicesRequest) => {
    const request = validateModelChoicesRequest(input);
    return validateModelChoicesResult(await ipcRenderer.invoke(MODEL_CHOICES_CHANNEL, request), request);
  },
}));
contextBridge.exposeInMainWorld("m5SaveRecovery", Object.freeze({
  resolve: async (input: unknown) => {
    const request = validateSaveRecoveryResolveRequest(input);
    return validateSaveRecoveryResolveResult(await ipcRenderer.invoke(M5_SAVE_RECOVERY_RESOLVE_CHANNEL, request), request);
  },
  inspect: async (input: unknown) => {
    const request = validateSaveRecoveryRequest(input);
    return validateSaveRecoveryResult(await ipcRenderer.invoke(M5_SAVE_RECOVERY_CHANNEL, request), request);
  },
}));
contextBridge.exposeInMainWorld("m5Projects", Object.freeze({
  request: async (input: ProjectCatalogRequest) => {
    const request = validateProjectCatalogRequest(input);
    return validateProjectCatalogResult(await ipcRenderer.invoke(M5_PROJECT_CATALOG_CHANNEL, request), request);
  },
}));
contextBridge.exposeInMainWorld("m5ExternalLink", Object.freeze({
  open: async (input: string) => {
    const url = validateExternalLink(input);
    if (!url) throw new Error("invalid external link");
    return validateExternalLinkResult(await ipcRenderer.invoke(M5_EXTERNAL_LINK_CHANNEL, url));
  },
}));
contextBridge.exposeInMainWorld("m5Approval", Object.freeze({
  request: async (input: ApprovalRequest) => {
    const request = validateApprovalRequest(input);
    return validateApprovalResult(await ipcRenderer.invoke(M5_APPROVAL_CHANNEL, request), request);
  },
}));
contextBridge.exposeInMainWorld("m5Device", Object.freeze({
  inspect: async () => validateDeviceReport(await ipcRenderer.invoke(M5_DEVICE_REPORT_CHANNEL)),
}));
contextBridge.exposeInMainWorld("m5Diagnostics", Object.freeze({
  inspect: async () => validateSessionDiagnostics(await ipcRenderer.invoke(M5_SESSION_DIAGNOSTICS_CHANNEL)),
}));
contextBridge.exposeInMainWorld("m5Login", Object.freeze({
  request: async (input: LoginRequest) => {
    const request = validateLoginRequest(input);
    return validateLoginStatus(await ipcRenderer.invoke(M5_LOGIN_CHANNEL, request),request.provider);
  },
}));
contextBridge.exposeInMainWorld("m5Install", Object.freeze({
  request: async (input: InstallRequest) => {
    const request = validateInstallRequest(input);
    return validateInstallStatus(await ipcRenderer.invoke(M5_INSTALL_CHANNEL, request));
  },
}));

const sidecarListeners = new Set<(event: EventV1) => void>();
const projectListeners = new Set<Parameters<M5BridgeV6["onProjectEvent"]>[0]>();
const workspaceListeners = new Set<Parameters<M5BridgeV6["onWorkspaceEvent"]>[0]>();
const chatListeners = new Set<Parameters<M5BridgeV6["onChatEvent"]>[0]>();
const agentPublicEventListeners = new Set<Parameters<M5BridgeV6["onAgentPublicEvent"]>[0]>();
const agentRunStateChangedListeners = new Set<Parameters<M5BridgeV6["onAgentRunStateChanged"]>[0]>();
const agentOutputListeners = new Set<(head: OutputHead) => void>();
// Acknowledge delivery in every case. The credit only paces delivery; an event
// that fails its check or a listener that throws must not leave the credit
// queue stuck, which would make the very next acknowledgement look
// out of order and stop the whole engine.
function delivered(payload: Pick<EventV1, "streamId" | "seq">, handle: () => void): void {
  try { handle(); }
  catch (error) { console.error("m5: renderer could not handle a sidecar event", error); }
  finally { acknowledge(payload); }
}
ipcRenderer.on(M5_BRIDGE_V8_CHANNELS.agentOutputAvailable, (_event, payload: { event: OutputHead; streamId: string; seq: number }) => delivered(payload, () => {
  const head = validateOutputHead(payload.event, payload.event.binding);
  for (const listener of agentOutputListeners) listener(head);
}));
ipcRenderer.on(M5_INTERNAL_CHANNELS_V1.sidecarEvent, (_event, payload: EventV1) => delivered(payload, () => { for (const listener of sidecarListeners) listener(payload); }));
ipcRenderer.on(M5_BRIDGE_V2_CHANNELS.projectEvent, (_event, payload: { event: Parameters<M5BridgeV6["onProjectEvent"]>[0] extends (event: infer T) => unknown ? T : never; streamId: string; seq: number }) => delivered(payload, () => { for (const listener of projectListeners) listener(payload.event); }));
ipcRenderer.on(M5_BRIDGE_V3_CHANNELS.workspaceEvent, (_event, payload: { event: unknown; streamId: string; seq: number }) => delivered(payload, () => {
  const workspaceEvent = validateWorkspaceEventV1(payload.event);
  for (const listener of workspaceListeners) listener(workspaceEvent);
}));
ipcRenderer.on(M5_BRIDGE_V3_CHANNELS.chatEvent, (_event, payload: { event: unknown; streamId: string; seq: number }) => delivered(payload, () => {
  const message = validateTeamMessageV1(payload.event);
  for (const listener of chatListeners) listener(message);
}));
ipcRenderer.on(M5_BRIDGE_V4_CHANNELS.agentPublicEvent, (_event, payload: { event: unknown; streamId: string; seq: number }) => delivered(payload, () => {
  const agentPublicEvent = validateAgentPublicEventV1(payload.event);
  for (const listener of agentPublicEventListeners) listener(agentPublicEvent);
}));
ipcRenderer.on(M5_BRIDGE_V5_CHANNELS.agentRunStateChanged, (_event, payload: { event: unknown; streamId: string; seq: number }) => delivered(payload, () => {
  const stateChanged = validateAgentRunStateChangedV1(payload.event);
  for (const listener of agentRunStateChangedListeners) listener(stateChanged);
}));
function subscribe<T>(listeners: Set<(event: T) => void>, listener: (event: T) => void): () => void { listeners.add(listener); return () => listeners.delete(listener); }
function acknowledge(event: Pick<EventV1, "streamId" | "seq">): void { ipcRenderer.send(M5_BRIDGE_V2_CHANNELS.eventAck, { streamId: event.streamId, seq: event.seq }); }

function isProjectPickerCancelled(value: unknown): boolean {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const result = value as Record<string, unknown>;
  return Object.keys(result).length === 1 && result.outcome === "cancelled";
}

contextBridge.exposeInMainWorld("m5GitSelection", Object.freeze({
  request: async (action: GitSelectionAction) => validateGitSelection(await ipcRenderer.invoke(M5_GIT_SELECTION_CHANNEL, validateGitSelectionAction(action))),
}));

const M5_APP_UPDATE_CHANNEL = "m5:app-update:open";
const M5_UPDATE_READINESS = "m5:app-update:readiness";
const M5_UPDATE_READINESS_REPLY = "m5:app-update:readiness-reply";
const M5_APP_UPDATE_STATE = "m5:app-update:state";
const M5_APP_UPDATE_REQUEST = "m5:app-update:request";
const M5_APP_UPDATE_CHANGED = "m5:app-update:changed";
const APP_UPDATE_STATES = ["unavailable", "idle", "current", "checking", "downloading", "ready", "restarting", "error"] as const;
type AppUpdateState = typeof APP_UPDATE_STATES[number];
const asUpdateState = (value: unknown): AppUpdateState => (APP_UPDATE_STATES as readonly unknown[]).includes(value) ? value as AppUpdateState : "unavailable";
const appUpdateApi = Object.freeze({
  open: () => ipcRenderer.invoke(M5_APP_UPDATE_CHANNEL),
  // The in-app banner (ADE U8): main's state, and requests main decides on.
  state: async (): Promise<AppUpdateState> => asUpdateState(await ipcRenderer.invoke(M5_APP_UPDATE_STATE)),
  onState: (listener: (state: AppUpdateState) => void) => {
    const handler = (_event: Electron.IpcRendererEvent, state: unknown) => listener(asUpdateState(state));
    ipcRenderer.on(M5_APP_UPDATE_CHANGED, handler);
    return () => { ipcRenderer.removeListener(M5_APP_UPDATE_CHANGED, handler); };
  },
  request: async (action: unknown): Promise<AppUpdateState> => {
    if (action !== "check" && action !== "restart") throw new Error("invalid app update request");
    return asUpdateState(await ipcRenderer.invoke(M5_APP_UPDATE_REQUEST, action));
  },
  onRestartCheck: (check: () => boolean, resume: () => void) => {
    const listener = (_event: Electron.IpcRendererEvent, nonce: unknown) => {
      if (typeof nonce !== "string") return;
      let ready = false;
      try { ready = check() === true; } catch { /* Fail closed. */ }
      void ipcRenderer.invoke(M5_UPDATE_READINESS_REPLY, nonce, ready).catch(() => resume());
    };
    ipcRenderer.on(M5_UPDATE_READINESS, listener);
    const resumeListener = () => resume();
    ipcRenderer.on("m5:app-update:resume", resumeListener);
    return () => { ipcRenderer.removeListener(M5_UPDATE_READINESS, listener); ipcRenderer.removeListener("m5:app-update:resume", resumeListener); };
  },
});
contextBridge.exposeInMainWorld("m5AppUpdate", appUpdateApi);

// Screen brightness choice (system / light / dark). main keeps the record the
// settings screen starts from and follows the choice for native parts.
const M5_APPEARANCE_CHANNEL = "m5:appearance:set";
const M5_APPEARANCE_GET_CHANNEL = "m5:appearance:get";
const isTheme = (value: unknown): value is "system" | "light" | "dark" => value === "system" || value === "light" || value === "dark";
const field = (value: unknown, name: string): unknown => typeof value === "object" && value !== null ? (value as Record<string, unknown>)[name] : undefined;
contextBridge.exposeInMainWorld("m5Appearance", Object.freeze({
  get: async () => {
    const result: unknown = await ipcRenderer.invoke(M5_APPEARANCE_GET_CHANNEL);
    const theme = field(result, "theme");
    return { theme: isTheme(theme) ? theme : "system", saved: field(result, "saved") === true && isTheme(theme), dark: field(result, "dark") === true };
  },
  set: async (theme: unknown) => {
    if (!isTheme(theme)) throw new Error("invalid appearance");
    const result: unknown = await ipcRenderer.invoke(M5_APPEARANCE_CHANNEL, theme);
    return { dark: field(result, "dark") === true, saved: field(result, "saved") === true };
  },
}));

contextBridge.exposeInMainWorld("m5ClaudeUsage", Object.freeze({
  read: async () => validateClaudeUsageResult(await ipcRenderer.invoke(M5_CLAUDE_USAGE_CHANNEL)),
}));

// Result preview (ADE 1차-나 F12). On by default; with `ELSECREW_PREVIEW=0`
// main refuses all but `status`. The page shown in a preview never
// gets this bridge: its view has no preload at all.
const previewListeners = new Set<(event: PreviewEventV1) => void>();
ipcRenderer.on(M5_PREVIEW_EVENT_CHANNEL, (_event, payload: unknown) => {
  let event: PreviewEventV1;
  try { event = validatePreviewEventV1(payload); } catch { return; }
  for (const listener of previewListeners) listener(event);
});
async function previewCall<T>(input: unknown): Promise<T> {
  const request = validatePreviewIpcRequestV1(input);
  return validatePreviewIpcResultV1(await ipcRenderer.invoke(M5_PREVIEW_CHANNEL, request), request) as T;
}
type PreviewBridgeResult<K extends keyof M5PreviewBridgeV1> = Awaited<ReturnType<M5PreviewBridgeV1[K]>>;
const previewBridge: M5PreviewBridgeV1 = Object.freeze({
  status: () => previewCall<PreviewBridgeResult<"status">>({ action: "status" }),
  open: (input: Parameters<M5PreviewBridgeV1["open"]>[0]) => previewCall<PreviewBridgeResult<"open">>({ ...input, action: "open" }),
  layout: (input: Parameters<M5PreviewBridgeV1["layout"]>[0]) => previewCall<PreviewBridgeResult<"layout">>({ ...input, action: "layout" }),
  reload: (input: { previewId: string }) => previewCall<PreviewBridgeResult<"reload">>({ ...input, action: "reload" }),
  hide: (input: { previewId: string }) => previewCall<PreviewBridgeResult<"hide">>({ ...input, action: "hide" }),
  close: (input: { previewId: string }) => previewCall<PreviewBridgeResult<"close">>({ ...input, action: "close" }),
  onEvent: (listener: (event: PreviewEventV1) => void) => subscribe(previewListeners, listener),
});
contextBridge.exposeInMainWorld("m5Preview", previewBridge);
