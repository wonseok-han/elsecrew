import { contextBridge } from "electron";

const bridge = Object.freeze({
  version: 1 as const,
});

contextBridge.exposeInMainWorld("m5", bridge);
