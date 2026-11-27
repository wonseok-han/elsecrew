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

export const PREVIEW_METHODS_V1 = ["preview.resolve", "preview.readAsset", "preview.buildPlan", "preview.buildReserve", "preview.buildRelease", "preview.buildHolds"] as const;
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

/** In CSS pixels of the app page (`getBoundingClientRect`). Main turns them
 * into window coordinates with the page's zoom factor; `pixelRatio` is the
 * page's `devicePixelRatio` when it measured, so main can tell a measurement
 * taken before a zoom change (PR #9 review B1). `page` is the page's own
 * size (`innerWidth`/`innerHeight`) when it measured: main uses a
 * measurement only when that size at the current zoom is the window's
 * (a measurement from before a resize keeps the view hidden). */
export interface PreviewBoundsV1 { x: number; y: number; width: number; height: number }
export interface PreviewPageSizeV1 { width: number; height: number }
export type PreviewViewportV1 = "wide" | "narrow";
export type PreviewSourceV1 = "workspace" | "build";

export type PreviewIpcRequestV1 =
  | { action: "status" }
  /** `source: "build"` opens the task's last successful build output (main
   * knows its folder; `entry` is then null). Paths of that page are served
   * from the output folder, and a path with no extension is the page itself
   * (a single-page app's own route). */
  | { action: "open"; projectId: string; chatId: string; entry: string | null; source: PreviewSourceV1; bounds: PreviewBoundsV1; viewport: PreviewViewportV1; pixelRatio: number; page: PreviewPageSizeV1 }
  /** `obscured`: something of the app's own covers the area (dialog, sheet,
   * drawer, popover, toast). The view hides and the answer carries a still
   * image to show in its place. */
  | { action: "layout"; previewId: string; bounds: PreviewBoundsV1; viewport: PreviewViewportV1; obscured: boolean; pixelRatio: number; page: PreviewPageSizeV1 }
  | { action: "reload"; previewId: string }
  /** Another tab: hidden now, closed after 30 seconds unless shown again. */
  | { action: "hide"; previewId: string }
  | { action: "close"; previewId: string };

export type PreviewStateV1 = "loading" | "shown" | "hidden" | "closed";
/** `noBuild`: a build preview was asked for and the task has no finished build. */
export type PreviewEmptyReasonV1 = "noEntry" | "noBuild" | PreviewEntryRefusalV1;

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
function pixelRatio(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0.25 || value > 16) fail("pixelRatio is invalid");
  return value as number;
}
function pageSize(value: unknown): PreviewPageSizeV1 {
  const object = requireObject(value, "preview page size");
  exactKeys(object, ["width", "height"]);
  for (const field of ["width", "height"] as const) {
    const number = object[field];
    if (typeof number !== "number" || !Number.isFinite(number) || number <= 0 || number > 100_000) fail(`page ${field} is invalid`);
  }
  return { width: object.width as number, height: object.height as number };
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
      exactKeys(object, ["action", "projectId", "chatId", "entry", "source", "bounds", "viewport", "pixelRatio", "page"]);
      if (object.source !== "workspace" && object.source !== "build") fail("preview source is invalid");
      if (object.source === "build" && object.entry !== null) fail("a build preview names no entry");
      return {
        action: "open", projectId: id(object.projectId, "projectId"), chatId: id(object.chatId, "chatId"),
        entry: entryPath(object.entry, "entry"), source: object.source as PreviewSourceV1, bounds: bounds(object.bounds), viewport: viewport(object.viewport), pixelRatio: pixelRatio(object.pixelRatio), page: pageSize(object.page),
      };
    case "layout":
      exactKeys(object, ["action", "previewId", "bounds", "viewport", "obscured", "pixelRatio", "page"]);
      if (typeof object.obscured !== "boolean") fail("obscured must be a boolean");
      return { action: "layout", previewId: previewId(object.previewId), bounds: bounds(object.bounds), viewport: viewport(object.viewport), obscured: object.obscured as boolean, pixelRatio: pixelRatio(object.pixelRatio), page: pageSize(object.page) };
    case "reload": case "hide": case "close":
      exactKeys(object, ["action", "previewId"]);
      return { action: object.action, previewId: previewId(object.previewId) };
    default:
      return fail("preview action is unknown");
  }
}

