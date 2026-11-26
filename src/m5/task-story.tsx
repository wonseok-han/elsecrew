import { submitChatShortcut } from "./chat-submit-shortcut.ts";
import { AgentOutputPanel } from "./agent-output-panel.tsx";
import { groupRunHistory, runOutcomeNotice, runOrder, runDurationSeconds } from "./work-status.ts";
import { modelLabel } from "./model-catalog.ts";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../i18n";
import { type RoleDefinitionV3 } from "./contracts/run-recovery-v1.ts";
import { roleLabel } from "./role-names.ts";
import { canAbandonAgentRecovery, ChatUiController } from "./chat-state.ts";
import { ApprovalCard, ApprovalWithdrawnNote, type WithdrawnApproval } from "./approval-card.tsx";
import { noticesForChat } from "./approval-display.ts";
import { taskPhaseLabelKey, taskPhaseTone, type TaskKind, type TaskPhase } from "./task-status.ts";
import { timelineMessages } from "./timeline-messages.ts";
import { StageBar } from "./stage-bar.tsx";
import { MemberCard, StoryMessageCard, UserBubble } from "./member-card.tsx";
import { storyItems } from "./task-story-model.ts";
import { createPortal } from "react-dom";
import { roleSettingsAsV3 } from "./contracts/run-recovery-v1.ts";

export function FollowupComposer({ controller, chat, admissionReady }: { controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean }) {
  const { t, lang } = useI18n();
  const draft = chat.followups?.[chat.activeChat!.chatId];
  // Busy is not "unavailable": while a run starts or works the composer just waits.
  const busy = chat.activeChat?.state === "running" || Boolean(chat.agentStartingRoleId) || chat.agentRuns.some(run => ["preparing", "running", "cancelling"].includes(run.state));
  const allowed = admissionReady && !!chat.activeChat && chat.activeChat.schemaVersion !== 1 && !busy && controller.canAppendFollowup();
  return <form className="m5-followup" onKeyDown={submitChatShortcut} aria-label={t("m5.chat.followup")} onSubmit={event => { event.preventDefault(); void controller.appendFollowup(admissionReady, true, lang); }}>
    <label htmlFor="m5-followup-text" className="m5-sr-only">{t("m5.chat.input")}</label>
    <textarea id="m5-followup-text" value={draft?.text ?? ""} onChange={event => controller.editFollowup(event.target.value)} placeholder={busy ? t("m5.story.composerBusy") : t("m5.chat.placeholder")} rows={2} />
    {draft?.error && <div role="alert"><p>{t("m5.chat.saveError")}</p><details><summary>{t("m5.chat.details")}</summary><pre>{draft.error}</pre></details></div>}
    {draft?.request && !draft.sending && <details><summary>{t("m5.chat.retryMessage")}</summary><p>{draft.request.text}</p></details>}
    <div className="m5-followup-actions">
      {/* Stop sits by the always-visible composer so a long conversation never hides it. */}
      {!!chat.activeChat && chat.activeChat.schemaVersion !== 1 && chat.activeChat.state === "running" && window.m5Projects && <button type="button" className="m5-secondary m5-stop-team" onClick={() => void controller.cancelTeamTask(input => window.m5Projects!.request(input))}>{t("m5.chat.stopTeam")}</button>}
      <button type="submit" title={t("m5.chat.sendShortcut")} aria-keyshortcuts="Enter Meta+Enter Control+Enter" disabled={!allowed || draft?.sending || (!draft?.request && !draft?.text.trim())}>{draft?.sending ? t("m5.chat.saving") : draft?.request ? t("m5.chat.retry") : t("m5.chat.add")}</button>
    </div>
    {!allowed && !busy && <p>{t("m5.chat.unavailable")}</p>}
  </form>;
}


