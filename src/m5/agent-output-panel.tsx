import { orchestratorMessage } from "./orchestrator-message.ts";
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { AgentOutputReader, type AgentOutputSource } from "./agent-output-reader.ts";
import { reduceOutputWindow, startingSentence, type OutputDisplayRow, type OutputWindow } from "./agent-output-window.ts";
import { resolveM5Bridge } from "./bridge-boundary.ts";
import type { AgentRunRecordV1 } from "./contracts/agent-adapter-v1.ts";
import type { RoleDefinitionV3 } from "./contracts/run-recovery-v1.ts";
import { isLeadRole } from "./role-colors.ts";
import { roleLabel } from "./role-names.ts";
import { decodeOutputPage, type OutputPage, type OutputPageRequest } from "./contracts/agent-output-pages.ts";
import { activityLine, collapseRepeats, parseWorkActivity, WORK_OUTCOME_KEYS } from "./work-history.ts";

function abortable<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  return new Promise((resolve, reject) => {
    const abort = () => reject(new Error("output read stopped"));
    if (signal.aborted) { promise.catch(() => {}); abort(); return; }
    signal.addEventListener("abort", abort, { once: true });
    promise.then(resolve, reject).finally(() => signal.removeEventListener("abort", abort));
  });
}

/** One tool use, as a compact line: a Korean verb and the place it worked
 * on. The agent's own words about a command fold under the line. Unreadable
 * rows are left out. */
const WorkActivityRow = memo(function WorkActivityRow({ text, count = 1 }: { text: string; count?: number }) {
  const { t } = useI18n();
  const activity = parseWorkActivity(text);
  if (!activity) return null;
  const line = activityLine(activity);
  const head = <>
    <span className="m5-activity-mark" aria-hidden="true" />
    <span className="m5-activity-label">{t(line.label, { tool: activity.tool ?? "" })}</span>
    {line.target && <span className="m5-activity-target">{line.target}</span>}
    {count > 1 && <span className="m5-activity-count">{t("m5.output.activity.repeat", { count })}</span>}
    {activity.outcome && <span className="m5-activity-outcome">{t(WORK_OUTCOME_KEYS[activity.outcome])}</span>}
  </>;
  return line.original
    ? <details className="m5-activity-row m5-activity-folded" data-action={activity.action} data-outcome={activity.outcome}><summary>{head}</summary><p className="m5-activity-original" dir="auto">{line.original}</p></details>
    : <p className="m5-activity-row" data-action={activity.action} data-outcome={activity.outcome}>{head}</p>;
});

/** The agent thinking aloud between tool uses: one line, the rest folded. */
function OutputNote({ text }: { text: string }) {
  const first = text.trim().split("\n")[0] ?? "";
  const short = Array.from(first).length > 90 ? `${Array.from(first).slice(0, 89).join("")}…` : first;
  if (short === text.trim()) return <p className="m5-output-note" dir="auto">{short}</p>;
  return <details className="m5-output-note"><summary dir="auto">{short}</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{text}</pre></details>;
}

const LiveOutputRow = memo(function LiveOutputRow({ row, truncatedLabel, role, team, count }: {
  row: OutputDisplayRow; truncatedLabel: string; role: string; team?: readonly string[]; count: number;
}) {
  if (row.channel === "activity") return <WorkActivityRow text={row.text} count={count} />;
  return <article>{row.truncated && <small>{truncatedLabel}</small>}<OutputNote text={orchestratorMessage(row.text, role, team)} /></article>;
});

/** One selected run/window at a time; switching never starts or cancels a run.
 * Text the conversation already shows as that run's message is left out here,
 * so an answer is read once; tool activity and text still streaming stay. */
