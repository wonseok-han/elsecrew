import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type DragEvent, type KeyboardEvent, type ReactNode } from "react";
import { useI18n } from "../i18n";
import type { ChatUiController } from "./chat-state.ts";
import type { ModelChoicesResult } from "./contracts/model-choices-v1.ts";
import { validateProjectCatalogResult } from "./contracts/project-catalog-v1.ts";
import { G5_ROLES_MAX_V3, G5_ROLE_NAME_MAX_BYTES_V3, G5_ROLE_RESPONSIBILITY_MAX_BYTES_V3, isBlankText, roleNameProblemsV3, sameJsonValue, utf8ByteLength as nameBytes, type ProjectRoleSettingsV1, type RoleDefinitionV3, type RoleProfileV1 } from "./contracts/run-recovery-v1.ts";
import { cachedModels, hasUsableModel, loadModels } from "./model-catalog.ts";
import { roleColorIndex, roleInitial } from "./role-colors.ts";
import { isLegacyRoleId, roleLabel } from "./role-names.ts";
import { insertTemplate, ROLE_TEMPLATE_KEYS, templateInTeam, templatePermission, templateText, type RoleTemplateKey } from "./role-templates.ts";
import { looksLikeSecret } from "./secret-shape.ts";
import { correctSettingsDraft, editSettingsDraft, loadSettingsDraft, settingsDraftDirty, settingsDraftSavable } from "./settings-draft.ts";
import { canAddWorker, canRemoveWorker, coordinatorDefaults, duplicateWorker, insertWorker, isValidTeamDraft, moveWorker, newMemberId, removedWorker, removeWorker, restoreWorker, sameTeam, teamChanges, teamDraftFrom, uniqueRoleName, updateDefinition, updateProfile, type RemovedWorker, type TeamDraft } from "./team-draft.ts";

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


type MenuState =
  | { kind: "row"; roleId: string }
  | { kind: "add"; index: number; anchor: string | null }
  | null;

/** The app defaults as a team, or null when they cannot be read. */
async function readAppDefaults(): Promise<{ team: TeamDraft; revision: number } | null> {
  if (!window.m5Projects) return null;
  const request = { action: "getAppRoleDefaults" as const };
  const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
  return result.action === request.action ? { team: teamDraftFrom(result.settings), revision: result.settings.revision } : null;
}