export function TeamTimeline({ controller, chat, admissionReady, fixtureAvailable, onCoordinatorChat, phase = null, phaseKind = null, onApprovalsWaiting, pinnedSlot = null }: { pinnedSlot?: HTMLElement | null; onCoordinatorChat: () => void; controller: ChatUiController; chat: ReturnType<ChatUiController["snapshot"]>; admissionReady: boolean; fixtureAvailable: boolean; phase?: TaskPhase | null; phaseKind?: TaskKind | null; onApprovalsWaiting?: (count: number) => void }) {
  const { lang, t } = useI18n();
  const messageTime = useMemo(() => new Intl.DateTimeFormat(lang, { hour: "2-digit", minute: "2-digit" }), [lang]);
  const messagesRef = useRef<HTMLDivElement>(null);
  const scrollSurface = () => messagesRef.current?.closest<HTMLDivElement>(".m5-conversation-scroll") ?? messagesRef.current;
  const followLatest = useRef(true);
  const [newMessages, setNewMessages] = useState(false);
  const [historyStart, setHistoryStart] = useState<number | null>(null);
  const messageStart = Math.min(historyStart ?? Math.max(0, chat.messages.length - 200), Math.max(0, chat.messages.length - 1));
  // The goal is the title; filler status lines are left out (timeline-messages.ts).
  const visibleMessages = timelineMessages(chat.messages.slice(messageStart, messageStart + 200), messageStart === 0 ? chat.activeChat?.goal ?? null : null);
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
  // Requests still waiting when their run ended: said here for a moment, since
  // the run's card goes away with the run.
  const [withdrawnAtEnd, setWithdrawnAtEnd] = useState<WithdrawnApproval[]>([]);
  const timelineChat = useRef<string | null>(null);
  timelineChat.current = chat.activeChat?.chatId ?? null;
  const onWithdrawnAtEnd = useCallback((items: WithdrawnApproval[]) => {
    // A late notice from a task left behind stays out of this one.
    const mine = noticesForChat(items, timelineChat.current);
    if (!mine.length) return;
    setWithdrawnAtEnd(list => [...list, ...mine]);
    const ids = new Set(mine.map(item => item.approvalId));
    window.setTimeout(() => setWithdrawnAtEnd(list => list.filter(item => !ids.has(item.approvalId))), 8000);
  }, []);
  useEffect(() => { setWithdrawnAtEnd([]); }, [chat.activeChat?.chatId]);
  const startingActual = chat.agentStartingRoleId !== null;
  const blocked = controller.isActionBlocked();
  const identityUnavailable = chat.agentRecovery !== null && !canAbandonAgentRecovery(chat.agentRecovery);
  // One card per run; messages of runs earlier than this page stay with their page.
  const firstShown = messageStart > 0 ? Date.parse(visibleMessages[0]?.createdAt ?? "") : Number.NEGATIVE_INFINITY;
  const items = storyItems(visibleMessages, chat.agentRuns.filter(run => !(Date.parse(run.createdAt) < firstShown)));
  const teamRoleIds = chat.settings ? new Set(roleSettingsAsV3(chat.settings).roles.map(role => role.roleId)) : null;
  const working = phase === "draft" || phase === "running" || phase === "attention" || chat.activeChat?.state === "running" || chat.activeChat?.state === "preparing" || startingActual;
  return <section className="m5-timeline m5-story" aria-label={t("m5.chat.title")}>
    <TaskHeader chat={chat.activeChat!} phase={phase} phaseKind={phaseKind} working={working} actions={<div className="m5-run-actions">{fixtureActive && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelRun()}>{t("m5.chat.cancelDemo")}</button>}{actualActive && chat.activeChat?.schemaVersion === 1 && <button type="button" className="m5-secondary" disabled={!admissionReady} onClick={() => void controller.cancelAgentRun(actualActive)}>{t("m5.chat.cancelActual")}</button>}</div>} />
    {chat.activeChat?.state === "discarded" && !controller.isRecoveryBlocked() && <section className="m5-notice"><p>{t("m5.chat.discardedHelp")}</p></section>}{controller.isRecoveryBlocked() && <section className="m5-error"><strong>{t("m5.chat.recovery")}</strong><p>{identityUnavailable ? t("m5.chat.identityUnavailable") : t("m5.chat.blocked")}</p>{chat.recoveries.filter((item) => item.state === "unresolved").map((item) => <button key={item.recoveryId} type="button" disabled={!admissionReady} onClick={() => void controller.abandonRecovery(item, lang)}>{t("m5.chat.discard")}</button>)}</section>}{chat.agentRecovery && <section className="m5-error">{identityUnavailable ? <><strong>{t("m5.chat.protected")}</strong><p>{t("m5.chat.protectedHelp")}</p></> : <><strong>{t("m5.chat.termination")}</strong><p>{t("m5.chat.terminationHelp")}</p><button type="button" disabled={!admissionReady || chat.agentRecoveryResolving} onClick={() => void controller.abandonAgentRecovery(lang)}>{chat.agentRecoveryResolving ? t("m5.chat.checkingTermination") : t("m5.chat.discardAfterTermination")}</button></>}</section>}
    <StageBar runs={chat.agentRuns} working={working} definitions={chat.activeChat!.roleDefinitions} />
    {chat.messages.length > 200 && <nav className="m5-diff-pages" aria-label={t("m5.chat.messages")}><button type="button" disabled={messageStart === 0} onClick={() => moveHistory(messageStart - 200)}>{t("m5.chat.older")}</button><span>{t("m5.chat.range", { first: messageStart + 1, last: messageStart + visibleMessages.length, total: chat.messages.length })}</span><button type="button" disabled={messageStart + 200 >= chat.messages.length} onClick={() => moveHistory(messageStart + 200)}>{t("m5.chat.newer")}</button><button type="button" disabled={historyStart === null} onClick={showLatest}>{t("m5.chat.latest")}</button></nav>}
    <div ref={messagesRef} className="m5-timeline-list m5-story-flow" tabIndex={0} aria-label={t("m5.chat.messages")}>
      {items.map(item => item.kind === "run"
        ? <MemberCard key={item.key} run={item.run} reports={item.reports} definitions={chat.activeChat!.roleDefinitions} teamRoleIds={teamRoleIds} phase={phase} clock={messageTime} />
        : item.kind === "user" ? <UserBubble key={item.key} message={item.message} clock={messageTime} />
        : <StoryMessageCard key={item.key} message={item.message} definitions={chat.activeChat!.roleDefinitions} clock={messageTime} />)}
      {phase === "review" && !working && <p className="m5-story-next">{t("m5.story.reviewHint")}</p>}
      {(phase === "merged" || phase === "applied") && !working && <p className="m5-story-next m5-story-done">{t("m5.story.inFolder")}</p>}
      {noticesForChat(withdrawnAtEnd, chat.activeChat?.chatId ?? null).length > 0 && <section className="m5-approval-card m5-approval-ended" aria-label={t("m5.approval.title")}>{noticesForChat(withdrawnAtEnd, chat.activeChat?.chatId ?? null).map(item => <ApprovalWithdrawnNote key={item.approvalId} item={item} />)}</section>}
    </div>
    {pinnedSlot ? createPortal(actualActive && admissionReady && !blocked && <ApprovalCard key={actualActive.runId} onPendingChange={onApprovalsWaiting} onWithdrawn={onWithdrawnAtEnd} identity={{chatId:actualActive.chatId,taskId:actualActive.taskId,projectId:actualActive.projectId,worktreeId:actualActive.worktreeId,workspaceIdentity:actualActive.workspaceIdentity,runId:actualActive.runId}} onCancel={() => { if ((chat.activeChat?.schemaVersion ?? 1) !== 1 && window.m5Projects) void controller.cancelTeamTask(input => window.m5Projects!.request(input)); else void controller.cancelAgentRun(actualActive); }} />, pinnedSlot) : actualActive && admissionReady && !blocked && <ApprovalCard key={actualActive.runId} onPendingChange={onApprovalsWaiting} onWithdrawn={onWithdrawnAtEnd} identity={{chatId:actualActive.chatId,taskId:actualActive.taskId,projectId:actualActive.projectId,worktreeId:actualActive.worktreeId,workspaceIdentity:actualActive.workspaceIdentity,runId:actualActive.runId}} onCancel={() => { if ((chat.activeChat?.schemaVersion ?? 1) !== 1 && window.m5Projects) void controller.cancelTeamTask(input => window.m5Projects!.request(input)); else void controller.cancelAgentRun(actualActive); }} />}
    {newMessages && <button type="button" className="m5-secondary m5-new-messages" onClick={showLatest}>{t("m5.chat.newMessages")}</button>}<footer className="m5-execution-footer"><div className="m5-demo-controls">{fixtureAvailable && <span>{t("m5.chat.demoHelp")}</span>}{fixtureAvailable && (["planning", "coding", "review"] as const).map((role) => <button key={`demo-${role}`} type="button" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={() => void controller.startDemo(role)}>{t("m5.chat.demo", { role: t(`m5.roles.${role}`) })}</button>)}</div><div className="m5-actual-controls">{chat.activeChat?.schemaVersion === 1 && <div><p>{t("m5.chat.legacyCoordinator")}</p><button type="button" disabled={!admissionReady || fixtureActive || Boolean(actualActive) || startingActual || blocked} onClick={onCoordinatorChat}>{t("m5.navigation.newChat")}</button></div>}</div></footer></section>;
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

