import { contextBridge, ipcRenderer } from "electron";

import { M5_INTERNAL_CHANNELS_V1 } from "../src/m5/contracts/lifecycle-v1.ts";
import type { M5BridgeV3 } from "../src/m5/contracts/bridge-v3.ts";
import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";
import { validateProjectIdentityV1 } from "../src/m5/contracts/project-v1.ts";
import { validateTeamMessageV1 } from "../src/m5/contracts/run-recovery-v1.ts";
import { validateWorkspaceEventV1 } from "../src/m5/contracts/workspace-addressing-v1.ts";
import { M5_BRIDGE_V2_CHANNELS, M5_BRIDGE_V3_CHANNELS } from "./ipc-channels.ts";
import {
  validateChatCreateInput, validateChatGetInput, validateChatListInput, validateFixtureRunStartInput,
  validatePathInput, validateProjectInput, validateRecoveryInspectInput, validateRecoveryListInput,
  validateRecoveryMarkResolvedInput, validateRoleSettingsGetInput, validateRoleSettingsUpdateInput,
  validateRunCancelInput, validateSaveBeginInput, validateSaveChunkInput, validateSaveCommitInput,
  validateTreeInput, validateWorkspaceDiffFileInput, validateWorkspaceFileReadInput,
  validateWorkspaceRefInput, validateWorkspaceSaveBeginInput, validateWorkspaceSaveChunkInput,
  validateWorkspaceSaveCommitInput, validateWorkspaceTreeInput,
} from "./bridge-input.ts";

const bridge: M5BridgeV3 = Object.freeze({
  version: 3,
  getSystemStatus: () => ipcRenderer.invoke(M5_INTERNAL_CHANNELS_V1.systemGetStatus),
  prepareQuit: () => ipcRenderer.invoke(M5_INTERNAL_CHANNELS_V1.appPrepareQuit),
  onSidecarEvent: (listener) => subscribe(sidecarListeners, listener),
  openProject: async () => {
    const result: unknown = await ipcRenderer.invoke(M5_BRIDGE_V2_CHANNELS.projectOpen);
    if (isProjectPickerCancelled(result)) throw new DOMException("Project selection cancelled", "AbortError");
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
});

contextBridge.exposeInMainWorld("m5", bridge);

const sidecarListeners = new Set<(event: EventV1) => void>();
const projectListeners = new Set<Parameters<M5BridgeV3["onProjectEvent"]>[0]>();
const workspaceListeners = new Set<Parameters<M5BridgeV3["onWorkspaceEvent"]>[0]>();
const chatListeners = new Set<Parameters<M5BridgeV3["onChatEvent"]>[0]>();
ipcRenderer.on(M5_INTERNAL_CHANNELS_V1.sidecarEvent, (_event, payload: EventV1) => { for (const listener of sidecarListeners) listener(payload); acknowledge(payload); });
ipcRenderer.on(M5_BRIDGE_V2_CHANNELS.projectEvent, (_event, payload: { event: Parameters<M5BridgeV3["onProjectEvent"]>[0] extends (event: infer T) => unknown ? T : never; streamId: string; seq: number }) => { for (const listener of projectListeners) listener(payload.event); acknowledge(payload); });
ipcRenderer.on(M5_BRIDGE_V3_CHANNELS.workspaceEvent, (_event, payload: { event: unknown; streamId: string; seq: number }) => {
  const workspaceEvent = validateWorkspaceEventV1(payload.event);
  for (const listener of workspaceListeners) listener(workspaceEvent);
  acknowledge(payload);
});
ipcRenderer.on(M5_BRIDGE_V3_CHANNELS.chatEvent, (_event, payload: { event: unknown; streamId: string; seq: number }) => {
  const message = validateTeamMessageV1(payload.event);
  for (const listener of chatListeners) listener(message);
  acknowledge(payload);
});
function subscribe<T>(listeners: Set<(event: T) => void>, listener: (event: T) => void): () => void { listeners.add(listener); return () => listeners.delete(listener); }
function acknowledge(event: Pick<EventV1, "streamId" | "seq">): void { ipcRenderer.send(M5_BRIDGE_V2_CHANNELS.eventAck, { streamId: event.streamId, seq: event.seq }); }

function isProjectPickerCancelled(value: unknown): boolean {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const result = value as Record<string, unknown>;
  return Object.keys(result).length === 1 && result.outcome === "cancelled";
}
