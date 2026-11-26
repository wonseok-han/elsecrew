import { app, dialog, Menu, MenuItem } from "electron";
import { AppUpdateController, type UpdateState } from "./app-update.ts";
import { removeStaleUpdates, SelfUpdater } from "./self-update.ts";
import { platformKey, UPDATE_FEED_URL } from "./update-manifest.ts";

// Updates come from the project's own signed release manifest (no paid code
// signing). Only packaged builds on supported platforms use the channel.

/** What main exposes to the in-app banner (ADE U8, Q7): the state it owns, a
 * check it may start, and the restart it alone confirms and performs. */
export interface AppUpdateHandle {
  state(): UpdateState;
  /** Starts a check (and download) when the controller allows it; nothing else. */
  check(): void;
  /** Restarts into the downloaded update; only after main's own confirmation (caller). */
  restart(): Promise<void>;
}

/**
 * The menu item stays (Q7) but answers in the app: a check starts at once and
 * its progress shows on the in-app banner; an unavailable build opens the
 * app's settings (`reveal`). Only the restart into a downloaded update keeps
 * main's own native confirmation (PR #16 review I4).
 */
export function installAppUpdateMenu(restart: (install: () => void) => Promise<boolean>, changed: (state: UpdateState) => void = () => undefined, reveal: () => void = () => undefined): AppUpdateHandle {
  const ko = app.getLocale().toLowerCase().startsWith("ko");
  const labels: Record<UpdateState, string> = {
    unavailable: ko ? "앱 업데이트 배포 준비 중" : "App updates are not configured",
    idle: ko ? "앱 업데이트 확인 및 다운로드…" : "Check and download app update…",
    current: ko ? "최신 버전이에요 — 다시 확인…" : "Up to date — check again…",
    checking: ko ? "앱 업데이트 확인 중…" : "Checking for app updates…",
    downloading: ko ? "앱 업데이트 다운로드 중…" : "Downloading app update…",
    ready: ko ? "업데이트 적용 및 재시작…" : "Install update and restart…",
    restarting: ko ? "안전한 종료 확인 중…" : "Checking safe shutdown…",
    error: ko ? "업데이트 실패 — 다시 확인…" : "Update failed — retry…",
  };
  let busy = false;
  const controller = new AppUpdateController(new SelfUpdater(), restart, refresh);
  function refresh(): void {
    const menu = Menu.getApplicationMenu();
    const item = menu?.getMenuItemById("elsecrew-app-update");
    if (item) {
      item.label = labels[controller.state];
      item.enabled = !busy && !["checking", "downloading", "restarting"].includes(controller.state);
    }
    changed(controller.state);
  }
  const menu = Menu.getApplicationMenu() ?? Menu.buildFromTemplate([
    ...(process.platform === "darwin" ? [{ role: "appMenu" as const }] : []),
    { role: "fileMenu" }, { role: "editMenu" }, { role: "viewMenu" }, { role: "windowMenu" },
  ]);
  const updateMenu = Menu.buildFromTemplate([{
    id: "elsecrew-app-update", label: labels[controller.state],
    click: async () => {
      if (busy) return;
      busy = true; refresh();
      try {
        if (controller.state === "unavailable") {
          reveal();
        } else if (controller.state === "ready") {
          const answer = await dialog.showMessageBox({ type: "question", message: ko ? "업데이트를 적용하고 다시 시작할까요?" : "Install the update and restart?",
            detail: ko ? "편집 중인 내용을 먼저 저장하세요. 진행 중인 작업이 안전하게 멈추는 것을 확인할 수 없으면 다시 시작하지 않아요." : "Save your edits first. The app does not restart unless running work can be confirmed to stop safely.",
            buttons: ko ? ["취소", "다시 시작"] : ["Cancel", "Restart"], defaultId: 0, cancelId: 0 });
          if (answer.response === 1) await controller.restart();
        } else {
          // Checking and downloading change nothing on this computer until a restart, which main still confirms.
          controller.check();
          reveal();
        }
      } finally { busy = false; refresh(); }
    },
  }]);
  // Preserve the existing platform menus and their standard editing shortcuts.
  menu.append(new MenuItem({ label: ko ? "앱 업데이트" : "App Update", submenu: updateMenu }));
  Menu.setApplicationMenu(menu);
  const updatable = app.isPackaged && platformKey(process.platform, process.arch) !== null;
  controller.configure(UPDATE_FEED_URL, updatable);
  if (updatable) removeStaleUpdates();
  return { state: () => controller.state, check: () => controller.check(), restart: () => controller.restart() };
}
