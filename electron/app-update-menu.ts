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

export function installAppUpdateMenu(restart: (install: () => void) => Promise<boolean>, changed: (state: UpdateState) => void = () => undefined): AppUpdateHandle {
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
          await dialog.showMessageBox({ type: "info", message: ko ? "이 실행에서는 앱 업데이트를 쓸 수 없어요." : "App updates are not available in this build.",
            detail: ko ? `현재 버전: ${app.getVersion()}\n배포된 Elsecrew(macOS·Windows)에서만 새 버전을 받아 교체해요. 에이전트 업데이트와는 별도 기능이에요.`
              : `Current version: ${app.getVersion()}\nOnly released Elsecrew builds for macOS and Windows download and install new versions. App updates are separate from agent updates.` });
        } else if (controller.state === "ready") {
          const answer = await dialog.showMessageBox({ type: "question", message: ko ? "업데이트를 적용하고 다시 시작할까요?" : "Install the update and restart?",
            detail: ko ? "편집 중인 내용을 먼저 저장하세요. 진행 중인 작업이 안전하게 멈추는 것을 확인할 수 없으면 다시 시작하지 않아요." : "Save your edits first. The app does not restart unless running work can be confirmed to stop safely.",
            buttons: ko ? ["취소", "다시 시작"] : ["Cancel", "Restart"], defaultId: 0, cancelId: 0 });
          if (answer.response === 1) await controller.restart();
        } else {
          const answer = await dialog.showMessageBox({ type: "question", message: ko ? "새 앱 버전을 확인하고 다운로드할까요?" : "Check for and download a new app version?",
            detail: ko ? "새 버전이 있으면 다운로드해요. 바로 재시작하지는 않아요." : "An available update will be downloaded. The app will not restart immediately.",
            buttons: ko ? ["취소", "확인 및 다운로드"] : ["Cancel", "Check and download"], defaultId: 0, cancelId: 0 });
          if (answer.response === 1) controller.check();
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
