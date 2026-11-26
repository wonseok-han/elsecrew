/**
 * Result preview (ADE 1차-나 F12; `docs/proposals/m5-ade-layout-plan.md` 2장).
 *
 * Two contracts live here:
 * - the sidecar methods `preview.resolve` / `preview.readAsset` (read-only;
 *   the same rules as `crates/elsecrew-protocol/src/preview_v1.rs`), and
 * - the renderer ↔ main channel `m5:preview:v1` (preload `window.m5Preview`).
 *
 * The page shown in a preview is untrusted. The renderer never names a
 * workspace address, a token or a URL: it names the project and the chat, and
 * main asks the sidecar which workspace that chat owns.
 */
import { exactKeys, fail, requireObject } from "./project-v1.ts";

export const PREVIEW_METHODS_V1 = ["preview.resolve", "preview.readAsset"] as const;
export const PREVIEW_ASSET_MAX_BYTES_V1 = 5 * 1024 * 1024;
export const PREVIEW_ASSET_CHUNK_BYTES_V1 = 512 * 1024;
export const PREVIEW_PATH_MAX_BYTES_V1 = 1024;
export const PREVIEW_PATH_MAX_SEGMENTS_V1 = 32;
/** One opened preview (one token): total bytes and requests before it closes. */
export const PREVIEW_TOKEN_MAX_BYTES_V1 = 64 * 1024 * 1024;
export const PREVIEW_TOKEN_MAX_REQUESTS_V1 = 2000;
export const PREVIEW_ENTRY_CANDIDATES_V1 = [
  "index.html", "public/index.html", "dist/index.html", "build/index.html", "docs/index.html",
] as const;

/** Content type by extension only: no sniffing. Every other extension is not found. */
export const PREVIEW_MIME_V1: Readonly<Record<string, string>> = Object.freeze({
  html: "text/html; charset=utf-8",
  htm: "text/html; charset=utf-8",
  css: "text/css; charset=utf-8",
  js: "text/javascript; charset=utf-8",
  mjs: "text/javascript; charset=utf-8",
  json: "application/json; charset=utf-8",
  map: "application/json; charset=utf-8",
  txt: "text/plain; charset=utf-8",
  md: "text/plain; charset=utf-8",
  svg: "image/svg+xml",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  avif: "image/avif",
  ico: "image/x-icon",
  bmp: "image/bmp",
  woff: "font/woff",
  woff2: "font/woff2",
  ttf: "font/ttf",
  otf: "font/otf",
  mp3: "audio/mpeg",
  wav: "audio/wav",
  ogg: "audio/ogg",
  mp4: "video/mp4",
  webm: "video/webm",
});
export const PREVIEW_EXTENSIONS_V1 = Object.freeze(Object.keys(PREVIEW_MIME_V1));
const ENTRY_EXTENSIONS = new Set(["html", "htm"]);

export type PreviewPathRefusalV1 =
  | "empty" | "tooLong" | "notRelative" | "control" | "encoded" | "tooDeep" | "traversal" | "hidden" | "extension";

/** Mirrors `preview_path_refusal_v1`: why a path never reaches a file. */
export function previewPathRefusalV1(path: string): PreviewPathRefusalV1 | null {
  if (path.length === 0) return "empty";
  if (new TextEncoder().encode(path).length > PREVIEW_PATH_MAX_BYTES_V1) return "tooLong";
  if (path.startsWith("/") || path.includes("\\") || path.includes(":")) return "notRelative";
  // Rust `char::is_control`: C0, DEL and C1.
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(path)) return "control";
  const lower = path.toLowerCase();
  if (["%2e", "%2f", "%5c", "%00"].some(encoded => lower.includes(encoded))) return "encoded";
  const segments = path.split("/");
  if (segments.length > PREVIEW_PATH_MAX_SEGMENTS_V1) return "tooDeep";
  for (const segment of segments) {
    if (segment === "" || segment === "." || segment === "..") return "traversal";
    if (segment.startsWith(".")) return "hidden";
  }
  if (previewExtensionV1(path) === null) return "extension";
  return null;
}

