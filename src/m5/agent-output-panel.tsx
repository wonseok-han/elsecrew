import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../i18n";
import { AgentOutputReader, type AgentOutputSource } from "./agent-output-reader.ts";
import { reduceOutputWindow, type OutputWindow } from "./agent-output-window.ts";
import { resolveM5Bridge } from "./bridge-boundary.ts";
import type { AgentRunRecordV1 } from "./contracts/agent-adapter-v1.ts";

function abortable<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  return new Promise((resolve, reject) => {
    const abort = () => reject(new Error("output read stopped"));
    if (signal.aborted) { promise.catch(() => {}); abort(); return; }
    signal.addEventListener("abort", abort, { once: true });
    promise.then(resolve, reject).finally(() => signal.removeEventListener("abort", abort));
  });
}

/** One selected run/window at a time; switching never starts or cancels a run. */
export function AgentOutputPanel({ runs }: { runs: readonly AgentRunRecordV1[] }) {
  const { t } = useI18n();
  const [selected, select] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const run = runs.find(value => value.runId === selected) ?? runs[0];
  const binding = useMemo(() => run ? {
    chatId: run.chatId, taskId: run.taskId, projectId: run.projectId,
    worktreeId: run.worktreeId, workspaceIdentity: run.workspaceIdentity,
    runId: run.runId, roleId: run.selectedProfile.roleId,
    capabilityFingerprint: run.capabilityFingerprint,
  } : null, [run?.runId, run?.chatId, run?.taskId, run?.projectId, run?.worktreeId, run?.workspaceIdentity, run?.selectedProfile.roleId, run?.capabilityFingerprint]);
  const [view, setView] = useState<OutputWindow | null>(null);
  const [state, setState] = useState<"loading" | "legacy" | "live" | "failed">("loading");
  useEffect(() => {
    const bridge = resolveM5Bridge(window.m5);
    setView(null);
    if (!binding || bridge?.version !== 8) { setState("legacy"); return; }
    const abort = new AbortController();
    let reader: AgentOutputReader | undefined;
    let timer: ReturnType<typeof setInterval> | undefined;
    let current: OutputWindow = { binding, cursor: 0, rows: [], hasEarlier: false };
    setState("loading");
    const fail = () => { if (!abort.signal.aborted) { setState("failed"); clearInterval(timer); } };
    const source: AgentOutputSource = {
      subscribe: async (_binding, notify) => bridge.onAgentPublicEvent(() => notify()),
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
  if (!binding) return null;
  return <section className="m5-durable-output" aria-label={t("m5.output.title")}>
    <label>{t("m5.output.title")} <select value={run!.runId} onChange={event => select(event.target.value)}>
      {runs.map(value => <option key={value.runId} value={value.runId}>{t(`m5.roles.${value.selectedProfile.roleId}`)} · {value.runId}</option>)}
    </select></label>
    {state === "loading" && <p role="status">{t("m5.output.loading")}</p>}
    {state === "legacy" && <p>{t("m5.output.legacy")}</p>}
    {state === "failed" && <p role="alert">{t("m5.output.failed")} <button onClick={() => setRetry(value => value + 1)}>{t("m5.output.retry")}</button></p>}
    {view?.binding.runId === binding.runId && <>
      {view.hasEarlier && <p>{t("m5.output.window")}</p>}
      {view.rows.map(row => <article key={row.key}>{row.truncated && <small>{t("m5.output.truncated")}</small>}<pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{row.text}</pre></article>)}
    </>}
  </section>;
}
