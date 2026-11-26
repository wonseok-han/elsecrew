/**
 * Result preview manager (ADE 1차-나 F12, behind `ELSECREW_PREVIEW=1`).
 *
 * Shows a task workspace's static web output in a `WebContentsView` over the
 * main window. Design: `docs/proposals/m5-ade-layout-plan.md` 2.2–2.4; the
 * pure rules are in `preview-policy.ts`.
 *
 * - One view at most. It exists only while the screen asks for it: an idle
 *   app with no preview has no extra process.
 * - Each opened preview has its own 128-bit token, which is its host and its
 *   memory-only partition (`preview-<token>`, no `persist:`). The token stays
 *   in main; the renderer only holds a separate `previewId`.
 * - The session is configured before the first load: every permission
 *   refused, every request but its own origin cancelled (`webRequest`), all
 *   connections sent to a dead proxy (loopback too), WebRTC limited to proxied
 *   UDP (= none), downloads cancelled. Its protocol handler is the only
 *   source of content and only answers GET/HEAD with fixed headers.
 * - The workspace address comes from the sidecar (`preview.resolve`), from
 *   the chat's durable binding; each file is read with `preview.readAsset`.
 * - Closing (requested, replaced, idle, crashed, limit, quitting) removes the
 *   view, closes its page, drops the token and clears the partition's data.
 *   A partition object lives until the app ends, so a token is never reused.
 */
import type { BrowserWindow, Session, WebContents, WebContentsView, WebPreferences } from "electron";
import {
  PREVIEW_ASSET_CHUNK_BYTES_V1,
  PREVIEW_TOKEN_MAX_BYTES_V1,
  PREVIEW_TOKEN_MAX_REQUESTS_V1,
  type PreviewBoundsV1,
  type PreviewPageSizeV1,
  type PreviewCloseReasonV1,
  type PreviewEventV1,
  type PreviewIpcRequestV1,
  type PreviewIpcResultV1,
  type PreviewReadAssetRequestV1,
  type PreviewReadAssetResultV1,
  type PreviewResolveRequestV1,
  type PreviewResolveResultV1,
  type PreviewStateV1,
  type PreviewViewportV1,
  type PreviewWorkspaceRefV1,
} from "../src/m5/contracts/preview-v1.ts";
import {
  PREVIEW_ALLOWED_METHODS,
  PREVIEW_PROXY,
  PREVIEW_SCHEME,
  genericUserAgent,
  isPreviewNavigationAllowed,
  isPreviewRequestAllowed,
  newPreviewId,
  newPreviewToken,
  parsePreviewRequestUrl,
  previewEntryUrl,
  previewPartition,
  previewResponseHeaders,
  previewViewRect,
  pageSizeMatches,
  pixelRatioMatches,
  windowBounds,
} from "./preview-policy.ts";

export const PREVIEW_IDLE_CLOSE_MS = 30_000;
export const PREVIEW_SNAPSHOT_MAX_CHARS = 8 * 1024 * 1024;
/** Files of one preview read from the sidecar at once, and waiting behind
 * them (PR #6 review I2): the shared sidecar queue never holds more than
 * this many preview reads, so stopping, saving and quitting stay prompt. */
export const PREVIEW_MAX_CONCURRENT_READS = 4;
export const PREVIEW_MAX_QUEUED_READS = 64;
/** One file's read, all chunks; the page gets 504 after it. */
export const PREVIEW_READ_DEADLINE_MS = 15_000;
/** The still image shown while the app covers the preview is taken ahead
 * of time (review I3): after loading, after each shown layout and after
 * input, never while a cover is waiting. */
export const PREVIEW_SNAPSHOT_DELAY_MS = 400;
export const PREVIEW_SNAPSHOT_TIMEOUT_MS = 1_000;

export interface PreviewSidecarV1 {
  previewResolve(input: PreviewResolveRequestV1): Promise<PreviewResolveResultV1>;
  previewReadAsset(input: PreviewReadAssetRequestV1): Promise<PreviewReadAssetResultV1>;
}

