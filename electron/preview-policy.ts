/**
 * Result preview policy (ADE 1차-나 F12): the pure decisions the
 * PreviewManager applies. No Electron import, so every rule here is unit
 * tested directly (`tests/m5-preview-policy.test.mts`).
 *
 * Design: `docs/proposals/m5-ade-layout-plan.md` 2.2–2.4. The page in a
 * preview is untrusted code: its only reachable origin is its own
 * `elsecrew-preview://<token>/`, served read-only from the task workspace.
 */
import { randomBytes } from "node:crypto";
import {
  PREVIEW_MIME_V1,
  PREVIEW_MIN_SIZE_V1,
  PREVIEW_NARROW_WIDTH_V1,
  previewExtensionV1,
  previewPathRefusalV1,
  type PreviewBoundsV1,
  type PreviewViewportV1,
} from "../src/m5/contracts/preview-v1.ts";

export const PREVIEW_SCHEME = "elsecrew-preview";
/** Registered once with the app scheme. The handler itself exists only on
 * each preview's own session, never on the app's default session. */
export const PREVIEW_SCHEME_PRIVILEGES = Object.freeze({
  standard: true,
  secure: true,
  supportFetchAPI: true,
  corsEnabled: false,
  allowServiceWorkers: false,
  bypassCSP: false,
  stream: false,
  codeCache: false,
});

const TOKEN = /^[0-9a-f]{32}$/;
/** 128 random bits, a preview's host and partition name. Never sent to the renderer. */
export function newPreviewToken(): string { return randomBytes(16).toString("hex"); }
/** The renderer's handle for one opened preview (not the token). */
export function newPreviewId(): string { return `pv_${randomBytes(16).toString("hex")}`; }
export function isPreviewToken(value: string): boolean { return TOKEN.test(value); }

/** No `persist:` prefix: the partition lives in memory only and leaves
 * nothing under the app data folder (T12). */
export function previewPartition(token: string): string {
  if (!isPreviewToken(token)) throw new Error("invalid preview token");
  return `preview-${token}`;
}

export function previewOrigin(token: string): string {
  if (!isPreviewToken(token)) throw new Error("invalid preview token");
  return `${PREVIEW_SCHEME}://${token}`;
}

export function previewEntryUrl(token: string, entry: string): string {
  if (previewPathRefusalV1(entry) !== null) throw new Error("invalid preview entry");
  return `${previewOrigin(token)}/${entry.split("/").map(encodeURIComponent).join("/")}`;
}

export type PreviewUrlDecision = { ok: true; path: string } | { ok: false; reason: string };

/** A request the preview's own protocol handler serves: this exact token,
 * no port or credentials, no second level of encoding, then the same path
 * rules as the sidecar. `/` is the entry page. */
