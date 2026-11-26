// Result preview tab (P3): the screen-side rules for T8 and T16.
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

import { closedView, coveredAtPoints, coveredByRect, emptyView, hasPreviewPage, intersection, isPreviewablePage, overlaps, PREVIEW_WAIT_ATTRIBUTE, previewBounds, sameLayout, samplePoints } from "../src/m5/preview-tab-model.ts";

const viewport = { width: 1280, height: 800 };

test("the view is the stage, never more: it never reaches the app's frame above it (T8)", () => {
  const frame = { left: 880, top: 96, width: 392, height: 40 };
  const stage = { left: 880.4, top: 144.6, width: 391.2, height: 560.5 };
  const bounds = previewBounds(stage, viewport)!;
  assert.deepEqual(bounds, { x: 881, y: 145, width: 390, height: 560 });
  // Inside the stage on every side (rounded inwards), so it cannot cover the frame.
  assert.ok(bounds.x >= stage.left && bounds.y >= stage.top);
  assert.ok(bounds.x + bounds.width <= stage.left + stage.width && bounds.y + bounds.height <= stage.top + stage.height);
  assert.equal(overlaps({ left: bounds.x, top: bounds.y, width: bounds.width, height: bounds.height }, frame), false);
  // A stage running past the window is cut at its edge.
  assert.deepEqual(previewBounds({ left: 900, top: 600, width: 500, height: 400 }, viewport), { x: 900, y: 600, width: 380, height: 200 });
  // Too small, collapsed or off screen: no view at all.
  for (const tiny of [{ left: 0, top: 0, width: 119, height: 500 }, { left: 0, top: 0, width: 500, height: 0 }, { left: 1270, top: 0, width: 300, height: 300 }, { left: Number.NaN, top: 0, width: 300, height: 300 }]) {
    assert.equal(previewBounds(tiny, viewport), null, JSON.stringify(tiny));
  }
});

