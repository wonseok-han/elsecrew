import type { M5BridgeV3 } from "./contracts/bridge-v3.ts";
import type { M5BridgeV4 } from "./contracts/bridge-v4.ts";
import type { M5BridgeV5 } from "./contracts/bridge-v5.ts";
import { isAgentChatDetailPageV1, type AgentRecoveryRecordV1, type AgentRunRecordV1, type AgentPublicEventV1, type AgentRunStateChangedV1 } from "./contracts/agent-adapter-v1.ts";
import type {
  ChatRecordV1, ProjectRoleSettingsV1, RecoveryRecordV1, RoleIdV1, RoleProfileV1,
  RunRecordV1, TeamMessageV1, PreparedChatWorkspaceV1,
} from "./contracts/run-recovery-v1.ts";

export interface ChatUiSnapshot {
  projectId: string | null;
  settings: ProjectRoleSettingsV1 | null;
  chats: readonly ChatRecordV1[];
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
  error: { message: string; detail: string } | null;
}

type M5ChatBridge = M5BridgeV3 | M5BridgeV4 | M5BridgeV5;
type ChatDetail = Awaited<ReturnType<M5BridgeV4["getChat"]>>;

/** Renderer-only projection. The sidecar owns all durable identity, state transitions and CAS. */
export class ChatUiController {
  readonly #bridge: M5ChatBridge;
  readonly #listeners = new Set<() => void>();
  #unsubscribe: (() => void) | null = null;
  #agentUnsubscribe: (() => void) | null = null;
  #agentStateUnsubscribe: (() => void) | null = null;
  #disposed = false;
  #generation = 0;
  #pending: { chat: ChatRecordV1; generation: number; messages: TeamMessageV1[]; detail: ChatDetail | null } | null = null;
  #hydrations = new Map<string, Promise<void>>();
  #hydrationTriggers = new Map<string, number>();
  #state: ChatUiSnapshot = { projectId: null, settings: null, chats: [], activeChat: null, messages: [], runs: [], agentRuns: [], agentStartingRoleId: null, agentRecovery: null, agentRecoveryResolving: false, recoveries: [], refreshNeededChatIds: [], preparing: false, switching: false, preparationApiUnavailable: false, activeRunId: null, notice: null, error: null };

