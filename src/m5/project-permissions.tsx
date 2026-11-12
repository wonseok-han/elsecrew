import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type AutomationPermissions } from "./contracts/project-catalog-v1.ts";

/** One consent per selected project, not per generated file. No inference from login. */
export function ProjectPermissions({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [saved, setSaved] = useState<AutomationPermissions | null>(null);
  const [draft, setDraft] = useState<AutomationPermissions>({ files: false, commands: false });
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const epoch = useRef(0);
  useEffect(() => {
    const generation = ++epoch.current;
    setSaved(null); setError(false);
    if (disabled || !window.m5Projects) return;
    const request = { action: "getAutomationPermissions" as const, projectId };
    void window.m5Projects.request(request).then(value => {
      const result = validateProjectCatalogResult(value, request);
      if (epoch.current === generation && result.action === request.action) {
        setSaved(result.permissions); setDraft(result.permissions);
      }
    }).catch(() => { if (epoch.current === generation) setError(true); });
    return () => { epoch.current++; };
  }, [projectId, disabled]);
  async function save() {
    if (disabled || pending.current || !saved || !window.m5Projects) return;
    pending.current = true; setBusy(true); setError(false);
    const generation = epoch.current;
    const request = { action: "setAutomationPermissions" as const, projectId, permissions: draft, confirmed: true as const };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation === epoch.current && result.action === request.action) { setSaved(result.permissions); }
    } catch { if (generation === epoch.current) setError(true); }
    finally { pending.current = false; setBusy(false); }
  }
  const dirty = saved && (draft.files !== saved.files || draft.commands !== saved.commands);
  return <section className="m5-project-permissions" aria-busy={busy}>
    <h3>{t("m5.permissions.title")}</h3>
    <p className="m5-settings-intro">{t("m5.permissions.scope")}</p>
    <div className="m5-settings-group m5-permission-options">
      <label className="m5-preferences-row"><span>{t("m5.permissions.files")}</span><input type="checkbox" checked={draft.files} disabled={disabled || busy || !saved} onChange={e => setDraft({ ...draft, files: e.target.checked })} /></label>
      <label className="m5-preferences-row"><span>{t("m5.permissions.commands")}</span><input type="checkbox" checked={draft.commands} disabled={disabled || busy || !saved} onChange={e => setDraft({ ...draft, commands: e.target.checked })} /></label>
    </div>
    <p className="m5-settings-intro">{t("m5.permissions.boundary")}</p>
    <div className="m5-storage-actions"><button type="button" disabled={disabled || busy || !dirty} onClick={() => void save()}>{t("m5.permissions.save")}</button></div>
    {error && <p role="alert">{t("m5.permissions.error")}</p>}
  </section>;
}