test("the frame is the app's and sits outside the stage in the tab's markup (T8)", () => {
  const source = readFileSync("src/m5/preview-tab.tsx", "utf8");
  const frame = source.indexOf('data-testid="preview-frame"');
  const stage = source.indexOf('data-testid="preview-stage"');
  const frameEnd = source.indexOf("</header>", frame);
  assert.ok(frame > 0 && stage > 0 && frameEnd > frame && frameEnd < stage, "the frame closes before the stage begins");
  for (const label of ["m5.preview.frameTitle", "m5.preview.readOnly", "m5.preview.offline"]) {
    const at = source.indexOf(label);
    assert.ok(at > frame && at < frameEnd, `${label} is in the frame`);
  }
  // The view's rectangle comes from the stage only.
  assert.match(source, /previewBounds\(stage\.current\.getBoundingClientRect\(\)/);
});

test("anything of the app on top of the stage covers it (T16)", () => {
  const stage = { left: 100, top: 100, width: 400, height: 300 };
  const points = samplePoints(stage);
  assert.equal(points.length, 49);
  for (const point of points) assert.ok(point.x > stage.left && point.x < stage.left + stage.width && point.y > stage.top && point.y < stage.top + stage.height);
  // Down to 2px from every edge (it was 8px: review I1).
  assert.equal(Math.min(...points.map(point => point.x)), stage.left + 2);
  assert.equal(Math.max(...points.map(point => point.y)), stage.top + stage.height - 2);
  const STAGE = Symbol("stage");
  const HOLDER = Symbol("what holds the stage");
  const isStage = (element: unknown) => element === STAGE || element === HOLDER;
  assert.equal(coveredAtPoints(points, () => STAGE, isStage), false);
  // A rounded corner finds what holds the stage: not a cover.
  assert.equal(coveredAtPoints(points, (x, y) => x < 103 && y < 103 ? HOLDER : STAGE, isStage), false);
  // A notification panel over the stage's top-right corner.
  const panel = { left: 380, top: 90, width: 200, height: 120 };
  const top = (x: number, y: number) => x >= panel.left && x <= panel.left + panel.width && y >= panel.top && y <= panel.top + panel.height ? "panel" : STAGE;
  assert.equal(coveredAtPoints(points, top, isStage), true);
  // A toast in the bottom-left corner only.
  assert.equal(coveredAtPoints(points, (x, y) => x < 140 && y > 360 ? "toast" : STAGE, isStage), true);
  // Nothing at a point (off the page) is not a cover.
  assert.equal(coveredAtPoints(points, () => null, isStage), false);
});

test("a cover over only the stage's edge or between the points is found by its rectangle (PR #9 review I1)", () => {
  const stage = { left: 100, top: 100, width: 400, height: 300 };
  const points = samplePoints(stage);
  // A notification card reaching 1.5px into the stage's right edge: no grid point sees it.
  const card = { left: 498.5, top: 120, width: 320, height: 80 };
  const ownsCard = (element: unknown) => element === "card";
  const topWithCard = (x: number, y: number) => x >= card.left && x <= card.left + card.width && y >= card.top && y <= card.top + card.height ? "card" : "stage";
  assert.equal(coveredAtPoints(points, topWithCard, element => element === "stage"), false, "the grid misses it");
  assert.equal(coveredByRect(stage, card, topWithCard, ownsCard), true, "its rectangle does not");
  assert.deepEqual(intersection(stage, card), { left: 498.5, top: 120, width: 1.5, height: 80 });
  // A thin popover between two columns of points.
  const sliver = { left: 160, top: 150, width: 4, height: 40 };
  const topWithSliver = (x: number, y: number) => x >= sliver.left && x <= sliver.left + sliver.width && y >= sliver.top && y <= sliver.top + sliver.height ? "sliver" : "stage";
  assert.equal(coveredByRect(stage, sliver, topWithSliver, element => element === "sliver"), true);
  // Beside the stage, or under it (the stage is on top there): not a cover.
  assert.equal(coveredByRect(stage, { left: 500, top: 120, width: 300, height: 80 }, topWithCard, ownsCard), false);
  assert.equal(coveredByRect(stage, card, () => "stage", ownsCard), false);
  assert.equal(intersection(stage, { left: 0, top: 0, width: 100, height: 100 }), null, "touching corners share nothing");
});

test("the view is hidden first, then the cover is shown (PR #9 review I1, re-review)", () => {
  const source = readFileSync("src/m5/preview-tab.tsx", "utf8");
  // New or changed app elements over the stage are held before the page paints…
  assert.match(source, /new MutationObserver\(records => \{/);
  // …invisible AND out of reach: no focus, no input (inert), focus given back after.
  assert.match(source, /element\.setAttribute\(PREVIEW_WAIT_ATTRIBUTE, ""\);\n\s+if \(!wasInert\) element\.setAttribute\("inert", ""\);\n\s+loop\.hold\(/);
  assert.match(source, /focused\.focus\(\)/);
  // The order itself (hide first, hides never queued, let go only on main's answer,
  // detach at the limit) is PreviewLayoutLoop's: tests/m5-preview-layout-loop.test.mts.
  assert.match(source, /new PreviewLayoutLoop\(\{/);
  assert.match(source, /close: \(\) => bridge\.close\(\{ previewId \}\)/);
  // Bands that move the stage appear only once the view is hidden (re-review B1).
  for (const change of ["setNotice(true)", "setNotice(false)", "setNewResult(true)", "setNewResult(false)"]) assert.ok(source.includes(`shift(() => ${change})`), change);
  assert.match(source, /useLayoutEffect\(\(\) => \{ bands\.current = \{ newResult, notice \}; loopRef\.current\?\.settled\(\); \}, \[newResult, notice\]\);/);
  // A band change that would change nothing hides nothing (ADE V check, 2026-11-26).
  assert.match(source, /if \(bands\.current\.newResult\) shift\(\(\) => setNewResult\(false\)\)/);
  assert.match(source, /!bands\.current\.notice\) shift\(\(\) => setNotice\(true\)\)/);
  // Something in the tab that moved the stage is held the same way.
  assert.match(source, /if \(own\?\.contains\(node\)\) \{\n\s+if \(moved\) \{ holdElement\(node\);/);
  // Every measurement carries the page's devicePixelRatio and size.
  assert.match(source, /pixelRatio: window\.devicePixelRatio, page: \{ width: window\.innerWidth, height: window\.innerHeight \}/);
  assert.match(source, /return \{ bounds: rect \? previewBounds\(rect, page\) : null, pixelRatio: window\.devicePixelRatio, page \};/);
  const css = readFileSync("src/m5/styles.css", "utf8");
  assert.ok(css.includes(`[${PREVIEW_WAIT_ATTRIBUTE}] { opacity: 0 !important; }`));
  assert.ok(css.includes(`[${PREVIEW_WAIT_ATTRIBUTE}]::backdrop { opacity: 0 !important; background: transparent !important; backdrop-filter: none !important; }`), "the modal backdrop too");
});

test("closed and empty states say what happened; files offer only pages", () => {
  assert.equal(closedView("crashed"), "stalled");
  assert.equal(closedView("workspaceUnavailable"), "workspaceGone");
  assert.equal(closedView("limit"), "tooMuch");
  // Quitting may be called off (an update that failed to prepare, review I4): the tab waits and reopens.
  assert.equal(closedView("quitting"), "suspended");
  for (const reason of ["requested", "idle", "replaced", "windowClosed", "reloaded"] as const) assert.equal(closedView(reason), null, reason);
  assert.equal(emptyView("noEntry"), "noPage");
  assert.equal(emptyView("notFound"), "notOpenable");
  assert.equal(emptyView("hidden"), "notOpenable");
  for (const page of ["index.html", "docs/About.HTM", "a/b/c.html"]) assert.equal(isPreviewablePage(page), true, page);
  for (const other of [null, undefined, "", "app.js", "README.md", ".hidden/index.html", "a/.git/x.html", "index.html.bak"]) assert.equal(isPreviewablePage(other), false, String(other));
  // A task whose changes include a page (not one it deleted) opens on the preview (review minor).
  assert.equal(hasPreviewPage([{ kind: "modified", path: "src/app.js" }, { kind: "added", path: "index.html" }]), true);
  assert.equal(hasPreviewPage([{ kind: "deleted", path: "index.html" }, { kind: "modified", path: "README.md" }]), false);
  assert.equal(hasPreviewPage(null), false);
});

test("the tab copes with a page that failed to load and with a quit that was called off (review minor, I4)", () => {
  const source = readFileSync("src/m5/preview-tab.tsx", "utf8");
  assert.match(source, /event\.kind === "loadFailed"\) \{ setStill\(null\); setPhase\(\{ kind: "failed" \}\); \}/);
  // While suspended it asks main every second and opens again once allowed.
  assert.match(source, /const suspended = phase\.kind === "closed" && phase\.view === "suspended";/);
  assert.match(source, /if \(!live \|\| !status\.enabled\) return;[\s\S]{0,120}setOpenRequest\(value => value \+ 1\);/);
  assert.match(source, /data-testid="preview-suspended"/);
  // The workbench passes whether the task has a page to the default tab rule.
  const workbench = readFileSync("src/m5/workbench.tsx", "utf8");
  assert.equal(workbench.includes("defaultRightTab(activePhase, false)"), false);
  assert.match(workbench, /hasPreviewPage\(snapshot\.diffSummary\?\.entries\)/);
});

test("a layout is sent again only when something changed", () => {
  const base = { bounds: { x: 1, y: 2, width: 300, height: 300 }, viewport: "wide" as const, obscured: false };
  assert.equal(sameLayout(null, base), false);
  assert.equal(sameLayout(base, { ...base, bounds: { ...base.bounds } }), true);
  assert.equal(sameLayout(base, { ...base, obscured: true }), false);
  assert.equal(sameLayout(base, { ...base, viewport: "narrow" }), false);
  assert.equal(sameLayout(base, { ...base, bounds: { ...base.bounds, height: 301 } }), false);
});

test("the tab hides the view for every cover (T16)", () => {
  const source = readFileSync("src/m5/preview-tab.tsx", "utf8");
  assert.match(source, /const obscured = covered \|\| overlay \|\| !active;/);
  // The explicit covers still include sheets, drawers and dialogs.
  const layout = readFileSync("src/m5/task-layout-model.ts", "utf8");
  assert.match(layout, /return causes\.sheetOpen \|\| causes\.dialogOpen \|\| causes\.popoverOpen;/);
});
