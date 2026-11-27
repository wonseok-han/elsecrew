// Result preview manager with fake Electron objects (plan 2.6 P2: session
// setup order, protocol handler, lifecycle, quit-while-opening race).
import assert from "node:assert/strict";
import test from "node:test";

import { isAppShortcut, PreviewManagerV1, type PreviewManagerOptionsV1 } from "../electron/preview-manager.ts";
import type { PreviewEventV1, PreviewReadAssetRequestV1, PreviewResolveRequestV1 } from "../src/m5/contracts/preview-v1.ts";

type Listener = (...args: any[]) => unknown;

class FakeSession {
  readonly partition: string;
  readonly calls: string[] = [];
  readonly listeners = new Map<string, Listener>();
  permissionRequest?: Listener; permissionCheck?: Listener; devicePermission?: Listener; displayMedia?: Listener;
  beforeRequest?: Listener; headersReceived?: Listener;
  proxy: unknown;
  userAgent = "Mozilla/5.0 AppleWebKit/537.36 elsecrew/0.1.2 Chrome/140.0.0.0 Electron/43.4.0 Safari/537.36";
  handler?: (request: Request) => Promise<Response>;
  readonly log: string[];
  constructor(partition: string, log: string[]) { this.partition = partition; this.log = log; }
  setPermissionRequestHandler(handler: Listener) { this.permissionRequest = handler; this.log.push("permissions"); }
  setPermissionCheckHandler(handler: Listener) { this.permissionCheck = handler; }
  setDevicePermissionHandler(handler: Listener) { this.devicePermission = handler; }
  setDisplayMediaRequestHandler(handler: Listener) { this.displayMedia = handler; }
  setBluetoothPairingHandler() { throw new Error("unsupported on this platform"); }
  on(event: string, listener: Listener) { this.listeners.set(event, listener); }
  webRequest = {
    onBeforeRequest: (filter: { urls: string[] }, listener: Listener) => { assert.deepEqual(filter.urls, ["<all_urls>"]); this.beforeRequest = listener; this.log.push("webRequest"); },
    onHeadersReceived: (_filter: unknown, listener: Listener) => { this.headersReceived = listener; },
  };
  setSpellCheckerEnabled(enabled: boolean) { assert.equal(enabled, false); }
  getUserAgent() { return this.userAgent; }
  setUserAgent(value: string) { this.userAgent = value; }
  async setProxy(config: unknown) { this.proxy = config; this.log.push("proxy"); }
  protocol = {
    handle: (scheme: string, handler: (request: Request) => Promise<Response>) => { assert.equal(scheme, "elsecrew-preview"); this.handler = handler; this.log.push("protocol"); },
    unhandle: (scheme: string) => { assert.equal(scheme, "elsecrew-preview"); this.handler = undefined; this.calls.push("unhandle"); },
  };
  async clearStorageData() { this.calls.push("clearStorageData"); }
  async clearCache() { this.calls.push("clearCache"); }
  async closeAllConnections() { this.calls.push("closeAllConnections"); }
}

class FakeContents {
  readonly listeners = new Map<string, Listener>();
  openHandler?: () => { action: string };
  destroyed = false;
  url = "";
  webRtcPolicy = "";
  calls: string[] = [];
  readonly log: string[];
  constructor(log: string[]) { this.log = log; }
  on(event: string, listener: Listener) { this.listeners.set(event, listener); }
  setWindowOpenHandler(handler: () => { action: string }) { this.openHandler = handler; }
  setWebRTCIPHandlingPolicy(policy: string) { this.webRtcPolicy = policy; }
  async loadURL(url: string) { this.url = url; this.log.push("load"); }
  isDestroyed() { return this.destroyed; }
  close() { this.destroyed = true; this.calls.push("close"); }
  reloadIgnoringCache() { this.calls.push("reload"); }
  closeDevTools() { this.calls.push("closeDevTools"); }
  /** Set to hold captures until released. */
  captureGate: Promise<void> | undefined;
  captures = 0;
  async capturePage() {
    this.captures++;
    if (this.captureGate) await this.captureGate;
    return { isEmpty: () => false, toJPEG: (quality: number) => { assert.equal(quality, 70); return Buffer.from("jpeg"); } };
  }
}

class FakeView {
  readonly webContents: FakeContents;
  bounds: unknown; visible: boolean | undefined; background = "";
  readonly webPreferences: Record<string, unknown>;
  constructor(webPreferences: Record<string, unknown>, log: string[]) { this.webPreferences = webPreferences; this.webContents = new FakeContents(log); log.push("view"); }
  setBounds(bounds: unknown) { this.bounds = bounds; }
  setVisible(visible: boolean) { this.visible = visible; }
  setBackgroundColor(color: string) { this.background = color; }
}