const STATES = new Set<PreviewStateV1>(["loading", "shown", "hidden", "closed"]);
const EMPTY_REASONS = new Set<string>(["noEntry", "noBuild", ...REFUSALS]);
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
  open(input: { projectId: string; chatId: string; entry: string | null; source: PreviewSourceV1; bounds: PreviewBoundsV1; viewport: PreviewViewportV1; pixelRatio: number; page: PreviewPageSizeV1 }): Promise<Extract<PreviewIpcResultV1, { action: "open" }>>;
  layout(input: { previewId: string; bounds: PreviewBoundsV1; viewport: PreviewViewportV1; obscured: boolean; pixelRatio: number; page: PreviewPageSizeV1 }): Promise<Extract<PreviewIpcResultV1, { action: "layout" }>>;
  reload(input: { previewId: string }): Promise<{ previewId: string; state: PreviewStateV1 }>;
  hide(input: { previewId: string }): Promise<{ previewId: string; state: PreviewStateV1 }>;
  close(input: { previewId: string }): Promise<{ previewId: string; state: PreviewStateV1 }>;
  onEvent(listener: (event: PreviewEventV1) => void): () => void;
}

// ---------------------------------------------------------------------------
// Build-then-preview (결과 미리보기 확장 1단계, 2026-11-27)
//
// `preview.buildPlan` is read-only: the sidecar looks at the task's own
// workspace (from the chat binding) and says what a build needs. Main runs
// the installation and the build itself, each only after the person agrees
// in main's native box; the page never names a folder or a command.

export type PreviewPackageManagerV1 = "npm" | "pnpm" | "yarn";
export const PREVIEW_BUILD_REFUSALS_V1 = [
  "noPackage", "noBuildScript", "invalidPackage", "unsupportedManager", "unsupportedWorkspace",
  "unsafeOutput", "dependenciesNotIgnored", "outputNotIgnored", "linkedInput", "unsupportedSettings",
] as const;
export type PreviewBuildRefusalV1 = (typeof PREVIEW_BUILD_REFUSALS_V1)[number];

export interface PreviewBuildPlanRequestV1 { projectId: string; chatId: string }
export interface PreviewBuildPlanResultV1 extends PreviewWorkspaceRefV1 {
  refusal: PreviewBuildRefusalV1 | null;
  /** Main only: never sent to the renderer. */
  workspacePath: string | null;
  packageManager: PreviewPackageManagerV1 | null;
  lockfile: boolean;
  buildScript: string | null;
  /** `prebuild`/`postbuild`: npm runs them with `build` (shown in the consent box). */
  buildHooks: { name: string; script: string }[] | null;
  /** Declared package names (first 40) and how many there are. */
  dependencies: string[] | null;
  dependencyCount: number | null;
  yarnBerry: boolean;
  outputDir: string | null;
  needsInstall: boolean;
  /** SHA-256 over the folder and every input file (PR #22 review B1): main
   * runs only while it is the one the person agreed to. */
  fingerprint: string | null;
  /** The folder's device:inode, shown with its path (re-review B1). */
  folderIdentity: string | null;
  /** Every input file there is, with its SHA-256: exactly what the
   * fingerprint stands for, and shown in the box (re-review B1). */
  inputs: { name: string; sha256: string }[] | null;
}

/** Main names the hold (PR #25 review B1): a reserve whose answer was lost
 * can still be released by that name. */
export interface PreviewBuildReserveRequestV1 { projectId: string; chatId: string; holdId: string }
export interface PreviewBuildReserveResultV1 { holdId: string | null; refusal: "busy" | null }
export interface PreviewBuildReleaseRequestV1 { holdId: string }
export interface PreviewBuildReleaseResultV1 { holdId: string; released: boolean }
const HOLD_ID = /^pbh_[0-9a-f]{32}$/;
export function validatePreviewBuildReserveRequestV1(value: unknown): PreviewBuildReserveRequestV1 {
  const object = requireObject(value, "preview build reserve request");
  exactKeys(object, ["projectId", "chatId", "holdId"]);
  if (typeof object.holdId !== "string" || !HOLD_ID.test(object.holdId)) fail("holdId is invalid");
  return { projectId: id(object.projectId, "projectId"), chatId: id(object.chatId, "chatId"), holdId: object.holdId as string };
}
/** Main's own holds after an engine start, and whether every process an
 * earlier app run's build left has ended (PR #25 review B2). */
