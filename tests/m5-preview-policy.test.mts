// Result preview: the pure rules (plan 2.3/2.4/2.6 "TS 순수 함수 단위").
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

import {
  PREVIEW_MIME_V1,
  isPreviewEntryV1,
  previewPathRefusalV1,
  validatePreviewEventV1,
  validatePreviewIpcRequestV1,
  validatePreviewIpcResultV1,
  validatePreviewReadAssetRequestV1,
  validatePreviewReadAssetResultV1,
  validatePreviewResolveRequestV1,
  validatePreviewResolveResultV1,
} from "../src/m5/contracts/preview-v1.ts";
import {
  PREVIEW_CSP,
  PREVIEW_PROXY,
  genericUserAgent,
  isPreviewNavigationAllowed,
  isPreviewRequestAllowed,
  newPreviewId,
  newPreviewToken,
  parsePreviewRequestUrl,
  previewEntryUrl,
  previewPartition,
  previewResponseHeaders,
  pageSizeMatches,
  pixelRatioMatches,
  previewViewRect,
  windowBounds,
} from "../electron/preview-policy.ts";

// A preview host is 32 lowercase hex digits; these two are fixed fakes.
const token = "a".repeat(32);
const other = "b".repeat(32);

test("the path table matches the sidecar's (preview_v1.rs)", () => {
  for (const path of ["index.html", "css/app.css", "assets/Logo.PNG", "a b/c d.js", "fonts/x.woff2", "100%.png"]) {
    assert.equal(previewPathRefusalV1(path), null, path);
  }
  const refused: [string, string][] = [
    ["", "empty"], ["/etc/hosts.txt", "notRelative"], ["C:/x.txt", "notRelative"], ["a\\b.txt", "notRelative"],
    ["a\u0000b.txt", "control"], ["a\nb.txt", "control"], ["../secret.txt", "traversal"], ["a/../../b.txt", "traversal"],
    ["a//b.txt", "traversal"], ["./a.txt", "traversal"], ["a/", "traversal"], ["%2e%2e/a.txt", "encoded"], ["a%2Fb.txt", "encoded"],
    ["a%5cb.txt", "encoded"], ["a%00.txt", "encoded"], [".env", "hidden"], [".git/HEAD", "hidden"], [".git/config", "hidden"],
    ["a/.secret/x.js", "hidden"], [".ssh/id_rsa", "hidden"], ["key.pem", "extension"], ["server.key", "extension"],
    ["prod.env", "extension"], ["app.wasm", "extension"], ["doc.pdf", "extension"], ["Makefile", "extension"], ["noext.", "extension"],
    [`${"a/".repeat(40)}x.txt`, "tooDeep"], [`${"a".repeat(1100)}.txt`, "tooLong"],
  ];
  for (const [path, reason] of refused) assert.equal(previewPathRefusalV1(path), reason, JSON.stringify(path));
  assert.equal(isPreviewEntryV1("docs/Index.HTM"), true);
  assert.equal(isPreviewEntryV1("app.js"), false);
  assert.equal(isPreviewEntryV1(".hidden/index.html"), false);
});

test("content types come from the extension table only", () => {
  assert.equal(PREVIEW_MIME_V1.html, "text/html; charset=utf-8");
  assert.equal(PREVIEW_MIME_V1.js, "text/javascript; charset=utf-8");
  assert.equal(PREVIEW_MIME_V1.svg, "image/svg+xml");
  assert.equal(PREVIEW_MIME_V1.woff2, "font/woff2");
  for (const missing of ["wasm", "pdf", "env", "pem", "key", "exe", "sh"]) assert.equal(Object.hasOwn(PREVIEW_MIME_V1, missing), false, missing);
  assert.equal(previewResponseHeaders("A/B.PNG")["Content-Type"], "image/png");
  assert.equal(previewResponseHeaders(null)["Content-Type"], "text/plain; charset=utf-8");
});

