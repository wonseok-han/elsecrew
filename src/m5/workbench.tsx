import { DiscardedWorkCleanup } from "./discarded-work-cleanup.tsx";
import { ProjectPermissions, type ChosenPermissions, type PermissionDraft } from "./project-permissions.tsx";
import { submitChatShortcut } from "./chat-submit-shortcut.ts";
import { ProviderUsage } from "./provider-usage.tsx";
import { ProjectRecoveryHistory } from "./project-recovery-history-view.tsx";
import { PreparationRecovery } from "./preparation-recovery-view";
import { BackupStorageUsageView } from "./backup-storage-usage";
import { flushSync } from "react-dom";
import { AppSettings, type SettingsSection } from "./app-settings";
import { GitSelectionView } from "./git-selection-view.tsx";
import { SaveRecoveryView } from "./save-recovery-view.tsx";
import { chatConnectionTarget, formatChatNotice } from "./chat-notice.ts";
import { AllChanges, ChangeList, LineCounts } from "./changes-view.tsx";
import { markStartScreenReady } from "./startup-mark.ts";
import { emptyProjectGuide, parseProjectGuide, projectGuideKey, projectGuideStep, type ProjectGuidePreference } from "./project-guide-state.ts";
import type { ChatRecordV1 } from "./contracts/run-recovery-v1.ts";
import { hasUsableModel, loadModels, modelLabel, withRecommendedModels } from "./model-catalog.ts";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { I18nProvider, useI18n } from "../i18n";
import { ProjectEditorController, type EditorTab } from "./editor-state.ts";
import { createEditorDraftStore } from "./editor-drafts.ts";
import type { TreeEntryV1 } from "./contracts/project-v1.ts";
import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";
import { roleSettingsAsV3, type RoleProfileV1 } from "./contracts/run-recovery-v1.ts";
import { AppRoleDefaultsEditor, RoleSettings } from "./role-settings.tsx";
import { roleLabel } from "./role-names.ts";
import { StorageAdmissionPoller, type AdmissionView } from "./admission-state.ts";
import { resolveM5Bridge, type SupportedM5Bridge } from "./bridge-boundary.ts";
import { ChatUiController } from "./chat-state.ts";
import { TaskLayout } from "./task-layout.tsx";
import { approvalCovered, changesLine, MENU_SELECTOR, resultBoxMode, revealResults, teamWorking } from "./task-view-model.ts";
import { ChangesStandingLine, ChangesStatusLine, ResultBox } from "./result-box.tsx";
import { PreviewTab, type PreviewEntryRequest } from "./preview-tab.tsx";
import { hasPreviewPage, isPreviewablePage } from "./preview-tab-model.ts";
import { HistoryTab } from "./history-tab.tsx";
import { TaskList, type TaskListItem } from "./task-list.tsx";
import { ReviewSheet } from "./review-sheet.tsx";
import { ProjectSwitcher } from "./project-switcher.tsx";
import { BAND_NARROW_MIN, BAND_REGULAR_MIN, BAND_WIDE_MIN, bandSizes, bandUsesDrawer, defaultRightTab, previewObscured, RIGHT_TABS, clampRightWidth, layoutBand, panelRequestTarget, parseRightWidth, RIGHT_WIDTH_KEY, type LayoutBand, type RightTab, type SheetMode } from "./task-layout-model.ts";
import { ProviderLogin, type ConnectionReveal } from "./provider-login.tsx";
import { ChatWorkspaceDiscard } from "./chat-workspace-discard.tsx";
import { ChatWorkspaceMerge } from "./chat-workspace-merge.tsx";
import { WorkspaceFolderSetting } from "./workspace-folder-setting.tsx";
import { ApplyLatest, ChangesMergeStatus } from "./apply-latest.tsx";
import { FileSearch } from "./file-search-view.tsx";
import { ProjectCatalog } from "./project-catalog-view.tsx";
import { WorkStatusPanel } from "./work-status-view.tsx";
import { NotificationBell, type BellNoticeTone } from "./notification-bell.tsx";
import { startNotificationFeed, type NotificationFeedBridge } from "./notification-feed.ts";
import { NotificationCenter, osNotificationsEnabled, type AppNotification } from "./notifications.ts";
import { validateProjectCatalogResult } from "./contracts/project-catalog-v1.ts";
import { dispositionFromApply, dispositionFromMerge, standingRead, phaseIsSettled, phaseIsWorking, resultBarView, taskPhase, taskPhaseLabelKey, taskPhaseTone, type TaskPhase } from "./task-status.ts";
import { TaskDispositionStore } from "./task-disposition.ts";
import { failStandingRead, finishStandingRead, standingView, startStandingRead, type StandingRead } from "./standing-read.ts";
import { findRememberedProject, forgetLastProject, recallLastProject, rememberLastProject, shouldResumeProject } from "./last-session.ts";
import { DiffDetails, EditorContent, Empty, MonacoDiff, VirtualFileList } from "./editor-surface.tsx";
import { fileName, isProsePath } from "./editor-paths.ts";
import { FollowupComposer, TeamTimeline } from "./task-story.tsx";

/** A merge or apply standing read with no answer by then counts as failed. */
const STANDING_READ_TIMEOUT_MS = 20_000;

/** Poll rather than subscribe: G5D deliberately adds no public storage-admission event. */
function useStorageAdmission(bridge: SupportedM5Bridge, onRecovered: () => void): { admission: AdmissionView; fixtureAvailable: boolean } {
  const recovered = useRef(onRecovered);
  recovered.current = onRecovered;
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
    }, setAdmission, undefined, () => recovered.current());
    poller.start();
    return () => { active = false; poller.stop(); };
  }, [bridge]);
  return { admission, fixtureAvailable };
}

function chatNoticeTone(key: string | undefined): BellNoticeTone {
  if (key === "m5.agentNotice.checking") return "progress";
  if (key === "m5.agentNotice.ready") return "success";
  if (key === "m5.agentNotice.blocked") return "warning";
  return "info";
}