export interface PreviewBuildHoldsRequestV1 { holds: { holdId: string; chatId: string }[]; leftoversSettled: boolean }
export interface PreviewBuildHoldsResultV1 { held: number; blocking: boolean }
export const PREVIEW_BUILD_HOLDS_MAX_V1 = 64;
export function validatePreviewBuildHoldsRequestV1(value: unknown): PreviewBuildHoldsRequestV1 {
  const object = requireObject(value, "preview build holds request");
  exactKeys(object, ["holds", "leftoversSettled"]);
  if (!Array.isArray(object.holds) || object.holds.length > PREVIEW_BUILD_HOLDS_MAX_V1 || typeof object.leftoversSettled !== "boolean") fail("build holds are invalid");
  const holds = (object.holds as unknown[]).map(entry => {
    const hold = requireObject(entry, "build hold");
    exactKeys(hold, ["holdId", "chatId"]);
    if (typeof hold.holdId !== "string" || !HOLD_ID.test(hold.holdId)) fail("holdId is invalid");
    return { holdId: hold.holdId as string, chatId: id(hold.chatId, "chatId") };
  });
  return { holds, leftoversSettled: object.leftoversSettled as boolean };
}
export function validatePreviewBuildHoldsResultV1(value: unknown): PreviewBuildHoldsResultV1 {
  const object = requireObject(value, "preview build holds result");
  exactKeys(object, ["held", "blocking"]);
  if (!Number.isSafeInteger(object.held) || (object.held as number) < 0 || (object.held as number) > PREVIEW_BUILD_HOLDS_MAX_V1 || typeof object.blocking !== "boolean") fail("build holds result is invalid");
  return { held: object.held as number, blocking: object.blocking as boolean };
}
export function validatePreviewBuildReserveResultV1(value: unknown): PreviewBuildReserveResultV1 {
  const object = requireObject(value, "preview build reserve result");
  exactKeys(object, ["holdId", "refusal"]);
  if (object.holdId === null) {
    if (object.refusal !== "busy") fail("an unheld reserve says why");
    return { holdId: null, refusal: "busy" };
  }
  if (typeof object.holdId !== "string" || !HOLD_ID.test(object.holdId) || object.refusal !== null) fail("build hold is invalid");
  return { holdId: object.holdId as string, refusal: null };
}
export function validatePreviewBuildReleaseRequestV1(value: unknown): PreviewBuildReleaseRequestV1 {
  const object = requireObject(value, "preview build release request");
  exactKeys(object, ["holdId"]);
  if (typeof object.holdId !== "string" || !HOLD_ID.test(object.holdId)) fail("holdId is invalid");
  return { holdId: object.holdId as string };
}
export function validatePreviewBuildReleaseResultV1(value: unknown, request: PreviewBuildReleaseRequestV1): PreviewBuildReleaseResultV1 {
  const object = requireObject(value, "preview build release result");
  exactKeys(object, ["holdId", "released"]);
  if (object.holdId !== request.holdId || typeof object.released !== "boolean") fail("build release is invalid");
  return { holdId: request.holdId, released: object.released as boolean };
}

/** The sidecar's output folder rule (`safe_output_dir`): plain segments inside the workspace. */
export function isPreviewOutputDirV1(value: string): boolean {
  if (value.length === 0 || value.length > 120 || value.split("/").length > 4 || !/^[A-Za-z0-9._/-]+$/.test(value)) return false;
  if (["src", "public", "node_modules"].includes(value.split("/")[0]!)) return false;
  return previewPathRefusalV1(`${value}/index.html`) === null;
}

export function validatePreviewBuildPlanRequestV1(value: unknown): PreviewBuildPlanRequestV1 {
  const object = requireObject(value, "preview build plan request");
  exactKeys(object, ["projectId", "chatId"]);
  return { projectId: id(object.projectId, "projectId"), chatId: id(object.chatId, "chatId") };
}