export function AgentRuns({ runs, definitions, posted, phase = null }: { runs: ReturnType<ChatUiController["snapshot"]>["agentRuns"]; definitions?: readonly RoleDefinitionV3[]; posted?: readonly { runId: string | null; text: string }[]; phase?: TaskPhase | null }) {
  const { t, lang } = useI18n();
  // A running card counts its time live; finished ones keep their record.
  const going = runs.some(run => ["preparing", "running", "cancelling"].includes(run.state));
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!going) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [going]);
  if (runs.length === 0) return null;
  const { current, previous } = groupRunHistory(runs);
  const order = runOrder(runs);
  const clock = new Intl.DateTimeFormat(lang === "ko" ? "ko-KR" : "en-US", { hour: "numeric", minute: "2-digit" });
  const renderRun = (run: typeof runs[number]) => {
      const notice = runOutcomeNotice(run, phase);
      const seconds = runDurationSeconds(run, now);
      const started = Date.parse(run.createdAt);
      return <article key={run.runId}>
        <div className="m5-agent-run-heading">
        <span className="m5-run-order">{t("m5.run.order", { n: order.get(run.runId) ?? 0 })}</span>
        <strong>{roleLabel(run.selectedProfile.roleId, definitions, t)}</strong>
        <span className={`m5-run-state m5-run-state-${run.state}`}>{t(`m5.run.${run.state}`)}</span>
        <span className="m5-run-meta">{Number.isFinite(started) ? clock.format(new Date(started)) : ""}{seconds !== null ? ` · ${seconds >= 60 ? t("m5.run.duration.minutes", { m: Math.floor(seconds / 60), s: seconds % 60 }) : t("m5.run.duration.seconds", { s: seconds })}` : ""}</span>
        <span className="m5-run-meta">{run.selectedProfile.runtime === "codex" ? "Codex" : "Claude Code"} · {modelLabel(run.selectedProfile.runtime, run.selectedProfile.modelSelector)}{run.selectedProfile.reasoningEffort ? ` · ${run.selectedProfile.reasoningEffort}` : ""}</span>
        </div>
        {notice && <p>{t(notice)}</p>}
        {run.failure && <p role="alert">{t(`m5.run.failure.${run.failure.stage}`)} <code>{run.failure.stage}/{run.failure.code}</code></p>}
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
    <AgentOutputPanel runs={runs} definitions={definitions} posted={posted} />
    {previous.length > 0 && <details className="m5-agent-run-history">
      <summary>{t("m5.run.previous", { count: previous.length })}</summary>
      <div>{previous.map(renderRun)}</div>
    </details>}
  </section>;
}


