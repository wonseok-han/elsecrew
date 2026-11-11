import { validateProjectCatalogResult, type TeamTaskCancelRequest } from "./contracts/project-catalog-v1.ts";
import { ko, type MessageKey } from "../locales/ko.ts";
import { chatNotice, recoveryConfirmation, agentContextConfirmation, type ChatNoticeKey, type ChatNoticeContext } from "./chat-notice.ts";
type ChatErrorKey = Extract<MessageKey, `m5.chatError.${string}`>;
import type { M5BridgeV3 } from "./contracts/bridge-v3.ts";
import type { M5BridgeV4 } from "./contracts/bridge-v4.ts";
import type { M5BridgeV5 } from "./contracts/bridge-v5.ts";
import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";
import type { M5BridgeV7 } from "./contracts/bridge-v7.ts";
import type { M5BridgeV8 } from "./contracts/bridge-v8.ts";
import { coordinatorRoles, validateChatAppendMessageRequestV1, validateChatAppendMessageTranscriptV1, type ChatAppendMessageRequestV1 } from "./contracts/run-recovery-v1.ts";

export interface FollowupDraft {
  text: string;
  request: ChatAppendMessageRequestV1 | null;
  sending: boolean;
  error: string | null;
}
import { isAgentChatDetailPageV1, type AgentRecoveryRecordV1, type AgentRunRecordV1, type AgentPublicEventV1, type AgentRunStateChangedV1 } from "./contracts/agent-adapter-v1.ts";
import type {
  ChatRecordV1, ProjectRoleSettingsV1, RecoveryRecordV1, RoleIdV1, RoleProfileV1,
  RunRecordV1, TeamMessageV1, PreparedChatWorkspaceV1,
} from "./contracts/run-recovery-v1.ts";

export interface ChatUiSnapshot {
  followups?: Readonly<Record<string, FollowupDraft>>;
  projectId: string | null;
  settings: ProjectRoleSettingsV1 | null;
  chats: readonly ChatRecordV1[];
  nextChatCursor: string | null;
  loadingMoreChats: boolean;
  activeChat: ChatRecordV1 | null;
  messages: readonly TeamMessageV1[];
  runs: readonly RunRecordV1[];
  agentRuns: readonly AgentRunRecordV1[];
  agentStartingRoleId: RoleIdV1 | null;
  agentRecovery: AgentRecoveryRecordV1 | null;
  agentRecoveryResolving: boolean;
  recoveries: readonly RecoveryRecordV1[];
  refreshNeededChatIds: readonly string[];
  preparing: boolean;
  switching: boolean;
  preparationApiUnavailable: boolean;
  activeRunId: string | null;
  notice: string | null;
  noticeKey?: ChatNoticeKey;
  noticeContext?: ChatNoticeContext;
  error: { message: string; messageKey: ChatErrorKey; detail: string; retryChatId?: string } | null;
}

type M5ChatBridge = M5BridgeV3 | M5BridgeV4 | M5BridgeV5 | M5BridgeV6 | M5BridgeV7 | M5BridgeV8;
type ChatDetail = Awaited<ReturnType<M5BridgeV4["getChat"]>>;

/** Renderer-only projection. The sidecar owns all durable identity, state transitions and CAS. */
export class ChatUiController {
  readonly #bridge: M5ChatBridge;
  readonly #listeners = new Set<() => void>();
  #unsubscribe: (() => void) | null = null;
  #agentUnsubscribe: (() => void) | null = null;
  #agentStateUnsubscribe: (() => void) | null = null;
  #disposed = false;
  #creatingCoordinator = false;
  #generation = 0;
  #pending: { chat: ChatRecordV1; generation: number; messages: TeamMessageV1[]; detail: ChatDetail | null } | null = null;
  #hydrations = new Map<string, Promise<void>>();
  #hydrationTriggers = new Map<string, number>();
  #state: ChatUiSnapshot = { projectId: null, settings: null, chats: [], nextChatCursor: null, loadingMoreChats: false, activeChat: null, messages: [], runs: [], agentRuns: [], agentStartingRoleId: null, agentRecovery: null, agentRecoveryResolving: false, recoveries: [], refreshNeededChatIds: [], preparing: false, switching: false, preparationApiUnavailable: false, activeRunId: null, notice: null, error: null };

