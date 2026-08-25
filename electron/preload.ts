import { contextBridge, ipcRenderer } from "electron";

import { M5_INTERNAL_CHANNELS_V1 } from "../src/m5/contracts/lifecycle-v1.ts";
import type { M5BridgeV1 } from "../src/m5/contracts/bridge-v1.ts";
import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";

const bridge: M5BridgeV1 = Object.freeze({
  version: 1,
  getSystemStatus: () => ipcRenderer.invoke(M5_INTERNAL_CHANNELS_V1.systemGetStatus),
  prepareQuit: () => ipcRenderer.invoke(M5_INTERNAL_CHANNELS_V1.appPrepareQuit),
  onSidecarEvent: (listener) => {
    const wrapped = (_event: Electron.IpcRendererEvent, payload: EventV1) => listener(payload);
    ipcRenderer.on(M5_INTERNAL_CHANNELS_V1.sidecarEvent, wrapped);
    return () => ipcRenderer.removeListener(M5_INTERNAL_CHANNELS_V1.sidecarEvent, wrapped);
  },
});

contextBridge.exposeInMainWorld("m5", bridge);