export function previewExtensionV1(path: string): string | null {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return null;
  const extension = name.slice(dot + 1).toLowerCase();
  return Object.prototype.hasOwnProperty.call(PREVIEW_MIME_V1, extension) ? extension : null;
}

export function isPreviewEntryV1(path: string): boolean {
  const extension = previewPathRefusalV1(path) === null ? previewExtensionV1(path) : null;
  return extension !== null && ENTRY_EXTENSIONS.has(extension);
}

// ---------------------------------------------------------------------------
// Sidecar methods

/** The same bound as `requireId` (project-v1), without control characters. */
function id(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 512 || /[\u0000-\u001f\u007f]/u.test(value)) fail(`${field} is invalid`);
  return value as string;
}
const PREVIEW_HANDLE = /^ph_[0-9a-f]{32}$/;
function previewHandle(value: unknown): string {
  if (typeof value !== "string" || !PREVIEW_HANDLE.test(value)) fail("previewHandle is invalid");
  return value as string;
}
function entryPath(value: unknown, field: string): string | null {
  if (value === null) return null;
  if (typeof value !== "string" || !isPreviewEntryV1(value)) fail(`${field} must be a preview page path or null`);
  return value as string;
}
function assetPath(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || new TextEncoder().encode(value).length > PREVIEW_PATH_MAX_BYTES_V1) {
    fail("preview path is invalid");
  }
  return value as string;
}

export interface PreviewResolveRequestV1 { projectId: string; chatId: string; entry: string | null }
export interface PreviewWorkspaceRefV1 { chatId: string; projectId: string; worktreeId: string; workspaceIdentity: string }
export type PreviewEntryRefusalV1 = PreviewPathRefusalV1 | "notPage" | "notFound";
export interface PreviewResolveResultV1 extends PreviewWorkspaceRefV1 {
  /** The sidecar's record of the workspace root it verified; every read names it. */
  previewHandle: string;
  entry: string | null;
  refusal: PreviewEntryRefusalV1 | null;
}
export interface PreviewReadAssetRequestV1 extends PreviewWorkspaceRefV1 { previewHandle: string; path: string; offset: number }
export type PreviewAssetOutcomeV1 = "ok" | "notFound" | "refused" | "tooLarge";
export interface PreviewReadAssetResultV1 extends PreviewWorkspaceRefV1 {
  path: string;
  outcome: PreviewAssetOutcomeV1;
  byteLength: number | null;
  version: `sha256:${string}` | null;
  offset: number;
  bytesBase64: string | null;
}

const REFUSALS = new Set<PreviewEntryRefusalV1>([
  "empty", "tooLong", "notRelative", "control", "encoded", "tooDeep", "traversal", "hidden", "extension", "notPage", "notFound",
]);

export function validatePreviewResolveRequestV1(value: unknown): PreviewResolveRequestV1 {
  const object = requireObject(value, "preview resolve request");
  exactKeys(object, ["projectId", "chatId", "entry"]);
  return { projectId: id(object.projectId, "projectId"), chatId: id(object.chatId, "chatId"), entry: entryPath(object.entry, "entry") };
}

export function validatePreviewResolveResultV1(value: unknown, request: PreviewResolveRequestV1): PreviewResolveResultV1 {
  const object = requireObject(value, "preview resolve result");
  exactKeys(object, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "previewHandle", "entry", "refusal"]);
  if (object.chatId !== request.chatId || object.projectId !== request.projectId) fail("preview resolve answered another chat");
  const entry = object.entry === null ? null : entryPath(object.entry, "entry");
  if (request.entry !== null && entry !== null && entry !== request.entry) fail("preview resolve changed the named entry");
  const refusal = object.refusal;
  if (refusal !== null && (typeof refusal !== "string" || !REFUSALS.has(refusal as PreviewEntryRefusalV1))) fail("preview refusal is invalid");
  if (entry !== null && refusal !== null) fail("a resolved preview entry has no refusal");
  if (request.entry === null && refusal !== null) fail("preview candidates are never refused");
  return {
    chatId: request.chatId, projectId: request.projectId,
    worktreeId: id(object.worktreeId, "worktreeId"), workspaceIdentity: id(object.workspaceIdentity, "workspaceIdentity"),
    previewHandle: previewHandle(object.previewHandle), entry, refusal: refusal as PreviewEntryRefusalV1 | null,
  };
}

