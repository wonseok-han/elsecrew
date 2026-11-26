/**
 * The result preview tab (ADE 1차-나 P3; docs/proposals/m5-ade-layout-plan.md
 * 1.2 "오른쪽", 2.3 "화면 겹침", 3.2 P3).
 *
 * The page the team made is shown by main in an isolated view drawn ABOVE
 * this page (`electron/preview-manager.ts`). This component only owns the
 * place it goes (the stage), the app's own frame around it, and the words
 * for every state. Two screen-side threats are handled here:
 * - T8 (a page dressing up as the app): the frame — "미리보기 — 팀이 만든
 *   결과", "읽기 전용", "인터넷 차단" — is outside the stage and the view is
 *   never larger than the stage (`previewBounds`), so no page can cover it.
 * - T16 (the view drawn over the app's own dialogs): whenever anything of
 *   the app covers the stage — the explicit covers from the layout, or
 *   anything the page finds on top of the stage at a grid of points — the
 *   view is hidden at once and a still image takes its place.
 */
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { M5PreviewBridgeV1, PreviewCloseReasonV1, PreviewEmptyReasonV1, PreviewViewportV1 } from "./contracts/preview-v1.ts";
import { closedView, coveredAtPoints, emptyView, previewBounds, sameLayout, samplePoints, type LayoutState } from "./preview-tab-model.ts";

type Phase =
  | { kind: "idle" }
  | { kind: "off" }
  | { kind: "opening" }
  | { kind: "open"; previewId: string; entry: string; loaded: boolean }
  | { kind: "empty"; reason: PreviewEmptyReasonV1 }
  | { kind: "closed"; view: "stalled" | "workspaceGone" | "tooMuch" }
  | { kind: "failed" };

export interface PreviewEntryRequest { path: string; nonce: number }