export interface PreviewManagerOptionsV1 {
  enabled: boolean;
  /** Development builds only, and only on request. */
  devTools: boolean;
  fromPartition(partition: string): Session;
  createView(webPreferences: WebPreferences): WebContentsView;
  window(): BrowserWindow | undefined;
  sidecar(): PreviewSidecarV1 | undefined;
  send(event: PreviewEventV1): void;
  /** Fixed labels only: never a path, URL or page content. */
  log(label: string): void;
  idleCloseMs?: number;
  /** The app page's zoom factor and its display's scale (PR #9 review B1). */
  zoomFactor(): number;
  displayScale(): number;
  /** Tests only shorten these. */
  readDeadlineMs?: number;
  snapshotDelayMs?: number;
}

interface PreviewInstance {
  readonly previewId: string;
  readonly token: string;
  readonly entry: string;
  readonly ref: PreviewWorkspaceRefV1;
  /** The sidecar's record of the verified workspace root (review B1). */
  readonly handle: string;
  readonly session: Session;
  view: WebContentsView | undefined;
  state: PreviewStateV1;
  /** Window coordinates; null until a measurement matches the current zoom. */
  bounds: PreviewBoundsV1 | null;
  viewport: PreviewViewportV1;
  obscured: boolean;
  /** No view is shown before the page has sent a layout after the open
   * (PR #9 review I1): the page checks its task and stage first. */
  confirmed: boolean;
  /** Whether the view is on screen now (the last `#apply`). */
  visible: boolean;
  /** Where the view was last put (window coordinates). */
  rect: PreviewBoundsV1 | null;
  /** The screen asked to hide it (another tab); it closes after a while. */
  hidden: boolean;
  requests: number;
  bytes: number;
  /** Bytes of chunks asked for and not answered yet. */
  reserved: number;
  /** Files being read now, and requests waiting for a free read. */
  reading: number;
  waiting: Array<() => void>;
  /** Bumped by every layout or hide: a capture finishing after a newer
   * request is thrown away. */
  layoutGeneration: number;
  lastSnapshot: string | null;
  snapshotTimer: ReturnType<typeof setTimeout> | undefined;
  blockedNoticeSent: boolean;
  /** Labels already logged for this preview: each once, not per request. */
  logged: Set<string>;
  idleTimer: ReturnType<typeof setTimeout> | undefined;
}

/** Counters a probe or test reads; never content or addresses. */
export interface PreviewDiagnosticsV1 {
  opened: number;
  closed: number;
  blockedRequests: number;
  blockedNavigations: number;
  blockedDownloads: number;
  deniedPermissions: number;
  refusedMethods: number;
  notFound: number;
  unexpectedResponses: number;
}

/** One refusal of the preview's permission handlers (PR #9 review I3). */
export interface PreviewDenialV1 { previewId: string; kind: "request" | "check" | "device" | "display" | "bluetooth" | "select"; permission: string; at: number }
const DENIALS_MAX = 500;
const VISIBILITY_MAX = 500;

export class PreviewManagerV1 {
  readonly #options: PreviewManagerOptionsV1;
  #current: PreviewInstance | undefined;
  /** Bumped by every open and close: an open that awaited the sidecar
   * across a newer request (or quitting) is abandoned. */
  #generation = 0;
  #quitting = false;
  /** Quit preparation in progress (an update, say): may be resumed. */
  #suspended = false;
  #windowHidden = false;
  /** What was refused, by whom and which permission: for the isolation probe (never IPC). */
  readonly denials: PreviewDenialV1[] = [];
  /** Measurements main did not use (stale zoom or size, off screen): for the probes (never IPC). */
  readonly refusedMeasurements: { at: number; css: PreviewBoundsV1; pixelRatio: number; page: PreviewPageSizeV1; zoom: number; scale: number; content: { width: number; height: number } }[] = [];
  /** When each view was shown or hidden: for the UI smoke's ordering checks (never IPC). */
  readonly visibility: { previewId: string; visible: boolean; at: number }[] = [];
  readonly diagnostics: PreviewDiagnosticsV1 = {
    opened: 0, closed: 0, blockedRequests: 0, blockedNavigations: 0, blockedDownloads: 0,
    deniedPermissions: 0, refusedMethods: 0, notFound: 0, unexpectedResponses: 0,
  };

  constructor(options: PreviewManagerOptionsV1) {
    this.#options = options;
  }