export function validatePreviewReadAssetRequestV1(value: unknown): PreviewReadAssetRequestV1 {
  const object = requireObject(value, "preview read request");
  exactKeys(object, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "previewHandle", "path", "offset"]);
  const offset = object.offset;
  if (typeof offset !== "number" || !Number.isSafeInteger(offset) || offset < 0 || offset >= PREVIEW_ASSET_MAX_BYTES_V1) {
    fail("preview read offset is invalid");
  }
  return {
    chatId: id(object.chatId, "chatId"), projectId: id(object.projectId, "projectId"),
    worktreeId: id(object.worktreeId, "worktreeId"), workspaceIdentity: id(object.workspaceIdentity, "workspaceIdentity"),
    previewHandle: previewHandle(object.previewHandle), path: assetPath(object.path), offset: offset as number,
  };
}

const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;
export function validatePreviewReadAssetResultV1(value: unknown, request: PreviewReadAssetRequestV1): PreviewReadAssetResultV1 {
  const object = requireObject(value, "preview read result");
  exactKeys(object, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "path", "outcome", "byteLength", "version", "offset", "bytesBase64"]);
  for (const field of ["chatId", "projectId", "worktreeId", "workspaceIdentity", "path", "offset"] as const) {
    if (object[field] !== request[field]) fail(`preview read answered another ${field}`);
  }
  const outcome = object.outcome;
  if (outcome !== "ok" && outcome !== "notFound" && outcome !== "refused" && outcome !== "tooLarge") fail("preview read outcome is invalid");
  if (outcome !== "ok") {
    if (object.byteLength !== null || object.version !== null || object.bytesBase64 !== null) fail("a failed preview read carries no content");
    return { ...request, outcome, byteLength: null, version: null, bytesBase64: null };
  }
  const length = object.byteLength;
  if (typeof length !== "number" || !Number.isSafeInteger(length) || length < 0 || length > PREVIEW_ASSET_MAX_BYTES_V1) fail("preview length is invalid");
  if (typeof object.version !== "string" || !/^sha256:[0-9a-f]{64}$/.test(object.version)) fail("preview version is invalid");
  const bytes = object.bytesBase64;
  if (typeof bytes !== "string" || !BASE64.test(bytes) || bytes.length % 4 !== 0) fail("preview bytes are invalid");
  const decoded = decodedLength(bytes as string);
  const expected = Math.min(PREVIEW_ASSET_CHUNK_BYTES_V1, (length as number) - request.offset);
  if (expected < 0 || decoded !== expected || (request.offset > 0 && request.offset >= (length as number))) fail("preview chunk does not match its file");
  return { ...request, outcome, byteLength: length as number, version: object.version as `sha256:${string}`, bytesBase64: bytes as string };
}

function decodedLength(base64: string): number {
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return (base64.length / 4) * 3 - padding;
}

// ---------------------------------------------------------------------------
// Renderer ↔ main (`m5:preview:v1`)

export const PREVIEW_CHANNEL_V1 = "m5:preview:v1";
export const PREVIEW_EVENT_CHANNEL_V1 = "m5:preview:v1:event";
export const PREVIEW_MIN_SIZE_V1 = 120;
export const PREVIEW_NARROW_WIDTH_V1 = 390;

/** In the main window's content coordinates (CSS pixels of the app page). */
export interface PreviewBoundsV1 { x: number; y: number; width: number; height: number }
export type PreviewViewportV1 = "wide" | "narrow";

export type PreviewIpcRequestV1 =
  | { action: "status" }
  | { action: "open"; projectId: string; chatId: string; entry: string | null; bounds: PreviewBoundsV1; viewport: PreviewViewportV1 }
  /** `obscured`: something of the app's own covers the area (dialog, sheet,
   * drawer, popover, toast). The view hides and the answer carries a still
   * image to show in its place. */
  | { action: "layout"; previewId: string; bounds: PreviewBoundsV1; viewport: PreviewViewportV1; obscured: boolean }
  | { action: "reload"; previewId: string }
  /** Another tab: hidden now, closed after 30 seconds unless shown again. */
  | { action: "hide"; previewId: string }
  | { action: "close"; previewId: string };

