/**
 * The preview tab's build bar (결과 미리보기 확장 1단계): "빌드해서 보기",
 * installing what a build needs, stopping, and the build log. The page only
 * asks; main asks the sidecar for the plan and the person in its own box
 * (electron/preview-build.ts). The log arrives already masked.
 */
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { M5PreviewBuildBridgeV1, PreviewBuildJobViewV1, PreviewBuildPlanViewV1, PreviewBuildStepV1 } from "./contracts/preview-v1.ts";
import type { MessageKey } from "../locales/ko";
import { appendBuildLines, previewBuildBar, previewBuildFinished, previewBuildStatus, type PreviewBuildNote, type PreviewBuildStatus } from "./preview-build-model.ts";

const NOTE_KEYS: Record<PreviewBuildNote, MessageKey> = {
  notIgnored: "m5.previewBuild.note.notIgnored", toolMissing: "m5.previewBuild.note.toolMissing", unsupported: "m5.previewBuild.note.unsupported",
};
const STATUS_KEYS: Record<PreviewBuildStatus, MessageKey> = {
  installed: "m5.previewBuild.status.installed", installFailed: "m5.previewBuild.status.installFailed", built: "m5.previewBuild.status.built",
  buildFailed: "m5.previewBuild.status.buildFailed", cancelled: "m5.previewBuild.status.cancelled", timedOut: "m5.previewBuild.status.timedOut",
  declined: "m5.previewBuild.status.declined", busy: "m5.previewBuild.status.busy", refused: "m5.previewBuild.status.refused",
};

const POLL_MS = 500;

