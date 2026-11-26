/**
 * The View menu's zoom, as plain data (PR #9 re-review B1); the menu itself
 * is set by `zoom-menu.ts`. No Electron import, so tests read it directly.
 */
import type { MenuItemConstructorOptions } from "electron";

export type ZoomStep = -1 | 0 | 1;
/** Chromium's own zoom step (`zoomIn`/`zoomOut` roles) and its limits. */
export const ZOOM_LEVEL_STEP = 0.5;
export const ZOOM_LEVEL_MIN = -8;
export const ZOOM_LEVEL_MAX = 9;

/** The page's zoom level after one step (0: actual size). */
export function zoomLevelAfter(level: number, step: ZoomStep): number {
  if (step === 0 || !Number.isFinite(level)) return 0;
  return Math.min(ZOOM_LEVEL_MAX, Math.max(ZOOM_LEVEL_MIN, level + step * ZOOM_LEVEL_STEP));
}

/** The View menu: the platform's roles, with the zoom items routed through `zoom`. */
export function viewMenuTemplate(ko: boolean, zoom: (step: ZoomStep) => void): MenuItemConstructorOptions {
  return {
    label: ko ? "보기" : "View",
    submenu: [
      { role: "reload" }, { role: "forceReload" }, { role: "toggleDevTools" },
      { type: "separator" },
      { label: ko ? "실제 크기" : "Actual Size", accelerator: "CommandOrControl+0", click: () => zoom(0) },
      { label: ko ? "확대" : "Zoom In", accelerator: "CommandOrControl+Plus", click: () => zoom(1) },
      // ⌘= (the key without Shift) zooms in as in the built-in menu.
      { label: ko ? "확대" : "Zoom In", accelerator: "CommandOrControl+=", visible: false, acceleratorWorksWhenHidden: true, click: () => zoom(1) },
      { label: ko ? "축소" : "Zoom Out", accelerator: "CommandOrControl+-", click: () => zoom(-1) },
      { type: "separator" },
      { role: "togglefullscreen" },
    ],
  };
}