export type PreviewStateV1 = "loading" | "shown" | "hidden" | "closed";
export type PreviewEmptyReasonV1 = "noEntry" | PreviewEntryRefusalV1;

export type PreviewIpcResultV1 =
  | { action: "status"; enabled: boolean }
  | { action: "open"; outcome: "opened"; previewId: string; entry: string; state: "loading" }
  /** Nothing to show: the task has no page yet (or the named one is refused). */
  | { action: "open"; outcome: "empty"; reason: PreviewEmptyReasonV1 }
  | { action: "layout"; previewId: string; state: PreviewStateV1; snapshot: string | null }
  | { action: "reload" | "hide" | "close"; previewId: string; state: PreviewStateV1 };

/** Main → renderer notices. `closed` carries why, so the screen can offer "다시 열기". */
export type PreviewEventV1 =
  | { previewId: string; kind: "loaded" }
  | { previewId: string; kind: "loadFailed" }
  | { previewId: string; kind: "blockedNavigation" }
  | { previewId: string; kind: "unresponsive" }
  | { previewId: string; kind: "responsive" }
  | { previewId: string; kind: "closed"; reason: PreviewCloseReasonV1 };
export type PreviewCloseReasonV1 = "requested" | "idle" | "replaced" | "crashed" | "limit" | "workspaceUnavailable" | "quitting" | "windowClosed" | "reloaded";

const PREVIEW_ID = /^pv_[0-9a-f]{32}$/;
function previewId(value: unknown): string {
  if (typeof value !== "string" || !PREVIEW_ID.test(value)) fail("previewId is invalid");
  return value as string;
}
function viewport(value: unknown): PreviewViewportV1 {
  if (value !== "wide" && value !== "narrow") fail("viewport is invalid");
  return value as PreviewViewportV1;
}
function bounds(value: unknown): PreviewBoundsV1 {
  const object = requireObject(value, "preview bounds");
  exactKeys(object, ["x", "y", "width", "height"]);
  for (const field of ["x", "y", "width", "height"] as const) {
    const number = object[field];
    if (typeof number !== "number" || !Number.isFinite(number) || Math.abs(number) > 100_000) fail(`preview bounds ${field} is invalid`);
  }
  return { x: object.x as number, y: object.y as number, width: object.width as number, height: object.height as number };
}

export function validatePreviewIpcRequestV1(value: unknown): PreviewIpcRequestV1 {
  const object = requireObject(value, "preview request");
  switch (object.action) {
    case "status":
      exactKeys(object, ["action"]);
      return { action: "status" };
    case "open":
      exactKeys(object, ["action", "projectId", "chatId", "entry", "bounds", "viewport"]);
      return {
        action: "open", projectId: id(object.projectId, "projectId"), chatId: id(object.chatId, "chatId"),
        entry: entryPath(object.entry, "entry"), bounds: bounds(object.bounds), viewport: viewport(object.viewport),
      };
    case "layout":
      exactKeys(object, ["action", "previewId", "bounds", "viewport", "obscured"]);
      if (typeof object.obscured !== "boolean") fail("obscured must be a boolean");
      return { action: "layout", previewId: previewId(object.previewId), bounds: bounds(object.bounds), viewport: viewport(object.viewport), obscured: object.obscured as boolean };
    case "reload": case "hide": case "close":
      exactKeys(object, ["action", "previewId"]);
      return { action: object.action, previewId: previewId(object.previewId) };
    default:
      return fail("preview action is unknown");
  }
}

const STATES = new Set<PreviewStateV1>(["loading", "shown", "hidden", "closed"]);
const EMPTY_REASONS = new Set<string>(["noEntry", ...REFUSALS]);
const SNAPSHOT = /^data:image\/jpeg;base64,[A-Za-z0-9+/]+={0,2}$/;
export const PREVIEW_SNAPSHOT_MAX_CHARS_V1 = 8 * 1024 * 1024;

