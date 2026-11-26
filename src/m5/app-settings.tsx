import { useEffect, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../i18n";
import { osNotificationsEnabled, readNotificationPreferences, saveNotificationPreferences, setOsNotificationsEnabled, type NotificationKind, type NotificationPreferences } from "./notifications.ts";
import { setThemePreference, THEME_PREFERENCES, useTheme } from "./theme.ts";
import { Setting, SettingsGroup, Switch } from "./settings-ui.tsx";
const appVersion = typeof __ELSECREW_VERSION__ === "string" ? __ELSECREW_VERSION__ : "";
export type SettingsSection = "general" | "notifications" | "roles" | "agents" | "storage" | "project";
/** App-wide pages, then this project (ADE U4, DESIGN Q8: the project page sits in the navigation). */
export const APP_SECTIONS: readonly SettingsSection[] = ["general", "notifications", "agents", "roles", "storage"];
const settingsIcons: Record<SettingsSection, string> = {
  general: "M4 7h16M4 17h16M8 4v6M16 14v6",
  notifications: "M6 8a6 6 0 1 1 12 0c0 7 3 8 3 8H3s3-1 3-8M10 20a2 2 0 0 0 4 0",
  roles: "M8 10a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM2 20v-2a6 6 0 0 1 12 0v2M16 4a3 3 0 0 1 0 6M17 14a5 5 0 0 1 5 4v2",
  agents: "M8 3v4M16 3v4M6 7h12v5a6 6 0 0 1-12 0V7ZM12 18v4",
  storage: "M4 4h16v6H4zM4 14h16v6H4zM7 7h.01M7 17h.01",
  project: "M3 6h7l2 3h9v11H3V6Z",
};
function SettingsIcon({ section }: { section: SettingsSection }) {
  return <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={settingsIcons[section]} /></svg>;
}

const NOTIFICATION_KINDS: readonly NotificationKind[] = ["attention", "done", "stopped"];

function NotificationSettings() {
  const { t } = useI18n();
  const [osBanners, setOsBanners] = useState(() => osNotificationsEnabled());
  const [preferences, setPreferences] = useState<NotificationPreferences>(() => readNotificationPreferences());
  const change = (next: NotificationPreferences) => { setPreferences(next); saveNotificationPreferences(next); };
  return <>
    <SettingsGroup>
      <Setting title={t("m5.notifications.osBanner")} help={t("m5.notifications.osBannerHelp")}
        control={<Switch label={t("m5.notifications.osBanner")} checked={osBanners} onChange={value => { setOsBanners(value); setOsNotificationsEnabled(value); }} />} />
    </SettingsGroup>
    <SettingsGroup label={t("m5.notifications.settings.kinds")} testId="notification-kinds">
      {NOTIFICATION_KINDS.map(kind => <Setting key={kind} className={`m5-notification-kind-setting m5-notification-${kind}`} title={t(`m5.notifications.settings.${kind}`)} help={t(`m5.notifications.settings.${kind}Help`)}
        control={<Switch label={t(`m5.notifications.settings.${kind}`)} checked={preferences.kinds[kind]} onChange={value => change({ ...preferences, kinds: { ...preferences.kinds, [kind]: value } })} />} />)}
    </SettingsGroup>
    <SettingsGroup>
      <Setting title={t("m5.notifications.settings.prune")} help={t("m5.notifications.settings.pruneHelp")}
        control={<Switch label={t("m5.notifications.settings.prune")} checked={preferences.pruneResolved} onChange={value => change({ ...preferences, pruneResolved: value })} />} />
    </SettingsGroup>
  </>;
}

export function AppSettings({ hasProject, projectName, open, section, onSection, onClose, restartLocked, rolesEditor, storage, project, children }: {
  hasProject: boolean; projectName?: string; open: boolean; section: SettingsSection; onSection: (section: SettingsSection) => void;
  onClose: () => void; onRoles: () => void; disabled: boolean; restartLocked: boolean; rolesEditor: ReactNode; storage: ReactNode; project: ReactNode; children: ReactNode;
}) {
  const { t, lang, setLang } = useI18n();
  const dialog = useRef<HTMLElement>(null);
  const [updateError, setUpdateError] = useState(false);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateUnavailableHere, setUpdateUnavailableHere] = useState(false);
  const theme = useTheme();
  useEffect(() => {
    if (open) dialog.current?.focus();
  }, [open]);
  const navButton = (key: SettingsSection, label: ReactNode = t(`m5.preferences.${key}`)) =>
    <button key={key} type="button" aria-current={section === key ? "page" : undefined} onClick={() => onSection(key)}><SettingsIcon section={key} /><span>{label}</span></button>;
  return <section hidden={!open} tabIndex={-1} inert={restartLocked} ref={dialog} className="m5-app-settings" aria-labelledby="app-settings-title">
    <header>
      <button type="button" className="m5-settings-return m5-btn-quiet" onClick={onClose}>{t(hasProject ? "m5.preferences.close" : "m5.preferences.home")}</button>
      <div className="m5-settings-location"><h2 id="app-settings-title">{t("m5.preferences.title")}</h2><span aria-hidden="true">›</span><span>{section === "project" && hasProject && projectName ? projectName : t(`m5.preferences.${section}`)}</span></div>
    </header>
    <div className="m5-preferences-layout">
      <nav aria-label={t("m5.preferences.title")}>
        <p className="m5-group-label" aria-hidden="true">{t("m5.preferences.groupApp")}</p>
        {APP_SECTIONS.map(key => navButton(key))}
        <p className="m5-group-label" aria-hidden="true">{t(hasProject ? "m5.preferences.groupThisProject" : "m5.preferences.groupProjects")}</p>
        {navButton("project", t("m5.preferences.project"))}
      </nav>
      <div className="m5-preferences-content">
        {/* Updates live with the other app-wide choices; a section of one button read as empty. */}
        <section hidden={section !== "general"}><h3>{t("m5.preferences.general")}</h3>
          <SettingsGroup>
            <Setting as="label" title={t("m5.shell.language")} help={t("m5.shell.languageScope")}
              control={<select value={lang} onChange={event => setLang(event.target.value === "en" ? "en" : "ko")}><option value="ko">한국어</option><option value="en">English</option></select>} />
            <Setting title={t("m5.preferences.theme")} titleId="m5-theme-label" help={t("m5.preferences.themeHelp")}
              control={<div className="m5-segmented" role="group" aria-labelledby="m5-theme-label" data-testid="theme-choice">{THEME_PREFERENCES.map(choice =>
                <button key={choice} type="button" aria-pressed={theme.preference === choice} onClick={() => setThemePreference(choice)}>{t(`m5.preferences.theme.${choice}`)}</button>)}</div>}>
              {theme.saveFailed && <p role="status" data-testid="theme-save-failed">{t("m5.preferences.themeSaveFailed")}</p>}
            </Setting>
            <Setting title={t("m5.preferences.updates")} help={appVersion ? t("m5.preferences.currentVersion", { version: appVersion }) : t("m5.preferences.updateHelp")}
              control={<button type="button" className="m5-secondary" disabled={!window.m5AppUpdate || updateBusy} onClick={async () => {
                setUpdateError(false); setUpdateBusy(true);
                // In the app (PR #16 review I4): main starts the check and the banner follows it; no native box.
                try {
                  const api = window.m5AppUpdate;
                  if (api?.request) setUpdateUnavailableHere((await api.request("check")) === "unavailable");
                  else await api?.open();
                } catch { setUpdateError(true); } finally { setUpdateBusy(false); }
              }}>{t("m5.preferences.checkUpdate")}</button>}>
              {(!window.m5AppUpdate || updateUnavailableHere) && <p role="status">{t("m5.preferences.updateUnavailable")}</p>}
              {updateError && <p role="alert">{t("m5.preferences.updateError")}</p>}
            </Setting>
          </SettingsGroup>
        </section>
        <section hidden={section !== "notifications"}><h3>{t("m5.preferences.notifications")}</h3><p className="m5-settings-intro">{t("m5.notifications.settings.intro")}</p>
          <NotificationSettings /></section>
        <section hidden={section !== "roles"}><h3>{t("m5.preferences.roles")}</h3><p className="m5-settings-intro">{t("m5.roles.appDefaultsHelp")}</p>
          {rolesEditor}</section>
        <section hidden={section !== "agents"}><h3>{t("m5.preferences.agents")}</h3>{children}</section>
        <section hidden={section !== "storage"}><h3>{t("m5.preferences.storage")}</h3><p className="m5-settings-intro">{t("m5.preferences.storageIntro")}</p>{storage}</section>
        <section hidden={section !== "project"}><h3>{section === "project" && hasProject && projectName ? projectName : t("m5.preferences.project")}</h3>{project}</section>
      </div>
    </div>
  </section>;
}
