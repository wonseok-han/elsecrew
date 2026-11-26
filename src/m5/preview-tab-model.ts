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

/** A grid of points inside the stage, near its edges too, to ask what is
 * drawn on top. Kept a few pixels in from the edge: the stage's rounded
 * corners are not the stage to the page's hit test. */
export function samplePoints(stage: Rect, steps = 4): { x: number; y: number }[] {
  const inset = Math.min(8, stage.width / 4, stage.height / 4);
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
 * at a grid of points: anything that is not the stage itself covers it.
 */
export function coveredAtPoints(points: readonly { x: number; y: number }[], topAt: (x: number, y: number) => unknown, isStage: (element: unknown) => boolean): boolean {
  return points.some(point => {
    const element = topAt(point.x, point.y);
    return element !== null && element !== undefined && !isStage(element);
  });
}

/** What the tab shows for a closed preview; `null`: nothing to say (it simply reopens). */
export type PreviewClosedView = "stalled" | "workspaceGone" | "tooMuch" | null;
export function closedView(reason: PreviewCloseReasonV1): PreviewClosedView {
  if (reason === "crashed") return "stalled";
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

/** The layout request sent to main, and when it is worth sending again. */
export interface LayoutState { bounds: PreviewBoundsV1; viewport: "wide" | "narrow"; obscured: boolean }
export function sameLayout(a: LayoutState | null, b: LayoutState): boolean {
  return a !== null && a.viewport === b.viewport && a.obscured === b.obscured
    && a.bounds.x === b.bounds.x && a.bounds.y === b.bounds.y && a.bounds.width === b.bounds.width && a.bounds.height === b.bounds.height;
}
