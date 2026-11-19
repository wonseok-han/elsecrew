import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type AutomationPermissions } from "./contracts/project-catalog-v1.ts";

/** One consent per selected project, not per generated file. No inference from login.
 * `proposeAll` (the first task's setup, user decision 2026-11-18 "기본 전부 허용,
 * 첫 작업 때 한 번 확인"): a project with nothing allowed yet shows both boxes
 * checked as an unsaved draft. Nothing is stored here; the host saves the draft
 * (`onDraft`) only when the user confirms and sends the first task. */
export type PermissionDraft = {
  projectId: string; permissions: AutomationPermissions;
  /** Must be stored on the first task: changed, or never recorded at all. */
  needsSave: boolean;
  /** The user set these boxes themselves. */
  touched: boolean;
  /** What was saved when the user chose (a later saved change wins). */
  basis: { permissions: AutomationPermissions; recorded: boolean };
};
export type ChosenPermissions = { permissions: AutomationPermissions; basis: PermissionDraft["basis"] };
export function ProjectPermissions({ projectId, disabled, proposeAll = false, chosen = null, onDraft }: {
  projectId: string; disabled: boolean; proposeAll?: boolean;
  /** The user's own earlier choice for this project in this window (kept by
   * the host across a remount, e.g. after editing the team). */
  chosen?: ChosenPermissions | null;
  onDraft?: (draft: PermissionDraft | null) => void;
}) {
  const { t } = useI18n();
  const [saved, setSaved] = useState<AutomationPermissions | null>(null);
  const [recorded, setRecorded] = useState(true);
  const [draft, setDraft] = useState<AutomationPermissions>({ files: false, commands: false });
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  // The user's own changes are never replaced by a later reload's proposal.
  const touched = useRef(chosen !== null);
  const epoch = useRef(0);
  useEffect(() => {
    const generation = ++epoch.current;
    setSaved(null); setError(false);
    if (disabled || !window.m5Projects) return;
    const request = { action: "getAutomationPermissions" as const, projectId };
    void window.m5Projects.request(request).then(value => {
      const result = validateProjectCatalogResult(value, request);
      if (epoch.current === generation && result.action === request.action) {
        setSaved(result.permissions); setRecorded(result.recorded);
        // The user's choice first; otherwise propose full access only for a
        // project that never recorded a choice.
        const sameBasis = chosen && chosen.basis.recorded === result.recorded
          && chosen.basis.permissions.files === result.permissions.files && chosen.basis.permissions.commands === result.permissions.commands;
        if (chosen && sameBasis) setDraft(chosen.permissions);
        else if (chosen) { touched.current = false; setDraft(proposeAll && !result.recorded ? { files: true, commands: true } : result.permissions); }
        else if (!touched.current) setDraft(proposeAll && !result.recorded ? { files: true, commands: true } : result.permissions);
      }
    }).catch(() => { if (epoch.current === generation) setError(true); });
    return () => { epoch.current++; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `chosen` is read once per load
  }, [projectId, disabled, proposeAll]);
  async function save() {
    if (disabled || pending.current || !saved || !window.m5Projects) return;
    pending.current = true; setBusy(true); setError(false);
    const generation = epoch.current;
    const request = { action: "setAutomationPermissions" as const, projectId, permissions: draft, confirmed: true as const };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation === epoch.current && result.action === request.action) { setSaved(result.permissions); setRecorded(true); }
    } catch { if (generation === epoch.current) setError(true); }
    finally { pending.current = false; setBusy(false); }
  }
  const dirty = saved && (draft.files !== saved.files || draft.commands !== saved.commands);
  const needsSave = Boolean(dirty) || (proposeAll && saved !== null && !recorded);
  useEffect(() => { onDraft?.(saved ? { projectId, permissions: draft, needsSave, touched: touched.current, basis: { permissions: saved, recorded } } : null); }, [saved, recorded, draft, needsSave, onDraft, projectId]);
  useEffect(() => () => onDraft?.(null), [onDraft]);
  return <section className="m5-project-permissions" aria-busy={busy}>
    <h3>{t("m5.permissions.title")}</h3>
    <p className="m5-settings-intro">{t("m5.permissions.scope")}</p>
    <div className="m5-settings-group m5-permission-options">
      <label className="m5-preferences-row"><span>{t("m5.permissions.files")}</span><input type="checkbox" checked={draft.files} disabled={disabled || busy || !saved} onChange={e => { const files = e.target.checked; touched.current = true; setDraft(current => ({ ...current, files })); }} /></label>
      <label className="m5-preferences-row"><span>{t("m5.permissions.commands")}</span><input type="checkbox" checked={draft.commands} disabled={disabled || busy || !saved} onChange={e => { const commands = e.target.checked; touched.current = true; setDraft(current => ({ ...current, commands })); }} /></label>
    </div>
    <p className="m5-settings-intro">{t("m5.permissions.boundary")}</p>
    <div className="m5-storage-actions"><button type="button" disabled={disabled || busy || !dirty} onClick={() => void save()}>{t("m5.permissions.save")}</button></div>
    {error && <p role="alert">{t("m5.permissions.error")}</p>}
  </section>;
}
