import { useEffect, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../i18n";
export type SettingsSection = "general" | "roles" | "agents" | "updates" | "storage" | "project";
export function AppSettings({ open, section, onSection, onClose, onRoles, disabled, restartLocked, rolesEditor, storage, project, children }: {
  open: boolean; section: SettingsSection; onSection: (section: SettingsSection) => void;
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
    <header><div><p className="m5-eyebrow">Elsecrew</p><h2 id="app-settings-title">{t("m5.preferences.title")}</h2></div>
      <button type="button" className="m5-secondary" disabled={Boolean(rolesEditor)} onClick={onClose}>{t("m5.preferences.close")}</button></header>
    <div className="m5-preferences-layout">
      <nav aria-label={t("m5.preferences.title")}>{(["general", "roles", "agents", "storage", "project", "updates"] as const).map(key =>
        <button key={key} type="button" aria-current={section === key ? "page" : undefined} disabled={Boolean(rolesEditor)} onClick={() => onSection(key)}>{t(`m5.preferences.${key}`)}</button>)}</nav>
      <div className="m5-preferences-content">
        <section hidden={section !== "general"}><h3>{t("m5.preferences.general")}</h3><p>{t("m5.shell.languageScope")}</p>
          <label className="m5-preferences-row">{t("m5.shell.language")}<select value={lang} onChange={event => setLang(event.target.value === "en" ? "en" : "ko")}><option value="ko">한국어</option><option value="en">English</option></select></label></section>
        <section hidden={section !== "roles"}><h3>{t("m5.preferences.roles")}</h3><p>{t("m5.roles.appDefaultsHelp")}</p>
          <button type="button" className="m5-secondary" disabled={disabled || Boolean(rolesEditor)} onClick={onRoles}>{t("m5.roles.appDefaults")}</button>{rolesEditor}</section>
        <section hidden={section !== "agents"}><h3>{t("m5.preferences.agents")}</h3>{children}</section>
        <section hidden={section !== "storage"}><h3>{t("m5.preferences.storage")}</h3>{storage}</section>
        <section hidden={section !== "project"}><h3>{t("m5.preferences.project")}</h3>{project}</section>
        <section hidden={section !== "updates"}><h3>{t("m5.preferences.updates")}</h3><p>{t("m5.preferences.updateHelp")}</p>
          <button type="button" className="m5-secondary" disabled={!window.m5AppUpdate || updateBusy} onClick={async () => {
            setUpdateError(false); setUpdateBusy(true);
            try { await window.m5AppUpdate?.open(); } catch { setUpdateError(true); } finally { setUpdateBusy(false); }
          }}>{t("m5.preferences.checkUpdate")}</button>
          {!window.m5AppUpdate && <p role="status">{t("m5.preferences.updateUnavailable")}</p>}
          {updateError && <p role="alert">{t("m5.preferences.updateError")}</p>}</section>
      </div>
    </div>
  </section>;
}
