import type { M5BridgeV3 } from "./contracts/bridge-v3.ts";
import type {
  ChatRecordV1, ProjectRoleSettingsV1, RecoveryRecordV1, RoleIdV1, RoleProfileV1,
  RunRecordV1, TeamMessageV1,
} from "./contracts/run-recovery-v1.ts";
import type { WorktreeManifestV1 } from "./contracts/project-v1.ts";

export interface ChatUiSnapshot {
  projectId: string | null;
  settings: ProjectRoleSettingsV1 | null;
  chats: readonly ChatRecordV1[];
  activeChat: ChatRecordV1 | null;
  messages: readonly TeamMessageV1[];
  runs: readonly RunRecordV1[];
  recoveries: readonly RecoveryRecordV1[];
  preparing: boolean;
  activeRunId: string | null;
  notice: string | null;
  error: { message: string; detail: string } | null;
}

/** Renderer-only projection. The sidecar owns all durable identity, state transitions and CAS. */
export class ChatUiController {
  readonly #bridge: M5BridgeV3;
  readonly #listeners = new Set<() => void>();
  #unsubscribe: (() => void) | null = null;
  #disposed = false;
  #generation = 0;
  #state: ChatUiSnapshot = { projectId: null, settings: null, chats: [], activeChat: null, messages: [], runs: [], recoveries: [], preparing: false, activeRunId: null, notice: null, error: null };