export function RoleSettings({ navigationGuard, settings, disabled, feedback, onSave, onReload, onClose, returnToDraft = false, projectName, scope = "project", activeTasks = 0, onAppDefaultsSaved }: { navigationGuard?: RoleNavigationGuard; scope?: "project" | "app"; settings: NonNullable<ReturnType<ChatUiController["snapshot"]>["settings"]>; activeChat: ReturnType<ChatUiController["snapshot"]>["activeChat"]; disabled: boolean; feedback: ReactNode; onSave: (roles: readonly RoleProfileV1[], roleDefinitions: readonly RoleDefinitionV3[]) => Promise<void>; onReload: () => Promise<void>; onClose: () => void; returnToDraft?: boolean; projectName: string; activeTasks?: number; onAppDefaultsSaved?: () => void }) {
  const { t, lang } = useI18n();
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
  // Removed members, newest last, so a removal can be undone before saving.
  const [removed, setRemoved] = useState<readonly RemovedWorker[]>([]);
  const [menu, setMenu] = useState<MenuState>(null);
  const [announcement, setAnnouncement] = useState("");
  const [dragging, setDragging] = useState<string | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);
  const [focusTarget, setFocusTarget] = useState<{ roleId: string; part: "row" | "name" } | null>(null);
  const [appDefaults, setAppDefaults] = useState<"loaded" | "saved" | "error" | null>(null);
  // Every ID the team had while this screen was open: a new member never takes one (R2).
  const taken = useRef(new Set<string>());
  const rowButtons = useRef(new Map<string, HTMLButtonElement>());
  const menuButtons = useRef(new Map<string, HTMLButtonElement>());
  const nameInput = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  for (const role of roles) taken.current.add(role.roleId);
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
  useEffect(() => {
    setDraft(loadSettingsDraft(initial)); setSelectedRole("coordinator"); setDiscardAction(null);
    setRemoved([]); setMenu(null); setAppDefaults(null);
  }, [initial]);
  // Focus follows a move, a duplicate or a new role.
  useEffect(() => {
    if (!focusTarget) return;
    if (focusTarget.part === "name") nameInput.current?.focus();
    else rowButtons.current.get(focusTarget.roleId)?.focus();
    setFocusTarget(null);
  }, [focusTarget]);
  // A menu closes on a click outside it.
  useEffect(() => {
    if (!menu) return;
    const close = (event: PointerEvent) => {
      if (!(event.target instanceof Element) || !event.target.closest(".m5-team-menu, .m5-team-more, .m5-team-add-button, .m5-team-insert")) setMenu(null);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [menu]);
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
  const workers = roles.length - 1;
  const full = !canAddWorker(team);
  const position = (roleId: string) => roles.findIndex(role => role.roleId === roleId);
  const announce = (text: string) => setAnnouncement(old => old === text ? `${text} ` : text);
  // Every change below edits the draft only; nothing is stored until '저장'.
  const insertBlank = (index: number) => {
    const roleId = newMemberId(taken.current);
    const name = uniqueRoleName(team, t("m5.roles.newRoleName"));
    const next = insertWorker(team, index, { roleId, name, responsibility: "", permission: "readOnly" });
    if (next === team) return;
    setTeam(next); setMenu(null); setSelectedRole(roleId); setFocusTarget({ roleId, part: "name" });
    announce(t("m5.roles.added", { name, position: index + 1 }));
  };
  const insertFromTemplate = (index: number, key: RoleTemplateKey) => {
    const shown = templateLabel(key);
    const result = insertTemplate(team, index, key, lang, shown, taken.current);
    if (result.draft === team) return;
    setTeam(result.draft); setMenu(null); setSelectedRole(result.roleId); setFocusTarget({ roleId: result.roleId, part: "row" });
    announce(t("m5.roles.added", { name: roleLabel(result.roleId, result.draft.roleDefinitions, t), position: index + 1 }));
  };
  const move = (roleId: string, to: number) => {
    const from = position(roleId);
    const next = moveWorker(team, from, to);
    if (next === team) return;
    setTeam(next); setMenu(null); setFocusTarget({ roleId, part: "row" });
    announce(t("m5.roles.moved", { name: label(roleId), from: from + 1, to: next.roles.findIndex(role => role.roleId === roleId) + 1 }));
  };
  const duplicate = (roleId: string) => {
    if (full || roleId === "coordinator") return;
    const newId = newMemberId(taken.current);
    const name = uniqueRoleName(team, t("m5.roles.copyName", { name: label(roleId) }));
    const next = duplicateWorker(team, roleId, name, newId);
    if (next === team) return;
    setTeam(next); setMenu(null); setSelectedRole(newId); setFocusTarget({ roleId: newId, part: "row" });
    announce(t("m5.roles.added", { name, position: next.roles.findIndex(role => role.roleId === newId) + 1 }));
  };
  const remove = (roleId: string) => {
    const entry = removedWorker(team, roleId);
    const next = removeWorker(team, roleId);
    if (!entry || next === team) return;
    const name = label(roleId);
    setTeam(next); setMenu(null); setRemoved(old => [...old, entry]);
    const neighbour = next.roles[Math.min(entry.index, next.roles.length - 1)]!.roleId;
    setSelectedRole(neighbour); setFocusTarget({ roleId: neighbour, part: "row" });
    announce(t("m5.roles.removed", { name }));
  };
  const undoRemove = () => {
    const entry = removed[removed.length - 1];
    if (!entry || full) return;
    const next = restoreWorker(team, entry);
    setRemoved(old => old.slice(0, -1));
    if (next === team) return;
    setTeam(next); setSelectedRole(entry.profile.roleId); setFocusTarget({ roleId: entry.profile.roleId, part: "row" });
  };
  const templateLabel = (key: RoleTemplateKey) => key === "planning" || key === "design" || key === "coding" || key === "review" ? t(`m5.roles.${key}`) : templateText(key, lang).name;
  const rowKeys = (event: KeyboardEvent<HTMLButtonElement>, roleId: string) => {
    const index = position(roleId);
    if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      if (index > 0 && !editing) move(roleId, index + (event.key === "ArrowUp" ? -1 : 1));
    } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "d") {
      event.preventDefault();
      if (!editing) duplicate(roleId);
    } else if (!event.altKey && !event.metaKey && !event.ctrlKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      const next = roles[index + (event.key === "ArrowUp" ? -1 : 1)];
      if (next) rowButtons.current.get(next.roleId)?.focus();
    }
  };
  // Menus: arrows move between items, Escape closes the innermost one only.
  const menuKeys = (event: KeyboardEvent<HTMLDivElement>, returnTo: () => HTMLElement | null | undefined) => {
    const items = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setMenu(null); returnTo()?.focus(); }
    else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      items[(current + (event.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    }
  };
  useEffect(() => {
    if (!menu) return;
    requestAnimationFrame(() => listRef.current?.querySelector<HTMLButtonElement>(".m5-team-menu button:not(:disabled)")?.focus());
  }, [menu]);
  // Drag a member by its row; the drop point is above or below the row's middle.
  const dragOver = (event: DragEvent<HTMLLIElement>, index: number) => {
    if (!dragging) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    const box = event.currentTarget.getBoundingClientRect();
    setDropIndex(Math.max(1, event.clientY < box.top + box.height / 2 ? index : index + 1));
  };
  const drop = (event: DragEvent) => {
    event.preventDefault();
    if (dragging && dropIndex !== null) {
      const from = position(dragging);
      move(dragging, dropIndex > from ? dropIndex - 1 : dropIndex);
    }
    setDragging(null); setDropIndex(null);
  };
  const nameProblems = roleNameProblemsV3(team.roleDefinitions.map(item => item.name));
  const problemOf = (roleId: string) => nameProblems[position(roleId)] ?? null;
  const changes = teamChanges(draft.baseline, team);
  const changeParts = [
    changes.added > 0 && t("m5.roles.changeAdded", { count: changes.added }),
    changes.removed > 0 && t("m5.roles.changeRemoved", { count: changes.removed }),
    changes.moved && t("m5.roles.changeMoved"),
    changes.edited > 0 && t("m5.roles.changeEdited", { count: changes.edited }),
  ].filter(Boolean).join(", ");
  const noWriter = !roles.some(role => role.permission === "workspaceWrite");
  const blankResponsibility = team.roleDefinitions.some(item => item.roleId !== "coordinator" && isBlankText(item.responsibility));
  const loadAppDefaults = async () => {
    setAppDefaults(null);
    try {
      const defaults = await readAppDefaults();
      if (!defaults) { setAppDefaults("error"); return; }
      setTeam(defaults.team); setRemoved([]); setSelectedRole("coordinator"); setAppDefaults("loaded");
    } catch { setAppDefaults("error"); }
  };
  // The app defaults first: if they cannot be saved, the project's team is not saved either.
  const saveWithAppDefaults = async () => {
    setAppDefaults(null);
    try {
      const defaults = await readAppDefaults();
      if (!defaults || !window.m5Projects) { setAppDefaults("error"); return; }
      const request = { action: "updateAppRoleDefaults" as const, expectedRevision: defaults.revision, roles: team.roles, roleDefinitions: team.roleDefinitions };
      validateProjectCatalogResult(await window.m5Projects.request(request), request);
      // Saving the project remounts this screen: the workbench keeps the notice.
      setAppDefaults("saved"); onAppDefaultsSaved?.();
    } catch { setAppDefaults("error"); return; }
    await onSave(team.roles, team.roleDefinitions);
  };
  const selected = roles.find(role => role.roleId === selectedRole) ?? roles[0]!;
  const selectedDefinition = team.roleDefinitions.find(item => item.roleId === selected.roleId);
  const coordinatorSelected = selected.roleId === "coordinator";
  const addMenu = (index: number, anchor: string | null) => <div className="m5-team-menu m5-team-add-menu" role="menu" aria-label={t("m5.roles.addMenu")}
    onKeyDown={event => menuKeys(event, () => anchor ? menuButtons.current.get(anchor) : listRef.current?.querySelector<HTMLButtonElement>(".m5-team-add-button"))}>
    {full && <p className="m5-team-menu-note">{t("m5.roles.full", { max: G5_ROLES_MAX_V3 - 1 })}</p>}
    <p className="m5-team-menu-group">{t("m5.roles.blankGroup")}</p>
    <button type="button" role="menuitem" disabled={full} onClick={() => insertBlank(index)}><strong>{t("m5.roles.blank")}</strong><small>{t("m5.roles.blankHelp")}</small></button>
    <p className="m5-team-menu-group">{t("m5.roles.templateGroup")}</p>
    {ROLE_TEMPLATE_KEYS.map(key => <button key={key} type="button" role="menuitem" disabled={full} onClick={() => insertFromTemplate(index, key)}>
      <strong>{templateLabel(key)}<span className="m5-team-badge" data-permission={templatePermission(key)}>{t(templatePermission(key) === "readOnly" ? "m5.roles.readOnlyBadge" : "m5.roles.writeBadge")}</span></strong>
      <small>{templateInTeam(team, key) ? t("m5.roles.templateInTeam") : t(`m5.roles.template.${key}`)}</small>
    </button>)}
  </div>;
  const rowMenu = (role: RoleProfileV1, index: number) => {
    const coordinator = role.roleId === "coordinator";
    const removable = canRemoveWorker(team, role.roleId);
    return <div className="m5-team-menu" role="menu" aria-label={t("m5.roles.menu", { name: label(role.roleId) })} onKeyDown={event => menuKeys(event, () => menuButtons.current.get(role.roleId))}>
      {!coordinator && <button type="button" role="menuitem" disabled={full} onClick={() => setMenu({ kind: "add", index, anchor: role.roleId })}>{t("m5.roles.insertAbove")}</button>}
      <button type="button" role="menuitem" disabled={full} onClick={() => setMenu({ kind: "add", index: index + 1, anchor: role.roleId })}>{t("m5.roles.insertBelow")}</button>
      {!coordinator && <button type="button" role="menuitem" disabled={full} onClick={() => duplicate(role.roleId)}>{t("m5.roles.duplicate")}<kbd>⌘D</kbd></button>}
      {!coordinator && <button type="button" role="menuitem" disabled={index <= 1} onClick={() => move(role.roleId, index - 1)}>{t("m5.roles.moveUp")}<kbd>⌥↑</kbd></button>}
      {!coordinator && <button type="button" role="menuitem" disabled={index >= roles.length - 1} onClick={() => move(role.roleId, index + 1)}>{t("m5.roles.moveDown")}<kbd>⌥↓</kbd></button>}
      {coordinator && <button type="button" role="menuitem" disabled={sameJsonValue(coordinatorDefaults(), { name: team.roleDefinitions[0]?.name, responsibility: team.roleDefinitions[0]?.responsibility })} onClick={() => { setTeam(old => updateDefinition(old, "coordinator", coordinatorDefaults())); setMenu(null); }}>{t("m5.roles.resetCoordinator")}</button>}
      {!coordinator && <button type="button" role="menuitem" className="m5-danger" disabled={!removable} title={removable ? undefined : t("m5.roles.removeLast")} onClick={() => remove(role.roleId)}>{t("m5.roles.remove")}</button>}
    </div>;
  };
  const insertLine = (index: number) => <button type="button" tabIndex={-1} className="m5-team-insert" data-drop={dropIndex === index ? "true" : undefined} aria-label={t("m5.roles.insertHere")} title={t("m5.roles.insertHere")} disabled={editing || full}
    onClick={() => setMenu({ kind: "add", index, anchor: roles[index - 1]?.roleId ?? null })}><span aria-hidden="true">+</span></button>;
  const responsibilityBytes = nameBytes(selectedDefinition?.responsibility ?? "");
  const selectedName = selectedDefinition?.name ?? "";
  const selectedProblem = problemOf(selected.roleId);
  const content = <>
    {scope !== "app" && <header><div><p className="m5-eyebrow">{t("m5.roles.version", { revision: settings.revision })}</p><h2>{t("m5.shell.roles")}</h2></div><button className="m5-secondary" type="button" disabled={busy || discardAction !== null} onClick={requestClose}>{t(returnToDraft ? "m5.roles.backToDraft" : "m5.roles.back")}</button></header>}
    <div className="m5-settings-body">
    <p className="m5-team-intro">{t("m5.roles.intro")}</p>
    <section className="m5-settings-scope" aria-label={t("m5.roles.scope")}><span>{t(scope === "app" ? "m5.roles.appScope" : "m5.roles.scope")}</span><strong>{projectName}</strong><p>{t(scope === "app" ? "m5.roles.appDefaultsHelp" : "m5.roles.scopeHelp")}</p></section>
    {scope !== "app" && activeTasks > 0 && <p className="m5-team-running" role="status">{t("m5.roles.runningTasks", { count: activeTasks })}</p>}
    {feedback}
    {appDefaults === "loaded" && <p role="status">{t("m5.roles.appDefaultsLoaded")}</p>}
    {appDefaults === "saved" && <p role="status">{t("m5.roles.appDefaultsAlsoSaved")}</p>}
    {appDefaults === "error" && <p role="alert">{t("m5.roles.appDefaultsError")}</p>}
    {!dirty && savable && <p className="m5-settings-filled">{t("m5.roles.filledNotSaved")}</p>}
    {discardAction && <section role="alertdialog" aria-label={t("m5.roles.discardLabel")}>
      <p>{t("m5.roles.discardQuestion")}</p>
      <button type="button" onClick={() => setDiscardAction(null)}>{t("m5.roles.keep")}</button>
      <button type="button" disabled={disabled || busy} onClick={() => { const action = discardAction; setDiscardAction(null); if (action === "close") finishClose(); else void run(onReload); }}>{t(discardAction === "close" ? "m5.roles.discardClose" : "m5.roles.discardReload")}</button>
    </section>}
    <details className="m5-settings-help"><summary>{t("m5.roles.how")}</summary><p>{t("m5.roles.help")}</p><p>{t("m5.roles.teamHelp")}</p></details>
    {choicesError && <p role="alert">{t("m5.roles.choicesError")}</p>}
    {nameProblems.some(Boolean) && <p role="alert" className="m5-error">{t("m5.roles.nameFix")}</p>}
    <div className="m5-team-editor">
    <div className="m5-team-pane" ref={listRef}>
      <p className="m5-team-head"><span>{t("m5.roles.orderHint")}</span><span>{t("m5.roles.count", { count: workers, max: G5_ROLES_MAX_V3 - 1 })}</span></p>
      <ol className="m5-team-list" aria-label={t("m5.roles.selectRole")} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropIndex(null); }}>
        {roles.map((role, index) => {
          const coordinator = role.roleId === "coordinator";
          const name = label(role.roleId);
          const problem = problemOf(role.roleId);
          return <li key={role.roleId} className="m5-team-row" data-selected={selectedRole === role.roleId ? "true" : undefined} data-dragging={dragging === role.roleId ? "true" : undefined}
            draggable={!coordinator && !editing} onDragStart={event => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", role.roleId); setDragging(role.roleId); setMenu(null); }}
            onDragEnd={() => { setDragging(null); setDropIndex(null); }} onDragOver={event => dragOver(event, index)} onDrop={drop}>
            {!coordinator && insertLine(index)}
            <span className="m5-team-handle" aria-hidden="true" title={coordinator ? t("m5.roles.coordinatorLocked") : t("m5.roles.dragHandle")}>{coordinator ? "🔒" : "⋮⋮"}</span>
            <button type="button" className="m5-team-select" ref={element => { if (element) rowButtons.current.set(role.roleId, element); else rowButtons.current.delete(role.roleId); }}
              aria-pressed={selectedRole === role.roleId} aria-controls="m5-role-editor" aria-keyshortcuts={coordinator ? undefined : "Alt+ArrowUp Alt+ArrowDown Meta+D"}
              onClick={() => setSelectedRole(role.roleId)} onKeyDown={event => rowKeys(event, role.roleId)}>
              <span className="m5-role-icon" data-role-color={roleColorIndex(role.roleId)} aria-hidden="true">{roleInitial(name)}</span>
              <span className="m5-team-row-text"><strong><bdi>{name}</bdi></strong><small className={usable(role) ? undefined : "m5-role-model-warning"}>{role.runtime === "codex" ? "Codex" : "Claude Code"} · {usable(role) ? role.modelSelector : t("m5.roles.unselectedModel")}{role.reasoningEffort ? ` · ${role.reasoningEffort}` : ""}</small></span>
            </button>
            {problem && <span className="m5-team-problem" role="img" aria-label={t(`m5.roles.nameProblem.${problem}`)} title={t(`m5.roles.nameProblem.${problem}`)}>!</span>}
            <span className="m5-team-badge" data-permission={role.permission}>{t(role.permission === "readOnly" ? "m5.roles.readOnlyBadge" : "m5.roles.writeBadge")}</span>
            <button type="button" className="m5-team-more" disabled={editing} aria-haspopup="menu" aria-expanded={menu?.kind === "row" && menu.roleId === role.roleId}
              aria-label={t("m5.roles.menu", { name })} ref={element => { if (element) menuButtons.current.set(role.roleId, element); else menuButtons.current.delete(role.roleId); }}
              onClick={() => setMenu(old => old?.kind === "row" && old.roleId === role.roleId ? null : { kind: "row", roleId: role.roleId })}>⋯</button>
            {menu?.kind === "row" && menu.roleId === role.roleId && rowMenu(role, index)}
            {menu?.kind === "add" && menu.anchor === role.roleId && addMenu(menu.index, role.roleId)}
          </li>;
        })}
        <li className="m5-team-end" onDragOver={event => { if (dragging) { event.preventDefault(); setDropIndex(roles.length); } }} onDrop={drop}>{insertLine(roles.length)}</li>
      </ol>
      <div className="m5-team-foot">
        <button type="button" className="m5-secondary m5-team-add-button" disabled={editing} aria-haspopup="menu" aria-expanded={menu?.kind === "add" && menu.anchor === null}
          onClick={() => setMenu(old => old?.kind === "add" && old.anchor === null ? null : { kind: "add", index: roles.length, anchor: null })}>{t("m5.roles.addMenu")} ▾</button>
        <small>{t("m5.roles.moveHint")}</small>
        {menu?.kind === "add" && menu.anchor === null && addMenu(menu.index, null)}
      </div>
      <div className="m5-role-team-tools">
        {roles.some(role => !usable(role)) && <button type="button" className="m5-secondary" disabled={editing || choicesBusy || !window.m5Models} onClick={() => void fillRecommended()}>{t("m5.roles.fillRecommended")}</button>}
        <button type="button" className="m5-secondary" disabled={editing || choicesBusy || !window.m5Models} onClick={() => void loadChoices(selected, true)}>{t(choicesBusy ? "m5.roles.busy" : "m5.roles.loadChoices")}</button>
        {fillMissed && <small role="status">{t("m5.roles.fillMissed")}</small>}
      </div>
      <p className="m5-sr-only" role="status" aria-live="polite">{announcement}</p>
    </div>
    <article id="m5-role-editor" className="m5-role-editor" aria-labelledby="m5-role-editor-heading">
      <header>
        <span className="m5-role-icon" data-role-color={roleColorIndex(selected.roleId)} aria-hidden="true">{roleInitial(label(selected.roleId))}</span>
        <h3 id="m5-role-editor-heading"><bdi>{label(selected.roleId)}</bdi></h3>
        <small>{t(coordinatorSelected ? "m5.roles.originCoordinator" : isLegacyRoleId(selected.roleId) ? "m5.roles.originTemplate" : "m5.roles.originCustom")}</small>
        <details><summary>{t("m5.feedback.details")}</summary><code>{t("m5.roles.idLabel", { id: selected.roleId })}</code></details>
      </header>
      <label className="m5-role-field-wide">{t("m5.roles.name")}<input ref={nameInput} disabled={editing} value={selectedName}
        aria-invalid={nameBytes(selectedName) > G5_ROLE_NAME_MAX_BYTES_V3 || isBlankText(selectedName) || selectedProblem !== null} onChange={event => define(selected.roleId, "name", event.target.value)} /></label>
      {selectedProblem && <p className="m5-error m5-role-field-wide" role="alert">{t(`m5.roles.nameProblem.${selectedProblem}`)}</p>}
      {nameBytes(selectedName) > G5_ROLE_NAME_MAX_BYTES_V3 * 0.75 && <small className={`m5-role-field-wide${nameBytes(selectedName) > G5_ROLE_NAME_MAX_BYTES_V3 ? " m5-error" : ""}`}>{t("m5.roles.nameBytes", { count: nameBytes(selectedName), max: G5_ROLE_NAME_MAX_BYTES_V3 })}</small>}
      <label className="m5-role-field-wide">{t(coordinatorSelected ? "m5.roles.responsibility" : "m5.roles.responsibilityMember")}<textarea disabled={editing} rows={4} value={selectedDefinition?.responsibility ?? ""} placeholder={t("m5.roles.responsibilityPlaceholder")} onChange={event => define(selected.roleId, "responsibility", event.target.value)} /></label>
      <p className="m5-role-field-wide m5-role-text-meta"><small>{t(coordinatorSelected ? "m5.roles.coordinatorHelp" : "m5.roles.responsibilityHelp")}</small><small className={responsibilityBytes > G5_ROLE_RESPONSIBILITY_MAX_BYTES_V3 ? "m5-error" : undefined}>{t("m5.roles.responsibilityBytes", { count: responsibilityBytes, max: G5_ROLE_RESPONSIBILITY_MAX_BYTES_V3 })}</small></p>
      {looksLikeSecret(`${selectedName}\n${selectedDefinition?.responsibility ?? ""}`) && <p className="m5-role-field-wide m5-role-warning" role="alert">{t("m5.roles.secretWarning")}</p>}
      <label>{t("m5.roles.runtime")}<select disabled={editing} value={selected.runtime} onChange={event => {
        choiceEpoch.current++; setChoices(new Map()); setChoicesError(false);
        setTeam(old => updateProfile(old, selected.roleId, { runtime: event.target.value as RoleProfileV1["runtime"], modelSelector: "", reasoningEffort: null }));
      }}><option value="codex">Codex</option><option value="claude-code">Claude Code</option></select></label>
      <label>{t("m5.roles.model")}<select disabled={editing || !choices.get(selected.roleId)?.length} value={usable(selected) ? selected.modelSelector : ""} onChange={event => pickModel(selected, event.target.value)}>
        <option value="">{t("m5.roles.chooseModel")}</option>
        {usable(selected) && !choices.get(selected.roleId)?.some(model => model.modelSelector === selected.modelSelector) && <option value={selected.modelSelector}>{selected.modelSelector} · {t("m5.roles.savedModel")}</option>}
        {choices.get(selected.roleId)?.map(model => <option key={model.modelSelector} value={model.modelSelector}>{model.modelSelector}{model.resolvedModel ? ` · ${model.resolvedModel}` : ""}{model.isDefault ? ` · ${t("m5.roles.providerDefault")}` : ""}</option>)}
      </select></label>
      <label>{t("m5.roles.reasoning")}<select disabled={editing || !choices.get(selected.roleId)} value={selected.reasoningEffort ?? ""} onChange={event => change(selected.roleId, "reasoningEffort", event.target.value || null)}>
        <option value="" disabled={selected.runtime === "codex"}>{t("m5.roles.default")}</option>
        {selected.reasoningEffort && !choices.get(selected.roleId)?.find(model => model.modelSelector === selected.modelSelector)?.reasoningEfforts.includes(selected.reasoningEffort) && <option value={selected.reasoningEffort}>{selected.reasoningEffort} · {t("m5.roles.savedModel")}</option>}
        {choices.get(selected.roleId)?.find(model => model.modelSelector === selected.modelSelector)?.reasoningEfforts.map(effort => <option key={effort} value={effort}>{effort}</option>)}
      </select></label>
      {coordinatorSelected
        ? <div className="m5-role-permission-fixed"><span>{t("m5.roles.permission")}</span><strong>{t("m5.roles.readOnlyBadge")}</strong><small>{t("m5.roles.coordinatorPermission")}</small></div>
        : <fieldset className="m5-role-permissions" disabled={editing}><legend>{t("m5.roles.permission")}</legend>
          {(["readOnly", "workspaceWrite"] as const).map(permission => <label key={permission} data-checked={selected.permission === permission ? "true" : undefined}>
            <input type="radio" name={`m5-permission-${selected.roleId}`} value={permission} checked={selected.permission === permission} onChange={() => change(selected.roleId, "permission", permission)} />
            <span><strong>{t(permission === "readOnly" ? "m5.roles.permissionReadOnlyShort" : "m5.roles.permissionWriteShort")}</strong><small>{t(permission === "readOnly" ? "m5.roles.permissionReadOnlyHelp" : "m5.roles.permissionWriteHelp")}</small></span>
          </label>)}
        </fieldset>}
      <p className="m5-role-field-wide m5-role-permission-note">{t("m5.roles.permissionNote")}</p>
      {coordinatorSelected && <button type="button" className="m5-secondary m5-role-field-wide m5-role-reset" disabled={editing || sameJsonValue(coordinatorDefaults(), { name: selectedDefinition?.name, responsibility: selectedDefinition?.responsibility })}
        onClick={() => setTeam(old => updateDefinition(old, "coordinator", coordinatorDefaults()))}>{t("m5.roles.resetCoordinator")}</button>}
      <details className="m5-role-field-wide"><summary>{t("m5.roles.manualModel")}</summary>
        <label>{t("m5.roles.model")}<input disabled={editing} value={selected.modelSelector} onChange={event => { const modelSelector = event.target.value; setTeam(old => updateProfile(old, selected.roleId, modelSelector ? { modelSelector } : { modelSelector, reasoningEffort: null })); }} /></label>
        <label>{t("m5.roles.reasoning")}<input disabled={editing} value={selected.reasoningEffort ?? ""} onChange={event => change(selected.roleId, "reasoningEffort", event.target.value || null)} /></label>
      </details>
      <small className="m5-role-model-help m5-role-field-wide">{t("m5.roles.modelHelp")}</small>
    </article>
    </div>
    {!valid && !nameProblems.some(Boolean) && <p role="alert">{t("m5.roles.invalid")} {t("m5.roles.invalidTeam")}</p>}
    {valid && (noWriter || blankResponsibility) && <ul className="m5-team-warnings" role="status">
      {noWriter && <li>{t("m5.roles.noWriter")}</li>}
      {blankResponsibility && <li>{t("m5.roles.blankResponsibility")}</li>}
    </ul>}
    </div>
    <footer className="m5-settings-actions m5-team-actions">
    <p className="m5-team-unsaved" role="status">{dirty ? t("m5.roles.unsaved", { parts: changeParts || t("m5.roles.changeEdited", { count: 0 }) }) : ""}</p>
    {removed.length > 0 && <button type="button" className="m5-secondary" disabled={editing || full} onClick={undoRemove}>{t("m5.roles.undoRemove", { name: roleLabel(removed[removed.length - 1]!.profile.roleId, [removed[removed.length - 1]!.definition], t) })}</button>}
    {scope !== "app" && <button type="button" className="m5-secondary" disabled={editing || !window.m5Projects} onClick={() => void run(loadAppDefaults)}>{t("m5.roles.loadApp")}</button>}
    {scope !== "app" && <button type="button" className="m5-secondary" disabled={editing || !valid || !window.m5Projects} onClick={() => void run(saveWithAppDefaults)}>{t("m5.roles.saveWithApp")}</button>}
    <button className="m5-secondary" type="button" title={t("m5.roles.reloadHelp")} disabled={editing} onClick={() => dirty ? setDiscardAction("reload") : void run(onReload)}>{t("m5.roles.reload")}</button>
    <button type="button" className="m5-team-save" disabled={editing || !valid || !savable} onClick={() => void run(() => onSave(team.roles, team.roleDefinitions))}>{t(busy ? "m5.roles.busy" : scope === "app" ? "m5.roles.appSave" : "m5.roles.save")}</button>
    </footer>
  </>;
  return scope === "app"
    ? <section className="m5-settings m5-settings-inline" aria-label={t("m5.roles.label")}>{content}</section>
    : <dialog ref={dialogRef} className="m5-settings m5-settings-dialog" aria-label={t("m5.roles.label")} onCancel={event => { event.preventDefault(); if (menu) { setMenu(null); return; } requestClose(); }}>{content}</dialog>;
}