export function PreviewTab({ projectId, chatId, active, covered, resultKey, entryRequest, onChanges }: {
  projectId: string | null;
  chatId: string | null;
  /** The preview tab is the selected one and on screen. */
  active: boolean;
  /** The layout's explicit covers (task-layout-model `previewObscured`). */
  covered: boolean;
  /** Changes when a run of the task ends: the page may have new results. */
  resultKey: string;
  /** "미리보기로 열기" from the files tab. */
  entryRequest: PreviewEntryRequest | null;
  onChanges: () => void;
}) {
  const { t } = useI18n();
  const bridge: M5PreviewBridgeV1 | undefined = window.m5Preview;
  const stage = useRef<HTMLDivElement>(null);
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [viewport, setViewport] = useState<PreviewViewportV1>("wide");
  const [still, setStill] = useState<string | null>(null);
  const [newResult, setNewResult] = useState(false);
  const [notice, setNotice] = useState(false);
  const [enabled, setEnabled] = useState<boolean | null>(null);
  /** Bumped to ask for a (new) open: "다시 열기", or a page named in the files tab. */
  const [openRequest, setOpenRequest] = useState(0);
  /** Each open's number: an answer for an older one is closed, never shown. */
  const openGeneration = useRef(0);
  const phaseRef = useRef<Phase>({ kind: "idle" });
  phaseRef.current = phase;
  const previewId = phase.kind === "open" ? phase.previewId : null;
  const previewRef = useRef<string | null>(null);
  previewRef.current = previewId;
  const lastLayout = useRef<LayoutState | null>(null);
  const hidden = useRef(false);
  const entry = useRef<string | null>(null);
  const shownResultKey = useRef(resultKey);
  const [overlay, setOverlay] = useState(false);

  // Is the preview available at all (`ELSECREW_PREVIEW`)?
  useEffect(() => {
    let live = true;
    if (!bridge) { setEnabled(false); return; }
    bridge.status().then(status => { if (live) setEnabled(status.enabled); }, () => { if (live) setEnabled(false); });
    return () => { live = false; };
  }, [bridge]);

  // A named page from the files tab: the next open uses it.
  useEffect(() => {
    if (!entryRequest) return;
    entry.current = entryRequest.path;
    setOpenRequest(value => value + 1);
  }, [entryRequest]);

  // Another task: a new preview (a new token: storage starts empty).
  useEffect(() => {
    entry.current = null;
    openGeneration.current++;
    setPhase({ kind: "idle" });
    setNewResult(false);
    shownResultKey.current = resultKey;
  }, [projectId, chatId]); // eslint-disable-line react-hooks/exhaustive-deps

  // Open when the tab is shown and nothing is open yet (or a page was named).
  const wanted = active && enabled === true && Boolean(projectId && chatId);
  const handledRequest = useRef(0);
  useEffect(() => {
    if (!wanted || !bridge || !projectId || !chatId || !stage.current) return;
    const asked = openRequest !== handledRequest.current;
    if (phaseRef.current.kind !== "idle" && !asked) return;
    const bounds = previewBounds(stage.current.getBoundingClientRect(), { width: window.innerWidth, height: window.innerHeight });
    if (!bounds) return;
    handledRequest.current = openRequest;
    const generation = ++openGeneration.current;
    setPhase({ kind: "opening" });
    setStill(null);
    lastLayout.current = null;
    hidden.current = false;
    bridge.open({ projectId, chatId, entry: entry.current, bounds, viewport }).then(result => {
      if (generation !== openGeneration.current) {
        if (result.outcome === "opened") void bridge.close({ previewId: result.previewId }).catch(() => undefined);
        return;
      }
      if (result.outcome === "empty") setPhase({ kind: "empty", reason: result.reason });
      else setPhase({ kind: "open", previewId: result.previewId, entry: result.entry, loaded: false });
    }, () => { if (generation === openGeneration.current) setPhase({ kind: "failed" }); });
  }, [wanted, projectId, chatId, openRequest]); // eslint-disable-line react-hooks/exhaustive-deps

  // Closing on the way out: another task, or the tab unmounted.
  useEffect(() => {
    if (!previewId || !bridge) return;
    return () => { void bridge.close({ previewId }).catch(() => undefined); };
  }, [previewId, bridge]);

  // What main says about this preview.
  useEffect(() => {
    if (!bridge) return;
    return bridge.onEvent(event => {
      if (event.previewId !== previewRef.current) return;
      if (event.kind === "loaded") setPhase(current => current.kind === "open" ? { ...current, loaded: true } : current);
      else if (event.kind === "blockedNavigation") setNotice(true);
      // "unresponsive" is followed by "closed" (main closes a hung page).
      else if (event.kind === "closed") onClosed(event.reason);
    });
  }, [bridge]); // eslint-disable-line react-hooks/exhaustive-deps
  const onClosed = (reason: PreviewCloseReasonV1) => {
    const view = closedView(reason);
    setStill(null);
    setPhase(view ? { kind: "closed", view } : { kind: "idle" });
  };

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(false), 6000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  // A run ended while the preview is open: offer a reload, never reload by itself.
  useEffect(() => {
    if (resultKey === shownResultKey.current) return;
    shownResultKey.current = resultKey;
    if (phase.kind === "open") setNewResult(true);
  }, [resultKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Anything of the app drawn over the stage (T16), asked of the page itself.
  useEffect(() => {
    if (!previewId) return;
    let frame = 0;
    const check = () => {
      frame = 0;
      const element = stage.current;
      if (!element) return;
      const rect = element.getBoundingClientRect();
      const on = rect.width > 0 && rect.height > 0 && coveredAtPoints(samplePoints(rect), (x, y) => document.elementFromPoint(x, y), node => node instanceof Node && element.contains(node));
      setOverlay(on);
      // A cover may leave by a transition, with no change in the page to
      // notice: while covered, look again shortly.
      window.clearTimeout(recheck);
      if (on) recheck = window.setTimeout(schedule, 250);
    };
    let recheck = 0;
    const schedule = () => { if (!frame) frame = requestAnimationFrame(check); };
    const observer = new MutationObserver(schedule);
    observer.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["open", "hidden", "class", "style", "aria-expanded"] });
    window.addEventListener("resize", schedule);
    document.addEventListener("scroll", schedule, true);
    document.addEventListener("toggle", schedule, true);
    document.addEventListener("transitionend", schedule, true);
    document.addEventListener("animationend", schedule, true);
    schedule();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", schedule);
      for (const name of ["scroll", "toggle", "transitionend", "animationend"]) document.removeEventListener(name, schedule, true);
      if (frame) cancelAnimationFrame(frame);
      window.clearTimeout(recheck);
    };
  }, [previewId]);

  // Keep the view on the stage: its place, its width, hidden when covered.
  const obscured = covered || overlay || !active;
  useEffect(() => {
    if (!previewId || !bridge || !stage.current) return;
    let frame = 0;
    const sync = () => {
      frame = 0;
      const element = stage.current;
      if (!element) return;
      const bounds = previewBounds(element.getBoundingClientRect(), { width: window.innerWidth, height: window.innerHeight }) ?? lastLayout.current?.bounds ?? null;
      if (!bounds) return;
      const next: LayoutState = { bounds, viewport, obscured: obscured || !previewBounds(element.getBoundingClientRect(), { width: window.innerWidth, height: window.innerHeight }) };
      if (sameLayout(lastLayout.current, next)) return;
      const wasObscured = lastLayout.current?.obscured ?? false;
      lastLayout.current = next;
      void bridge.layout({ previewId, ...next }).then(result => {
        if (previewRef.current !== previewId) return;
        if (result.state === "closed") return;
        if (next.obscured && !wasObscured && result.snapshot) setStill(result.snapshot);
        if (!next.obscured) setStill(null);
      }, () => undefined);
    };
    const schedule = () => { if (!frame) frame = requestAnimationFrame(sync); };
    const resize = new ResizeObserver(schedule);
    resize.observe(stage.current);
    window.addEventListener("resize", schedule);
    document.addEventListener("scroll", schedule, true);
    sync();
    return () => { resize.disconnect(); window.removeEventListener("resize", schedule); document.removeEventListener("scroll", schedule, true); if (frame) cancelAnimationFrame(frame); };
  }, [previewId, bridge, viewport, obscured]);

  // Another tab: hidden now and closed after a while by main.
  useEffect(() => {
    if (!previewId || !bridge) return;
    if (!active && !hidden.current) { hidden.current = true; void bridge.hide({ previewId }).catch(() => undefined); }
    if (active) hidden.current = false;
  }, [active, previewId, bridge]);

  const reload = () => {
    setNewResult(false);
    if (previewId && bridge) void bridge.reload({ previewId }).catch(() => undefined);
  };
  const reopen = () => {
    if (previewId && bridge) void bridge.close({ previewId }).catch(() => undefined);
    setPhase({ kind: "idle" });
    setOpenRequest(value => value + 1);
  };

  const openPhase = phase.kind === "open" || phase.kind === "opening";
  return <section className="m5-preview" data-testid="preview-tab" data-phase={phase.kind} data-obscured={openPhase && obscured ? "true" : "false"} data-overlay={overlay ? "true" : "false"} data-active={active ? "true" : "false"} data-covered={covered ? "true" : "false"} aria-label={t("m5.preview.title")}>
    <header className="m5-preview-frame" data-testid="preview-frame">
      <div className="m5-preview-badges">
        <strong>{t("m5.preview.frameTitle")}</strong>
        <span className="m5-preview-badge">{t("m5.preview.readOnly")}</span>
        <span className="m5-preview-badge">{t("m5.preview.offline")}</span>
      </div>
      {phase.kind === "open" && <div className="m5-preview-controls">
        <span className="m5-preview-entry" title={phase.entry}>{phase.entry}</span>
        <div className="m5-preview-viewport" role="group" aria-label={t("m5.preview.width")}>
          <button type="button" aria-pressed={viewport === "wide"} data-testid="preview-wide" onClick={() => setViewport("wide")}>{t("m5.preview.wide")}</button>
          <button type="button" aria-pressed={viewport === "narrow"} data-testid="preview-narrow" onClick={() => setViewport("narrow")}>{t("m5.preview.narrow")}</button>
        </div>
        <button type="button" className="m5-secondary" data-testid="preview-reload" onClick={reload}>{t("m5.preview.reload")}</button>
      </div>}
    </header>
    {newResult && phase.kind === "open" && <p className="m5-preview-band" role="status" data-testid="preview-new-result">
      {t("m5.preview.newResult")} <button type="button" className="m5-link-button" onClick={reload}>{t("m5.preview.reload")}</button>
    </p>}
    {notice && <p className="m5-preview-band" role="status" data-testid="preview-blocked">{t("m5.preview.blockedLink")}</p>}
    <div ref={stage} className="m5-preview-stage" data-testid="preview-stage" data-viewport={viewport}>
      {still && openPhase && obscured && <img className="m5-preview-still" src={still} alt={t("m5.preview.still")} />}
      {phase.kind === "opening" && <p className="m5-preview-state" aria-busy="true">{t("m5.preview.opening")}</p>}
      {enabled === false && <div className="m5-preview-state" data-testid="preview-off"><strong>{t("m5.preview.off")}</strong><p>{t("m5.preview.offHelp")}</p></div>}
      {!chatId && enabled !== false && <div className="m5-preview-state"><p>{t("m5.preview.noTask")}</p></div>}
      {phase.kind === "empty" && <div className="m5-preview-state" data-testid="preview-empty">
        <strong>{emptyView(phase.reason) === "noPage" ? t("m5.preview.noPage") : t("m5.preview.notOpenable")}</strong>
        <p>{t("m5.preview.noPageHelp")}</p>
        <button type="button" className="m5-secondary" onClick={onChanges}>{t("m5.preview.toChanges")}</button>
      </div>}
      {phase.kind === "failed" && <div className="m5-preview-state" role="alert" data-testid="preview-failed">
        <strong>{t("m5.preview.failed")}</strong>
        <button type="button" className="m5-secondary" onClick={reopen}>{t("m5.preview.reopen")}</button>
      </div>}
      {phase.kind === "closed" && phase.view === "stalled" && <div className="m5-preview-state" role="alert" data-testid="preview-stalled">
        <strong>{t("m5.preview.stalled")}</strong>
        <p>{t("m5.preview.stalledHelp")}</p>
        <button type="button" className="m5-secondary" onClick={reopen}>{t("m5.preview.reopen")}</button>
      </div>}
      {phase.kind === "closed" && phase.view === "workspaceGone" && <div className="m5-preview-state" data-testid="preview-gone"><strong>{t("m5.preview.workspaceGone")}</strong></div>}
      {phase.kind === "closed" && phase.view === "tooMuch" && <div className="m5-preview-state" data-testid="preview-too-much">
        <strong>{t("m5.preview.tooMuch")}</strong>
        <button type="button" className="m5-secondary" onClick={reopen}>{t("m5.preview.reopen")}</button>
      </div>}
    </div>
    <p className="m5-preview-note">{t("m5.preview.storageNote")}</p>
  </section>;
}