  constructor(bridge: M5ChatBridge) { this.#bridge = bridge; }
  snapshot(): ChatUiSnapshot { return this.#state; }
  subscribe(listener: () => void): () => void { this.#listeners.add(listener); return () => this.#listeners.delete(listener); }
  start(): void {
    if (this.#unsubscribe) return;
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
    this.#set({ projectId, chats: [], activeChat: null, messages: [], runs: [], agentRuns: [], agentStartingRoleId: null, agentRecovery: null, agentRecoveryResolving: false, recoveries: [], refreshNeededChatIds: [], notice: null, error: null });
    try {
      const [settings, page] = await Promise.all([
        this.#bridge.getProjectRoleSettings({ projectId }),
        this.#bridge.listChats({ projectId, cursor: null, limit: 100 }),
      ]);
      if (!this.#live(generation) || this.#state.projectId !== projectId) return;
      this.#set({ settings, chats: page.items });
    } catch (error) { if (this.#live(generation)) this.#error("프로젝트 채팅을 불러오지 못했어요.", error); }
  }

  async updateSettings(roles: readonly RoleProfileV1[]): Promise<void> {
    const settings = this.#state.settings;
    if (!settings) return;
    try {
      const updated = await this.#bridge.updateProjectRoleSettings({ projectId: settings.projectId, expectedRevision: settings.revision, roles });
      if (this.#state.settings?.revision !== settings.revision) return;
      this.#set({ settings: updated, notice: "새 채팅에 사용할 역할과 모델을 저장했어요. 이미 만든 채팅의 설정은 바뀌지 않아요.", error: null });
    } catch (error) {
      this.#error("설정이 다른 곳에서 바뀌었어요. 최신 설정을 다시 불러와 확인해 주세요.", error);
      void this.openProject(settings.projectId);
    }
  }

  async createChat(source: { projectId: string }, goal: string): Promise<ChatRecordV1 | null> {
    const settings = this.#state.settings;
    if (!settings || this.#state.switching || this.isRecoveryBlocked() || settings.projectId !== source.projectId || !goal.trim() || this.#state.preparing) return null;
    const generation = this.#generation;
    let prepared: PreparedChatWorkspaceV1 | null = null;
    this.#set({ preparing: true, preparationApiUnavailable: false, error: null, notice: "새 채팅 전용 작업 복사본을 준비하고 있어요." });
    try {
      prepared = await this.#bridge.prepareChatWorkspace({ projectId: source.projectId });
      const manifest = prepared.manifest;
      if (prepared.bindingState !== "unbound" || manifest.projectId !== source.projectId) throw new Error("prepared workspace identity mismatch");
      const created = await this.#bridge.createChat({ preparationId: prepared.preparationId, projectId: source.projectId, worktreeId: manifest.worktreeId, workspaceIdentity: manifest.workspaceIdentity, goal: goal.trim(), expectedSettingsRevision: settings.revision });
      if (!this.#live(generation) || this.#state.projectId !== source.projectId) return created;
      this.#set({ chats: [created, ...this.#state.chats.filter((item) => item.chatId !== created.chatId)], notice: "새 채팅을 준비했어요. 이 채팅만의 작업 복사본을 사용해요." });
      return created;
    } catch (error) {
      if (prepared?.bindingState === "unbound") {
        const manifest = prepared.manifest;
        try { await this.#bridge.abandonPreparedWorkspace({ preparationId: prepared.preparationId, projectId: manifest.projectId, worktreeId: manifest.worktreeId, workspaceIdentity: manifest.workspaceIdentity }); } catch { /* original create error remains the user-facing evidence */ }
      }
      if (this.#live(generation)) this.#error("새 채팅을 준비하지 못했어요. 원본 프로젝트는 바꾸지 않았어요.", error);
      return null;
    } finally { if (this.#live(generation)) this.#set({ preparing: false }); }
  }

  async prepareSwitch(chatId: string, canLeaveWorkspace: boolean): Promise<ChatRecordV1 | null> {
    if (this.#state.switching) return null;
    if (this.isActionBlocked()) { this.#set({ notice: "이 채팅 작업 복사본은 안전 상태를 확인할 때까지 바꿀 수 없어요." }); return null; }
    if (!canLeaveWorkspace) { this.#set({ notice: "저장하지 않은 편집이 있어 채팅을 바꿀 수 없어요." }); return null; }
    const chat = this.#state.chats.find((item) => item.chatId === chatId);
    if (!chat) return null;
    const generation = this.#generation;
    this.#pending = { chat, generation, messages: [], detail: null };
    this.#set({ switching: true, notice: "채팅 작업 복사본을 안전하게 전환하는 중이에요.", error: null });
    try {
      await this.#hydrateChat(chat, generation);
      if (!this.#live(generation) || this.#pending?.chat.chatId !== chat.chatId) return null;
      return chat;
    } catch (error) { if (this.#live(generation)) this.#endSwitch("채팅 기록을 불러오지 못했어요.", error); return null; }
  }

  async commitPrepared(chatId: string, bound: boolean): Promise<void> {
    const pending = this.#pending;
    if (!pending || pending.chat.chatId !== chatId) return;
    if (!bound || !pending.detail) { this.#endSwitch("채팅 작업 복사본을 열지 못했어요."); return; }
    const detail = pending.detail;
    this.#pending = null;
    this.#set({ activeChat: detail.chat, messages: pending.messages, runs: detail.runs, agentRuns: isAgentChatDetailPageV1(detail) ? detail.agentRuns : [], agentStartingRoleId: null, agentRecovery: agentRecoveryFor(detail), agentRecoveryResolving: false, recoveries: [], refreshNeededChatIds: this.#state.refreshNeededChatIds.filter((item) => item !== detail.chat.chatId), activeRunId: detail.runs.find(isActive)?.runId ?? null, switching: false, notice: null });
    await this.#loadRecoveries(detail.chat.chatId, pending.generation);
  }

  async startDemo(roleId: RoleIdV1): Promise<void> {
    const chat = this.#state.activeChat;
    if (!chat || this.#state.switching || this.#state.activeRunId || this.isActionBlocked()) return;
    const scenario = ({ planning: "planning-success", coding: "coding-success", review: "review-success" } as const)[roleId];
    this.#set({ activeRunId: "starting", error: null, notice: "가짜 에이전트 데모를 시작하는 중이에요. 실제 모델을 설치·확인·실행하지 않습니다." });
    try {
      const run = await this.#bridge.startFixtureRun({ chatId: chat.chatId, roleId, scenario });
      if (this.#state.activeChat?.chatId !== chat.chatId) return;
      this.#set({ runs: [run, ...this.#state.runs.filter((item) => item.runId !== run.runId)], activeRunId: isActive(run) ? run.runId : null });
    } catch (error) { this.#error("선택한 역할·모델을 지금 실행할 수 없어요. 다른 모델로 자동 변경하지 않았어요.", error); }
  }

  async cancelRun(): Promise<void> {
    const chat = this.#state.activeChat; const runId = this.#state.activeRunId; const generation = this.#generation;
    if (!chat || this.#state.switching || !runId || runId === "starting") return;
    try {
      const run = await this.#bridge.cancelRun({ chatId: chat.chatId, runId });
      if (!this.#live(generation) || this.#state.switching || this.#state.activeChat?.chatId !== chat.chatId || this.#state.activeRunId !== runId) return;
      this.#set({ runs: [run, ...this.#state.runs.filter((item) => item.runId !== run.runId)], activeRunId: null, notice: run.state === "recoveryRequired" ? "종료 확인이 필요해요. 복구 상태를 먼저 확인해 주세요." : "데모 실행을 멈췄어요." });
      await this.#loadRecoveries(chat.chatId, generation);
    } catch (error) { this.#error("실행 종료를 확인하지 못했어요. 복구 상태를 확인해 주세요.", error); }
  }

  async startAgent(roleId: RoleIdV1): Promise<void> {
    const chat = this.#state.activeChat; const generation = this.#generation; const bridge = this.#agentBridge();
    if (!chat || !bridge || this.#state.switching || this.#state.activeRunId || this.#state.agentStartingRoleId || this.#state.agentRuns.some(isAgentActive) || this.isActionBlocked()) {
      if (chat && !bridge) this.#set({ notice: "이 앱 연결에서는 실제 에이전트 실행을 아직 사용할 수 없어요. 데모는 계속 사용할 수 있어요." });
      return;
    }
    const profile = chat.roleSnapshots.find((item) => item.roleId === roleId);
    if (!profile) return;
    this.#set({ agentStartingRoleId: roleId, error: null, notice: `${roleLabel(roleId)} 역할의 ${runtimeLabel(profile.runtime)} 준비 상태를 확인하고 있어요.` });
    try {
      const capability = await bridge.getAgentCapabilities({ runtime: profile.runtime, refresh: true });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      if (!capability.canStart) {
        this.#set({ notice: unavailableCapabilityNotice(profile, capability.blockedReason) });
        return;
      }
      await this.#hydrateChat(chat, generation);
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      const inputMessageIds = this.#state.messages.map((message) => message.messageId);
      if (inputMessageIds.length === 0) {
        this.#set({ notice: "실제 에이전트에 전달할 저장된 대화가 아직 없어요. 먼저 목표를 적어 새 채팅을 만들어 주세요." });
        return;
      }
      const accepted = await bridge.startAgentRun({ chatId: chat.chatId, roleId, inputMessageIds, expectedCapabilityFingerprint: capability.fingerprint });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      this.#set({ notice: `${roleLabel(roleId)} 역할 실행을 안전하게 준비했어요. 진행 내용은 저장된 대화 기록으로 확인해요.` });
      await this.#hydrateChat(chat, generation);
      // The accepted binding is independently validated by the bridge. The durable ledger owns terminal state.
      if (accepted.runId.length === 0) throw new Error("empty accepted agent run identity");
    } catch (error) { this.#error("선택한 역할·모델을 지금 실행할 수 없어요. 다른 모델로 자동 변경하지 않았어요.", error); }
    finally { if (this.#live(generation) && this.#state.activeChat?.chatId === chat.chatId) this.#set({ agentStartingRoleId: null }); }
  }

  async cancelAgentRun(run: AgentRunRecordV1): Promise<void> {
    const chat = this.#state.activeChat; const bridge = this.#agentBridge(); const generation = this.#generation;
    if (!chat || !bridge || !isAgentActive(run) || !matchesAgentRun(run, chat) || this.#state.switching) return;
    try {
      const result = await bridge.cancelAgentRun({ chatId: run.chatId, taskId: run.taskId, projectId: run.projectId, worktreeId: run.worktreeId, workspaceIdentity: run.workspaceIdentity, runId: run.runId });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      this.#set({ agentRecovery: result.outcome === "recoveryRequired" ? result.recovery : null, notice: result.outcome === "recoveryRequired" ? "실제 에이전트의 종료를 아직 확인하지 못했어요. 복구가 필요하므로 이 작업 복사본을 다시 사용하지 않았어요." : "실제 에이전트 실행을 멈췄어요." });
      await this.#hydrateChat(chat, generation);
    } catch (error) { this.#error("실제 에이전트의 종료를 확인하지 못했어요. 복구 상태를 확인해 주세요.", error); }
  }

  isRecoveryBlocked(): boolean { return this.#state.agentRecovery !== null || this.#state.activeChat?.state === "recoveryRequired" || this.#state.recoveries.some((item) => item.state === "unresolved"); }
  isActionBlocked(): boolean { return this.isRecoveryBlocked() || this.#state.activeChat?.state === "discarded"; }
  async abandonRecovery(recovery: RecoveryRecordV1): Promise<void> {
    if (!window.confirm("종료 증거를 다시 확인한 뒤 이 작업 복사본을 버릴까요? 원본 프로젝트에는 적용하지 않습니다.")) return;
    try { await this.#bridge.markRecoveryResolved({ recoveryId: recovery.recoveryId, expectedRevision: recovery.revision, resolution: "abandon" }); await this.#loadRecoveries(recovery.chatId, this.#generation); this.#set({ notice: "복구가 필요한 작업 복사본을 버렸어요." }); }
    catch (error) { this.#error("아직 종료를 확인할 수 없어요. 자세한 정보를 확인해 주세요.", error); }
  }

  async abandonAgentRecovery(): Promise<void> {
    const chat = this.#state.activeChat; const recovery = this.#state.agentRecovery; const bridge = this.#agentBridge(); const generation = this.#generation;
    if (!chat || !recovery || !bridge || this.#state.switching || this.#state.agentRecoveryResolving || !matchesAgentRun(recovery, chat)) return;
    if (!globalThis.confirm("종료 증거를 다시 확인한 뒤 이 실제 에이전트 작업 복사본을 버릴까요? 원본 프로젝트에는 적용하지 않습니다.")) return;
    this.#set({ agentRecoveryResolving: true, error: null });
    try {
      const inspected = await bridge.inspectAgentRecovery(agentRecoveryIdentity(recovery));
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId || this.#state.agentRecovery?.recoveryId !== recovery.recoveryId) return;
      if (!matchesAgentRun(inspected, chat) || inspected.recoveryId !== recovery.recoveryId || inspected.state !== "unresolved" || !inspected.poisoned) throw new Error("agent recovery inspect identity mismatch");
      const result = await bridge.resolveAgentRecovery({ ...agentRecoveryIdentity(inspected), expectedRevision: inspected.revision, resolution: "abandon" });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId || this.#state.agentRecovery?.recoveryId !== recovery.recoveryId) return;
      const notice = result.outcome === "resolvedAbandoned"
        ? "작업 복사본을 버리도록 기록했어요. 저장된 상태를 다시 확인하고 있어요."
        : result.outcome === "processesRemain"
          ? "아직 종료를 확인할 수 없어 이 작업 복사본을 계속 막고 있어요."
          : "복구 기록이 다른 곳에서 바뀌었어요. 최신 상태를 다시 확인하고 있어요.";
      this.#set({ notice });
      await this.#hydrateChat(chat, generation);
    } catch (error) { this.#error("실제 에이전트의 종료 상태를 확인하지 못했어요. 이 작업 복사본은 계속 보호하고 있어요.", error); }
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
    try { this.#set({ messages: mergeMessages(this.#state.messages, [event]) }); } catch (error) { this.#error("대화 기록 순서를 안전하게 확인하지 못했어요. 다시 열어 주세요.", error); }
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
    if (pending && matchesAgentRun(run, pending.chat)) { void this.#hydrateChat(pending.chat, pending.generation); return; }
    const chat = this.#state.activeChat;
    if (chat && matchesAgentRun(run, chat)) { void this.#hydrateChat(chat, this.#generation); return; }
    const inactive = this.#state.chats.find((candidate) => matchesAgentRun(run, candidate));
    if (inactive) {
      this.#set({ refreshNeededChatIds: this.#state.refreshNeededChatIds.includes(inactive.chatId) ? this.#state.refreshNeededChatIds : [...this.#state.refreshNeededChatIds, inactive.chatId], notice: "다른 채팅에 새 에이전트 상태가 있어요. 그 채팅을 열면 저장된 기록을 안전하게 다시 확인해요." });
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
  #endSwitch(message: string, error?: unknown): void { this.#pending = null; this.#set({ switching: false, notice: message, error: error ? { message, detail: error instanceof Error ? error.message : String(error) } : this.#state.error }); }
  #live(generation: number): boolean { return !this.#disposed && this.#generation === generation; }
  #error(message: string, error: unknown): void { this.#set({ error: { message, detail: error instanceof Error ? error.message.slice(0, 2048) : String(error).slice(0, 2048) } }); }
  #set(update: Partial<ChatUiSnapshot>): void { if (this.#disposed) return; this.#state = { ...this.#state, ...update }; for (const listener of this.#listeners) listener(); }
}

export function isActive(run: RunRecordV1): boolean { return run.state === "preparing" || run.state === "running" || run.state === "cancelling"; }
export function isAgentActive(run: AgentRunRecordV1): boolean { return run.state === "preparing" || run.state === "running" || run.state === "cancelling"; }
export function stateLabel(state: ChatRecordV1["state"]): string { return ({ draft: "초안", preparing: "준비 중", running: "진행 중", waitingForUser: "사용자 확인 대기", reviewReady: "검토 준비", applying: "적용 중", stopping: "중지 중", completed: "완료", needsAttention: "확인 필요", recoveryRequired: "복구 필요", canceled: "취소됨", discarded: "버림" })[state]; }

function matchesChat(event: TeamMessageV1, chat: ChatRecordV1): boolean { return event.chatId === chat.chatId && event.taskId === chat.taskId && event.worktreeId === chat.worktreeId; }
function matchesChatRecord(left: ChatRecordV1, right: ChatRecordV1): boolean { return left.chatId === right.chatId && left.taskId === right.taskId && left.projectId === right.projectId && left.worktreeId === right.worktreeId && left.workspaceIdentity === right.workspaceIdentity; }
function matchesAgentRun(run: Pick<AgentRunRecordV1, "chatId" | "taskId" | "projectId" | "worktreeId" | "workspaceIdentity"> | AgentPublicEventV1, chat: ChatRecordV1): boolean { return run.chatId === chat.chatId && run.taskId === chat.taskId && run.projectId === chat.projectId && run.worktreeId === chat.worktreeId && run.workspaceIdentity === chat.workspaceIdentity; }
function agentRecoveryFor(detail: ChatDetail): AgentRecoveryRecordV1 | null { return isAgentChatDetailPageV1(detail) ? detail.agentRecovery : null; }
function agentRecoveryIdentity(recovery: AgentRecoveryRecordV1) { return { chatId: recovery.chatId, taskId: recovery.taskId, projectId: recovery.projectId, worktreeId: recovery.worktreeId, workspaceIdentity: recovery.workspaceIdentity, runId: recovery.runId, recoveryId: recovery.recoveryId }; }
function roleLabel(roleId: RoleIdV1): string { return ({ planning: "기획", coding: "코딩", review: "검토" })[roleId]; }
function runtimeLabel(runtime: RoleProfileV1["runtime"]): string { return runtime === "codex" ? "Codex" : "Claude Code"; }
function unavailableCapabilityNotice(profile: RoleProfileV1, reason: string | null): string {
  const cause = ({ platformUnsupported: "이 운영체제에서는", notInstalled: "설치되지 않아", installationIncomplete: "설치가 완전하지 않아", unsupportedVersion: "지원하지 않는 버전이라", notAuthenticated: "로그인하지 않아", authExpired: "로그인이 만료되어", capabilityUnknown: "사용 가능 여부를 안전하게 확인할 수 없어", exactModelUnavailable: "선택한 정확한 모델을 사용할 수 없어" } as Record<string, string>)[reason ?? ""] ?? "준비 상태를 확인할 수 없어";
  return `${roleLabel(profile.roleId)} 역할의 ${runtimeLabel(profile.runtime)}은 ${cause} 시작하지 않았어요. 다른 모델로 자동 변경하지 않았어요.`;
}
function mergeMessages(base: readonly TeamMessageV1[], incoming: readonly TeamMessageV1[]): TeamMessageV1[] {
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
