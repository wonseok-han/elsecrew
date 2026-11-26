import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../i18n";
import type { ChatUiController } from "./chat-state.ts";
import type { ModelChoicesResult } from "./contracts/model-choices-v1.ts";
import { validateProjectCatalogResult } from "./contracts/project-catalog-v1.ts";
import { G5_ROLES_MAX_V3, G5_ROLE_NAME_MAX_BYTES_V3, G5_ROLE_RESPONSIBILITY_MAX_BYTES_V3, isBlankText, utf8ByteLength as nameBytes, type ProjectRoleSettingsV1, type RoleDefinitionV3, type RoleProfileV1 } from "./contracts/run-recovery-v1.ts";
import { cachedModels, hasUsableModel, loadModels } from "./model-catalog.ts";
import { roleLabel } from "./role-names.ts";
import { correctSettingsDraft, editSettingsDraft, loadSettingsDraft, settingsDraftDirty, settingsDraftSavable } from "./settings-draft.ts";
import { addWorker, canAddWorker, canRemoveWorker, isValidTeamDraft, removeWorker, sameTeam, teamDraftFrom, updateDefinition, updateProfile, type TeamDraft } from "./team-draft.ts";

/** Project team roles (dialog) and the app defaults (settings page). Moved from workbench.tsx (custom roles R0). */
export type RoleNavigationGuard = { current: ((next: () => void) => void) | null };

export function AppRoleDefaultsEditor({ disabled, onClose, navigationGuard, activeChat }: { activeChat: ReturnType<ChatUiController["snapshot"]>["activeChat"]; disabled: boolean; onClose: () => void; navigationGuard: RoleNavigationGuard }) {
  const { t } = useI18n();
  const [settings, setSettings] = useState<ProjectRoleSettingsV1 | null>(null);
  const [error, setError] = useState(false);
  const [saved, setSaved] = useState(false);
  const [errorDetail, setErrorDetail] = useState("");
  const mounted = useRef(false);
  const generation = useRef(0);
  const load = useCallback(async () => {
    const epoch = ++generation.current;
    setError(false); setSaved(false);
    try {
      const request = { action: "getAppRoleDefaults" as const };
      const result = validateProjectCatalogResult(await window.m5Projects!.request(request), request);
      if (mounted.current && generation.current === epoch && result.action === request.action) setSettings({ ...result.settings, projectId: "app_defaults" });
    } catch (error) { if (mounted.current && generation.current === epoch) { setError(true); setErrorDetail(error instanceof Error ? error.message : "defaults request failed"); } }
  }, []);
  useEffect(() => { mounted.current = true; if (!disabled) void load(); return () => { mounted.current = false; generation.current++; }; }, [disabled, load]);
  const save = async (roles: readonly RoleProfileV1[], roleDefinitions: readonly RoleDefinitionV3[]) => {
    if (!settings) return;
    const epoch = ++generation.current;
    setError(false); setSaved(false);
    try {
      const request = { action: "updateAppRoleDefaults" as const, expectedRevision: settings.revision, roles, roleDefinitions };
      const result = validateProjectCatalogResult(await window.m5Projects!.request(request), request);
      if (mounted.current && generation.current === epoch && result.action === request.action) { setSettings({ ...result.settings, projectId: "app_defaults" }); setSaved(true); }
    } catch (error) { if (mounted.current && generation.current === epoch) { setError(true); setErrorDetail(error instanceof Error ? error.message : "defaults request failed"); } }
  };
  if (!settings) return <section className="m5-notice" role="status">
    <p>{t(error ? "m5.roles.appDefaultsError" : "m5.roles.busy")}</p>
    {error && <button type="button" disabled={disabled} onClick={() => void load()}>{t("m5.roles.reload")}</button>}
    {error && <details><summary>{t("m5.feedback.details")}</summary><pre>{errorDetail}</pre></details>}
  </section>;
  return <RoleSettings navigationGuard={navigationGuard} scope="app" settings={settings} activeChat={activeChat} disabled={disabled}
    feedback={<>{error && <p role="alert">{t("m5.roles.appDefaultsError")}</p>}{saved && <p role="status">{t("m5.roles.appDefaultsSaved")}</p>}</>}
    onSave={save} onReload={load} onClose={onClose} projectName={t("m5.roles.appDefaults")} />;
}