export function validatePreviewBuildPlanResultV1(value: unknown, request: PreviewBuildPlanRequestV1): PreviewBuildPlanResultV1 {
  const object = requireObject(value, "preview build plan");
  exactKeys(object, ["chatId", "projectId", "worktreeId", "workspaceIdentity", "refusal", "workspacePath", "packageManager", "lockfile", "buildScript", "buildHooks", "dependencies", "dependencyCount", "yarnBerry", "outputDir", "needsInstall", "fingerprint", "folderIdentity", "inputs"]);
  if (object.chatId !== request.chatId || object.projectId !== request.projectId) fail("preview build plan answered another chat");
  const ref = { chatId: request.chatId, projectId: request.projectId, worktreeId: id(object.worktreeId, "worktreeId"), workspaceIdentity: id(object.workspaceIdentity, "workspaceIdentity") };
  if (typeof object.lockfile !== "boolean" || typeof object.needsInstall !== "boolean" || typeof object.yarnBerry !== "boolean") fail("preview build flags are invalid");
  if (object.refusal !== null) {
    if (typeof object.refusal !== "string" || !(PREVIEW_BUILD_REFUSALS_V1 as readonly string[]).includes(object.refusal)) fail("preview build refusal is invalid");
    if (object.workspacePath !== null || object.packageManager !== null || object.buildScript !== null || object.outputDir !== null || object.lockfile || object.needsInstall
      || object.buildHooks !== null || object.dependencies !== null || object.dependencyCount !== null || object.yarnBerry || object.fingerprint !== null
      || object.folderIdentity !== null || object.inputs !== null) {
      fail("a refused build plan carries no plan");
    }
    return { ...ref, refusal: object.refusal as PreviewBuildRefusalV1, workspacePath: null, packageManager: null, lockfile: false, buildScript: null,
      buildHooks: null, dependencies: null, dependencyCount: null, yarnBerry: false, outputDir: null, needsInstall: false, fingerprint: null, folderIdentity: null, inputs: null };
  }
  const hooks = object.buildHooks;
  if (!Array.isArray(hooks) || hooks.length > 2 || hooks.some(hook => {
    const entry = requireObject(hook, "build hook");
    exactKeys(entry, ["name", "script"]);
    return (entry.name !== "prebuild" && entry.name !== "postbuild") || typeof entry.script !== "string" || entry.script.length > 1200;
  })) fail("buildHooks is invalid");
  const dependencies = object.dependencies;
  if (!Array.isArray(dependencies) || dependencies.length > 40 || dependencies.some(name => typeof name !== "string" || name.length === 0 || name.length > 214 || /[\u0000-\u001f\u007f]/u.test(name))) fail("dependencies are invalid");
  const count = object.dependencyCount;
  if (typeof count !== "number" || !Number.isSafeInteger(count) || count < dependencies.length) fail("dependencyCount is invalid");
  if (typeof object.fingerprint !== "string" || !/^[0-9a-f]{64}$/.test(object.fingerprint)) fail("fingerprint is invalid");
  if (typeof object.folderIdentity !== "string" || !/^\d+:\d+$/.test(object.folderIdentity)) fail("folderIdentity is invalid");
  const inputs = object.inputs;
  if (!Array.isArray(inputs) || inputs.length === 0 || inputs.length > 16 || inputs.some(entry => {
    const input = requireObject(entry, "build input");
    exactKeys(input, ["name", "sha256"]);
    return typeof input.name !== "string" || !/^[A-Za-z0-9._-]{1,40}$/.test(input.name) || typeof input.sha256 !== "string" || !/^[0-9a-f]{64}$/.test(input.sha256);
  }) || !inputs.some(entry => (entry as { name: string }).name === "package.json")) fail("inputs are invalid");
  const path = object.workspacePath;
  if (typeof path !== "string" || !path.startsWith("/") && !/^[A-Za-z]:\\/.test(path) || path.length > 4096 || /[\u0000-\u001f\u007f]/u.test(path)) fail("workspacePath is invalid");
  if (object.packageManager !== "npm" && object.packageManager !== "pnpm" && object.packageManager !== "yarn") fail("packageManager is invalid");
  const script = object.buildScript;
  if (typeof script !== "string" || script.length === 0 || script.length > 1200) fail("buildScript is invalid");
  if (typeof object.outputDir !== "string" || !isPreviewOutputDirV1(object.outputDir)) fail("outputDir is invalid");
  return {
    ...ref, refusal: null, workspacePath: path as string, packageManager: object.packageManager as PreviewPackageManagerV1,
    lockfile: object.lockfile as boolean, buildScript: script as string, buildHooks: hooks as { name: string; script: string }[],
    dependencies: dependencies as string[], dependencyCount: count as number, yarnBerry: object.yarnBerry as boolean,
    outputDir: object.outputDir as string, needsInstall: object.needsInstall as boolean, fingerprint: object.fingerprint as string,
    folderIdentity: object.folderIdentity as string, inputs: inputs as { name: string; sha256: string }[],
  };
}

// Renderer ↔ main (`m5:previewBuild:v1`). The page names the project and
// the chat; main asks the sidecar for the plan every time and shows the
// exact command and folder in its own box before anything runs.