function StorageAdmissionNotice({ admission }: { admission: AdmissionView }) {
  const { t } = useI18n();
  if (admission === "ready") return null;
  const failed = admission === "failed" || admission === "stopped";
  return <section className={failed ? "m5-admission m5-admission-failed" : "m5-admission"} role={failed ? "alert" : "status"}>
    <strong>{t(admission === "stopped" ? "m5.admission.stopped" : admission === "failed" ? "m5.admission.failed" : "m5.admission.preparing")}</strong>
    <p>{t(admission === "stopped" ? "m5.admission.stoppedHelp" : admission === "failed" ? "m5.admission.help" : "m5.admission.wait")}</p>
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

function browserStorage(): Storage | null {
  try { return typeof localStorage === "undefined" ? null : localStorage; } catch { return null; }
}

function M5WorkbenchReady({ bridge }: { bridge: SupportedM5Bridge }) {
  const { lang, t } = useI18n();
  // Unsaved edits are kept across a restart, like a code editor's hot exit.
  const controller = useMemo(() => new ProjectEditorController(bridge, { drafts: createEditorDraftStore(browserStorage()) }), [bridge]);
  useEffect(() => {
    const keep = () => controller.flushDrafts();
    const hidden = () => { if (document.visibilityState === "hidden") keep(); };
    window.addEventListener("pagehide", keep);
    document.addEventListener("visibilitychange", hidden);
    return () => { window.removeEventListener("pagehide", keep); document.removeEventListener("visibilitychange", hidden); };
  }, [controller]);
  const [railExpanded, setRailExpanded] = useState(true);
  const [catalogRevision, setCatalogRevision] = useState(0);
  const [removal, setRemoval] = useState<{ projectId: string; chatId?: string; label: string } | null>(null);
  const [removing, setRemoving] = useState(false);
  const [removalError, setRemovalError] = useState(false);
  const removalDialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { if (removal) removalDialog.current?.showModal(); else removalDialog.current?.close(); }, [removal]);

  useEffect(() => {
    const menus = () => document.querySelectorAll<HTMLDetailsElement>(MENU_SELECTOR);
    const outside = (event: PointerEvent) => menus().forEach(menu => { if (!menu.contains(event.target as Node)) menu.open = false; });
    // Closing a menu uses up the Escape: the sheet or drawer under it stays (review #5).
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") menus().forEach(menu => { if (menu.open) { menu.open = false; event.preventDefault(); menu.querySelector<HTMLElement>("summary")?.focus(); } }); };
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
  // A new engine is running after the old one stopped. Refresh so everything
  // shown comes from it, unless that would lose something not yet saved.
  const [engineRestarted, setEngineRestarted] = useState(false);
  // Run states on screen still come from the old engine and mean nothing
  // here; only what the person typed or left open counts.
  const engineRecovered = () => {
    const current = chatController.snapshot();
    const typing = goal.trim() !== "" || Object.values(current.followups ?? {}).some(draft => Boolean(draft?.text?.trim()) || Boolean(draft?.sending));
    const untouched = !settingsOpen && !appDefaultsOpen && !newChatOpen && !reloadDraft && !reloadingDraft
      && !leavingProject && !leaveError && !typing && controller.canLeaveProject();
    if (untouched) window.location.reload();
    else setEngineRestarted(true);
  };
  const { admission, fixtureAvailable } = useStorageAdmission(bridge, engineRecovered);
  const admissionReady = admission === "ready";
  const admissionReadyRef = useRef(admissionReady);
  admissionReadyRef.current = admissionReady;
  const [showHiddenFiles, setShowHiddenFiles] = useState(false);
  const [goal, setGoal] = useState("");
  // Unknown until the saved project list has been read: no first-run guide
  // flashes before it (D3⑤).
  const [hasSavedProjects, setHasSavedProjects] = useState<boolean | null>(null);
  /** Reopening the project open when the app last closed (last-session.ts). */
  const [resumingProject, setResumingProject] = useState(false);
  const resumeTried = useRef(false);
  const goalInputRef = useRef<HTMLInputElement>(null);
  const newChatDialogRef = useRef<HTMLDialogElement>(null);
  const newChatButtonRef = useRef<HTMLButtonElement>(null);
  const [newChatOpen, setNewChatOpen] = useState(false);
  // What the open task's own panels read, tagged with the task: a value read
  // for one task is never shown for another (task-status.ts).
  const [standing, setStanding] = useState<StandingRead | null>(null);
  /** Permission requests waiting in the open task's current run. */
  const [approvalsWaiting, setApprovalsWaiting] = useState<{ chatId: string; count: number } | null>(null);
  // The first chat of a project must not start before the user has seen the
  // team (provider/model per role) and the project's access permissions.
  const [setupConfirmed, setSetupConfirmed] = useState(false);
  const [firstPermissions, setFirstPermissions] = useState<PermissionDraft | null>(null);
  // Boxes the user set on the first task's setup survive a remount (team edit).
  const chosenPermissions = useRef(new Map<string, ChosenPermissions>());
  const reportPermissions = useCallback((draft: PermissionDraft | null) => {
    if (draft?.touched) chosenPermissions.current.set(draft.projectId, { permissions: draft.permissions, basis: draft.basis });
    setFirstPermissions(draft);
  }, []);
  // Each opening of the new-task dialog reads the saved permissions afresh.
  const [newChatOpenings, setNewChatOpenings] = useState(0);
  useEffect(() => { if (newChatOpen) { setNewChatOpenings(count => count + 1); setPermissionsSaveFailed(false); } }, [newChatOpen]);
  const [permissionsSaveFailed, setPermissionsSaveFailed] = useState(false);
  useEffect(() => { if (newChatOpen) setSetupConfirmed(false); }, [newChatOpen]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [appDefaultsOpen, setAppDefaultsOpen] = useState(false);
  // What "앱 기본값으로도 저장" saved; the role screen remounts after saving, so the workbench keeps it.
  const [appDefaultsOutcome, setAppDefaultsOutcome] = useState<"both" | "projectOnly" | null>(null);
  const roleNavigation = useRef<((next: () => void) => void) | null>(null);
  const navigatePreferences = (next: () => void) => roleNavigation.current ? roleNavigation.current(next) : next();
  const [updatePreparing, setUpdatePreparing] = useState(false);
  const [managedProject, setManagedProject] = useState<{ projectId: string; canonicalPath: string } | null>(null);
  // Work that still holds the app (shown inside the bell, marked on it).
  const [workAttention, setWorkAttention] = useState(false);
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
  // A task starts from the last commit (task = PR): say so, and how many
  // uncommitted changes in the user's folder it will not include.
  const [sourceChanges, setSourceChanges] = useState<{ applies: boolean; uncommitted: number } | null>(null);
  const openProjectId = snapshot.project?.id;
  useEffect(() => {
    setSourceChanges(null);
    if (!newChatOpen || !openProjectId || !window.m5Projects) return;
    let live = true;
    const request = { action: "inspectSourceChanges" as const, projectId: openProjectId };
    void window.m5Projects.request(request).then(raw => {
      const result = validateProjectCatalogResult(raw, request);
      if (live && result.action === "inspectSourceChanges") setSourceChanges(result.changes);
    }).catch(() => undefined);
    return () => { live = false; };
  }, [newChatOpen, openProjectId]);
  // Restart picks up where the person left off (F13): the open project is
  // remembered, and reopened once at start through the normal engine checks.
  useEffect(() => { if (snapshot.project) rememberLastProject(browserStorage(), snapshot.project.id); }, [snapshot.project?.id]);
  // Waits while opening is locked (work still finishing, a selection in
  // progress) and tries once it is free: the same lock a click would meet.
  const resumeBlocked = snapshot.actionBlocked || snapshot.selectionBlocked;
  useEffect(() => {
    if (resumeTried.current || !admissionReady || snapshot.project || !window.m5Projects) return;
    const remembered = recallLastProject(browserStorage());
    if (!remembered) { resumeTried.current = true; return; }
    if (resumeBlocked) return;
    resumeTried.current = true;
    setResumingProject(true);
    void (async () => {
      try {
        // Every page of the saved list (50 a page) is read before deciding the
        // project is gone; a list read only in part keeps the memory.
        const standing = await findRememberedProject(remembered, async cursor => {
          const input: { action: "list"; cursor: string | null } = { action: "list", cursor };
          const result = validateProjectCatalogResult(await window.m5Projects!.request(input), input);
          if (result.action !== "list") throw new Error("unexpected project catalog response");
          return { ids: result.projects.map(entry => entry.projectId), nextCursor: result.nextCursor };
        });
        if (standing !== "found") { if (standing === "gone") forgetLastProject(browserStorage()); return; }
        const known = [remembered];
        // Opening can be locked for a moment while the screen settles: try a
        // few times, each through the same checks as a click.
        for (let attempt = 0; attempt < 5; attempt++) {
          const current = controller.snapshot();
          const target = shouldResumeProject({ remembered, attempted: false, ready: admissionReadyRef.current, projectOpen: Boolean(current.project), busy: current.actionBlocked || current.selectionBlocked, known });
          if (target) await controller.reopenProject(target, request => window.m5Projects!.request(request));
          const after = controller.snapshot();
          if (after.project) return;
          if (after.error?.messageKey === "m5.editorError.reopenProject") { forgetLastProject(browserStorage()); return; }
          await new Promise(resolve => window.setTimeout(resolve, 1000));
        }
      } catch { /* the home screen stays; nothing was opened */ }
      finally { setResumingProject(false); }
    })();
  }, [admissionReady, snapshot.project?.id, resumeBlocked, controller]);
  // Settings shows the open project first; a project picked earlier must not
  // linger once another project is opened.
  useEffect(() => { setManagedProject(null); }, [snapshot.project?.id]);
  const chat = useSyncExternalStore(chatController.subscribe.bind(chatController), () => chatController.snapshot(), () => chatController.snapshot());
  // Runs the latest read of the open task confirmed to be working normally.
  // They are not "something to check": only stopped or unconfirmed work raises
  // the bell's warning. Events alone never hide a run, since one that failed
  // its check could leave a stopped run looking live.
  const liveRunIds = useMemo(() => chat.agentRuns
    .filter(run => ["preparing", "running", "cancelling"].includes(run.state))
    .map(run => run.runId).sort(), [chat.agentRuns]);
  // One task status for the list, result bar, changes panel and run cards.
  // Finished tasks not open here are asked about once per version, so the
  // list can say "merged" or "applied" too (task-disposition.ts).
  const dispositions = useMemo(() => new TaskDispositionStore(async chatId => {
    const item = chatController.snapshot().chats.find(entry => entry.chatId === chatId);
    if (!item || !window.m5Projects) return null;
    const mergeRequest = { action: "inspectChatMerge" as const, chatId: item.chatId, projectId: item.projectId, worktreeId: item.worktreeId, workspaceIdentity: item.workspaceIdentity };
    const merge = validateProjectCatalogResult(await window.m5Projects.request(mergeRequest), mergeRequest);
    if (merge.action !== "inspectChatMerge") return null;
    if (merge.merge.kind === "gitWorktree") return dispositionFromMerge(merge.merge);
    const applyRequest = { action: "inspectChatApply" as const, projectId: item.projectId, chatId: item.chatId };
    const apply = validateProjectCatalogResult(await window.m5Projects.request(applyRequest), applyRequest);
    return apply.action === "inspectChatApply" ? dispositionFromApply(apply) : null;
  }, browserStorage()), [chatController]);
  useSyncExternalStore(dispositions.subscribe, () => dispositions.version(), () => dispositions.version());
  // A kept standing is shown, then checked again whenever a project opens:
  // the folder or the base branch can change without the task's record.
  useEffect(() => { dispositions.revalidate(); }, [dispositions, chat.projectId]);
  useEffect(() => { if (admissionReady) dispositions.request(chat.chats.filter(item => item.chatId !== chat.activeChat?.chatId)); }, [dispositions, chat.chats, chat.activeChat?.chatId, admissionReady, chat.projectId]);
  // The open task's merge and apply standings are read again whenever its
  // changes are (a save in the workspace, a refresh).
  const [diffRevision, setDiffRevision] = useState(0);
  useEffect(() => { setDiffRevision(value => value + 1); }, [snapshot.diffSummary]);
  const activeChatId = chat.activeChat?.chatId ?? null;
  /** "Look again" (and after a merge or apply): read the open task's standing anew. */
  const [standingNonce, setStandingNonce] = useState(0);
  const standingGeneration = useRef(0);
  // One place reads the open task's standing (standing-read.ts): the panels'
  // own reads are only for their buttons and never write it (review N3).
  const standingFor = chat.activeChat ? { chatId: chat.activeChat.chatId, updatedAt: chat.activeChat.updatedAt } : null;
  const view = standingView(standing, standingFor);
  const mergeInfo = view.merge;
  const onApprovalsWaiting = useCallback((count: number) => { if (activeChatId) setApprovalsWaiting({ chatId: activeChatId, count }); }, [activeChatId]);
  // The latest read wins: after a failed read, no earlier read or kept
  // standing is shown as current (review N2, #4). While a read is out, a kept
  // standing of this exact task version shows.
  const activeReadFailed = view.failed;
  const activeDisposition = view.disposition ?? (chat.activeChat && !view.failed ? dispositions.get(chat.activeChat) : null);
  useEffect(() => { if (chat.activeChat && standing?.status === "known" && view.disposition) dispositions.record(chat.activeChat, view.disposition); });
  // The open task's latest failed read is the list's too: after switching
  // away, its row says "check failed", never an older "merged" (review #4).
  useEffect(() => { if (chat.activeChat && view.failed) dispositions.reportFailed(chat.activeChat); }, [chat.activeChat?.chatId, chat.activeChat?.updatedAt, view.failed, dispositions]);
  const activePhase: TaskPhase | null = chat.activeChat ? taskPhase({ state: chat.activeChat.state, disposition: activeDisposition,
    approvalPending: (approvalsWaiting?.chatId === activeChatId ? approvalsWaiting.count : 0) > 0 }) : null;
  const activeKind = activeDisposition?.kind ?? view.kind;
  const activeWorking = activePhase !== null && phaseIsWorking(activePhase);
  // Opening a task that is not working settles its old permission notices.
  useEffect(() => { if (chat.activeChat && activePhase && !activeWorking && activePhase !== "attention") notifications.resolveApprovalsFor(chat.activeChat.chatId); }, [chat.activeChat?.chatId, activePhase, activeWorking]);
  // Reading the open task's standing runs on its own, apart from whether the
  // result bar shows (N2): for this task version, whenever its changes or
  // "look again" change and the team is not working. Leaving this effect for
  // any reason (another task, the team starting, a newer read) moves the
  // generation on, so a late answer is dropped (N3). No answer in time is a
  // failed read.
  useEffect(() => {
    const target = chat.activeChat;
    const leave = () => { standingGeneration.current++; };
    if (!target || activeWorking || chat.switching || !admissionReady || !window.m5Projects) {
      // Work again after a merge or apply: what was read no longer holds.
      if (activeWorking) setStanding(null);
      return leave;
    }
    const generation = ++standingGeneration.current;
    const task = { chatId: target.chatId, updatedAt: target.updatedAt };
    setStanding(previous => startStandingRead(previous, task, generation));
    const ask = async <T,>(request: Promise<T>): Promise<T> => {
      let timer = 0;
      try { return await Promise.race([request, new Promise<never>((_, reject) => { timer = window.setTimeout(() => reject(new Error("standing read timed out")), STANDING_READ_TIMEOUT_MS); })]); }
      finally { window.clearTimeout(timer); }
    };
    void (async () => {
      let readKind: StandingRead["kind"] = null;
      try {
        const mergeRequest = { action: "inspectChatMerge" as const, chatId: target.chatId, projectId: target.projectId, worktreeId: target.worktreeId, workspaceIdentity: target.workspaceIdentity };
        const merge = validateProjectCatalogResult(await ask(window.m5Projects!.request(mergeRequest)), mergeRequest);
        if (merge.action !== "inspectChatMerge") throw new Error("unexpected merge answer");
        readKind = merge.merge.kind === "snapshot" ? "copy" : "git";
        let apply: StandingRead["apply"] = null;
        if (merge.merge.kind === "snapshot" && generation === standingGeneration.current) {
          const applyRequest = { action: "inspectChatApply" as const, projectId: target.projectId, chatId: target.chatId };
          const answer = validateProjectCatalogResult(await ask(window.m5Projects!.request(applyRequest)), applyRequest);
          if (answer.action !== "inspectChatApply") throw new Error("unexpected apply answer");
          apply = { applied: answer.applied, pending: answer.pending, total: answer.total };
        }
        const settled = merge.merge;
        setStanding(previous => finishStandingRead(previous, { ...task, generation, merge: settled, apply }, standingGeneration.current));
      } catch { setStanding(previous => failStandingRead(previous, { ...task, generation, kind: readKind }, standingGeneration.current)); }
    })();
    return leave;
  }, [chat.activeChat?.chatId, chat.activeChat?.updatedAt, activeWorking, chat.switching, admissionReady, diffRevision, standingNonce]);
  const dispositionKnown = standingRead({ disposition: activeDisposition, failed: activeReadFailed });
  const resultView = activePhase ? resultBarView({ phase: activePhase, kind: activeKind, known: dispositionKnown, switching: chat.switching, failed: activeReadFailed }) : null;
  /** "Check again": the open task and every finished task in the list. */
  const recheckStandings = () => { setStandingNonce(value => value + 1); dispositions.revalidate(); dispositions.request(chat.chats.filter(item => item.chatId !== chat.activeChat?.chatId)); };
  // The built-in team names no model; a task starts only once every role has one.
  const unselectedRoles = chat.settings ? (() => { const team = roleSettingsAsV3(chat.settings); return team.roles.filter(role => !hasUsableModel(role.runtime, role.modelSelector)).map(role => roleLabel(role.roleId, team.roleDefinitions, t)); })() : [];
  useEffect(() => window.m5AppUpdate?.onRestartCheck(() => {
    const ready = !settingsOpen && !appDefaultsOpen && !newChatOpen && !reloadDraft && !reloadingDraft
      && !leavingProject && !leaveError && controller.canLeaveProject() && chatController.canLeaveProjectView();
    if (ready) flushSync(() => setUpdatePreparing(true));
    return ready;
  }, () => setUpdatePreparing(false)), [settingsOpen, appDefaultsOpen, newChatOpen, reloadDraft, reloadingDraft, leavingProject, leaveError, controller, chatController]);
  const openDiffFile = useCallback((path: string) => { void controller.openFile(path); }, [controller]);
  const readDiff = useCallback((path: string) => controller.readDiff(path), [controller]);
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
      // Closed on purpose: the next start shows the home screen.
      forgetLastProject(browserStorage());
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
  // Adding a folder can reveal a hidden or new catalog entry even when opening
  // it then stops (recovery, busy agent); refresh the lists after every attempt.
  const addProject = () => controller.chooseProject().finally(() => setCatalogRevision(value => value + 1));
  // Deleting is the user's call; running work is stopped first.
  const canRemove = admissionReady && !settingsOpen && !newChatOpen && !removing && !leavingProject && !chat.switching && !chat.preparing;
  // Files go once the stopped runs have provably exited; ask again shortly
  // after a deletion and once at startup.
  const finishDeletions = useCallback(() => {
    if (!window.m5Projects) return;
    const input = { action: "finishDeletions" as const };
    void window.m5Projects.request(input).then(raw => validateProjectCatalogResult(raw, input)).catch(() => undefined);
  }, []);
  useEffect(() => { if (admissionReady) finishDeletions(); }, [admissionReady, finishDeletions]);
  const deleteTarget = async (target: { projectId: string; chatId?: string }) => {
    if (!canRemove || !window.m5Projects) return;
    setRemoving(true); setRemovalError(false);
    const wasCurrent = controller.snapshot().project?.id === target.projectId;
    const wasActiveChat = Boolean(target.chatId) && chat.activeChat?.chatId === target.chatId;
    try {
      const input = target.chatId ? { action: "deleteChat" as const, projectId: target.projectId, chatId: target.chatId, confirmed: true as const }
        : { action: "deleteProject" as const, projectId: target.projectId, confirmed: true as const };
      // The sidecar records the deletion and stops what still runs there; then
      // the open view closes even if those runs are still winding down.
      const result = validateProjectCatalogResult(await window.m5Projects.request(input), input);
      // Deleting the latest task closes the project's own copy (it was that
      // task's workspace); the project then reopens from the original.
      const projectClosed = result.action === "deleteChat" && result.projectClosed;
      notifications.removeFor(target.projectId, target.chatId);
      if (wasCurrent && (!target.chatId || wasActiveChat || projectClosed) && controller.snapshot().project?.id === target.projectId) {
        await window.m5Projects.request({ action: "releaseView", projectId: target.projectId }).catch(() => undefined);
        if (controller.snapshot().project?.id === target.projectId) {
          chatController.clearProjectView(true);
          controller.leaveProjectView(true);
        }
        // A deleted task leaves its project open on the project's own files.
        if (wasActiveChat || projectClosed) await controller.reopenProject(target.projectId, request => window.m5Projects!.request(request)).catch(() => undefined);
      } else if (wasCurrent && target.chatId) {
        await chatController.refreshChatList();
        controller.revealChatList();
      }
      if (!target.chatId) setCatalogRevision(value => value + 1);
      setRemoval(null);
      for (const delay of [2_000, 8_000, 30_000]) window.setTimeout(finishDeletions, delay);
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
  // Notifications: a per-viewer list of finished, waiting and stopped tasks.
  const notifications = useMemo(() => new NotificationCenter(), []);
  const [pendingOpen, setPendingOpen] = useState<{ projectId: string; chatId: string } | null>(null);
  const revealStoryRef = useRef<() => void>(() => {});
  const openNotification = (item: AppNotification) => {
    if (controller.snapshot().project?.id === item.projectId) {
      // The open task: close what covers its story and go to the request (review #1).
      if (chat.activeChat?.chatId !== item.chatId) void switchChat(item.chatId);
      else revealStoryRef.current();
      return;
    }
    if (!window.m5Projects) return;
    setPendingOpen({ projectId: item.projectId, chatId: item.chatId });
    void controller.reopenProject(item.projectId, input => window.m5Projects!.request(input));
  };
  const openNotificationRef = useRef(openNotification);
  openNotificationRef.current = openNotification;
  useEffect(() => {
    if (pendingOpen && chat.projectId === pendingOpen.projectId && chat.chats.some(item => item.chatId === pendingOpen.chatId)) {
      setPendingOpen(null);
      if (chat.activeChat?.chatId !== pendingOpen.chatId) void switchChat(pendingOpen.chatId);
    }
  }, [pendingOpen, chat.projectId, chat.chats]);
  // Reopening a project picks up the task last open in it, the way an editor
  // reopens its last file; only once per opening, so closing or deleting a
  // task never pulls another one in behind the user's back.
  const resumedProject = useRef<string | null>(null);
  useEffect(() => {
    if (chat.activeChat) { try { localStorage.setItem(lastChatKey(chat.activeChat.projectId), chat.activeChat.chatId); } catch { /* optional */ } }
  }, [chat.activeChat?.chatId]);
  useEffect(() => {
    if (!chat.projectId) { resumedProject.current = null; return; }
    if (!chat.settings || resumedProject.current === chat.projectId || !admissionReady) return;
    resumedProject.current = chat.projectId;
    if (chat.activeChat || chat.switching || pendingOpen || !chat.chats.length) return;
    let remembered: string | null = null;
    try { remembered = localStorage.getItem(lastChatKey(chat.projectId)); } catch { /* optional */ }
    const target = chat.chats.find(item => item.chatId === remembered) ?? chat.chats[0];
    void switchChat(target.chatId);
  }, [chat.projectId, chat.settings, admissionReady]);
  useEffect(() => startNotificationFeed(bridge as unknown as NotificationFeedBridge, notifications, window.m5Approval, id => {
    // A macOS banner only while the app is in the background.
    if (document.hasFocus() || !osNotificationsEnabled() || typeof Notification === "undefined") return;
    const item = notifications.snapshot().find(entry => entry.id === id);
    if (!item) return;
    try {
      const banner = new Notification(t(`m5.notifications.kind.${item.kind}`), { body: item.text ? `${item.goal}\n${item.text}` : item.goal });
      banner.onclick = () => { window.focus(); notifications.markRead(item.id); openNotificationRef.current(item); };
    } catch { /* banners are optional */ }
  }), [bridge, notifications]);
  // A role still without a model starts on its provider's recommendation, so
  // a first task never needs a trip to the settings screen. Saved like any
  // settings change; a provider without one recommendation leaves the choice
  // to the user.
  const [recommendationMissing, setRecommendationMissing] = useState(false);
  // "View all" in the changes panel: every file's comparison in the editor area.
  const [allChanges, setAllChanges] = useState<{ focus: { path: string; at: number } | null } | null>(null);
  useEffect(() => { setAllChanges(null); }, [snapshot.project?.id, snapshot.workspace?.workspaceIdentity, snapshot.diffFile]);
  useEffect(() => setRecommendationMissing(false), [chat.settings?.projectId, chat.settings?.revision]);
  const fillRecommendedRoles = async (): Promise<boolean> => {
    const settings = chatController.snapshot().settings;
    if (!settings) return false;
    const team = roleSettingsAsV3(settings);
    if (team.roles.every(role => hasUsableModel(role.runtime, role.modelSelector))) return true;
    if (!window.m5Models) { setRecommendationMissing(true); return false; }
    try {
      const catalogs = new Map<RoleProfileV1["runtime"], Awaited<ReturnType<typeof loadModels>>["models"]>();
      for (const runtime of new Set(team.roles.map(role => role.runtime))) catalogs.set(runtime, (await loadModels(runtime, input => window.m5Models!.request(input))).models);
      const filled = withRecommendedModels(team.roles, catalogs);
      if (!filled) { setRecommendationMissing(true); return false; }
      await chatController.updateSettings(filled, team.roleDefinitions);
      return chatController.snapshot().settings?.revision !== settings.revision;
    } catch { setRecommendationMissing(true); return false; }
  };
  const createChat = async () => {
    if (!snapshot.project) return;
    controller.setActionBlocked(true, { key: "m5.editorNotice.newChat" });
    if (!await fillRecommendedRoles()) { controller.setActionBlocked(chatController.isActionBlocked()); return; }
    // The first task's confirmed setup includes the permissions shown checked.
    if (chat.chats.length === 0 && setupConfirmed && firstPermissions?.projectId === snapshot.project.id && firstPermissions.needsSave && window.m5Projects) {
      const request = { action: "setAutomationPermissions" as const, projectId: snapshot.project.id, permissions: firstPermissions.permissions, confirmed: true as const };
      try { validateProjectCatalogResult(await window.m5Projects.request(request), request); setPermissionsSaveFailed(false); setFirstPermissions(null); chosenPermissions.current.delete(request.projectId); }
      catch { controller.setActionBlocked(chatController.isActionBlocked()); setPermissionsSaveFailed(true); return; }
    }
    const created = await chatController.createCoordinatorChat({ projectId: snapshot.project.id }, goal);
    if (created) { setGoal(""); setNewChatOpen(false); await switchChat(created.chatId); if (chatController.snapshot().activeChat?.chatId === created.chatId) await chatController.startAgent("coordinator", lang); }
    else controller.setActionBlocked(chatController.isActionBlocked());
  };

  const activeTab = snapshot.activePath
    ? snapshot.tabs.find((tab) => tab.path === snapshot.activePath) ?? null
    : null;
  const openRoleSettings = (fromNewChat = false) => {
    if (!admissionReady || !projectDataReady || settingsOpen || chat.preparing || chat.switching) return;
    setAppDefaultsOutcome(null);
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

  // The task's changes are already in the user's folder (merged or applied),
  // read from the single task status only.
  const mergedIntoBase = activePhase === "merged";
  // Applied or merged and nothing new: the list is what this task changed.
  const changesDone = activePhase !== null && phaseIsSettled(activePhase);
  // The result box is the one place that merges or applies; elsewhere only points there.
  const focusResultBox = () => {
    setSheetOpen(false); setAllChanges(null);
    showResults();
    requestAnimationFrame(() => {
      const box = document.querySelector<HTMLElement>("[data-testid=result-box]");
      const target = box?.querySelector<HTMLButtonElement>(".m5-merge > button:not(:disabled), .m5-apply-latest button:not(:disabled)") ?? box;
      target?.scrollIntoView({ block: "nearest" });
      target?.focus();
    });
  };

  // ===== Task layout (ADE 1차-나): list · story · results, and a wide review sheet =====
  const [viewportWidth, setViewportWidth] = useState(() => window.innerWidth);
  useEffect(() => {
    // The CSS switches columns on these media queries; listen to the same
    // ones so the rail and drawer never lag the columns (a resize event can
    // come late), and to resize for the panel width limits.
    const update = () => setViewportWidth(window.innerWidth);
    const queries = [BAND_WIDE_MIN, BAND_REGULAR_MIN, BAND_NARROW_MIN].map(width => window.matchMedia(`(min-width: ${width}px)`));
    queries.forEach(query => query.addEventListener("change", update));
    window.addEventListener("resize", update);
    update();
    return () => { queries.forEach(query => query.removeEventListener("change", update)); window.removeEventListener("resize", update); };
  }, []);
  const layoutBandNow: LayoutBand = layoutBand(viewportWidth);
  const [rightTab, setRightTab] = useState<RightTab>("changes");
  const [drawerOpen, setDrawerOpen] = useState(false);
  // ⌘\ hides the results panel in wide windows.
  const [rightCollapsed, setRightCollapsed] = useState(false);
  /** Open what holds the results (panel or drawer) before showing a tab or the result box (review #2). */
  const showResults = () => { const next = revealResults(bandUsesDrawer(layoutBand(window.innerWidth))); setRightCollapsed(next.rightCollapsed); if (next.drawerOpen) setDrawerOpen(true); };
  useEffect(() => { if (!bandUsesDrawer(layoutBandNow)) setDrawerOpen(false); }, [layoutBandNow]);
  const [rightWidthPreference, setRightWidthPreference] = useState<number | null>(() => { try { return parseRightWidth(localStorage.getItem(RIGHT_WIDTH_KEY)); } catch { return null; } });
  const rightWidth = clampRightWidth(rightWidthPreference ?? bandSizes(layoutBandNow).right, viewportWidth);
  const [sheetOpen, setSheetOpen] = useState(false);
  // Permission requests pin above the composer, whatever the scroll (L4b).
  const [pinnedSlot, setPinnedSlot] = useState<HTMLDivElement | null>(null);
  const sheetMode: SheetMode = allChanges || snapshot.diffVisible ? "changes" : "editor";
  const closeSheet = () => { setSheetOpen(false); setAllChanges(null); };
  const selectSheetMode = (mode: SheetMode) => {
    if (mode === "editor") { setAllChanges(null); controller.selectPanel("editor"); return; }
    if (!allChanges && !snapshot.diffVisible) setAllChanges({ focus: null });
  };
  const selectRightTab = (tab: RightTab) => setRightTab(tab);
  // Keyboard (PLAN 1.6): ⌘1 list · ⌘2 composer · ⌘3 results · ⌘\ results open/closed · ⌘N new task.
  // Esc closes the sheet or drawer (task-layout.tsx).
  const shortcutState = useRef({ open: false, drawer: false, newTask: () => {} });
  shortcutState.current = {
    open: Boolean(snapshot.project) && !preferencesOpen && !settingsOpen && !newChatOpen,
    drawer: bandUsesDrawer(layoutBandNow),
    newTask: () => { if (admissionReady && projectDataReady && !chat.preparing && !chat.switching) setNewChatOpen(true); },
  };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const state = shortcutState.current;
      const mod = /Mac/.test(navigator.platform) ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
      if (!mod || event.altKey || event.shiftKey || !state.open || document.querySelector("dialog[open]")) return;
      const focus = (selector: string) => requestAnimationFrame(() => document.querySelector<HTMLElement>(selector)?.focus());
      if (event.key === "1") { event.preventDefault(); focus("[data-testid=layout-left] [aria-current=true], [data-testid=layout-left] .m5-new-task, [data-testid=layout-left] .m5-rail-group"); }
      else if (event.key === "2") { event.preventDefault(); focus("[data-testid=layout-center] #m5-followup-text, [data-testid=layout-center] .m5-timeline-list"); }
      else if (event.key === "3") { event.preventDefault(); if (state.drawer) setDrawerOpen(true); else setRightCollapsed(false); focus("[data-testid=layout-right] [role=tab][aria-selected=true]"); }
      else if (event.key === "\\") { event.preventDefault(); if (state.drawer) setDrawerOpen(open => !open); else setRightCollapsed(value => !value); }
      else if (event.key.toLowerCase() === "n") { event.preventDefault(); state.newTask(); }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
  // What covers the preview area (for the isolated preview view, P3); one place.
  // The open task's state words (review #3, #4): the team really working, and
  // the changes tab's line, which claims nothing while the standing is unread.
  const activeTeamWorking = teamWorking(chat.activeChat?.state, chat.agentStartingRoleId !== null);
  const changesLineNow = chat.activeChat && activePhase ? changesLine({ phase: activePhase, working: activeTeamWorking, kind: activeKind, known: activeDisposition !== null, failed: activeReadFailed, changeCount: snapshot.diffSummary?.entries.length ?? 0 }) : null;
  const changesLineSettled = changesLineNow === null || changesLineNow === "notYet" || changesLineNow === "done" || changesLineNow === "changedSince";
  const approvalWaitingNow = Boolean(activeChatId && approvalsWaiting?.chatId === activeChatId && approvalsWaiting.count > 0);
  /** Close what covers the story and go to the waiting request (or the composer) (review #1). */
  const revealStory = () => {
    setSheetOpen(false); setAllChanges(null); setDrawerOpen(false);
    // Focus the request card itself, never its first button ("stop run" would be one Enter away).
    window.setTimeout(() => {
      const card = document.querySelector<HTMLElement>("[data-testid=pinned-approval] .m5-approval-card");
      if (card) { card.tabIndex = -1; card.focus(); card.scrollIntoView({ block: "nearest" }); }
      else document.querySelector<HTMLElement>("[data-testid=layout-center] #m5-followup-text")?.focus();
    }, 80);
  };
  revealStoryRef.current = revealStory;
  // A page named in the files tab, and a key that changes when a run of the
  // task ends (the preview then offers "새 결과 · 다시 불러오기").
  const [previewEntry, setPreviewEntry] = useState<PreviewEntryRequest | null>(null);
  /** "미리보기로 열기" (files tab, or the editor sheet over it): the sheet closes, the preview tab shows that page. */
  const openInPreview = (path: string) => {
    setPreviewEntry(value => ({ path, nonce: (value?.nonce ?? 0) + 1 }));
    setSheetOpen(false); setAllChanges(null);
    setRightTab("preview");
    showResults();
  };
  const previewResultKey = `${chat.activeChat?.chatId ?? ""}:${chat.agentRuns.filter(run => !["preparing", "running", "cancelling"].includes(run.state)).length}`;
  const previewCovered = previewObscured({ sheetOpen, drawerUsed: bandUsesDrawer(layoutBandNow), drawerOpen, dialogOpen: newChatOpen || Boolean(removal) || preferencesOpen || settingsOpen, popoverOpen: false, rightTab });

  // A task opens on the results tab its state is about (PLAN 1.5); once per
  // task. A task whose changes include a page opens on the preview; the
  // changes may arrive after the task opened, so a tab picked here (and not
  // since changed) moves to the preview then.
  const previewPageNow = Boolean(chat.activeChat && snapshot.workspace?.chatId === chat.activeChat.chatId && hasPreviewPage(snapshot.diffSummary?.entries));
  const defaultTabFor = useRef<string | null>(null);
  const pickedTab = useRef<RightTab | null>(null);
  useEffect(() => {
    const id = chat.activeChat?.chatId ?? null;
    if (!id || chat.switching || !activePhase) return;
    if (defaultTabFor.current === id) {
      const better = defaultRightTab(activePhase, previewPageNow);
      if (better === "preview" && pickedTab.current !== null && pickedTab.current !== better && rightTab === pickedTab.current) { pickedTab.current = better; setRightTab(better); }
      return;
    }
    defaultTabFor.current = id;
    pickedTab.current = defaultRightTab(activePhase, previewPageNow);
    setRightTab(pickedTab.current);
  }, [chat.activeChat?.chatId, chat.switching, activePhase, previewPageNow]); // eslint-disable-line react-hooks/exhaustive-deps
  // When the team finishes, the results show what it made (PLAN 1.5: review → changes).
  const wasWorking = useRef<{ chatId: string; working: boolean } | null>(null);
  useEffect(() => {
    const id = chat.activeChat?.chatId ?? null;
    if (!id || chat.switching || !activePhase) return;
    const before = wasWorking.current;
    wasWorking.current = { chatId: id, working: activeWorking };
    if (before?.chatId === id && before.working && !activeWorking) { pickedTab.current = defaultRightTab(activePhase, previewPageNow); setRightTab(pickedTab.current); }
  }, [chat.activeChat?.chatId, chat.switching, activePhase, activeWorking]); // eslint-disable-line react-hooks/exhaustive-deps
  const dirtyTabs = snapshot.tabs.filter(tab => tab.saveState !== "clean").length;
  // "View all changes" opens the sheet; another task or project closes it.
  useEffect(() => { if (allChanges) setSheetOpen(true); }, [allChanges]);
  useEffect(() => { setSheetOpen(false); }, [snapshot.project?.id, snapshot.workspace?.workspaceIdentity]);
  // The editor controller still asks for "files" | "editor" | "changes" (and
  // the conversation or task list); the layout reads those requests here.
  useEffect(() => {
    const request = snapshot.panelRequest;
    if (!request) return;
    const target = panelRequestTarget(request.panel, controller.snapshot().diffVisible);
    if (target.kind === "sheet") { setSheetOpen(true); return; }
    if (target.kind === "tab") {
      setRightTab(target.tab);
      showResults();
      return;
    }
    setSheetOpen(false); setAllChanges(null); setDrawerOpen(false);
    requestAnimationFrame(() => document.querySelector<HTMLElement>(target.kind === "story"
      ? "[data-testid='layout-center'] #m5-followup-text, [data-testid='layout-center'] .m5-timeline-list"
      : "[data-testid='layout-left'] [aria-current='true'], [data-testid='layout-left'] .m5-new-chat-button")?.focus());
  }, [snapshot.panelRequest]);

  // The task list's rows: the same task status the open task's panels read (task-status.ts).
  const activeRun = chat.agentRuns.find(run => ["preparing", "running", "cancelling"].includes(run.state)) ?? null;
  const taskListItems: TaskListItem[] = chat.chats.map(item => {
    const selected = chat.activeChat?.chatId === item.chatId;
    const phase = selected && activePhase ? activePhase : taskPhase({ state: item.state, disposition: dispositions.get(item) });
    const kind = selected ? activeKind : dispositions.get(item)?.kind ?? null;
    const state = `${taskPhaseTone(phase)} m5-task-phase-${phase}`;
    const changed = chat.refreshNeededChatIds.includes(item.chatId);
    // A finished task whose standing could not be read again says so
    // instead of keeping an old "merged" or "applied" (review #4).
    // The open task's latest read counts too (review #4).
    const readFailed = selected ? activeReadFailed : dispositions.failed(item);
    const status = changed ? t("m5.navigation.newState") : readFailed ? t("m5.task.readFailed") : t(taskPhaseLabelKey(phase, kind));
    const role = activeRun && selected ? roleLabel(activeRun.selectedProfile.roleId, chat.activeChat?.roleDefinitions, t) : null;
    return {
      chatId: item.chatId, goal: item.goal, phase, readFailed, updatedAt: item.updatedAt, selected, status,
      statusClass: changed ? "m5-chat-update" : readFailed ? "m5-chat-status m5-chat-status-neutral" : `m5-chat-status m5-chat-status-${state}`,
      statusPhase: readFailed ? "readFailed" : phase,
      detail: !changed && role && phase === "attention" ? { kind: "approval" as const, role } : !changed && role && activeRun && (phase === "running" || phase === "draft") ? { kind: "working" as const, role, startedAt: activeRun.createdAt } : undefined,
      fileCount: selected && (phase === "review" || phase === "changedSince") ? snapshot.diffSummary?.entries.length : undefined,
    };
  });

  const preferencesButton = <button type="button" className="m5-secondary m5-open-preferences" disabled={settingsOpen || newChatOpen || appDefaultsOpen} onClick={() => setPreferencesOpen(true)} title={t("m5.preferences.title")} aria-label={t("m5.preferences.title")}><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round"><path d="m10 3-.6 2.4-2 .9L5 5.6 3 9l1.8 1.7v2.6L3 15l2 3.4 2.4-.7 2 .9L10 21h4l.6-2.4 2-.9 2.4.7 2-3.4-1.8-1.7v-2.6L21 9l-2-3.4-2.4.7-2-.9L14 3Z"/><circle cx="12" cy="12" r="3"/></svg><span>{t("m5.preferences.title")}</span></button>;
  // The project list: a rail on the home screen, folded into the project
  // switcher at the top of the task list once a project is open (1차-나).
  const projectRail = <ProjectCatalog onCloseProject={() => void leaveProject()} closeDisabled={!admissionReady || settingsOpen || leavingProject || leaveError || !controller.canLeaveProject() || !chatController.canLeaveProjectView()} onRoles={() => openRoleSettings()} rolesDisabled={!admissionReady || settingsOpen || !projectDataReady || chat.preparing || chat.switching} rail onAdd={addProject} railExpanded={snapshot.project ? true : railExpanded} onRailExpandedChange={setRailExpanded} refreshKey={catalogRevision} removeDisabled={!canRemove} onRemove={entry => { setRemovalError(false); setRemoval({ projectId: entry.projectId, label: entry.canonicalPath }); }} settingsAction={snapshot.project ? undefined : preferencesButton} onManage={entry => { setManagedProject(entry); setPreferencesSection("project"); setPreferencesOpen(true); }} onHasProjects={setHasSavedProjects} ready={admissionReady} controller={controller} currentId={snapshot.project?.id} disabled={settingsOpen || !admissionReady || snapshot.actionBlocked || !controller.canLeaveProject() || snapshot.selectionBlocked || Boolean(snapshot.workspace)} />;

  return (
    <main inert={updatePreparing} className={`m5-workbench${preferencesOpen ? " m5-settings-open" : ""}${snapshot.project ? " m5-task-mode" : window.m5Projects ? " m5-with-project-rail" : ""}`} lang={lang} aria-label={t("m5.shell.label")}>
      {!snapshot.project && projectRail}
      <dialog ref={removalDialog} className="m5-discard-dialog" aria-labelledby="m5-remove-title" onCancel={event => { event.preventDefault(); if (!removing) setRemoval(null); }}>
        <h2 id="m5-remove-title">{t("m5.catalog.remove")}</h2>
        <p>{removal?.label}</p><p>{t(removal?.chatId ? "m5.catalog.removeChatHelp" : "m5.catalog.removeHelp")}</p>
        {removalError && <p role="alert">{t("m5.catalog.removeError")}</p>}
        <div className="m5-dialog-actions"><button type="button" disabled={removing} onClick={() => setRemoval(null)}>{t("m5.editor.cancel")}</button>
        <button type="button" className="m5-danger" disabled={!canRemove} onClick={() => removal && void deleteTarget(removal)}>{t("m5.catalog.remove")}</button></div>
      </dialog>
      {!snapshot.project && !railExpanded && <button type="button" className="m5-sidebar-reopen" onClick={() => setRailExpanded(true)} aria-label={t("m5.catalog.expand")} title={t("m5.catalog.expand")} aria-expanded={false}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg></button>}
      <header className={`m5-toolbar${snapshot.project ? " m5-toolbar-project m5-topbar" : ""}`} data-testid="top-bar">
        {snapshot.project ? <div className="m5-topbar-path">
          <span className="m5-topbar-brand">Elsecrew</span>
          <h1 className="m5-topbar-crumb" title={chat.activeChat ? `${snapshot.project.name} › ${chat.activeChat.goal}` : snapshot.project.name}><span>{snapshot.project.name}</span>{chat.activeChat && !chat.switching && <><span aria-hidden="true"> › </span><b>{chat.activeChat.goal}</b></>}</h1>
        </div> : <div>
          <div className="m5-toolbar-title">

          <h1>{t(hasSavedProjects !== false ? "m5.home.welcome" : "m5.shell.start")}</h1></div>
          <p className="m5-subtitle">
            {t("m5.shell.intro")}
          </p>
        </div>}
        <div className="m5-toolbar-actions">
      {snapshot.project && bandUsesDrawer(layoutBandNow) && <nav className="m5-drawer-toggles" data-testid="drawer-toggles" aria-label={t("m5.layout.openResults")}>
        {RIGHT_TABS.map(tab => <button key={tab} type="button" aria-pressed={drawerOpen && rightTab === tab} data-testid={`drawer-toggle-${tab}`}
          onClick={() => { if (drawerOpen && rightTab === tab) setDrawerOpen(false); else { setRightTab(tab); setDrawerOpen(true); } }}>
          {t(`m5.layout.tab.${tab}`)}{tab === "changes" && snapshot.diffSummary && !snapshot.rescanRequired ? ` ${snapshot.diffSummary.entries.length}` : ""}
          {tab === "changes" && (activePhase === "review" || activePhase === "changedSince") && !drawerOpen && <span className="m5-layout-dot" aria-label={t("m5.layout.ready")} />}
        </button>)}
      </nav>}
      {/* Everything about what is happening lives with the bell: progress and
          errors under it, anything that still needs attention inside it. */}
      <NotificationBell center={notifications} onOpen={openNotification} hasProblem={workAttention}
        problem={<WorkStatusPanel bridge={bridge} ready={admissionReady} refreshKey={JSON.stringify([snapshot.error?.message, chat.notice, chat.activeChat?.state])} liveRunIds={liveRunIds} onAttention={setWorkAttention} />}
        errors={[snapshot.error ? { title: t(snapshot.error.messageKey), key: `snapshot:${snapshot.error.messageKey}:${snapshot.error.technicalDetail ?? ""}`, body: <>
          {snapshot.error.messageKey === "m5.editorError.reopenProject" && <button type="button" className="m5-secondary" disabled={settingsOpen || !admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked || Boolean(snapshot.workspace)} onClick={() => void addProject()}>{t("m5.editor.reselectOriginal")}</button>}
          <details><summary>{t("m5.feedback.details")}</summary><pre>{snapshot.error.technicalDetail}</pre></details></> }
          : null, chat.error ? { title: t(chat.error.messageKey), key: `chat:${chat.error.messageKey}:${chat.error.detail ?? ""}`, body: <>
          {chat.activeChat && chat.error.retryChatId === chat.activeChat.chatId && <button type="button" className="m5-secondary" disabled={!admissionReady || chat.switching} onClick={() => void chatController.retryHistory()}>{t("m5.chat.reloadHistory")}</button>}
          <details><summary>{t("m5.feedback.details")}</summary><pre>{chat.error.detail}</pre></details></> }
          : null].filter(error => error !== null)}
        // One place per sentence: a notice an open dialog or the admission
        // banner already says is not repeated as a toast (D4⑥).
        notice={chat.notice && !newChatOpen && !settingsOpen ? { text: formatChatNotice(chat, lang), tone: chatNoticeTone(chat.noticeKey), action: chatConnectionTarget(chat) ? <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => {
        const provider = chatConnectionTarget(chat);
        if (provider) { setConnectionReveal(current => ({ provider, sequence: (current?.sequence ?? 0) + 1 })); setPreferencesSection("agents"); setPreferencesOpen(true); }
      }}>{t("m5.setup.openConnection", { name: chatConnectionTarget(chat) === "codex" ? "Codex" : "Claude Code" })}</button> : undefined }
        : snapshot.notice && !(admission !== "ready" && snapshot.noticeKey === "m5.editorNotice.preparing") ? { text: snapshot.noticeKey ? t(snapshot.noticeKey) : snapshot.notice }
        : discardedWorkspaceNotice && !snapshot.project ? { text: t("m5.discard.done") } : null} />

      {snapshot.project && <ProjectGuide key={snapshot.project.id} hasChat={Boolean(chat.activeChat)}
        projectId={snapshot.project.id} activeChat={chat.activeChat} onResume={id => void switchChat(id)}
        hasOpenFile={snapshot.tabs.length > 0 || snapshot.diffVisible}
        disabled={!admissionReady || !projectDataReady || settingsOpen || snapshot.actionBlocked || chat.preparing || chat.switching}
        onGoal={() => setNewChatOpen(true)} onTeam={() => openRoleSettings()}
        onConversation={() => controller.revealConversation()}
        onDecision={() => focusResultBox()}
        onChanges={() => { controller.selectPanel("changes"); void controller.refreshChanges(); }} />}
      {snapshot.project && preferencesButton}
        </div>
      </header>



      {chat.activeChat && chat.activeChat.state !== "discarded" && <SaveRecoveryView address={{
        chatId: chat.activeChat.chatId, projectId: chat.activeChat.projectId,
        worktreeId: chat.activeChat.worktreeId, workspaceIdentity: chat.activeChat.workspaceIdentity,
      }} disabled={!admissionReady || chat.switching || settingsOpen || leavingProject || leaveError}
        onResolved={(result, path) => controller.observeCompletedSaveRecovery(result, path)} />}
      {leaveError && <p className="m5-error" role="alert">{t("m5.feedback.leaveError")}</p>}
      <AppSettings hasProject={Boolean(snapshot.project)} storage={<><WorkspaceFolderSetting disabled={!admissionReady} /><DiscardedWorkCleanup disabled={!admissionReady} /><BackupStorageUsageView disabled={!admissionReady} /></>} project={managedProject ? <><button type="button" className="m5-secondary" onClick={() => setManagedProject(null)}>{t("m5.preferences.projectList")}</button><p className="m5-managed-project"><strong>{managedProject.canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? managedProject.projectId}</strong><span title={managedProject.canonicalPath}>{managedProject.canonicalPath}</span></p><ProjectPermissions key={`permissions:${managedProject.projectId}`} projectId={managedProject.projectId} disabled={!admissionReady} /><details className="m5-troubleshooting"><summary>{t("m5.preferences.troubleshoot")}</summary><p className="m5-settings-intro">{t("m5.preferences.troubleshootHelp")}</p><ProjectRecoveryHistory key={`history:${managedProject.projectId}`} bridge={bridge} projectId={managedProject.projectId} disabled={!admissionReady || leavingProject || leaveError} /><PreparationRecovery projectId={managedProject.projectId} disabled={!admissionReady || snapshot.actionBlocked || snapshot.selectionBlocked || Boolean(snapshot.workspace) || settingsOpen || leavingProject || leaveError || !controller.canLeaveProject() || chat.switching} /></details></> : <ProjectCatalog management ready={admissionReady && preferencesOpen && preferencesSection === "project"} refreshKey={catalogRevision} controller={controller} disabled={!admissionReady} onManage={setManagedProject} />} restartLocked={updatePreparing} open={preferencesOpen} section={preferencesSection} onSection={section => { if (section !== preferencesSection) navigatePreferences(() => {
        // Like Orca's per-repository page: with a project open, its settings come first.
        if (section === "project" && !managedProject && snapshot.project) setManagedProject({ projectId: snapshot.project.id, canonicalPath: snapshot.project.root });
        setPreferencesSection(section); setAppDefaultsOpen(section === "roles"); }); }} onClose={() => navigatePreferences(() => { setPreferencesOpen(false); setPreferencesSection("general"); setAppDefaultsOpen(false); setManagedProject(null); })}
        rolesEditor={appDefaultsOpen && <AppRoleDefaultsEditor activeChat={chat.activeChat} navigationGuard={roleNavigation} disabled={!admissionReady} onClose={() => { setAppDefaultsOpen(false); setPreferencesSection("general"); }} />}
        disabled={!admissionReady || !window.m5Projects} onRoles={() => setAppDefaultsOpen(true)}>
        <ProviderLogin settingsPage disabled={!admissionReady} returning={hasSavedProjects === true} reveal={connectionReveal} />
      </AppSettings>
      <StorageAdmissionNotice admission={admission} />
      {engineRestarted && <section className="m5-admission" role="status"><strong>{t("m5.admission.restarted")}</strong><p>{t("m5.admission.restartedHelp")}</p><button type="button" className="m5-secondary" onClick={() => window.location.reload()}>{t("m5.admission.refresh")}</button></section>}
      {!snapshot.project && (hasSavedProjects === null || resumingProject
        ? <section className="m5-start-pending" role="status" aria-busy="true"><span className="m5-skeleton-line" />{resumingProject && <p>{t("m5.start.resuming")}</p>}</section>
        : <GuidedStart returning={hasSavedProjects} disabled={!admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked} onChoose={() => void addProject()} />)}
      {!snapshot.project && hasSavedProjects && !resumingProject && <ProjectCatalog home refreshKey={catalogRevision} removeDisabled={!canRemove} onRemove={entry => { setRemovalError(false); setRemoval({ projectId: entry.projectId, label: entry.canonicalPath }); }} onManage={entry => { setManagedProject(entry); setPreferencesSection("project"); setPreferencesOpen(true); }} ready={admissionReady} controller={controller}
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
        <form className="m5-new-chat-form" onKeyDown={submitChatShortcut} onSubmit={(event) => { event.preventDefault(); if (admissionReady && projectDataReady && goal.trim() && !chat.preparing && !chat.switching && !chat.preparationApiUnavailable && !(snapshot.workspace && !controller.canLeaveProject()) && (chat.chats.length > 0 || setupConfirmed) && !(recommendationMissing && unselectedRoles.length > 0)) void createChat(); }}>
          <input ref={goalInputRef} aria-label={t("m5.navigation.goal")} value={goal} onChange={(event) => setGoal(event.target.value)} placeholder={t("m5.navigation.goalPlaceholder")} disabled={!admissionReady || chat.preparing} />
          <p className="m5-first-chat-note">{sourceChanges?.applies
            ? <>{t("m5.newTask.worktreeNote")}{sourceChanges.uncommitted > 0 && <> {t("m5.newTask.uncommitted", { count: sourceChanges.uncommitted })}</>}</>
            : t("m5.application.autoBaseline")}</p>
          {sourceChanges?.applies && <details className="m5-first-chat-detail"><summary>{t("m5.feedback.details")}</summary><p>{t("m5.newTask.worktreeDetail")}</p></details>}
          <button type="submit" title={t("m5.chat.sendShortcut")} aria-keyshortcuts="Enter Meta+Enter Control+Enter" disabled={!admissionReady || !projectDataReady || !goal.trim() || chat.preparing || chat.preparationApiUnavailable || chat.switching || Boolean(snapshot.workspace && !controller.canLeaveProject()) || (chat.chats.length === 0 && !setupConfirmed) || (recommendationMissing && unselectedRoles.length > 0)}>{t("m5.chat.coordinatorSend")}</button>
        </form>
        {unselectedRoles.length > 0 && <p className="m5-team-unselected" role="status">{t(recommendationMissing ? "m5.firstChat.unselectedModels" : "m5.firstChat.recommendedModels", { roles: unselectedRoles.join(", ") })}</p>}
        {chat.chats.length === 0 ? <section className="m5-first-chat-setup" aria-labelledby="m5-first-chat-setup-heading">
          <h3 id="m5-first-chat-setup-heading">{t("m5.firstChat.setup.title")}</h3>
          <p>{t("m5.firstChat.setup.help")}</p>
          <h4>{t("m5.firstChat.team")}</h4>
          {chat.settings && <dl className="m5-new-chat-team">{roleSettingsAsV3(chat.settings).roles.map(role => <div key={role.roleId}><dt>{roleLabel(role.roleId, roleSettingsAsV3(chat.settings!).roleDefinitions, t)}</dt><dd>{role.runtime === "codex" ? "Codex" : "Claude Code"}<small>{hasUsableModel(role.runtime, role.modelSelector) ? modelLabel(role.runtime, role.modelSelector) : t("m5.roles.unselectedModel")}{role.reasoningEffort ? ` · ${role.reasoningEffort}` : ""}</small></dd></div>)}</dl>}
          <button type="button" className="m5-secondary" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching} onClick={() => openRoleSettings(true)}>{t("m5.firstChat.editTeam")}</button>
          <p>{t("m5.firstChat.editTeamHelp")}</p>
          <ProjectPermissions key={`${snapshot.project.id}:${newChatOpenings}`} projectId={snapshot.project.id} disabled={!admissionReady || chat.preparing || chat.switching} proposeAll chosen={chosenPermissions.current.get(snapshot.project.id) ?? null} onDraft={reportPermissions} />
          {permissionsSaveFailed && <p role="alert">{t("m5.permissions.error")}</p>}
          <label className="m5-first-chat-confirm"><input type="checkbox" checked={setupConfirmed} disabled={chat.preparing || chat.switching} onChange={event => setSetupConfirmed(event.target.checked)} /><span>{t("m5.firstChat.setup.confirm")}</span></label>
        </section> : <details className="m5-chat-team-details"><summary>{t("m5.firstChat.team")}</summary>
        {chat.settings && <dl className="m5-new-chat-team">{roleSettingsAsV3(chat.settings).roles.map(role => <div key={role.roleId}><dt>{roleLabel(role.roleId, roleSettingsAsV3(chat.settings!).roleDefinitions, t)}</dt><dd>{role.runtime === "codex" ? "Codex" : "Claude Code"}<small>{hasUsableModel(role.runtime, role.modelSelector) ? modelLabel(role.runtime, role.modelSelector) : t("m5.roles.unselectedModel")}{role.reasoningEffort ? ` · ${role.reasoningEffort}` : ""}</small></dd></div>)}</dl>}
        <button type="button" className="m5-secondary" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching} onClick={() => openRoleSettings(true)}>{t("m5.firstChat.editTeam")}</button>
        <p>{t("m5.firstChat.editTeamHelp")}</p>
        </details>}
        </dialog>
      </>}

      {settingsOpen && chat.settings && <RoleSettings key={`${chat.settings.projectId}:${chat.settings.revision}:${chat.activeChat?.chatId ?? "none"}`} settings={chat.settings} activeChat={chat.activeChat} disabled={!admissionReady} feedback={<>
        {!admissionReady && <StorageAdmissionNotice admission={admission} />}
        {chat.notice && <p className="m5-notice" role="status">{formatChatNotice(chat, lang)}</p>}
        {chat.error && <section className="m5-error" role="alert"><strong>{t(chat.error.messageKey)}</strong><details><summary>{t("m5.feedback.details")}</summary><pre>{chat.error.detail}</pre></details></section>}
        {appDefaultsOutcome === "both" && <p role="status">{t("m5.roles.appDefaultsAlsoSaved")}</p>}
        {appDefaultsOutcome === "projectOnly" && <p role="alert">{t("m5.roles.appDefaultsNotSaved")}</p>}
      </>} onAppDefaultsResult={outcome => setAppDefaultsOutcome(outcome === "none" ? null : outcome)} knownRoleIds={chat.chats.flatMap(item => item.roleSnapshots.map(role => role.roleId))} onSave={(roles, roleDefinitions) => chatController.updateSettings(roles, roleDefinitions)} onReload={() => chatController.reloadSettings()} onClose={closeRoleSettings} returnToDraft={returnToNewChat} projectName={snapshot.project?.name ?? chat.settings.projectId} activeTasks={chat.chats.filter(item => teamWorking(item.state)).length} />}

      {snapshot.project && <TaskLayout band={layoutBandNow} rightTab={rightTab} onRightTab={selectRightTab} rightCollapsed={rightCollapsed} previewCovered={previewCovered}
        approvalNotice={approvalCovered({ waiting: approvalWaitingNow, sheetOpen, drawerOpen: bandUsesDrawer(layoutBandNow) && drawerOpen })
          ? <div className="m5-approval-covered" role="alert" data-testid="approval-covered"><strong>{t("m5.layout.approvalWaiting")}</strong><button type="button" onClick={revealStory}>{t("m5.layout.approvalShow")}</button></div> : null}
        drawerOpen={drawerOpen} onDrawerOpenChange={setDrawerOpen} rightWidth={rightWidth} onRightWidth={setRightWidthPreference}
        sheetOpen={sheetOpen} onSheetClose={closeSheet} sheetLabel={t(sheetMode === "changes" ? "m5.sheet.changes" : "m5.sheet.editor")}
        list={<>
          <TaskList rail={bandUsesDrawer(layoutBandNow)} items={taskListItems} disabled={!admissionReady || settingsOpen || chat.switching} removeDisabled={!canRemove}
            onOpen={chatId => void switchChat(chatId)}
            onRemove={chatId => { const item = chat.chats.find(entry => entry.chatId === chatId); if (item) { setRemovalError(false); setRemoval({ projectId: item.projectId, chatId: item.chatId, label: item.goal }); } }}
            header={<>
              <ProjectSwitcher key={snapshot.project.id} name={snapshot.project.name} root={snapshot.project.root}>{projectRail}</ProjectSwitcher>
              <button ref={newChatButtonRef} type="button" className="m5-new-chat-button m5-new-task" title={t("m5.taskList.newShortcut")} disabled={!admissionReady || !projectDataReady || settingsOpen || chat.preparing || chat.switching} onClick={() => setNewChatOpen(true)}><span aria-hidden="true">＋</span><span className="m5-new-task-label">{t("m5.navigation.newChat")}</span></button>
            </>}
            footer={<div className="m5-task-list-footer">
              {removalError && !removal && <p role="alert">{t("m5.catalog.removeError")}</p>}
              {chat.chats.length > 0 && <button type="button" className="m5-link-button m5-task-recheck" disabled={!admissionReady || chat.switching} onClick={recheckStandings}>{t("m5.task.recheck")}</button>}
              {chat.nextChatCursor !== null && <button className="m5-secondary" type="button" disabled={!admissionReady || chat.loadingMoreChats || removing} onClick={() => void chatController.loadMoreChats()}>{chat.loadingMoreChats ? t("m5.navigation.loadingChats") : t("m5.navigation.moreChats")}</button>}
            </div>} />
        </>}
        story={chat.activeChat ? <div className="m5-conversation"><div className="m5-conversation-scroll"><TeamTimeline onCoordinatorChat={() => { setGoal(chat.activeChat?.goal ?? ""); setNewChatOpen(true); }} controller={chatController} chat={chat} admissionReady={admissionReady} fixtureAvailable={fixtureAvailable} phase={activePhase} phaseKind={activeKind} onApprovalsWaiting={onApprovalsWaiting} pinnedSlot={pinnedSlot} /></div><div className="m5-pinned-slot" data-testid="pinned-approval" ref={setPinnedSlot} />{chat.activeChat.schemaVersion !== 1 && <FollowupComposer controller={chatController} chat={chat} admissionReady={admissionReady} />}</div> : <section className="m5-first-chat" aria-label={t("m5.firstChat.title")}>
        <h2>{t("m5.firstChat.title")}</h2><p>{t("m5.firstChat.help")}</p>
        <button type="button" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching} onClick={() => setNewChatOpen(true)}>{t("m5.navigation.newChat")}</button>
        <div className="m5-goal-examples">{(["explain", "plan", "readme"] as const).map(example => <button key={example} type="button" className="m5-secondary" disabled={!admissionReady || !projectDataReady || chat.preparing || chat.switching || goal.trim().length > 0} onClick={() => { setGoal(t(`m5.firstChat.${example}`)); setNewChatOpen(true); }}>
          {t(`m5.firstChat.${example}`)}
        </button>)}</div><p className="m5-first-chat-note">{t("m5.firstChat.exampleHelp")}</p>
        {chat.settings && <details className="m5-chat-team-details"><summary>{t("m5.firstChat.team")}</summary><dl className="m5-team-preview">{roleSettingsAsV3(chat.settings).roles.map(role => <div key={role.roleId}><dt>{roleLabel(role.roleId, roleSettingsAsV3(chat.settings!).roleDefinitions, t)}</dt><dd>{role.runtime === "codex" ? "Codex" : "Claude Code"}<small>{hasUsableModel(role.runtime, role.modelSelector) ? modelLabel(role.runtime, role.modelSelector) : t("m5.roles.unselectedModel")}{role.reasoningEffort ? ` · ${role.reasoningEffort}` : ""}</small></dd></div>)}</dl><p className="m5-first-chat-note">{t("m5.firstChat.teamHelp")}</p></details>}
      </section>}
        resultBox={chat.activeChat && <ResultBox view={resultView} mode={resultBoxMode({ phase: activePhase, working: activeTeamWorking, switching: chat.switching, hasView: resultView !== null })}
          detail={resultView && <>{resultView.loading ? <span className="m5-skeleton-line" aria-label={t("m5.task.loading")} />
            : resultView.detailKey === "m5.task.result.review" ? <>{snapshot.diffSummary ? <>{t("m5.result.files", { count: snapshot.diffSummary.entries.length })} <LineCounts entry={snapshot.diffSummary.entries.reduce((sum, entry) => ({ added: sum.added + (entry.added ?? 0), removed: sum.removed + (entry.removed ?? 0) }), { added: 0, removed: 0 })} /> · </> : null}{t(resultView.detailKey)}</>
            : resultView.failed ? <>{t("m5.task.result.readFailed")} <button type="button" className="m5-link-button" disabled={!admissionReady} onClick={() => setStandingNonce(value => value + 1)}>{t("m5.merge.inspectAgain")}</button></>
            : resultView.detailKey ? t(resultView.detailKey, { base: mergeInfo?.kind === "gitWorktree" ? mergeInfo.baseRef ?? "main" : "main", count: activeDisposition?.pending ?? 0 }) : null}</>}
          actions={resultView && <span className="m5-result-actions-inner" inert={chat.switching} aria-hidden={chat.switching || undefined}>
          {/* Viewing this task's changes never waits for its standing; deciding does. */}
          <button type="button" className="m5-secondary" disabled={!admissionReady || chat.switching} onClick={() => { controller.selectPanel("changes"); void controller.refreshChanges(); setAllChanges({ focus: null }); }}>{t("m5.result.view")}</button>
          <span className="m5-result-decide" inert={resultView.loading} aria-hidden={resultView.loading || undefined}>
          <ChatWorkspaceMerge key={`merge:${chat.activeChat.chatId}`} chat={chat.activeChat} revision={`${diffRevision}`} disabled={!admissionReady || resultView.loading} merged={mergedIntoBase}
            onMerged={() => { dispositions.forget(chat.activeChat!.chatId); setStandingNonce(value => value + 1); void controller.refreshChanges(); }}
            onAskTeam={text => { chatController.editFollowup(text); void chatController.appendFollowup(admissionReady, true, lang); }}
            fallback={null} />
          <ChatWorkspaceDiscard key={chat.activeChat.chatId} compact chat={chat.activeChat} disabled={!admissionReady || settingsOpen || !controller.canLeaveProject() || !chatController.canLeaveProjectView()} onCompleted={async discarded => {
        const request = { action: "releaseView" as const, projectId: discarded.projectId };
        validateProjectCatalogResult(await window.m5Projects!.request(request), request);
        if (!controller.leaveProjectView() || !chatController.clearProjectView()) throw new Error("discarded workspace view could not be released");
        setDiscardedWorkspaceNotice(true);
      }} />
          </span>
        </span>}
          extra={<>{chat.activeChat && activePhase && !chat.switching && !phaseIsWorking(activePhase) && view.kind === "copy" && <ApplyLatest revision={`${diffRevision}`} projectId={chat.activeChat.projectId} chatId={chat.activeChat.chatId} changeCount={snapshot.diffSummary?.entries.length ?? 0} onApplied={() => { dispositions.forget(chat.activeChat!.chatId); setStandingNonce(value => value + 1); }} disabled={!admissionReady || chat.switching || chatController.isActionBlocked() || chat.activeChat.state === "running" || chat.agentRuns.some(run => ["preparing", "running", "cancelling"].includes(run.state))} />}</>} />}
        tabs={[
          { id: "preview", label: t("m5.layout.tab.preview"), content: <PreviewTab projectId={chat.activeChat?.projectId ?? null} chatId={chat.switching ? null : chat.activeChat?.chatId ?? null}
            active={rightTab === "preview" && !(bandUsesDrawer(layoutBandNow) ? !drawerOpen : rightCollapsed)} covered={previewCovered} resultKey={previewResultKey}
            entryRequest={previewEntry} onChanges={() => setRightTab("changes")} /> },
          { id: "changes", label: t("m5.layout.tab.changes"), count: snapshot.rescanRequired ? undefined : snapshot.diffSummary?.entries.length, content: <>
        <aside className="m5-panel m5-changes" aria-label={t("m5.editor.changes")}>
          {snapshot.project ? (
            <>
              {snapshot.diffLoading && <p className="m5-empty" role="status">{t("m5.editor.loadingChanges")}</p>}
              {/* A task worktree merges (result bar); only a copy task applies. */}
              {/* One status line, read from the task status: while the team works it never says merged or applied. */}
              {chat.activeChat && activePhase && !chat.switching && changesLineNow && !changesLineSettled && <ChangesStatusLine line={changesLineNow} count={snapshot.diffSummary?.entries.length ?? 0} phase={activePhase} onRetry={() => setStandingNonce(value => value + 1)} retryDisabled={!admissionReady} />}
              {chat.activeChat && activePhase && !chat.switching && changesLineSettled && !phaseIsWorking(activePhase) && view.kind === "git" && <ChangesMergeStatus state={mergeInfo} phase={activePhase} changeCount={snapshot.diffSummary?.entries.length ?? 0} canMerge={resultView !== null && !resultView.loading} onMerge={focusResultBox} />}
              {chat.activeChat && activePhase && !chat.switching && changesLineSettled && !phaseIsWorking(activePhase) && view.kind === "copy" && <ChangesStandingLine line={changesLineNow} pending={activeDisposition?.pending ?? 0} onGoToResult={focusResultBox} />}
              {snapshot.diffSummary && changesDone && !chat.switching && <p className="m5-changes-done-label">{t("m5.changes.doneList")}</p>}
              {snapshot.diffSummary && <ChangeList key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}`} entries={snapshot.diffSummary.entries} selected={allChanges ? allChanges.focus?.path : snapshot.diffFile?.path} disabled={!admissionReady || snapshot.actionBlocked}
                onRefresh={() => { recheckStandings(); void controller.refreshChanges(); }} refreshing={snapshot.diffLoading} allOpen={Boolean(allChanges)} onViewAll={() => { if (allChanges) setAllChanges(null); else { setAllChanges({ focus: null }); controller.selectPanel("editor"); } }}
                onOpen={path => { if (allChanges) setAllChanges({ focus: { path, at: Date.now() } }); else void controller.openDiff(path); }} />}
            </>
          ) : <Empty text={t("m5.editor.chooseProject")} />}
        </aside>
          </> },
          { id: "files", label: t("m5.layout.tab.files"), content: <>
        <aside className="m5-panel m5-files" aria-label={t("m5.navigation.files")}>
          {snapshot.project ? (
            <>
              <label className="m5-hidden-files"><input type="checkbox" checked={showHiddenFiles} onChange={event => setShowHiddenFiles(event.target.checked)} />{t("m5.files.showHidden")}</label>
              {chat.activeChat && snapshot.workspace && isPreviewablePage(snapshot.activePath) && <button type="button" className="m5-secondary m5-preview-open-file" data-testid="preview-open-file"
                onClick={() => openInPreview(snapshot.activePath!)}>{t("m5.preview.openFile")}</button>}
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
          </> },
          { id: "history", label: t("m5.layout.tab.history"), content: <HistoryTab controller={chatController} chat={chat} admissionReady={admissionReady} fixtureAvailable={fixtureAvailable} phase={activePhase} /> },
        ]}
        sheet={<ReviewSheet mode={sheetMode} onMode={selectSheetMode} entries={snapshot.diffSummary?.entries ?? null}
          selectedPath={allChanges ? allChanges.focus?.path ?? null : snapshot.diffVisible ? snapshot.diffFile?.path ?? null : null}
          onSelectFile={path => { if (allChanges) setAllChanges({ focus: { path, at: Date.now() } }); else void controller.openDiff(path); }}
          dirty={dirtyTabs} onClose={closeSheet}>
        <section className="m5-panel m5-editor" aria-label={t("m5.editor.title")}>
          <div hidden={snapshot.tabs.length === 0 || snapshot.diffVisible || Boolean(allChanges)} className="m5-tab-list" role="tablist" aria-label={t("m5.editor.tabs")}>
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
          <div hidden={!activeTab || snapshot.diffVisible || Boolean(allChanges)} className="m5-editor-head">
            {snapshot.diffVisible && <button type="button" className="m5-secondary" onClick={() => controller.selectPanel("editor")}>{t("m5.editor.title")}</button>}
            <span title={snapshot.diffVisible ? snapshot.diffFile?.path : activeTab?.path}>{(snapshot.diffVisible ? snapshot.diffFile?.path : activeTab?.path) ?? t("m5.editor.select")}</span>
            {!snapshot.diffVisible && chat.activeChat && snapshot.workspace && isPreviewablePage(activeTab?.path) && <button type="button" className="m5-secondary" data-testid="sheet-preview-open-file" onClick={() => openInPreview(activeTab!.path)}>{t("m5.preview.openFile")}</button>}
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
          <div className="m5-editor-surface" hidden={snapshot.diffVisible || Boolean(allChanges)}>
          <EditorContent key={`${snapshot.project.id}:${snapshot.workspace?.workspaceIdentity ?? "project"}`} tab={activeTab} readOnly={!admissionReady || snapshot.actionBlocked} onChange={(text) => activeTab && controller.updateText(activeTab.path, text)} onSave={(path) => {
            if (admissionReadyRef.current && controller.snapshot().activePath === path) void controller.saveActive();
          }} />
          </div>
              {allChanges && snapshot.diffSummary && <AllChanges entries={snapshot.diffSummary.entries} focusPath={allChanges.focus} read={readDiff}
                renderComparison={(path, before, after) => <MonacoDiff path={path} before={before} after={after} sideBySide wrap fit prose={isProsePath(path)} onFailed={() => {}} />}
                onOpenFile={path => { setAllChanges(null); void controller.openFile(path); }} onClose={() => setAllChanges(null)} />}
              {!allChanges && snapshot.diffVisible && snapshot.diffFile && <DiffDetails onReturn={() => controller.selectPanel("editor")} descriptor={snapshot.diffFile} text={snapshot.diffText} canOpen={admissionReady && !snapshot.actionBlocked} onOpen={openDiffFile}
                canDiscuss={admissionReady && !snapshot.actionBlocked && Boolean(snapshot.workspace) && chatController.canAppendFollowup() && !chat.followups?.[chat.activeChat?.chatId ?? ""]?.request}
                onDiscuss={discussDiff} />}
        </section>
        </ReviewSheet>} />}
      <footer className="m5-app-statusbar" aria-label={t("m5.usage.title")}><ProviderUsage disabled={!admissionReady}/></footer>
    </main>
  );
}


const lastChatKey = (projectId: string) => `elsecrew.lastChat.v1.${projectId}`;

function ProjectGuide({ projectId, activeChat, onResume, hasChat, hasOpenFile, disabled, onGoal, onTeam, onConversation, onDecision, onChanges }: {
  projectId: string; activeChat: ChatRecordV1 | null; onResume: (chatId: string) => void;
  hasChat: boolean; hasOpenFile: boolean; disabled: boolean; onGoal: () => void; onTeam: () => void; onConversation: () => void;
  /** Merging and applying live in the result box only (1차-나): the guide's "apply or recover" opens it. */
  onDecision: () => void; onChanges: () => void;
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
      {step === "review" && <><button type="button" disabled={disabled} onClick={onChanges}>{t("m5.guide.openChanges")}</button><button type="button" disabled={disabled} onClick={onDecision}>{t("m5.guide.openDecision")}</button></>}
      <button type="button" className="m5-secondary" onClick={() => { if (savePreference({ ...preference, dismissed: true })) setOpen(false); }}>{t("m5.guide.later")}</button>
      {saveFailed && <p role="status">{t("m5.guide.saveFailed")}</p>}
    </section>
    <details><summary>{t("m5.guide.allSteps")}</summary><ol>
      <li><h3>{t("m5.guide.goal")}</h3><p>{t("m5.guide.goalHelp")}</p><button type="button" disabled={disabled} onClick={onGoal}>{t("m5.navigation.newChat")}</button><button type="button" className="m5-secondary" disabled={disabled} onClick={onTeam}>{t("m5.firstChat.editTeam")}</button></li>
      <li><h3>{t("m5.guide.observe")}</h3><p>{t("m5.guide.observeHelp")}</p><button type="button" className="m5-secondary" disabled={disabled || !hasChat} onClick={onConversation}>{t("m5.guide.openConversation")}</button></li>
      <li><h3>{t("m5.guide.review")}</h3><p>{t("m5.guide.reviewHelp")}</p><button type="button" className="m5-secondary" disabled={disabled || !hasChat} onClick={onChanges}>{t("m5.guide.openChanges")}</button><button type="button" className="m5-secondary" disabled={disabled || !hasChat} onClick={onDecision}>{t("m5.guide.openDecision")}</button></li>
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
