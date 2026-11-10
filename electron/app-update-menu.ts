import { app, autoUpdater, dialog, Menu, MenuItem } from "electron";
import { AppUpdateController, type UpdateState } from "./app-update.ts";

// Release-owned configuration. Enable only with a signed macOS distribution and
// its reviewed HTTPS feed; the current development ZIP is intentionally disabled.
const RELEASE_FEED: string | null = null;
const SIGNED_UPDATE_PACKAGE = false;

export function installAppUpdateMenu(restart: (install: () => void) => Promise<boolean>): void {
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
  const controller = new AppUpdateController(autoUpdater, restart, refresh);
  function refresh(): void {
    const menu = Menu.getApplicationMenu();
    const item = menu?.getMenuItemById("hello-agent-app-update");
    if (item) {
      item.label = labels[controller.state];
      item.enabled = !busy && !["checking", "downloading", "restarting"].includes(controller.state);
    }
  }
  const menu = Menu.getApplicationMenu() ?? Menu.buildFromTemplate([
    ...(process.platform === "darwin" ? [{ role: "appMenu" as const }] : []),
    { role: "fileMenu" }, { role: "editMenu" }, { role: "viewMenu" }, { role: "windowMenu" },
  ]);
  const updateMenu = Menu.buildFromTemplate([{
    id: "hello-agent-app-update", label: labels[controller.state],
    click: async () => {
      if (busy) return;
      busy = true; refresh();
      try {
        if (controller.state === "unavailable") {
          await dialog.showMessageBox({ type: "info", message: ko ? "앱 업데이트 배포를 준비 중이에요." : "App update distribution is not ready yet.",
            detail: ko ? `현재 버전: ${app.getVersion()}\n이 개발 앱은 서명된 업데이트 배포 경로가 아직 없어 자동으로 교체할 수 없어요. 에이전트 업데이트와는 별도 기능이에요.`
              : `Current version: ${app.getVersion()}\nThis development build has no signed update channel yet. App updates are separate from agent updates.` });
        } else if (controller.state === "ready") {
          const answer = await dialog.showMessageBox({ type: "question", message: ko ? "업데이트를 적용하고 다시 시작할까요?" : "Install the update and restart?",
            detail: ko ? "편집 중인 내용을 먼저 저장하세요. 진행 중인 작업의 안전한 종료를 확인할 수 없으면 재시작하지 않아요." : "Save your edits first. Restart is blocked if safe task shutdown cannot be confirmed.",
            buttons: ko ? ["취소", "저장했어요 — 재시작"] : ["Cancel", "Edits saved — restart"], defaultId: 0, cancelId: 0 });
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
  controller.configure(RELEASE_FEED, app.isPackaged && process.platform === "darwin" && SIGNED_UPDATE_PACKAGE);
}
