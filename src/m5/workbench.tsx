import { submitChatShortcut } from "./chat-submit-shortcut.ts";
import { ProviderUsage } from "./provider-usage.tsx";
import { ProjectRecoveryHistory } from "./project-recovery-history-view.tsx";
import { OrchestrationMessageBody } from "./orchestration-message-body.tsx";
import { PreparationRecovery } from "./preparation-recovery-view";
import { BackupStorageUsageView } from "./backup-storage-usage";
import { flushSync } from "react-dom";
import { AppSettings, type SettingsSection } from "./app-settings";
import { GitSelectionView } from "./git-selection-view.tsx";
import { SaveRecoveryView } from "./save-recovery-view.tsx";
import { chatConnectionTarget, formatChatNotice } from "./chat-notice.ts";
import { AgentOutputPanel } from "./agent-output-panel.tsx";
import { markStartScreenReady } from "./startup-mark.ts";
import { emptyProjectGuide, parseProjectGuide, projectGuideKey, projectGuideStep, type ProjectGuidePreference } from "./project-guide-state.ts";
import type { ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { groupRunHistory, runOutcomeNotice } from "./work-status.ts";
import { cachedModels, loadModels } from "./model-catalog.ts";
import type { ModelChoicesResult } from "./contracts/model-choices-v1.ts";
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api";
import { I18nProvider, useI18n } from "../i18n";

import {
  ProjectEditorController,
  type EditorTab,
  type WorkbenchPanel,
} from "./editor-state.ts";
import type { DiffEntryV1 } from "./contracts/diff-v1.ts";
import type { TreeEntryV1 } from "./contracts/project-v1.ts";
import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";
import { coordinatorRoles, validateRoleProfileV1, type RoleProfileV1, type ProjectRoleSettingsV1 } from "./contracts/run-recovery-v1.ts";
import { StorageAdmissionPoller, type AdmissionView } from "./admission-state.ts";
import { resolveM5Bridge, type SupportedM5Bridge } from "./bridge-boundary.ts";
import { canAbandonAgentRecovery, ChatUiController } from "./chat-state.ts";
import { WorkbenchDock } from "./workbench-dock.tsx";
import { ProviderLogin, type ConnectionReveal } from "./provider-login.tsx";
import { ApprovalCard } from "./approval-card.tsx";
import { ChatWorkspaceDiscard } from "./chat-workspace-discard.tsx";
import { ChatApplicationPreview } from "./chat-application-preview.tsx";
import { diffPages, splitDiffPage, DIFF_PAGE_ROWS } from "./diff-view.ts";
import { FileSearch } from "./file-search-view.tsx";
import { requestEditorSave } from "./editor-actions.ts";
import { languageFor } from "./editor-language.ts";
import { ProjectCatalog } from "./project-catalog-view.tsx";
import { WorkStatusNotice } from "./work-status-view.tsx";
import { validateProjectCatalogResult } from "./contracts/project-catalog-v1.ts";

type MonacoApi = typeof Monaco;

let monacoLoader: Promise<MonacoApi> | undefined;

function loadMonaco(): Promise<MonacoApi> {
  monacoLoader ??= Promise.all([
    import("monaco-editor/esm/vs/editor/editor.api"),
    import("monaco-editor/esm/vs/editor/editor.worker?worker"),
    import("monaco-editor/esm/vs/language/json/json.worker?worker"),
    import("monaco-editor/esm/vs/language/json/monaco.contribution"),
    import("monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution"),
    import("monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution"),
    import("monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution"),
    import("monaco-editor/esm/vs/basic-languages/python/python.contribution"),
    import("monaco-editor/esm/vs/basic-languages/rust/rust.contribution"),
    import("monaco-editor/esm/vs/basic-languages/go/go.contribution"),
    import("monaco-editor/esm/vs/basic-languages/shell/shell.contribution"),
    import("monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution"),
    import("monaco-editor/esm/vs/basic-languages/css/css.contribution"),
    import("monaco-editor/esm/vs/basic-languages/html/html.contribution"),
    import("monaco-editor/esm/vs/basic-languages/sql/sql.contribution"),
    import("monaco-editor/esm/vs/basic-languages/dockerfile/dockerfile.contribution"),
  ]).then(([api, editorWorker, jsonWorker]) => {
    // Monaco 0.52 JSON defaults disable remote schema requests. Keep local workers.
    // Vite emits the worker alongside the local renderer assets; no CDN or eval loader is used.
    (globalThis as typeof globalThis & { MonacoEnvironment?: Monaco.Environment }).MonacoEnvironment = {
      getWorker: (_id, label) => label === "json" ? new jsonWorker.default() : new editorWorker.default(),
    };
    return api;
  }).catch((error: unknown) => {
    monacoLoader = undefined;
    throw error;
  });
  return monacoLoader;
}

/** Poll rather than subscribe: G5D deliberately adds no public storage-admission event. */
function useStorageAdmission(bridge: SupportedM5Bridge): { admission: AdmissionView; fixtureAvailable: boolean } {
  const [admission, setAdmission] = useState<AdmissionView>(() => bridge.version >= 6 ? "checking" : "ready");
  const [fixtureAvailable, setFixtureAvailable] = useState(bridge.version < 6);
  useEffect(() => {
    const v6 = bridge as Partial<M5BridgeV6>;
    const getSystemStatus = v6.getSystemStatus;
    if (bridge.version < 6 || typeof getSystemStatus !== "function") { setAdmission("ready"); setFixtureAvailable(true); return; }
    let active = true;
    setFixtureAvailable(false);
    const poller = new StorageAdmissionPoller(async () => {
      const status = await v6.getSystemStatus!();
      if (active) setFixtureAvailable(status.sidecar.capabilities.includes("run.startFixture") && !status.sidecar.capabilities.includes("run.startAgent"));
      return status;
    }, setAdmission);
    poller.start();
    return () => { active = false; poller.stop(); };
  }, [bridge]);
  return { admission, fixtureAvailable };
}

function StorageAdmissionNotice({ admission }: { admission: AdmissionView }) {
  const { t } = useI18n();
  if (admission === "ready") return null;
  return <section className={admission === "failed" ? "m5-admission m5-admission-failed" : "m5-admission"} role="status">
    <strong>{t(admission === "failed" ? "m5.admission.failed" : "m5.admission.preparing")}</strong>
    <p>{t(admission === "failed" ? "m5.admission.help" : "m5.admission.wait")}</p>
  </section>;
}

export function M5Workbench({ bridge }: { bridge?: unknown }) {
  const candidate = bridge ?? (typeof window === "undefined" ? undefined : window.m5);
  const resolved = resolveM5Bridge(candidate);
  return <I18nProvider>{resolved ? <M5WorkbenchReady bridge={resolved} /> : <WorkbenchUnavailable />}</I18nProvider>;
}

function WorkbenchUnavailable() {
  const { lang, t } = useI18n();
  return <main className="m5-workbench" lang={lang} aria-label={t("m5.shell.label")}>
    <section className="m5-admission m5-admission-failed" role="alert">
      <strong>{t("m5.admission.failed")}</strong>
      <p>{t("m5.admission.help")}</p>
    </section>
  </main>;
}

function M5WorkbenchReady({ bridge }: { bridge: SupportedM5Bridge }) {
  const { lang, t } = useI18n();
  const controller = useMemo(() => new ProjectEditorController(bridge), [bridge]);
  const [railExpanded, setRailExpanded] = useState(true);
  const [catalogRevision, setCatalogRevision] = useState(0);
  const [removal, setRemoval] = useState<{ projectId: string; chatId?: string; label: string } | null>(null);
  const [removing, setRemoving] = useState(false);
  const [removalError, setRemovalError] = useState(false);
  const removalDialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { if (removal) removalDialog.current?.showModal(); else removalDialog.current?.close(); }, [removal]);

  useEffect(() => {
    const menus = () => document.querySelectorAll<HTMLDetailsElement>(".m5-project-menu, .m5-project-guide, .m5-project-overflow, .m5-diff-options, .m5-notifications");
    const outside = (event: PointerEvent) => menus().forEach(menu => { if (!menu.contains(event.target as Node)) menu.open = false; });
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") menus().forEach(menu => { if (menu.open) { menu.open = false; menu.querySelector<HTMLElement>("summary")?.focus(); } }); };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", outside, true); document.removeEventListener("keydown", escape); };
  }, []);
  // Draft edits do not change the tree. Keep its callback stable so memo can
  // skip rebuilding up to 120 rows on every keystroke without ignoring props.
  const loadMoreTree = useCallback(() => {
    const state = controller.snapshot();
    if (state.treeNextCursor) void controller.loadNextTreePage();
    else if (state.error?.messageKey === "m5.editorError.tree") void controller.loadTree(state.treeParent);
  }, [controller]);
  const readTreeBranch = useCallback((path: string, cursor: string | null) => controller.readTreeBranch(path, cursor), [controller]);
  const openTreeEntry = useCallback((entry: TreeEntryV1) => {
    if (entry.kind === "file") void controller.openFile(entry.path);
  }, [controller]);
  const chatController = useMemo(() => new ChatUiController(bridge), [bridge]);
  const { admission, fixtureAvailable } = useStorageAdmission(bridge);
  const admissionReady = admission === "ready";
  const admissionReadyRef = useRef(admissionReady);
  admissionReadyRef.current = admissionReady;
  const [showHiddenFiles, setShowHiddenFiles] = useState(false);
  const [goal, setGoal] = useState("");
  const [hasSavedProjects, setHasSavedProjects] = useState(false);
  const goalInputRef = useRef<HTMLInputElement>(null);
  const newChatDialogRef = useRef<HTMLDialogElement>(null);
  const newChatButtonRef = useRef<HTMLButtonElement>(null);
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [appDefaultsOpen, setAppDefaultsOpen] = useState(false);
  const roleNavigation = useRef<((next: () => void) => void) | null>(null);
  const navigatePreferences = (next: () => void) => roleNavigation.current ? roleNavigation.current(next) : next();
  const [updatePreparing, setUpdatePreparing] = useState(false);
  const [managedProject, setManagedProject] = useState<{ projectId: string; canonicalPath: string } | null>(null);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [preferencesSection, setPreferencesSection] = useState<SettingsSection>("general");
  const [connectionReveal, setConnectionReveal] = useState<ConnectionReveal>();
  const [discardedWorkspaceNotice, setDiscardedWorkspaceNotice] = useState(false);
  const [returnToNewChat, setReturnToNewChat] = useState(false);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const restoreSettingsFocus = useRef(false);
  const [leavingProject, setLeavingProject] = useState(false);
  const [leaveError, setLeaveError] = useState(false);
  const [reloadDraft, setReloadDraft] = useState<EditorTab | null>(null);
  const [reloadingDraft, setReloadingDraft] = useState(false);
  const snapshot = useSyncExternalStore(
    (listener) => controller.subscribe(listener),
    () => controller.snapshot(),
    () => controller.snapshot(),
  );
  const chat = useSyncExternalStore(chatController.subscribe.bind(chatController), () => chatController.snapshot(), () => chatController.snapshot());
  useEffect(() => window.m5AppUpdate?.onRestartCheck(() => {
    const ready = !settingsOpen && !appDefaultsOpen && !newChatOpen && !reloadDraft && !reloadingDraft
      && !leavingProject && !leaveError && controller.canLeaveProject() && chatController.canLeaveProjectView();
    if (ready) flushSync(() => setUpdatePreparing(true));
    return ready;
  }, () => setUpdatePreparing(false)), [settingsOpen, appDefaultsOpen, newChatOpen, reloadDraft, reloadingDraft, leavingProject, leaveError, controller, chatController]);
  const openDiffFile = useCallback((path: string) => { void controller.openFile(path); }, [controller]);
  const discussDiff = useCallback((path: string, side?: "before" | "after", line?: number) => {
    const current = controller.snapshot();
    if (!admissionReady || current.actionBlocked || !current.workspace || current.diffFile?.path !== path) return;
    const reference = side && line !== undefined
      ? t("m5.diff.lineReference", { path: side === "before" ? current.diffFile.oldPath ?? path : path, side: t(`m5.diff.${side}`), line })
      : t("m5.diff.reference", { path });
    if (chatController.addReviewReference(current.workspace, reference)) controller.revealConversation();
  }, [controller, chatController, admissionReady, t]);
  const projectDataReady = Boolean(snapshot.project && chat.projectId === snapshot.project.id && chat.settings?.projectId === snapshot.project.id);
  useEffect(() => {
    if (!admissionReady || snapshot.actionBlocked || snapshot.project) return;
    return markStartScreenReady();
  }, [admissionReady, snapshot.actionBlocked, snapshot.project?.id]);
  useEffect(() => {
    // Run after the close commit and dialog cleanup, when the toolbar button
    // is enabled again. A frame callback from the click can beat that commit.
    if (!settingsOpen && !newChatOpen && restoreSettingsFocus.current) {
      restoreSettingsFocus.current = false;
      settingsButtonRef.current?.focus();
    }
  }, [settingsOpen, newChatOpen]);
  useEffect(() => { setNewChatOpen(false); setReturnToNewChat(false); setGoal(""); }, [snapshot.project?.id]);
  useEffect(() => {
    const dialog = newChatDialogRef.current;
    if (newChatOpen) { dialog?.showModal(); goalInputRef.current?.focus(); }
    else dialog?.close();
    return () => { dialog?.close(); };
  }, [newChatOpen]);
  useEffect(() => {
    controller.start();
    chatController.start();
    return () => { controller.dispose(); chatController.dispose(); };
  }, [controller, chatController]);
  useEffect(() => { if (snapshot.project?.id !== chat.projectId) { if (snapshot.project) void chatController.openProject(snapshot.project.id); } }, [snapshot.project?.id, chat.projectId, chatController]);
  useEffect(() => { controller.setActionBlocked(!admissionReady || settingsOpen || leavingProject || leaveError || chat.switching || chatController.isActionBlocked(), !admissionReady ? { key: "m5.editorNotice.preparing" } : settingsOpen ? { key: "m5.editorNotice.settings" } : chat.switching ? { key: "m5.editorNotice.switching" } : { key: "m5.editorNotice.protected" }); }, [admissionReady, settingsOpen, leavingProject, leaveError, controller, chat.switching, chat.agentRecovery, chat.agentRecoveryResolving, chat.recoveries, chat.activeChat?.state]);
  const leaveProject = async () => {
    const project = controller.snapshot().project;
    if (!project || settingsOpen || !window.m5Projects || !admissionReady || controller.snapshot().actionBlocked || !controller.canLeaveProject() || !chatController.beginLeavingProjectView()) return;
    controller.setActionBlocked(true);
    setLeavingProject(true);
    try {
      const input = { action: "releaseView" as const, projectId: project.id };
      validateProjectCatalogResult(await window.m5Projects.request(input), input);
      chatController.finishLeavingProjectView();
      if (!admissionReadyRef.current || !chatController.canLeaveProjectView() || controller.snapshot().project?.id !== project.id) throw new Error("project leave state changed");
      controller.setActionBlocked(false);
      if (!controller.leaveProjectView()) throw new Error("project view changed during release");
      chatController.clearProjectView();
    } catch {
      setLeaveError(true);
      controller.setActionBlocked(true);
      // Keep chat actions locked when watcher release cannot be confirmed.
    } finally { setLeavingProject(false); }
  };
  const canRemove = admissionReady && !settingsOpen && !newChatOpen && !snapshot.actionBlocked && !snapshot.selectionBlocked && controller.canLeaveProject() && chatController.canLeaveProjectView() && !chat.loadingMoreChats && !removing;
  const changeList = async (target: { projectId: string; chatId?: string }, restore = false) => {
    if (!canRemove || !window.m5Projects) return;
    setRemoving(true); setRemovalError(false);
    const wasCurrent = controller.snapshot().project?.id === target.projectId;
    try {
      if (wasCurrent && !restore && !target.chatId) {
        await leaveProject();
        if (controller.snapshot().project) throw new Error("project view could not be released");
      }
      const input = restore ? { action: "restoreChats" as const, projectId: target.projectId }
        : target.chatId ? { action: "removeChat" as const, projectId: target.projectId, chatId: target.chatId }
        : { action: "removeProject" as const, projectId: target.projectId };
      validateProjectCatalogResult(await window.m5Projects.request(input), input);
      if (!restore && !target.chatId) setCatalogRevision(value => value + 1);
      setRemoval(null);
      if (wasCurrent && (restore || target.chatId)) {
        await chatController.refreshChatList();
        if (target.chatId) controller.revealChatList();
      }
    } catch { setRemovalError(true); }
    finally { setRemoving(false); }
  };
  const switchChat = async (chatId: string) => {
    controller.setActionBlocked(true, { key: "m5.editorNotice.switching" });
    const candidate = await chatController.prepareSwitch(chatId, controller.canLeaveProject());
    if (!candidate) { controller.setActionBlocked(chatController.isActionBlocked()); return; }
    const bound = await controller.bindChat(candidate);
    await chatController.commitPrepared(candidate.chatId, bound);
    controller.setActionBlocked(chatController.isActionBlocked());
  };
  const createChat = async () => {
    if (!snapshot.project) return;
    controller.setActionBlocked(true, { key: "m5.editorNotice.newChat" });
    const created = await chatController.createCoordinatorChat({ projectId: snapshot.project.id }, goal);
    if (created) { setGoal(""); setNewChatOpen(false); await switchChat(created.chatId); if (chatController.snapshot().activeChat?.chatId === created.chatId) await chatController.startAgent("coordinator", lang); }
    else controller.setActionBlocked(chatController.isActionBlocked());
  };

  const activeTab = snapshot.activePath
    ? snapshot.tabs.find((tab) => tab.path === snapshot.activePath) ?? null
    : null;
  const openRoleSettings = (fromNewChat = false) => {
    if (!admissionReady || !projectDataReady || settingsOpen || chat.preparing || chat.switching) return;
    setReturnToNewChat(fromNewChat);
    restoreSettingsFocus.current = false;
    setNewChatOpen(false);
    controller.setActionBlocked(true);
    setSettingsOpen(true);
  };
  const closeRoleSettings = () => {
    setSettingsOpen(false);
    if (returnToNewChat) {
      setReturnToNewChat(false);
      setNewChatOpen(true);
    } else restoreSettingsFocus.current = true;
  };

  return (
    <main inert={updatePreparing} className={`m5-workbench${preferencesOpen ? " m5-settings-open" : ""}${window.m5Projects ? " m5-with-project-rail" : ""}`} lang={lang} aria-label={t("m5.shell.label")}>
      <ProjectCatalog rail railExpanded={railExpanded} onRailExpandedChange={setRailExpanded} refreshKey={catalogRevision} removeDisabled={!canRemove} onRemove={entry => { setRemovalError(false); setRemoval({ projectId: entry.projectId, label: entry.canonicalPath }); }} settingsAction={<button type="button" className="m5-secondary m5-open-preferences" disabled={settingsOpen || newChatOpen || appDefaultsOpen} onClick={() => setPreferencesOpen(true)} title={t("m5.preferences.title")} aria-label={t("m5.preferences.title")}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round"><path d="m10 3-.6 2.4-2 .9L5 5.6 3 9l1.8 1.7v2.6L3 15l2 3.4 2.4-.7 2 .9L10 21h4l.6-2.4 2-.9 2.4.7 2-3.4-1.8-1.7v-2.6L21 9l-2-3.4-2.4.7-2-.9L14 3Z"/><circle cx="12" cy="12" r="3"/></svg><span>{t("m5.preferences.title")}</span></button>} onManage={entry => { setManagedProject(entry); setPreferencesSection("project"); setPreferencesOpen(true); }} onHasProjects={setHasSavedProjects} ready={admissionReady} controller={controller} currentId={snapshot.project?.id} disabled={settingsOpen || !admissionReady || snapshot.actionBlocked || !controller.canLeaveProject() || snapshot.selectionBlocked || Boolean(snapshot.workspace)} />
      <dialog ref={removalDialog} className="m5-discard-dialog" aria-labelledby="m5-remove-title" onCancel={event => { event.preventDefault(); if (!removing) setRemoval(null); }}>
        <h2 id="m5-remove-title">{t("m5.catalog.remove")}</h2>
        <p>{removal?.label}</p><p>{t(removal?.chatId ? "m5.catalog.removeChatHelp" : "m5.catalog.removeHelp")}</p>
        {removalError && <p role="alert">{t("m5.catalog.removeError")}</p>}
        <div className="m5-dialog-actions"><button type="button" disabled={removing} onClick={() => setRemoval(null)}>{t("m5.editor.cancel")}</button>
        <button type="button" className="m5-danger" disabled={!canRemove} onClick={() => removal && void changeList(removal)}>{t("m5.catalog.remove")}</button></div>
      </dialog>
      {!railExpanded && <button type="button" className="m5-sidebar-reopen" onClick={() => setRailExpanded(true)} aria-label={t("m5.catalog.expand")} title={t("m5.catalog.expand")} aria-expanded={false}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg></button>}
      <header className={`m5-toolbar${snapshot.project ? " m5-toolbar-project" : ""}`}>
        <div>
          <div className="m5-toolbar-title">

          <h1>{snapshot.project?.name ?? t(hasSavedProjects ? "m5.home.welcome" : "m5.shell.start")}</h1></div>
          <p className="m5-subtitle">
            {t(snapshot.project ? "m5.shell.active" : "m5.shell.intro")}
          </p>
        </div>
        <div className="m5-toolbar-actions">
      <WorkStatusNotice feedbackTitle={chat.notice ? formatChatNotice(chat, lang) : snapshot.notice ? (snapshot.noticeKey ? t(snapshot.noticeKey) : snapshot.notice) : discardedWorkspaceNotice && !snapshot.project ? t("m5.discard.done") : undefined} hasFeedback={Boolean(chat.notice || snapshot.notice || (discardedWorkspaceNotice && !snapshot.project))} bridge={bridge} ready={admissionReady} refreshKey={JSON.stringify([snapshot.error?.message, chat.notice, chat.activeChat?.state])}
        errorTitle={chat.error ? t(chat.error.messageKey) : snapshot.error ? t(snapshot.error.messageKey) : undefined}
        feedback={<>      {!chat.notice && !snapshot.notice && discardedWorkspaceNotice && !snapshot.project && <p className="m5-notice" role="status">{t("m5.discard.done")}</p>}
      {!chat.notice && snapshot.notice && <p className="m5-notice" role="status">{snapshot.noticeKey ? t(snapshot.noticeKey) : snapshot.notice}</p>}
      {chat.notice && <p className="m5-notice m5-chat-notice" role="status">{formatChatNotice(chat, lang)}{chatConnectionTarget(chat) && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => {
        const provider = chatConnectionTarget(chat);
        if (provider) { setConnectionReveal(current => ({ provider, sequence: (current?.sequence ?? 0) + 1 })); setPreferencesSection("agents"); setPreferencesOpen(true); }
      }}>{t("m5.setup.openConnection", { name: chatConnectionTarget(chat) === "codex" ? "Codex" : "Claude Code" })}</button>}</p>}
</>}
        errors={<>      {snapshot.error && (
        <section className="m5-error" role="alert">
          <strong>{t(snapshot.error.messageKey)}</strong>
          {snapshot.error.messageKey === "m5.editorError.reopenProject" && <button type="button" className="m5-secondary" disabled={settingsOpen || !admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked || Boolean(snapshot.workspace)} onClick={() => void controller.chooseProject()}>{t("m5.editor.reselectOriginal")}</button>}
          <details><summary>{t("m5.feedback.details")}</summary><pre>{snapshot.error.technicalDetail}</pre></details>
        </section>
      )}
      {chat.error && <section className="m5-error" role="alert"><strong>{t(chat.error.messageKey)}</strong><details><summary>{t("m5.feedback.details")}</summary><pre>{chat.error.detail}</pre></details>{chat.activeChat && chat.error.retryChatId === chat.activeChat.chatId && <button type="button" disabled={!admissionReady || chat.switching} onClick={() => void chatController.retryHistory()}>{t("m5.chat.reloadHistory")}</button>}</section>}
</>} />

      {snapshot.project && <ProjectGuide key={snapshot.project.id} hasChat={Boolean(chat.activeChat)}
        projectId={snapshot.project.id} activeChat={chat.activeChat} onResume={id => void switchChat(id)}
        hasOpenFile={snapshot.tabs.length > 0 || snapshot.diffVisible}
        disabled={!admissionReady || !projectDataReady || settingsOpen || snapshot.actionBlocked || chat.preparing || chat.switching}
        onGoal={() => setNewChatOpen(true)} onTeam={() => openRoleSettings()}
        onConversation={() => controller.revealConversation()}
        onChanges={() => { controller.selectPanel("changes"); void controller.refreshChanges(); }} />}

          {snapshot.project && <details className="m5-project-menu"><summary aria-label={t("m5.preferences.projectMenu")} title={t("m5.preferences.projectMenu")}><svg viewBox="0 0 24 24" aria-hidden="true" fill="currentColor"><circle cx="5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="19" cy="12" r="1.8"/></svg></summary><div>          {snapshot.project && <button type="button" onClick={() => void controller.chooseProject()} disabled={settingsOpen || !admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked}>
            {t("m5.shell.switch")}
          </button>}
          {snapshot.project && (
            <button className="m5-secondary" type="button" onClick={() => void leaveProject()} disabled={!admissionReady || settingsOpen || leavingProject || leaveError || !window.m5Projects || snapshot.actionBlocked || !controller.canLeaveProject() || !chatController.canLeaveProjectView()} title={settingsOpen ? t("m5.shell.finishSettings") : undefined}>
              {t("m5.shell.close")}
            </button>
          )}
          {snapshot.project && <button ref={settingsButtonRef} className="m5-secondary" type="button" onClick={() => openRoleSettings()} disabled={!admissionReady || settingsOpen || !projectDataReady || chat.preparing || chat.switching}>{t("m5.shell.roles")}</button>}</div></details>}
        </div>
      </header>

      {chat.activeChat && chat.activeChat.state !== "discarded" && <SaveRecoveryView address={{
        chatId: chat.activeChat.chatId, projectId: chat.activeChat.projectId,
        worktreeId: chat.activeChat.worktreeId, workspaceIdentity: chat.activeChat.workspaceIdentity,
      }} disabled={!admissionReady || chat.switching || settingsOpen || leavingProject || leaveError}
        onResolved={(result, path) => controller.observeCompletedSaveRecovery(result, path)} />}
      {leaveError && <p className="m5-error" role="alert">{t("m5.feedback.leaveError")}</p>}
      <AppSettings hasProject={Boolean(snapshot.project)} storage={<BackupStorageUsageView disabled={!admissionReady} />} project={managedProject ? <><button type="button" className="m5-secondary" onClick={() => setManagedProject(null)}>{t("m5.preferences.projectList")}</button><p>{managedProject.canonicalPath}</p><ProjectRecoveryHistory key={managedProject.projectId} bridge={bridge} projectId={managedProject.projectId} disabled={!admissionReady || leavingProject || leaveError} /><PreparationRecovery projectId={managedProject.projectId} disabled={!admissionReady || snapshot.actionBlocked || snapshot.selectionBlocked || Boolean(snapshot.workspace) || settingsOpen || leavingProject || leaveError || !controller.canLeaveProject() || chat.switching} /></> : <ProjectCatalog management ready={admissionReady && preferencesOpen && preferencesSection === "project"} refreshKey={catalogRevision} controller={controller} disabled={!admissionReady} onManage={setManagedProject} />} restartLocked={updatePreparing} open={preferencesOpen} section={preferencesSection} onSection={section => { if (section !== preferencesSection) navigatePreferences(() => { setPreferencesSection(section); setAppDefaultsOpen(section === "roles"); }); }} onClose={() => navigatePreferences(() => { setPreferencesOpen(false); setPreferencesSection("general"); setAppDefaultsOpen(false); })}
        rolesEditor={appDefaultsOpen && <AppRoleDefaultsEditor activeChat={chat.activeChat} navigationGuard={roleNavigation} disabled={!admissionReady} onClose={() => { setAppDefaultsOpen(false); setPreferencesSection("general"); }} />}
        disabled={!admissionReady || !window.m5Projects} onRoles={() => setAppDefaultsOpen(true)}>
        <ProviderLogin settingsPage disabled={!admissionReady} returning={hasSavedProjects} reveal={connectionReveal} />
      </AppSettings>
      <StorageAdmissionNotice admission={admission} />
      {!snapshot.project && <GuidedStart returning={hasSavedProjects} disabled={!admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked} onChoose={() => void controller.chooseProject()} />}
      {!snapshot.project && hasSavedProjects && <ProjectCatalog home refreshKey={catalogRevision} removeDisabled={!canRemove} onRemove={entry => { setRemovalError(false); setRemoval({ projectId: entry.projectId, label: entry.canonicalPath }); }} onManage={entry => { setManagedProject(entry); setPreferencesSection("project"); setPreferencesOpen(true); }} ready={admissionReady} controller={controller}
        disabled={settingsOpen || !admissionReady || snapshot.actionBlocked || !controller.canLeaveProject() || snapshot.selectionBlocked} />}
      <GitSelectionView disabled={leavingProject || settingsOpen || leaveError} />

      {snapshot.project && !projectDataReady && <section className="m5-notice" role="status">
        {chat.error?.messageKey === "m5.chatError.project" ? <><span>{t("m5.shell.projectRetryHelp")}</span><button type="button" className="m5-secondary" disabled={!admissionReady || settingsOpen} onClick={() => { if (snapshot.project && !chat.activeChat && !chat.settings) void chatController.openProject(snapshot.project.id); }}>{t("m5.feedback.retry")}</button></> : <span>{t("m5.shell.projectLoading")}</span>}
      </section>}

      {snapshot.rescanRequired && (
        <section className="m5-rescan" role="status">
          <span>{t("m5.feedback.overflow")}</span>
          <button type="button" onClick={() => void Promise.all([controller.loadTree(), controller.refreshChanges()])}>{t("m5.feedback.refresh")}</button>
        </section>
      )}

      {snapshot.project && <>
        <dialog ref={newChatDialogRef} className="m5-new-chat-dialog" aria-labelledby="m5-new-chat-heading" onCancel={event => { event.preventDefault(); if (!chat.preparing && !chat.switching) setNewChatOpen(false); }} onClose={() => { if (!settingsOpen) { controller.revealChatList(); requestAnimationFrame(() => newChatButtonRef.current?.focus()); } }}>
        <header><div><p className="m5-eyebrow">{snapshot.project.name}</p><h2 id="m5-new-chat-heading">{t("m5.navigation.newChat")}</h2></div><button type="button" className="m5-secondary" disabled={chat.preparing || chat.switching} onClick={() => setNewChatOpen(false)}>{t("m5.editor.cancel")}</button></header>
        <p>{t("m5.navigation.newChatHelp")}</p>
        {chat.error && <p role="alert" className="m5-error">{t(chat.error.messageKey)}</p>}
        {chat.notice && <p role="status">{formatChatNotice(chat, lang)}</p>}
        <form className="m5-new-chat-form" onKeyDown={submitChatShortcut} onSubmit={(event) => { event.preventDefault(); if (admissionReady && projectDataReady && goal.trim() && !chat.preparing && !chat.switching && !chat.preparationApiUnavailable && !(snapshot.workspace && !controller.canLeaveProject())) void createChat(); }}>
          <input ref={goalInputRef} aria-label={t("m5.navigation.goal")} value={goal} onChange={(event) => setGoal(event.target.value)} placeholder={t("m5.navigation.goalPlaceholder")} disabled={!admissionReady || chat.preparing} />
          <p className="m5-first-chat-note">{t("m5.application.autoBaseline")}</p>
          <button type="submit" title="⌘ Enter / Ctrl Enter" aria-keyshortcuts="Meta+Enter Control+Enter" disabled={!admissionReady || !projectDataReady || !goal.trim() || chat.preparing || chat.preparationApiUnavailable || chat.switching || Boolean(snapshot.workspace && !controller.canLeaveProject())}>{t("m5.chat.coordinatorSend")}</button>
        </form>
        <details className="m5-chat-team-details"><summary>{t("m5.firstChat.team")}</summary>
        {chat.settings && <dl className="m5-new-chat-team">{coordinatorRoles(chat.settings.roles).map(role => <div key={role.roleId}><dt>{t(`m5.roles.${role.roleId}`)}</dt><dd>{role.runtime === "codex" ? "Codex" : "Claude Code"}<small>{role.modelSelector}{role.reasoningEffort ? ` · ${role.reasoningEffort}` : ""}</small></dd></div>)}</dl>}
        <button type="button" className="m5-secondary" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching} onClick={() => openRoleSettings(true)}>{t("m5.firstChat.editTeam")}</button>
        <p>{t("m5.firstChat.editTeamHelp")}</p>
        </details>
        </dialog>
      </>}

      {settingsOpen && chat.settings && <RoleSettings key={`${chat.settings.projectId}:${chat.settings.revision}:${chat.activeChat?.chatId ?? "none"}`} settings={chat.settings} activeChat={chat.activeChat} disabled={!admissionReady} feedback={<>
        {!admissionReady && <StorageAdmissionNotice admission={admission} />}
        {chat.notice && <p className="m5-notice" role="status">{formatChatNotice(chat, lang)}</p>}
        {chat.error && <section className="m5-error" role="alert"><strong>{t(chat.error.messageKey)}</strong><details><summary>{t("m5.feedback.details")}</summary><pre>{chat.error.detail}</pre></details></section>}
      </>} onSave={roles => chatController.updateSettings(roles)} onReload={() => chatController.reloadSettings()} onClose={closeRoleSettings} returnToDraft={returnToNewChat} projectName={snapshot.project?.name ?? chat.settings.projectId} />}

      {snapshot.project && <WorkbenchDock changeCount={snapshot.rescanRequired ? undefined : snapshot.diffSummary?.entries.length} reveal={snapshot.panelRequest} conversation={chat.activeChat ? <div className="m5-conversation"><div className="m5-conversation-scroll"><TeamTimeline onCoordinatorChat={() => { setGoal(chat.activeChat?.goal ?? ""); setNewChatOpen(true); }} controller={chatController} chat={chat} admissionReady={admissionReady} fixtureAvailable={fixtureAvailable} />{chat.activeChat.state === "reviewReady" && <ChatWorkspaceDiscard key={chat.activeChat.chatId} chat={chat.activeChat} disabled={!admissionReady || settingsOpen || !controller.canLeaveProject() || !chatController.canLeaveProjectView()} onCompleted={async discarded => {
        const request = { action: "releaseView" as const, projectId: discarded.projectId };
        validateProjectCatalogResult(await window.m5Projects!.request(request), request);
        if (!controller.leaveProjectView() || !chatController.clearProjectView()) throw new Error("discarded workspace view could not be released");
        setDiscardedWorkspaceNotice(true);
      }} />}</div>{chat.activeChat.schemaVersion === 2 && <FollowupComposer controller={chatController} chat={chat} admissionReady={admissionReady} />}</div> : <section className="m5-first-chat" aria-label={t("m5.firstChat.title")}>
        <h2>{t("m5.firstChat.title")}</h2><p>{t("m5.firstChat.help")}</p>
        <button type="button" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching} onClick={() => setNewChatOpen(true)}>{t("m5.navigation.newChat")}</button>
        <div className="m5-goal-examples">{(["explain", "plan", "readme"] as const).map(example => <button key={example} type="button" className="m5-secondary" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching || goal.trim().length > 0} onClick={() => { setGoal(t(`m5.firstChat.${example}`)); setNewChatOpen(true); }}>
          {t(`m5.firstChat.${example}`)}
        </button>)}</div><p className="m5-first-chat-note">{t("m5.firstChat.exampleHelp")}</p>
        {chat.settings && <details className="m5-chat-team-details"><summary>{t("m5.firstChat.team")}</summary><dl className="m5-team-preview">{coordinatorRoles(chat.settings.roles).map(role => <div key={role.roleId}><dt>{t(`m5.roles.${role.roleId}`)}</dt><dd>{role.runtime === "codex" ? "Codex" : "Claude Code"}<small>{role.modelSelector}{role.reasoningEffort ? ` · ${role.reasoningEffort}` : ""}</small></dd></div>)}</dl><p className="m5-first-chat-note">{t("m5.firstChat.teamHelp")}</p></details>}
      </section>} chats={<section className="m5-chat-navigation" aria-label={t("m5.navigation.chats")}>
        <div className="m5-chat-list">{chat.chats.map(item => {
          const selected = chat.activeChat?.chatId === item.chatId;
          const state = selected ? chat.activeChat!.state : item.state;
          const changed = chat.refreshNeededChatIds.includes(item.chatId);
          const status = changed ? t("m5.navigation.newState") : t(`m5.chat.state.${state}`);
          return <div className="m5-chat-list-row" key={item.chatId}><button type="button" disabled={!admissionReady || settingsOpen || chat.switching} aria-current={selected ? "true" : undefined} title={`${item.goal} · ${status}`} className={selected ? "m5-chat-chip selected" : "m5-chat-chip"} onClick={() => void switchChat(item.chatId)}>
            <span className="m5-chat-goal">{item.goal}</span><span className={changed ? "m5-chat-update" : `m5-chat-status m5-chat-status-${state}`}>{status}</span>
          </button><button type="button" className="m5-chat-remove" title={t("m5.catalog.remove")} aria-label={t("m5.catalog.removeChat", { goal: item.goal })} disabled={!canRemove} onClick={() => { setRemovalError(false); setRemoval({ projectId: item.projectId, chatId: item.chatId, label: item.goal }); }}>×</button></div>;
        })}</div>
        <button type="button" className="m5-chat-restore" disabled={!canRemove} onClick={() => snapshot.project && void changeList({ projectId: snapshot.project.id }, true)}>{t("m5.catalog.restoreChats")}</button>
        {removalError && !removal && <p role="alert">{t("m5.catalog.removeError")}</p>}
        {chat.nextChatCursor !== null && <button className="m5-secondary" type="button" disabled={!admissionReady || chat.loadingMoreChats || removing} onClick={() => void chatController.loadMoreChats()}>{chat.loadingMoreChats ? t("m5.navigation.loadingChats") : t("m5.navigation.moreChats")}</button>}
        <button ref={newChatButtonRef} type="button" className="m5-new-chat-button" disabled={!admissionReady || !projectDataReady || settingsOpen || chat.preparing || chat.switching} onClick={() => setNewChatOpen(true)}>{t("m5.navigation.newChat")}</button>
      </section>}>
        <aside className="m5-panel m5-files" aria-label={t("m5.navigation.files")}>
          {snapshot.project ? (
            <>
              <label className="m5-hidden-files"><input type="checkbox" checked={showHiddenFiles} onChange={event => setShowHiddenFiles(event.target.checked)} />{t("m5.files.showHidden")}</label>
              <FileSearch showHidden={showHiddenFiles} key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}`} disabled={!admissionReady || snapshot.actionBlocked} search={(query, signal) => controller.searchFiles(query, signal)} open={path => { void controller.openFile(path); }} />
              {snapshot.treeParent && <nav className="m5-breadcrumbs" aria-label={t("m5.navigation.folder")}>
                <button className="m5-root" type="button" title={snapshot.project.root} disabled={!admissionReady || snapshot.actionBlocked || snapshot.treeLoading} onClick={() => void controller.loadTree("")}>{snapshot.project.name}</button>
                {snapshot.treeParent.split("/").filter(Boolean).map((part, index, parts) => <button key={index} type="button" aria-current={index === parts.length - 1 ? "location" : undefined} disabled={!admissionReady || snapshot.actionBlocked || snapshot.treeLoading} onClick={() => void controller.loadTree(parts.slice(0, index + 1).join("/"))}>/ {part}</button>)}
              </nav>}
              <VirtualFileList showHidden={showHiddenFiles} key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}:${snapshot.treeParent}`} readBranch={readTreeBranch} rootPage={snapshot.treePage} entries={snapshot.tree} activePath={snapshot.activePath} disabled={!admissionReady || snapshot.actionBlocked} onOpen={openTreeEntry}
                hasMore={Boolean(snapshot.treeNextCursor)} loading={snapshot.treeLoading} failed={snapshot.error?.messageKey === "m5.editorError.tree"} onLoadMore={loadMoreTree} />
            </>
          ) : <Empty text={t("m5.navigation.empty")} />}
        </aside>

        <section className="m5-panel m5-editor" aria-label={t("m5.editor.title")}>
          <div hidden={snapshot.tabs.length === 0 || snapshot.diffVisible} className="m5-tab-list" role="tablist" aria-label={t("m5.editor.tabs")}>
            {snapshot.tabs.map((tab) => (
              <div className="m5-editor-tab" key={tab.path}>
                <button type="button" role="tab" title={tab.path} aria-selected={snapshot.activePath === tab.path} tabIndex={snapshot.activePath === tab.path ? 0 : -1}
                  onFocus={event => event.currentTarget.scrollIntoView({ block: "nearest", inline: "nearest" })}
                  onKeyDown={event => {
                    if (event.altKey || event.ctrlKey || event.metaKey || snapshot.actionBlocked) return;
                    const tabs = Array.from(event.currentTarget.closest(".m5-tab-list")!.querySelectorAll<HTMLButtonElement>('button[role="tab"]:not(:disabled)'));
                    const index = tabs.indexOf(event.currentTarget);
                    const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1
                      : event.key === "ArrowRight" ? (index + 1) % tabs.length
                      : event.key === "ArrowLeft" ? (index + tabs.length - 1) % tabs.length : -1;
                    if (next < 0) return;
                    event.preventDefault();
                    tabs[next]?.focus();
                    tabs[next]?.click();
                  }} onClick={() => controller.openFile(tab.path)} disabled={!admissionReady}>
                  <span className="m5-editor-tab-name">{fileName(tab.path)}</span> {tab.saveState !== "clean" && <span className="m5-editor-tab-dirty" aria-label={t("m5.editor.dirty")}>●</span>}
                </button>
                <button type="button" className="m5-close-tab" aria-label={t("m5.editor.close", { path: tab.path })} onClick={() => controller.closeTab(tab.path)} disabled={!admissionReady}>×</button>
              </div>
            ))}
          </div>
          <div hidden={!activeTab || snapshot.diffVisible} className="m5-editor-head">
            {snapshot.diffVisible && <button type="button" className="m5-secondary" onClick={() => controller.selectPanel("editor")}>{t("m5.editor.title")}</button>}
            <span title={snapshot.diffVisible ? snapshot.diffFile?.path : activeTab?.path}>{(snapshot.diffVisible ? snapshot.diffFile?.path : activeTab?.path) ?? t("m5.editor.select")}</span>
            {!snapshot.diffVisible && activeTab && snapshot.diffSummary?.entries.some(entry => entry.path === activeTab.path) && <button type="button" className="m5-secondary" disabled={!admissionReady || snapshot.actionBlocked} onClick={() => void controller.openDiff(activeTab.path)}>{t("m5.diff.viewSaved")}</button>}
            {!snapshot.diffVisible && activeTab?.mode === "text" && (activeTab.saveState === "dirty" || activeTab.saveState === "conflicted") && <button type="button" className="m5-secondary" disabled={!admissionReady || snapshot.actionBlocked || reloadingDraft} onClick={() => setReloadDraft(activeTab)}>{t("m5.editor.reload")}</button>}
            {!snapshot.diffVisible && activeTab?.mode === "text" && (
              <button type="button" title={t("m5.editor.shortcut")} onClick={() => void controller.saveActive()} disabled={!admissionReady || activeTab.saveState !== "dirty"}>
                {activeTab.saveState === "saving" ? t("m5.editor.saving") : activeTab.saveState === "conflicted" ? t("m5.editor.resolve") : t("m5.editor.save")}
              </button>
            )}
          </div>
          {reloadDraft && <section role="alertdialog" aria-label={t("m5.editor.confirm")} className="m5-error">
            <p><strong>{reloadDraft.path}</strong>{t("m5.editor.discardHelp")}</p>
            <button type="button" disabled={reloadingDraft} onClick={() => setReloadDraft(null)}>{t("m5.editor.cancel")}</button>
            <button type="button" disabled={!admissionReady || snapshot.actionBlocked || reloadingDraft || !snapshot.tabs.includes(reloadDraft)} onClick={async () => {
              setReloadingDraft(true);
              try { await controller.reloadConfirmedDraft(reloadDraft); } finally { setReloadingDraft(false); setReloadDraft(null); }
            }}>{reloadingDraft ? t("m5.editor.reloading") : t("m5.editor.discard")}</button>
            {!snapshot.tabs.includes(reloadDraft) && <p>{t("m5.editor.changed")}</p>}
          </section>}
          <div className="m5-editor-surface" hidden={snapshot.diffVisible}>
          <EditorContent key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}`} tab={activeTab} readOnly={!admissionReady || snapshot.actionBlocked} onChange={(text) => activeTab && controller.updateText(activeTab.path, text)} onSave={(path) => {
            if (admissionReadyRef.current && controller.snapshot().activePath === path) void controller.saveActive();
          }} />
          </div>
              {snapshot.diffVisible && snapshot.diffFile && <DiffDetails onReturn={() => controller.selectPanel("editor")} descriptor={snapshot.diffFile} text={snapshot.diffText} canOpen={admissionReady && !snapshot.actionBlocked} onOpen={openDiffFile}
                canDiscuss={admissionReady && !snapshot.actionBlocked && Boolean(snapshot.workspace) && chatController.canAppendFollowup() && !chat.followups?.[chat.activeChat?.chatId ?? ""]?.request}
                onDiscuss={discussDiff} />}
        </section>

        <aside className="m5-panel m5-changes" aria-label={t("m5.editor.changes")}>
          <PanelTitle active={snapshot.panel === "changes"} panel="changes" onSelect={controller.selectPanel.bind(controller)}>
            {t("m5.editor.changes")} {snapshot.diffSummary ? `(${snapshot.diffSummary.entries.length})` : ""}
          </PanelTitle>
          {snapshot.project ? (
            <>
              <button className="m5-refresh" type="button" onClick={() => void controller.refreshChanges()} disabled={!admissionReady || snapshot.diffLoading}>{t("m5.editor.refresh")}</button>
              {snapshot.diffLoading && <p className="m5-empty" role="status">{t("m5.editor.loadingChanges")}</p>}
              {chat.activeChat && <ChatApplicationPreview key={`${chat.activeChat.chatId}:${chat.activeChat.state}`} projectId={chat.activeChat.projectId} chatId={chat.activeChat.chatId} worktreeId={chat.activeChat.worktreeId} disabled={!admissionReady || chat.switching || chatController.isActionBlocked()} />}
              {snapshot.diffSummary && <ChangeFileList key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}`} entries={snapshot.diffSummary.entries} selected={snapshot.diffFile?.path} disabled={!admissionReady || snapshot.actionBlocked} onOpen={path => { void controller.openDiff(path); }} />}
            </>
          ) : <Empty text={t("m5.editor.chooseProject")} />}
        </aside>
      </WorkbenchDock>}
      <footer className="m5-app-statusbar" aria-label={t("m5.usage.title")}><ProviderUsage disabled={!admissionReady}/></footer>
    </main>
  );
}

