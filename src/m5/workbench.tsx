import { formatChatNotice } from "./chat-notice.ts";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api";
import { I18nProvider, useI18n } from "../i18n";

import {
  ProjectEditorController,
  type EditorTab,
  type WorkbenchPanel,
} from "./editor-state.ts";
import type { DiffEntryV1 } from "./contracts/diff-v1.ts";
import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";
import { validateRoleProfileV1, type RoleProfileV1 } from "./contracts/run-recovery-v1.ts";
import { StorageAdmissionPoller, type AdmissionView } from "./admission-state.ts";
import { resolveM5Bridge, type SupportedM5Bridge } from "./bridge-boundary.ts";
import { canAbandonAgentRecovery, ChatUiController } from "./chat-state.ts";
import { WorkbenchDock } from "./workbench-dock.tsx";
import { ProviderLogin } from "./provider-login.tsx";
import { ApprovalCard } from "./approval-card.tsx";
import { ChatApplicationPreview } from "./chat-application-preview.tsx";
import { diffRows, DIFF_PAGE_ROWS } from "./diff-view.ts";
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
function useStorageAdmission(bridge: SupportedM5Bridge): AdmissionView {
  const [admission, setAdmission] = useState<AdmissionView>(() => bridge.version >= 6 ? "checking" : "ready");
  useEffect(() => {
    const v6 = bridge as Partial<M5BridgeV6>;
    const getSystemStatus = v6.getSystemStatus;
    if (bridge.version < 6 || typeof getSystemStatus !== "function") { setAdmission("ready"); return; }
    const poller = new StorageAdmissionPoller(() => v6.getSystemStatus!(), setAdmission);
    poller.start();
    return () => poller.stop();
  }, [bridge]);
  return admission;
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
  const chatController = useMemo(() => new ChatUiController(bridge), [bridge]);
  const admission = useStorageAdmission(bridge);
  const admissionReady = admission === "ready";
  const admissionReadyRef = useRef(admissionReady);
  admissionReadyRef.current = admissionReady;
  const [goal, setGoal] = useState("");
  const [applicationBaseline, setApplicationBaseline] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
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
    if (created) { setGoal(""); await switchChat(created.chatId); }
    else controller.setActionBlocked(chatController.isActionBlocked());
  };

  const activeTab = snapshot.activePath
    ? snapshot.tabs.find((tab) => tab.path === snapshot.activePath) ?? null
    : null;

  return (
    <main className="m5-workbench" lang={lang} aria-label={t("m5.shell.label")}>
      <header className="m5-toolbar">
        <div>
          <p className="m5-eyebrow">{t("m5.shell.eyebrow")}</p>
          <h1>{snapshot.project?.name ?? t("m5.shell.start")}</h1>
          <p className="m5-subtitle">
            {t(snapshot.project ? "m5.shell.active" : "m5.shell.intro")}
          </p>
        </div>
        <div className="m5-toolbar-actions">
          <select aria-label={t("m5.shell.language")} title={t("m5.shell.languageScope")} value={lang} onChange={event => setLang(event.target.value === "en" ? "en" : "ko")}><option value="ko">한국어</option><option value="en">English</option></select>
          <ProjectCatalog controller={controller} currentId={snapshot.project?.id} disabled={settingsOpen || !admissionReady || snapshot.actionBlocked || !controller.canLeaveProject() || snapshot.selectionBlocked || Boolean(snapshot.workspace)} />
          <button type="button" onClick={() => void controller.chooseProject()} disabled={settingsOpen || !admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked}>
            {t(snapshot.project ? "m5.shell.switch" : "m5.shell.choose")}
          </button>
          {snapshot.project && (
            <button className="m5-secondary" type="button" onClick={() => void leaveProject()} disabled={!admissionReady || settingsOpen || leavingProject || leaveError || !window.m5Projects || snapshot.actionBlocked || !controller.canLeaveProject() || !chatController.canLeaveProjectView()} title={settingsOpen ? t("m5.shell.finishSettings") : undefined}>
              {t("m5.shell.close")}
            </button>
          )}
          {snapshot.project && <button className="m5-secondary" type="button" onClick={() => { controller.setActionBlocked(true); setSettingsOpen(true); }} disabled={!admissionReady || settingsOpen}>{t("m5.shell.roles")}</button>}
        </div>
      </header>

      {snapshot.notice && <p className="m5-notice" role="status">{snapshot.noticeKey ? t(snapshot.noticeKey) : snapshot.notice}</p>}
      {chat.notice && <p className="m5-notice m5-chat-notice" role="status">{formatChatNotice(chat, lang)}</p>}
      {leaveError && <p className="m5-error" role="alert">{t("m5.feedback.leaveError")}</p>}
      <StorageAdmissionNotice admission={admission} />
      <WorkStatusNotice bridge={bridge} ready={admissionReady} refreshKey={JSON.stringify([snapshot.error?.message, chat.notice, chat.activeChat?.state])} />
      <ProviderLogin disabled={!admissionReady} />
      {snapshot.error && (
        <section className="m5-error" role="alert">
          <strong>{t(snapshot.error.messageKey)}</strong>
          <details><summary>{t("m5.feedback.details")}</summary><pre>{snapshot.error.technicalDetail}</pre></details>
        </section>
      )}
      {chat.error && <section className="m5-error" role="alert"><strong>{t(chat.error.messageKey)}</strong><details><summary>{t("m5.feedback.details")}</summary><pre>{chat.error.detail}</pre></details></section>}
      {snapshot.rescanRequired && (
        <section className="m5-rescan" role="status">
          <span>{t("m5.feedback.overflow")}</span>
          <button type="button" onClick={() => void Promise.all([controller.loadTree(), controller.refreshChanges()])}>{t("m5.feedback.refresh")}</button>
        </section>
      )}

      {snapshot.project && <section className="m5-chat-strip" aria-label={t("m5.navigation.chats")}>
        <div className="m5-chat-list"><strong>{t("m5.navigation.chatTitle")}</strong>{chat.chats.map((item) => <button key={item.chatId} type="button" disabled={!admissionReady || chat.switching} className={chat.activeChat?.chatId === item.chatId ? "m5-chat-chip selected" : "m5-chat-chip"} onClick={() => void switchChat(item.chatId)}>{item.goal}{chat.refreshNeededChatIds.includes(item.chatId) && <span className="m5-chat-update">{t("m5.navigation.newState")}</span>}</button>)}</div>
        {chat.nextChatCursor !== null && <button className="m5-secondary" type="button" disabled={!admissionReady || chat.loadingMoreChats} onClick={() => void chatController.loadMoreChats()}>{chat.loadingMoreChats ? t("m5.navigation.loadingChats") : t("m5.navigation.moreChats")}</button>}
        <form className="m5-new-chat-form" onSubmit={(event) => { event.preventDefault(); void createChat(); }}>
          <input aria-label={t("m5.navigation.goal")} value={goal} onChange={(event) => setGoal(event.target.value)} placeholder={t("m5.navigation.goalPlaceholder")} disabled={!admissionReady || chat.preparing} />
          <label><input type="checkbox" checked={applicationBaseline} disabled={!admissionReady || chat.preparing} onChange={event => setApplicationBaseline(event.target.checked)} />{t("m5.application.chatBaseline")}</label>
          {applicationBaseline && <p>{t("m5.application.chatBaselineHelp")}</p>}
          <button type="submit" disabled={!admissionReady || !goal.trim() || chat.preparing || chat.preparationApiUnavailable || chat.switching || Boolean(snapshot.workspace && !controller.canLeaveProject())}>{t("m5.navigation.newChat")}</button>
        </form>
      </section>}

      {settingsOpen && chat.settings && <RoleSettings key={chat.settings.projectId} settings={chat.settings} disabled={!admissionReady} onSave={roles => chatController.updateSettings(roles)} onReload={() => chatController.reloadSettings()} onClose={() => setSettingsOpen(false)} />}

      <WorkbenchDock reveal={snapshot.panelRequest} conversation={chat.activeChat ? <div className="m5-conversation"><TeamTimeline controller={chatController} chat={chat} admissionReady={admissionReady} /><ChatApplicationPreview key={`${chat.activeChat.chatId}:${chat.activeChat.state}`} projectId={chat.activeChat.projectId} chatId={chat.activeChat.chatId} worktreeId={chat.activeChat.worktreeId} disabled={!admissionReady || chat.switching || chatController.isActionBlocked()} /><FollowupComposer controller={chatController} chat={chat} admissionReady={admissionReady} /></div> : undefined}>
        <aside className="m5-panel m5-files" aria-label={t("m5.navigation.files")}>
          <PanelTitle active={snapshot.panel === "files"} panel="files" onSelect={controller.selectPanel.bind(controller)}>
            {t("m5.navigation.files")}
          </PanelTitle>
          {snapshot.project ? (
            <>
              <FileSearch key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}`} disabled={!admissionReady || snapshot.actionBlocked} search={(query, signal) => controller.searchFiles(query, signal)} open={path => { void controller.openFile(path); }} />
              <nav className="m5-breadcrumbs" aria-label={t("m5.navigation.folder")}>
                <button className="m5-root" type="button" title={snapshot.project.root} disabled={!admissionReady || snapshot.actionBlocked || snapshot.treeLoading} onClick={() => void controller.loadTree("")}>{snapshot.project.name}</button>
                {snapshot.treeParent.split("/").filter(Boolean).map((part, index, parts) => <button key={index} type="button" aria-current={index === parts.length - 1 ? "location" : undefined} disabled={!admissionReady || snapshot.actionBlocked || snapshot.treeLoading} onClick={() => void controller.loadTree(parts.slice(0, index + 1).join("/"))}>/ {part}</button>)}
              </nav>
              <div className="m5-file-list">
                {snapshot.tree.map((entry) => (
                  <button key={entry.path} type="button" className="m5-file-row" data-kind={entry.kind} onClick={() => entry.kind === "file" ? void controller.openFile(entry.path) : entry.kind === "directory" ? void controller.loadTree(entry.path) : undefined} disabled={!admissionReady || entry.kind === "symlink"}>
                    {entry.name}
                  </button>
                ))}
              </div>
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
                <button type="button" role="tab" aria-selected={snapshot.activePath === tab.path} onClick={() => controller.openFile(tab.path)} disabled={!admissionReady}>
                  {fileName(tab.path)} {tab.saveState !== "clean" && <span aria-label={t("m5.editor.dirty")}>●</span>}
                </button>
                <button type="button" className="m5-close-tab" aria-label={t("m5.editor.close", { path: tab.path })} onClick={() => controller.closeTab(tab.path)} disabled={!admissionReady}>×</button>
              </div>
            ))}
          </div>
          <div className="m5-editor-head">
            <span>{activeTab?.path ?? t("m5.editor.select")}</span>
            {activeTab?.mode === "text" && (activeTab.saveState === "dirty" || activeTab.saveState === "conflicted") && <button type="button" className="m5-secondary" disabled={!admissionReady || snapshot.actionBlocked || reloadingDraft} onClick={() => setReloadDraft(activeTab)}>{t("m5.editor.reload")}</button>}
            {activeTab?.mode === "text" && (
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
          <EditorContent tab={activeTab} readOnly={!admissionReady || snapshot.actionBlocked} onChange={(text) => activeTab && controller.updateText(activeTab.path, text)} onSave={(path) => {
            if (admissionReadyRef.current && controller.snapshot().activePath === path) void controller.saveActive();
          }} />
        </section>

        <aside className="m5-panel m5-changes" aria-label={t("m5.editor.changes")}>
          <PanelTitle active={snapshot.panel === "changes"} panel="changes" onSelect={controller.selectPanel.bind(controller)}>
            {t("m5.editor.changes")} {snapshot.diffSummary ? `(${snapshot.diffSummary.entries.length})` : ""}
          </PanelTitle>
          {snapshot.project ? (
            <>
              <button className="m5-refresh" type="button" onClick={() => void controller.refreshChanges()} disabled={!admissionReady}>{t("m5.editor.refresh")}</button>
              <div className="m5-change-list">
                {snapshot.diffSummary?.entries.map((entry) => (
                  <button key={`${entry.kind}:${entry.path}`} type="button" className="m5-change-row" onClick={() => void controller.openDiff(entry.path)} disabled={!admissionReady}>
                    <ChangeKind entry={entry} />
                    <span>{entry.path}</span>
                  </button>
                ))}
                {snapshot.diffSummary?.entries.length === 0 && <Empty text={t("m5.editor.noChanges")} />}
              </div>
              {snapshot.diffFile && <DiffDetails descriptor={snapshot.diffFile} text={snapshot.diffText} />}
            </>
          ) : <Empty text={t("m5.editor.chooseProject")} />}
        </aside>
      </WorkbenchDock>
    </main>
  );
}

function RoleSettings({ settings, disabled, onSave, onReload, onClose }: { settings: NonNullable<ReturnType<ChatUiController["snapshot"]>["settings"]>; disabled: boolean; onSave: (roles: readonly RoleProfileV1[]) => Promise<void>; onReload: () => Promise<void>; onClose: () => void }) {
  const { t } = useI18n();
  const [roles, setRoles] = useState<readonly RoleProfileV1[]>(settings.roles);
  const [busy, setBusy] = useState(false);
  const [discardAction, setDiscardAction] = useState<"close" | "reload" | null>(null);
  const dirty = JSON.stringify(roles) !== JSON.stringify(settings.roles);
  useEffect(() => { setRoles(settings.roles); setDiscardAction(null); }, [settings]);
  const change = (index: number, field: keyof RoleProfileV1, value: string | null) => setRoles((old) => old.map((role, current) => current === index ? { ...role, [field]: value } : role));
  let valid = roles.length === 3;
  try { roles.forEach(role => { validateRoleProfileV1(role); if (!role.modelSelector.trim()) valid = false; }); } catch { valid = false; }
  const run = async (action: () => Promise<void>) => { setBusy(true); try { await action(); } finally { setBusy(false); } };
  return <section className="m5-settings" aria-label={t("m5.roles.label")}>
    <header><div><p className="m5-eyebrow">{t("m5.roles.version", { revision: settings.revision })}</p><h2>{t("m5.shell.roles")}</h2></div><button className="m5-secondary" type="button" disabled={busy || discardAction !== null} onClick={() => dirty ? setDiscardAction("close") : onClose()}>{t("m5.roles.back")}</button></header>
    {dirty && <p role="status">{t("m5.roles.dirty")}</p>}
    {discardAction && <section role="alertdialog" aria-label={t("m5.roles.discardLabel")}>
      <p>{t("m5.roles.discardQuestion")}</p>
      <button type="button" onClick={() => setDiscardAction(null)}>{t("m5.roles.keep")}</button>
      <button type="button" disabled={disabled || busy} onClick={() => { const action = discardAction; setDiscardAction(null); if (action === "close") onClose(); else void run(onReload); }}>{t(discardAction === "close" ? "m5.roles.discardClose" : "m5.roles.discardReload")}</button>
    </section>}
    <p>{t("m5.roles.help")}</p>
    <div className="m5-role-cards">{roles.map((role, index) => <article key={role.roleId} className="m5-role-card">
      <strong>{t(`m5.roles.${role.roleId}`)}</strong>
      <label>{t("m5.roles.runtime")}<select disabled={disabled || busy || discardAction !== null} value={role.runtime} onChange={event => change(index, "runtime", event.target.value)}><option value="codex">Codex</option><option value="claude-code">Claude Code</option></select></label>
      <label>{t("m5.roles.model")}<input disabled={disabled || busy || discardAction !== null} value={role.modelSelector} onChange={event => change(index, "modelSelector", event.target.value)} /></label>
      <label>{t("m5.roles.reasoning")}<input disabled={disabled || busy || discardAction !== null} value={role.reasoningEffort ?? ""} placeholder={t("m5.roles.default")} onChange={event => change(index, "reasoningEffort", event.target.value || null)} /></label>
      <small>{t("m5.roles.modelHelp")}</small>
      <span>{t(role.permission === "readOnly" ? "m5.roles.readOnly" : "m5.roles.write")}</span>
    </article>)}</div>
    {!valid && <p role="alert">{t("m5.roles.invalid")}</p>}
    <button type="button" disabled={disabled || busy || !valid || discardAction !== null} onClick={() => void run(() => onSave(roles))}>{t(busy ? "m5.roles.busy" : "m5.roles.save")}</button>
    <button className="m5-secondary" type="button" disabled={disabled || busy || discardAction !== null} onClick={() => dirty ? setDiscardAction("reload") : void run(onReload)}>{t("m5.roles.reload")}</button>
    <p>{t("m5.roles.reloadHelp")}</p>
  </section>;
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

function TeamTimeline({ controller, chat, admissionReady }: { controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean }) {
  const { lang, t } = useI18n();
  const fixtureActive = Boolean(chat.activeRunId);
  const actualActive = chat.agentRuns.find((run) => run.state === "preparing" || run.state === "running" || run.state === "cancelling") ?? null;
  const startingActual = chat.agentStartingRoleId !== null;
  const blocked = controller.isActionBlocked();
  const identityUnavailable = chat.agentRecovery !== null && !canAbandonAgentRecovery(chat.agentRecovery);
return <section className="m5-timeline" aria-label={t("m5.chat.title")}><header><div><p className="m5-eyebrow">{t("m5.chat.title")} · {t(`m5.chat.state.${chat.activeChat!.state}`)}</p><h2>{chat.activeChat!.goal}</h2></div><div className="m5-run-actions">{fixtureActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelRun()}>{t("m5.chat.cancelDemo")}</button>}{actualActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelAgentRun(actualActive)}>{t("m5.chat.cancelActual")}</button>}</div></header>{blocked && <section className="m5-error"><strong>{t("m5.chat.recovery")}</strong><p>{identityUnavailable ? t("m5.chat.identityUnavailable") : t("m5.chat.blocked")}</p>{chat.recoveries.filter((item) => item.state === "unresolved").map((item) => <button key={item.recoveryId} type="button" disabled={!admissionReady} onClick={() => void controller.abandonRecovery(item, lang)}>{t("m5.chat.discard")}</button>)}</section>}{chat.agentRecovery && <section className="m5-error">{identityUnavailable ? <><strong>{t("m5.chat.protected")}</strong><p>{t("m5.chat.protectedHelp")}</p></> : <><strong>{t("m5.chat.termination")}</strong><p>{t("m5.chat.terminationHelp")}</p><button type="button" disabled={!admissionReady || chat.agentRecoveryResolving} onClick={() => void controller.abandonAgentRecovery(lang)}>{chat.agentRecoveryResolving ? t("m5.chat.checkingTermination") : t("m5.chat.discardAfterTermination")}</button></>}</section>}<AgentRuns runs={chat.agentRuns} />{actualActive && admissionReady && !blocked && <ApprovalCard key={actualActive.runId} identity={{chatId:actualActive.chatId,taskId:actualActive.taskId,projectId:actualActive.projectId,worktreeId:actualActive.worktreeId,workspaceIdentity:actualActive.workspaceIdentity,runId:actualActive.runId}} onCancel={() => void controller.cancelAgentRun(actualActive)} />}<div className="m5-timeline-list">{chat.messages.map((message) => <article key={message.messageId} className={`m5-message m5-message-${message.kind}`}><div><strong>{message.kind === "user" ? t("m5.chat.me") : message.roleId ? t(`m5.roles.${message.roleId}`) : t("m5.chat.status")}</strong>{message.roleId && <span> · {profileLabel(chat.activeChat!, message.roleId)}</span>}<time>{new Date(message.createdAt).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" })}</time></div><p>{message.text}</p>{message.runId && <small>{t("m5.chat.runId", { id: message.runId })}</small>}</article>)}</div><footer><span>{t("m5.chat.demoHelp")}</span>{(["planning", "coding", "review"] as const).map((role) => <button key={`demo-${role}`} type="button" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={() => void controller.startDemo(role)}>{t("m5.chat.demo", { role: t(`m5.roles.${role}`) })}</button>)}{(["planning", "coding", "review"] as const).map((role) => <button key={`actual-${role}`} type="button" className="m5-secondary" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={() => void controller.startAgent(role)}>{chat.agentStartingRoleId === role ? t("m5.chat.connecting") : t("m5.chat.prepare", { role: t(`m5.roles.${role}`) })}</button>)}</footer></section>;
}

function AgentRuns({ runs }: { runs: ReturnType<ChatUiController["snapshot"]>["agentRuns"] }) {
  const { t } = useI18n();
  if (runs.length === 0) return null;
  return <section className="m5-agent-runs" aria-label={t("m5.run.label")}><strong>{t("m5.run.title")}</strong>{runs.map((run) => <article key={run.runId}><span>{t(`m5.roles.${run.selectedProfile.roleId}`)}</span><span>{run.selectedProfile.runtime === "codex" ? "Codex" : "Claude Code"} · {run.selectedProfile.modelSelector}{run.selectedProfile.reasoningEffort ? ` · ${run.selectedProfile.reasoningEffort}` : ""}</span><span> · {t(`m5.run.${run.state}`)}</span></article>)}</section>;
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
    };
    void loadMonaco().then((monaco) => {
      if (cancelled || !host.current) return;
      const current = latest.current;
      const nextModel = monaco.editor.createModel(current.tab.text, languageFor(current.tab.path));
      resources.push(nextModel);
      model.current = nextModel;
      const nextEditor = monaco.editor.create(host.current, {
        model: nextModel,
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
      const subscription = nextModel.onDidChangeContent(() => latest.current.onChange(nextModel.getValue()));
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
    if (currentModel && currentModel.getValue() !== tab.text) currentModel.setValue(tab.text);
    editor.current?.updateOptions({ readOnly: readOnly || tab.saveState === "saving" });
  }, [tab.text, tab.saveState, readOnly]);

  return <>{failed && <section className="m5-error" role="alert"><strong>{t("m5.feedback.failed")}</strong><p>{t("m5.feedback.help")}</p><textarea aria-label={t("m5.editor.preserved")} readOnly value={tab.text} style={{ width: "100%", minHeight: 240, boxSizing: "border-box" }} /><button type="button" onClick={() => setAttempt(value => value + 1)}>{t("m5.feedback.retry")}</button></section>}<div className="m5-monaco" ref={host} style={failed ? { display: "none" } : undefined} /></>;
}

function DiffDetails({ descriptor, text }: { descriptor: NonNullable<ReturnType<ProjectEditorController["snapshot"]>["diffFile"]>; text: string }) {
  const { t } = useI18n();
  const rows = useMemo(() => diffRows(text), [text]);
  const [page, setPage] = useState(0);
  useEffect(() => setPage(0), [descriptor, text]);
  const pages = Math.max(1, Math.ceil(rows.length / DIFF_PAGE_ROWS));
  const current = Math.min(page, pages - 1);
  const visible = rows.slice(current * DIFF_PAGE_ROWS, (current + 1) * DIFF_PAGE_ROWS);
  return (
    <section className="m5-diff-details" aria-label={t("m5.diff.title")}>
      <strong>{descriptor.path}</strong>
      <p>{t(`m5.diff.${descriptor.kind}`)}{descriptor.oldPath ? t("m5.diff.old", { path: descriptor.oldPath }) : ""}</p>
      {descriptor.kind === "binary" ? <Empty text={t("m5.diff.binaryHelp")} /> : !text ? <Empty text={t("m5.diff.empty")} /> : <>
        <p>{t("m5.diff.help")}</p>
        <div className="m5-diff-lines" tabIndex={0} aria-label={t("m5.diff.comparison")}>
          <table><thead><tr><th scope="col">{t("m5.diff.before")}</th><th scope="col">{t("m5.diff.after")}</th><th scope="col">{t("m5.diff.content")}</th></tr></thead><tbody>
            {visible.map((row, index) => <tr key={current * DIFF_PAGE_ROWS + index} className={`m5-diff-${row.kind}`}><td>{row.before}</td><td>{row.after}</td><td><code>{row.text}</code></td></tr>)}
          </tbody></table>
        </div>
        {pages > 1 && <nav className="m5-diff-pages" aria-label={t("m5.diff.pages")}>
          <button type="button" disabled={current === 0} onClick={() => setPage(current - 1)}>{t("m5.diff.previous")}</button>
          <span aria-live="polite">{t("m5.diff.page", { page: current + 1, pages, rows: rows.length })}</span>
          <button type="button" disabled={current === pages - 1} onClick={() => setPage(current + 1)}>{t("m5.diff.next")}</button>
        </nav>}
      </>}
    </section>
  );
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