export function validatePreviewIpcResultV1(value: unknown, request: PreviewIpcRequestV1): PreviewIpcResultV1 {
  const object = requireObject(value, "preview result");
  if (object.action !== request.action) fail("preview result answers another action");
  const state = (candidate: unknown) => {
    if (typeof candidate !== "string" || !STATES.has(candidate as PreviewStateV1)) fail("preview state is invalid");
    return candidate as PreviewStateV1;
  };
  switch (request.action) {
    case "status":
      exactKeys(object, ["action", "enabled"]);
      if (typeof object.enabled !== "boolean") fail("enabled must be a boolean");
      return { action: "status", enabled: object.enabled as boolean };
    case "open":
      if (object.outcome === "empty") {
        exactKeys(object, ["action", "outcome", "reason"]);
        if (typeof object.reason !== "string" || !EMPTY_REASONS.has(object.reason)) fail("empty reason is invalid");
        return { action: "open", outcome: "empty", reason: object.reason as PreviewEmptyReasonV1 };
      }
      exactKeys(object, ["action", "outcome", "previewId", "entry", "state"]);
      if (object.outcome !== "opened" || object.state !== "loading") fail("preview open result is invalid");
      return { action: "open", outcome: "opened", previewId: previewId(object.previewId), entry: entryPath(object.entry, "entry") ?? fail("entry is required"), state: "loading" };
    case "layout": {
      exactKeys(object, ["action", "previewId", "state", "snapshot"]);
      if (object.previewId !== request.previewId) fail("preview layout answered another preview");
      const snapshot = object.snapshot;
      if (snapshot !== null && (typeof snapshot !== "string" || snapshot.length > PREVIEW_SNAPSHOT_MAX_CHARS_V1 || !SNAPSHOT.test(snapshot))) fail("snapshot is invalid");
      return { action: "layout", previewId: request.previewId, state: state(object.state), snapshot: snapshot as string | null };
    }
    default:
      exactKeys(object, ["action", "previewId", "state"]);
      if (object.previewId !== request.previewId) fail("preview result answered another preview");
      return { action: request.action, previewId: request.previewId, state: state(object.state) };
  }
}

const EVENT_KINDS = new Set(["loaded", "loadFailed", "blockedNavigation", "unresponsive", "responsive", "closed"]);
const CLOSE_REASONS = new Set<PreviewCloseReasonV1>(["requested", "idle", "replaced", "crashed", "limit", "workspaceUnavailable", "quitting", "windowClosed", "reloaded"]);
export function validatePreviewEventV1(value: unknown): PreviewEventV1 {
  const object = requireObject(value, "preview event");
  if (typeof object.kind !== "string" || !EVENT_KINDS.has(object.kind)) fail("preview event kind is invalid");
  if (object.kind === "closed") {
    exactKeys(object, ["previewId", "kind", "reason"]);
    if (typeof object.reason !== "string" || !CLOSE_REASONS.has(object.reason as PreviewCloseReasonV1)) fail("close reason is invalid");
    return { previewId: previewId(object.previewId), kind: "closed", reason: object.reason as PreviewCloseReasonV1 };
  }
  exactKeys(object, ["previewId", "kind"]);
  return { previewId: previewId(object.previewId), kind: object.kind as Exclude<PreviewEventV1["kind"], "closed"> } as PreviewEventV1;
}

/** `window.m5Preview` (preload). On by default; with `ELSECREW_PREVIEW=0`
 * main refuses everything but `status`. */
export interface M5PreviewBridgeV1 {
  status(): Promise<{ enabled: boolean }>;
  open(input: { projectId: string; chatId: string; entry: string | null; bounds: PreviewBoundsV1; viewport: PreviewViewportV1 }): Promise<Extract<PreviewIpcResultV1, { action: "open" }>>;
  layout(input: { previewId: string; bounds: PreviewBoundsV1; viewport: PreviewViewportV1; obscured: boolean }): Promise<Extract<PreviewIpcResultV1, { action: "layout" }>>;
  reload(input: { previewId: string }): Promise<{ previewId: string; state: PreviewStateV1 }>;
  hide(input: { previewId: string }): Promise<{ previewId: string; state: PreviewStateV1 }>;
  close(input: { previewId: string }): Promise<{ previewId: string; state: PreviewStateV1 }>;
  onEvent(listener: (event: PreviewEventV1) => void): () => void;
}
