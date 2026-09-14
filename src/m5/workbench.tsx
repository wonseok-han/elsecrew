import { useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api";

import {
  ProjectEditorController,
  type EditorTab,
  type WorkbenchPanel,
} from "./editor-state.ts";
import type { DiffEntryV1 } from "./contracts/diff-v1.ts";
import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";
import type { RoleProfileV1 } from "./contracts/run-recovery-v1.ts";
import { StorageAdmissionPoller, type AdmissionView } from "./admission-state.ts";
import { resolveM5Bridge, type SupportedM5Bridge } from "./bridge-boundary.ts";
import { canAbandonAgentRecovery, ChatUiController, stateLabel } from "./chat-state.ts";
import { WorkbenchDock } from "./workbench-dock.tsx";
import { ProviderLogin } from "./provider-login.tsx";
import { diffRows, DIFF_PAGE_ROWS } from "./diff-view.ts";

type MonacoApi = typeof Monaco;

let monacoLoader: Promise<MonacoApi> | undefined;

function loadMonaco(): Promise<MonacoApi> {
  monacoLoader ??= Promise.all([
    import("monaco-editor/esm/vs/editor/editor.api"),
    import("monaco-editor/esm/vs/editor/editor.worker?worker"),
    import("monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution"),
    import("monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution"),
  ]).then(([api, editorWorker]) => {
    // Vite emits the worker alongside the local renderer assets; no CDN or eval loader is used.
    (globalThis as typeof globalThis & { MonacoEnvironment?: Monaco.Environment }).MonacoEnvironment = {
      getWorker: () => new editorWorker.default(),
    };
    return api;
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
  if (admission === "ready") return null;
  return <section className={admission === "failed" ? "m5-admission m5-admission-failed" : "m5-admission"} role="status">
    <strong>{admission === "failed" ? "작업 공간을 아직 열 수 없어요." : "작업 공간을 안전하게 준비하고 있어요."}</strong>
    <p>{admission === "failed" ? "앱을 다시 열어 보세요. 계속되면 도움을 요청해 주세요. 기존 프로젝트 파일은 바꾸지 않았어요." : "프로젝트와 채팅은 곧 사용할 수 있어요. 이 동안에는 변경을 만들거나 저장하지 않아요."}</p>
  </section>;
}

export function M5Workbench({ bridge }: { bridge?: unknown }) {
  const candidate = bridge ?? (typeof window === "undefined" ? undefined : window.m5);
  const resolved = resolveM5Bridge(candidate);
  if (!resolved) return <WorkbenchUnavailable />;
  return <M5WorkbenchReady bridge={resolved} />;
}

function WorkbenchUnavailable() {
  return <main className="m5-workbench" aria-label="M5 코딩 작업대">
    <section className="m5-admission m5-admission-failed" role="alert">
      <strong>작업 공간을 아직 열 수 없어요.</strong>
      <p>앱을 다시 열어 보세요. 계속되면 도움을 요청해 주세요. 기존 프로젝트 파일은 바꾸지 않았어요.</p>
    </section>
  </main>;
}

function M5WorkbenchReady({ bridge }: { bridge: SupportedM5Bridge }) {
  const controller = useMemo(() => new ProjectEditorController(bridge), [bridge]);
  const chatController = useMemo(() => new ChatUiController(bridge), [bridge]);
  const admission = useStorageAdmission(bridge);
  const admissionReady = admission === "ready";
  const [goal, setGoal] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
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
  useEffect(() => { controller.setActionBlocked(!admissionReady || chat.switching || chatController.isActionBlocked(), !admissionReady ? "작업 공간을 준비하는 동안 변경을 만들거나 저장하지 않아요." : chat.switching ? "채팅을 전환하는 동안 파일 작업을 잠시 멈췄어요." : "이 작업 복사본에서는 위험한 파일 작업을 할 수 없어요."); }, [admissionReady, controller, chat.switching, chat.agentRecovery, chat.agentRecoveryResolving, chat.recoveries, chat.activeChat?.state]);
  const switchChat = async (chatId: string) => {
    controller.setActionBlocked(true, "채팅을 전환하는 동안 파일 작업을 잠시 멈췄어요.");
    const candidate = await chatController.prepareSwitch(chatId, controller.canLeaveProject());
    if (!candidate) { controller.setActionBlocked(chatController.isActionBlocked()); return; }
    const bound = await controller.bindChat(candidate);
    await chatController.commitPrepared(candidate.chatId, bound);
    controller.setActionBlocked(chatController.isActionBlocked());
  };
  const createChat = async () => {
    if (!snapshot.project) return;
    controller.setActionBlocked(true, "새 채팅 작업 복사본을 준비하는 동안 파일 작업을 잠시 멈췄어요.");
    const created = await chatController.createChat({ projectId: snapshot.project.id }, goal);
    if (created) { setGoal(""); await switchChat(created.chatId); }
    else controller.setActionBlocked(chatController.isActionBlocked());
  };

  const activeTab = snapshot.activePath
    ? snapshot.tabs.find((tab) => tab.path === snapshot.activePath) ?? null
    : null;

  return (
    <main className="m5-workbench" aria-label="M5 코딩 작업대">
      <header className="m5-toolbar">
        <div>
          <p className="m5-eyebrow">M5 · 원본과 분리된 작업 복사본</p>
          <h1>{snapshot.project?.name ?? "프로젝트를 선택해 시작하세요"}</h1>
          <p className="m5-subtitle">
            {snapshot.project ? "원본 프로젝트와 분리된 복사본에서 파일과 변경 내용을 안전하게 확인하고 있어요." : "Git 프로젝트를 고르면 원본과 분리된 작업 복사본의 파일과 변경 내용을 안전하게 확인할 수 있어요."}
          </p>
        </div>
        <div className="m5-toolbar-actions">
          <button type="button" onClick={() => void controller.chooseProject()} disabled={!admissionReady || !controller.canLeaveProject() || snapshot.selectionBlocked}>
            {snapshot.project ? "프로젝트 바꾸기" : "프로젝트 선택"}
          </button>
          {snapshot.project && (
            <button className="m5-secondary" type="button" onClick={() => void controller.abandonProject()} disabled={!admissionReady || !controller.canLeaveProject()}>
              작업 복사본 닫기
            </button>
          )}
          {snapshot.project && <button className="m5-secondary" type="button" onClick={() => setSettingsOpen((open) => !open)} disabled={!admissionReady}>역할과 모델</button>}
        </div>
      </header>

      {snapshot.notice && <p className="m5-notice" role="status">{snapshot.notice}</p>}
      <StorageAdmissionNotice admission={admission} />
      <ProviderLogin disabled={!admissionReady} />
      {snapshot.error && (
        <section className="m5-error" role="alert">
          <strong>{snapshot.error.message}</strong>
          <details><summary>자세히 보기</summary><pre>{snapshot.error.technicalDetail}</pre></details>
        </section>
      )}
      {chat.error && <section className="m5-error" role="alert"><strong>{chat.error.message}</strong><details><summary>자세히 보기</summary><pre>{chat.error.detail}</pre></details></section>}
      {snapshot.rescanRequired && (
        <section className="m5-rescan" role="status">
          <span>파일 변경이 많아 목록이 최신이 아닐 수 있어요.</span>
          <button type="button" onClick={() => void Promise.all([controller.loadTree(), controller.refreshChanges()])}>다시 확인</button>
        </section>
      )}

      {snapshot.project && <section className="m5-chat-strip" aria-label="프로젝트 채팅">
        <div className="m5-chat-list"><strong>채팅</strong>{chat.chats.map((item) => <button key={item.chatId} type="button" disabled={!admissionReady || chat.switching} className={chat.activeChat?.chatId === item.chatId ? "m5-chat-chip selected" : "m5-chat-chip"} onClick={() => void switchChat(item.chatId)}>{item.goal}{chat.refreshNeededChatIds.includes(item.chatId) && <span className="m5-chat-update">새 상태</span>}</button>)}</div>
        {chat.nextChatCursor !== null && <button className="m5-secondary" type="button" disabled={!admissionReady || chat.loadingMoreChats} onClick={() => void chatController.loadMoreChats()}>{chat.loadingMoreChats ? "채팅 불러오는 중…" : "채팅 더 보기"}</button>}
        <form className="m5-new-chat-form" onSubmit={(event) => { event.preventDefault(); void createChat(); }}>
          <input aria-label="새 채팅 목표" value={goal} onChange={(event) => setGoal(event.target.value)} placeholder="이 프로젝트에서 이루고 싶은 일을 적어주세요" disabled={!admissionReady || chat.preparing} />
          <button type="submit" disabled={!admissionReady || !goal.trim() || chat.preparing || chat.preparationApiUnavailable || chat.switching || Boolean(snapshot.workspace && !controller.canLeaveProject())}>새 채팅</button>
        </form>
      </section>}

      {settingsOpen && chat.settings && <RoleSettings settings={chat.settings} disabled={!admissionReady} onSave={(roles) => void chatController.updateSettings(roles)} onClose={() => setSettingsOpen(false)} />}

      <WorkbenchDock conversation={chat.activeChat ? <div className="m5-conversation"><TeamTimeline controller={chatController} chat={chat} admissionReady={admissionReady} /><FollowupComposer controller={chatController} chat={chat} admissionReady={admissionReady} /></div> : undefined}>
        <aside className="m5-panel m5-files" aria-label="파일">
          <PanelTitle active={snapshot.panel === "files"} panel="files" onSelect={controller.selectPanel.bind(controller)}>
            파일
          </PanelTitle>
          {snapshot.project ? (
            <>
              <button className="m5-root" type="button" title={snapshot.project.root} onClick={() => void controller.loadTree("")}>{snapshot.treeParent || snapshot.project.root}</button>
              <div className="m5-file-list">
                {snapshot.tree.map((entry) => (
                  <button key={entry.path} type="button" className="m5-file-row" data-kind={entry.kind} onClick={() => entry.kind === "file" ? void controller.openFile(entry.path) : entry.kind === "directory" ? void controller.loadTree(entry.path) : undefined} disabled={!admissionReady || entry.kind === "symlink"}>
                    {entry.name}
                  </button>
                ))}
              </div>
              <nav className="m5-tree-pages" aria-label="파일 목록 페이지">
                <button type="button" disabled={!admissionReady || snapshot.treeLoading || snapshot.treePage === 1} onClick={() => void controller.loadTree(snapshot.treeParent)}>처음으로</button>
                <span>{snapshot.treeLoading ? "불러오는 중…" : `${snapshot.treePage}페이지`}</span>
                <button type="button" disabled={!admissionReady || snapshot.treeLoading || !snapshot.treeNextCursor} onClick={() => void controller.loadNextTreePage()}>다음 목록</button>
              </nav>
            </>
          ) : <Empty text="프로젝트를 고르면 파일을 표시해요." />}
        </aside>

        <section className="m5-panel m5-editor" aria-label="코드 편집기">
          <div className="m5-tab-list" role="tablist" aria-label="열린 파일">
            {snapshot.tabs.map((tab) => (
              <div className="m5-editor-tab" key={tab.path}>
                <button type="button" role="tab" aria-selected={snapshot.activePath === tab.path} onClick={() => controller.openFile(tab.path)} disabled={!admissionReady}>
                  {fileName(tab.path)} {tab.saveState !== "clean" && <span aria-label="저장되지 않은 변경">●</span>}
                </button>
                <button type="button" className="m5-close-tab" aria-label={`${tab.path} 탭 닫기`} onClick={() => controller.closeTab(tab.path)} disabled={!admissionReady}>×</button>
              </div>
            ))}
          </div>
          <div className="m5-editor-head">
            <span>{activeTab?.path ?? "파일을 선택하세요"}</span>
            {activeTab?.mode === "text" && (activeTab.saveState === "dirty" || activeTab.saveState === "conflicted") && <button type="button" className="m5-secondary" disabled={!admissionReady || snapshot.actionBlocked || reloadingDraft} onClick={() => setReloadDraft(activeTab)}>저장된 내용 다시 불러오기</button>}
            {activeTab?.mode === "text" && (
              <button type="button" onClick={() => void controller.saveActive()} disabled={!admissionReady || activeTab.saveState !== "dirty"}>
                {activeTab.saveState === "saving" ? "저장 중…" : activeTab.saveState === "conflicted" ? "충돌 해결 필요" : "저장"}
              </button>
            )}
          </div>
          {reloadDraft && <section role="alertdialog" aria-label="편집 내용 버리기 확인" className="m5-error">
            <p><strong>{reloadDraft.path}</strong>의 저장하지 않은 편집 내용을 버리고 저장된 파일로 돌아갈까요? 필요한 내용은 먼저 복사해 주세요.</p>
            <button type="button" disabled={reloadingDraft} onClick={() => setReloadDraft(null)}>취소</button>
            <button type="button" disabled={!admissionReady || snapshot.actionBlocked || reloadingDraft || !snapshot.tabs.includes(reloadDraft)} onClick={async () => {
              setReloadingDraft(true);
              try { await controller.reloadConfirmedDraft(reloadDraft); } finally { setReloadingDraft(false); setReloadDraft(null); }
            }}>{reloadingDraft ? "다시 불러오는 중…" : "편집 내용 버리고 다시 불러오기"}</button>
            {!snapshot.tabs.includes(reloadDraft) && <p>확인 중 파일 상태가 바뀌었어요. 취소한 뒤 다시 확인해 주세요.</p>}
          </section>}
          <EditorContent tab={activeTab} readOnly={!admissionReady} onChange={(text) => activeTab && controller.updateText(activeTab.path, text)} />
        </section>

        <aside className="m5-panel m5-changes" aria-label="변경">
          <PanelTitle active={snapshot.panel === "changes"} panel="changes" onSelect={controller.selectPanel.bind(controller)}>
            변경 {snapshot.diffSummary ? `(${snapshot.diffSummary.entries.length})` : ""}
          </PanelTitle>
          {snapshot.project ? (
            <>
              <button className="m5-refresh" type="button" onClick={() => void controller.refreshChanges()} disabled={!admissionReady}>변경 새로 고침</button>
              <div className="m5-change-list">
                {snapshot.diffSummary?.entries.map((entry) => (
                  <button key={`${entry.kind}:${entry.path}`} type="button" className="m5-change-row" onClick={() => void controller.openDiff(entry.path)} disabled={!admissionReady}>
                    <ChangeKind entry={entry} />
                    <span>{entry.path}</span>
                  </button>
                ))}
                {snapshot.diffSummary?.entries.length === 0 && <Empty text="아직 확인할 변경이 없어요." />}
              </div>
              {snapshot.diffFile && <DiffDetails descriptor={snapshot.diffFile} text={snapshot.diffText} />}
            </>
          ) : <Empty text="변경 내용은 프로젝트를 선택한 뒤 보여요." />}
        </aside>
      </WorkbenchDock>
    </main>
  );
}

function RoleSettings({ settings, disabled, onSave, onClose }: { settings: NonNullable<ReturnType<ChatUiController["snapshot"]>["settings"]>; disabled: boolean; onSave: (roles: readonly RoleProfileV1[]) => void; onClose: () => void }) {
  const [roles, setRoles] = useState<readonly RoleProfileV1[]>(settings.roles);
  useEffect(() => setRoles(settings.roles), [settings]);
  const change = (index: number, field: keyof RoleProfileV1, value: string | null) => setRoles((old) => old.map((role, current) => current === index ? { ...role, [field]: value } : role));
  return <section className="m5-settings" aria-label="프로젝트 역할과 모델 설정"><header><div><p className="m5-eyebrow">이 프로젝트 전용 설정 · 버전 {settings.revision}</p><h2>역할과 모델</h2></div><button className="m5-secondary" type="button" onClick={onClose}>작업대로 돌아가기</button></header><p>모델을 실제로 사용할 수 있는지는 실행 전에 확인해요. 데모는 모델 설치·로그인·실행을 확인하지 않습니다. 이미 만든 채팅은 당시 설정을 그대로 보존해요.</p><div className="m5-role-cards">{roles.map((role, index) => <article key={role.roleId} className="m5-role-card"><strong>{({ planning: "기획", coding: "코딩", review: "검토" })[role.roleId]}</strong><label>실행 환경<select disabled={disabled} value={role.runtime} onChange={(event) => change(index, "runtime", event.target.value)}><option value="codex">Codex</option><option value="claude-code">Claude Code</option></select></label><label>정확한 모델 이름<input disabled={disabled} value={role.modelSelector} onChange={(event) => change(index, "modelSelector", event.target.value)} /></label><label>추론 수준<select disabled={disabled} value={role.reasoningEffort ?? ""} onChange={(event) => change(index, "reasoningEffort", event.target.value || null)}><option value="">기본값</option><option value="low">low</option><option value="medium">medium</option><option value="high">high</option></select></label><span>권한: {role.permission === "readOnly" ? "읽기 전용" : "격리된 작업 복사본에만 쓰기"}</span></article>)}</div><button type="button" disabled={disabled} onClick={() => onSave(roles)}>새 채팅 설정 저장</button></section>;
}

function FollowupComposer({ controller, chat, admissionReady }: { controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean }) {
  const draft = chat.followups?.[chat.activeChat!.chatId];
  const allowed = admissionReady && controller.canAppendFollowup();
  return <form className="m5-followup" aria-label="후속 메시지" onSubmit={event => { event.preventDefault(); void controller.appendFollowup(admissionReady); }}>
    <label htmlFor="m5-followup-text">추가로 알려줄 내용</label>
    <textarea id="m5-followup-text" value={draft?.text ?? ""} onChange={event => controller.editFollowup(event.target.value)} placeholder="계획에 보완할 점이나 다음에 할 일을 적어 주세요." rows={3} />
    <p>대화에 저장해요. 진행 중인 에이전트에 바로 전달되지는 않으며, 다음 실제 실행에서 함께 사용해요.</p>
    {draft?.error && <div role="alert"><p>저장 여부를 확인하지 못했어요. 입력은 보존했어요. 다시 시도하면 같은 메시지로 확인해요.</p><details><summary>기술 정보</summary><pre>{draft.error}</pre></details></div>}
    {draft?.request && !draft.sending && <details><summary>다시 확인할 메시지</summary><p>{draft.request.text}</p></details>}
    <button type="submit" disabled={!allowed || draft?.sending || (!draft?.request && !draft?.text.trim())}>{draft?.sending ? "저장 확인 중…" : draft?.request ? "같은 메시지 다시 확인" : "대화에 추가"}</button>
    {!allowed && <p>현재 앱 또는 작업 공간 상태에서는 메시지를 추가할 수 없어요.</p>}
  </form>;
}

function TeamTimeline({ controller, chat, admissionReady }: { controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean }) {
  const fixtureActive = Boolean(chat.activeRunId);
  const actualActive = chat.agentRuns.find((run) => run.state === "preparing" || run.state === "running" || run.state === "cancelling") ?? null;
  const startingActual = chat.agentStartingRoleId !== null;
  const blocked = controller.isActionBlocked();
  const identityUnavailable = chat.agentRecovery !== null && !canAbandonAgentRecovery(chat.agentRecovery);
  return <section className="m5-timeline" aria-label="팀 대화"><header><div><p className="m5-eyebrow">팀 대화 · {stateLabel(chat.activeChat!.state)}</p><h2>{chat.activeChat!.goal}</h2></div><div className="m5-run-actions">{fixtureActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelRun()}>데모 실행 취소</button>}{actualActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelAgentRun(actualActive)}>실제 실행 취소</button>}</div></header>{blocked && <section className="m5-error"><strong>복구가 필요한 작업 복사본이에요.</strong><p>{identityUnavailable ? "종료 상태를 안전하게 확인할 수 없어 이 작업 복사본을 보호하고 있어요. 지금은 실행·저장·닫기 같은 동작을 할 수 없어요." : "실행·저장·닫기 같은 위험한 동작을 막았어요. 종료 증거를 확인한 뒤 이 복사본을 버릴 수 있습니다."}</p>{chat.recoveries.filter((item) => item.state === "unresolved").map((item) => <button key={item.recoveryId} type="button" disabled={!admissionReady} onClick={() => void controller.abandonRecovery(item)}>확인 후 작업 복사본 버리기</button>)}</section>}{chat.agentRecovery && <section className="m5-error">{identityUnavailable ? <><strong>이 작업 복사본은 보호된 상태예요.</strong><p>실제 에이전트의 종료 상태를 안전하게 확인할 수 없어요. 원본 프로젝트에는 적용하지 않았고, 이 화면에서 복구를 진행하거나 작업 복사본을 버릴 수 없어요.</p></> : <><strong>실제 에이전트 실행의 종료 확인이 필요해요.</strong><p>이 실행은 복구가 필요한 상태로 남아 있어요. 원본 프로젝트에는 적용하지 않으며, 종료 상태를 다시 확인한 뒤에만 이 작업 복사본을 버릴 수 있어요.</p><button type="button" disabled={!admissionReady || chat.agentRecoveryResolving} onClick={() => void controller.abandonAgentRecovery()}>{chat.agentRecoveryResolving ? "종료 상태 확인 중…" : "종료 상태 확인 후 작업 복사본 버리기"}</button></>}</section>}<AgentRuns runs={chat.agentRuns} /><div className="m5-timeline-list">{chat.messages.map((message) => <article key={message.messageId} className={`m5-message m5-message-${message.kind}`}><div><strong>{message.kind === "user" ? "나" : message.roleId ? ({ planning: "기획", coding: "코딩", review: "검토" })[message.roleId] : "상태"}</strong>{message.roleId && <span> · {profileLabel(chat.activeChat!, message.roleId)}</span>}<time>{new Date(message.createdAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time></div><p>{message.text}</p>{message.runId && <small>실행 ID: {message.runId}</small>}</article>)}</div><footer><span>데모는 실제 모델을 실행하지 않아요. 실제 실행은 설치·로그인·정확한 역할 설정을 확인한 뒤에만 시작해요.</span>{(["planning", "coding", "review"] as const).map((role) => <button key={`demo-${role}`} type="button" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={() => void controller.startDemo(role)}>데모 실행 · {({ planning: "기획", coding: "코딩", review: "검토" })[role]}</button>)}{(["planning", "coding", "review"] as const).map((role) => <button key={`actual-${role}`} type="button" className="m5-secondary" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={() => void controller.startAgent(role)}>{chat.agentStartingRoleId === role ? "실제 연결 확인 중…" : `실제 실행 준비 · ${({ planning: "기획", coding: "코딩", review: "검토" })[role]}`}</button>)}</footer></section>;
}

function AgentRuns({ runs }: { runs: ReturnType<ChatUiController["snapshot"]>["agentRuns"] }) {
  if (runs.length === 0) return null;
  return <section className="m5-agent-runs" aria-label="실제 에이전트 실행 상태"><strong>실제 에이전트 실행</strong>{runs.map((run) => <article key={run.runId}><span>{({ planning: "기획", coding: "코딩", review: "검토" })[run.selectedProfile.roleId]}</span><span>{run.selectedProfile.runtime === "codex" ? "Codex" : "Claude Code"} · {run.selectedProfile.modelSelector}{run.selectedProfile.reasoningEffort ? ` · ${run.selectedProfile.reasoningEffort}` : ""}</span><span> · {run.state}</span></article>)}</section>;
}

function profileLabel(chat: NonNullable<ReturnType<ChatUiController["snapshot"]>["activeChat"]>, roleId: RoleProfileV1["roleId"]): string { const profile = chat.roleSnapshots.find((item) => item.roleId === roleId); return profile ? `${profile.runtime === "codex" ? "Codex" : "Claude Code"} · ${profile.modelSelector}${profile.reasoningEffort ? ` · ${profile.reasoningEffort}` : ""}` : ""; }

function PanelTitle({ active, panel, onSelect, children }: { active: boolean; panel: WorkbenchPanel; onSelect: (panel: WorkbenchPanel) => void; children: ReactNode }) {
  return <button type="button" className="m5-panel-title" aria-pressed={active} onClick={() => onSelect(panel)}>{children}</button>;
}

function EditorContent({ tab, readOnly, onChange }: { tab: EditorTab | null; readOnly: boolean; onChange: (text: string) => void }) {
  if (!tab) return <Empty text="파일을 열면 여기서 코드를 읽고 편집할 수 있어요." />;
  if (tab.mode === "binary") return <Empty text="이 파일은 이진 파일이라 내용을 편집하지 않아요." />;
  if (tab.mode === "tooLarge") return <Empty text="5MiB보다 큰 파일은 안전하게 미리 보지 않아요." />;
  if (tab.saveState === "conflicted") {
    return <section className="m5-conflict"><strong>저장 충돌</strong><p>내 초안은 그대로 보존했어요. 필요한 내용을 아래에서 복사한 뒤 저장된 파일을 다시 불러올 수 있어요.</p><textarea aria-label="보존된 편집 내용" readOnly value={tab.text} style={{ width: "100%", minHeight: 240, boxSizing: "border-box" }} /><details><summary>기술 정보</summary><pre>{tab.technicalDetail ?? "external file change"}</pre></details></section>;
  }
  return <MonacoEditor tab={tab} readOnly={readOnly} onChange={onChange} />;
}

function MonacoEditor({ tab, readOnly, onChange }: { tab: EditorTab; readOnly: boolean; onChange: (text: string) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const model = useRef<Monaco.editor.ITextModel | null>(null);

  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | undefined;
    void loadMonaco().then((monaco) => {
      if (cancelled || !host.current) return;
      const nextModel = monaco.editor.createModel(tab.text, languageFor(tab.path));
      model.current = nextModel;
      const nextEditor = monaco.editor.create(host.current, {
        model: nextModel,
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        readOnly: readOnly || tab.saveState === "saving",
        ariaLabel: `${tab.path} 코드 편집기`,
      });
      editor.current = nextEditor;
      const subscription = nextModel.onDidChangeContent(() => onChange(nextModel.getValue()));
      cleanup = () => {
        subscription.dispose();
        nextEditor.dispose();
        nextModel.dispose();
        editor.current = null;
        model.current = null;
      };
    });
    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [tab.path]);

  useEffect(() => {
    const currentModel = model.current;
    if (currentModel && currentModel.getValue() !== tab.text) currentModel.setValue(tab.text);
    editor.current?.updateOptions({ readOnly: tab.saveState === "saving" });
  }, [tab.text, tab.saveState, readOnly]);

  return <div className="m5-monaco" ref={host} />;
}

function DiffDetails({ descriptor, text }: { descriptor: NonNullable<ReturnType<ProjectEditorController["snapshot"]>["diffFile"]>; text: string }) {
  const rows = useMemo(() => diffRows(text), [text]);
  const [page, setPage] = useState(0);
  useEffect(() => setPage(0), [descriptor, text]);
  const pages = Math.max(1, Math.ceil(rows.length / DIFF_PAGE_ROWS));
  const current = Math.min(page, pages - 1);
  const visible = rows.slice(current * DIFF_PAGE_ROWS, (current + 1) * DIFF_PAGE_ROWS);
  return (
    <section className="m5-diff-details" aria-label="선택한 변경 상세">
      <strong>{descriptor.path}</strong>
      <p>{labelForKind(descriptor.kind)}{descriptor.oldPath ? ` · 이전: ${descriptor.oldPath}` : ""}</p>
      {descriptor.kind === "binary" ? <Empty text="이진 변경은 내용 대신 파일 변경 사실만 보여요." /> : !text ? <Empty text="표시할 텍스트 변경이 없어요." /> : <>
        <p>− 삭제된 줄 · + 추가된 줄 · 왼쪽 번호는 변경 전, 오른쪽은 변경 후예요.</p>
        <div className="m5-diff-lines" tabIndex={0} aria-label="변경 전후 줄 비교">
          <table><thead><tr><th scope="col">이전</th><th scope="col">이후</th><th scope="col">내용</th></tr></thead><tbody>
            {visible.map((row, index) => <tr key={current * DIFF_PAGE_ROWS + index} className={`m5-diff-${row.kind}`}><td>{row.before}</td><td>{row.after}</td><td><code>{row.text}</code></td></tr>)}
          </tbody></table>
        </div>
        {pages > 1 && <nav className="m5-diff-pages" aria-label="변경 내용 페이지">
          <button type="button" disabled={current === 0} onClick={() => setPage(current - 1)}>이전 변경</button>
          <span aria-live="polite">{current + 1} / {pages} 페이지 · 전체 {rows.length}줄</span>
          <button type="button" disabled={current === pages - 1} onClick={() => setPage(current + 1)}>다음 변경</button>
        </nav>}
      </>}
    </section>
  );
}

function ChangeKind({ entry }: { entry: DiffEntryV1 }) {
  return <span className={`m5-change-kind m5-kind-${entry.kind}`}>{labelForKind(entry.kind)}</span>;
}

function labelForKind(kind: DiffEntryV1["kind"]): string {
  return ({ added: "추가", modified: "수정", deleted: "삭제", renamed: "이름 변경", binary: "이진" })[kind];
}

function languageFor(path: string): string {
  if (path.endsWith(".tsx") || path.endsWith(".ts")) return "typescript";
  if (path.endsWith(".js") || path.endsWith(".jsx")) return "javascript";
  if (path.endsWith(".json")) return "json";
  if (path.endsWith(".md")) return "markdown";
  return "plaintext";
}

function fileName(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

function Empty({ text }: { text: string }) {
  return <p className="m5-empty">{text}</p>;
}