  constructor(bridge: M5BridgeV3) { this.#bridge = bridge; }
  snapshot(): ChatUiSnapshot { return this.#state; }
  subscribe(listener: () => void): () => void { this.#listeners.add(listener); return () => this.#listeners.delete(listener); }
  start(): void { if (this.#unsubscribe) return; this.#unsubscribe = this.#bridge.onChatEvent((event) => this.#onEvent(event)); }
  dispose(): void { this.#disposed = true; this.#generation++; this.#unsubscribe?.(); this.#unsubscribe = null; this.#listeners.clear(); }

  async openProject(projectId: string): Promise<void> {
    const generation = ++this.#generation;
    this.#set({ projectId, chats: [], activeChat: null, messages: [], runs: [], recoveries: [], notice: null, error: null });
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
    if (!settings || settings.projectId !== source.projectId || !goal.trim() || this.#state.preparing) return null;
    const generation = this.#generation;
    this.#set({ preparing: true, error: null, notice: "새 채팅 전용 작업 복사본을 준비하고 있어요." });
    let snapshot: WorktreeManifestV1 | null = null;
    try {
      // A snapshot is made for every chat. Its exact triple is then captured by createChat.
      snapshot = await this.#bridge.createProjectWorktree({ projectId: source.projectId });
      if (!this.#live(generation) || this.#state.projectId !== source.projectId) {
        await this.#bridge.closeProject({ projectId: source.projectId });
        return null;
      }
      const chat = await this.#bridge.createChat({
        projectId: source.projectId, worktreeId: snapshot.worktreeId, workspaceIdentity: snapshot.workspaceIdentity,
        goal: goal.trim(), expectedSettingsRevision: settings.revision,
      });
      if (!this.#live(generation)) return null;
      await this.#activate(chat, generation);
      this.#set({ chats: [chat, ...this.#state.chats.filter((item) => item.chatId !== chat.chatId)], notice: "새 채팅을 시작했어요. 이 채팅만의 작업 복사본을 사용해요." });
      return chat;
    } catch (error) {
      if (this.#live(generation)) this.#error("새 채팅을 준비하지 못했어요. 원본 프로젝트는 바꾸지 않았어요.", error);
      return null;
    } finally { if (this.#live(generation)) this.#set({ preparing: false }); }
  }

  async switchChat(chatId: string, canLeaveWorkspace: boolean): Promise<ChatRecordV1 | null> {
    if (!canLeaveWorkspace) { this.#set({ notice: "저장하지 않은 편집이 있어 채팅을 바꿀 수 없어요." }); return null; }
    const chat = this.#state.chats.find((item) => item.chatId === chatId);
    if (!chat) return null;
    await this.#activate(chat, this.#generation);
    return this.#state.activeChat?.chatId === chatId ? chat : null;
  }

  async startDemo(roleId: RoleIdV1): Promise<void> {
    const chat = this.#state.activeChat;
    if (!chat || this.#state.activeRunId || this.isRecoveryBlocked()) return;
    const scenario = ({ planning: "planning-success", coding: "coding-success", review: "review-success" } as const)[roleId];
    this.#set({ activeRunId: "starting", error: null, notice: "가짜 에이전트 데모를 시작하는 중이에요. 실제 모델을 설치·확인·실행하지 않습니다." });
    try {
      const run = await this.#bridge.startFixtureRun({ chatId: chat.chatId, roleId, scenario });
      if (this.#state.activeChat?.chatId !== chat.chatId) return;
      this.#set({ runs: [run, ...this.#state.runs.filter((item) => item.runId !== run.runId)], activeRunId: isActive(run) ? run.runId : null });
    } catch (error) { this.#error("선택한 역할·모델을 지금 실행할 수 없어요. 다른 모델로 자동 변경하지 않았어요.", error); }
  }

  async cancelRun(): Promise<void> {
    const chat = this.#state.activeChat; const runId = this.#state.activeRunId;
    if (!chat || !runId || runId === "starting") return;
    try {
      const run = await this.#bridge.cancelRun({ chatId: chat.chatId, runId });
      this.#set({ runs: [run, ...this.#state.runs.filter((item) => item.runId !== run.runId)], activeRunId: null, notice: run.state === "recoveryRequired" ? "종료 확인이 필요해요. 복구 상태를 먼저 확인해 주세요." : "데모 실행을 멈췄어요." });
      await this.#loadRecoveries(chat.chatId, this.#generation);
    } catch (error) { this.#error("실행 종료를 확인하지 못했어요. 복구 상태를 확인해 주세요.", error); }
  }

  isRecoveryBlocked(): boolean { return this.#state.activeChat?.state === "recoveryRequired" || this.#state.recoveries.some((item) => item.state === "unresolved"); }
  async abandonRecovery(recovery: RecoveryRecordV1): Promise<void> {
    if (!window.confirm("종료 증거를 다시 확인한 뒤 이 작업 복사본을 버릴까요? 원본 프로젝트에는 적용하지 않습니다.")) return;
    try { await this.#bridge.markRecoveryResolved({ recoveryId: recovery.recoveryId, expectedRevision: recovery.revision, resolution: "abandon" }); await this.#loadRecoveries(recovery.chatId, this.#generation); this.#set({ notice: "복구가 필요한 작업 복사본을 버렸어요." }); }
    catch (error) { this.#error("아직 종료를 확인할 수 없어요. 자세한 정보를 확인해 주세요.", error); }
  }

  async #activate(chat: ChatRecordV1, generation: number): Promise<void> {
    this.#set({ activeChat: chat, messages: [], runs: [], recoveries: [], activeRunId: null, error: null });
    try {
      const detail = await this.#bridge.getChat({ chatId: chat.chatId, afterEventSequence: 0, cursor: null, limit: 50 });
      if (!this.#live(generation) || this.#state.activeChat?.chatId !== chat.chatId) return;
      this.#set({ activeChat: detail.chat, messages: detail.messages, runs: detail.runs, activeRunId: detail.runs.find(isActive)?.runId ?? null });
      await this.#loadRecoveries(chat.chatId, generation);
    } catch (error) { if (this.#live(generation)) this.#error("채팅 기록을 불러오지 못했어요.", error); }
  }
  async #loadRecoveries(chatId: string, generation: number): Promise<void> { const page = await this.#bridge.listRecoveries({ cursor: null, limit: 100 }); if (this.#live(generation) && this.#state.activeChat?.chatId === chatId) this.#set({ recoveries: page.items.filter((item) => item.chatId === chatId) }); }
  #onEvent(event: TeamMessageV1): void { const chat = this.#state.activeChat; if (!chat || event.chatId !== chat.chatId || event.taskId !== chat.taskId || event.worktreeId !== chat.worktreeId) return; if (this.#state.messages.some((item) => item.messageId === event.messageId)) return; this.#set({ messages: [...this.#state.messages, event].sort((a, b) => a.eventSequence - b.eventSequence) }); }
  #live(generation: number): boolean { return !this.#disposed && this.#generation === generation; }
  #error(message: string, error: unknown): void { this.#set({ error: { message, detail: error instanceof Error ? error.message.slice(0, 2048) : String(error).slice(0, 2048) } }); }
  #set(update: Partial<ChatUiSnapshot>): void { if (this.#disposed) return; this.#state = { ...this.#state, ...update }; for (const listener of this.#listeners) listener(); }
}

export function isActive(run: RunRecordV1): boolean { return run.state === "preparing" || run.state === "running" || run.state === "cancelling"; }
export function stateLabel(state: ChatRecordV1["state"]): string { return ({ draft: "초안", preparing: "준비 중", running: "진행 중", waitingForUser: "사용자 확인 대기", reviewReady: "검토 준비", applying: "적용 중", stopping: "중지 중", completed: "완료", needsAttention: "확인 필요", recoveryRequired: "복구 필요", canceled: "취소됨", discarded: "버림" })[state]; }