export const PREVIEW_BUILD_CHANNEL_V1 = "m5:previewBuild:v1";
export type PreviewBuildStepV1 = "install" | "build";
export type PreviewBuildStateV1 = "idle" | "running" | "succeeded" | "failed" | "cancelled" | "timedOut";
export const PREVIEW_BUILD_LOG_MAX_LINES_V1 = 1000;
export const PREVIEW_BUILD_LINE_MAX_CHARS_V1 = 2000;

export type PreviewBuildIpcRequestV1 =
  | { action: "plan"; projectId: string; chatId: string }
  /** Asks main to run one step. Main shows its own box first; "취소" there is `declined`. */
  | { action: "start"; projectId: string; chatId: string; step: PreviewBuildStepV1 }
  | { action: "cancel"; projectId: string; chatId: string }
  /** The job's state and its log lines from `cursor` on. */
  | { action: "status"; projectId: string; chatId: string; cursor: number };

/** What the page may know of a plan: the commands as shown, never the folder. */
export interface PreviewBuildPlanViewV1 {
  /** `unavailable`: the engine could not answer; `toolMissing`: no `node` or
   * package manager was found on this computer. */
  refusal: PreviewBuildRefusalV1 | "unavailable" | "toolMissing" | null;
  installCommand: string | null;
  buildCommand: string | null;
  needsInstall: boolean;
  outputDir: string | null;
}

export interface PreviewBuildJobViewV1 {
  step: PreviewBuildStepV1 | null;
  state: PreviewBuildStateV1;
  /** The next cursor and the lines since the asked one (already masked). */
  cursor: number;
  lines: string[];
  exitCode: number | null;
  /** A build that succeeded: its output can be opened (`open` with `source: "build"`). */
  outputReady: boolean;
  startedAt: number | null;
  endedAt: number | null;
  /** Processes of the job (or that it started) not seen to end after it
   * was stopped (PR #22 review B3). While any lives, this task cannot build. */
  leftovers: number;
}

/** `changed`: what the person agreed to changed before it ran (nothing ran;
 * main asked again and that was not agreed either). `cancelled`: stopped while
 * the box was open. `leftovers`: an earlier job's processes still live. */
export type PreviewBuildStartOutcomeV1 = "started" | "declined" | "busy" | "refused" | "changed" | "cancelled" | "leftovers";
export const PREVIEW_BUILD_START_OUTCOMES_V1: readonly PreviewBuildStartOutcomeV1[] = ["started", "declined", "busy", "refused", "changed", "cancelled", "leftovers"];

export type PreviewBuildIpcResultV1 =
  | { action: "plan"; plan: PreviewBuildPlanViewV1; job: PreviewBuildJobViewV1 }
  | { action: "start"; outcome: PreviewBuildStartOutcomeV1; job: PreviewBuildJobViewV1 }
  | { action: "cancel" | "status"; job: PreviewBuildJobViewV1 };

function step(value: unknown): PreviewBuildStepV1 {
  if (value !== "install" && value !== "build") fail("build step is invalid");
  return value as PreviewBuildStepV1;
}

export function validatePreviewBuildIpcRequestV1(value: unknown): PreviewBuildIpcRequestV1 {
  const object = requireObject(value, "preview build request");
  switch (object.action) {
    case "plan": case "cancel":
      exactKeys(object, ["action", "projectId", "chatId"]);
      return { action: object.action, projectId: id(object.projectId, "projectId"), chatId: id(object.chatId, "chatId") };
    case "start":
      exactKeys(object, ["action", "projectId", "chatId", "step"]);
      return { action: "start", projectId: id(object.projectId, "projectId"), chatId: id(object.chatId, "chatId"), step: step(object.step) };
    case "status": {
      exactKeys(object, ["action", "projectId", "chatId", "cursor"]);
      const cursor = object.cursor;
      if (typeof cursor !== "number" || !Number.isSafeInteger(cursor) || cursor < 0) fail("cursor is invalid");
      return { action: "status", projectId: id(object.projectId, "projectId"), chatId: id(object.chatId, "chatId"), cursor: cursor as number };
    }
    default:
      return fail("preview build action is unknown");
  }
}

const BUILD_STATES = new Set<PreviewBuildStateV1>(["idle", "running", "succeeded", "failed", "cancelled", "timedOut"]);
function nullableTime(value: unknown, field: string): number | null {
  if (value === null) return null;
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) fail(`${field} is invalid`);
  return value as number;
}
function command(value: unknown, field: string): string | null {
  if (value === null) return null;
  if (typeof value !== "string" || value.length === 0 || value.length > 200) fail(`${field} is invalid`);
  return value as string;
}