function harness(overrides: Partial<PreviewManagerOptionsV1> = {}, files: Record<string, Buffer> = { "index.html": Buffer.from("<p>hi</p>") }) {
  const log: string[] = [];
  const sessions: FakeSession[] = [];
  const views: FakeView[] = [];
  const events: PreviewEventV1[] = [];
  const reads: PreviewReadAssetRequestV1[] = [];
  const resolves: PreviewResolveRequestV1[] = [];
  const children: FakeView[] = [];
  const zoom = { factor: 1, scale: 1 };
  const window = {
    destroyed: false,
    size: [1280, 800],
    isDestroyed() { return this.destroyed; },
    getContentSize() { return this.size; },
    webContents: { focused: 0, sent: [] as unknown[], focus() { this.focused++; }, sendInputEvent(event: unknown) { this.sent.push(event); } },
    contentView: {
      addChildView: (view: FakeView) => { children.push(view); log.push("attach"); },
      removeChildView: (view: FakeView) => { children.splice(children.indexOf(view), 1); },
    },
  };
  let resolveGate: Promise<void> = Promise.resolve();
  let readGate: Promise<void> = Promise.resolve();
  const sidecar = {
    previewResolve: async (input: PreviewResolveRequestV1) => {
      resolves.push(input);
      await resolveGate;
      return { ...input, worktreeId: "worktree_1", workspaceIdentity: "workspace_1", previewHandle: "ph_dddddddddddddddddddddddddddddddd", entry: "index.html" as string | null, refusal: null };
    },
    previewReadAsset: async (input: PreviewReadAssetRequestV1) => {
      reads.push(input);
      await readGate;
      const file = files[input.path];
      if (!file) return { ...input, outcome: "notFound" as const, byteLength: null, version: null, bytesBase64: null };
      const chunk = file.subarray(input.offset, input.offset + 512 * 1024);
      return { ...input, outcome: "ok" as const, byteLength: file.length, version: `sha256:${"a".repeat(64)}` as const, bytesBase64: chunk.toString("base64") };
    },
  };
  const manager = new PreviewManagerV1({
    enabled: true,
    devTools: false,
    fromPartition: (partition) => { const session = new FakeSession(partition, log); sessions.push(session); log.push("session"); return session as never; },
    createView: (webPreferences) => { const view = new FakeView(webPreferences as Record<string, unknown>, log); views.push(view); return view as never; },
    window: () => window as never,
    sidecar: () => sidecar,
    send: (event) => { events.push(event); },
    log: () => undefined,
    zoomFactor: () => zoom.factor,
    displayScale: () => zoom.scale,
    ...overrides,
  });
  return {
    manager, log, sessions, views, events, reads, resolves, children, window, sidecar, zoom,
    gate(promise: Promise<void>) { resolveGate = promise; },
    gateReads(promise: Promise<void>) { readGate = promise; },
    open: (entry: string | null = null) => manager.request({ action: "open", projectId: "project_1", chatId: "chat_1", entry, source: "workspace", bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", pixelRatio: 1, page: { width: 1280, height: 800 } }),
  };
}

async function opened(h: ReturnType<typeof harness>) {
  const result = await h.open();
  assert.equal(result.action, "open");
  assert.ok(result.action === "open" && result.outcome === "opened");
  return { result, session: h.sessions.at(-1)!, view: h.views.at(-1)!, token: h.sessions.at(-1)!.partition.slice("preview-".length) };
}

test("a preview session is fully configured before its view exists or loads", async () => {
  const h = harness();
  const { result, session, view, token } = await opened(h);
  assert.deepEqual(h.log, ["session", "permissions", "webRequest", "proxy", "protocol", "view", "attach", "load"]);
  assert.match(session.partition, /^preview-[0-9a-f]{32}$/);
  assert.equal(result.action === "open" && result.outcome === "opened" && result.previewId.includes(token), false, "the renderer never gets the token");
  assert.deepEqual(h.resolves, [{ projectId: "project_1", chatId: "chat_1", entry: null }]);
  assert.equal(view.webContents.url, `elsecrew-preview://${token}/index.html`);
  // No preload, sandboxed, isolated, no Node, no webview, no devtools.
  const prefs = view.webPreferences;
  assert.equal(prefs.session, session);
  assert.equal("preload" in prefs, false);
  for (const [key, value] of Object.entries({ sandbox: true, contextIsolation: true, nodeIntegration: false, nodeIntegrationInSubFrames: false, webSecurity: true, allowRunningInsecureContent: false, webviewTag: false, devTools: false, spellcheck: false, navigateOnDragDrop: false, safeDialogs: true, autoplayPolicy: "document-user-activation-required", backgroundThrottling: true })) {
    assert.equal(prefs[key], value, key);
  }
  assert.equal(prefs.disableBlinkFeatures, "FileSystemAccess,FileSystemAccessLocal");
  assert.equal(view.webContents.webRtcPolicy, "disable_non_proxied_udp");
  assert.deepEqual(session.proxy, { mode: "fixed_servers", proxyRules: "127.0.0.1:9", proxyBypassRules: "<-loopback>" });
  assert.equal(session.userAgent, "Mozilla/5.0 AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36");
  assert.deepEqual(view.bounds, { x: 700, y: 60, width: 500, height: 600 });
});

test("every permission, device and screen request is refused; downloads are cancelled (T7·T10)", async () => {
  const h = harness();
  const { session } = await opened(h);
  let granted: unknown;
  session.permissionRequest!({}, "media", (value: boolean) => { granted = value; });
  assert.equal(granted, false);
  for (const permission of ["geolocation", "notifications", "clipboard-read", "fullscreen", "fileSystem", "hid", "serial", "usb", "midiSysex"]) {
    assert.equal(session.permissionCheck!({}, permission), false, permission);
  }
  assert.equal(session.devicePermission!({ deviceType: "hid" }), false);
  let streams: unknown;
  session.displayMedia!({}, (value: unknown) => { streams = value; });
  assert.deepEqual(streams, {});
  for (const event of ["select-hid-device", "select-usb-device"]) {
    let prevented = false; let chosen: unknown = "unset";
    session.listeners.get(event)!({ preventDefault: () => { prevented = true; } }, {}, (id?: string) => { chosen = id; });
    assert.equal(prevented, true); assert.equal(chosen, undefined);
  }
  let port: unknown;
  session.listeners.get("select-serial-port")!({ preventDefault() {} }, [], {}, (id: string) => { port = id; });
  assert.equal(port, "");
  let downloadPrevented = false;
  session.listeners.get("will-download")!({ preventDefault: () => { downloadPrevented = true; } });
  assert.equal(downloadPrevented, true);
  assert.equal(h.manager.diagnostics.blockedDownloads, 1);
});

test("webRequest cancels everything but the preview's own origin (T1)", async () => {
  const h = harness();
  const { session, token } = await opened(h);
  const decide = (url: string) => { let cancel: unknown; session.beforeRequest!({ url }, (value: { cancel: boolean }) => { cancel = value.cancel; }); return cancel; };
  assert.equal(decide(`elsecrew-preview://${token}/a.js`), false);
  assert.equal(decide("data:image/png;base64,AA=="), false);
  for (const url of ["https://example.com/x", "http://127.0.0.1:9/", "ws://127.0.0.1:1/", "file:///etc/hosts", "app://m5/index.html", `elsecrew-preview://${"f".repeat(32)}/a.js`]) {
    assert.equal(decide(url), true, url);
  }
  assert.equal(h.manager.diagnostics.blockedRequests, 6);
});

test("the protocol handler serves GET/HEAD only, with fixed headers, from this workspace (T4·T7·T15)", async () => {
  const big = Buffer.alloc(1_300_000, 7);
  const h = harness({}, { "index.html": Buffer.from("<p>hi</p>"), "big.js": big, "logo.png": Buffer.from([0, 1, 2]) });
  const { session, token } = await opened(h);
  const serve = (url: string, method = "GET") => session.handler!(new Request(url, { method }));
  const page = await serve(`elsecrew-preview://${token}/`);
  assert.equal(page.status, 200);
  assert.equal(await page.text(), "<p>hi</p>");
  assert.equal(page.headers.get("content-type"), "text/html; charset=utf-8");
  assert.equal(page.headers.get("x-content-type-options"), "nosniff");
  assert.equal(page.headers.get("cache-control"), "no-store");
  assert.match(page.headers.get("content-security-policy")!, /connect-src 'self'/);
  // Every read names the sidecar's record of the verified root (review B1).
  assert.deepEqual(h.reads.at(-1), { chatId: "chat_1", projectId: "project_1", worktreeId: "worktree_1", workspaceIdentity: "workspace_1", previewHandle: "ph_dddddddddddddddddddddddddddddddd", path: "index.html", offset: 0 });
  // Chunks are joined in order.
  const script = await serve(`elsecrew-preview://${token}/big.js`);
  assert.deepEqual(Buffer.from(await script.arrayBuffer()), big);
  assert.deepEqual(h.reads.filter(read => read.path === "big.js").map(read => read.offset), [0, 524288, 1048576]);
  const head = await serve(`elsecrew-preview://${token}/logo.png`, "HEAD");
  assert.equal(head.status, 200);
  assert.equal(head.headers.get("content-type"), "image/png");
  for (const method of ["POST", "PUT", "DELETE", "PATCH"]) {
    const refused = await serve(`elsecrew-preview://${token}/index.html`, method);
    assert.equal(refused.status, 405, method);
    assert.equal(refused.headers.get("allow"), "GET, HEAD");
  }
  const readsBefore = h.reads.length;
  for (const url of [
    `elsecrew-preview://${"f".repeat(32)}/index.html`, `elsecrew-preview://${token}/.env`, `elsecrew-preview://${token}/.git/HEAD`,
    `elsecrew-preview://${token}/a%2f..%2f..%2fx.txt`, `elsecrew-preview://${token}/key.pem`,
  ]) assert.equal((await serve(url)).status, 404, url);
  // `%2e%2e` is a dot segment to the URL parser itself: it folds inside the
  // origin's root before the handler sees it, never above it.
  assert.equal(new Request(`elsecrew-preview://${token}/%2e%2e/%2e%2e/index.html`).url, `elsecrew-preview://${token}/index.html`);
  assert.equal(h.reads.length, readsBefore, "refused paths never reach the sidecar");
  assert.equal((await serve(`elsecrew-preview://${token}/missing.html`)).status, 404);
});

test("a workspace that went away closes the preview; a file changing mid-read is retried by the page", async () => {
  const h = harness();
  const { session, token, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  const handler = session.handler!;
  let version = 0;
  h.sidecar.previewReadAsset = async (input) => ({ ...input, outcome: "ok", byteLength: 600 * 1024, version: `sha256:${String(version++ % 10).repeat(64)}` as `sha256:${string}`, bytesBase64: Buffer.alloc(input.offset === 0 ? 512 * 1024 : 88 * 1024).toString("base64") });
  assert.equal((await session.handler!(new Request(`elsecrew-preview://${token}/index.html`))).status, 503);
  h.sidecar.previewReadAsset = async () => { throw new Error("recoveryRequired: chat workspace snapshot is not active"); };
  assert.equal((await session.handler!(new Request(`elsecrew-preview://${token}/index.html`))).status, 503);
  assert.deepEqual(h.events.at(-1), { previewId, kind: "closed", reason: "workspaceUnavailable" });
  assert.equal(session.handler, undefined, "the scheme is unhandled on that session");
  assert.equal((await handler(new Request(`elsecrew-preview://${token}/index.html`))).status, 404, "a closed token serves nothing");
});

test("one token serves at most 2,000 requests", async () => {
  const h = harness();
  const { session, token } = await opened(h);
  for (let index = 0; index < 2000; index++) assert.equal((await session.handler!(new Request(`elsecrew-preview://${token}/index.html`))).status, 200);
  assert.equal((await session.handler!(new Request(`elsecrew-preview://${token}/index.html`))).status, 404);
  assert.equal(h.events.at(-1)?.kind, "closed");
  assert.equal((h.events.at(-1) as { reason: string }).reason, "limit");
});

test("navigation and new windows stay inside; the screen hears about it once (T9)", async () => {
  const h = harness();
  const { view, token } = await opened(h);
  const contents = view.webContents;
  assert.deepEqual(contents.openHandler!(), { action: "deny" });
  for (const [event, url, isMainFrame] of [["will-navigate", "https://example.com/", true], ["will-redirect", "http://127.0.0.1:1/", true], ["will-frame-navigate", "https://example.com/frame", false]] as const) {
    let prevented = false;
    contents.listeners.get(event)!({ url, isMainFrame, preventDefault: () => { prevented = true; } });
    assert.equal(prevented, true, event);
  }
  let inside = false;
  contents.listeners.get("will-navigate")!({ url: `elsecrew-preview://${token}/b.html`, isMainFrame: true, preventDefault: () => { inside = true; } });
  assert.equal(inside, false);
  let blank = false;
  contents.listeners.get("will-frame-navigate")!({ url: "about:blank", isMainFrame: false, preventDefault: () => { blank = true; } });
  assert.equal(blank, false);
  assert.equal(h.events.filter(event => event.kind === "blockedNavigation").length, 1);
  let webview = false;
  contents.listeners.get("will-attach-webview")!({ preventDefault: () => { webview = true; } });
  assert.equal(webview, true);
  let unload = false;
  contents.listeners.get("will-prevent-unload")!({ preventDefault: () => { unload = true; } });
  assert.equal(unload, true);
});

test("replacing, closing and crashing drop the token and clear the partition (T12)", async () => {
  const h = harness();
  const first = await opened(h);
  const second = await opened(h);
  assert.notEqual(first.session.partition, second.session.partition);
  assert.deepEqual(first.session.calls.sort(), ["clearCache", "clearStorageData", "closeAllConnections", "unhandle"].sort());
  assert.equal(first.view.webContents.destroyed, true);
  assert.equal(h.children.length, 1);
  assert.equal(h.events.find(event => event.kind === "closed")?.previewId, first.result.action === "open" && first.result.outcome === "opened" ? first.result.previewId : "");
  // A stale previewId is no longer the open one.
  const firstId = first.result.action === "open" && first.result.outcome === "opened" ? first.result.previewId : "";
  assert.equal((await h.manager.request({ action: "reload", previewId: firstId })).action, "reload");
  assert.deepEqual(first.view.webContents.calls.filter(call => call === "reload"), []);
  second.view.webContents.listeners.get("unresponsive")!();
  assert.equal(h.events.at(-2)?.kind, "unresponsive");
  assert.equal(second.view.webContents.destroyed, true);
  assert.equal(h.children.length, 0);
  assert.equal((h.events.at(-1) as { reason: string }).reason, "crashed");
  assert.equal(h.manager.currentWebContents, undefined);
});

test("quitting while the sidecar is still resolving creates nothing (종료 중 열기 경합)", async () => {
  const h = harness();
  let release!: () => void;
  h.gate(new Promise<void>(resolve => { release = resolve; }));
  const pending = h.open();
  await Promise.resolve();
  h.manager.shutdown();
  release();
  await assert.rejects(pending, /superseded/);
  assert.deepEqual(h.sessions, []);
  assert.deepEqual(h.views, []);
  await assert.rejects(h.open(), /quits/);
  assert.equal(JSON.stringify(await h.manager.request({ action: "status" })), JSON.stringify({ action: "status", enabled: false }));
});

test("a newer open wins over one still resolving", async () => {
  const h = harness();
  let release!: () => void;
  h.gate(new Promise<void>(resolve => { release = resolve; }));
  const older = h.open();
  h.gate(Promise.resolve());
  const newer = h.open();
  release();
  await assert.rejects(older, /superseded/);
  assert.equal((await newer).action, "open");
  assert.equal(h.views.length, 1);
});

test("quitting closes an open preview at once", async () => {
  const h = harness();
  const { view, session } = await opened(h);
  h.manager.shutdown();
  assert.equal(view.webContents.destroyed, true);
  assert.ok(session.calls.includes("clearStorageData"));
  assert.equal((h.events.at(-1) as { reason: string }).reason, "quitting");
});

test("hidden tabs close after the idle wait; layout shows, obscures with a still image, and narrows", async () => {
  const h = harness({ idleCloseMs: 20, snapshotDelayMs: 5 });
  const { view, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  view.webContents.listeners.get("did-finish-load")!();
  assert.notEqual(view.visible, true, "hidden until the page's first layout (review I1)");
  const shown = await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "narrow", obscured: false, pixelRatio: 1, page: { width: 1280, height: 800 } });
  assert.equal(view.visible, true);
  assert.equal(shown.action === "layout" && shown.state, "shown");
  assert.deepEqual(view.bounds, { x: 755, y: 60, width: 390, height: 600 });
  // The still image is taken ahead of time while shown.
  await new Promise(resolve => setTimeout(resolve, 30));
  const covered = await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "narrow", obscured: true, pixelRatio: 1, page: { width: 1280, height: 800 } });
  assert.equal(covered.action === "layout" && covered.snapshot, `data:image/jpeg;base64,${Buffer.from("jpeg").toString("base64")}`);
  assert.equal(view.visible, false);
  // A stage that does not fit hides the view; it is never an error (review B1).
  const outside = await h.manager.request({ action: "layout", previewId, bounds: { x: 1200, y: 0, width: 500, height: 600 }, viewport: "wide", obscured: false, pixelRatio: 1, page: { width: 1280, height: 800 } });
  assert.equal(outside.action, "layout");
  assert.equal(view.visible, false);
  h.manager.setWindowHidden(true);
  await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", obscured: false, pixelRatio: 1, page: { width: 1280, height: 800 } });
  assert.equal(view.visible, false, "a minimized window keeps it hidden");
  h.manager.setWindowHidden(false);
  assert.equal(view.visible, true);
  await h.manager.request({ action: "hide", previewId });
  assert.equal(view.visible, false);
  await new Promise(resolve => setTimeout(resolve, 40));
  assert.equal(view.webContents.destroyed, true);
  assert.equal((h.events.at(-1) as { reason: string }).reason, "idle");
});