test("every answer carries the fixed header set (snapshot)", () => {
  assert.deepEqual(previewResponseHeaders("index.html"), {
    "Content-Type": "text/html; charset=utf-8",
    "X-Content-Type-Options": "nosniff",
    "Cache-Control": "no-store",
    "Cross-Origin-Opener-Policy": "same-origin",
    "Cross-Origin-Resource-Policy": "same-origin",
    "Referrer-Policy": "no-referrer",
    "Permissions-Policy": "camera=(), microphone=(), geolocation=(), usb=(), serial=(), hid=(), bluetooth=(), payment=(), clipboard-read=(), display-capture=()",
    "Content-Security-Policy": "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self'; worker-src 'self' blob:; frame-src 'self'; child-src 'self' blob:; object-src 'none'; base-uri 'self'; form-action 'none'; manifest-src 'none'; upgrade-insecure-requests",
  });
  assert.doesNotMatch(PREVIEW_CSP, /unsafe-eval|\*|https?:|wss?:/);
});

test("tokens are 128 random bits, partitions are memory-only, ids are not tokens", () => {
  const tokens = new Set(Array.from({ length: 200 }, () => newPreviewToken()));
  assert.equal(tokens.size, 200);
  for (const value of tokens) assert.match(value, /^[0-9a-f]{32}$/);
  assert.equal(previewPartition(token), `preview-${token}`);
  assert.doesNotMatch(previewPartition(token), /^persist:/);
  assert.throws(() => previewPartition("persist:x"));
  assert.throws(() => previewPartition("A".repeat(32)));
  const id = newPreviewId();
  assert.match(id, /^pv_[0-9a-f]{32}$/);
  assert.equal(previewEntryUrl(token, "docs/my page.html"), `elsecrew-preview://${token}/docs/my%20page.html`);
  assert.throws(() => previewEntryUrl(token, "../x.html"));
});

test("the protocol handler reads only this token's origin and checks paths before decoding (T4·T15)", () => {
  const ok = (url: string, path: string) => assert.deepEqual(parsePreviewRequestUrl(url, token, "index.html"), { ok: true, path }, url);
  const no = (url: string, reason?: string) => {
    const decision = parsePreviewRequestUrl(url, token, "index.html");
    assert.equal(decision.ok, false, url);
    if (reason && !decision.ok) assert.equal(decision.reason, reason, url);
  };
  ok(`elsecrew-preview://${token}/`, "index.html");
  ok(`elsecrew-preview://${token}`, "index.html");
  ok(`elsecrew-preview://${token}/css/app.css?v=2#x`, "css/app.css");
  ok(`elsecrew-preview://${token}/my%20page.html`, "my page.html");
  // Chromium lowercases a standard scheme's host before the handler sees it;
  // anything else is not this token.
  no(`elsecrew-preview://${token.toUpperCase()}/index.html`, "origin");
  // WHATWG URL resolves dot segments inside the origin; it never leaves it.
  ok(`elsecrew-preview://${token}/a/../index.html`, "index.html");
  no(`elsecrew-preview://${other}/index.html`, "origin");
  no(`elsecrew-preview://${token}:8080/index.html`, "origin");
  no(`elsecrew-preview://a:b@${token}/index.html`, "origin");
  no(`app://${token}/index.html`, "scheme");
  no(`https://${token}/index.html`, "scheme");
  no(`elsecrew-preview://${token}/%2e%2e/secret.txt`, "encoded");
  no(`elsecrew-preview://${token}/a%2f..%2fb.txt`, "encoded");
  no(`elsecrew-preview://${token}/a%5cb.txt`, "encoded");
  no(`elsecrew-preview://${token}/x%00.txt`, "encoded");
  no(`elsecrew-preview://${token}/.env`, "hidden");
  no(`elsecrew-preview://${token}/.git/HEAD`, "hidden");
  no(`elsecrew-preview://${token}/a/.secret/x.js`, "hidden");
  no(`elsecrew-preview://${token}/key.pem`, "extension");
  no(`elsecrew-preview://${token}/%E0%A4%A.txt`, "decode");
  assert.equal(parsePreviewRequestUrl(`elsecrew-preview://${token}/x.html`, "short", "index.html").ok, false);
});

