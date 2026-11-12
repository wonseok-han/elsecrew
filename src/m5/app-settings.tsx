import { useEffect, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../i18n";
export type SettingsSection = "general" | "roles" | "agents" | "updates" | "storage" | "project";
const settingsIcons: Record<SettingsSection, string> = {
  general: "M4 7h16M4 17h16M8 4v6M16 14v6",
  roles: "M8 10a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM2 20v-2a6 6 0 0 1 12 0v2M16 4a3 3 0 0 1 0 6M17 14a5 5 0 0 1 5 4v2",
  agents: "M8 3v4M16 3v4M6 7h12v5a6 6 0 0 1-12 0V7ZM12 18v4",
  storage: "M4 4h16v6H4zM4 14h16v6H4zM7 7h.01M7 17h.01",
  project: "M3 6h7l2 3h9v11H3V6Z",
  updates: "M20 7v5h-5M4 17v-5h5M5 8a8 8 0 0 1 13-3l2 3M4 16l2 3a8 8 0 0 0 13-3",
};
function SettingsIcon({ section }: { section: SettingsSection }) {
  return <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={settingsIcons[section]} /></svg>;
}
export function AppSettings({ hasProject, open, section, onSection, onClose, restartLocked, rolesEditor, storage, project, children }: {
  hasProject: boolean; open: boolean; section: SettingsSection; onSection: (section: SettingsSection) => void;
  onClose: () => void; onRoles: () => void; disabled: boolean; restartLocked: boolean; rolesEditor: ReactNode; storage: ReactNode; project: ReactNode; children: ReactNode;
}) {
  const { t, lang, setLang } = useI18n();
  const dialog = useRef<HTMLElement>(null);
  const [updateError, setUpdateError] = useState(false);
  const [updateBusy, setUpdateBusy] = useState(false);
  useEffect(() => {
    if (open) dialog.current?.focus();
  }, [open]);
  return <section hidden={!open} tabIndex={-1} inert={restartLocked} ref={dialog} className="m5-app-settings" aria-labelledby="app-settings-title">
    <header>
      <button type="button" className="m5-settings-return" onClick={onClose}>{t(hasProject ? "m5.preferences.close" : "m5.preferences.home")}</button>
      <div className="m5-settings-location"><h2 id="app-settings-title">{t("m5.preferences.title")}</h2><span aria-hidden="true">/</span><span>{t(`m5.preferences.${section}`)}</span></div>
    </header>
    <div className="m5-preferences-layout">
      <nav aria-label={t("m5.preferences.title")}>
        <span className="m5-settings-nav-caption">{t("m5.preferences.workspaceGroup")}</span>
        {(["general", "roles", "agents", "storage", "project", "updates"] as const).map(key =>
          <button key={key} type="button" aria-current={section === key ? "page" : undefined} onClick={() => onSection(key)}><SettingsIcon section={key} /><span>{t(`m5.preferences.${key}`)}</span></button>)}
        <div className="m5-settings-brand">Elsecrew<span>{t("m5.preferences.settingsHelp")}</span></div>
      </nav>
      <div className="m5-preferences-content">
        <section hidden={section !== "general"}><h3>{t("m5.preferences.appearance")}</h3>
          <div className="m5-settings-group"><label className="m5-preferences-row"><span><strong>{t("m5.shell.language")}</strong><small>{t("m5.shell.languageScope")}</small></span><select value={lang} onChange={event => setLang(event.target.value === "en" ? "en" : "ko")}><option value="ko">한국어</option><option value="en">English</option></select></label></div></section>
        <section hidden={section !== "roles"}><h3>{t("m5.preferences.roles")}</h3><p className="m5-settings-intro">{t("m5.roles.appDefaultsHelp")}</p>
          {rolesEditor}</section>
        <section hidden={section !== "agents"}><h3>{t("m5.preferences.agents")}</h3>{children}</section>
        <section hidden={section !== "storage"}><h3>{t("m5.preferences.storage")}</h3>{storage}</section>
        <section hidden={section !== "project"}><h3>{t("m5.preferences.project")}</h3>{project}</section>
        <section hidden={section !== "updates"}><h3>{t("m5.preferences.updates")}</h3><div className="m5-settings-group"><div className="m5-preferences-row"><span><strong>Elsecrew</strong><small>{t("m5.preferences.updateHelp")}</small></span>
          <button type="button" className="m5-secondary" disabled={!window.m5AppUpdate || updateBusy} onClick={async () => {
            setUpdateError(false); setUpdateBusy(true);
            try { await window.m5AppUpdate?.open(); } catch { setUpdateError(true); } finally { setUpdateBusy(false); }
          }}>{t("m5.preferences.checkUpdate")}</button></div></div>
          {!window.m5AppUpdate && <p role="status">{t("m5.preferences.updateUnavailable")}</p>}
          {updateError && <p role="alert">{t("m5.preferences.updateError")}</p>}</section>
      </div>
    </div>
  </section>;
}
