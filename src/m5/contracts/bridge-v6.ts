import { M5_BRIDGE_V5_METHODS, type M5BridgeV5 } from "./bridge-v5.ts";
import type { SystemStatusG5DV1 } from "./lifecycle-g5d-v1.ts";
import {
  STORAGE_STATUS_BRIDGE_METHOD_V1,
  type StorageStatusRequestV1,
  type StorageStatusV1,
} from "./storage-lifetime-v1.ts";

/** Planned G5D D0A surface. It is not advertised or dispatchable until D1 implements it end to end. */
export const M5_BRIDGE_V6_METHODS = [
  ...M5_BRIDGE_V5_METHODS,
  STORAGE_STATUS_BRIDGE_METHOD_V1,
] as const;

export type M5BridgeV6 = Omit<M5BridgeV5, "version" | "getSystemStatus"> & {
  readonly version: 6;
  getSystemStatus(): Promise<SystemStatusG5DV1>;
  getStorageStatus(input: StorageStatusRequestV1): Promise<StorageStatusV1>;
};
