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

export interface FileDigestVectorV1 {
  inputHex: string;
  version: FileVersionV1;
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

export function validateFileDigestVectorV1(value: unknown): FileDigestVectorV1 {
  const object = requireObject(value, "file digest vector");
  exactKeys(object, ["inputHex", "version"]);
  if (typeof object.inputHex !== "string" || !/^(?:[0-9a-f]{2})*$/.test(object.inputHex)) {
    fail("digest inputHex must encode exact bytes as lowercase hex");
  }
  const expected = validateFileVersionV1(object.version);
  const bytes = new Uint8Array(object.inputHex.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(object.inputHex.slice(index * 2, index * 2 + 2), 16);
  }
  if (sha256FileVersionV1(bytes) !== expected) fail("file version digest does not match exact bytes");
  return object as unknown as FileDigestVectorV1;
}

export function sha256FileVersionV1(input: Uint8Array): FileVersionV1 {
  const paddedLength = Math.ceil((input.byteLength + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLength);
  padded.set(input);
  padded[input.byteLength] = 0x80;
  const bitLength = BigInt(input.byteLength) * 8n;
  const view = new DataView(padded.buffer);
  view.setUint32(paddedLength - 8, Number((bitLength >> 32n) & 0xffff_ffffn));
  view.setUint32(paddedLength - 4, Number(bitLength & 0xffff_ffffn));

  const hash = new Uint32Array([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ]);
  const words = new Uint32Array(64);
  for (let offset = 0; offset < paddedLength; offset += 64) {
    for (let index = 0; index < 16; index += 1) {
      words[index] = view.getUint32(offset + index * 4);
    }
    for (let index = 16; index < 64; index += 1) {
      const left = words[index - 15];
      const right = words[index - 2];
      const sigma0 = rotateRight(left, 7) ^ rotateRight(left, 18) ^ (left >>> 3);
      const sigma1 = rotateRight(right, 17) ^ rotateRight(right, 19) ^ (right >>> 10);
      words[index] = (words[index - 16] + sigma0 + words[index - 7] + sigma1) >>> 0;
    }
    let [a, b, c, d, e, f, g, h] = hash;
    for (let index = 0; index < 64; index += 1) {
      const choice = (e & f) ^ (~e & g);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const sum0 = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
      const sum1 = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
      const first = (h + sum1 + choice + SHA256_ROUND_CONSTANTS[index] + words[index]) >>> 0;
      const second = (sum0 + majority) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d + first) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (first + second) >>> 0;
    }
    hash[0] = (hash[0] + a) >>> 0;
    hash[1] = (hash[1] + b) >>> 0;
    hash[2] = (hash[2] + c) >>> 0;
    hash[3] = (hash[3] + d) >>> 0;
    hash[4] = (hash[4] + e) >>> 0;
    hash[5] = (hash[5] + f) >>> 0;
    hash[6] = (hash[6] + g) >>> 0;
    hash[7] = (hash[7] + h) >>> 0;
  }
  return `${FILE_VERSION_PREFIX_V1}${[...hash]
    .map((word) => word.toString(16).padStart(8, "0"))
    .join("")}` as FileVersionV1;
}

const SHA256_ROUND_CONSTANTS = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
  0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
  0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
  0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
  0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
  0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
  0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
  0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
  0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

function rotateRight(value: number, bits: number): number {
  return (value >>> bits) | (value << (32 - bits));
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