test("a task with no page creates nothing; the flag off refuses everything but status", async () => {
  const h = harness();
  h.sidecar.previewResolve = async (input) => ({ ...input, worktreeId: "w", workspaceIdentity: "i", entry: null, refusal: null });
  assert.deepEqual(await h.open(), { action: "open", outcome: "empty", reason: "noEntry" });
  assert.deepEqual(h.sessions, []);
  const off = harness({ enabled: false });
  assert.equal(JSON.stringify(await off.manager.request({ action: "status" })), JSON.stringify({ action: "status", enabled: false }));
  await assert.rejects(off.open(), /not enabled/);
  assert.deepEqual(off.resolves, []);
});

test("covering hides at once even while a capture hangs; a late capture is thrown away (review I3)", async () => {
  const h = harness({ idleCloseMs: 30, snapshotDelayMs: 5 });
  const { view, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  let release!: () => void;
  view.webContents.captureGate = new Promise<void>(resolve => { release = resolve; });
  view.webContents.listeners.get("did-finish-load")!();
  const bounds = { x: 700, y: 60, width: 500, height: 600 };
  await h.manager.request({ action: "layout", previewId, bounds, viewport: "wide", obscured: false, pixelRatio: 1, page: { width: 1280, height: 800 } });
  await new Promise(resolve => setTimeout(resolve, 20));
  assert.ok(view.webContents.captures >= 1, "a capture is in flight");
  // The cover answers without waiting for that capture, and the view is hidden already.
  const covered = await Promise.race([
    h.manager.request({ action: "layout", previewId, bounds, viewport: "wide", obscured: true, pixelRatio: 1, page: { width: 1280, height: 800 } }),
    new Promise(resolve => setTimeout(() => resolve("waited"), 50)),
  ]);
  assert.notEqual(covered, "waited");
  assert.equal(view.visible, false);
  assert.equal((covered as { snapshot: string | null }).snapshot, null, "nothing taken yet: no stale image");
  // Another tab while the capture still hangs: it stays hidden and still closes when idle.
  await h.manager.request({ action: "hide", previewId });
  release();
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(view.visible, false, "the late capture changed nothing");
  await new Promise(resolve => setTimeout(resolve, 50));
  assert.equal(view.webContents.destroyed, true);
  assert.equal((h.events.at(-1) as { reason: string }).reason, "idle");
});

test("parallel reads: at most four reach the sidecar, a full queue is busy, closing drops the waiting ones (review I2)", async () => {
  const h = harness({}, { "index.html": Buffer.from("<p>hi</p>"), "a.js": Buffer.from("a") });
  const { session, token, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  let release!: () => void;
  h.gateReads(new Promise<void>(resolve => { release = resolve; }));
  const pending = Array.from({ length: 4 + 64 }, () => session.handler!(new Request(`elsecrew-preview://${token}/a.js`)));
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(h.reads.length, 4, "only four sidecar reads at once");
  // The queue is full: one more is answered busy without waiting.
  assert.equal((await session.handler!(new Request(`elsecrew-preview://${token}/a.js`))).status, 503);
  await h.manager.request({ action: "close", previewId });
  release();
  const statuses = (await Promise.all(pending)).map(response => response.status);
  assert.ok(statuses.every(status => status === 404), JSON.stringify([...new Set(statuses)]));
  assert.equal(h.reads.length, 4, "the waiting requests never reached the sidecar");
});

test("a read past its deadline answers 504 and keeps its slot until the sidecar answers (review I2)", async () => {
  const h = harness({ readDeadlineMs: 20 }, { "index.html": Buffer.from("<p>hi</p>"), "a.js": Buffer.from("a") });
  const { session, token } = await opened(h);
  let release!: () => void;
  h.gateReads(new Promise<void>(resolve => { release = resolve; }));
  const slow = Array.from({ length: 4 }, () => session.handler!(new Request(`elsecrew-preview://${token}/a.js`)));
  assert.deepEqual((await Promise.all(slow)).map(response => response.status), [504, 504, 504, 504]);
  // Four calls are still in the sidecar: the next request waits for a slot.
  const next = session.handler!(new Request(`elsecrew-preview://${token}/a.js`));
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(h.reads.length, 4);
  h.gateReads(Promise.resolve());
  release();
  assert.equal((await next).status, 200);
  assert.equal(h.reads.length, 5);
});

test("the byte budget is checked before asking the sidecar (review I2)", async () => {
  const big = Buffer.alloc(5 * 1024 * 1024, 1);
  const h = harness({}, { "index.html": Buffer.from("<p>hi</p>"), "big.js": big });
  const { session, token } = await opened(h);
  let status = 200;
  for (let index = 0; index < 13 && status === 200; index++) {
    status = (await session.handler!(new Request(`elsecrew-preview://${token}/big.js`))).status;
  }
  assert.equal(status, 404);
  // 64 MiB is 128 chunks: no 129th chunk was ever asked for.
  assert.equal(h.reads.length, 128);
  assert.equal((h.events.at(-1) as { reason: string }).reason, "limit");
});

test("refused requests count toward the request limit too (review I2)", async () => {
  const h = harness();
  const { session, token } = await opened(h);
  for (let index = 0; index < 2000; index++) {
    assert.equal((await session.handler!(new Request(`elsecrew-preview://${token}/index.html`, { method: "POST" }))).status, 405);
  }
  assert.equal((await session.handler!(new Request(`elsecrew-preview://${token}/index.html`))).status, 404);
  assert.equal(h.reads.length, 0);
  assert.equal((h.events.at(-1) as { reason: string }).reason, "limit");
});

test("quit preparation suspends and closes; a called-off one resumes; a replaced page closes and cancels an open on its way (review I4)", async () => {
  const h = harness();
  const { view, session, token } = await opened(h);
  const handler = session.handler!;
  h.manager.suspend();
  assert.equal(view.webContents.destroyed, true);
  assert.equal((h.events.at(-1) as { reason: string }).reason, "quitting");
  assert.equal((await handler(new Request(`elsecrew-preview://${token}/index.html`))).status, 404);
  await assert.rejects(h.open(), /quits/);
  assert.equal(JSON.stringify(await h.manager.request({ action: "status" })), JSON.stringify({ action: "status", enabled: false }));
  h.manager.resume();
  const reopened = await opened(h);
  // The app page reloads while an open waits for the sidecar.
  let release!: () => void;
  h.gate(new Promise<void>(resolve => { release = resolve; }));
  const pendingOpen = h.open();
  await Promise.resolve();
  h.manager.pageReplaced();
  release();
  await assert.rejects(pendingOpen, /superseded/);
  assert.equal(reopened.view.webContents.destroyed, true);
  assert.equal(h.views.length, 2, "the open on its way created no view");
  // After shutdown, resume never reopens.
  h.manager.shutdown();
  h.manager.resume();
  await assert.rejects(h.open(), /quits/);
});

test("bounds are measured again after the sidecar and session answer; the view waits for the page's layout (review minor, I1)", async () => {
  const h = harness();
  let release!: () => void;
  h.gate(new Promise<void>(resolve => { release = resolve; }));
  const pending = h.open();
  await Promise.resolve();
  h.window.size = [900, 800];
  release();
  const result = await pending;
  assert.ok(result.action === "open" && result.outcome === "opened");
  const view = h.views[0]!;
  assert.notEqual(view.visible, true);
  view.webContents.listeners.get("did-finish-load")!();
  await h.manager.request({ action: "layout", previewId: result.previewId, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", obscured: false, pixelRatio: 1, page: { width: 900, height: 800 } });
  // Cut to the smaller window, never past it.
  assert.deepEqual(view.bounds, { x: 700, y: 60, width: 200, height: 600 });
  assert.equal(view.visible, true);
});

test("zoom: the page's CSS pixels become window pixels, rounded inwards; a stale measurement hides (PR #9 review B1)", async () => {
  const h = harness();
  h.zoom.factor = 1.25;
  h.zoom.scale = 2;
  const result = await h.manager.request({ action: "open", projectId: "project_1", chatId: "chat_1", entry: null, source: "workspace", bounds: { x: 700.4, y: 60, width: 400, height: 500 }, viewport: "wide", pixelRatio: 2.5, page: { width: 1024, height: 640 } });
  assert.ok(result.action === "open" && result.outcome === "opened");
  const previewId = result.previewId;
  const view = h.views[0]!;
  view.webContents.listeners.get("did-finish-load")!();
  // The page's own size at each zoom (innerWidth × zoom is the window's 1280×800).
  const pageAt = (zoom: number) => ({ width: Math.floor(1280 / zoom), height: Math.floor(800 / zoom) });
  const layout = (css: { x: number; y: number; width: number; height: number }, pixelRatio: number, obscured = false, page = pageAt(pixelRatio / 2)) =>
    h.manager.request({ action: "layout", previewId, bounds: css, viewport: "wide", obscured, pixelRatio, page });
  await layout({ x: 700.4, y: 60, width: 400, height: 500 }, 2.5);
  // 700.4 × 1.25 = 875.5 → 876; (700.4 + 400) × 1.25 = 1375.5 → cut at the window's 1280.
  assert.deepEqual(view.bounds, { x: 876, y: 75, width: 404, height: 625 });
  assert.equal(view.visible, true);
  // Zoomed in to 150 % while the page still reports 125 %: hidden, not misplaced.
  h.zoom.factor = 1.5;
  await layout({ x: 700.4, y: 60, width: 400, height: 500 }, 2.5);
  assert.equal(view.visible, false);
  // The right ratio but the page's size from before the zoom: still not used.
  await layout({ x: 500, y: 40, width: 300, height: 400 }, 3, false, pageAt(1.25));
  assert.equal(view.visible, false);
  // The page's fresh measurement at 150 %.
  await layout({ x: 500, y: 40, width: 300, height: 400 }, 3);
  assert.deepEqual(view.bounds, { x: 750, y: 60, width: 450, height: 600 });
  assert.equal(view.visible, true);
  // Zoomed out to 80 %: a stage past the window's width is cut to it.
  h.zoom.factor = 0.8;
  await layout({ x: 1300, y: 60, width: 400, height: 500 }, 1.6);
  assert.deepEqual(view.bounds, { x: 1040, y: 48, width: 240, height: 400 });
  // Covered with a measurement that fits nothing (and a stale ratio): hidden anyway.
  await layout({ x: 5000, y: 5000, width: 400, height: 500 }, 9, true, pageAt(0.8));
  assert.equal(view.visible, false);
  // Uncovered but still unplaceable: stays hidden.
  await layout({ x: 5000, y: 5000, width: 400, height: 500 }, 1.6);
  assert.equal(view.visible, false);
});

test("a resize or zoom hides the view before it happens; only a measurement taken after it shows the view again (PR #9 re-review B1)", async () => {
  const h = harness();
  const { view, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  view.webContents.listeners.get("did-finish-load")!();
  const layout = (page: { width: number; height: number }, bounds = { x: 700, y: 60, width: 500, height: 600 }) =>
    h.manager.request({ action: "layout", previewId, bounds, viewport: "wide", obscured: false, pixelRatio: 1, page });
  await layout({ width: 1280, height: 800 });
  assert.equal(view.visible, true);
  // Main hears of the resize first: hidden at once, with no layout from the page yet.
  h.manager.geometryChanged();
  assert.equal(view.visible, false);
  h.window.size = [1100, 800];
  // A measurement the page took before the resize arrives late: not used.
  await layout({ width: 1280, height: 800 });
  assert.equal(view.visible, false);
  // The page's measurement after the resize: placed, then shown.
  await layout({ width: 1100, height: 800 }, { x: 600, y: 60, width: 480, height: 600 });
  assert.deepEqual(view.bounds, { x: 600, y: 60, width: 480, height: 600 });
  assert.equal(view.visible, true);
  assert.equal(h.manager.visibility.filter(entry => entry.previewId === previewId).map(entry => entry.visible).join(","), "true,false,true");
});

test("a shown view that changes place is hidden first, moved, then shown — never moved in sight (PR #9 re-review B1)", async () => {
  const h = harness();
  const { view, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  view.webContents.listeners.get("did-finish-load")!();
  const page = { width: 1280, height: 800 };
  await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", obscured: false, pixelRatio: 1, page });
  const order: string[] = [];
  const setVisible = view.setVisible.bind(view);
  const setBounds = view.setBounds.bind(view);
  view.setVisible = (visible: boolean) => { order.push(`visible:${visible}`); setVisible(visible); };
  view.setBounds = (bounds: unknown) => { order.push("bounds"); setBounds(bounds); };
  // A band pushed the stage down by 40px.
  await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 100, width: 500, height: 560 }, viewport: "wide", obscured: false, pixelRatio: 1, page });
  assert.deepEqual(order, ["visible:false", "bounds", "visible:true"]);
  // The same place again: nothing moves, nothing flickers.
  order.length = 0;
  await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 100, width: 500, height: 560 }, viewport: "wide", obscured: false, pixelRatio: 1, page });
  assert.deepEqual(order, ["visible:true"]);
});