export function AgentOutputPanel({ runs, definitions, posted = [] }: {
  runs: readonly AgentRunRecordV1[]; definitions?: readonly RoleDefinitionV3[];
  posted?: readonly { runId: string | null; text: string }[];
}) {
  const { t } = useI18n();
  const team = useMemo(() => definitions?.map(definition => definition.roleId), [definitions]);
  const [selected, select] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const orderedRuns = useMemo(() => [...runs].sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt)), [runs]);
  const run = orderedRuns.find(value => value.runId === selected) ?? orderedRuns[0];
  const binding = useMemo(() => run ? {
    chatId: run.chatId, taskId: run.taskId, projectId: run.projectId,
    worktreeId: run.worktreeId, workspaceIdentity: run.workspaceIdentity,
    runId: run.runId, roleId: run.selectedProfile.roleId,
    capabilityFingerprint: run.capabilityFingerprint,
  } : null, [run?.runId, run?.chatId, run?.taskId, run?.projectId, run?.worktreeId, run?.workspaceIdentity, run?.selectedProfile.roleId, run?.capabilityFingerprint]);
  const [view, setView] = useState<OutputWindow | null>(null);
  const [state, setState] = useState<"loading" | "legacy" | "live" | "failed">("loading");
  const [historyRequest, setHistoryRequest] = useState<OutputPageRequest | null>(null);
  const [historyPage, setHistoryPage] = useState<OutputPage | null>(null);
  const [historyState, setHistoryState] = useState<"loading" | "ready" | "failed">("ready");
  const outputRef = useRef<HTMLDivElement>(null);
  const followLatest = useRef(true);
  const revealLatest = useRef(false);
  const followConversation = useRef(false);
  const conversationTop = useRef<number | null>(null);
  // Track the outer viewport separately: reaching the inner list's bottom
  // does not mean its latest row remains visible in the conversation.
  useEffect(() => {
    const list = outputRef.current;
    const conversation = list?.closest<HTMLElement>(".m5-conversation-scroll");
    if (!list || !conversation) return;
    const remember = () => {
      conversationTop.current = conversation.scrollTop;
      const row = list.lastElementChild?.getBoundingClientRect();
      const viewport = conversation.getBoundingClientRect();
      followConversation.current = Boolean(row && conversation.clientHeight > 0
        && row.bottom > viewport.top && row.bottom <= viewport.top + conversation.clientTop + conversation.clientHeight + 2);
    };
    conversation.addEventListener("scroll", remember);
    remember();
    return () => conversation.removeEventListener("scroll", remember);
  }, [binding]);
  const [newOutput, setNewOutput] = useState(false);
  useLayoutEffect(() => {
    followLatest.current = true;
    revealLatest.current = false;
    followConversation.current = false;
    conversationTop.current = null;
    setNewOutput(false);
  }, [binding]);
  useLayoutEffect(() => {
    const list = outputRef.current;
    if (!list || view?.binding.runId !== binding?.runId || historyRequest) return;
    if (followLatest.current) {
      list.scrollTop = list.scrollHeight;
      // Preserve an already visible latest row as layout grows; never pull a
      // reader back from older content in the surrounding conversation.
      const conversation = list.closest<HTMLElement>(".m5-conversation-scroll");
      // Scroll events are queued. Honor a changed position even if its event
      // has not arrived before this output commit.
      if (conversation && conversationTop.current !== null
        && Math.abs(conversation.scrollTop - conversationTop.current) > 2) followConversation.current = false;
      if (conversation && followConversation.current && conversation.clientHeight > 0) {
        const bottom = conversation.getBoundingClientRect().top + conversation.clientTop + conversation.clientHeight;
        const overflow = list.getBoundingClientRect().bottom - bottom;
        if (overflow > 0) conversation.scrollTop += overflow;
      }
      // An explicit latest action can also return from older conversation content.
      if (revealLatest.current) {
        list.lastElementChild?.scrollIntoView({ block: "nearest", inline: "nearest" });
        revealLatest.current = false;
      }
    } else setNewOutput(true);
    const conversation = list.closest<HTMLElement>(".m5-conversation-scroll");
    const row = list.lastElementChild?.getBoundingClientRect();
    if (conversation && row) {
      conversationTop.current = conversation.scrollTop;
      const viewport = conversation.getBoundingClientRect();
      followConversation.current = conversation.clientHeight > 0 && row.bottom > viewport.top
        && row.bottom <= viewport.top + conversation.clientTop + conversation.clientHeight + 2;
    }
  }, [view, binding, historyRequest]);
  useLayoutEffect(() => {
    if (historyPage && outputRef.current) outputRef.current.scrollTop = 0;
  }, [historyPage]);
  const showLatest = () => {
    followLatest.current = true;
    setNewOutput(false);
    revealLatest.current = Boolean(historyRequest);
    setHistoryRequest(null);
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
      if (!historyRequest) outputRef.current.lastElementChild?.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  };
  useEffect(() => {
    const bridge = resolveM5Bridge(window.m5);
    setHistoryPage(null);
    if (!historyRequest || !binding || historyRequest.binding.runId !== binding.runId || bridge?.version !== 8) return;
    const abort = new AbortController();
    setHistoryState("loading");
    void abortable(bridge.getAgentOutputPage(historyRequest), abort.signal).then(value => {
      if (abort.signal.aborted) return;
      const page = decodeOutputPage(new TextEncoder().encode(JSON.stringify(value)), historyRequest);
      setHistoryPage({ ...page, request: historyRequest });
      setHistoryState("ready");
    }).catch(() => { if (!abort.signal.aborted) setHistoryState("failed"); });
    return () => abort.abort();
  }, [binding, historyRequest]);
  useEffect(() => {
    const bridge = resolveM5Bridge(window.m5);
    setView(null);
    setHistoryRequest(null);
    if (!binding || bridge?.version !== 8) { setState("legacy"); return; }
    const abort = new AbortController();
    let reader: AgentOutputReader | undefined;
    let timer: ReturnType<typeof setInterval> | undefined;
    let current: OutputWindow = { binding, cursor: 0, rows: [], hasEarlier: false };
    setState("loading");
    const fail = () => { if (!abort.signal.aborted) { setState("failed"); clearInterval(timer); } };
    const source: AgentOutputSource = {
      subscribe: async (selectedBinding, notify) => {
        const publicOff = bridge.onAgentPublicEvent(event => {
          if (event.runId === selectedBinding.runId) notify();
        });
        const stateOff = bridge.onAgentRunStateChanged(event => {
          if (event.run.runId === selectedBinding.runId) notify();
        });
        const outputOff = bridge.onAgentOutputAvailable(head => {
          if (head.binding.runId === selectedBinding.runId) notify();
        });
        return () => { publicOff(); stateOff(); outputOff(); };
      },
      head: (value, signal) => abortable(bridge.getAgentOutputHead(value), signal),
      page: async (request, signal) => new TextEncoder().encode(JSON.stringify(await abortable(bridge.getAgentOutputPage(request), signal))),
    };
    void abortable(bridge.getAgentOutputHead(binding), abort.signal).then(async head => {
      if (abort.signal.aborted) return;
      if (head === null) { setState("legacy"); return; }
      reader = new AgentOutputReader(source, binding, async (rows, signal) => {
        if (signal.aborted || abort.signal.aborted) return;
        current = reduceOutputWindow(current, rows);
        setView(current);
      }, fail);
      await reader.start();
      if (abort.signal.aborted) return;
      setState("live");
      if (!reader.sealed) timer = setInterval(() => {
        void reader!.refresh().then(() => { if (reader!.sealed) clearInterval(timer); }).catch(fail);
      }, 500);
    }).catch(fail);
    return () => { abort.abort(); clearInterval(timer); reader?.stop(); };
  }, [binding, retry]);
  const shown = useMemo(() => new Set(posted.filter(message => message.runId === binding?.runId).map(message => message.text.trim())), [posted, binding?.runId]);
  if (!binding) return null;
  // A member just started: until its first activity arrives (often 10–20 s
  // while the agent starts and reads its task), say what it is doing rather
  // than "loading" (ADE 1차 완료 기준 2: never 10 s without a sentence).
  const starting = startingSentence(run!.state, view?.binding.runId === binding.runId ? view.rows : null);
  const historyActive = historyRequest?.binding.runId === binding.runId;
  const visibleHistory = historyActive && historyPage?.request === historyRequest ? historyPage : null;
  return <section className="m5-durable-output" data-run-id={run!.runId} aria-label={t("m5.output.title")}>
    <label className="m5-output-heading"><span>{t("m5.output.title")}</span> {orderedRuns.length > 1 && <select aria-label={t("m5.output.title")} value={run!.runId} onChange={event => select(event.target.value)}>
      {orderedRuns.map(value => <option key={value.runId} value={value.runId}>{/* Coordinator marked by ID; a name is isolated so it cannot reorder the line (review PR #8 I1). */}{isLeadRole(value.selectedProfile.roleId) ? "★ " : ""}{`\u2068${roleLabel(value.selectedProfile.roleId, definitions, t)}\u2069`} · {value.selectedProfile.runtime === "codex" ? "Codex" : "Claude Code"} · {new Date(value.createdAt).toLocaleTimeString([], {hour:"2-digit",minute:"2-digit"})}</option>)}
    </select>}</label>
    {state === "loading" && !starting && <p role="status">{t("m5.output.loading")}</p>}
    {starting && !historyActive && <p className="m5-activity-row m5-activity-start" data-action="start" role="status">
      <span className="m5-activity-mark" aria-hidden="true" /><span className="m5-activity-label">{t("m5.output.starting")}</span></p>}
    {state === "legacy" && <p>{t("m5.output.legacy")}</p>}
    {state === "failed" && <p role="alert">{t("m5.output.failed")} <button onClick={() => setRetry(value => value + 1)}>{t("m5.output.retry")}</button></p>}
    {view?.binding.runId === binding.runId && (view.hasEarlier || historyActive) && <nav aria-label={t("m5.output.history")}>
      <button type="button" disabled={view.cursor === 0 || (historyActive && historyState === "loading")}
        onClick={() => setHistoryRequest({ formatVersion: 1, binding, afterSequence: 0,
          throughSequence: historyActive ? historyRequest!.throughSequence : view.cursor, limit: 100 })}>{t("m5.output.first")}</button>
      {historyActive && <>
        <button type="button" disabled={!visibleHistory || historyState !== "ready" || visibleHistory.nextAfterSequence >= historyRequest!.throughSequence}
          onClick={() => setHistoryRequest({ ...historyRequest!, afterSequence: visibleHistory!.nextAfterSequence })}>{t("m5.output.next")}</button>
      </>}
      {historyActive && <button type="button" onClick={showLatest}>{t("m5.chat.latest")}</button>}
    </nav>}
    <div ref={outputRef} className="m5-output-scroll" tabIndex={0} aria-label={t("m5.output.title")}
      onScroll={event => {
        if (historyActive) return;
        const list = event.currentTarget;
        if (list.clientHeight === 0) return;
        followLatest.current = list.scrollHeight - list.scrollTop - list.clientHeight <= 32;
        if (followLatest.current) setNewOutput(false);
      }}>
    {historyActive && <>
      <p>{t("m5.output.historyHelp")}</p>
      {historyState === "loading" && <p role="status">{t("m5.output.loading")}</p>}
      {historyState === "failed" && <p role="alert">{t("m5.output.failed")} <button type="button" onClick={() => setHistoryRequest({ ...historyRequest! })}>{t("m5.output.retry")}</button></p>}
      {visibleHistory && <>
        <p>{t("m5.output.range", { first: visibleHistory.request.afterSequence + 1, last: visibleHistory.nextAfterSequence, total: visibleHistory.request.throughSequence })}</p>
        {visibleHistory.rows.map(row => row.itemUpdate?.channel === "activity"
          ? <WorkActivityRow key={row.event.eventId} text={row.event.text} />
          : <article key={row.event.eventId}>
          <small>{row.outputSequence} · {row.itemUpdate ? t(row.itemUpdate.operation === "append" ? "m5.output.append" : "m5.output.complete") : t("m5.output.message")}</small>
          <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{orchestratorMessage(row.event.text, run.selectedProfile.roleId, team)}</pre>
        </article>)}
      </>}
    </>}
    {!historyActive && view?.binding.runId === binding.runId && <>
      {view.hasEarlier && <p>{t("m5.output.window")}</p>}
      {collapseRepeats(view.rows.filter(row => row.channel === "activity" || !shown.has(row.text.trim())), (a, b) => a.channel === "activity" && b.channel === "activity" && a.text === b.text)
        .map(({ row, count }) => <LiveOutputRow key={row.key} row={row} count={count} role={run.selectedProfile.roleId} team={team} truncatedLabel={t("m5.output.truncated")} />)}
    </>}
    </div>
    {!historyActive && newOutput && <button type="button" onClick={showLatest}>{t("m5.chat.newMessages")}</button>}
  </section>;
}
