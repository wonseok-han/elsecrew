import type { M5BridgeV1, UnsubscribeV1 } from "./bridge-v1.ts";
import type {
  G4SafetyErrorV1,
  ProjectCloseResultV1,
  ProjectIdentityV1,
  TreePageV1,
  WatchEventV1,
  WorktreeManifestV1,
} from "./project-v1.ts";
import type {
  FileReadDescriptorV1,
  FileVersionV1,
  SaveBeginV1,
  SaveChunkV1,
  SaveCommitV1,
} from "./file-v1.ts";
import type { DiffFileDescriptorV1, DiffSummaryV1 } from "./diff-v1.ts";

export const M5_BRIDGE_V2_METHODS = [
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
] as const;

export type M5BridgeV2 = Omit<M5BridgeV1, "version"> & {
  readonly version: 2;
  openProject(): Promise<ProjectIdentityV1>;
  getProjectTree(input: {
    projectId: string;
    parentPath: string;
    cursor: string | null;
  }): Promise<TreePageV1>;
  createProjectWorktree(input: { projectId: string }): Promise<WorktreeManifestV1>;
  closeProject(input: { projectId: string }): Promise<ProjectCloseResultV1>;
  readFile(input: { projectId: string; path: string }): Promise<FileReadDescriptorV1>;
  beginFileSave(
    input: Omit<SaveBeginV1, "saveId" | "chunkBytes">,
  ): Promise<{ saveId: string; chunkBytes: SaveBeginV1["chunkBytes"] }>;
  appendFileSaveChunk(input: SaveChunkV1): Promise<{ acceptedSeq: number }>;
  commitFileSave(input: SaveCommitV1): Promise<
    | { outcome: "committed"; version: FileVersionV1 }
    | { outcome: "dirtyConflict"; error: G4SafetyErrorV1 }
  >;
  watchProjectFiles(input: { projectId: string }): Promise<{ watching: true }>;
  getDiffSummary(input: { projectId: string }): Promise<DiffSummaryV1>;
  getDiffFile(input: { projectId: string; path: string }): Promise<DiffFileDescriptorV1>;
  onProjectEvent(listener: (event: WatchEventV1) => void): UnsubscribeV1;
};