const VirtualFileList = memo(function VirtualFileList({ showHidden, entries: rootEntries, readBranch, rootPage, activePath, disabled, onOpen, hasMore, loading, failed, onLoadMore }: { showHidden: boolean; entries: readonly TreeEntryV1[]; rootPage: number; readBranch: (path: string, cursor: string | null) => Promise<{entries: readonly TreeEntryV1[]; nextCursor: string | null}>; activePath: string | null; disabled: boolean; onOpen: (entry: TreeEntryV1) => void; hasMore: boolean; loading: boolean; failed: boolean; onLoadMore: () => void }) {
  type Branch = { entries: readonly TreeEntryV1[]; cursor: string | null; open: boolean; loading: boolean; failed: boolean };
  const [branches, setBranches] = useState<Record<string, Branch>>({});
  const branchesRef = useRef(branches);
  const epoch = useRef(0);
  const updateBranches = (next: Record<string, Branch>) => { branchesRef.current = next; setBranches(next); };
  useEffect(() => { if (rootPage === 1) { epoch.current++; updateBranches({}); } }, [rootEntries, rootPage]);
  useEffect(() => () => { epoch.current++; }, []);
  const loadBranch = async (path: string) => {
    const previous = branchesRef.current[path];
    if (disabled || previous?.loading) return;
    const generation = epoch.current;
    updateBranches({ ...branchesRef.current, [path]: { ...(previous ?? { entries: [], cursor: null, open: true }), loading: true, failed: false } });
    try {
      const page = await readBranch(path, previous?.cursor ?? null);
      if (generation !== epoch.current) return;
      updateBranches({ ...branchesRef.current, [path]: { ...branchesRef.current[path], entries: [...(previous?.entries ?? []), ...page.entries], cursor: page.nextCursor, loading: false, failed: false } });
    } catch {
      if (generation !== epoch.current) return;
      updateBranches({ ...branchesRef.current, [path]: { ...branchesRef.current[path], loading: false, failed: true } });
    }
  };
  const toggle = (entry: TreeEntryV1) => {
    if (entry.kind !== "directory") { onOpen(entry); return; }
    const branch = branchesRef.current[entry.path];
    if (!branch) { void loadBranch(entry.path); return; }
    updateBranches({ ...branchesRef.current, [entry.path]: { ...branch, open: !branch.open } });
  };
  type Row = TreeEntryV1 & { depth: number; moreFor?: string };
  const entries: Row[] = [];
  const append = (children: readonly TreeEntryV1[], depth: number) => {
    for (const entry of children) {
      if (!showHidden && entry.name.startsWith(".")) continue;
      entries.push({ ...entry, depth });
      const branch = branches[entry.path];
      if (entry.kind === "directory" && branch?.open) {
        append(branch.entries, depth + 1);
        if (branch.cursor || branch.loading || branch.failed) entries.push({ path: entry.path + "/", name: "", kind: "file", depth: depth + 1, moreFor: entry.path });
      }
    }
  };
  append(rootEntries, 0);
  const [scrollTop, setScrollTop] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const { t } = useI18n();
  const sentinelRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const root = listRef.current, sentinel = sentinelRef.current;
    if (!root || !sentinel || disabled || loading || failed || !hasMore) return;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting && root.clientHeight > 0) onLoadMore();
    }, { root, rootMargin: "0px 0px 160px 0px" });
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [disabled, loading, failed, hasMore, entries.length, onLoadMore]);
  const rowHeight = 32;
  const start = Math.min(Math.max(0, entries.length - 1), Math.max(0, Math.floor(scrollTop / rowHeight) - 8));
  const end = Math.min(entries.length, start + 120);
  return <div ref={listRef} className="m5-file-list m5-virtual-file-list" aria-busy={loading} onScroll={event => setScrollTop(event.currentTarget.scrollTop)} onKeyDown={event => {
    if (disabled || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const current = Number((event.target as HTMLElement).closest<HTMLButtonElement>("button[data-index]")?.dataset.index);
    if (!Number.isInteger(current)) return;
    event.preventDefault();
    const direction = event.key === "ArrowUp" || event.key === "End" ? -1 : 1;
    let next = event.key === "Home" ? 0 : event.key === "End" ? entries.length - 1 : current + direction;
    while (next >= 0 && next < entries.length && (entries[next].kind === "symlink" || entries[next].moreFor)) next += direction;
    if (next < 0 || next >= entries.length) return;
    const list = listRef.current;
    if (!list) return;
    if (next * rowHeight < list.scrollTop) list.scrollTop = next * rowHeight;
    else if ((next + 1) * rowHeight > list.scrollTop + list.clientHeight) list.scrollTop = (next + 1) * rowHeight - list.clientHeight;
    setScrollTop(list.scrollTop);
    requestAnimationFrame(() => list.querySelector<HTMLButtonElement>(`button[data-index="${next}"]`)?.focus());
  }}>
    <div aria-hidden="true" style={{ height: start * rowHeight }} />
    {entries.slice(start, end).map((entry, index) => entry.moreFor ? <TreeBranchMore key={entry.path} root={listRef} loading={branches[entry.moreFor].loading} failed={branches[entry.moreFor].failed} disabled={disabled} depth={entry.depth} onLoad={() => void loadBranch(entry.moreFor!)} /> : <button key={entry.path} data-index={start + index} type="button" className="m5-file-row" data-kind={entry.kind} aria-current={entry.kind === "file" && activePath === entry.path ? "true" : undefined} title={entry.path} aria-expanded={entry.kind === "directory" ? Boolean(branches[entry.path]?.open) : undefined} style={{ paddingLeft: 7 + entry.depth * 16 }} onClick={() => toggle(entry)} disabled={disabled || entry.kind === "symlink"}>
      <span className="m5-tree-chevron" aria-hidden="true">{entry.kind === "directory" ? (branches[entry.path]?.open ? "⌄" : "›") : ""}</span>
      <svg className="m5-file-glyph" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">{entry.kind === "directory" ? <path d="M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Z" /> : <><path d="M14 3H5v18h14V8l-5-5Z M14 3v5h5" />{entry.kind === "symlink" ? <path d="m8 16 7-5m-4 0h4v4" /> : <path d="M8 13h8M8 17h6" />}</>}</svg>
      <span className="m5-tree-name">{entry.name}</span>
    </button>)}
    <div aria-hidden="true" style={{ height: (entries.length - end) * rowHeight }} />
    <div ref={sentinelRef} className="m5-tree-more">
      {loading ? <span role="status">{t("m5.navigation.loading")}</span> : failed ? <button type="button" disabled={disabled} onClick={onLoadMore}>{t("m5.navigation.retry")}</button> : null}
    </div>
  </div>;
});

