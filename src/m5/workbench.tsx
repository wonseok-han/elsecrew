import { chatConnectionTarget, formatChatNotice } from "./chat-notice.ts";
import { AgentOutputPanel } from "./agent-output-panel.tsx";
import { markStartScreenReady } from "./startup-mark.ts";
import { emptyProjectGuide, parseProjectGuide, projectGuideKey, projectGuideStep, type ProjectGuidePreference } from "./project-guide-state.ts";
import type { ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { groupRunHistory, runOutcomeNotice } from "./work-status.ts";
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
import { validateRoleProfileV1, type RoleProfileV1, type ProjectRoleSettingsV1 } from "./contracts/run-recovery-v1.ts";
import { StorageAdmissionPoller, type AdmissionView } from "./admission-state.ts";
import { resolveM5Bridge, type SupportedM5Bridge } from "./bridge-boundary.ts";
import { canAbandonAgentRecovery, ChatUiController } from "./chat-state.ts";
import { WorkbenchDock } from "./workbench-dock.tsx";
import { MessageBody } from "./message-body.tsx";
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

// Enable together with sidecar admission after the discard safety review.
const REVIEW_DISCARD_ENABLED = false;

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
      if (active) setFixtureAvailable(status.sidecar.capabilities.includes("run.startFixture"));
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
  const { lang, setLang, t } = useI18n();
  const controller = useMemo(() => new ProjectEditorController(bridge), [bridge]);
  // Draft edits do not change the tree. Keep its callback stable so memo can
  // skip rebuilding up to 120 rows on every keystroke without ignoring props.
  const openTreeEntry = useCallback((entry: TreeEntryV1) => {
    if (entry.kind === "file") void controller.openFile(entry.path);
    else if (entry.kind === "directory") void controller.loadTree(entry.path);
  }, [controller]);
  const chatController = useMemo(() => new ChatUiController(bridge), [bridge]);
  const { admission, fixtureAvailable } = useStorageAdmission(bridge);
  const admissionReady = admission === "ready";
  const admissionReadyRef = useRef(admissionReady);
  admissionReadyRef.current = admissionReady;
  const [goal, setGoal] = useState("");
  const goalInputRef = useRef<HTMLInputElement>(null);
  const newChatDialogRef = useRef<HTMLDialogElement>(null);
  const newChatButtonRef = useRef<HTMLButtonElement>(null);
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [applicationBaseline, setApplicationBaseline] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [appDefaultsOpen, setAppDefaultsOpen] = useState(false);
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
  useEffect(() => { setNewChatOpen(false); setReturnToNewChat(false); setGoal(""); setApplicationBaseline(false); }, [snapshot.project?.id]);
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
    const created = await chatController.createChat({ projectId: snapshot.project.id }, goal, applicationBaseline);
    if (created) { setGoal(""); setNewChatOpen(false); await switchChat(created.chatId); }
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
    <main className={`m5-workbench${window.m5Projects ? " m5-with-project-rail" : ""}`} lang={lang} aria-label={t("m5.shell.label")}>
      <ProjectCatalog rail ready={admissionReady} controller={controller} currentId={snapshot.project?.id} disabled={settingsOpen || !admissionReady || snapshot.actionBlocked || !controller.canLeaveProject() || snapshot.selectionBlocked || Boolean(snapshot.workspace)} />
      <header className={`m5-toolbar${snapshot.project ? " m5-toolbar-project" : ""}`}>
        <div>
          <p className="m5-eyebrow">{t("m5.shell.eyebrow")}</p>
          <h1>{snapshot.project?.name ?? t("m5.shell.start")}</h1>
          <p className="m5-subtitle">
            {t(snapshot.project ? "m5.shell.active" : "m5.shell.intro")}
          </p>
        </div>
        <div className="m5-toolbar-actions">
          <button type="button" className="m5-secondary" disabled={!admissionReady || !window.m5Projects || settingsOpen || newChatOpen || appDefaultsOpen} onClick={() => setAppDefaultsOpen(true)}>{t("m5.roles.appDefaults")}</button>
          <select aria-label={t("m5.shell.language")} title={t("m5.shell.languageScope")} value={lang} onChange={event => setLang(event.target.value === "en" ? "en" : "ko")}><option value="ko">한국어</option><option value="en">English</option></select>
          {snapshot.project && <button type="button" onClick={() => void controller.chooseProject()} disabled={settingsOpen || !admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked}>
            {t("m5.shell.switch")}
          </button>}
          {snapshot.project && (
            <button className="m5-secondary" type="button" onClick={() => void leaveProject()} disabled={!admissionReady || settingsOpen || leavingProject || leaveError || !window.m5Projects || snapshot.actionBlocked || !controller.canLeaveProject() || !chatController.canLeaveProjectView()} title={settingsOpen ? t("m5.shell.finishSettings") : undefined}>
              {t("m5.shell.close")}
            </button>
          )}
          {snapshot.project && <button ref={settingsButtonRef} className="m5-secondary" type="button" onClick={() => openRoleSettings()} disabled={!admissionReady || settingsOpen || !projectDataReady || chat.preparing || chat.switching}>{t("m5.shell.roles")}</button>}
        </div>
      </header>

      {discardedWorkspaceNotice && !snapshot.project && <p className="m5-notice" role="status">{t("m5.discard.done")}</p>}
      {snapshot.notice && <p className="m5-notice" role="status">{snapshot.noticeKey ? t(snapshot.noticeKey) : snapshot.notice}</p>}
      {chat.notice && <p className="m5-notice m5-chat-notice" role="status">{formatChatNotice(chat, lang)}{chatConnectionTarget(chat) && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => {
        const provider = chatConnectionTarget(chat);
        if (provider) setConnectionReveal(current => ({ provider, sequence: (current?.sequence ?? 0) + 1 }));
      }}>{t("m5.setup.openConnection", { name: chatConnectionTarget(chat) === "codex" ? "Codex" : "Claude Code" })}</button>}</p>}
      {leaveError && <p className="m5-error" role="alert">{t("m5.feedback.leaveError")}</p>}
      {appDefaultsOpen && <AppRoleDefaultsEditor disabled={!admissionReady} onClose={() => setAppDefaultsOpen(false)} />}
      <StorageAdmissionNotice admission={admission} />
      <WorkStatusNotice bridge={bridge} ready={admissionReady} refreshKey={JSON.stringify([snapshot.error?.message, chat.notice, chat.activeChat?.state])} />
      {!snapshot.project && <GuidedStart disabled={!admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked} onChoose={() => void controller.chooseProject()} />}
      <ProviderLogin disabled={!admissionReady} compact={Boolean(snapshot.project)} reveal={connectionReveal} />
      {snapshot.error && (
        <section className="m5-error" role="alert">
          <strong>{t(snapshot.error.messageKey)}</strong>
          <details><summary>{t("m5.feedback.details")}</summary><pre>{snapshot.error.technicalDetail}</pre></details>
        </section>
      )}
      {chat.error && <section className="m5-error" role="alert"><strong>{t(chat.error.messageKey)}</strong><details><summary>{t("m5.feedback.details")}</summary><pre>{chat.error.detail}</pre></details>{chat.activeChat && chat.error.retryChatId === chat.activeChat.chatId && <button type="button" disabled={!admissionReady || chat.switching} onClick={() => void chatController.retryHistory()}>{t("m5.chat.reloadHistory")}</button>}</section>}
      {snapshot.project && !projectDataReady && <section className="m5-notice" role="status">
        {chat.error?.messageKey === "m5.chatError.project" ? <><span>{t("m5.shell.projectRetryHelp")}</span><button type="button" className="m5-secondary" disabled={!admissionReady || settingsOpen} onClick={() => { if (snapshot.project && !chat.activeChat && !chat.settings) void chatController.openProject(snapshot.project.id); }}>{t("m5.feedback.retry")}</button></> : <span>{t("m5.shell.projectLoading")}</span>}
      </section>}
      {snapshot.project && <ProjectGuide key={snapshot.project.id} hasChat={Boolean(chat.activeChat)}
        projectId={snapshot.project.id} activeChat={chat.activeChat} onResume={id => void switchChat(id)}
        hasOpenFile={snapshot.tabs.length > 0 || snapshot.diffVisible}
        disabled={!admissionReady || !projectDataReady || settingsOpen || snapshot.actionBlocked || chat.preparing || chat.switching}
        onGoal={() => setNewChatOpen(true)} onTeam={() => openRoleSettings()}
        onConversation={() => controller.revealConversation()}
        onChanges={() => { controller.selectPanel("changes"); void controller.refreshChanges(); }} />}
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
        {chat.settings && <dl className="m5-new-chat-team">{chat.settings.roles.map(role => <div key={role.roleId}><dt>{t(`m5.roles.${role.roleId}`)}</dt><dd>{role.runtime === "codex" ? "Codex" : "Claude Code"}<small>{role.modelSelector}{role.reasoningEffort ? ` · ${role.reasoningEffort}` : ""}</small></dd></div>)}</dl>}
        <button type="button" className="m5-secondary" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching} onClick={() => openRoleSettings(true)}>{t("m5.firstChat.editTeam")}</button>
        <p>{t("m5.firstChat.editTeamHelp")}</p>
        {chat.error && <p role="alert" className="m5-error">{t(chat.error.messageKey)}</p>}
        {chat.notice && <p role="status">{formatChatNotice(chat, lang)}</p>}
        <form className="m5-new-chat-form" onSubmit={(event) => { event.preventDefault(); if (admissionReady && projectDataReady && goal.trim() && !chat.preparing && !chat.switching && !chat.preparationApiUnavailable && !(snapshot.workspace && !controller.canLeaveProject())) void createChat(); }}>
          <input ref={goalInputRef} aria-label={t("m5.navigation.goal")} value={goal} onChange={(event) => setGoal(event.target.value)} placeholder={t("m5.navigation.goalPlaceholder")} disabled={!admissionReady || chat.preparing} />
          <label><input type="checkbox" checked={applicationBaseline} disabled={!admissionReady || chat.preparing} onChange={event => setApplicationBaseline(event.target.checked)} />{t("m5.application.chatBaseline")}</label>
          {applicationBaseline && <p>{t("m5.application.chatBaselineHelp")}</p>}
          <button type="submit" disabled={!admissionReady || !projectDataReady || !goal.trim() || chat.preparing || chat.preparationApiUnavailable || chat.switching || Boolean(snapshot.workspace && !controller.canLeaveProject())}>{t("m5.navigation.newChat")}</button>
        </form>
        </dialog>
      </>}

      {settingsOpen && chat.settings && <RoleSettings key={`${chat.settings.projectId}:${chat.settings.revision}:${chat.activeChat?.chatId ?? "none"}`} settings={chat.settings} activeChat={chat.activeChat} disabled={!admissionReady} feedback={<>
        {!admissionReady && <StorageAdmissionNotice admission={admission} />}
        {chat.notice && <p className="m5-notice" role="status">{formatChatNotice(chat, lang)}</p>}
        {chat.error && <section className="m5-error" role="alert"><strong>{t(chat.error.messageKey)}</strong><details><summary>{t("m5.feedback.details")}</summary><pre>{chat.error.detail}</pre></details></section>}
      </>} onSave={roles => chatController.updateSettings(roles)} onReload={() => chatController.reloadSettings()} onClose={closeRoleSettings} returnToDraft={returnToNewChat} projectName={snapshot.project?.name ?? chat.settings.projectId} />}

      {snapshot.project && <WorkbenchDock changeCount={snapshot.rescanRequired ? undefined : snapshot.diffSummary?.entries.length} reveal={snapshot.panelRequest} conversation={chat.activeChat ? <div className="m5-conversation"><div className="m5-conversation-scroll"><TeamTimeline controller={chatController} chat={chat} admissionReady={admissionReady} fixtureAvailable={fixtureAvailable} />{REVIEW_DISCARD_ENABLED && chat.activeChat.state === "reviewReady" && <ChatWorkspaceDiscard key={chat.activeChat.chatId} chat={chat.activeChat} disabled={!admissionReady || settingsOpen || !controller.canLeaveProject() || !chatController.canLeaveProjectView()} onCompleted={async discarded => {
        const request = { action: "releaseView" as const, projectId: discarded.projectId };
        validateProjectCatalogResult(await window.m5Projects!.request(request), request);
        if (!controller.leaveProjectView() || !chatController.clearProjectView()) throw new Error("discarded workspace view could not be released");
        setDiscardedWorkspaceNotice(true);
      }} />}<ChatApplicationPreview onReviewDiff={() => { if (controller.snapshot().actionBlocked) return; controller.selectPanel("changes"); void controller.refreshChanges(); }} key={`${chat.activeChat.chatId}:${chat.activeChat.state}`} projectId={chat.activeChat.projectId} chatId={chat.activeChat.chatId} worktreeId={chat.activeChat.worktreeId} disabled={!admissionReady || chat.switching || chatController.isActionBlocked()} /></div><FollowupComposer controller={chatController} chat={chat} admissionReady={admissionReady} /></div> : <section className="m5-first-chat" aria-label={t("m5.firstChat.title")}>
        <h2>{t("m5.firstChat.title")}</h2><p>{t("m5.firstChat.help")}</p>
        <button type="button" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching} onClick={() => setNewChatOpen(true)}>{t("m5.navigation.newChat")}</button>
        <div className="m5-goal-examples">{(["explain", "plan", "readme"] as const).map(example => <button key={example} type="button" className="m5-secondary" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching || goal.trim().length > 0} onClick={() => { setGoal(t(`m5.firstChat.${example}`)); setNewChatOpen(true); }}>
          {t(`m5.firstChat.${example}`)}
        </button>)}</div><p className="m5-first-chat-note">{t("m5.firstChat.exampleHelp")}</p>
        {chat.settings && <><h3>{t("m5.firstChat.team")}</h3><dl className="m5-team-preview">{chat.settings.roles.map(role => <div key={role.roleId}><dt>{t(`m5.roles.${role.roleId}`)}</dt><dd>{role.runtime === "codex" ? "Codex" : "Claude Code"}<small>{role.modelSelector}{role.reasoningEffort ? ` · ${role.reasoningEffort}` : ""}</small></dd></div>)}</dl></>}
        <p className="m5-first-chat-note">{t("m5.firstChat.teamHelp")}</p>
      </section>} chats={<section className="m5-chat-navigation" aria-label={t("m5.navigation.chats")}>
        <div className="m5-chat-list">{chat.chats.map(item => {
          const selected = chat.activeChat?.chatId === item.chatId;
          const state = selected ? chat.activeChat!.state : item.state;
          const changed = chat.refreshNeededChatIds.includes(item.chatId);
          const status = changed ? t("m5.navigation.newState") : t(`m5.chat.state.${state}`);
          return <button key={item.chatId} type="button" disabled={!admissionReady || settingsOpen || chat.switching} aria-current={selected ? "true" : undefined} title={`${item.goal} · ${status}`} className={selected ? "m5-chat-chip selected" : "m5-chat-chip"} onClick={() => void switchChat(item.chatId)}>
            <span className="m5-chat-goal">{item.goal}</span><span className={changed ? "m5-chat-update" : `m5-chat-status m5-chat-status-${state}`}>{status}</span>
          </button>;
        })}</div>
        {chat.nextChatCursor !== null && <button className="m5-secondary" type="button" disabled={!admissionReady || chat.loadingMoreChats} onClick={() => void chatController.loadMoreChats()}>{chat.loadingMoreChats ? t("m5.navigation.loadingChats") : t("m5.navigation.moreChats")}</button>}
        <button ref={newChatButtonRef} type="button" className="m5-new-chat-button" disabled={!admissionReady || !projectDataReady || settingsOpen || chat.preparing || chat.switching} onClick={() => setNewChatOpen(true)}>{t("m5.navigation.newChat")}</button>
      </section>}>
        <aside className="m5-panel m5-files" aria-label={t("m5.navigation.files")}>
          {snapshot.project ? (
            <>
              <FileSearch key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}`} disabled={!admissionReady || snapshot.actionBlocked} search={(query, signal) => controller.searchFiles(query, signal)} open={path => { void controller.openFile(path); }} />
              <nav className="m5-breadcrumbs" aria-label={t("m5.navigation.folder")}>
                <button className="m5-root" type="button" title={snapshot.project.root} disabled={!admissionReady || snapshot.actionBlocked || snapshot.treeLoading} onClick={() => void controller.loadTree("")}>{snapshot.project.name}</button>
                {snapshot.treeParent.split("/").filter(Boolean).map((part, index, parts) => <button key={index} type="button" aria-current={index === parts.length - 1 ? "location" : undefined} disabled={!admissionReady || snapshot.actionBlocked || snapshot.treeLoading} onClick={() => void controller.loadTree(parts.slice(0, index + 1).join("/"))}>/ {part}</button>)}
              </nav>
              <VirtualFileList key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}:${snapshot.treeParent}:${snapshot.treePage}`} entries={snapshot.tree} activePath={snapshot.activePath} disabled={!admissionReady || snapshot.actionBlocked} onOpen={openTreeEntry} />
              <nav className="m5-tree-pages" aria-label={t("m5.navigation.pages")}>
                <button type="button" disabled={!admissionReady || snapshot.treeLoading || snapshot.treePage === 1} onClick={() => void controller.loadTree(snapshot.treeParent)}>{t("m5.navigation.first")}</button>
                <span>{snapshot.treeLoading ? t("m5.navigation.loading") : t("m5.navigation.page", { page: snapshot.treePage })}</span>
                <button type="button" disabled={!admissionReady || snapshot.treeLoading || !snapshot.treeNextCursor} onClick={() => void controller.loadNextTreePage()}>{t("m5.navigation.next")}</button>
              </nav>
            </>
          ) : <Empty text={t("m5.navigation.empty")} />}
        </aside>

        <section className="m5-panel m5-editor" aria-label={t("m5.editor.title")}>
          <div className="m5-tab-list" role="tablist" aria-label={t("m5.editor.tabs")}>
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
          <div className="m5-editor-head">
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
          <EditorContent tab={activeTab} readOnly={!admissionReady || snapshot.actionBlocked} onChange={(text) => activeTab && controller.updateText(activeTab.path, text)} onSave={(path) => {
            if (admissionReadyRef.current && controller.snapshot().activePath === path) void controller.saveActive();
          }} />
          </div>
              {snapshot.diffVisible && snapshot.diffFile && <DiffDetails descriptor={snapshot.diffFile} text={snapshot.diffText} canOpen={admissionReady && !snapshot.actionBlocked} onOpen={path => { void controller.openFile(path); }}
                canDiscuss={admissionReady && !snapshot.actionBlocked && Boolean(snapshot.workspace) && chatController.canAppendFollowup() && !chat.followups?.[chat.activeChat?.chatId ?? ""]?.request}
                onDiscuss={(path, side, line) => {
                  const current = controller.snapshot();
                  if (!admissionReady || current.actionBlocked || !current.workspace || current.diffFile?.path !== path) return;
                  const reference = side && line !== undefined
                    ? t("m5.diff.lineReference", { path: side === "before" ? current.diffFile.oldPath ?? path : path, side: t(`m5.diff.${side}`), line })
                    : t("m5.diff.reference", { path });
                  if (chatController.addReviewReference(current.workspace, reference)) controller.revealConversation();
                }} />}
        </section>

        <aside className="m5-panel m5-changes" aria-label={t("m5.editor.changes")}>
          <PanelTitle active={snapshot.panel === "changes"} panel="changes" onSelect={controller.selectPanel.bind(controller)}>
            {t("m5.editor.changes")} {snapshot.diffSummary ? `(${snapshot.diffSummary.entries.length})` : ""}
          </PanelTitle>
          {snapshot.project ? (
            <>
              <button className="m5-refresh" type="button" onClick={() => void controller.refreshChanges()} disabled={!admissionReady || snapshot.diffLoading}>{t("m5.editor.refresh")}</button>
              {snapshot.diffLoading && <p className="m5-empty" role="status">{t("m5.editor.loadingChanges")}</p>}
              {snapshot.diffSummary && <ChangeFileList key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}`} entries={snapshot.diffSummary.entries} selected={snapshot.diffFile?.path} disabled={!admissionReady || snapshot.actionBlocked} onOpen={path => { void controller.openDiff(path); }} />}
            </>
          ) : <Empty text={t("m5.editor.chooseProject")} />}
        </aside>
      </WorkbenchDock>}
    </main>
  );
}

const VirtualFileList = memo(function VirtualFileList({ entries, activePath, disabled, onOpen }: { entries: readonly TreeEntryV1[]; activePath: string | null; disabled: boolean; onOpen: (entry: TreeEntryV1) => void }) {
  const [scrollTop, setScrollTop] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const rowHeight = 32;
  const start = Math.min(Math.max(0, entries.length - 1), Math.max(0, Math.floor(scrollTop / rowHeight) - 8));
  const end = Math.min(entries.length, start + 120);
  return <div ref={listRef} className="m5-file-list m5-virtual-file-list" style={{ height: Math.min(400, entries.length * rowHeight) }} onScroll={event => setScrollTop(event.currentTarget.scrollTop)} onKeyDown={event => {
    if (disabled || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const current = Number((event.target as HTMLElement).closest<HTMLButtonElement>("button[data-index]")?.dataset.index);
    if (!Number.isInteger(current)) return;
    event.preventDefault();
    const direction = event.key === "ArrowUp" || event.key === "End" ? -1 : 1;
    let next = event.key === "Home" ? 0 : event.key === "End" ? entries.length - 1 : current + direction;
    while (next >= 0 && next < entries.length && entries[next].kind === "symlink") next += direction;
    if (next < 0 || next >= entries.length) return;
    const list = listRef.current;
    if (!list) return;
    if (next * rowHeight < list.scrollTop) list.scrollTop = next * rowHeight;
    else if ((next + 1) * rowHeight > list.scrollTop + list.clientHeight) list.scrollTop = (next + 1) * rowHeight - list.clientHeight;
    setScrollTop(list.scrollTop);
    requestAnimationFrame(() => list.querySelector<HTMLButtonElement>(`button[data-index="${next}"]`)?.focus());
  }}>
    <div aria-hidden="true" style={{ height: start * rowHeight }} />
    {entries.slice(start, end).map((entry, index) => <button key={entry.path} data-index={start + index} type="button" className="m5-file-row" data-kind={entry.kind} aria-current={entry.kind === "file" && activePath === entry.path ? "true" : undefined} title={entry.path} onClick={() => onOpen(entry)} disabled={disabled || entry.kind === "symlink"}>
      <span className="m5-file-glyph" aria-hidden="true">{entry.kind === "directory" ? "▸" : entry.kind === "symlink" ? "↗" : entry.name.includes(".") ? entry.name.split(".").pop()!.slice(0, 4).toUpperCase() : "—"}</span>
      <span className="m5-tree-name">{entry.name}</span>
    </button>)}
    <div aria-hidden="true" style={{ height: (entries.length - end) * rowHeight }} />
  </div>;
});

function ProjectGuide({ projectId, activeChat, onResume, hasChat, hasOpenFile, disabled, onGoal, onTeam, onConversation, onChanges }: {
  projectId: string; activeChat: ChatRecordV1 | null; onResume: (chatId: string) => void;
  hasChat: boolean; hasOpenFile: boolean; disabled: boolean; onGoal: () => void; onTeam: () => void; onConversation: () => void; onChanges: () => void;
}) {
  const { t } = useI18n();
  const [preference, setPreference] = useState(() => { try { return parseProjectGuide(localStorage.getItem(projectGuideKey(projectId))); } catch { return emptyProjectGuide(); } });
  const [saveFailed, setSaveFailed] = useState(false);
  const working = hasOpenFile || hasChat;
  const [open, setOpen] = useState(!preference.dismissed && !working);
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
    <summary>{t("m5.guide.title")} · {t(`m5.guide.step.${step}`)}</summary>
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
    </ol></details><p>{t("m5.guide.safety")}</p>
  </details>;
}

function GuidedStart({ disabled, onChoose }: { disabled: boolean; onChoose: () => void }) {
  const { t } = useI18n();
  return <section className="m5-guided-start" aria-label={t("m5.start.title")}>
    <div className="m5-start-heading"><h2>{t("m5.start.title")}</h2><p>{t("m5.start.help")}</p></div>
    <ol className="m5-start-steps">{(["connect", "project", "goal"] as const).map((step, index) => <li key={step}>
      <span className="m5-start-number" aria-hidden="true">{index + 1}</span><div><h3>{t(`m5.start.${step}`)}</h3><p>{t(`m5.start.${step}Help`)}</p></div>
    </li>)}</ol>
    <div className="m5-start-action"><button type="button" onClick={onChoose} disabled={disabled}>{t("m5.shell.choose")}</button><p>{t("m5.start.projectLimit")}</p></div>
  </section>;
}

function AppRoleDefaultsEditor({ disabled, onClose }: { disabled: boolean; onClose: () => void }) {
  const { t } = useI18n();
  const [settings, setSettings] = useState<ProjectRoleSettingsV1 | null>(null);
  const [error, setError] = useState(false);
  const [saved, setSaved] = useState(false);
  const mounted = useRef(false);
  const generation = useRef(0);
  const load = useCallback(async () => {
    const epoch = ++generation.current;
    setError(false); setSaved(false);
    try {
      const request = { action: "getAppRoleDefaults" as const };
      const result = validateProjectCatalogResult(await window.m5Projects!.request(request), request);
      if (mounted.current && generation.current === epoch && result.action === request.action) setSettings({ ...result.settings, projectId: "app_defaults" });
    } catch { if (mounted.current && generation.current === epoch) setError(true); }
  }, []);
  useEffect(() => { mounted.current = true; void load(); return () => { mounted.current = false; generation.current++; }; }, [load]);
  const save = async (roles: readonly RoleProfileV1[]) => {
    if (!settings) return;
    const epoch = ++generation.current;
    setError(false); setSaved(false);
    try {
      const request = { action: "updateAppRoleDefaults" as const, expectedRevision: settings.revision, roles };
      const result = validateProjectCatalogResult(await window.m5Projects!.request(request), request);
      if (mounted.current && generation.current === epoch && result.action === request.action) { setSettings({ ...result.settings, projectId: "app_defaults" }); setSaved(true); }
    } catch { if (mounted.current && generation.current === epoch) setError(true); }
  };
  if (!settings) return <section className="m5-notice" role="status">
    <p>{t(error ? "m5.roles.appDefaultsError" : "m5.roles.busy")}</p>
    {error && <button type="button" disabled={disabled} onClick={() => void load()}>{t("m5.roles.reload")}</button>}
    <button type="button" onClick={onClose}>{t("m5.roles.back")}</button>
  </section>;
  return <RoleSettings scope="app" settings={settings} activeChat={null} disabled={disabled}
    feedback={<>{error && <p role="alert">{t("m5.roles.appDefaultsError")}</p>}{saved && <p role="status">{t("m5.roles.appDefaultsSaved")}</p>}</>}
    onSave={save} onReload={load} onClose={onClose} projectName={t("m5.roles.appDefaults")} />;
}

function RoleSettings({ settings, activeChat, disabled, feedback, onSave, onReload, onClose, returnToDraft = false, projectName, scope = "project" }: { scope?: "project" | "app"; settings: NonNullable<ReturnType<ChatUiController["snapshot"]>["settings"]>; activeChat: ReturnType<ChatUiController["snapshot"]>["activeChat"]; disabled: boolean; feedback: ReactNode; onSave: (roles: readonly RoleProfileV1[]) => Promise<void>; onReload: () => Promise<void>; onClose: () => void; returnToDraft?: boolean; projectName: string }) {
  const { t } = useI18n();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [roles, setRoles] = useState<readonly RoleProfileV1[]>(settings.roles);
  const [busy, setBusy] = useState(false);
  const [discardAction, setDiscardAction] = useState<"close" | "reload" | null>(null);
  const [choices, setChoices] = useState<Partial<Record<RoleProfileV1["roleId"], ModelChoicesResult["models"]>>>({});
  const [choicesError, setChoicesError] = useState(false);
  const choiceEpoch = useRef(0);
  useEffect(() => {
    choiceEpoch.current += 1;
    setChoices({}); setChoicesError(false);
    return () => { choiceEpoch.current += 1; };
  }, [settings, activeChat?.chatId]);
  const loadChoices = async (role: RoleProfileV1) => {
    if (!activeChat || !window.m5Models || role.runtime !== "codex") return;
    const epoch = choiceEpoch.current;
    setBusy(true); setChoicesError(false);
    try {
      const result = await window.m5Models.request({ runtime: "codex", refresh: true, chatId: activeChat.chatId, roleId: role.roleId });
      if (epoch !== choiceEpoch.current) return;
      if (result.scope.worktreeId !== activeChat.worktreeId || result.scope.workspaceIdentity !== activeChat.workspaceIdentity) throw new Error("model choice scope changed");
      setChoices(old => ({ ...old, [role.roleId]: result.models }));
    } catch { if (epoch === choiceEpoch.current) setChoicesError(true); }
    finally { setBusy(false); }
  };
  const dirty = JSON.stringify(roles) !== JSON.stringify(settings.roles);
  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => { dialog?.close(); };
  }, []);
  useEffect(() => { setRoles(settings.roles); setDiscardAction(null); }, [settings]);
  const change = (index: number, field: keyof RoleProfileV1, value: string | null) => setRoles((old) => old.map((role, current) => current === index ? { ...role, [field]: value } : role));
  let valid = roles.length === 3;
  try { roles.forEach(role => { validateRoleProfileV1(role); if (!role.modelSelector.trim()) valid = false; }); } catch { valid = false; }
  const run = async (action: () => Promise<void>) => { setBusy(true); try { await action(); } finally { setBusy(false); } };
  const requestClose = () => { if (!busy && discardAction === null) { if (dirty) setDiscardAction("close"); else onClose(); } };
  return <dialog ref={dialogRef} className="m5-settings m5-settings-dialog" aria-label={t("m5.roles.label")} onCancel={event => { event.preventDefault(); requestClose(); }}>
    <header><div><p className="m5-eyebrow">{t("m5.roles.version", { revision: settings.revision })}</p><h2>{t(scope === "app" ? "m5.roles.appDefaults" : "m5.shell.roles")}</h2></div><button className="m5-secondary" type="button" disabled={busy || discardAction !== null} onClick={requestClose}>{t(returnToDraft ? "m5.roles.backToDraft" : "m5.roles.back")}</button></header>
    <div className="m5-settings-body">
    <section className="m5-settings-scope" aria-label={t("m5.roles.scope")}><span>{t(scope === "app" ? "m5.roles.appScope" : "m5.roles.scope")}</span><strong>{projectName}</strong><p>{t(scope === "app" ? "m5.roles.appDefaultsHelp" : "m5.roles.scopeHelp")}</p></section>
    {feedback}
    {dirty && <p role="status">{t("m5.roles.dirty")}</p>}
    {discardAction && <section role="alertdialog" aria-label={t("m5.roles.discardLabel")}>
      <p>{t("m5.roles.discardQuestion")}</p>
      <button type="button" onClick={() => setDiscardAction(null)}>{t("m5.roles.keep")}</button>
      <button type="button" disabled={disabled || busy} onClick={() => { const action = discardAction; setDiscardAction(null); if (action === "close") onClose(); else void run(onReload); }}>{t(discardAction === "close" ? "m5.roles.discardClose" : "m5.roles.discardReload")}</button>
    </section>}
    <p>{t("m5.roles.help")}</p>
    {choicesError && <p role="alert">{t("m5.roles.choicesError")}</p>}
    <div className="m5-role-cards">{roles.map((role, index) => <article key={role.roleId} className="m5-role-card" aria-labelledby={`m5-role-heading-${role.roleId}`}>
      <strong id={`m5-role-heading-${role.roleId}`}>{t(`m5.roles.${role.roleId}`)}</strong>
      <label>{t("m5.roles.runtime")}<select disabled={disabled || busy || discardAction !== null} value={role.runtime} onChange={event => change(index, "runtime", event.target.value)}><option value="codex">Codex</option><option value="claude-code">Claude Code</option></select></label>
      <label>{t("m5.roles.model")}<input disabled={disabled || busy || discardAction !== null} value={role.modelSelector} onChange={event => change(index, "modelSelector", event.target.value)} /></label>
      <label>{t("m5.roles.reasoning")}<input disabled={disabled || busy || discardAction !== null} value={role.reasoningEffort ?? ""} placeholder={t("m5.roles.default")} onChange={event => change(index, "reasoningEffort", event.target.value || null)} /></label>
      <span className="m5-role-permission">{t(role.permission === "readOnly" ? "m5.roles.readOnly" : "m5.roles.write")}</span>
      {scope === "project" && role.runtime === "codex" && <div className="m5-role-model-tools">
        <button type="button" className="m5-secondary" disabled={disabled || busy || discardAction !== null || !activeChat || !window.m5Models} onClick={() => void loadChoices(role)}>{t("m5.roles.loadChoices")}</button>
        {!activeChat && <small>{t("m5.roles.choicesNeedChat")}</small>}
        {choices[role.roleId] && <label>{t("m5.roles.availableChoices")}<select value="" disabled={disabled || busy || discardAction !== null} onChange={event => { if (event.target.value) change(index, "modelSelector", event.target.value); }}>
          <option value="">{t("m5.roles.chooseModel")}</option>
          {choices[role.roleId]!.map(model => <option key={model.modelSelector} value={model.modelSelector}>{model.modelSelector} · {model.reasoningEfforts.join(", ")}</option>)}
        </select><small>{t("m5.roles.choicesHelp")}</small></label>}
      </div>}
      <small className="m5-role-model-help">{t("m5.roles.modelHelp")}</small>
    </article>)}</div>
    {!valid && <p role="alert">{t("m5.roles.invalid")}</p>}
    </div>
    <footer className="m5-settings-actions">
    <button type="button" disabled={disabled || busy || !valid || discardAction !== null} onClick={() => void run(() => onSave(roles))}>{t(busy ? "m5.roles.busy" : scope === "app" ? "m5.roles.appSave" : "m5.roles.save")}</button>
    <button className="m5-secondary" type="button" disabled={disabled || busy || discardAction !== null} onClick={() => dirty ? setDiscardAction("reload") : void run(onReload)}>{t("m5.roles.reload")}</button>
    <p>{t("m5.roles.reloadHelp")}</p>
    </footer>
  </dialog>;
}

function FollowupComposer({ controller, chat, admissionReady }: { controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean }) {
  const { t } = useI18n();
  const draft = chat.followups?.[chat.activeChat!.chatId];
  const allowed = admissionReady && controller.canAppendFollowup();
  return <form className="m5-followup" aria-label={t("m5.chat.followup")} onSubmit={event => { event.preventDefault(); void controller.appendFollowup(admissionReady); }}>
    <label htmlFor="m5-followup-text">{t("m5.chat.input")}</label>
    <textarea id="m5-followup-text" value={draft?.text ?? ""} onChange={event => controller.editFollowup(event.target.value)} placeholder={t("m5.chat.placeholder")} rows={3} />
    <p>{t("m5.chat.delivery")}</p>
    {draft?.error && <div role="alert"><p>{t("m5.chat.saveError")}</p><details><summary>{t("m5.chat.details")}</summary><pre>{draft.error}</pre></details></div>}
    {draft?.request && !draft.sending && <details><summary>{t("m5.chat.retryMessage")}</summary><p>{draft.request.text}</p></details>}
    <button type="submit" disabled={!allowed || draft?.sending || (!draft?.request && !draft?.text.trim())}>{draft?.sending ? t("m5.chat.saving") : draft?.request ? t("m5.chat.retry") : t("m5.chat.add")}</button>
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
    <MessageBody text={message.text} />
    {message.runId && <small>{t("m5.chat.runId", { id: message.runId })}</small>}
  </article>;
});

function TeamTimeline({ controller, chat, admissionReady, fixtureAvailable }: { controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean; fixtureAvailable: boolean }) {
  const { lang, t } = useI18n();
  const messageTime = useMemo(() => new Intl.DateTimeFormat(lang, { hour: "2-digit", minute: "2-digit" }), [lang]);
  const messagesRef = useRef<HTMLDivElement>(null);
  const followLatest = useRef(true);
  const [newMessages, setNewMessages] = useState(false);
  const [historyStart, setHistoryStart] = useState<number | null>(null);
  const messageStart = Math.min(historyStart ?? Math.max(0, chat.messages.length - 200), Math.max(0, chat.messages.length - 1));
  const visibleMessages = chat.messages.slice(messageStart, messageStart + 200);
  const moveHistory = (start: number) => {
    followLatest.current = false;
    setHistoryStart(Math.max(0, start));
    requestAnimationFrame(() => { if (messagesRef.current) messagesRef.current.scrollTop = 0; });
  };
  const lastMessage = chat.messages[chat.messages.length - 1];
  const messageRevision = `${chat.messages.length}:${lastMessage?.messageId ?? ""}:${lastMessage?.text ?? ""}`;
  const observedRevision = useRef(messageRevision);
  const showLatest = () => {
    followLatest.current = true;
    setHistoryStart(null);
    setNewMessages(false);
    const list = messagesRef.current;
    if (list) list.scrollTop = list.scrollHeight;
  };
  useLayoutEffect(() => {
    followLatest.current = true;
    setHistoryStart(null);
    setNewMessages(false);
  }, [chat.activeChat?.chatId]);
  useLayoutEffect(() => {
    const list = messagesRef.current;
    if (followLatest.current) {
      if (list) list.scrollTop = list.scrollHeight;
    } else if (observedRevision.current !== messageRevision) {
      setNewMessages(true);
    }
    observedRevision.current = messageRevision;
  }, [chat.activeChat?.chatId, messageRevision, historyStart]);
  useEffect(() => {
    const list = messagesRef.current;
    if (!list) return;
    const observer = new ResizeObserver(() => {
      if (followLatest.current && list.clientHeight > 0) list.scrollTop = list.scrollHeight;
    });
    observer.observe(list);
    return () => observer.disconnect();
  }, []);
  const fixtureActive = Boolean(chat.activeRunId);
  const actualActive = chat.agentRuns.find((run) => run.state === "preparing" || run.state === "running" || run.state === "cancelling") ?? null;
  const startingActual = chat.agentStartingRoleId !== null;
  const blocked = controller.isActionBlocked();
  const identityUnavailable = chat.agentRecovery !== null && !canAbandonAgentRecovery(chat.agentRecovery);
return <section className="m5-timeline" aria-label={t("m5.chat.title")}><header><div><p className="m5-eyebrow">{t("m5.chat.title")} · {t(`m5.chat.state.${chat.activeChat!.state}`)}</p><h2>{chat.activeChat!.goal}</h2></div><div className="m5-run-actions">{fixtureActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelRun()}>{t("m5.chat.cancelDemo")}</button>}{actualActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelAgentRun(actualActive)}>{t("m5.chat.cancelActual")}</button>}</div></header>{chat.activeChat?.state === "discarded" && !controller.isRecoveryBlocked() && <section className="m5-notice"><p>{t("m5.chat.discardedHelp")}</p></section>}{controller.isRecoveryBlocked() && <section className="m5-error"><strong>{t("m5.chat.recovery")}</strong><p>{identityUnavailable ? t("m5.chat.identityUnavailable") : t("m5.chat.blocked")}</p>{chat.recoveries.filter((item) => item.state === "unresolved").map((item) => <button key={item.recoveryId} type="button" disabled={!admissionReady} onClick={() => void controller.abandonRecovery(item, lang)}>{t("m5.chat.discard")}</button>)}</section>}{chat.agentRecovery && <section className="m5-error">{identityUnavailable ? <><strong>{t("m5.chat.protected")}</strong><p>{t("m5.chat.protectedHelp")}</p></> : <><strong>{t("m5.chat.termination")}</strong><p>{t("m5.chat.terminationHelp")}</p><button type="button" disabled={!admissionReady || chat.agentRecoveryResolving} onClick={() => void controller.abandonAgentRecovery(lang)}>{chat.agentRecoveryResolving ? t("m5.chat.checkingTermination") : t("m5.chat.discardAfterTermination")}</button></>}</section>}<AgentRuns runs={chat.agentRuns} />{actualActive && admissionReady && !blocked && <ApprovalCard key={actualActive.runId} identity={{chatId:actualActive.chatId,taskId:actualActive.taskId,projectId:actualActive.projectId,worktreeId:actualActive.worktreeId,workspaceIdentity:actualActive.workspaceIdentity,runId:actualActive.runId}} onCancel={() => void controller.cancelAgentRun(actualActive)} />}{chat.messages.length > 200 && <nav className="m5-diff-pages" aria-label={t("m5.chat.messages")}><button type="button" disabled={messageStart === 0} onClick={() => moveHistory(messageStart - 200)}>{t("m5.chat.older")}</button><span>{t("m5.chat.range", { first: messageStart + 1, last: messageStart + visibleMessages.length, total: chat.messages.length })}</span><button type="button" disabled={messageStart + 200 >= chat.messages.length} onClick={() => moveHistory(messageStart + 200)}>{t("m5.chat.newer")}</button><button type="button" disabled={historyStart === null} onClick={showLatest}>{t("m5.chat.latest")}</button></nav>}<div ref={messagesRef} className="m5-timeline-list" tabIndex={0} aria-label={t("m5.chat.messages")} onScroll={event => { const list = event.currentTarget; if (list.clientHeight === 0) return; const atBottom = list.scrollHeight - list.scrollTop - list.clientHeight <= 32; if (!atBottom && historyStart === null) { followLatest.current = false; setHistoryStart(messageStart); } else if (atBottom && historyStart === null) { followLatest.current = true; setNewMessages(false); } }}>{visibleMessages.map((message) => <TeamMessageRow key={message.messageId} message={message} roleLabel={message.roleId ? profileLabel(chat.activeChat!, message.roleId) : ""} messageTime={messageTime} />)}</div>{newMessages && <button type="button" className="m5-secondary m5-new-messages" onClick={showLatest}>{t("m5.chat.newMessages")}</button>}<footer>{fixtureAvailable && <span>{t("m5.chat.demoHelp")}</span>}{fixtureAvailable && (["planning", "coding", "review"] as const).map((role) => <button key={`demo-${role}`} type="button" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={() => void controller.startDemo(role)}>{t("m5.chat.demo", { role: t(`m5.roles.${role}`) })}</button>)}{(["planning", "coding", "review"] as const).map((role) => <button key={`actual-${role}`} type="button" className="m5-secondary" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={() => void controller.startAgent(role, lang)}>{chat.agentStartingRoleId === role ? t("m5.chat.connecting") : t("m5.chat.prepare", { role: t(`m5.roles.${role}`) })}</button>)}</footer></section>;
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
  if (!tab) return <Empty text={t("m5.editor.empty")} />;
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
      for (const resource of resources.splice(0).reverse()) resource.dispose();
      editor.current = null;
      model.current = null;
      modelText.current = null;
    };
    void loadMonaco().then((monaco) => {
      if (cancelled || !host.current) return;
      const current = latest.current;
      const nextModel = monaco.editor.createModel(current.tab.text, languageFor(current.tab.path));
      resources.push(nextModel);
      model.current = nextModel;
      modelText.current = current.tab.text;
      const nextEditor = monaco.editor.create(host.current, {
        model: nextModel,
        theme: "vs-dark",
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        readOnly: current.readOnly || current.tab.saveState === "saving",
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
      const subscription = nextModel.onDidChangeContent(() => {
        const text = nextModel.getValue();
        modelText.current = text;
        latest.current.onChange(text);
      });
      resources.push(subscription);
    }).catch(() => {
      if (cancelled) return;
      cleanup();
      setFailed(true);
    });
    return () => {
      cancelled = true;
      cleanup();
    };
  }, [tab.path, attempt]);

  useEffect(() => { refreshLabels.current?.(); }, [labels.save, labels.name]);

  useEffect(() => {
    const currentModel = model.current;
    if (currentModel && modelText.current !== tab.text) {
      currentModel.setValue(tab.text);
      modelText.current = tab.text;
    }
  }, [tab.text]);

  useEffect(() => {
    editor.current?.updateOptions({ readOnly: readOnly || tab.saveState === "saving" });
  }, [tab.saveState, readOnly]);

  return <>{failed && <section className="m5-error" role="alert"><strong>{t("m5.feedback.failed")}</strong><p>{t("m5.feedback.help")}</p><textarea aria-label={t("m5.editor.preserved")} readOnly value={tab.text} style={{ width: "100%", minHeight: 240, boxSizing: "border-box" }} /><button type="button" onClick={() => setAttempt(value => value + 1)}>{t("m5.feedback.retry")}</button></section>}<div className="m5-monaco" ref={host} style={failed ? { display: "none" } : undefined} /></>;
}

function DiffDetails({ descriptor, text, canOpen, onOpen, canDiscuss, onDiscuss }: { descriptor: NonNullable<ReturnType<ProjectEditorController["snapshot"]>["diffFile"]>; text: string; canOpen: boolean; onOpen: (path: string) => void; canDiscuss: boolean; onDiscuss: (path: string, side?: "before" | "after", line?: number) => void }) {
  const { t } = useI18n();
  const indexed = useMemo(() => diffPages(text), [text]);
  const [page, setPage] = useState(0);
  const [wrapLines, setWrapLines] = useState(true);
  const [diffMode, setDiffMode] = useState<"inline" | "split">("inline");
  useEffect(() => setPage(0), [descriptor, text]);
  const pages = indexed.pageCount;
  const current = Math.min(page, pages - 1);
  const visible = useMemo(() => indexed.page(current), [indexed, current]);
  const splitRows = useMemo(() => diffMode === "split" ? splitDiffPage(visible) : [], [visible, diffMode]);
  const lineNumber = (side: "before" | "after", line: number | null) => line !== null && canDiscuss
    ? <button type="button" className="m5-diff-line-reference" title={t("m5.diff.discussLine", { side: t(`m5.diff.${side}`), line })} aria-label={t("m5.diff.discussLine", { side: t(`m5.diff.${side}`), line })} onClick={() => onDiscuss(descriptor.path, side, line)}>{line}</button>
    : line;
  return (
    <section className="m5-diff-details" aria-label={t("m5.diff.title")}>
      <strong>{descriptor.path}</strong>
      <button type="button" className="m5-secondary" disabled={!canDiscuss} onClick={() => onDiscuss(descriptor.path)}>{t("m5.diff.discuss")}</button>
      {descriptor.kind !== "deleted" && descriptor.worktreeVersion !== null && <button type="button" className="m5-secondary" disabled={!canOpen} onClick={() => onOpen(descriptor.path)}>{t("m5.diff.openEditor")}</button>}
      <p>{t(`m5.diff.${descriptor.kind}`)}{descriptor.oldPath ? t("m5.diff.old", { path: descriptor.oldPath }) : ""}</p>
      {descriptor.kind === "binary" ? <Empty text={t("m5.diff.binaryHelp")} /> : !text ? <Empty text={t("m5.diff.empty")} /> : <>
        <p>{t("m5.diff.help")}</p>
        {canDiscuss && <p>{t("m5.diff.lineHelp")}</p>}
        <div className="m5-diff-mode" role="group" aria-label={t("m5.diff.displayMode")}>
          <button type="button" aria-pressed={diffMode === "inline"} onClick={() => setDiffMode("inline")}>{t("m5.diff.inline")}</button>
          <button type="button" aria-pressed={diffMode === "split"} onClick={() => setDiffMode("split")}>{t("m5.diff.split")}</button>
        </div>
        <label className="m5-diff-wrap"><input type="checkbox" checked={wrapLines} onChange={event => setWrapLines(event.target.checked)} />{t("m5.diff.wrapLines")}</label>
        <div className={`m5-diff-lines${wrapLines ? " m5-diff-lines-wrapped" : ""}${diffMode === "split" ? " m5-diff-split" : ""}`} tabIndex={0} aria-label={t("m5.diff.comparison")}>
          {diffMode === "split" ? <table>
            <colgroup><col className="m5-diff-number-column" /><col /><col className="m5-diff-number-column" /><col /></colgroup>
            <thead><tr><th scope="colgroup" colSpan={2}>{t("m5.diff.before")}</th><th scope="colgroup" colSpan={2}>{t("m5.diff.after")}</th></tr></thead>
            <tbody>{splitRows.map((row, index) => row.metadata
              ? <tr key={index} className="m5-diff-metadata"><td colSpan={4}><code>{row.metadata.text}</code></td></tr>
              : <tr key={index}>
                <td className={row.before ? `m5-diff-${row.before.kind}` : "m5-diff-absent"}>{lineNumber("before", row.before?.before ?? null)}</td>
                <td className={`m5-diff-side-code m5-diff-${row.before?.kind ?? "absent"}`}><code>{row.before?.text}</code></td>
                <td className={row.after ? `m5-diff-${row.after.kind}` : "m5-diff-absent"}>{lineNumber("after", row.after?.after ?? null)}</td>
                <td className={`m5-diff-side-code m5-diff-${row.after?.kind ?? "absent"}`}><code>{row.after?.text}</code></td>
              </tr>)}</tbody>
          </table> : <table><thead><tr><th scope="col">{t("m5.diff.before")}</th><th scope="col">{t("m5.diff.after")}</th><th scope="col">{t("m5.diff.content")}</th></tr></thead><tbody>
            {visible.map((row, index) => <tr key={current * DIFF_PAGE_ROWS + index} className={`m5-diff-${row.kind}`}><td>{lineNumber("before", row.before)}</td><td>{lineNumber("after", row.after)}</td><td><code>{row.text}</code></td></tr>)}
          </tbody></table>}
        </div>
        {pages > 1 && <nav className="m5-diff-pages" aria-label={t("m5.diff.pages")}>
          <button type="button" disabled={current === 0} onClick={() => setPage(current - 1)}>{t("m5.diff.previous")}</button>
          <span aria-live="polite">{t("m5.diff.page", { page: current + 1, pages, rows: indexed.rowCount })}</span>
          <button type="button" disabled={current === pages - 1} onClick={() => setPage(current + 1)}>{t("m5.diff.next")}</button>
        </nav>}
      </>}
    </section>
  );
}

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
        <span className="m5-change-file"><strong>{fileName(entry.path)}</strong><small>{entry.path.includes("/") ? entry.path.slice(0, entry.path.lastIndexOf("/")) : "/"}</small></span>
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