test("webRequest allows the preview's own origin only (T1·T14·T15)", () => {
  for (const url of [`elsecrew-preview://${token}/a.js`, "data:image/png;base64,AA==", `blob:elsecrew-preview://${token}/4b1e`]) {
    assert.equal(isPreviewRequestAllowed(url, token), true, url);
  }
  for (const url of [
    "https://example.com/", "http://127.0.0.1:8080/", "ws://127.0.0.1:1/", "wss://example.com/", "file:///etc/hosts",
    "app://m5/index.html", `elsecrew-preview://${other}/a.js`, `blob:https://example.com/4b1e`, `blob:elsecrew-preview://${other}/x`,
    "ftp://example.com/", "chrome://gpu", "devtools://devtools/bundled/inspector.html", "about:blank",
    `elsecrew-preview://${token}:1/a.js`,
  ]) assert.equal(isPreviewRequestAllowed(url, token), false, url);
});

test("navigation stays inside the origin; a subframe may be empty (T9)", () => {
  assert.equal(isPreviewNavigationAllowed(`elsecrew-preview://${token}/b.html`, token, true), true);
  assert.equal(isPreviewNavigationAllowed("about:blank", token, false), true);
  assert.equal(isPreviewNavigationAllowed("about:srcdoc", token, false), true);
  for (const url of ["about:blank", "https://example.com/", "http://127.0.0.1/", `elsecrew-preview://${other}/`, "data:text/html,x", "javascript:alert(1)", "file:///"]) {
    assert.equal(isPreviewNavigationAllowed(url, token, true), false, url);
  }
  assert.equal(isPreviewNavigationAllowed("https://example.com/", token, false), false);
});

test("the dead proxy also catches loopback; the user agent drops product tokens", () => {
  assert.deepEqual({ ...PREVIEW_PROXY }, { mode: "fixed_servers", proxyRules: "127.0.0.1:9", proxyBypassRules: "<-loopback>" });
  assert.equal(
    genericUserAgent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) elsecrew/0.1.2 Chrome/140.0.0.0 Electron/43.4.0 Safari/537.36"),
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
  );
});