export function PreviewBuildBar({ projectId, chatId, active, teamWorking, showingBuild, onBuilt, onShowSource, onShowBuild }: {
  projectId: string | null;
  chatId: string | null;
  /** The preview tab is on screen: only then is the engine asked for the
   * plan (it runs Git checks; switching tasks stays quick). */
  active: boolean;
  /** The task's team is working: a build waits (it would read half-written files). */
  teamWorking: boolean;
  showingBuild: boolean;
  onBuilt: () => void;
  onShowSource: () => void;
  onShowBuild: () => void;
}) {
  const { t } = useI18n();
  const bridge: M5PreviewBuildBridgeV1 | undefined = window.m5PreviewBuild;
  const [plan, setPlan] = useState<PreviewBuildPlanViewV1 | null>(null);
  const [job, setJob] = useState<PreviewBuildJobViewV1 | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [logOpen, setLogOpen] = useState(false);
  const [start, setStart] = useState<"declined" | "busy" | "refused" | null>(null);
  const [asking, setAsking] = useState(false);
  const [planKey, setPlanKey] = useState(0);
  const cursor = useRef(0);
  const jobRef = useRef<PreviewBuildJobViewV1 | null>(null);
  jobRef.current = job;
  const callbacks = useRef({ onBuilt });
  callbacks.current = { onBuilt };
  const logBox = useRef<HTMLPreElement>(null);

  // Another task: its own plan and log. A job of the task left behind is
  // stopped (the build belongs to the task being looked at).
  useEffect(() => {
    setPlan(null);
    setJob(null);
    setLines([]);
    setStart(null);
    cursor.current = 0;
    if (!bridge || !projectId || !chatId) return;
    let live = true;
    bridge.status({ projectId, chatId, cursor: 0 }).then(({ job: now }) => {
      if (!live) return;
      cursor.current = now.cursor;
      setLines(now.lines);
      setJob(now);
    }, () => undefined);
    return () => {
      live = false;
      if (jobRef.current?.state === "running") void bridge.cancel({ projectId, chatId }).catch(() => undefined);
    };
  }, [bridge, projectId, chatId]);

  // The plan, asked when the tab is shown, again after a job ends and when
  // the team stops working.
  useEffect(() => {
    if (!bridge || !projectId || !chatId || !active) return;
    let live = true;
    bridge.plan({ projectId, chatId }).then(result => { if (live) setPlan(result.plan); }, () => { if (live) setPlan(null); });
    return () => { live = false; };
  }, [bridge, projectId, chatId, planKey, teamWorking, active]);

  // While a job runs: its new lines and its end.
  const running = job?.state === "running";
  useEffect(() => {
    if (!running || !bridge || !projectId || !chatId) return;
    let live = true;
    const timer = window.setInterval(() => {
      bridge.status({ projectId, chatId, cursor: cursor.current }).then(({ job: now }) => {
        if (!live) return;
        const before = jobRef.current;
        cursor.current = now.cursor;
        if (now.lines.length) setLines(previous => appendBuildLines(previous, now.lines));
        setJob(now);
        const ended = previewBuildFinished(before, now);
        if (ended === "built") callbacks.current.onBuilt();
        if (ended) setPlanKey(value => value + 1);
      }, () => undefined);
    }, POLL_MS);
    return () => { live = false; window.clearInterval(timer); };
  }, [running, bridge, projectId, chatId]);

  // The log follows its newest line while open.
  useEffect(() => {
    const box = logBox.current;
    if (box && logOpen) box.scrollTop = box.scrollHeight;
  }, [lines, logOpen]);

  if (!bridge || !projectId || !chatId) return null;
  const bar = previewBuildBar(plan, job, teamWorking);
  if (bar.kind === "hidden") return null;
  if (bar.kind === "note") {
    return <p className="m5-preview-build-note" data-testid="preview-build-note" data-note={bar.note}>{t(NOTE_KEYS[bar.note])}</p>;
  }

  const run = (step: PreviewBuildStepV1) => {
    setStart(null);
    setAsking(true);
    bridge.start({ projectId, chatId, step }).then(result => {
      if (result.outcome === "started") {
        cursor.current = result.job.cursor;
        setLines([]);
        setJob(result.job);
        setLogOpen(true);
      } else {
        setStart(result.outcome);
      }
    }, () => setStart("refused")).finally(() => setAsking(false));
  };
  const stop = () => { void bridge.cancel({ projectId, chatId }).catch(() => undefined); };
  const status = previewBuildStatus(job, start);
  const built = job?.state === "succeeded" && job.step === "build" && job.outputReady;

  return <div className="m5-preview-build" data-testid="preview-build" data-state={job?.state ?? "idle"}>
    <div className="m5-preview-build-row">
      {bar.running ? <>
        <span className="m5-preview-build-running" role="status" aria-busy="true" data-testid="preview-build-running">
          {t(bar.running === "install" ? "m5.previewBuild.installing" : "m5.previewBuild.building")}
        </span>
        <button type="button" className="m5-secondary" data-testid="preview-build-cancel" onClick={stop}>{t("m5.previewBuild.cancel")}</button>
      </> : <>
        {bar.needsInstall && <button type="button" className="m5-secondary" data-testid="preview-build-install" disabled={!bar.canInstall || asking} title={bar.installCommand} onClick={() => run("install")}>{t("m5.previewBuild.install")}</button>}
        <button type="button" className={bar.needsInstall ? "m5-secondary" : "m5-btn-primary"} data-testid="preview-build-start" disabled={!bar.canBuild || asking} title={bar.buildCommand} onClick={() => run("build")}>
          {t(built ? "m5.previewBuild.rebuild" : "m5.previewBuild.build")}
        </button>
        {built && (showingBuild
          ? <button type="button" className="m5-link-button" data-testid="preview-show-source" onClick={onShowSource}>{t("m5.previewBuild.showSource")}</button>
          : <button type="button" className="m5-link-button" data-testid="preview-show-build" onClick={onShowBuild}>{t("m5.previewBuild.showBuild")}</button>)}
      </>}
      <button type="button" className="m5-link-button m5-preview-build-log-toggle" aria-expanded={logOpen} data-testid="preview-build-log-toggle" onClick={() => setLogOpen(open => !open)}>{t("m5.previewBuild.log")}</button>
    </div>
    {bar.teamWorking && !bar.running && <p className="m5-preview-build-help">{t("m5.previewBuild.teamWorking")}</p>}
    {!bar.teamWorking && !bar.running && bar.needsInstall && <p className="m5-preview-build-help">{t("m5.previewBuild.installHelp")}</p>}
    {status && <p className="m5-preview-build-status" role="status" data-testid="preview-build-status" data-status={status}>{t(STATUS_KEYS[status])}</p>}
    {logOpen && <pre ref={logBox} className="m5-preview-build-log" data-testid="preview-build-log" tabIndex={0} aria-label={t("m5.previewBuild.log")}>
      {lines.length ? lines.join("\n") : t("m5.previewBuild.logEmpty")}
    </pre>}
  </div>;
}