test("hiding always wins: a cover hides the view before its measurement is even looked at (PR #9 review B1)", async () => {
  const h = harness();
  const { view, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  view.webContents.listeners.get("did-finish-load")!();
  await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", obscured: false, pixelRatio: 1, page: { width: 1280, height: 800 } });
  assert.equal(view.visible, true);
  const order: string[] = [];
  const setVisible = view.setVisible.bind(view);
  const setBounds = view.setBounds.bind(view);
  view.setVisible = (visible: boolean) => { order.push(`visible:${visible}`); setVisible(visible); };
  view.setBounds = (bounds: unknown) => { order.push("bounds"); setBounds(bounds); };
  h.zoom.factor = 0.5;
  const covered = await h.manager.request({ action: "layout", previewId, bounds: { x: 2000, y: 60, width: 500, height: 600 }, viewport: "wide", obscured: true, pixelRatio: 1, page: { width: 1280, height: 800 } });
  assert.equal(covered.action, "layout");
  assert.equal(order[0], "visible:false", "hidden first");
  assert.equal(view.visible, false);
});

test("a new view stays hidden until the page's first layout, also when a cover came up while it opened (PR #9 review I1)", async () => {
  const h = harness();
  const { view, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  assert.notEqual(view.visible, true);
  view.webContents.listeners.get("did-finish-load")!();
  assert.notEqual(view.visible, true, "loading done is not a reason to show");
  // The page's first layout says a dialog opened meanwhile.
  await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", obscured: true, pixelRatio: 1, page: { width: 1280, height: 800 } });
  assert.equal(view.visible, false);
  await h.manager.request({ action: "layout", previewId, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", obscured: false, pixelRatio: 1, page: { width: 1280, height: 800 } });
  assert.equal(view.visible, true);
  // A window un-minimized before any layout of a newer preview does not show it either.
  const next = await opened(h);
  h.manager.setWindowHidden(true);
  h.manager.setWindowHidden(false);
  assert.notEqual(next.view.visible, true);
});

test("each refusal is recorded with its preview, kind and permission (PR #9 review I3)", async () => {
  const h = harness();
  const { session, result } = await opened(h);
  const previewId = result.action === "open" && result.outcome === "opened" ? result.previewId : "";
  session.permissionRequest!({}, "media", () => undefined, { mediaTypes: ["video"] });
  session.permissionRequest!({}, "fullscreen", () => undefined, {});
  session.permissionCheck!({}, "geolocation");
  session.devicePermission!({ deviceType: "usb" });
  session.displayMedia!({}, () => undefined);
  session.listeners.get("select-hid-device")!({ preventDefault() {} }, {}, () => undefined);
  assert.deepEqual(h.manager.denials.map(denial => [denial.previewId === previewId, denial.kind, denial.permission]), [
    [true, "request", "media:video"],
    [true, "request", "fullscreen"],
    [true, "check", "geolocation"],
    [true, "device", "usb"],
    [true, "display", "display-capture"],
    [true, "select", "hid"],
  ]);
});

test("the app's window shortcuts typed in the preview reach the app; nothing else does (PR #9 review minor)", async () => {
  const key = (input: Partial<{ type: string; key: string; meta: boolean; control: boolean; alt: boolean; shift: boolean }>) => ({ type: "keyDown", key: "1", meta: false, control: false, alt: false, shift: false, ...input });
  for (const value of ["1", "2", "3", "\\", "n", "N"]) assert.equal(isAppShortcut(key({ key: value, meta: true }), "darwin"), true, value);
  assert.equal(isAppShortcut(key({ control: true }), "win32"), true);
  assert.equal(isAppShortcut(key({ control: true }), "darwin"), false, "⌘ on macOS");
  assert.equal(isAppShortcut(key({ meta: true }), "win32"), false, "Ctrl elsewhere");
  for (const other of [key({ meta: true, shift: true }), key({ meta: true, alt: true }), key({ meta: true, type: "keyUp" }), key({ meta: true, key: "c" }), key({ key: "1" })]) assert.equal(isAppShortcut(other, "darwin"), false, JSON.stringify(other));
  const h = harness();
  const { view } = await opened(h);
  const send = (input: ReturnType<typeof key>) => { let prevented = false; view.webContents.listeners.get("before-input-event")!({ preventDefault: () => { prevented = true; } }, input); return prevented; };
  const shortcut = process.platform === "darwin" ? key({ key: "2", meta: true }) : key({ key: "2", control: true });
  assert.equal(send(shortcut), true);
  assert.equal(h.window.webContents.focused, 1);
  assert.deepEqual(h.window.webContents.sent, [{ type: "keyDown", keyCode: "2", modifiers: [process.platform === "darwin" ? "meta" : "control"] }]);
  assert.equal(send(key({ key: "a" })), false);
  assert.equal(h.window.webContents.sent.length, 1);
});

test("a build preview serves the task's last build from its output folder, with a single-page app's routes (결과 미리보기 확장 1단계)", async () => {
  const files = { "dist/index.html": Buffer.from("<div id=root></div><script src=/assets/app.js></script>"), "dist/assets/app.js": Buffer.from("ok()"), "index.html": Buffer.from("source") };
  let output: { dir: string; workspaceIdentity: string } | null = null;
  const h = harness({ buildOutput: () => output }, files);
  h.sidecar.previewResolve = async (input: PreviewResolveRequestV1) => {
    h.resolves.push(input);
    return { ...input, worktreeId: "worktree_1", workspaceIdentity: "workspace_1", previewHandle: "ph_dddddddddddddddddddddddddddddddd", entry: input.entry ?? "index.html", refusal: null };
  };
  const openBuild = () => h.manager.request({ action: "open", projectId: "project_1", chatId: "chat_1", entry: null, source: "build", bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", pixelRatio: 1, page: { width: 1280, height: 800 } });
  // Nothing built yet: nothing opens and the engine is not asked.
  assert.deepEqual(await openBuild(), { action: "open", outcome: "empty", reason: "noBuild" });
  assert.deepEqual(h.resolves, []);
  // A build of another workspace (the task's folder was replaced since): not shown.
  output = { dir: "dist", workspaceIdentity: "workspace_old" };
  assert.deepEqual(await openBuild(), { action: "open", outcome: "empty", reason: "noBuild" });
  output = { dir: "dist", workspaceIdentity: "workspace_1" };
  const result = await openBuild();
  assert.ok(result.action === "open" && result.outcome === "opened");
  assert.equal(result.entry, "dist/index.html");
  assert.deepEqual(h.resolves.at(-1), { projectId: "project_1", chatId: "chat_1", entry: "dist/index.html" });
  const session = h.sessions.at(-1)!;
  const token = session.partition.slice("preview-".length);
  // The page loads at its origin's root, where its own paths begin.
  assert.equal(h.views.at(-1)!.webContents.url, `elsecrew-preview://${token}/index.html`);
  const serve = async (path: string) => { const response = await session.handler!(new Request(`elsecrew-preview://${token}${path}`)); return { status: response.status, body: await response.text() }; };
  // The built page's absolute paths are inside the output folder.
  assert.deepEqual(await serve("/assets/app.js"), { status: 200, body: "ok()" });
  assert.equal(h.reads.at(-1)!.path, "dist/assets/app.js");
  // Its own routes are the page (history fallback); the workspace's sources are not reachable.
  for (const route of ["/", "/about", "/users/42/", "/index.html"]) assert.equal((await serve(route)).body, files["dist/index.html"].toString(), route);
  assert.equal((await serve("/missing.js")).status, 404);
  assert.equal(h.reads.at(-1)!.path, "dist/missing.js");
  for (const path of ["/.env", "/assets/.secret.js", "/src/main.tsx"]) assert.equal((await serve(path)).status, 404, path);
});