test("bounds: the page's CSS pixels times the zoom, rounded inwards, cut to the window, at least 120×120; narrow is a centered 390px column (PR #9 review B1)", () => {
  const content = { width: 1280, height: 800 };
  assert.deepEqual(windowBounds({ x: 700.4, y: 60.6, width: 579.6, height: 700 }, 1, content), { x: 701, y: 61, width: 579, height: 699 });
  // Running past the window: cut at its edge, never an error.
  assert.deepEqual(windowBounds({ x: 700, y: 0, width: 581, height: 800 }, 1, content), { x: 700, y: 0, width: 580, height: 800 });
  // Zoomed in: the stage is further right and larger in window pixels.
  assert.deepEqual(windowBounds({ x: 400, y: 50, width: 300, height: 400 }, 1.25, content), { x: 500, y: 63, width: 375, height: 499 });
  // Zoomed out: smaller, and a stage measured past the window is cut to it.
  assert.deepEqual(windowBounds({ x: 1300, y: 50, width: 300, height: 400 }, 0.8, content), { x: 1040, y: 40, width: 240, height: 320 });
  // Inside the stage on every side: never left of or above what was measured.
  for (const zoom of [0.5, 0.67, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 3]) {
    const css = { x: 333.3, y: 77.7, width: 401.9, height: 455.5 };
    const placed = windowBounds(css, zoom, { width: 4000, height: 4000 })!;
    assert.ok(placed.x >= css.x * zoom && placed.y >= css.y * zoom, `${zoom}`);
    assert.ok(placed.x + placed.width <= (css.x + css.width) * zoom && placed.y + placed.height <= (css.y + css.height) * zoom, `${zoom}`);
  }
  for (const [bad, zoom] of [
    [{ x: 0, y: 0, width: 119, height: 300 }, 1], [{ x: 0, y: 0, width: 300, height: 100 }, 1],
    [{ x: 1200, y: 0, width: 300, height: 300 }, 1], [{ x: 0, y: 700, width: 300, height: 300 }, 1],
    [{ x: Number.NaN, y: 0, width: 300, height: 300 }, 1], [{ x: 0, y: 0, width: Number.POSITIVE_INFINITY, height: 300 }, 1],
    [{ x: 0, y: 0, width: 300, height: 300 }, 0], [{ x: 0, y: 0, width: 300, height: 300 }, Number.NaN], [{ x: 0, y: 0, width: 200, height: 300 }, 0.5],
  ] as const) assert.equal(windowBounds(bad, zoom, content), null, JSON.stringify([bad, zoom]));
  // A measurement from another zoom (or display) is not used.
  assert.equal(pixelRatioMatches(2, 1, 2), true);
  assert.equal(pixelRatioMatches(2.5, 1.25, 2), true);
  assert.equal(pixelRatioMatches(2.5, 1.5, 2), false);
  assert.equal(pixelRatioMatches(1, 1, 2), false, "measured on a standard display, now on a Retina one");
  assert.equal(pixelRatioMatches(Number.NaN, 1, 1), false);
  // …nor one taken at another window size (PR #9 re-review B1).
  assert.equal(pageSizeMatches({ width: 1280, height: 800 }, 1, content), true);
  assert.equal(pageSizeMatches({ width: 853, height: 533 }, 1.5, content), true, "innerWidth rounds down at a fractional zoom");
  assert.equal(pageSizeMatches({ width: 1600, height: 1000 }, 0.8, content), true);
  assert.equal(pageSizeMatches({ width: 1280, height: 800 }, 1.25, content), false, "measured before zooming in");
  assert.equal(pageSizeMatches({ width: 1100, height: 800 }, 1, content), false, "measured before the window grew");
  assert.equal(pageSizeMatches({ width: Number.NaN, height: 800 }, 1, content), false);
  const bounds = { x: 700, y: 60, width: 500, height: 600 };
  assert.deepEqual(previewViewRect(bounds, "wide"), bounds);
  assert.deepEqual(previewViewRect(bounds, "narrow"), { x: 755, y: 60, width: 390, height: 600 });
  assert.deepEqual(previewViewRect({ ...bounds, width: 300 }, "narrow"), { ...bounds, width: 300 });
});