function TreeBranchMore({ root, loading, failed, disabled, depth, onLoad }: { root: React.RefObject<HTMLDivElement | null>; loading: boolean; failed: boolean; disabled: boolean; depth: number; onLoad: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const { t } = useI18n();
  useEffect(() => {
    if (!ref.current || !root.current || loading || failed || disabled) return;
    const observer = new IntersectionObserver(([entry]) => { if (entry.isIntersecting && root.current!.clientHeight > 0) onLoad(); }, {root: root.current, rootMargin: "160px"});
    observer.observe(ref.current);
    return () => observer.disconnect();
  }, [loading, failed, disabled, onLoad, root]);
  return <div ref={ref} className="m5-tree-branch-more" style={{paddingLeft: 7 + depth * 16}}>{failed ? <button disabled={disabled} onClick={onLoad}>{t("m5.navigation.retry")}</button> : loading ? <span role="status">{t("m5.navigation.loading")}</span> : null}</div>;
}

function ProjectGuide({ projectId, activeChat, onResume, hasChat, hasOpenFile, disabled, onGoal, onTeam, onConversation, onChanges }: {
  projectId: string; activeChat: ChatRecordV1 | null; onResume: (chatId: string) => void;
  hasChat: boolean; hasOpenFile: boolean; disabled: boolean; onGoal: () => void; onTeam: () => void; onConversation: () => void; onChanges: () => void;
}) {
  const { t } = useI18n();
  const [preference, setPreference] = useState(() => { try { return parseProjectGuide(localStorage.getItem(projectGuideKey(projectId))); } catch { return emptyProjectGuide(); } });
  const [saveFailed, setSaveFailed] = useState(false);
  const working = hasOpenFile || hasChat;
  const [open, setOpen] = useState(false);
  const savePreference = (next: ProjectGuidePreference) => {
    setPreference(next);
    try { localStorage.setItem(projectGuideKey(projectId), JSON.stringify(next)); setSaveFailed(false); return true; } catch { setSaveFailed(true); return false; }
  };
  // Collapse only on entering work, not on each file/chat update. A manually
  // reopened guide stays open, and safety/recovery notices live outside it.
  useEffect(() => { if (working) setOpen(false); }, [working]);
  useEffect(() => {
    if (activeChat && !preference.chatId) {
      savePreference({ ...preference, chatId: activeChat.chatId, teamReviewed: true });
    }
  }, [activeChat?.chatId, preference.chatId]);
  const step = projectGuideStep(preference, activeChat?.chatId ?? null, activeChat?.state ?? null);
  const beginGoal = () => { savePreference({ ...preference, teamReviewed: true }); onGoal(); };
  return <details className="m5-project-guide" open={open} onToggle={event => setOpen(event.currentTarget.open)}>
    <summary title={t("m5.preferences.guide")}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6"><circle cx="12" cy="12" r="9"/><path d="M9.5 9a2.5 2.5 0 1 1 4 2l-1.5 1v2m0 2v1"/></svg><span>{t("m5.preferences.guide")}</span></summary><div className="m5-guide-popover">
    <section className="m5-guide-next" aria-label={t("m5.guide.next")}>
      <strong>{t(`m5.guide.step.${step}`)}</strong>
      {step !== "resume" && activeChat && <p>{activeChat.goal} · {t(`m5.chat.state.${activeChat.state}`)}</p>}
      <p>{t(`m5.guide.help.${step}`)}</p>
      {step === "team" && <button type="button" disabled={disabled} onClick={onTeam}>{t("m5.firstChat.editTeam")}</button>}
      {(step === "team" || step === "goal") && <button type="button" disabled={disabled} onClick={beginGoal}>{t("m5.guide.writeGoal")}</button>}
      {step === "resume" && <button type="button" disabled={disabled} onClick={() => preference.chatId && onResume(preference.chatId)}>{t("m5.guide.resume")}</button>}
      {step === "resume" && <button type="button" className="m5-secondary" disabled={disabled} onClick={() => savePreference({ ...emptyProjectGuide(), chatId: activeChat?.chatId ?? null, teamReviewed: Boolean(activeChat) })}>{t("m5.guide.reset")}</button>}
      {(step === "observe" || step === "attention" || step === "recovery") && <button type="button" disabled={disabled} onClick={onConversation}>{t("m5.guide.openConversation")}</button>}
      {step === "review" && <><button type="button" disabled={disabled} onClick={onChanges}>{t("m5.guide.openChanges")}</button><button type="button" disabled={disabled} onClick={onConversation}>{t("m5.guide.openDecision")}</button></>}
      <button type="button" className="m5-secondary" onClick={() => { if (savePreference({ ...preference, dismissed: true })) setOpen(false); }}>{t("m5.guide.later")}</button>
      {saveFailed && <p role="status">{t("m5.guide.saveFailed")}</p>}
    </section>
    <details><summary>{t("m5.guide.allSteps")}</summary><ol>
      <li><h3>{t("m5.guide.goal")}</h3><p>{t("m5.guide.goalHelp")}</p><button type="button" disabled={disabled} onClick={onGoal}>{t("m5.navigation.newChat")}</button><button type="button" className="m5-secondary" disabled={disabled} onClick={onTeam}>{t("m5.firstChat.editTeam")}</button></li>
      <li><h3>{t("m5.guide.observe")}</h3><p>{t("m5.guide.observeHelp")}</p><button type="button" className="m5-secondary" disabled={disabled || !hasChat} onClick={onConversation}>{t("m5.guide.openConversation")}</button></li>
      <li><h3>{t("m5.guide.review")}</h3><p>{t("m5.guide.reviewHelp")}</p><button type="button" className="m5-secondary" disabled={disabled || !hasChat} onClick={onChanges}>{t("m5.guide.openChanges")}</button><button type="button" className="m5-secondary" disabled={disabled || !hasChat} onClick={onConversation}>{t("m5.guide.openDecision")}</button></li>
    </ol></details><p>{t("m5.guide.safety")}</p></div>
  </details>;
}

function GuidedStart({ disabled, onChoose, returning }: { disabled: boolean; onChoose: () => void; returning: boolean }) {
  const { t } = useI18n();
  return <section className={`m5-guided-start${returning ? " m5-guided-returning" : ""}`} aria-label={t("m5.start.title")}>
    <div className="m5-start-heading"><h2>{t(returning ? "m5.home.continue" : "m5.start.title")}</h2><p>{t(returning ? "m5.home.continueHelp" : "m5.start.help")}</p></div>
    {!returning && <ol className="m5-start-steps">{(["connect", "project", "goal"] as const).map((step, index) => <li key={step}>
      <span className="m5-start-number" aria-hidden="true">{index + 1}</span><div><h3>{t(`m5.start.${step}`)}</h3><p>{t(`m5.start.${step}Help`)}</p></div>
    </li>)}</ol>}
    <div className="m5-start-action"><button type="button" onClick={onChoose} disabled={disabled}>{t("m5.shell.choose")}</button><p>{t("m5.start.projectLimit")}</p></div>
  </section>;
}

type RoleNavigationGuard = { current: ((next: () => void) => void) | null };

function AppRoleDefaultsEditor({ disabled, onClose, navigationGuard, activeChat }: { activeChat: ReturnType<ChatUiController["snapshot"]>["activeChat"]; disabled: boolean; onClose: () => void; navigationGuard: RoleNavigationGuard }) {
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
  const save = async (roles: readonly RoleProfileV1[]) => {
    if (!settings) return;
    const epoch = ++generation.current;
    setError(false); setSaved(false);
    try {
      const request = { action: "updateAppRoleDefaults" as const, expectedRevision: settings.revision, roles };
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

function RoleSettings({ navigationGuard, settings, disabled, feedback, onSave, onReload, onClose, returnToDraft = false, projectName, scope = "project" }: { navigationGuard?: RoleNavigationGuard; scope?: "project" | "app"; settings: NonNullable<ReturnType<ChatUiController["snapshot"]>["settings"]>; activeChat: ReturnType<ChatUiController["snapshot"]>["activeChat"]; disabled: boolean; feedback: ReactNode; onSave: (roles: readonly RoleProfileV1[]) => Promise<void>; onReload: () => Promise<void>; onClose: () => void; returnToDraft?: boolean; projectName: string }) {
  const { t } = useI18n();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [roles, setRoles] = useState<readonly RoleProfileV1[]>(coordinatorRoles(settings.roles));
  const [selectedRole, setSelectedRole] = useState<RoleProfileV1["roleId"]>("coordinator");
  const [busy, setBusy] = useState(false);
  const [discardAction, setDiscardAction] = useState<"close" | "reload" | null>(null);
  const [choices, setChoices] = useState<Partial<Record<RoleProfileV1["roleId"], ModelChoicesResult["models"]>>>({});
  const [choicesError, setChoicesError] = useState(false);
  const [choicesBusy, setChoicesBusy] = useState(false);
  const choiceEpoch = useRef(0);
  useEffect(() => {
    choiceEpoch.current += 1;
    setChoices({}); setChoicesError(false);
    return () => { choiceEpoch.current += 1; };
  }, [settings]);
  const loadChoices = async (role: RoleProfileV1, refresh = false) => {
    if (!window.m5Models) return;
    const epoch = ++choiceEpoch.current;
    const cached = cachedModels(role.runtime);
    if (cached) setChoices(old => ({ ...old, [role.roleId]: cached }));
    setChoicesBusy(true); setChoicesError(false);
    try {
      const result = await loadModels(role.runtime, input => window.m5Models!.request(input), refresh);
      if (epoch !== choiceEpoch.current) return;
      setChoices(old => ({ ...old, [role.roleId]: result.models }));
      const defaults = result.models.filter(model => model.isDefault);
      if (defaults.length === 1) setRoles(old => old.map(item => {
        const initialDefault = scope === "app" && settings.revision === 1 && coordinatorRoles(settings.roles).some(initial => JSON.stringify(initial) === JSON.stringify(item));
        return item.runtime === role.runtime && ((!item.modelSelector && item.roleId === role.roleId) || initialDefault)
          ? { ...item, modelSelector: defaults[0].modelSelector, reasoningEffort: role.runtime === "codex" ? (defaults[0].reasoningEfforts.includes("medium") ? "medium" : defaults[0].reasoningEfforts[0] ?? null) : null } : item;
      }));
    } catch { if (epoch === choiceEpoch.current) setChoicesError(true); }
    finally { if (epoch === choiceEpoch.current) setChoicesBusy(false); }
  };
  const selectedRuntime = roles.find(role => role.roleId === selectedRole)?.runtime;
  useEffect(() => {
    const role = roles.find(item => item.roleId === selectedRole);
    if (role) void loadChoices(role);
    return () => { choiceEpoch.current++; };
  }, [selectedRole, selectedRuntime, settings]);
  const dirty = JSON.stringify(roles) !== JSON.stringify(coordinatorRoles(settings.roles));
  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => { dialog?.close(); };
  }, []);
  useEffect(() => { setRoles(coordinatorRoles(settings.roles)); setDiscardAction(null); }, [settings]);
  const change = (index: number, field: keyof RoleProfileV1, value: string | null) => setRoles((old) => old.map((role, current) => current === index ? { ...role, [field]: value } : role));
  let valid = roles.length === 5;
  try { roles.forEach(role => { validateRoleProfileV1(role); if (!role.modelSelector.trim()) valid = false; }); } catch { valid = false; }
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
  const content = <>
    {scope !== "app" && <header><div><p className="m5-eyebrow">{t("m5.roles.version", { revision: settings.revision })}</p><h2>{t("m5.shell.roles")}</h2></div><button className="m5-secondary" type="button" disabled={busy || discardAction !== null} onClick={requestClose}>{t(returnToDraft ? "m5.roles.backToDraft" : "m5.roles.back")}</button></header>}
    <div className="m5-settings-body">
    <section className="m5-settings-scope" aria-label={t("m5.roles.scope")}><span>{t(scope === "app" ? "m5.roles.appScope" : "m5.roles.scope")}</span><strong>{projectName}</strong><p>{t(scope === "app" ? "m5.roles.appDefaultsHelp" : "m5.roles.scopeHelp")}</p></section>
    {feedback}
    {dirty && <p role="status">{t("m5.roles.dirty")}</p>}
    {discardAction && <section role="alertdialog" aria-label={t("m5.roles.discardLabel")}>
      <p>{t("m5.roles.discardQuestion")}</p>
      <button type="button" onClick={() => setDiscardAction(null)}>{t("m5.roles.keep")}</button>
      <button type="button" disabled={disabled || busy} onClick={() => { const action = discardAction; setDiscardAction(null); if (action === "close") finishClose(); else void run(onReload); }}>{t(discardAction === "close" ? "m5.roles.discardClose" : "m5.roles.discardReload")}</button>
    </section>}
    <details className="m5-settings-help"><summary>{t("m5.roles.how")}</summary><p>{t("m5.roles.help")}</p></details>
    {choicesError && <p role="alert">{t("m5.roles.choicesError")}</p>}
    <div className="m5-role-workspace">
    <nav className="m5-role-navigation" aria-label={t("m5.roles.selectRole")}>{roles.map(role => <button
      key={role.roleId} type="button" aria-pressed={selectedRole === role.roleId} aria-controls={`m5-role-panel-${role.roleId}`}
      onClick={() => setSelectedRole(role.roleId)}><strong>{t(`m5.roles.${role.roleId}`)}</strong>
      <span>{role.runtime === "codex" ? "Codex" : "Claude Code"}</span><small>{role.modelSelector || t("m5.roles.chooseModel")}</small></button>)}</nav>
    <div className="m5-role-cards">{roles.map((role, index) => <article key={role.roleId} id={`m5-role-panel-${role.roleId}`} hidden={selectedRole !== role.roleId} className="m5-role-card" aria-labelledby={`m5-role-heading-${role.roleId}`}>
      <strong id={`m5-role-heading-${role.roleId}`}>{t(`m5.roles.${role.roleId}`)}</strong>
      <label>{t("m5.roles.runtime")}<select disabled={disabled || busy || discardAction !== null} value={role.runtime} onChange={event => {
        choiceEpoch.current++; setChoices({}); setChoicesError(false);
        setRoles(old => old.map((item, current) => current === index ? { ...item, runtime: event.target.value as RoleProfileV1["runtime"], modelSelector: "", reasoningEffort: null } : item));
      }}><option value="codex">Codex</option><option value="claude-code">Claude Code</option></select></label>
      <label>{t("m5.roles.model")}<select disabled={disabled || busy || discardAction !== null || !choices[role.roleId]?.length} value={role.modelSelector} onChange={event => {
        const model = choices[role.roleId]?.find(item => item.modelSelector === event.target.value);
        change(index, "modelSelector", event.target.value);
        change(index, "reasoningEffort", role.runtime === "codex" ? (model?.reasoningEfforts.includes("medium") ? "medium" : model?.reasoningEfforts[0] ?? null) : null);
      }}>
        <option value="">{t("m5.roles.chooseModel")}</option>
        {role.modelSelector && !choices[role.roleId]?.some(model => model.modelSelector === role.modelSelector) && <option value={role.modelSelector}>{role.modelSelector} · {t("m5.roles.savedModel")}</option>}
        {choices[role.roleId]?.map(model => <option key={model.modelSelector} value={model.modelSelector}>{model.modelSelector}{model.isDefault ? ` · ${t("m5.roles.providerDefault")}` : ""}</option>)}
      </select></label>
      <label>{t("m5.roles.reasoning")}<select disabled={disabled || busy || discardAction !== null || !choices[role.roleId]} value={role.reasoningEffort ?? ""} onChange={event => change(index, "reasoningEffort", event.target.value || null)}>
        <option value="" disabled={role.runtime === "codex"}>{t("m5.roles.default")}</option>
        {role.reasoningEffort && !choices[role.roleId]?.find(model => model.modelSelector === role.modelSelector)?.reasoningEfforts.includes(role.reasoningEffort) && <option value={role.reasoningEffort}>{role.reasoningEffort} · {t("m5.roles.savedModel")}</option>}
        {choices[role.roleId]?.find(model => model.modelSelector === role.modelSelector)?.reasoningEfforts.map(effort => <option key={effort} value={effort}>{effort}</option>)}
      </select></label>
      <span className="m5-role-permission">{t(role.permission === "readOnly" ? "m5.roles.readOnly" : "m5.roles.write")}</span>
      <div className="m5-role-model-tools">
        <button type="button" className="m5-secondary" disabled={disabled || busy || choicesBusy || discardAction !== null || !window.m5Models} onClick={() => void loadChoices(role, true)}>{t(choicesBusy ? "m5.roles.busy" : "m5.roles.loadChoices")}</button>
        {choices[role.roleId] && <small>{t("m5.roles.choicesHelp")}</small>}
      </div>
      <details><summary>{t("m5.roles.manualModel")}</summary>
        <label>{t("m5.roles.model")}<input disabled={disabled || busy || discardAction !== null} value={role.modelSelector} onChange={event => change(index, "modelSelector", event.target.value)} /></label>
        <label>{t("m5.roles.reasoning")}<input disabled={disabled || busy || discardAction !== null} value={role.reasoningEffort ?? ""} onChange={event => change(index, "reasoningEffort", event.target.value || null)} /></label>
      </details>
      <small className="m5-role-model-help">{t("m5.roles.modelHelp")}</small>
    </article>)}</div></div>
    {!valid && <p role="alert">{t("m5.roles.invalid")}</p>}
    </div>
    <footer className="m5-settings-actions">
    <button type="button" disabled={disabled || busy || !valid || discardAction !== null} onClick={() => void run(() => onSave(roles))}>{t(busy ? "m5.roles.busy" : scope === "app" ? "m5.roles.appSave" : "m5.roles.save")}</button>
    <button className="m5-secondary" type="button" disabled={disabled || busy || discardAction !== null} onClick={() => dirty ? setDiscardAction("reload") : void run(onReload)}>{t("m5.roles.reload")}</button>
    <details className="m5-settings-reload-help"><summary>{t("m5.roles.reloadInfo")}</summary><p>{t("m5.roles.reloadHelp")}</p></details>
    </footer>
  </>;
  return scope === "app"
    ? <section className="m5-settings m5-settings-inline" aria-label={t("m5.roles.label")}>{content}</section>
    : <dialog ref={dialogRef} className="m5-settings m5-settings-dialog" aria-label={t("m5.roles.label")} onCancel={event => { event.preventDefault(); requestClose(); }}>{content}</dialog>;
}

function FollowupComposer({ controller, chat, admissionReady }: { controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean }) {
  const { t, lang } = useI18n();
  const draft = chat.followups?.[chat.activeChat!.chatId];
  const allowed = admissionReady && chat.activeChat?.schemaVersion === 2 && chat.activeChat.state !== "running" && controller.canAppendFollowup() && !chat.agentStartingRoleId && !chat.agentRuns.some(run => ["preparing", "running", "cancelling"].includes(run.state));
  return <form className="m5-followup" onKeyDown={submitChatShortcut} aria-label={t("m5.chat.followup")} onSubmit={event => { event.preventDefault(); void controller.appendFollowup(admissionReady, true, lang); }}>
    <label htmlFor="m5-followup-text">{t("m5.chat.input")}</label>
    <textarea id="m5-followup-text" value={draft?.text ?? ""} onChange={event => controller.editFollowup(event.target.value)} placeholder={t("m5.chat.placeholder")} rows={3} />
    <p>{t("m5.chat.delivery")}</p>
    {draft?.error && <div role="alert"><p>{t("m5.chat.saveError")}</p><details><summary>{t("m5.chat.details")}</summary><pre>{draft.error}</pre></details></div>}
    {draft?.request && !draft.sending && <details><summary>{t("m5.chat.retryMessage")}</summary><p>{draft.request.text}</p></details>}
    <button type="submit" title="⌘ Enter / Ctrl Enter" aria-keyshortcuts="Meta+Enter Control+Enter" disabled={!allowed || draft?.sending || (!draft?.request && !draft?.text.trim())}>{draft?.sending ? t("m5.chat.saving") : draft?.request ? t("m5.chat.retry") : t("m5.chat.add")}</button>
    {!allowed && <p>{t("m5.chat.unavailable")}</p>}
  </form>;
}

const TeamMessageRow = memo(function TeamMessageRow({ message, roleLabel, messageTime }: {
  message: ReturnType<ChatUiController["snapshot"]>["messages"][number];
  roleLabel: string;
  messageTime: Intl.DateTimeFormat;
}) {
  const { t } = useI18n();
  return <article className={`m5-message m5-message-${message.kind}`} data-chat-id={message.chatId} data-run-id={message.runId ?? undefined}>
    <div><strong>{message.kind === "user" ? t("m5.chat.me") : message.roleId ? t(`m5.roles.${message.roleId}`) : t("m5.chat.status")}</strong>{message.roleId && <span> · {roleLabel}</span>}<time>{messageTime.format(new Date(message.createdAt))}</time></div>
    <OrchestrationMessageBody text={message.text} role={message.roleId} />
    {message.runId && <small>{t("m5.chat.runId", { id: message.runId })}</small>}
  </article>;
});

function TeamTimeline({ controller, chat, admissionReady, fixtureAvailable, onCoordinatorChat }: { onCoordinatorChat: () => void; controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean; fixtureAvailable: boolean }) {
  const { lang, t } = useI18n();
  const messageTime = useMemo(() => new Intl.DateTimeFormat(lang, { hour: "2-digit", minute: "2-digit" }), [lang]);
  const messagesRef = useRef<HTMLDivElement>(null);
  const scrollSurface = () => messagesRef.current?.closest<HTMLDivElement>(".m5-conversation-scroll") ?? messagesRef.current;
  const followLatest = useRef(true);
  const [newMessages, setNewMessages] = useState(false);
  const [historyStart, setHistoryStart] = useState<number | null>(null);
  const messageStart = Math.min(historyStart ?? Math.max(0, chat.messages.length - 200), Math.max(0, chat.messages.length - 1));
  const visibleMessages = chat.messages.slice(messageStart, messageStart + 200);
  const moveHistory = (start: number) => {
    followLatest.current = false;
    setHistoryStart(Math.max(0, start));
    requestAnimationFrame(() => { const surface = scrollSurface(); if (surface) surface.scrollTop = 0; });
  };
  const lastMessage = chat.messages[chat.messages.length - 1];
  const messageRevision = `${chat.messages.length}:${lastMessage?.messageId ?? ""}:${lastMessage?.text ?? ""}`;
  const observedRevision = useRef(messageRevision);
  const showLatest = () => {
    followLatest.current = true;
    setHistoryStart(null);
    setNewMessages(false);
    const list = scrollSurface();
    if (list) list.scrollTop = list.scrollHeight;
  };
  useLayoutEffect(() => {
    followLatest.current = true;
    setHistoryStart(null);
    setNewMessages(false);
  }, [chat.activeChat?.chatId]);
  useLayoutEffect(() => {
    const list = scrollSurface();
    if (followLatest.current) {
      if (list) list.scrollTop = list.scrollHeight;
    } else if (observedRevision.current !== messageRevision) {
      setNewMessages(true);
    }
    observedRevision.current = messageRevision;
  }, [chat.activeChat?.chatId, messageRevision, historyStart]);
  useEffect(() => {
    const list = scrollSurface();
    if (!list) return;
    const observer = new ResizeObserver(() => {
      if (followLatest.current && list.clientHeight > 0) list.scrollTop = list.scrollHeight;
    });
    observer.observe(list);
    if (messagesRef.current) observer.observe(messagesRef.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const list = scrollSurface();
    if (!list) return;
    let userScroll = false;
    const markScroll = () => { userScroll = true; };
    const markKey = (event: KeyboardEvent) => {
      if (["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(event.key)) markScroll();
    };
    const markPointer = (event: PointerEvent) => { if (event.target === list) userScroll = true; };
    const onScroll = () => {
      if (list.clientHeight === 0) return;
      const atBottom = list.scrollHeight - list.scrollTop - list.clientHeight <= 32;
      // Reaching the bottom resumes following, including after reading history.
      // Programmatic scrolling must not turn following off.
      if (atBottom) {
        followLatest.current = true;
        setHistoryStart(null);
        setNewMessages(false);
      } else if (userScroll && historyStart === null) {
        followLatest.current = false;
        setHistoryStart(messageStart);
      }
      userScroll = false;
    };
    list.addEventListener("scroll", onScroll);
    list.addEventListener("wheel", markScroll, { passive: true });
    list.addEventListener("keydown", markKey);
    list.addEventListener("pointerdown", markPointer);
    return () => {
      list.removeEventListener("scroll", onScroll);
      list.removeEventListener("wheel", markScroll);
      list.removeEventListener("keydown", markKey);
      list.removeEventListener("pointerdown", markPointer);
    };
  }, [historyStart, messageStart]);
  const fixtureActive = Boolean(chat.activeRunId);
  const actualActive = chat.agentRuns.find((run) => run.state === "preparing" || run.state === "running" || run.state === "cancelling") ?? null;
  const startingActual = chat.agentStartingRoleId !== null;
  const activity = actualActive ? t(`m5.run.${actualActive.state}`) : startingActual ? t("m5.chat.waitingResponse") : chat.activeChat?.state === "running" ? t("m5.chat.teamWorking") : null;
  const blocked = controller.isActionBlocked();
  const identityUnavailable = chat.agentRecovery !== null && !canAbandonAgentRecovery(chat.agentRecovery);
return <section className="m5-timeline" aria-label={t("m5.chat.title")}><header><div><p className="m5-eyebrow">{t("m5.chat.title")} · {t(`m5.chat.state.${chat.activeChat!.state}`)}</p><h2 title={chat.activeChat!.goal}>{chat.activeChat!.goal}</h2></div><div className="m5-run-actions">{fixtureActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelRun()}>{t("m5.chat.cancelDemo")}</button>}{actualActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelAgentRun(actualActive)}>{t("m5.chat.cancelActual")}</button>}</div></header>{chat.activeChat?.state === "discarded" && !controller.isRecoveryBlocked() && <section className="m5-notice"><p>{t("m5.chat.discardedHelp")}</p></section>}{controller.isRecoveryBlocked() && <section className="m5-error"><strong>{t("m5.chat.recovery")}</strong><p>{identityUnavailable ? t("m5.chat.identityUnavailable") : t("m5.chat.blocked")}</p>{chat.recoveries.filter((item) => item.state === "unresolved").map((item) => <button key={item.recoveryId} type="button" disabled={!admissionReady} onClick={() => void controller.abandonRecovery(item, lang)}>{t("m5.chat.discard")}</button>)}</section>}{chat.agentRecovery && <section className="m5-error">{identityUnavailable ? <><strong>{t("m5.chat.protected")}</strong><p>{t("m5.chat.protectedHelp")}</p></> : <><strong>{t("m5.chat.termination")}</strong><p>{t("m5.chat.terminationHelp")}</p><button type="button" disabled={!admissionReady || chat.agentRecoveryResolving} onClick={() => void controller.abandonAgentRecovery(lang)}>{chat.agentRecoveryResolving ? t("m5.chat.checkingTermination") : t("m5.chat.discardAfterTermination")}</button></>}</section>}{chat.activeChat?.schemaVersion === 2 && chat.activeChat.state === "running" && window.m5Projects && <button type="button" className="m5-secondary" onClick={() => void controller.cancelTeamTask(input => window.m5Projects!.request(input))}>{t("m5.chat.stopTeam")}</button>}{actualActive && admissionReady && !blocked && <ApprovalCard key={actualActive.runId} identity={{chatId:actualActive.chatId,taskId:actualActive.taskId,projectId:actualActive.projectId,worktreeId:actualActive.worktreeId,workspaceIdentity:actualActive.workspaceIdentity,runId:actualActive.runId}} onCancel={() => { if (chat.activeChat?.schemaVersion === 2 && window.m5Projects) void controller.cancelTeamTask(input => window.m5Projects!.request(input)); else void controller.cancelAgentRun(actualActive); }} />}{chat.messages.length > 200 && <nav className="m5-diff-pages" aria-label={t("m5.chat.messages")}><button type="button" disabled={messageStart === 0} onClick={() => moveHistory(messageStart - 200)}>{t("m5.chat.older")}</button><span>{t("m5.chat.range", { first: messageStart + 1, last: messageStart + visibleMessages.length, total: chat.messages.length })}</span><button type="button" disabled={messageStart + 200 >= chat.messages.length} onClick={() => moveHistory(messageStart + 200)}>{t("m5.chat.newer")}</button><button type="button" disabled={historyStart === null} onClick={showLatest}>{t("m5.chat.latest")}</button></nav>}<div ref={messagesRef} className="m5-timeline-list" tabIndex={0} aria-label={t("m5.chat.messages")}>{visibleMessages.map((message) => <TeamMessageRow key={message.messageId} message={message} roleLabel={message.roleId ? profileLabel(chat.activeChat!, message.roleId) : ""} messageTime={messageTime} />)}<AgentRuns runs={chat.agentRuns} />{activity && <div className="m5-chat-activity" role="status"><span className="m5-activity-dot" aria-hidden="true"/><strong>{activity}</strong>{actualActive && <span>{profileLabel(chat.activeChat!, actualActive.selectedProfile.roleId)} · {actualActive.selectedProfile.runtime === "codex" ? "Codex" : "Claude Code"}</span>}</div>}</div>{newMessages && <button type="button" className="m5-secondary m5-new-messages" onClick={showLatest}>{t("m5.chat.newMessages")}</button>}<footer className="m5-execution-footer"><div className="m5-demo-controls">{fixtureAvailable && <span>{t("m5.chat.demoHelp")}</span>}{fixtureAvailable && (["planning", "coding", "review"] as const).map((role) => <button key={`demo-${role}`} type="button" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={() => void controller.startDemo(role)}>{t("m5.chat.demo", { role: t(`m5.roles.${role}`) })}</button>)}</div><div className="m5-actual-controls">{chat.activeChat?.schemaVersion !== 2 && <div><p>{t("m5.chat.legacyCoordinator")}</p><button type="button" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={onCoordinatorChat}>{t("m5.navigation.newChat")}</button></div>}</div></footer></section>;
}

function SessionDiagnosticsHelp({ runtime }: { runtime: string }) {
  const { t } = useI18n();
  const [checkpoints, setCheckpoints] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  if (!window.m5Diagnostics) return null;
  const inspect = async () => {
    setBusy(true); setFailed(false); setCheckpoints(null);
    try {
      const diagnostics = await window.m5Diagnostics!.inspect();
      setCheckpoints(runtime === "codex" ? diagnostics.codexCheckpoints ?? [] : diagnostics.claudeCheckpoints);
    }
    catch { setFailed(true); }
    finally { setBusy(false); }
  };
  return <div>
    <p>{t("m5.run.sessionDiagnosticHelp")}</p>
    <button type="button" className="m5-secondary" disabled={busy} onClick={() => void inspect()}>{t("m5.run.sessionDiagnostic")}</button>
    {failed && <p role="alert">{t("m5.run.sessionDiagnosticFailed")}</p>}
    {checkpoints && (checkpoints.length ? <pre>{checkpoints.join("\n")}</pre> : <p>{t("m5.run.sessionDiagnosticEmpty")}</p>)}
  </div>;
}

function AgentRuns({ runs }: { runs: ReturnType<ChatUiController["snapshot"]>["agentRuns"] }) {
  const { t } = useI18n();
  if (runs.length === 0) return null;
  const { current, previous } = groupRunHistory(runs);
  const renderRun = (run: typeof runs[number]) => {
      const notice = runOutcomeNotice(run);
      return <article key={run.runId}>
        <div className="m5-agent-run-heading">
        <span>{t(`m5.roles.${run.selectedProfile.roleId}`)}</span>{" "}
        <span>{run.selectedProfile.runtime === "codex" ? "Codex" : "Claude Code"} · {run.selectedProfile.modelSelector}{run.selectedProfile.reasoningEffort ? ` · ${run.selectedProfile.reasoningEffort}` : ""}</span>
        <span> · {t(`m5.run.${run.state}`)}</span>
        </div>
        {notice && <p>{t(notice)}</p>}
        <details><summary>{t("m5.run.details")}</summary>
          <p>{t("m5.chat.runId", { id: run.runId })}</p>
          <p>{t(`m5.run.exit.${run.terminationOutcome ?? "unknown"}`)}</p>
          {(run.state === "failed" || run.state === "recoveryRequired") && <SessionDiagnosticsHelp runtime={run.selectedProfile.runtime} />}
        </details>
      </article>;
  };
  return <section className="m5-agent-runs" aria-label={t("m5.run.label")}>
    <strong>{t("m5.run.title")}</strong>
    {current.map(renderRun)}
    <AgentOutputPanel runs={runs} />
    {previous.length > 0 && <details className="m5-agent-run-history">
      <summary>{t("m5.run.previous", { count: previous.length })}</summary>
      <div>{previous.map(renderRun)}</div>
    </details>}
  </section>;
}

function profileLabel(chat: NonNullable<ReturnType<ChatUiController["snapshot"]>["activeChat"]>, roleId: RoleProfileV1["roleId"]): string { const profile = chat.roleSnapshots.find((item) => item.roleId === roleId); return profile ? `${profile.runtime === "codex" ? "Codex" : "Claude Code"} · ${profile.modelSelector}${profile.reasoningEffort ? ` · ${profile.reasoningEffort}` : ""}` : ""; }

function PanelTitle({ active, panel, onSelect, children }: { active: boolean; panel: WorkbenchPanel; onSelect: (panel: WorkbenchPanel) => void; children: ReactNode }) {
  return <button type="button" className="m5-panel-title" aria-pressed={active} onClick={() => onSelect(panel)}>{children}</button>;
}

function EditorContent({ tab, readOnly, onChange, onSave }: { tab: EditorTab | null; readOnly: boolean; onChange: (text: string) => void; onSave: (path: string) => void }) {
  const { t } = useI18n();
  if (!tab) return <section className="m5-editor-welcome"><div className="m5-welcome-mark" aria-hidden="true">E</div><h2>Elsecrew</h2><p>{t("m5.editor.empty")}</p><span>{t("m5.editor.welcomeHint")}</span></section>;
  if (tab.mode === "binary") return <Empty text={t("m5.editor.binary")} />;
  if (tab.mode === "tooLarge") return <Empty text={t("m5.editor.large")} />;
  if (tab.saveState === "conflicted") {
    return <section className="m5-conflict"><strong>{t("m5.editor.conflict")}</strong><p>{t("m5.editor.conflictHelp")}</p><textarea aria-label={t("m5.editor.preserved")} readOnly value={tab.text} style={{ width: "100%", minHeight: 240, boxSizing: "border-box" }} /><details><summary>{t("m5.editor.details")}</summary><pre>{tab.technicalDetail ?? "external file change"}</pre></details></section>;
  }
  return <MonacoEditor tab={tab} readOnly={readOnly} onChange={onChange} onSave={onSave} />;
}

function MonacoEditor({ tab, readOnly, onChange, onSave }: { tab: EditorTab; readOnly: boolean; onChange: (text: string) => void; onSave: (path: string) => void }) {
  const { t } = useI18n();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const model = useRef<Monaco.editor.ITextModel | null>(null);
  const modelText = useRef<string | null>(null);
  const syncModel = useRef<(() => void) | null>(null);
  const labels = { save: t("m5.editor.commandSave"), name: t("m5.editor.accessibleName", { path: tab.path }) };
  const refreshLabels = useRef<(() => void) | null>(null);
  const latest = useRef({ tab, readOnly, onChange, onSave, labels });
  useLayoutEffect(() => { latest.current = { tab, readOnly, onChange, onSave, labels }; }, [tab, readOnly, onChange, onSave, labels.save, labels.name]);

  useEffect(() => {
    let cancelled = false;
    setFailed(false);
    const resources: { dispose(): void }[] = [];
    const cleanup = () => {
      refreshLabels.current = null;
      syncModel.current = null;
      for (const resource of resources.splice(0).reverse()) resource.dispose();
      editor.current = null;
      model.current = null;
      modelText.current = null;
    };
    void loadMonaco().then((monaco) => {
      if (cancelled || !host.current) return;
      const current = latest.current;
      let appliedReadOnly = current.readOnly || current.tab.saveState === "saving";
      monaco.editor.defineTheme("elsecrew", {
        base: "vs-dark", inherit: true, rules: [],
        colors: { "editor.background": "#111315", "editor.foreground": "#eceeed",
          "editorLineNumber.foreground": "#697078", "editorLineNumber.activeForeground": "#c3c7cd",
          "editorGutter.background": "#111315", "editor.lineHighlightBackground": "#191b1e",
          "editor.selectionBackground": "#354352", "editor.inactiveSelectionBackground": "#29323b" },
      });
      const nextEditor = monaco.editor.create(host.current, {
        model: null,
        theme: "elsecrew",
        lineHeight: 22,
        padding: { top: 12, bottom: 12 },
        scrollBeyondLastLine: false,
        lineNumbersMinChars: 4,
        renderLineHighlight: "gutter",
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        readOnly: appliedReadOnly,
        ariaLabel: current.labels.name,
      });
      resources.push(nextEditor);
      editor.current = nextEditor;
      let saveAction: { dispose(): void } | null = null;
      resources.push({ dispose: () => { saveAction?.dispose(); saveAction = null; } });
      const registerLabels = () => {
        saveAction?.dispose();
        saveAction = null;
        nextEditor.updateOptions({ ariaLabel: latest.current.labels.name });
        saveAction = nextEditor.addAction({
          id: "hello-agent.save-file",
          label: latest.current.labels.save,
          keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
          contextMenuGroupId: "9_cutcopypaste",
          run: () => {
            const current = latest.current;
            requestEditorSave(current.tab, current.readOnly, current.onSave);
          },
        });
      };
      registerLabels();
      refreshLabels.current = () => {
        try { registerLabels(); }
        catch { cleanup(); setFailed(true); }
      };
      // Keep one editor surface and one file model. Switching tabs releases the
      // previous model/undo buffer instead of retaining a hidden editor per file.
      let modelPath: string | null = null;
      let subscription: Monaco.IDisposable | null = null;
      let synchronizing = false;
      const releaseModel = () => {
        subscription?.dispose();
        subscription = null;
        nextEditor.setModel(null);
        model.current?.dispose();
        model.current = null;
        modelText.current = null;
        modelPath = null;
      };
      resources.push({ dispose: releaseModel });
      const synchronize = () => {
        const current = latest.current;
        if (modelPath !== current.tab.path) {
          releaseModel();
          const nextModel = monaco.editor.createModel(current.tab.text, languageFor(current.tab.path));
          model.current = nextModel;
          modelText.current = current.tab.text;
          modelPath = current.tab.path;
          nextEditor.setModel(nextModel);
          const ownerPath = current.tab.path;
          subscription = nextModel.onDidChangeContent(() => {
            if (synchronizing || latest.current.tab.path !== ownerPath) return;
            const text = nextModel.getValue();
            modelText.current = text;
            latest.current.onChange(text);
          });
        } else if (model.current && modelText.current !== current.tab.text) {
          synchronizing = true;
          try {
            model.current.setValue(current.tab.text);
            modelText.current = current.tab.text;
          } finally { synchronizing = false; }
        }
        const nextReadOnly = current.readOnly || current.tab.saveState === "saving";
        if (nextReadOnly !== appliedReadOnly) {
          nextEditor.updateOptions({ readOnly: nextReadOnly });
          appliedReadOnly = nextReadOnly;
        }
      };
      syncModel.current = () => {
        try { synchronize(); }
        catch { cleanup(); setFailed(true); }
      };
      synchronize();
    }).catch(() => {
      if (cancelled) return;
      cleanup();
      setFailed(true);
    });
    return () => {
      cancelled = true;
      cleanup();
    };
  }, [attempt]);

  useEffect(() => { refreshLabels.current?.(); }, [labels.save, labels.name]);

  // Install the current file before paint; an old model must never send edits
  // through the new tab's callback during a switch.
  useLayoutEffect(() => { syncModel.current?.(); }, [tab.path, tab.text, tab.saveState, readOnly]);


  return <>{failed && <section className="m5-error" role="alert"><strong>{t("m5.feedback.failed")}</strong><p>{t("m5.feedback.help")}</p><textarea aria-label={t("m5.editor.preserved")} readOnly value={tab.text} style={{ width: "100%", minHeight: 240, boxSizing: "border-box" }} /><button type="button" onClick={() => setAttempt(value => value + 1)}>{t("m5.feedback.retry")}</button></section>}<div className="m5-monaco" ref={host} style={failed ? { display: "none" } : undefined} /></>;
}

// Draft/output updates leave the reviewed diff unchanged. Stable callbacks and
// ordinary shallow comparison skip its rows; context and permission changes still render.
const DiffDetails = memo(function DiffDetails({ descriptor, text, canOpen, onOpen, canDiscuss, onDiscuss, onReturn }: { onReturn: () => void; descriptor: NonNullable<ReturnType<ProjectEditorController["snapshot"]>["diffFile"]>; text: string; canOpen: boolean; onOpen: (path: string) => void; canDiscuss: boolean; onDiscuss: (path: string, side?: "before" | "after", line?: number) => void }) {
  const { t } = useI18n();
  const indexed = useMemo(() => diffPages(text), [text]);
  const [page, setPage] = useState(0);
  const [wrapLines, setWrapLines] = useState(true);
  const [diffMode, setDiffMode] = useState<"inline" | "split">("inline");
  useEffect(() => setPage(0), [descriptor, text]);
  const pages = indexed.pageCount;
  const current = Math.min(page, pages - 1);
  const rawRows = useMemo(() => indexed.page(current), [indexed, current]);
  // Keep unfamiliar or malformed metadata visible; only move known file headers out of the code view.
  const visible = useMemo(() => rawRows.filter(row => !rawRows.some(item => item.kind !== "metadata") || row.kind !== "metadata" || !/^(diff --git |index |--- |\+\+\+ )/.test(row.text)), [rawRows]);
  const metadataLabel = (value: string) => {
    const match = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/.exec(value);
    return match ? t("m5.diff.section", { before: match[1]!, after: match[2]! }) + match[3] : value;
  };
  const splitRows = useMemo(() => diffMode === "split" ? splitDiffPage(visible) : [], [visible, diffMode]);
  const lineNumber = (side: "before" | "after", line: number | null) => line !== null && canDiscuss
    ? <button type="button" className="m5-diff-line-reference" title={t("m5.diff.discussLine", { side: t(`m5.diff.${side}`), line })} aria-label={t("m5.diff.discussLine", { side: t(`m5.diff.${side}`), line })} onClick={() => onDiscuss(descriptor.path, side, line)}>{line}</button>
    : line;
  return (
    <section className="m5-diff-details" aria-label={t("m5.diff.title")}>
      <header className="m5-diff-header">
        <div className="m5-diff-identity"><strong>{descriptor.path}</strong><span>{t(`m5.diff.${descriptor.kind}`)}{descriptor.oldPath ? t("m5.diff.old", { path: descriptor.oldPath }) : ""}</span></div>
        <div className="m5-diff-actions">
          {descriptor.kind !== "deleted" && descriptor.worktreeVersion !== null && <button type="button" className="m5-icon-action" title={t("m5.diff.openEditor")} aria-label={t("m5.diff.openEditor")} disabled={!canOpen} onClick={() => onOpen(descriptor.path)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 5 4 4M4 20l4-1L20 7a2.8 2.8 0 0 0-4-4L4 15z"/></svg></button>}
          <details className="m5-diff-options"><summary title={t("m5.diff.options")} aria-label={t("m5.diff.options")}>⋯</summary><div className="m5-diff-options-panel">
            {descriptor.kind !== "binary" && Boolean(text) && <div className="m5-diff-toolbar">
              <div className="m5-diff-mode" role="group" aria-label={t("m5.diff.displayMode")}>
                <button type="button" aria-pressed={diffMode === "inline"} onClick={() => setDiffMode("inline")}>{t("m5.diff.inline")}</button>
                <button type="button" aria-pressed={diffMode === "split"} onClick={() => setDiffMode("split")}>{t("m5.diff.split")}</button>
              </div>
              <label className="m5-diff-wrap"><input type="checkbox" checked={wrapLines} onChange={event => setWrapLines(event.target.checked)} />{t("m5.diff.wrapLines")}</label>
              <details className="m5-diff-help"><summary>{t("m5.diff.readingHelp")}</summary><p>{t("m5.diff.help")}</p>{canDiscuss && <p>{t("m5.diff.lineHelp")}</p>}</details>
            </div>}
            {canDiscuss && <button type="button" onClick={() => onDiscuss(descriptor.path)}>{t("m5.diff.discuss")}</button>}
            <button type="button" onClick={onReturn}>{t("m5.diff.returnEditor")}</button>
          </div></details>
        </div>
      </header>
      {descriptor.kind === "binary" ? <Empty text={t("m5.diff.binaryHelp")} /> : !text ? <Empty text={t("m5.diff.empty")} /> : <>
        <div className={`m5-diff-lines${wrapLines ? " m5-diff-lines-wrapped" : ""}${diffMode === "split" ? " m5-diff-split" : ""}`} tabIndex={0} aria-label={t("m5.diff.comparison")}>
          {diffMode === "split" ? <table>
            <colgroup><col className="m5-diff-number-column" /><col /><col className="m5-diff-number-column" /><col /></colgroup>
            <thead><tr><th scope="colgroup" colSpan={2}>{t("m5.diff.before")}</th><th scope="colgroup" colSpan={2}>{t("m5.diff.after")}</th></tr></thead>
            <tbody>{splitRows.map((row, index) => row.metadata
              ? <tr key={index} className="m5-diff-metadata"><td colSpan={4}><code>{metadataLabel(row.metadata.text)}</code></td></tr>
              : <tr key={index}>
                <td className={row.before ? `m5-diff-${row.before.kind}` : "m5-diff-absent"}>{lineNumber("before", row.before?.before ?? null)}</td>
                <td className={`m5-diff-side-code m5-diff-${row.before?.kind ?? "absent"}`}><code>{row.before?.text}</code></td>
                <td className={row.after ? `m5-diff-${row.after.kind}` : "m5-diff-absent"}>{lineNumber("after", row.after?.after ?? null)}</td>
                <td className={`m5-diff-side-code m5-diff-${row.after?.kind ?? "absent"}`}><code>{row.after?.text}</code></td>
              </tr>)}</tbody>
          </table> : <table><thead><tr><th scope="col">{t("m5.diff.before")}</th><th scope="col">{t("m5.diff.after")}</th><th scope="col">{t("m5.diff.content")}</th></tr></thead><tbody>
            {visible.map((row, index) => <tr key={current * DIFF_PAGE_ROWS + index} className={`m5-diff-${row.kind}`}><td>{lineNumber("before", row.before)}</td><td>{lineNumber("after", row.after)}</td><td><code>{row.kind === "metadata" ? metadataLabel(row.text) : row.text}</code></td></tr>)}
          </tbody></table>}
        </div>
        <details className="m5-diff-source"><summary>{t("m5.diff.source")}</summary><pre>{rawRows.map(row => row.text).join("\n")}</pre></details>
        {pages > 1 && <nav className="m5-diff-pages" aria-label={t("m5.diff.pages")}>
          <button type="button" disabled={current === 0} onClick={() => setPage(current - 1)}>{t("m5.diff.previous")}</button>
          <span aria-live="polite">{t("m5.diff.page", { page: current + 1, pages, rows: indexed.rowCount })}</span>
          <button type="button" disabled={current === pages - 1} onClick={() => setPage(current + 1)}>{t("m5.diff.next")}</button>
        </nav>}
      </>}
    </section>
  );
});

function ChangeFileList({ entries, selected, disabled, onOpen }: { entries: readonly DiffEntryV1[]; selected?: string; disabled: boolean; onOpen: (path: string) => void }) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const needle = query.trim().toLowerCase();
  const matches = useMemo(() => entries.filter(entry => !needle || entry.path.toLowerCase().includes(needle) || entry.oldPath?.toLowerCase().includes(needle)), [entries, needle]);
  const pageSize = 100;
  const currentPage = Math.min(page, Math.max(0, Math.ceil(matches.length / pageSize) - 1));
  const start = currentPage * pageSize;
  const visible = matches.slice(start, start + pageSize);
  useEffect(() => setPage(0), [entries]);
  return <>
    {entries.length > 0 && <section className="m5-file-search" aria-label={t("m5.diff.filter")}>
      <input type="search" aria-label={t("m5.diff.filter")} placeholder={t("m5.diff.filter")} value={query} maxLength={256} disabled={disabled} onChange={event => { setQuery(event.target.value); setPage(0); }} onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); setQuery(""); setPage(0); } }} />
      <p role="status">{t("m5.diff.fileCount", { shown: matches.length, total: entries.length })}</p>
    </section>}
    <div className="m5-change-list">
      {visible.map(entry => <button key={`${entry.kind}:${entry.path}`} type="button" className="m5-change-row" aria-current={selected === entry.path ? "true" : undefined} title={entry.oldPath ? `${entry.oldPath} → ${entry.path}` : entry.path} onClick={() => onOpen(entry.path)} disabled={disabled}>
        <ChangeKind entry={entry} />
        <span className="m5-change-file"><strong>{fileName(entry.path)}</strong>{entry.path.includes("/") && <small>{entry.path.slice(0, entry.path.lastIndexOf("/"))}</small>}</span>
      </button>)}
      {matches.length === 0 && <Empty text={t(entries.length === 0 ? "m5.editor.noChanges" : "m5.diff.noMatches")} />}
    </div>
    {matches.length > pageSize && <nav className="m5-change-pages" aria-label={t("m5.diff.filePages")}>
      <button type="button" disabled={disabled || currentPage === 0} onClick={() => setPage(currentPage - 1)}>{t("m5.diff.previous")}</button>
      <span>{t("m5.diff.fileRange", { first: start + 1, last: start + visible.length, total: matches.length })}</span>
      <button type="button" disabled={disabled || start + visible.length >= matches.length} onClick={() => setPage(currentPage + 1)}>{t("m5.diff.next")}</button>
    </nav>}
  </>;
}

function ChangeKind({ entry }: { entry: DiffEntryV1 }) {
  const { t } = useI18n();
  return <span className={`m5-change-kind m5-kind-${entry.kind}`}>{t(`m5.diff.${entry.kind}`)}</span>;
}

function fileName(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

function Empty({ text }: { text: string }) {
  return <p className="m5-empty">{text}</p>;
}