  get enabled(): boolean { return this.#options.enabled; }
  /** The live view's page, for the isolation probe only (never IPC). */
  get currentWebContents(): WebContents | undefined { return this.#current?.view?.webContents; }
  /** Where the live view is and whether it is shown, for the UI smoke only (never IPC). */
  get currentLayout(): { state: PreviewStateV1; bounds: { x: number; y: number; width: number; height: number } } | undefined {
    const view = this.#current?.view;
    return view && this.#current ? { state: this.#current.state, bounds: view.getBounds() } : undefined;
  }

  async request(request: PreviewIpcRequestV1): Promise<PreviewIpcResultV1> {
    if (request.action === "status") return { action: "status", enabled: this.#options.enabled && !this.#quitting && !this.#suspended };
    if (!this.#options.enabled) throw new Error("preview is not enabled");
    if (this.#quitting || this.#suspended) throw new Error("preview is closed while the app quits");
    switch (request.action) {
      case "open": return this.#open(request);
      case "layout": return this.#layout(request);
      case "reload": {
        const instance = this.#find(request.previewId);
        if (instance?.view) instance.view.webContents.reloadIgnoringCache();
        return { action: "reload", previewId: request.previewId, state: instance?.state ?? "closed" };
      }
      case "hide": {
        const instance = this.#find(request.previewId);
        if (instance) {
          instance.hidden = true;
          instance.layoutGeneration++;
          this.#apply(instance);
          clearTimeout(instance.idleTimer);
          instance.idleTimer = setTimeout(() => this.#close(instance, "idle"), this.#options.idleCloseMs ?? PREVIEW_IDLE_CLOSE_MS);
        }
        return { action: "hide", previewId: request.previewId, state: instance?.state ?? "closed" };
      }
      case "close": {
        const instance = this.#find(request.previewId);
        if (instance) this.#close(instance, "requested");
        return { action: "close", previewId: request.previewId, state: "closed" };
      }
    }
  }

  /** Quitting started: close at once and refuse everything after. */
  shutdown(): void {
    this.#quitting = true;
    this.#generation++;
    if (this.#current) this.#close(this.#current, "quitting");
  }

  /** Quit preparation that may still be called off (an update): close now,
   * refuse opens and reads until `resume` (review I4). */
  suspend(): void {
    this.#suspended = true;
    this.#generation++;
    if (this.#current) this.#close(this.#current, "quitting");
  }

  resume(): void {
    if (!this.#quitting) this.#suspended = false;
  }

  windowClosed(): void {
    this.#generation++;
    if (this.#current) this.#close(this.#current, "windowClosed");
  }

  /** The app page reloaded or its process ended: the page that knew the
   * preview is gone, so is the preview and any open still on its way. */
  pageReplaced(): void {
    this.#generation++;
    if (this.#current) this.#close(this.#current, "reloaded");
  }

  /**
   * The window is about to change size, or the page its zoom (PR #9
   * re-review B1). Called by main BEFORE the change: the view is hidden at
   * once and forgets where it was, so it shows again only at a place the
   * page measures after the change (`#measure` refuses older measurements).
   */
  geometryChanged(): void {
    const instance = this.#current;
    if (!instance) return;
    instance.bounds = null;
    instance.layoutGeneration++;
    this.#apply(instance);
  }

  /** The main window was minimized or hidden (or shown again). */
  setWindowHidden(hidden: boolean): void {
    this.#windowHidden = hidden;
    if (this.#current) this.#apply(this.#current);
  }

  #find(previewId: string): PreviewInstance | undefined {
    return this.#current?.previewId === previewId ? this.#current : undefined;
  }

  #contentSize(window: BrowserWindow): { width: number; height: number } {
    const [width, height] = window.getContentSize();
    return { width: width ?? 0, height: height ?? 0 };
  }

  /** The page's measurement in window coordinates, or null when it is not
   * usable now (taken at another zoom or window size, too small, outside the
   * window). */
  #measure(window: BrowserWindow, css: PreviewBoundsV1, pixelRatio: number, page: PreviewPageSizeV1): PreviewBoundsV1 | null {
    const zoom = this.#options.zoomFactor();
    const content = this.#contentSize(window);
    const scale = this.#options.displayScale();
    const placed = pixelRatioMatches(pixelRatio, zoom, scale) && pageSizeMatches(page, zoom, content) ? windowBounds(css, zoom, content) : null;
    if (!placed) {
      this.refusedMeasurements.push({ at: Date.now(), css, pixelRatio, page, zoom, scale, content });
      if (this.refusedMeasurements.length > 50) this.refusedMeasurements.shift();
    }
    return placed;
  }

  async #open(request: Extract<PreviewIpcRequestV1, { action: "open" }>): Promise<PreviewIpcResultV1> {
    const window = this.#options.window();
    if (!window || window.isDestroyed()) throw new Error("preview needs the main window");
    const bounds = this.#measure(window, request.bounds, request.pixelRatio, request.page);
    if (!bounds) throw new Error("preview stage is not on screen at the current zoom");
    const generation = ++this.#generation;
    if (this.#current) this.#close(this.#current, "replaced");
    const sidecar = this.#options.sidecar();
    if (!sidecar) throw new Error("sidecar was not started");
    const resolved = await sidecar.previewResolve({ projectId: request.projectId, chatId: request.chatId, entry: request.entry });
    this.#assertCurrent(generation);
    if (resolved.entry === null) return { action: "open", outcome: "empty", reason: resolved.refusal ?? "noEntry" };

    const token = newPreviewToken();
    const session = this.#options.fromPartition(previewPartition(token));
    const instance: PreviewInstance = {
      previewId: newPreviewId(), token, entry: resolved.entry,
      ref: { chatId: resolved.chatId, projectId: resolved.projectId, worktreeId: resolved.worktreeId, workspaceIdentity: resolved.workspaceIdentity },
      handle: resolved.previewHandle,
      session, view: undefined, state: "loading", bounds, viewport: request.viewport,
      obscured: false, confirmed: false, visible: false, rect: null, hidden: false, requests: 0, bytes: 0, reserved: 0, reading: 0, waiting: [],
      layoutGeneration: 0, lastSnapshot: null, snapshotTimer: undefined,
      blockedNoticeSent: false, logged: new Set(), idleTimer: undefined,
    };
    try {
      await this.#configureSession(instance);
      this.#assertCurrent(generation);
    } catch (error) {
      this.#dispose(instance);
      throw error;
    }
    const target = this.#options.window();
    if (!target || target.isDestroyed()) { this.#dispose(instance); throw new Error("preview needs the main window"); }
    // The window may have changed size or zoom while the sidecar and session
    // answered; the view is made hidden either way and shows only after the
    // page's next layout.
    instance.bounds = this.#measure(target, request.bounds, request.pixelRatio, request.page);
    const view = this.#options.createView(previewWebPreferences(session, this.#options.devTools));
    instance.view = view;
    this.#configureContents(instance, view.webContents);
    view.setBackgroundColor("#ffffff");
    this.#current = instance;
    this.diagnostics.opened++;
    // Attached hidden: shown only by the page's first layout (review I1).
    view.setVisible(false);
    target.contentView.addChildView(view);
    this.#apply(instance);
    view.webContents.loadURL(previewEntryUrl(token, resolved.entry)).catch(() => {
      // `did-fail-load` reports it; a closed preview has nothing to report.
    });
    return { action: "open", outcome: "opened", previewId: instance.previewId, entry: resolved.entry, state: "loading" };
  }

  #assertCurrent(generation: number): void {
    if (this.#quitting || this.#suspended || generation !== this.#generation) throw new Error("preview open was superseded");
  }

  /** Applied at once, never after an await (review I3): a cover hides the
   * view before this returns, with the still image taken earlier. */
  async #layout(request: Extract<PreviewIpcRequestV1, { action: "layout" }>): Promise<PreviewIpcResultV1> {
    const instance = this.#find(request.previewId);
    const window = this.#options.window();
    if (!instance || !window || window.isDestroyed()) return { action: "layout", previewId: request.previewId, state: "closed", snapshot: null };
    instance.layoutGeneration++;
    const snapshot = request.obscured && !instance.obscured ? instance.lastSnapshot : null;
    // Hiding always wins (PR #9 review B1): it never depends on the bounds.
    instance.obscured = request.obscured;
    instance.confirmed = true;
    if (request.obscured) this.#apply(instance);
    // A measurement that does not fit (stale zoom, outside the window) hides
    // the view until a good one arrives; it is never an error.
    instance.bounds = this.#measure(window, request.bounds, request.pixelRatio, request.page);
    instance.viewport = request.viewport;
    // Shown again: no longer idle.
    instance.hidden = false;
    clearTimeout(instance.idleTimer);
    instance.idleTimer = undefined;
    this.#apply(instance);
    this.#scheduleSnapshot(instance);
    return { action: "layout", previewId: instance.previewId, state: instance.state, snapshot };
  }

  /** Takes the still image a little after the view settles while shown. A
   * capture that ends after a newer layout, a hide, a cover or a close, or
   * that takes too long, is thrown away. */
  #scheduleSnapshot(instance: PreviewInstance): void {
    clearTimeout(instance.snapshotTimer);
    instance.snapshotTimer = undefined;
    if (instance.state !== "shown") return;
    const generation = instance.layoutGeneration;
    instance.snapshotTimer = setTimeout(() => {
      instance.snapshotTimer = undefined;
      const view = instance.view;
      if (!view || view.webContents.isDestroyed() || instance.state !== "shown" || generation !== instance.layoutGeneration) return;
      let timer: ReturnType<typeof setTimeout> | undefined;
      const timeout = new Promise<null>(resolve => { timer = setTimeout(() => resolve(null), PREVIEW_SNAPSHOT_TIMEOUT_MS); });
      void Promise.race([view.webContents.capturePage(), timeout]).then((image) => {
        clearTimeout(timer);
        if (!image || image.isEmpty() || instance.state !== "shown" || generation !== instance.layoutGeneration) return;
        const url = `data:image/jpeg;base64,${image.toJPEG(70).toString("base64")}`;
        if (url.length <= PREVIEW_SNAPSHOT_MAX_CHARS) instance.lastSnapshot = url;
      }, () => clearTimeout(timer));
    }, this.#options.snapshotDelayMs ?? PREVIEW_SNAPSHOT_DELAY_MS);
  }

  #apply(instance: PreviewInstance): void {
    const view = instance.view;
    if (!view || instance.state === "closed") return;
    const visible = instance.confirmed && instance.bounds !== null && !instance.obscured && !instance.hidden && !this.#windowHidden;
    const rect = instance.bounds ? previewViewRect(instance.bounds, instance.viewport) : null;
    const moves = rect !== null && (instance.rect === null || rect.x !== instance.rect.x || rect.y !== instance.rect.y || rect.width !== instance.rect.width || rect.height !== instance.rect.height);
    // Hidden before it moves, moved before it shows (PR #9 re-review B1):
    // a shown view that changes place is hidden first, never moved in sight.
    if (!visible || (moves && instance.visible)) view.setVisible(false);
    if (rect && moves) { view.setBounds(rect); instance.rect = rect; }
    if (visible) view.setVisible(true);
    if (instance.state !== "loading") instance.state = visible ? "shown" : "hidden";
    if (visible !== instance.visible) {
      instance.visible = visible;
      this.visibility.push({ previewId: instance.previewId, visible, at: Date.now() });
      if (this.visibility.length > VISIBILITY_MAX) this.visibility.splice(0, this.visibility.length - VISIBILITY_MAX);
    }
  }

  async #configureSession(instance: PreviewInstance): Promise<void> {
    const { session, token } = instance;
    // Every refusal is recorded with its permission (PR #9 review I3).
    const deny = (kind: PreviewDenialV1["kind"], permission: string) => {
      this.diagnostics.deniedPermissions++;
      this.denials.push({ previewId: instance.previewId, kind, permission, at: Date.now() });
      if (this.denials.length > DENIALS_MAX) this.denials.splice(0, this.denials.length - DENIALS_MAX);
      return false;
    };
    session.setPermissionRequestHandler((_contents, permission, callback, details) => {
      const media = (details as { mediaTypes?: string[] } | undefined)?.mediaTypes?.length ? `:${(details as { mediaTypes: string[] }).mediaTypes.join("+")}` : "";
      deny("request", `${permission}${media}`);
      callback(false);
    });
    session.setPermissionCheckHandler((_contents, permission) => deny("check", permission));
    session.setDevicePermissionHandler((details) => deny("device", details.deviceType));
    session.setDisplayMediaRequestHandler((_request, callback) => { deny("display", "display-capture"); callback({}); });
    // Windows/Linux only; elsewhere the permission handlers above already refuse.
    try { session.setBluetoothPairingHandler((_details, callback) => { deny("bluetooth", "bluetooth"); callback({ confirmed: false }); }); } catch { /* unsupported */ }
    session.on("select-hid-device", (event, _details, callback) => { event.preventDefault(); deny("select", "hid"); callback(); });
    session.on("select-usb-device", (event, _details, callback) => { event.preventDefault(); deny("select", "usb"); callback(); });
    session.on("select-serial-port", (event, _ports, _contents, callback) => { event.preventDefault(); deny("select", "serial"); callback(""); });
    session.on("will-download", (event) => {
      event.preventDefault();
      this.diagnostics.blockedDownloads++;
      this.#note(instance, "preview download blocked");
    });
    session.webRequest.onBeforeRequest({ urls: ["<all_urls>"] }, (details, callback) => {
      const allowed = isPreviewRequestAllowed(details.url, token);
      if (!allowed) {
        this.diagnostics.blockedRequests++;
        this.#note(instance, "preview request blocked");
      }
      callback({ cancel: !allowed });
    });
    // No answer from outside the preview origin should ever arrive: watch for one.
    session.webRequest.onHeadersReceived({ urls: ["<all_urls>"] }, (details, callback) => {
      if (!isPreviewRequestAllowed(details.url, token)) {
        this.diagnostics.unexpectedResponses++;
        this.#note(instance, "preview unexpected response");
      }
      callback({});
    });
    session.setSpellCheckerEnabled(false);
    session.setUserAgent(genericUserAgent(session.getUserAgent()));
    await session.setProxy({ ...PREVIEW_PROXY });
    session.protocol.handle(PREVIEW_SCHEME, (request) => this.#serve(instance, request));
  }

  #configureContents(instance: PreviewInstance, contents: WebContents): void {
    const blocked = () => {
      this.diagnostics.blockedNavigations++;
      this.#note(instance, "preview navigation blocked");
      if (instance.blockedNoticeSent) return;
      instance.blockedNoticeSent = true;
      this.#emit({ previewId: instance.previewId, kind: "blockedNavigation" });
    };
    contents.setWindowOpenHandler(() => { blocked(); return { action: "deny" }; });
    contents.on("will-navigate", (details) => {
      if (!isPreviewNavigationAllowed(details.url, instance.token, true)) { details.preventDefault(); blocked(); }
    });
    contents.on("will-redirect", (details) => {
      if (!isPreviewNavigationAllowed(details.url, instance.token, details.isMainFrame)) { details.preventDefault(); blocked(); }
    });
    contents.on("will-frame-navigate", (details) => {
      if (!isPreviewNavigationAllowed(details.url, instance.token, details.isMainFrame)) { details.preventDefault(); blocked(); }
    });
    contents.on("will-attach-webview", (event) => event.preventDefault());
    // The app's window shortcuts (⌘/Ctrl+1·2·3·\\·N) still work while the
    // preview has focus: they go to the app page instead (PR #9 review minor).
    contents.on("before-input-event", (event, input) => {
      if (!isAppShortcut(input)) return;
      const window = this.#options.window();
      if (!window || window.isDestroyed()) return;
      event.preventDefault();
      window.webContents.focus();
      const modifiers = [input.meta && "meta", input.control && "control", input.shift && "shift", input.alt && "alt"].filter((value): value is string => Boolean(value));
      window.webContents.sendInputEvent({ type: "keyDown", keyCode: input.key, modifiers: modifiers as Electron.InputEvent["modifiers"] });
    });
    // A page never keeps itself open (beforeunload).
    contents.on("will-prevent-unload", (event) => event.preventDefault());
    contents.on("render-process-gone", () => this.#close(instance, "crashed"));
    // A page that stops answering is closed at once (plan 2.3): hiding it
    // would only stop Chromium watching it, and it would come back hung.
    contents.on("unresponsive", () => {
      this.#emit({ previewId: instance.previewId, kind: "unresponsive" });
      this.#close(instance, "crashed");
    });
    contents.on("did-finish-load", () => {
      if (instance.state === "closed") return;
      instance.state = "hidden";
      this.#apply(instance);
      this.#scheduleSnapshot(instance);
      this.#emit({ previewId: instance.previewId, kind: "loaded" });
    });
    // What the person did in the page shows in the next still image.
    contents.on("input-event", () => { if (!instance.snapshotTimer) this.#scheduleSnapshot(instance); });
    contents.on("did-fail-load", (_event, code, _description, _url, isMainFrame) => {
      // -3: aborted by a newer navigation (a reload, say).
      if (!isMainFrame || code === -3 || instance.state === "closed") return;
      this.#emit({ previewId: instance.previewId, kind: "loadFailed" });
    });
    if (!this.#options.devTools) contents.on("devtools-opened", () => contents.closeDevTools());
    contents.setWebRTCIPHandlingPolicy("disable_non_proxied_udp");
  }

  async #serve(instance: PreviewInstance, request: Request): Promise<Response> {
    const notFound = () => { this.diagnostics.notFound++; return new Response(null, { status: 404, headers: previewResponseHeaders(null) }); };
    const status = (code: number) => new Response(null, { status: code, headers: previewResponseHeaders(null) });
    if (instance.state === "closed") return notFound();
    // Every request counts, refused ones too (review I2).
    if (++instance.requests > PREVIEW_TOKEN_MAX_REQUESTS_V1) {
      this.#close(instance, "limit");
      return notFound();
    }
    if (!PREVIEW_ALLOWED_METHODS.includes(request.method)) {
      this.diagnostics.refusedMethods++;
      return new Response(null, { status: 405, headers: { ...previewResponseHeaders(null), Allow: PREVIEW_ALLOWED_METHODS.join(", ") } });
    }
    const decision = parsePreviewRequestUrl(request.url, instance.token, instance.entry);
    if (!decision.ok) return notFound();
    if (this.#suspended) return status(503);
    // At most a few files at once; the rest wait, and a full queue is busy.
    if (instance.reading >= PREVIEW_MAX_CONCURRENT_READS) {
      if (instance.waiting.length >= PREVIEW_MAX_QUEUED_READS) return status(503);
      await new Promise<void>(resolve => instance.waiting.push(resolve));
      if (isClosed(instance) || this.#suspended) return notFound();
    }
    instance.reading++;
    // The read slot is given back when the last sidecar call settles, not
    // when the page gets its answer: a timed-out call still occupies it.
    let lastCall: Promise<unknown> = Promise.resolve();
    try {
      return await this.#readFile(instance, request, decision.path, (call) => { lastCall = call; }, notFound, status);
    } finally {
      void lastCall.catch(() => undefined).finally(() => {
        instance.reading--;
        instance.waiting.shift()?.();
      });
    }
  }

  async #readFile(
    instance: PreviewInstance, request: Request, path: string,
    track: (call: Promise<unknown>) => void, notFound: () => Response, status: (code: number) => Response,
  ): Promise<Response> {
    const deadline = Date.now() + (this.#options.readDeadlineMs ?? PREVIEW_READ_DEADLINE_MS);
    const chunks: Buffer[] = [];
    let offset = 0;
    let length: number | null = null;
    let version: string | null = null;
    do {
      const sidecar = this.#options.sidecar();
      if (!sidecar || isClosed(instance) || this.#suspended) return notFound();
      // The byte budget is checked before asking, not after the answer.
      if (instance.bytes + instance.reserved + PREVIEW_ASSET_CHUNK_BYTES_V1 > PREVIEW_TOKEN_MAX_BYTES_V1) {
        this.#close(instance, "limit");
        return notFound();
      }
      const left = deadline - Date.now();
      if (left <= 0) return status(504);
      instance.reserved += PREVIEW_ASSET_CHUNK_BYTES_V1;
      const call = sidecar.previewReadAsset({ ...instance.ref, previewHandle: instance.handle, path, offset });
      track(call);
      let timer: ReturnType<typeof setTimeout> | undefined;
      let answer: PreviewReadAssetResultV1 | "timeout" | undefined;
      try {
        answer = await Promise.race([call, new Promise<"timeout">(resolve => { timer = setTimeout(() => resolve("timeout"), left); })]);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        // The workspace itself is gone (discarded, cleaned after a merge,
        // marked for recovery, swapped for another folder): this preview
        // cannot continue.
        if (/^(?:recoveryRequired|manifestMismatch|symlinkEscape):/.test(message)) this.#close(instance, "workspaceUnavailable");
        return status(503);
      } finally {
        clearTimeout(timer);
        // Answered (or failed): the reservation ends now. Timed out: it ends
        // when the sidecar finally answers.
        const unreserve = () => { instance.reserved -= PREVIEW_ASSET_CHUNK_BYTES_V1; };
        if (answer === "timeout") void call.catch(() => undefined).finally(unreserve);
        else unreserve();
      }
      if (answer === "timeout") return status(504);
      if (isClosed(instance) || answer.outcome !== "ok") return notFound();
      if (length === null) { length = answer.byteLength; version = answer.version; }
      else if (answer.byteLength !== length || answer.version !== version) {
        // Changed while being read: the page asks again (a reload).
        return status(503);
      }
      const bytes = Buffer.from(answer.bytesBase64 ?? "", "base64");
      instance.bytes += bytes.byteLength;
      if (instance.bytes > PREVIEW_TOKEN_MAX_BYTES_V1) {
        this.#close(instance, "limit");
        return notFound();
      }
      chunks.push(bytes);
      offset += bytes.byteLength;
      if (bytes.byteLength === 0) break;
    } while (offset < (length ?? 0));
    if (isClosed(instance)) return notFound();
    const body = request.method === "HEAD" ? null : Buffer.concat(chunks);
    return new Response(body, { status: 200, headers: previewResponseHeaders(path) });
  }

  #note(instance: PreviewInstance, label: string): void {
    if (instance.logged.has(label)) return;
    instance.logged.add(label);
    try { this.#options.log(label); } catch { /* diagnostics never block */ }
  }

  #emit(event: PreviewEventV1): void {
    try { this.#options.send(event); } catch { /* the screen is gone */ }
  }

  #close(instance: PreviewInstance, reason: PreviewCloseReasonV1): void {
    if (instance.state === "closed") return;
    instance.state = "closed";
    if (this.#current === instance) this.#current = undefined;
    this.diagnostics.closed++;
    this.#dispose(instance);
    this.#emit({ previewId: instance.previewId, kind: "closed", reason });
  }

  #dispose(instance: PreviewInstance): void {
    instance.state = "closed";
    clearTimeout(instance.idleTimer);
    instance.idleTimer = undefined;
    clearTimeout(instance.snapshotTimer);
    instance.snapshotTimer = undefined;
    instance.lastSnapshot = null;
    // Requests still waiting for a read never reach the sidecar.
    for (const release of instance.waiting.splice(0)) release();
    const view = instance.view;
    instance.view = undefined;
    if (view) {
      const window = this.#options.window();
      try { if (window && !window.isDestroyed()) window.contentView.removeChildView(view); } catch { /* already detached */ }
      try { if (!view.webContents.isDestroyed()) view.webContents.close(); } catch { /* already gone */ }
    }
    const { session } = instance;
    try { session.protocol.unhandle(PREVIEW_SCHEME); } catch { /* never handled */ }
    void session.clearStorageData().catch(() => undefined);
    void session.clearCache().catch(() => undefined);
    void session.closeAllConnections().catch(() => undefined);
  }
}

/** The app's own window shortcuts (workbench.tsx): ⌘ on macOS, Ctrl elsewhere,
 * with 1, 2, 3, \\ or N, as workbench.tsx reads them. Nothing else a page
 * types is taken from it. */
export function isAppShortcut(input: { type: string; key: string; meta: boolean; control: boolean; alt: boolean; shift: boolean }, platform: string = process.platform): boolean {
  const mod = platform === "darwin" ? input.meta && !input.control : input.control && !input.meta;
  if (input.type !== "keyDown" || !mod || input.alt || input.shift) return false;
  return ["1", "2", "3", "\\", "n", "N"].includes(input.key);
}

/** Read through a call: the state changes across awaits. */
function isClosed(instance: PreviewInstance): boolean { return instance.state === "closed"; }

/** The view's preferences (plan 2.3 table). No preload: the page has no
 * `ipcRenderer` and no app bridge. */
export function previewWebPreferences(session: Session, devTools: boolean): WebPreferences {
  return {
    session,
    sandbox: true,
    contextIsolation: true,
    nodeIntegration: false,
    nodeIntegrationInSubFrames: false,
    nodeIntegrationInWorker: false,
    webSecurity: true,
    allowRunningInsecureContent: false,
    webviewTag: false,
    devTools,
    spellcheck: false,
    navigateOnDragDrop: false,
    safeDialogs: true,
    autoplayPolicy: "document-user-activation-required",
    backgroundThrottling: true,
    enableWebSQL: false,
    plugins: false,
    experimentalFeatures: false,
    // No file pickers at all (T7): the page has no way to ask for a file
    // to write, whatever the permission handlers would answer.
    disableBlinkFeatures: "FileSystemAccess,FileSystemAccessLocal",
  };
}