test("sidecar contract: requests are exact, answers must echo them and match their size", () => {
  assert.throws(() => validatePreviewResolveRequestV1({ projectId: "p", chatId: "c" }));
  assert.throws(() => validatePreviewResolveRequestV1({ projectId: "p", chatId: "c", entry: "x.js" }));
  assert.throws(() => validatePreviewResolveRequestV1({ projectId: "p", chatId: "c", entry: null, worktreeId: "w" }));
  const resolveRequest = validatePreviewResolveRequestV1({ projectId: "p", chatId: "c", entry: null });
  const resolved = { chatId: "c", projectId: "p", worktreeId: "w", workspaceIdentity: "i", previewHandle: "ph_cccccccccccccccccccccccccccccccc", entry: "index.html", refusal: null };
  assert.throws(() => validatePreviewResolveResultV1({ ...resolved, previewHandle: "x" }, resolveRequest));
  assert.deepEqual(validatePreviewResolveResultV1(resolved, resolveRequest), resolved);
  assert.throws(() => validatePreviewResolveResultV1({ ...resolved, chatId: "other" }, resolveRequest));
  assert.throws(() => validatePreviewResolveResultV1({ ...resolved, entry: "../x.html" }, resolveRequest));
  assert.throws(() => validatePreviewResolveResultV1({ ...resolved, entry: null, refusal: "hidden" }, resolveRequest));
  const named = validatePreviewResolveRequestV1({ projectId: "p", chatId: "c", entry: "docs/a.html" });
  assert.throws(() => validatePreviewResolveResultV1({ ...resolved, entry: "index.html" }, named));
  assert.equal(validatePreviewResolveResultV1({ ...resolved, entry: null, refusal: "notFound" }, named).refusal, "notFound");

  const read = validatePreviewReadAssetRequestV1({ chatId: "c", projectId: "p", worktreeId: "w", workspaceIdentity: "i", previewHandle: "ph_cccccccccccccccccccccccccccccccc", path: "a.js", offset: 0 });
  assert.throws(() => validatePreviewReadAssetRequestV1({ ...read, previewHandle: undefined }));
  assert.throws(() => validatePreviewReadAssetRequestV1({ ...read, offset: -1 }));
  assert.throws(() => validatePreviewReadAssetRequestV1({ ...read, offset: 5 * 1024 * 1024 }));
  const version = `sha256:${"a".repeat(64)}`;
  // The sidecar echoes the address and path, not the handle.
  const { previewHandle: _handle, ...echo } = read;
  const ok = { ...echo, outcome: "ok", byteLength: 3, version, bytesBase64: Buffer.from("abc").toString("base64") };
  assert.equal(validatePreviewReadAssetResultV1(ok, read).bytesBase64, "YWJj");
  assert.throws(() => validatePreviewReadAssetResultV1({ ...ok, bytesBase64: Buffer.from("ab").toString("base64") }, read));
  assert.throws(() => validatePreviewReadAssetResultV1({ ...ok, path: "b.js" }, read));
  assert.throws(() => validatePreviewReadAssetResultV1({ ...ok, workspaceIdentity: "j" }, read));
  assert.throws(() => validatePreviewReadAssetResultV1({ ...ok, byteLength: 6 * 1024 * 1024 }, read));
  assert.throws(() => validatePreviewReadAssetResultV1({ ...ok, outcome: "notFound" }, read));
  const missing = { ...echo, outcome: "notFound", byteLength: null, version: null, bytesBase64: null };
  assert.equal(validatePreviewReadAssetResultV1(missing, read).outcome, "notFound");
  const big = 600 * 1024;
  const first = { ...ok, byteLength: big, bytesBase64: Buffer.alloc(512 * 1024).toString("base64") };
  assert.equal(validatePreviewReadAssetResultV1(first, read).byteLength, big);
  const second = validatePreviewReadAssetRequestV1({ ...read, offset: 512 * 1024 });
  assert.equal(validatePreviewReadAssetResultV1({ ...first, offset: 512 * 1024, bytesBase64: Buffer.alloc(big - 512 * 1024).toString("base64") }, second).offset, 512 * 1024);
});