export function parsePreviewRequestUrl(raw: string, token: string, entry: string): PreviewUrlDecision {
  if (!isPreviewToken(token)) return { ok: false, reason: "token" };
  let url: URL;
  try { url = new URL(raw); } catch { return { ok: false, reason: "invalid" }; }
  if (url.protocol !== `${PREVIEW_SCHEME}:`) return { ok: false, reason: "scheme" };
  if (url.hostname !== token || url.port !== "" || url.username !== "" || url.password !== "") return { ok: false, reason: "origin" };
  const rest = raw.slice(raw.indexOf("://") + 3);
  const slash = rest.search(/[/?#]/);
  const rawPath = slash < 0 || rest[slash] !== "/" ? "/" : rest.slice(slash).replace(/[?#][\s\S]*$/, "");
  if (/%(?:2e|2f|5c|00)/i.test(rawPath)) return { ok: false, reason: "encoded" };
  let pathname: string;
  try { pathname = decodeURIComponent(url.pathname); } catch { return { ok: false, reason: "decode" }; }
  // Chromium hands a standard scheme's root as `/`; Node's URL keeps it empty.
  if (pathname === "") pathname = "/";
  if (!pathname.startsWith("/")) return { ok: false, reason: "invalid" };
  const path = pathname === "/" ? entry : pathname.slice(1);
  const refusal = previewPathRefusalV1(path);
  return refusal === null ? { ok: true, path } : { ok: false, reason: refusal };
}

function sameOrigin(raw: string, token: string): boolean {
  try {
    const url = new URL(raw);
    return url.protocol === `${PREVIEW_SCHEME}:` && url.hostname === token && isPreviewToken(token)
      && url.port === "" && url.username === "" && url.password === "";
  } catch { return false; }
}

/** `webRequest` (every request of the preview session, T1/T14/T15): only
 * this preview's own origin. `data:` and same-origin `blob:` are not network. */
export function isPreviewRequestAllowed(raw: string, token: string): boolean {
  if (raw.startsWith("data:")) return true;
  if (raw.startsWith("blob:")) return sameOrigin(raw.slice(5), token);
  return sameOrigin(raw, token);
}

/** Navigation (T9): the page may move within its own origin only. A
 * subframe may also be an empty document. */
export function isPreviewNavigationAllowed(raw: string, token: string, mainFrame: boolean): boolean {
  if (!mainFrame && (raw === "about:blank" || raw === "about:srcdoc")) return true;
  return sameOrigin(raw, token);
}

export const PREVIEW_ALLOWED_METHODS: readonly string[] = Object.freeze(["GET", "HEAD"]);

/** Network-blocking second line (with webRequest and the dead proxy), not an
 * XSS defense: inline scripts are allowed, `eval` is not. */
export const PREVIEW_CSP = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-inline'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "font-src 'self' data:",
  "media-src 'self' data: blob:",
  "connect-src 'self'",
  "worker-src 'self' blob:",
  "frame-src 'self'",
  "child-src 'self' blob:",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'none'",
  "manifest-src 'none'",
  "upgrade-insecure-requests",
].join("; ");

export const PREVIEW_PERMISSIONS_POLICY = [
  "camera=()", "microphone=()", "geolocation=()", "usb=()", "serial=()", "hid=()", "bluetooth=()",
  "payment=()", "clipboard-read=()", "display-capture=()",
].join(", ");

/** The fixed headers of every preview answer. Content type comes from the
 * extension table only (`nosniff`). */
export function previewResponseHeaders(path: string | null): Record<string, string> {
  const extension = path === null ? null : previewExtensionV1(path);
  return {
    "Content-Type": extension ? PREVIEW_MIME_V1[extension]! : "text/plain; charset=utf-8",
    "X-Content-Type-Options": "nosniff",
    "Cache-Control": "no-store",
    "Cross-Origin-Opener-Policy": "same-origin",
    "Cross-Origin-Resource-Policy": "same-origin",
    "Referrer-Policy": "no-referrer",
    "Permissions-Policy": PREVIEW_PERMISSIONS_POLICY,
    "Content-Security-Policy": PREVIEW_CSP,
  };
}

/** Every proxied connection goes to the discard port, loopback included
 * (Chromium bypasses the proxy for loopback unless told otherwise). */
export const PREVIEW_PROXY = Object.freeze({
  mode: "fixed_servers" as const,
  proxyRules: "127.0.0.1:9",
  proxyBypassRules: "<-loopback>",
});

/** The app's user agent without the Electron and app product tokens. */
export function genericUserAgent(userAgent: string): string {
  return userAgent.replace(/\s(?:Electron|elsecrew|Elsecrew)\/\S+/g, "").replace(/\s{2,}/g, " ").trim();
}

/** The renderer's placeholder rectangle, checked in main: finite, inside the
 * window's content, at least 120×120. Rounded to whole pixels. */
export function validatePreviewBounds(bounds: PreviewBoundsV1, content: { width: number; height: number }): PreviewBoundsV1 {
  const rounded = {
    x: Math.round(bounds.x), y: Math.round(bounds.y),
    width: Math.round(bounds.width), height: Math.round(bounds.height),
  };
  if (Object.values(rounded).some(value => !Number.isSafeInteger(value))) throw new Error("preview bounds are not finite");
  if (rounded.x < 0 || rounded.y < 0 || rounded.width < PREVIEW_MIN_SIZE_V1 || rounded.height < PREVIEW_MIN_SIZE_V1) {
    throw new Error("preview bounds are outside the window or too small");
  }
  // One pixel of rounding at the far edges is the page's subpixel layout.
  if (rounded.x + rounded.width > content.width + 1 || rounded.y + rounded.height > content.height + 1) {
    throw new Error("preview bounds are outside the window");
  }
  return {
    ...rounded,
    width: Math.min(rounded.width, content.width - rounded.x),
    height: Math.min(rounded.height, content.height - rounded.y),
  };
}

/** Where the view goes inside the placeholder: all of it, or a phone-wide
 * (390px) column in its middle. */
export function previewViewRect(bounds: PreviewBoundsV1, viewport: PreviewViewportV1): PreviewBoundsV1 {
  if (viewport === "wide" || bounds.width <= PREVIEW_NARROW_WIDTH_V1) return { ...bounds };
  const width = PREVIEW_NARROW_WIDTH_V1;
  return { x: bounds.x + Math.floor((bounds.width - width) / 2), y: bounds.y, width, height: bounds.height };
}