/** The story's header (ADE 1차-나 L4a): status, when it started and for how long, and the goal as the title. */
function TaskHeader({ chat, phase, phaseKind, working, actions }: { chat: NonNullable<ReturnType<ChatUiController["snapshot"]>["activeChat"]>; phase: TaskPhase | null; phaseKind: TaskKind | null; working: boolean; actions: ReactNode }) {
  const { lang, t } = useI18n();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!working) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 15_000);
    return () => window.clearInterval(timer);
  }, [working]);
  const started = Date.parse(chat.createdAt);
  const sameDay = Number.isFinite(started) && new Date(started).toDateString() === new Date(now).toDateString();
  const clock = new Intl.DateTimeFormat(lang === "ko" ? "ko-KR" : "en-US", sameDay ? { hour: "numeric", minute: "2-digit" } : { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
  const minutes = Number.isFinite(started) ? Math.max(0, Math.floor((now - started) / 60_000)) : null;
  return <header className="m5-task-header">
    <div className="m5-task-eyebrow">
      {phase && <span className={`m5-pill m5-pill-${taskPhaseTone(phase)}`} data-task-phase={phase}>{t(taskPhaseLabelKey(phase, phaseKind))}</span>}
      {Number.isFinite(started) && <span>{t("m5.story.started", { time: clock.format(new Date(started)) })}{working && minutes !== null ? ` · ${t("m5.when.forMinutes", { n: minutes })}` : ""}</span>}
      <span className="m5-task-header-actions">{actions}</span>
    </div>
    <h2 className="m5-task-goal" title={chat.goal}>{chat.goal}</h2>
  </header>;
}
