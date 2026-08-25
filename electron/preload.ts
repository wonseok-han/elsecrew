import { contextBridge, ipcRenderer } from "electron";

import { M5_INTERNAL_CHANNELS_V1 } from "../src/m5/contracts/lifecycle-v1.ts";
import type { M5BridgeV2 } from "../src/m5/contracts/bridge-v2.ts";
import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";
import { validateProjectIdentityV1 } from "../src/m5/contracts/project-v1.ts";
import { M5_BRIDGE_V2_CHANNELS } from "./ipc-channels.ts";
import { validatePathInput, validateProjectInput, validateSaveBeginInput, validateSaveChunkInput, validateSaveCommitInput, validateTreeInput } from "./bridge-input.ts";

const bridge: M5BridgeV2 = Object.freeze({
  version: 2,
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
});

contextBridge.exposeInMainWorld("m5", bridge);

const sidecarListeners = new Set<(event: EventV1) => void>();
const projectListeners = new Set<Parameters<M5BridgeV2["onProjectEvent"]>[0]>();
ipcRenderer.on(M5_INTERNAL_CHANNELS_V1.sidecarEvent, (_event, payload: EventV1) => { for (const listener of sidecarListeners) listener(payload); acknowledge(payload); });
ipcRenderer.on(M5_BRIDGE_V2_CHANNELS.projectEvent, (_event, payload: { event: Parameters<M5BridgeV2["onProjectEvent"]>[0] extends (event: infer T) => unknown ? T : never; streamId: string; seq: number }) => { for (const listener of projectListeners) listener(payload.event); acknowledge(payload); });
function subscribe<T>(listeners: Set<(event: T) => void>, listener: (event: T) => void): () => void { listeners.add(listener); return () => listeners.delete(listener); }
function acknowledge(event: Pick<EventV1, "streamId" | "seq">): void { ipcRenderer.send(M5_BRIDGE_V2_CHANNELS.eventAck, { streamId: event.streamId, seq: event.seq }); }

function isProjectPickerCancelled(value: unknown): boolean {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const result = value as Record<string, unknown>;
  return Object.keys(result).length === 1 && result.outcome === "cancelled";
}
