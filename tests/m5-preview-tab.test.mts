// Result preview tab (P3): the screen-side rules for T8 and T16.
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

import { closedView, coveredAtPoints, emptyView, isPreviewablePage, overlaps, previewBounds, sameLayout, samplePoints } from "../src/m5/preview-tab-model.ts";

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
  assert.equal(points.length, 25);
  for (const point of points) assert.ok(point.x > stage.left && point.x < stage.left + stage.width && point.y > stage.top && point.y < stage.top + stage.height);
  const STAGE = Symbol("stage");
  const isStage = (element: unknown) => element === STAGE;
  assert.equal(coveredAtPoints(points, () => STAGE, isStage), false);
  // A notification panel over the stage's top-right corner.
  const panel = { left: 380, top: 90, width: 200, height: 120 };
  const top = (x: number, y: number) => x >= panel.left && x <= panel.left + panel.width && y >= panel.top && y <= panel.top + panel.height ? "panel" : STAGE;
  assert.equal(coveredAtPoints(points, top, isStage), true);
  // A toast in the bottom-left corner only.
  assert.equal(coveredAtPoints(points, (x, y) => x < 140 && y > 360 ? "toast" : STAGE, isStage), true);
  // Nothing at a point (off the page) is not a cover.
  assert.equal(coveredAtPoints(points, () => null, isStage), false);
});

test("closed and empty states say what happened; files offer only pages", () => {
  assert.equal(closedView("crashed"), "stalled");
  assert.equal(closedView("workspaceUnavailable"), "workspaceGone");
  assert.equal(closedView("limit"), "tooMuch");
  for (const reason of ["requested", "idle", "replaced", "quitting", "windowClosed", "reloaded"] as const) assert.equal(closedView(reason), null, reason);
  assert.equal(emptyView("noEntry"), "noPage");
  assert.equal(emptyView("notFound"), "notOpenable");
  assert.equal(emptyView("hidden"), "notOpenable");
  for (const page of ["index.html", "docs/About.HTM", "a/b/c.html"]) assert.equal(isPreviewablePage(page), true, page);
  for (const other of [null, undefined, "", "app.js", "README.md", ".hidden/index.html", "a/.git/x.html", "index.html.bak"]) assert.equal(isPreviewablePage(other), false, String(other));
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
