/**
 * The app menu with the page zoom in our own hands (PR #9 re-review B1).
 *
 * Electron's built-in View menu zooms the page by itself, so main would only
 * learn of a zoom from the page afterwards, while the result preview view
 * (drawn above the page, `preview-manager.ts`) still sat where the page used
 * to be. Here the zoom items call `zoom(step)`, which hides the preview first
 * and then changes the zoom. Everything else stays the platform's roles.
 */
import { app, Menu } from "electron";
import { viewMenuTemplate, type ZoomStep } from "./zoom-menu-model.ts";

export { zoomLevelAfter, type ZoomStep } from "./zoom-menu-model.ts";

/** Sets the application menu before anything appends to it (the app update menu does). */
export function installZoomGuardedMenu(zoom: (step: ZoomStep) => void): void {
  const ko = app.getLocale().toLowerCase().startsWith("ko");
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    ...(process.platform === "darwin" ? [{ role: "appMenu" as const }] : []),
    { role: "fileMenu" }, { role: "editMenu" }, viewMenuTemplate(ko, zoom), { role: "windowMenu" },
  ]));
}
