/**
 * Result preview tab (ADE 1차-나 P3): the pure rules of the screen side.
 * The view itself is a separate WebContentsView drawn above the app page
 * (`electron/preview-manager.ts`); this file decides where it goes, when it
 * must hide and what the tab says. Tested in `tests/m5-preview-tab.test.mts`.
 */
import type { PreviewBoundsV1, PreviewCloseReasonV1, PreviewEmptyReasonV1 } from "./contracts/preview-v1.ts";

export const PREVIEW_MIN_SIZE = 120;

export interface Rect { left: number; top: number; width: number; height: number }

/**
 * The view's rectangle: the stage, never more (T8). The app's own frame
 * (title, "읽기 전용", "인터넷 차단") sits outside the stage, so a page can
 * never paint over it. Too small or not on screen: no rectangle.
 */
export function previewBounds(stage: Rect, viewport: { width: number; height: number }): PreviewBoundsV1 | null {
  const left = Math.max(0, Math.ceil(stage.left));
  const top = Math.max(0, Math.ceil(stage.top));
  const right = Math.min(viewport.width, Math.floor(stage.left + stage.width));
  const bottom = Math.min(viewport.height, Math.floor(stage.top + stage.height));
  const width = right - left;
  const height = bottom - top;
  if (!Number.isFinite(width) || !Number.isFinite(height) || width < PREVIEW_MIN_SIZE || height < PREVIEW_MIN_SIZE) return null;
  return { x: left, y: top, width, height };
}

/** True when the two rectangles share any area. */
export function overlaps(a: Rect, b: Rect): boolean {
  return a.left < b.left + b.width && b.left < a.left + a.width && a.top < b.top + b.height && b.top < a.top + a.height;
}

/** A grid of points inside the stage, from just inside its edges, to ask
 * what is drawn on top. A point on a rounded corner finds what holds the
 * stage, which counts as the stage (`coveredAtPoints`). Something thinner
 * than the gaps is found by its own rectangle (`coveredByRect`). */
export function samplePoints(stage: Rect, steps = 6): { x: number; y: number }[] {
  const inset = Math.min(2, stage.width / 4, stage.height / 4);
  const points: { x: number; y: number }[] = [];
  for (let row = 0; row <= steps; row++) {
    for (let column = 0; column <= steps; column++) {
      points.push({
        x: stage.left + inset + (stage.width - 2 * inset) * column / steps,
        y: stage.top + inset + (stage.height - 2 * inset) * row / steps,
      });
    }
  }
  return points;
}

/**
 * Whether anything of the app is drawn over the stage (T16). The view sits
 * above the page, so it would hide a dialog, a sheet, a drawer, a menu or a
 * toast instead of being hidden by it. Besides the explicit list of covers
 * (task-layout-model `previewObscured`), this asks the page what is on top
 * at a grid of points: anything that is neither the stage (with what is in
 * it) nor what holds it covers it.
 */
export function coveredAtPoints(points: readonly { x: number; y: number }[], topAt: (x: number, y: number) => unknown, isStage: (element: unknown) => boolean): boolean {
  return points.some(point => {
    const element = topAt(point.x, point.y);
    return element !== null && element !== undefined && !isStage(element);
  });
}

/** The area two rectangles share, or `null`. */
export function intersection(a: Rect, b: Rect): Rect | null {
  const left = Math.max(a.left, b.left);
  const top = Math.max(a.top, b.top);
  const right = Math.min(a.left + a.width, b.left + b.width);
  const bottom = Math.min(a.top + a.height, b.top + b.height);
  return right > left && bottom > top ? { left, top, width: right - left, height: bottom - top } : null;
}

/**
 * An element of the app that appeared or changed (a notification panel, a
 * toast, a popover) covers the stage when it shares any area with it — even
 * a sliver at the edge or between the grid points (PR #9 review I1) — and
 * the page's hit test finds it on top somewhere in that shared area.
 */
export function coveredByRect(stage: Rect, cover: Rect, topAt: (x: number, y: number) => unknown, ownsElement: (element: unknown) => boolean): boolean {
  const shared = intersection(stage, cover);
  if (!shared) return false;
  const inset = Math.min(0.5, shared.width / 2, shared.height / 2);
  const xs = [shared.left + inset, shared.left + shared.width / 2, shared.left + shared.width - inset];
  const ys = [shared.top + inset, shared.top + shared.height / 2, shared.top + shared.height - inset];
  return ys.some(y => xs.some(x => ownsElement(topAt(x, y))));
}

/** Marks an app element held invisible until main has hidden the view
 * (PR #9 review I1: the view is hidden first, then the cover is shown). */
export const PREVIEW_WAIT_ATTRIBUTE = "data-preview-wait";

/** What the tab shows for a closed preview; `null`: nothing to say (it simply reopens). */
export type PreviewClosedView = "stalled" | "workspaceGone" | "tooMuch" | "suspended" | null;
export function closedView(reason: PreviewCloseReasonV1): PreviewClosedView {
  if (reason === "crashed") return "stalled";
  // The app began to quit (an update, say) and may yet stay (review I4):
  // the tab waits and opens again when main allows it.
  if (reason === "quitting") return "suspended";
  if (reason === "workspaceUnavailable") return "workspaceGone";
  if (reason === "limit") return "tooMuch";
  return null;
}

/** The empty state's words: the task has no page yet, or the named file is not one. */
export function emptyView(reason: PreviewEmptyReasonV1): "noPage" | "notOpenable" {
  return reason === "noEntry" ? "noPage" : "notOpenable";
}

/** A file the files tab may offer to open in the preview. */
export function isPreviewablePage(path: string | null | undefined): path is string {
  if (!path) return false;
  const name = path.slice(path.lastIndexOf("/") + 1);
  if (name.startsWith(".") || path.split("/").some(part => part.startsWith("."))) return false;
  return /\.html?$/i.test(name);
}

/** A task whose changes include a page: its results open on the preview (PLAN 1.5). */
export function hasPreviewPage(entries: readonly { kind: string; path: string }[] | null | undefined): boolean {
  return Boolean(entries?.some(entry => entry.kind !== "deleted" && isPreviewablePage(entry.path)));
}

/** The layout request sent to main, and when it is worth sending again. */
export interface LayoutState { bounds: PreviewBoundsV1; viewport: "wide" | "narrow"; obscured: boolean }
export function sameLayout(a: LayoutState | null, b: LayoutState): boolean {
  return a !== null && a.viewport === b.viewport && a.obscured === b.obscured
    && a.bounds.x === b.bounds.x && a.bounds.y === b.bounds.y && a.bounds.width === b.bounds.width && a.bounds.height === b.bounds.height;
}