export function RoleSettings({ navigationGuard, settings, disabled, feedback, onSave, onReload, onClose, returnToDraft = false, projectName, scope = "project" }: { navigationGuard?: RoleNavigationGuard; scope?: "project" | "app"; settings: NonNullable<ReturnType<ChatUiController["snapshot"]>["settings"]>; activeChat: ReturnType<ChatUiController["snapshot"]>["activeChat"]; disabled: boolean; feedback: ReactNode; onSave: (roles: readonly RoleProfileV1[], roleDefinitions: readonly RoleDefinitionV3[]) => Promise<void>; onReload: () => Promise<void>; onClose: () => void; returnToDraft?: boolean; projectName: string }) {
  const { t } = useI18n();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const initial = useMemo(() => teamDraftFrom(settings), [settings]);
  // Unsaved means the person changed something (settings-draft.ts): the
  // screen's own corrections apply to the loaded values too.
  const [draft, setDraft] = useState(() => loadSettingsDraft(initial));
  const team = draft.current;
  const setTeam = (update: TeamDraft | ((old: TeamDraft) => TeamDraft)) => setDraft(old => editSettingsDraft(old, current => typeof update === "function" ? update(current) : update));
  const correctTeam = (correct: (old: TeamDraft) => TeamDraft) => setDraft(old => correctSettingsDraft(old, correct));
  const roles = team.roles;
  const [selectedRole, setSelectedRole] = useState<RoleProfileV1["roleId"]>("coordinator");
  const [busy, setBusy] = useState(false);
  const [discardAction, setDiscardAction] = useState<"close" | "reload" | null>(null);
  // Role IDs are user-visible opaque strings; a Map avoids prototype keys such as `constructor`.
  const [choices, setChoices] = useState<ReadonlyMap<string, ModelChoicesResult["models"]>>(() => new Map());
  const [choicesError, setChoicesError] = useState(false);
  const [choicesBusy, setChoicesBusy] = useState(false);
  const [fillMissed, setFillMissed] = useState(false);
  const choiceEpoch = useRef(0);
  useEffect(() => {
    choiceEpoch.current += 1;
    setChoices(new Map()); setChoicesError(false);
    return () => { choiceEpoch.current += 1; };
  }, [settings]);
  const loadChoices = async (role: RoleProfileV1, refresh = false) => {
    if (!window.m5Models) return;
    const epoch = ++choiceEpoch.current;
    const cached = cachedModels(role.runtime);
    if (cached) setChoices(old => new Map(old).set(role.roleId, cached));
    setChoicesBusy(true); setChoicesError(false);
    try {
      const result = await loadModels(role.runtime, input => window.m5Models!.request(input), refresh);
      if (epoch !== choiceEpoch.current) return;
      setChoices(old => new Map(old).set(role.roleId, result.models));
      const defaults = result.models.filter(model => model.isDefault);
      if (defaults.length === 1) correctTeam(old => ({ ...old, roles: old.roles.map(item => {
        const initialDefault = scope === "app" && settings.revision === 1 && initial.roles.some(start => JSON.stringify(start) === JSON.stringify(item));
        return item.runtime === role.runtime && ((!item.modelSelector && item.roleId === role.roleId) || initialDefault)
          ? { ...item, modelSelector: defaults[0].modelSelector, reasoningEffort: role.runtime === "codex" ? (defaults[0].reasoningEfforts.includes("medium") ? "medium" : defaults[0].reasoningEfforts[0] ?? null) : null } : item;
      }) }));
    } catch { if (epoch === choiceEpoch.current) setChoicesError(true); }
    finally { if (epoch === choiceEpoch.current) setChoicesBusy(false); }
  };
  const usable = (role: RoleProfileV1) => hasUsableModel(role.runtime, role.modelSelector, choices.get(role.roleId) ?? cachedModels(role.runtime));
  // One click gives every role without a usable model its provider's
  // recommendation. Only the draft changes; saving stays explicit.
  const fillRecommended = async () => {
    if (!window.m5Models) return;
    const epoch = ++choiceEpoch.current;
    setChoicesBusy(true); setChoicesError(false); setFillMissed(false);
    try {
      const catalogs = new Map<RoleProfileV1["runtime"], readonly { modelSelector: string; isDefault?: boolean; reasoningEfforts: readonly string[] }[]>();
      for (const runtime of new Set(team.roles.map(role => role.runtime))) {
        catalogs.set(runtime, (await loadModels(runtime, input => window.m5Models!.request(input))).models);
      }
      if (epoch !== choiceEpoch.current) return;
      const needs = (role: RoleProfileV1) => !role.modelSelector || !catalogs.get(role.runtime)!.some(model => model.modelSelector === role.modelSelector);
      const recommended = (runtime: RoleProfileV1["runtime"]) => {
        const defaults = catalogs.get(runtime)!.filter(model => model.isDefault);
        return defaults.length === 1 ? defaults[0] : undefined;
      };
      // A provider without exactly one recommendation leaves its roles as they are; say so.
      setFillMissed(team.roles.some(role => needs(role) && !recommended(role.runtime)));
      setTeam(old => ({ ...old, roles: old.roles.map(item => {
        const model = needs(item) ? recommended(item.runtime) : undefined;
        return model ? { ...item, modelSelector: model.modelSelector, reasoningEffort: item.runtime === "codex" ? (model.reasoningEfforts.includes("medium") ? "medium" : model.reasoningEfforts[0] ?? null) : null } : item;
      }) }));
    } catch { if (epoch === choiceEpoch.current) setChoicesError(true); }
    finally { if (epoch === choiceEpoch.current) setChoicesBusy(false); }
  };
  const selectedRuntime = roles.find(role => role.roleId === selectedRole)?.runtime;
  useEffect(() => {
    const role = roles.find(item => item.roleId === selectedRole);
    if (role) void loadChoices(role);
    return () => { choiceEpoch.current++; };
  }, [selectedRole, selectedRuntime, settings]);
  const dirty = settingsDraftDirty(draft, sameTeam);
  // The screen's own corrections (a recommended model) are not "unsaved
  // changes", but they can be kept by saving.
  const savable = dirty || settingsDraftSavable(draft, sameTeam);
  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => { dialog?.close(); };
  }, []);
  useEffect(() => { setDraft(loadSettingsDraft(initial)); setSelectedRole("coordinator"); setDiscardAction(null); }, [initial]);
  const change = (roleId: string, field: "runtime" | "modelSelector" | "reasoningEffort" | "permission", value: string | null) => setTeam(old => updateProfile(old, roleId, { [field]: value } as Partial<RoleProfileV1>));
  const define = (roleId: string, field: "name" | "responsibility", value: string) => setTeam(old => updateDefinition(old, roleId, { [field]: value }));
  const valid = isValidTeamDraft(team);
  const editing = disabled || busy || discardAction !== null;
  const run = async (action: () => Promise<void>) => { setBusy(true); try { await action(); } finally { setBusy(false); } };
  const pendingNavigation = useRef<(() => void) | null>(null);
  const finishClose = () => { const next = pendingNavigation.current; pendingNavigation.current = null; if (next) next(); else onClose(); };
  useLayoutEffect(() => {
    if (!navigationGuard) return;
    navigationGuard.current = next => {
      if (busy || discardAction !== null) return;
      if (dirty) { pendingNavigation.current = next; setDiscardAction("close"); }
      else next();
    };
    return () => { navigationGuard.current = null; };
  }, [navigationGuard, busy, dirty, discardAction]);
  const requestClose = () => { if (!busy && discardAction === null) { if (dirty) setDiscardAction("close"); else finishClose(); } };
  const label = (roleId: string) => roleLabel(roleId, team.roleDefinitions, t);
  const pickModel = (role: RoleProfileV1, selector: string) => {
    const model = choices.get(role.roleId)?.find(item => item.modelSelector === selector);
    setTeam(old => updateProfile(old, role.roleId, { modelSelector: selector, reasoningEffort: role.runtime === "codex" ? (model?.reasoningEfforts.includes("medium") ? "medium" : model?.reasoningEfforts[0] ?? null) : null }));
  };
  const add = () => {
    const next = addWorker(team, t("m5.roles.newRoleName", { number: team.roles.length }));
    if (next === team) return;
    setTeam(next);
    setSelectedRole(next.roles[next.roles.length - 1].roleId);
  };
  const remove = (roleId: string) => { setTeam(old => removeWorker(old, roleId)); setSelectedRole("coordinator"); };
  const content = <>
    {scope !== "app" && <header><div><p className="m5-eyebrow">{t("m5.roles.version", { revision: settings.revision })}</p><h2>{t("m5.shell.roles")}</h2></div><button className="m5-secondary" type="button" disabled={busy || discardAction !== null} onClick={requestClose}>{t(returnToDraft ? "m5.roles.backToDraft" : "m5.roles.back")}</button></header>}
    <div className="m5-settings-body">
    <section className="m5-settings-scope" aria-label={t("m5.roles.scope")}><span>{t(scope === "app" ? "m5.roles.appScope" : "m5.roles.scope")}</span><strong>{projectName}</strong><p>{t(scope === "app" ? "m5.roles.appDefaultsHelp" : "m5.roles.scopeHelp")}</p></section>
    {feedback}
    {dirty && <p role="status">{t("m5.roles.dirty")}</p>}
    {!dirty && savable && <p className="m5-settings-filled">{t("m5.roles.filledNotSaved")}</p>}
    {discardAction && <section role="alertdialog" aria-label={t("m5.roles.discardLabel")}>
      <p>{t("m5.roles.discardQuestion")}</p>
      <button type="button" onClick={() => setDiscardAction(null)}>{t("m5.roles.keep")}</button>
      <button type="button" disabled={disabled || busy} onClick={() => { const action = discardAction; setDiscardAction(null); if (action === "close") finishClose(); else void run(onReload); }}>{t(discardAction === "close" ? "m5.roles.discardClose" : "m5.roles.discardReload")}</button>
    </section>}
    <details className="m5-settings-help"><summary>{t("m5.roles.how")}</summary><p>{t("m5.roles.help")}</p><p>{t("m5.roles.teamHelp")}</p></details>
    {choicesError && <p role="alert">{t("m5.roles.choicesError")}</p>}
    <div className="m5-role-workspace">
    <div className="m5-role-sidebar">
    <nav className="m5-role-navigation" aria-label={t("m5.roles.selectRole")}>{roles.map(role => <button
      key={role.roleId} type="button" aria-pressed={selectedRole === role.roleId} aria-controls={`m5-role-panel-${role.roleId}`}
      onClick={() => setSelectedRole(role.roleId)}><strong>{label(role.roleId)}</strong>
      <span>{role.runtime === "codex" ? "Codex" : "Claude Code"}</span><small className={usable(role) ? undefined : "m5-role-model-warning"}>{usable(role) ? role.modelSelector : t("m5.roles.unselectedModel")}</small></button>)}
    </nav>
    <div className="m5-role-team-tools">
      <button type="button" className="m5-secondary" disabled={editing || !canAddWorker(team)} onClick={add}>{t("m5.roles.add")}</button>
      {roles.some(role => !usable(role)) && <button type="button" className="m5-secondary" disabled={editing || choicesBusy || !window.m5Models} onClick={() => void fillRecommended()}>{t("m5.roles.fillRecommended")}</button>}
      <button type="button" className="m5-secondary" disabled={editing || choicesBusy || !window.m5Models || !roles.some(role => role.roleId === selectedRole)} onClick={() => { const role = roles.find(item => item.roleId === selectedRole); if (role) void loadChoices(role, true); }}>{t(choicesBusy ? "m5.roles.busy" : "m5.roles.loadChoices")}</button>
      {fillMissed && <small role="status">{t("m5.roles.fillMissed")}</small>}
      <small>{t("m5.roles.limit", { max: G5_ROLES_MAX_V3 - 1 })}</small>
    </div>
    </div>
    <div className="m5-role-cards">{roles.map(role => {
      const definition = team.roleDefinitions.find(item => item.roleId === role.roleId);
      const coordinator = role.roleId === "coordinator";
      return <article key={role.roleId} id={`m5-role-panel-${role.roleId}`} hidden={selectedRole !== role.roleId} className="m5-role-card" aria-labelledby={`m5-role-heading-${role.roleId}`}>
      <strong id={`m5-role-heading-${role.roleId}`}>{label(role.roleId)}</strong>
      <label>{t("m5.roles.name")}<input disabled={editing} value={definition?.name ?? ""} aria-invalid={nameBytes(definition?.name ?? "") > G5_ROLE_NAME_MAX_BYTES_V3 || isBlankText(definition?.name ?? "")} onChange={event => define(role.roleId, "name", event.target.value)} /></label>
      {/* The byte budget only matters near the limit (Korean uses 3 bytes a letter);
          the empty cell keeps the two-column card grid aligned. */}
      <small className={nameBytes(definition?.name ?? "") > G5_ROLE_NAME_MAX_BYTES_V3 ? "m5-error" : undefined}>{nameBytes(definition?.name ?? "") > G5_ROLE_NAME_MAX_BYTES_V3 * 0.75 ? t("m5.roles.nameBytes", { count: nameBytes(definition?.name ?? ""), max: G5_ROLE_NAME_MAX_BYTES_V3 }) : ""}</small>
      <label>{t("m5.roles.responsibility")}<textarea disabled={editing} rows={3} value={definition?.responsibility ?? ""} placeholder={t("m5.roles.responsibilityPlaceholder")} onChange={event => define(role.roleId, "responsibility", event.target.value)} /></label>
      <small>{t(coordinator ? "m5.roles.coordinatorHelp" : "m5.roles.responsibilityHelp", { max: G5_ROLE_RESPONSIBILITY_MAX_BYTES_V3 })}</small>
      <label>{t("m5.roles.runtime")}<select disabled={editing} value={role.runtime} onChange={event => {
        choiceEpoch.current++; setChoices(new Map()); setChoicesError(false);
        setTeam(old => updateProfile(old, role.roleId, { runtime: event.target.value as RoleProfileV1["runtime"], modelSelector: "", reasoningEffort: null }));
      }}><option value="codex">Codex</option><option value="claude-code">Claude Code</option></select></label>
      <label>{t("m5.roles.model")}<select disabled={editing || !choices.get(role.roleId)?.length} value={usable(role) ? role.modelSelector : ""} onChange={event => pickModel(role, event.target.value)}>
        <option value="">{t("m5.roles.chooseModel")}</option>
        {usable(role) && !choices.get(role.roleId)?.some(model => model.modelSelector === role.modelSelector) && <option value={role.modelSelector}>{role.modelSelector} · {t("m5.roles.savedModel")}</option>}
        {choices.get(role.roleId)?.map(model => <option key={model.modelSelector} value={model.modelSelector}>{model.modelSelector}{model.resolvedModel ? ` · ${model.resolvedModel}` : ""}{model.isDefault ? ` · ${t("m5.roles.providerDefault")}` : ""}</option>)}
      </select></label>
      <label>{t("m5.roles.reasoning")}<select disabled={editing || !choices.get(role.roleId)} value={role.reasoningEffort ?? ""} onChange={event => change(role.roleId, "reasoningEffort", event.target.value || null)}>
        <option value="" disabled={role.runtime === "codex"}>{t("m5.roles.default")}</option>
        {role.reasoningEffort && !choices.get(role.roleId)?.find(model => model.modelSelector === role.modelSelector)?.reasoningEfforts.includes(role.reasoningEffort) && <option value={role.reasoningEffort}>{role.reasoningEffort} · {t("m5.roles.savedModel")}</option>}
        {choices.get(role.roleId)?.find(model => model.modelSelector === role.modelSelector)?.reasoningEfforts.map(effort => <option key={effort} value={effort}>{effort}</option>)}
      </select></label>
      {coordinator
        ? <span className="m5-role-permission">{t("m5.roles.readOnly")}</span>
        : <label>{t("m5.roles.permission")}<select disabled={editing} value={role.permission} onChange={event => change(role.roleId, "permission", event.target.value)}>
          <option value="readOnly">{t("m5.roles.permissionReadOnly")}</option>
          <option value="workspaceWrite">{t("m5.roles.permissionWrite")}</option>
        </select></label>}
      <details><summary>{t("m5.roles.manualModel")}</summary>
        <label>{t("m5.roles.model")}<input disabled={editing} value={role.modelSelector} onChange={event => { const modelSelector = event.target.value; setTeam(old => updateProfile(old, role.roleId, modelSelector ? { modelSelector } : { modelSelector, reasoningEffort: null })); }} /></label>
        <label>{t("m5.roles.reasoning")}<input disabled={editing} value={role.reasoningEffort ?? ""} onChange={event => change(role.roleId, "reasoningEffort", event.target.value || null)} /></label>
      </details>
      <small className="m5-role-model-help">{t("m5.roles.modelHelp")}</small>
      {!coordinator && <button type="button" className="m5-secondary" disabled={editing || !canRemoveWorker(team, role.roleId)} onClick={() => remove(role.roleId)}>{t("m5.roles.remove")}</button>}
    </article>; })}</div></div>
    {!valid && <p role="alert">{t("m5.roles.invalid")} {t("m5.roles.invalidTeam")}</p>}
    </div>
    <footer className="m5-settings-actions">
    <button type="button" disabled={editing || !valid || !savable} onClick={() => void run(() => onSave(team.roles, team.roleDefinitions))}>{t(busy ? "m5.roles.busy" : scope === "app" ? "m5.roles.appSave" : "m5.roles.save")}</button>
    <button className="m5-secondary" type="button" title={t("m5.roles.reloadHelp")} disabled={editing} onClick={() => dirty ? setDiscardAction("reload") : void run(onReload)}>{t("m5.roles.reload")}</button>
    </footer>
  </>;
  return scope === "app"
    ? <section className="m5-settings m5-settings-inline" aria-label={t("m5.roles.label")}>{content}</section>
    : <dialog ref={dialogRef} className="m5-settings m5-settings-dialog" aria-label={t("m5.roles.label")} onCancel={event => { event.preventDefault(); requestClose(); }}>{content}</dialog>;
}
