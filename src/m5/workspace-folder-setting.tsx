import { useEffect, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult } from "./contracts/project-catalog-v1.ts";

type Folder = { path: string; custom: boolean };

/** "작업 공간 폴더": where new tasks get their worktree folder (like Orca). */
export function WorkspaceFolderSetting({ disabled }: { disabled: boolean }) {
  const { t } = useI18n();
  const [folder, setFolder] = useState<Folder | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  async function send(action: "getWorkspaceFolder" | "chooseWorkspaceFolder" | "resetWorkspaceFolder") {
    if (!window.m5Projects) return;
    setBusy(true); setError(false);
    const request = { action };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if ("folder" in result) setFolder(result.folder);
    } catch { setError(true); }
    finally { setBusy(false); }
  }
  useEffect(() => { void send("getWorkspaceFolder"); }, []);
  return <section className="m5-workspace-folder" aria-labelledby="m5-workspace-folder-title">
    <h3 id="m5-workspace-folder-title">{t("m5.workspaceFolder.title")}</h3>
    <p className="m5-settings-intro">{t("m5.workspaceFolder.help")}</p>
    {folder && <p className="m5-workspace-folder-path"><code title={folder.path}>{folder.path}</code>{!folder.custom && <small> · {t("m5.workspaceFolder.default")}</small>}</p>}
    <div className="m5-workspace-folder-actions">
      <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => void send("chooseWorkspaceFolder")}>{t("m5.workspaceFolder.choose")}</button>
      {folder?.custom && <button type="button" className="m5-link-button" disabled={disabled || busy} onClick={() => void send("resetWorkspaceFolder")}>{t("m5.workspaceFolder.reset")}</button>}
    </div>
    {error && <p role="alert">{t("m5.workspaceFolder.error")}</p>}
  </section>;
}