test("renderer contract: exact actions and answers", () => {
  const open = { action: "open", projectId: "p", chatId: "c", entry: null, bounds: { x: 1, y: 2, width: 300, height: 300 }, viewport: "narrow", pixelRatio: 2, page: { width: 1280, height: 800 } } as const;
  assert.deepEqual(validatePreviewIpcRequestV1(open), open);
  // The page's devicePixelRatio comes with every measurement (PR #9 review B1).
  const { pixelRatio: _ratio, ...unmeasured } = open;
  assert.throws(() => validatePreviewIpcRequestV1(unmeasured), /unexpected fields/);
  for (const bad of [0, -1, 0.1, 17, Number.NaN, Number.POSITIVE_INFINITY, "2"]) assert.throws(() => validatePreviewIpcRequestV1({ ...open, pixelRatio: bad }), /pixelRatio/, String(bad));
  // …and the page's own size, so main can refuse a measurement from before a resize.
  const { page: _page, ...sizeless } = open;
  assert.throws(() => validatePreviewIpcRequestV1(sizeless), /unexpected fields/);
  for (const bad of [{ width: 0, height: 800 }, { width: 1280 }, { width: 1280, height: 800, x: 1 }, { width: "1280", height: 800 }, { width: 1280, height: Number.NaN }]) assert.throws(() => validatePreviewIpcRequestV1({ ...open, page: bad }), JSON.stringify(bad));
  assert.throws(() => validatePreviewIpcRequestV1({ ...open, action: "navigate" }));
  assert.throws(() => validatePreviewIpcRequestV1({ action: "close", previewId: "a".repeat(32) }));
  const previewId = `pv_${"b".repeat(32)}`;
  const layout = validatePreviewIpcRequestV1({ action: "layout", previewId, bounds: open.bounds, viewport: "wide", obscured: true, pixelRatio: 1.25, page: { width: 1024, height: 640 } });
  assert.throws(() => validatePreviewIpcRequestV1({ action: "layout", previewId, bounds: open.bounds, viewport: "wide", obscured: true, pixelRatio: 1.25 }));
  assert.equal(validatePreviewIpcResultV1({ action: "layout", previewId, state: "hidden", snapshot: "data:image/jpeg;base64,AAAA" }, layout).state, "hidden");
  assert.throws(() => validatePreviewIpcResultV1({ action: "layout", previewId, state: "hidden", snapshot: "https://x/y.png" }, layout));
  const opened = validatePreviewIpcRequestV1(open);
  assert.throws(() => validatePreviewIpcResultV1({ action: "open", outcome: "opened", previewId, entry: "index.html", state: "loading", token: "x" }, opened));
  assert.equal(validatePreviewIpcResultV1({ action: "open", outcome: "empty", reason: "noEntry" }, opened).action, "open");
  assert.deepEqual(validatePreviewEventV1({ previewId, kind: "closed", reason: "crashed" }), { previewId, kind: "closed", reason: "crashed" });
  assert.throws(() => validatePreviewEventV1({ previewId, kind: "closed", reason: "other" }));
  assert.throws(() => validatePreviewEventV1({ previewId, kind: "loaded", url: "x" }));
});

test("the preview's real-app checks say they are macOS only and start without shell-specific syntax (PR #9 review I5)", () => {
  const scripts = JSON.parse(readFileSync("package.json", "utf8")).scripts as Record<string, string>;
  assert.equal(scripts["m5:test:preview-ui"], "node scripts/m5-preview-ui-run.mjs");
  for (const name of ["m5:test:preview-ui", "m5:test:preview-isolation"]) assert.doesNotMatch(scripts[name]!, /(?:^|\s)[A-Z_]+=\S/, `${name} sets no variable the POSIX way`);
  for (const file of ["scripts/m5-preview-ui-run.mjs", "scripts/m5-preview-ui-smoke.cjs", "scripts/m5-preview-isolation-probe.mjs"]) {
    const source = readFileSync(file, "utf8");
    assert.match(source, /process\.platform !== ["']darwin["']\) \{[\s\S]{0,200}NOT_RUN[\s\S]{0,240}process\.exit\(2\)/, `${file}: not run (exit 2) off macOS`);
  }
  // Exit 2 is NOT_READY in the harness, never a pass.
  assert.match(readFileSync("scripts/m5-harness.mjs", "utf8"), /result\.status === 2 && !result\.error\) \{\n\s+report\.status = "NOT_READY";/);
  const check = readFileSync("scripts/m5-preview-default-check.mjs", "utf8");
  assert.match(check, /Elsecrew-win32-\$\{process\.arch\}/);
  assert.match(check, /const onByDefault = process\.platform === "darwin";/);
  assert.match(readFileSync(".github/workflows/ci.yml", "utf8"), /m5:test:preview-ui[\s\S]{0,400}CI에서[\s\S]{0,40}돌리지 않는다/);
});
