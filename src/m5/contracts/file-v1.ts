import { MAX_SEQUENCE, MAX_TEXT_CHUNK_BYTES } from "./protocol-v1.ts";
import {
  exactKeys,
  fail,
  FILE_MAX_BYTES_V1,
  FILE_VERSION_ALGORITHM_V1,
  FILE_VERSION_PREFIX_V1,
  requireId,
  requireObject,
  requireRelativePath,
} from "./project-v1.ts";

export type FileClassificationV1 = "utf8" | "binary" | "tooLarge";
export type FileVersionV1 = `sha256:${string}`;

export interface FileClassificationProbeV1 {
  byteLength: number;
  utf8Valid: boolean;
  classification: FileClassificationV1;
}

export interface TextStreamDescriptorV1 {
  streamId: string;
  chunkBytes: typeof MAX_TEXT_CHUNK_BYTES;
}

export interface FileReadDescriptorV1 {
  projectId: string;
  path: string;
  classification: FileClassificationV1;
  byteLength: number;
  version: FileVersionV1;
  stream: TextStreamDescriptorV1 | null;
}

export interface SaveBeginV1 {
  saveId: string;
  projectId: string;
  path: string;
  expectedVersion: FileVersionV1;
  totalBytes: number;
  encoding: "utf8";
  chunkBytes: typeof MAX_TEXT_CHUNK_BYTES;
}

export interface SaveChunkV1 {
  saveId: string;
  seq: number;
  text: string;
}

export interface SaveCommitV1 {
  saveId: string;
  expectedVersion: FileVersionV1;
  lastSeq: number;
  totalBytes: number;
}

export interface SaveTranscriptV1 {
  begin: SaveBeginV1;
  chunks: SaveChunkV1[];
  commit: SaveCommitV1;
  currentVersion: FileVersionV1;
  outcome: "committed" | "dirtyConflict";
}

export function validateFileClassificationV1(value: unknown): FileClassificationProbeV1 {
  const object = requireObject(value, "file classification");
  exactKeys(object, ["byteLength", "utf8Valid", "classification"]);
  const byteLength = requireByteLength(object.byteLength, "file byteLength");
  if (typeof object.utf8Valid !== "boolean") fail("utf8Valid must be boolean");
  const expected: FileClassificationV1 =
    byteLength > FILE_MAX_BYTES_V1 ? "tooLarge" : object.utf8Valid ? "utf8" : "binary";
  if (object.classification !== expected) fail(`file classification must be ${expected}`);
  return object as unknown as FileClassificationProbeV1;
}

export function validateFileVersionV1(value: unknown): FileVersionV1 {
  if (
    typeof value !== "string" ||
    !new RegExp(`^${FILE_VERSION_PREFIX_V1}[0-9a-f]{64}$`).test(value)
  ) {
    fail(`file version must be ${FILE_VERSION_ALGORITHM_V1} with a lowercase 256-bit digest`);
  }
  return value as FileVersionV1;
}

export function validateFileReadDescriptorV1(value: unknown): FileReadDescriptorV1 {
  const object = requireObject(value, "file read descriptor");
  exactKeys(object, ["projectId", "path", "classification", "byteLength", "version", "stream"]);
  requireId(object.projectId, "projectId");
  requireRelativePath(object.path, "file path", false);
  validateFileClassificationV1({
    byteLength: object.byteLength,
    utf8Valid: object.classification === "utf8",
    classification: object.classification,
  });
  validateFileVersionV1(object.version);
  if (object.classification === "utf8") {
    const stream = requireObject(object.stream, "text stream");
    exactKeys(stream, ["streamId", "chunkBytes"]);
    requireId(stream.streamId, "streamId");
    if (stream.chunkBytes !== MAX_TEXT_CHUNK_BYTES) fail("text stream chunkBytes must be 256 KiB");
  } else if (object.stream !== null) {
    fail("binary and too-large files cannot expose a text stream");
  }
  return object as unknown as FileReadDescriptorV1;
}

export function validateSaveTranscriptV1(value: unknown): SaveTranscriptV1 {
  const object = requireObject(value, "save transcript");
  exactKeys(object, ["begin", "chunks", "commit", "currentVersion", "outcome"]);
  const begin = requireObject(object.begin, "save begin");
  exactKeys(begin, ["saveId", "projectId", "path", "expectedVersion", "totalBytes", "encoding", "chunkBytes"]);
  requireId(begin.saveId, "saveId");
  requireId(begin.projectId, "projectId");
  requireRelativePath(begin.path, "save path", false);
  validateFileVersionV1(begin.expectedVersion);
  const declaredBytes = requireByteLength(begin.totalBytes, "save totalBytes");
  if (declaredBytes > FILE_MAX_BYTES_V1) fail("save cannot exceed 5 MiB");
  if (begin.encoding !== "utf8" || begin.chunkBytes !== MAX_TEXT_CHUNK_BYTES) {
    fail("save begin must use UTF-8 and 256 KiB chunks");
  }
  if (!Array.isArray(object.chunks)) fail("save chunks must be an array");
  let receivedBytes = 0;
  for (let index = 0; index < object.chunks.length; index += 1) {
    const chunk = requireObject(object.chunks[index], "save chunk");
    exactKeys(chunk, ["saveId", "seq", "text"]);
    if (chunk.saveId !== begin.saveId) fail("save chunk saveId mismatch");
    if (chunk.seq !== index + 1 || chunk.seq > MAX_SEQUENCE) fail("save chunk seq must start at 1 and be contiguous");
    if (typeof chunk.text !== "string") fail("save chunk text is required");
    const bytes = new TextEncoder().encode(chunk.text).byteLength;
    if (bytes > MAX_TEXT_CHUNK_BYTES) fail("save chunk exceeds 256 KiB");
    receivedBytes += bytes;
  }
  const commit = requireObject(object.commit, "save commit");
  exactKeys(commit, ["saveId", "expectedVersion", "lastSeq", "totalBytes"]);
  if (commit.saveId !== begin.saveId || commit.expectedVersion !== begin.expectedVersion) {
    fail("save commit must match begin identity and expectedVersion");
  }
  if (commit.lastSeq !== object.chunks.length || commit.totalBytes !== declaredBytes || receivedBytes !== declaredBytes) {
    fail("save commit byte or sequence totals do not match received chunks");
  }
  const currentVersion = validateFileVersionV1(object.currentVersion);
  const expectedOutcome = currentVersion === begin.expectedVersion ? "committed" : "dirtyConflict";
  if (object.outcome !== expectedOutcome) fail(`save outcome must be ${expectedOutcome}`);
  return object as unknown as SaveTranscriptV1;
}

function requireByteLength(value: unknown, name: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) fail(`${name} must be a non-negative safe integer`);
  return value;
}