export function validatePreviewBuildJobViewV1(value: unknown): PreviewBuildJobViewV1 {
  const object = requireObject(value, "preview build job");
  exactKeys(object, ["step", "state", "cursor", "lines", "exitCode", "outputReady", "startedAt", "endedAt", "leftovers"]);
  if (typeof object.leftovers !== "number" || !Number.isSafeInteger(object.leftovers) || object.leftovers < 0) fail("leftovers is invalid");
  if (object.step !== null) step(object.step);
  if (typeof object.state !== "string" || !BUILD_STATES.has(object.state as PreviewBuildStateV1)) fail("build state is invalid");
  if (typeof object.cursor !== "number" || !Number.isSafeInteger(object.cursor) || object.cursor < 0) fail("cursor is invalid");
  if (!Array.isArray(object.lines) || object.lines.length > PREVIEW_BUILD_LOG_MAX_LINES_V1
    || object.lines.some(line => typeof line !== "string" || line.length > PREVIEW_BUILD_LINE_MAX_CHARS_V1)) fail("build lines are invalid");
  if (object.exitCode !== null && (typeof object.exitCode !== "number" || !Number.isSafeInteger(object.exitCode))) fail("exitCode is invalid");
  if (typeof object.outputReady !== "boolean") fail("outputReady is invalid");
  return {
    step: object.step as PreviewBuildStepV1 | null, state: object.state as PreviewBuildStateV1, cursor: object.cursor as number,
    lines: object.lines as string[], exitCode: object.exitCode as number | null, outputReady: object.outputReady as boolean,
    startedAt: nullableTime(object.startedAt, "startedAt"), endedAt: nullableTime(object.endedAt, "endedAt"), leftovers: object.leftovers as number,
  };
}

export function validatePreviewBuildIpcResultV1(value: unknown, request: PreviewBuildIpcRequestV1): PreviewBuildIpcResultV1 {
  const object = requireObject(value, "preview build result");
  if (object.action !== request.action) fail("preview build result answers another action");
  switch (request.action) {
    case "plan": {
      exactKeys(object, ["action", "plan", "job"]);
      const plan = requireObject(object.plan, "preview build plan view");
      exactKeys(plan, ["refusal", "installCommand", "buildCommand", "needsInstall", "outputDir"]);
      if (plan.refusal !== null && plan.refusal !== "unavailable" && plan.refusal !== "toolMissing" && !(PREVIEW_BUILD_REFUSALS_V1 as readonly string[]).includes(plan.refusal as string)) fail("plan refusal is invalid");
      if (typeof plan.needsInstall !== "boolean") fail("needsInstall is invalid");
      if (plan.outputDir !== null && (typeof plan.outputDir !== "string" || !isPreviewOutputDirV1(plan.outputDir))) fail("outputDir is invalid");
      return {
        action: "plan",
        plan: { refusal: plan.refusal as PreviewBuildPlanViewV1["refusal"], installCommand: command(plan.installCommand, "installCommand"), buildCommand: command(plan.buildCommand, "buildCommand"), needsInstall: plan.needsInstall as boolean, outputDir: plan.outputDir as string | null },
        job: validatePreviewBuildJobViewV1(object.job),
      };
    }
    case "start":
      exactKeys(object, ["action", "outcome", "job"]);
      if (!(PREVIEW_BUILD_START_OUTCOMES_V1 as readonly unknown[]).includes(object.outcome)) fail("start outcome is invalid");
      return { action: "start", outcome: object.outcome as PreviewBuildStartOutcomeV1, job: validatePreviewBuildJobViewV1(object.job) };
    default:
      exactKeys(object, ["action", "job"]);
      return { action: request.action, job: validatePreviewBuildJobViewV1(object.job) };
  }
}

/** `window.m5PreviewBuild` (preload). */
export interface M5PreviewBuildBridgeV1 {
  plan(input: { projectId: string; chatId: string }): Promise<Extract<PreviewBuildIpcResultV1, { action: "plan" }>>;
  start(input: { projectId: string; chatId: string; step: PreviewBuildStepV1 }): Promise<Extract<PreviewBuildIpcResultV1, { action: "start" }>>;
  cancel(input: { projectId: string; chatId: string }): Promise<{ job: PreviewBuildJobViewV1 }>;
  status(input: { projectId: string; chatId: string; cursor: number }): Promise<{ job: PreviewBuildJobViewV1 }>;
}