  constructor(bridge: M5ChatBridge) { this.#bridge = bridge; }
  snapshot(): ChatUiSnapshot { return this.#state; }
  canAppendFollowup(): boolean {
    const chat = this.#state.activeChat;
    return !this.#disposed && (this.#bridge.version === 7 || this.#bridge.version === 8) && Boolean(chat)
      && !this.#state.switching && !this.isRecoveryBlocked()
      && ["draft", "running", "waitingForUser", "reviewReady", "needsAttention", "canceled"].includes(chat!.state);
  }
  editFollowup(text: string): void {
    const chat = this.#state.activeChat;
    if (!chat || this.#disposed) return;
    const draft = this.#state.followups?.[chat.chatId] ?? { text: "", request: null, sending: false, error: null };
    this.#setFollowup(chat.chatId, { ...draft, text });
  }
  addReviewReference(workspace: Pick<ChatRecordV1, "chatId" | "projectId" | "worktreeId" | "workspaceIdentity">, reference: string): boolean {
    const chat = this.#state.activeChat;
    if (!chat || !this.canAppendFollowup() || !reference.trim()) return false;
    if (chat.chatId !== workspace.chatId || chat.projectId !== workspace.projectId
      || chat.worktreeId !== workspace.worktreeId || chat.workspaceIdentity !== workspace.workspaceIdentity) return false;
    const draft = this.#state.followups?.[chat.chatId];
    // An uncertain delivery must retry the original immutable message first.
    if (draft?.sending || draft?.request) return false;
    this.editFollowup(`${draft?.text ? `${draft.text}\n\n` : ""}${reference}\n`);
    return true;
  }
  async appendFollowup(admissionReady: boolean, sendToCoordinator = false, lang: "ko" | "en" = "ko"): Promise<void> {
    const chat = this.#state.activeChat;
    const bridge = this.#bridge;
    const draft = chat && this.#state.followups?.[chat.chatId];
    if (sendToCoordinator && (chat?.schemaVersion !== 2 || chat.state === "running" || this.#state.agentStartingRoleId || this.#state.agentRuns.some(isAgentActive))) return;
    if (!admissionReady || !chat || (bridge.version !== 7 && bridge.version !== 8) || !draft || draft.sending || !this.canAppendFollowup() || (!draft.request && !draft.text.trim())) return;
    const generation = this.#generation;
    let request: ChatAppendMessageRequestV1;
    try {
      request = draft.request ?? validateChatAppendMessageRequestV1({
        projectId: chat.projectId, chatId: chat.chatId, taskId: chat.taskId,
        worktreeId: chat.worktreeId, workspaceIdentity: chat.workspaceIdentity,
        clientMessageId: `user_${crypto.randomUUID().replace(/-/g, "")}`,
        expectedEventSequence: this.#state.messages[this.#state.messages.length - 1]?.eventSequence ?? 0,
        text: draft.text,
      });
    } catch (error) {
      this.#setFollowup(chat.chatId, { ...draft, error: error instanceof Error ? error.message : String(error) });
      return;
    }
    this.#setFollowup(chat.chatId, { ...draft, request, sending: true, error: null });
    try {
      const message = await bridge.appendChatMessage(request);
      validateChatAppendMessageTranscriptV1({ request, message });
      if (this.#disposed) return;
      const latest = this.#state.followups?.[chat.chatId];
      if (latest?.request !== request) return;
      this.#setFollowup(chat.chatId, { text: latest.text === request.text ? "" : latest.text, request: null, sending: false, error: null });
      if (this.#live(generation) && this.#state.activeChat?.chatId === chat.chatId) {
        this.#onEvent(message);
        if (sendToCoordinator && chat.schemaVersion === 2) await this.startAgent("coordinator", lang);
      }
    } catch (error) {
      if (this.#disposed) return;
      const latest = this.#state.followups?.[chat.chatId];
      if (latest?.request !== request) return;
      // Only the explicit no-write sequence rejection can rebase the same ID.
      // Unknown outcomes retain the exact request for deduplicated retry.
      let retry = request;
      const detail = error instanceof Error ? error.message : String(error);
      if (detail.includes("chat message sequence changed before append") && this.#live(generation) && this.#state.activeChat?.chatId === chat.chatId) {
        try {
          await this.#hydrateChat(chat, generation);
          const sequence = this.#state.messages[this.#state.messages.length - 1]?.eventSequence;
          if (sequence && this.#live(generation) && this.#state.activeChat?.chatId === chat.chatId) retry = { ...request, expectedEventSequence: sequence };
        } catch { /* Keep the original request when refresh cannot be verified. */ }
      }
      const current = this.#state.followups?.[chat.chatId];
      if (current?.request === request) this.#setFollowup(chat.chatId, { ...current, request: retry, sending: false, error: detail.slice(0, 2048) });
    }
  }
  #setFollowup(chatId: string, draft: FollowupDraft): void {
    this.#set({ followups: { ...this.#state.followups, [chatId]: draft } });
  }
  subscribe(listener: () => void): () => void { this.#listeners.add(listener); return () => this.#listeners.delete(listener); }
  start(): void {
    if (this.#unsubscribe) return;
    this.#disposed = false;
    this.#unsubscribe = this.#bridge.onChatEvent((event) => this.#onEvent(event));
    const bridge = this.#agentBridge();
    if (bridge) this.#agentUnsubscribe = bridge.onAgentPublicEvent((event) => this.#onAgentPublicEvent(event));
    const stateBridge = this.#agentStateBridge();
    if (stateBridge) this.#agentStateUnsubscribe = stateBridge.onAgentRunStateChanged((event) => this.#onAgentRunStateChanged(event));
  }
  dispose(): void {
    this.#disposed = true; this.#generation++; this.#unsubscribe?.(); this.#agentUnsubscribe?.(); this.#agentStateUnsubscribe?.();
    this.#unsubscribe = null; this.#agentUnsubscribe = null; this.#agentStateUnsubscribe = null; this.#hydrations.clear(); this.#hydrationTriggers.clear(); this.#listeners.clear();
  }

  async openProject(projectId: string): Promise<void> {
    const generation = ++this.#generation;
    this.#set({ projectId, settings: null, chats: [], nextChatCursor: null, loadingMoreChats: false, activeChat: null, messages: [], runs: [], agentRuns: [], agentStartingRoleId: null, agentRecovery: null, agentRecoveryResolving: false, recoveries: [], refreshNeededChatIds: [], notice: null, error: null });
    try {
      const [settings, page] = await Promise.all([
        this.#bridge.getProjectRoleSettings({ projectId }),
        this.#bridge.listChats({ projectId, cursor: null, limit: 100 }),
      ]);
      if (!this.#live(generation) || this.#state.projectId !== projectId) return;
      this.#set({ settings, chats: page.items, nextChatCursor: page.nextCursor });
    } catch (error) { if (this.#live(generation)) this.#error("m5.chatError.project", error); }
  }

  canLeaveProjectView(): boolean {
    return !this.#disposed && !this.#state.preparing && !this.#state.switching
      && !this.isRecoveryBlocked() && !this.#state.agentStartingRoleId && !this.#state.activeRunId
      && !this.#state.runs.some(isActive)
      && !this.#state.agentRuns.some(run => ["preparing", "running", "cancelling"].includes(run.state))
      && !Object.values(this.#state.followups ?? {}).some(draft => draft.sending);
  }

  clearProjectView(): boolean {
    if (!this.canLeaveProjectView()) return false;
    this.#generation++;
    this.#pending = null;
    this.#set({ projectId: null, settings: null, chats: [], nextChatCursor: null, loadingMoreChats: false,
      activeChat: null, messages: [], runs: [], agentRuns: [], agentStartingRoleId: null,
      agentRecovery: null, agentRecoveryResolving: false, recoveries: [], refreshNeededChatIds: [],
      preparing: false, switching: false, activeRunId: null, notice: null, error: null });
    return true;
  }

  beginLeavingProjectView(): boolean {
    if (!this.canLeaveProjectView()) return false;
    this.#set({ switching: true });
    return true;
  }

  finishLeavingProjectView(): void { this.#set({ switching: false }); }

  /** Refresh list metadata without replacing the selected chat or editor workspace. */
  async refreshChatList(): Promise<void> {
    const projectId = this.#state.projectId;
    if (!projectId || this.#disposed || this.#state.loadingMoreChats) return;
    const generation = this.#generation;
    this.#set({ loadingMoreChats: true });
    try {
      const page = await this.#bridge.listChats({ projectId, cursor: null, limit: 100 });
      if (!this.#live(generation) || this.#state.projectId !== projectId) return;
      if (page.projectId !== projectId || page.items.some(item => item.projectId !== projectId)) throw new Error("chat list identity mismatch");
      this.#set({ chats: page.items, nextChatCursor: page.nextCursor });
    } catch (error) {
      if (this.#live(generation) && this.#state.projectId === projectId) this.#error("m5.chatError.more", error);
    } finally {
      if (this.#live(generation) && this.#state.projectId === projectId) this.#set({ loadingMoreChats: false });
    }
  }

  async loadMoreChats(): Promise<void> {
    const { projectId, nextChatCursor: cursor, loadingMoreChats } = this.#state;
    if (!projectId || cursor === null || loadingMoreChats || this.#disposed) return;
    const generation = this.#generation;
    this.#set({ loadingMoreChats: true, error: null });
    try {
      const page = await this.#bridge.listChats({ projectId, cursor, limit: 100 });
      if (!this.#live(generation) || this.#state.projectId !== projectId) return;
      if (page.projectId !== projectId || page.items.some(item => item.projectId !== projectId) || page.nextCursor === cursor) throw new Error("chat page identity or cursor mismatch");
      const existing = new Set(this.#state.chats.map(item => item.chatId));
      const appended = page.items.filter(item => {
        if (existing.has(item.chatId)) return false;
        existing.add(item.chatId);
        return true;
      });
      this.#set({ chats: [...this.#state.chats, ...appended], nextChatCursor: page.nextCursor });
    } catch (error) {
      if (this.#live(generation) && this.#state.projectId === projectId) this.#error("m5.chatError.more", error);
    } finally {
      if (this.#live(generation) && this.#state.projectId === projectId) this.#set({ loadingMoreChats: false });
    }
  }

  async updateSettings(roles: readonly RoleProfileV1[]): Promise<void> {
    const settings = this.#state.settings;
    if (!settings) return;
    const generation = this.#generation;
    try {
      const updated = await this.#bridge.updateProjectRoleSettings({ projectId: settings.projectId, expectedRevision: settings.revision, roles });
      if (!this.#live(generation) || this.#state.projectId !== settings.projectId || this.#state.settings?.revision !== settings.revision) return;
      this.#set({ settings: updated, ...chatNotice("m5.chatNotice.settingsSaved"), error: null });
    } catch (error) {
      if (!this.#live(generation) || this.#state.projectId !== settings.projectId) return;
      this.#error("m5.chatError.settingsSave", error);
    }
  }

  async reloadSettings(): Promise<void> {
    const projectId = this.#state.projectId;
    if (!projectId) return;
    const generation = this.#generation;
    const settings = this.#state.settings;
    try {
      const updated = await this.#bridge.getProjectRoleSettings({ projectId });
      if (!this.#live(generation) || this.#state.projectId !== projectId || this.#state.settings !== settings) return;
      this.#set({ settings: updated, ...chatNotice("m5.chatNotice.settingsLoaded"), error: null });
    } catch (error) {
      if (this.#live(generation) && this.#state.projectId === projectId && this.#state.settings === settings) this.#error("m5.chatError.settingsLoad", error);
    }
  }

  async createCoordinatorChat(source: { projectId: string }, goal: string): Promise<ChatRecordV1 | null> {
    const settings = this.#state.settings;
    if (this.#creatingCoordinator || !settings || !goal.trim() || settings.projectId !== source.projectId || this.#state.preparing || this.#state.switching || this.isActionBlocked()) return null;
    const generation = this.#generation;
    this.#creatingCoordinator = true;
    try {
      if (settings.schemaVersion !== 2) {
        const updated = await this.#bridge.updateProjectRoleSettings({ projectId: source.projectId, expectedRevision: settings.revision, roles: coordinatorRoles(settings.roles) });
        if (!this.#live(generation) || this.#state.projectId !== source.projectId) return null;
        this.#set({ settings: updated });
      }
      return await this.createChat(source, goal, true);
    } catch (error) {
      if (this.#live(generation)) this.#error("m5.chatError.settingsSave", error);
      return null;
    } finally { this.#creatingCoordinator = false; }
  }

  async createChat(source: { projectId: string }, goal: string, applicationBaseline = true): Promise<ChatRecordV1 | null> {
    const settings = this.#state.settings;
    if (!settings || this.#state.switching || this.isRecoveryBlocked() || settings.projectId !== source.projectId || !goal.trim() || this.#state.preparing) return null;
    const generation = this.#generation;
    let prepared: PreparedChatWorkspaceV1 | null = null;
    this.#set({ preparing: true, preparationApiUnavailable: false, error: null, ...chatNotice("m5.chatNotice.preparing") });
    try {
      prepared = await this.#bridge.prepareChatWorkspace({ projectId: source.projectId, ...(applicationBaseline ? { applicationBaseline: true } : {}) });
      const manifest = prepared.manifest;
      if (prepared.bindingState !== "unbound" || manifest.projectId !== source.projectId) throw new Error("prepared workspace identity mismatch");
      const created = await this.#bridge.createChat({ preparationId: prepared.preparationId, projectId: source.projectId, worktreeId: manifest.worktreeId, workspaceIdentity: manifest.workspaceIdentity, goal: goal.trim(), expectedSettingsRevision: settings.revision });
      if (!this.#live(generation) || this.#state.projectId !== source.projectId) return created;
      this.#set({ chats: [created, ...this.#state.chats.filter((item) => item.chatId !== created.chatId)], ...chatNotice("m5.chatNotice.prepared") });
      return created;
    } catch (error) {
      if (prepared?.bindingState === "unbound") {
        const manifest = prepared.manifest;
        try { await this.#bridge.abandonPreparedWorkspace({ preparationId: prepared.preparationId, projectId: manifest.projectId, worktreeId: manifest.worktreeId, workspaceIdentity: manifest.workspaceIdentity }); } catch { /* original create error remains the user-facing evidence */ }
      }
      if (this.#live(generation)) this.#error("m5.chatError.prepare", error);
      return null;
    } finally { if (this.#live(generation)) this.#set({ preparing: false }); }
  }

  async prepareSwitch(chatId: string, canLeaveWorkspace: boolean): Promise<ChatRecordV1 | null> {
    if (this.#state.switching) return null;
    if (this.isActionBlocked()) { this.#set({ ...chatNotice("m5.chatNotice.protected") }); return null; }
    if (!canLeaveWorkspace) { this.#set({ ...chatNotice("m5.chatNotice.dirty") }); return null; }
    const chat = this.#state.chats.find((item) => item.chatId === chatId);
    if (!chat) return null;
    const generation = this.#generation;
    this.#pending = { chat, generation, messages: [], detail: null };
    this.#set({ switching: true, ...chatNotice("m5.chatNotice.switching"), error: null });
    try {
      await this.#hydrateChat(chat, generation);
      if (!this.#live(generation) || this.#pending?.chat.chatId !== chat.chatId) return null;
      return chat;
    } catch (error) { if (this.#live(generation)) this.#endSwitch("m5.chatError.history", error); return null; }
  }

  async commitPrepared(chatId: string, bound: boolean): Promise<void> {
    const pending = this.#pending;
    if (!pending || pending.chat.chatId !== chatId) return;
    if (!bound || !pending.detail) { this.#endSwitch("m5.chatError.workspace"); return; }
    const detail = pending.detail;
    this.#pending = null;
    this.#set({ activeChat: detail.chat, messages: pending.messages, runs: detail.runs, agentRuns: isAgentChatDetailPageV1(detail) ? detail.agentRuns : [], agentStartingRoleId: null, agentRecovery: agentRecoveryFor(detail), agentRecoveryResolving: false, recoveries: [], refreshNeededChatIds: this.#state.refreshNeededChatIds.filter((item) => item !== detail.chat.chatId), activeRunId: detail.runs.find(isActive)?.runId ?? null, switching: false, notice: null });
    await this.#loadRecoveries(detail.chat.chatId, pending.generation);
  }

  async startDemo(roleId: "planning" | "coding" | "review"): Promise<void> {
    const chat = this.#state.activeChat;
    if (!chat || this.#state.switching || this.#state.activeRunId || this.isActionBlocked()) return;
    const scenario = ({ planning: "planning-success", coding: "coding-success", review: "review-success" } as const)[roleId];
    this.#set({ activeRunId: "starting", error: null, ...chatNotice("m5.chatNotice.demo") });
    try {
      const run = await this.#bridge.startFixtureRun({ chatId: chat.chatId, roleId, scenario });
      if (this.#state.activeChat?.chatId !== chat.chatId) return;
      this.#set({ runs: [run, ...this.#state.runs.filter((item) => item.runId !== run.runId)], activeRunId: isActive(run) ? run.runId : null });
    } catch (error) { this.#error("m5.chatError.run", error); }
  }

  async cancelRun(): Promise<void> {
    const chat = this.#state.activeChat; const runId = this.#state.activeRunId; const generation = this.#generation;
    if (!chat || this.#state.switching || !runId || runId === "starting") return;
    try {
      const run = await this.#bridge.cancelRun({ chatId: chat.chatId, runId });
      if (!this.#live(generation) || this.#state.switching || this.#state.activeChat?.chatId !== chat.chatId || this.#state.activeRunId !== runId) return;
      this.#set({ runs: [run, ...this.#state.runs.filter((item) => item.runId !== run.runId)], activeRunId: null, ...chatNotice(run.state === "recoveryRequired" ? "m5.chatNotice.termination" : "m5.chatNotice.demoStopped") });
      await this.#loadRecoveries(chat.chatId, generation);
    } catch (error) { this.#error("m5.chatError.termination", error); }
  }

  async startAgent(roleId: RoleIdV1, lang: "ko" | "en" = "ko"): Promise<void> {
    const chat = this.#state.activeChat; const generation = this.#generation; const bridge = this.#agentBridge();
    if (!chat || !bridge || (chat.schemaVersion === 2 && chat.state === "running") || this.#state.switching || this.#state.activeRunId || this.#state.agentStartingRoleId || this.#state.agentRuns.some(isAgentActive) || this.isActionBlocked()) {
      if (chat && !bridge) this.#set({ ...chatNotice("m5.chatNotice.unavailable") });
      return;
    }
    const profile = chat.roleSnapshots.find((item) => item.roleId === roleId);
    if (!profile) return;
    this.#set({ agentStartingRoleId: roleId, error: null, ...chatNotice("m5.agentNotice.checking", { roleId, runtime: profile.runtime }) });
    try {
      const capability = await bridge.getAgentCapabilities({ runtime: profile.runtime, refresh: true, chatId: chat.chatId, roleId });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      if (capability.scope && (capability.scope.chatId !== chat.chatId || capability.scope.roleId !== roleId
        || capability.scope.worktreeId !== chat.worktreeId || capability.scope.workspaceIdentity !== chat.workspaceIdentity)) {
        throw new Error("Capability response does not match the selected chat and role");
      }
      if (!capability.canStart) {
        this.#set({ ...chatNotice("m5.agentNotice.blocked", { roleId, runtime: profile.runtime, reason: capability.blockedReason }) });
        return;
      }
      await this.#hydrateChat(chat, generation);
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      let inputMessageIds = this.#state.messages.map((message) => message.messageId);
      if (inputMessageIds.length === 0) {
        this.#set({ ...chatNotice("m5.chatNotice.noConversation") });
        return;
      }
      // The public request contract permits 100 IDs. Never silently truncate
      // history or expand the execution contract to accommodate a long chat.
      if (inputMessageIds.length > 100) {
        if (!globalThis.confirm(agentContextConfirmation(inputMessageIds.length, lang))) {
          this.#set({ ...chatNotice("m5.chatNotice.contextCanceled") });
          return;
        }
        inputMessageIds = [inputMessageIds[0]!, ...inputMessageIds.slice(-99)];
      }
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId
          || this.#state.switching || this.isActionBlocked() || this.#state.activeRunId
          || this.#state.agentRuns.some(isAgentActive)) return;
      const accepted = await bridge.startAgentRun({ chatId: chat.chatId, roleId, inputMessageIds, expectedCapabilityFingerprint: capability.fingerprint });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      this.#set({ ...chatNotice("m5.agentNotice.ready", { roleId, runtime: profile.runtime }) });
      await this.#hydrateChat(chat, generation);
      // The accepted binding is independently validated by the bridge. The durable ledger owns terminal state.
      if (accepted.runId.length === 0) throw new Error("empty accepted agent run identity");
    } catch (error) {
      if (this.#live(generation) && this.#state.activeChat?.chatId === chat.chatId) {
        this.#set({ notice: null });
        this.#error("m5.chatError.run", error);
      }
    }
    finally { if (this.#live(generation) && this.#state.activeChat?.chatId === chat.chatId) this.#set({ agentStartingRoleId: null }); }
  }

  async cancelTeamTask(send: (input: TeamTaskCancelRequest) => Promise<unknown>): Promise<void> {
    const chat = this.#state.activeChat; const generation = this.#generation;
    const trigger = [...this.#state.messages].reverse().find(message => message.kind === "user");
    if (!chat || chat.schemaVersion !== 2 || !trigger || this.#state.switching) return;
    const input: TeamTaskCancelRequest = { action: "cancelTeamTask", chatId: chat.chatId, taskId: chat.taskId, projectId: chat.projectId, worktreeId: chat.worktreeId, workspaceIdentity: chat.workspaceIdentity, triggerMessageId: trigger.messageId };
    try {
      validateProjectCatalogResult(await send(input), input);
      if (this.#live(generation) && this.#state.activeChat?.chatId === chat.chatId) await this.#hydrateChat(chat, generation);
    } catch (error) { this.#error("m5.chatError.agentTermination", error); }
  }

  async cancelAgentRun(run: AgentRunRecordV1): Promise<void> {
    const chat = this.#state.activeChat; const bridge = this.#agentBridge(); const generation = this.#generation;
    if (!chat || !bridge || !isAgentActive(run) || !matchesAgentRun(run, chat) || this.#state.switching) return;
    try {
      const result = await bridge.cancelAgentRun({ chatId: run.chatId, taskId: run.taskId, projectId: run.projectId, worktreeId: run.worktreeId, workspaceIdentity: run.workspaceIdentity, runId: run.runId });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      this.#set({ agentRecovery: result.outcome === "recoveryRequired" ? result.recovery : null, ...chatNotice(result.outcome === "recoveryRequired" ? "m5.chatNotice.agentRecovery" : "m5.chatNotice.agentStopped") });
      await this.#hydrateChat(chat, generation);
    } catch (error) { this.#error("m5.chatError.agentTermination", error); }
  }

  isRecoveryBlocked(): boolean { return this.#state.agentRecovery !== null || this.#state.activeChat?.state === "recoveryRequired" || this.#state.recoveries.some((item) => item.state === "unresolved"); }
  isActionBlocked(): boolean { return this.isRecoveryBlocked() || this.#state.activeChat?.state === "discarded"; }
  async abandonRecovery(recovery: RecoveryRecordV1, lang: "ko" | "en" = "ko"): Promise<void> {
    if (!window.confirm(recoveryConfirmation(false, lang))) return;
    try { await this.#bridge.markRecoveryResolved({ recoveryId: recovery.recoveryId, expectedRevision: recovery.revision, resolution: "abandon" }); await this.#loadRecoveries(recovery.chatId, this.#generation); this.#set({ ...chatNotice("m5.chatNotice.discarded") }); }
    catch (error) { this.#error("m5.chatError.pendingTermination", error); }
  }

  async abandonAgentRecovery(lang: "ko" | "en" = "ko"): Promise<void> {
    const chat = this.#state.activeChat; const recovery = this.#state.agentRecovery; const bridge = this.#agentBridge(); const generation = this.#generation;
    if (!chat || !recovery || !bridge || !canAbandonAgentRecovery(recovery) || this.#state.switching || this.#state.agentRecoveryResolving || !matchesAgentRun(recovery, chat)) return;
    if (!globalThis.confirm(recoveryConfirmation(true, lang))) return;
    this.#set({ agentRecoveryResolving: true, error: null });
    try {
      const inspected = await bridge.inspectAgentRecovery(agentRecoveryIdentity(recovery));
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId || this.#state.agentRecovery?.recoveryId !== recovery.recoveryId) return;
      if (!matchesAgentRun(inspected, chat) || inspected.recoveryId !== recovery.recoveryId || inspected.state !== "unresolved" || !inspected.poisoned) throw new Error("agent recovery inspect identity mismatch");
      const result = await bridge.resolveAgentRecovery({ ...agentRecoveryIdentity(inspected), expectedRevision: inspected.revision, resolution: "abandon" });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId || this.#state.agentRecovery?.recoveryId !== recovery.recoveryId) return;
      const notice = result.outcome === "resolvedAbandoned"
        ? "m5.chatNotice.abandonRecorded"
        : result.outcome === "processesRemain"
          ? "m5.chatNotice.stillProtected"
          : "m5.chatNotice.recoveryChanged";
      this.#set(chatNotice(notice));
      await this.#hydrateChat(chat, generation);
    } catch (error) { this.#error("m5.chatError.protected", error); }
    finally { if (this.#live(generation) && this.#state.activeChat?.chatId === chat.chatId) this.#set({ agentRecoveryResolving: false }); }
  }

  async #loadRecoveries(chatId: string, generation: number): Promise<void> { const page = await this.#bridge.listRecoveries({ cursor: null, limit: 100 }); if (this.#live(generation) && this.#state.activeChat?.chatId === chatId) this.#set({ recoveries: page.items.filter((item) => item.chatId === chatId) }); }
  #onEvent(event: TeamMessageV1): void {
    const pending = this.#pending;
    if (pending && matchesChat(event, pending.chat)) {
      // Detail hydration owns ordering; a seq2 event may legitimately arrive before detail seq1.
      if (!pending.messages.some((message) => message.messageId === event.messageId)) pending.messages.push(event);
      return;
    }
    const chat = this.#state.activeChat;
    if (!chat || !matchesChat(event, chat)) return;
    const tail = this.#state.messages[this.#state.messages.length - 1]?.eventSequence ?? 0;
    if (event.eventSequence > tail + 1) {
      // Provider final messages arrive through durable hydration; a following
      // team notice can win that race. Fetch the missing ledger, never skip it.
      this.#hydrateAfterNotification(chat, this.#generation);
      return;
    }
    try { this.#set({ messages: mergeMessages(this.#state.messages, [event]) }); } catch (error) { this.#error("m5.chatError.order", error); }
  }
  #onAgentPublicEvent(event: AgentPublicEventV1): void {
    this.#hydrateForAgentRun(event);
  }
  #onAgentRunStateChanged(event: AgentRunStateChangedV1): void {
    // This notification only schedules durable chat.get hydration. Never render its run record directly.
    this.#hydrateForAgentRun(event.run);
  }
  #hydrateForAgentRun(run: Pick<AgentRunRecordV1, "chatId" | "taskId" | "projectId" | "worktreeId" | "workspaceIdentity">): void {
    const pending = this.#pending;
    if (pending && matchesAgentRun(run, pending.chat)) { this.#hydrateAfterNotification(pending.chat, pending.generation); return; }
    const chat = this.#state.activeChat;
    if (chat && matchesAgentRun(run, chat)) { this.#hydrateAfterNotification(chat, this.#generation); return; }
    const inactive = this.#state.chats.find((candidate) => matchesAgentRun(run, candidate));
    if (inactive) {
      this.#set({ refreshNeededChatIds: this.#state.refreshNeededChatIds.includes(inactive.chatId) ? this.#state.refreshNeededChatIds : [...this.#state.refreshNeededChatIds, inactive.chatId], ...chatNotice("m5.chatNotice.otherChat") });
    }
  }
  #hydrateAfterNotification(chat: ChatRecordV1, generation: number): void {
    void this.#hydrateChat(chat, generation).catch(error => {
      if (!this.#live(generation)) return;
      const current = this.#pending?.chat ?? this.#state.activeChat;
      if (current && matchesChatRecord(current, chat)) this.#error("m5.chatError.history", error, chat.chatId);
    });
  }
  async retryHistory(): Promise<void> {
    const chat = this.#state.activeChat, failure = this.#state.error, generation = this.#generation;
    if (!chat || this.#state.switching || failure?.retryChatId !== chat.chatId || !this.#live(generation)) return;
    try {
      await this.#hydrateChat(chat, generation);
      if (this.#live(generation) && this.#state.activeChat && matchesChatRecord(this.#state.activeChat, chat) && this.#state.error === failure) this.#set({ error: null });
    } catch (error) {
      if (this.#live(generation) && this.#state.activeChat && matchesChatRecord(this.#state.activeChat, chat) && this.#state.error === failure) this.#error("m5.chatError.history", error, chat.chatId);
    }
  }
  async #hydrateChat(chat: ChatRecordV1, generation: number): Promise<void> {
    const key = `${generation}:${chat.chatId}`;
    const existing = this.#hydrations.get(key);
    if (existing) {
      this.#hydrationTriggers.set(key, (this.#hydrationTriggers.get(key) ?? 0) + 1);
      return existing;
    }
    this.#hydrationTriggers.set(key, 0);
    const task = this.#hydrateChatPages(chat, generation, key).finally(() => { this.#hydrations.delete(key); this.#hydrationTriggers.delete(key); });
    this.#hydrations.set(key, task);
    return task;
  }
  async #hydrateChatPages(chat: ChatRecordV1, generation: number, key: string): Promise<void> {
    const initialMessages = this.#messagesFor(chat);
    let after = initialMessages.length > 0 ? initialMessages[initialMessages.length - 1]!.eventSequence : 0;
    let consumedTriggers = 0;
    for (;;) {
      const detail = await this.#bridge.getChat({ chatId: chat.chatId, afterEventSequence: after, cursor: null, limit: 50 }) as ChatDetail;
      if (!this.#live(generation) || !matchesChatRecord(detail.chat, chat)) return;
      const messages = mergeMessages(this.#messagesFor(chat), detail.messages);
      this.#applyHydratedDetail(detail, messages);
      if (detail.nextEventSequence === null) {
        const triggers = this.#hydrationTriggers.get(key) ?? 0;
        if (triggers <= consumedTriggers) return;
        consumedTriggers = triggers;
        continue;
      }
      consumedTriggers = this.#hydrationTriggers.get(key) ?? 0;
      after = detail.nextEventSequence;
    }
  }
  #messagesFor(chat: ChatRecordV1): readonly TeamMessageV1[] {
    const pending = this.#pending;
    if (pending && matchesChatRecord(pending.chat, chat)) return pending.messages;
    return this.#state.activeChat && matchesChatRecord(this.#state.activeChat, chat) ? this.#state.messages : [];
  }
  #applyHydratedDetail(detail: ChatDetail, messages: TeamMessageV1[]): void {
    const pending = this.#pending;
    if (pending && matchesChatRecord(pending.chat, detail.chat)) { pending.messages = messages; pending.detail = detail; return; }
    if (!this.#state.activeChat || !matchesChatRecord(this.#state.activeChat, detail.chat)) return;
    const agentRuns = isAgentChatDetailPageV1(detail) ? detail.agentRuns : [];
    this.#set({ activeChat: detail.chat, chats: this.#state.chats.map((item) => item.chatId === detail.chat.chatId ? detail.chat : item), messages, runs: detail.runs, agentRuns, agentRecovery: agentRecoveryFor(detail), activeRunId: detail.runs.find(isActive)?.runId ?? null });
  }
  #agentBridge(): M5BridgeV4 | M5BridgeV5 | null {
    const bridge = this.#bridge as Partial<M5BridgeV4>;
    return typeof bridge.onAgentPublicEvent === "function" && typeof bridge.getAgentCapabilities === "function" && typeof bridge.startAgentRun === "function" && typeof bridge.cancelAgentRun === "function" ? bridge as M5BridgeV4 | M5BridgeV5 : null;
  }
  #agentStateBridge(): M5BridgeV5 | null {
    const bridge = this.#bridge as Partial<M5BridgeV5>;
    return typeof bridge.onAgentRunStateChanged === "function" ? bridge as M5BridgeV5 : null;
  }
  #endSwitch(message: ChatErrorKey, error?: unknown): void { this.#pending = null; this.#set({ switching: false, ...chatNotice(message), error: error ? { message: ko[message], messageKey: message, detail: error instanceof Error ? error.message : String(error) } : this.#state.error }); }
  #live(generation: number): boolean { return !this.#disposed && this.#generation === generation; }
  #error(message: ChatErrorKey, error: unknown, retryChatId?: string): void { this.#set({ error: { message: ko[message], messageKey: message, detail: error instanceof Error ? error.message.slice(0, 2048) : String(error).slice(0, 2048), ...(retryChatId ? { retryChatId } : {}) } }); }
  #set(update: Partial<ChatUiSnapshot>): void { if (this.#disposed) return; this.#state = { ...this.#state, ...update, ...("notice" in update ? { noticeKey: update.notice ? update.noticeKey : undefined, noticeContext: update.notice ? update.noticeContext : undefined } : {}) }; for (const listener of this.#listeners) listener(); }
}

export function isActive(run: RunRecordV1): boolean { return run.state === "preparing" || run.state === "running" || run.state === "cancelling"; }
export function isAgentActive(run: AgentRunRecordV1): boolean { return run.state === "preparing" || run.state === "running" || run.state === "cancelling"; }
/** An identityless recovery has no process evidence that can make abandon safe. */
export function canAbandonAgentRecovery(recovery: AgentRecoveryRecordV1 | null): boolean { return recovery !== null && recovery.reason !== "processIdentityUnavailable"; }
export function stateLabel(state: ChatRecordV1["state"]): string { return ({ draft: "초안", preparing: "준비 중", running: "진행 중", waitingForUser: "사용자 확인 대기", reviewReady: "검토 준비", applying: "적용 중", stopping: "중지 중", completed: "완료", needsAttention: "확인 필요", recoveryRequired: "복구 필요", canceled: "취소됨", discarded: "버림" })[state]; }

function matchesChat(event: TeamMessageV1, chat: ChatRecordV1): boolean { return event.chatId === chat.chatId && event.taskId === chat.taskId && event.worktreeId === chat.worktreeId; }
function matchesChatRecord(left: ChatRecordV1, right: ChatRecordV1): boolean { return left.chatId === right.chatId && left.taskId === right.taskId && left.projectId === right.projectId && left.worktreeId === right.worktreeId && left.workspaceIdentity === right.workspaceIdentity; }
function matchesAgentRun(run: Pick<AgentRunRecordV1, "chatId" | "taskId" | "projectId" | "worktreeId" | "workspaceIdentity"> | AgentPublicEventV1, chat: ChatRecordV1): boolean { return run.chatId === chat.chatId && run.taskId === chat.taskId && run.projectId === chat.projectId && run.worktreeId === chat.worktreeId && run.workspaceIdentity === chat.workspaceIdentity; }
function agentRecoveryFor(detail: ChatDetail): AgentRecoveryRecordV1 | null { return isAgentChatDetailPageV1(detail) ? detail.agentRecovery : null; }
function agentRecoveryIdentity(recovery: AgentRecoveryRecordV1) { return { chatId: recovery.chatId, taskId: recovery.taskId, projectId: recovery.projectId, worktreeId: recovery.worktreeId, workspaceIdentity: recovery.workspaceIdentity, runId: recovery.runId, recoveryId: recovery.recoveryId }; }
function mergeMessages(base: readonly TeamMessageV1[], incoming: readonly TeamMessageV1[]): TeamMessageV1[] {
  // Existing state is already contiguous. Avoid rebuilding and sorting its
  // full index for contiguous event/page appends; overlap still uses the
  // conflict-checking merge below.
  const tail = base[base.length - 1]?.eventSequence ?? 0;
  if (incoming.every((message, index) => message.eventSequence === tail + index + 1)) {
    return [...base, ...incoming];
  }
  const bySequence = new Map<number, TeamMessageV1>();
  for (const message of [...base, ...incoming]) {
    const previous = bySequence.get(message.eventSequence);
    if (previous && (previous.messageId !== message.messageId || previous.chatId !== message.chatId)) throw new Error("event sequence conflict");
    bySequence.set(message.eventSequence, message);
  }
  const ordered = [...bySequence.values()].sort((left, right) => left.eventSequence - right.eventSequence);
  if (ordered.length > 0 && ordered[0]!.eventSequence !== 1) throw new Error("event sequence does not start at 1");
  for (let index = 1; index < ordered.length; index += 1) if (ordered[index]!.eventSequence !== ordered[index - 1]!.eventSequence + 1) throw new Error("event sequence gap");
  return ordered;
}
