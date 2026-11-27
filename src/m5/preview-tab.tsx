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
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { M5PreviewBridgeV1, PreviewCloseReasonV1, PreviewEmptyReasonV1, PreviewViewportV1 } from "./contracts/preview-v1.ts";
import { closedView, coveredAtPoints, coveredByRect, emptyView, overlaps, PREVIEW_WAIT_ATTRIBUTE, PreviewBands, previewBounds, sameLayout, samplePoints, type Rect } from "./preview-tab-model.ts";
import { PREVIEW_HOLD_LIMIT_MS, PreviewLayoutLoop, type LoopMeasure } from "./preview-layout-loop.ts";
import { PreviewBuildBar } from "./preview-build-bar.tsx";
import type { PreviewSourceV1 } from "./contracts/preview-v1.ts";

type Phase =
  | { kind: "idle" }
  | { kind: "off" }
  | { kind: "opening" }
  | { kind: "open"; previewId: string; entry: string; loaded: boolean }
  | { kind: "empty"; reason: PreviewEmptyReasonV1 }
  | { kind: "closed"; view: "stalled" | "workspaceGone" | "tooMuch" | "suspended" }
  | { kind: "failed" };

export interface PreviewEntryRequest { path: string; nonce: number }

/** A window being resized or zoomed: the view stays hidden until it settles. */
const SETTLE_MS = 150;
/** Elements watched by their rectangle (notification panels, toasts, popovers). */
const MAX_WATCHED = 300;

export function PreviewTab({ projectId, chatId, active, covered, resultKey, entryRequest, onChanges, teamWorking = false }: {
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
  /** The task's team is working (no build meanwhile). */
  teamWorking?: boolean;
}) {
  const { t } = useI18n();
  const bridge: M5PreviewBridgeV1 | undefined = window.m5Preview;
  const stage = useRef<HTMLDivElement>(null);
  const section = useRef<HTMLElement>(null);
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
  /** The live preview's layout order (hide, place, show), or null. */
  const loopRef = useRef<PreviewLayoutLoop | null>(null);
  /** The tab's bands and when they may change (`PreviewBands`). */
  const bandModel = useRef<PreviewBands | null>(null);
  bandModel.current ??= new PreviewBands({
    queue: apply => { const loop = loopRef.current; if (loop) loop.afterHidden(apply); else apply(); },
    render: next => { setNewResult(next.newResult); setNotice(next.notice); },
    settled: () => loopRef.current?.settled(),
  });
  const setBand = (which: "newResult" | "notice", value: boolean) => bandModel.current!.set(which, value);
  const hidden = useRef(false);
  const entry = useRef<string | null>(null);
  /** What the next open shows: the workspace's own files, or its last build. */
  const [source, setSource] = useState<PreviewSourceV1>("workspace");
  const sourceRef = useRef<PreviewSourceV1>("workspace");
  sourceRef.current = source;
  const shownResultKey = useRef(resultKey);
  const [overlay, setOverlay] = useState(false);
  /** Everything that decides "hidden", read at once by the layout loop
   * (never a render later): a hide must not wait. */
  const causes = useRef({ covered, active, overlay: false, resizing: false });
  causes.current.covered = covered;
  causes.current.active = active;
  const viewportNow = useRef(viewport);
  viewportNow.current = viewport;
  const syncNow = useRef<() => void>(() => undefined);

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
    sourceRef.current = "workspace";
    setSource("workspace");
    setOpenRequest(value => value + 1);
  }, [entryRequest]);

  // Another task: a new preview (a new token: storage starts empty).
  useEffect(() => {
    entry.current = null;
    sourceRef.current = "workspace";
    setSource("workspace");
    openGeneration.current++;
    setPhase({ kind: "idle" });
    setNewResult(false);
    setNotice(false);
    bandModel.current!.reset();
    shownResultKey.current = resultKey;
  }, [projectId, chatId]); // eslint-disable-line react-hooks/exhaustive-deps

  // Open when the tab is shown and nothing is open yet (or a page was named).
  // Main keeps a new view hidden until the first layout below says where it
  // goes and that nothing covers it (review I1); an answer for another task
  // or an older open is closed without ever being shown.
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
    hidden.current = false;
    const from = sourceRef.current;
    bridge.open({ projectId, chatId, entry: from === "build" ? null : entry.current, source: from, bounds, viewport, pixelRatio: window.devicePixelRatio, page: { width: window.innerWidth, height: window.innerHeight } }).then(result => {
      if (generation !== openGeneration.current) {
        if (result.outcome === "opened") void bridge.close({ previewId: result.previewId }).catch(() => undefined);
        return;
      }
      if (result.outcome === "empty") setPhase({ kind: "empty", reason: result.reason });
      else setPhase({ kind: "open", previewId: result.previewId, entry: result.entry, loaded: false });
    }, () => { if (generation === openGeneration.current) setPhase({ kind: "failed" }); });
  }, [wanted, projectId, chatId, openRequest]); // eslint-disable-line react-hooks/exhaustive-deps

  // Closing on the way out: another task, a failure, or the tab unmounted.
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
      else if (event.kind === "blockedNavigation") setBand("notice", true);
      // The entry page did not load: say so and offer to open it again
      // (the preview itself is closed by leaving the "open" phase).
      else if (event.kind === "loadFailed") { setStill(null); setPhase({ kind: "failed" }); }
      // "unresponsive" is followed by "closed" (main closes a hung page).
      else if (event.kind === "closed") onClosed(event.reason);
    });
  }, [bridge]); // eslint-disable-line react-hooks/exhaustive-deps
  const onClosed = (reason: PreviewCloseReasonV1) => {
    const view = closedView(reason);
    setStill(null);
    setPhase(view ? { kind: "closed", view } : { kind: "idle" });
    // Closed on request while still the current one: only the layout order
    // does that, detaching a preview whose hide main did not answer in time
    // (re-review I1). It opens again, hidden until nothing covers the stage.
    if (reason === "requested") setOpenRequest(value => value + 1);
  };

  // The app began to quit and may yet stay (an update whose preparation
  // failed, review I4): main refuses opens until it resumes, then the tab
  // opens the preview again by itself.
  const suspended = phase.kind === "closed" && phase.view === "suspended";
  useEffect(() => {
    if (!suspended || !bridge) return;
    let live = true;
    const timer = window.setInterval(() => {
      bridge.status().then(status => {
        if (!live || !status.enabled) return;
        live = false;
        window.clearInterval(timer);
        setPhase({ kind: "idle" });
        setOpenRequest(value => value + 1);
      }, () => undefined);
    }, 1000);
    return () => { live = false; window.clearInterval(timer); };
  }, [suspended, bridge]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setBand("notice", false), 6000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  // A run ended while the preview is open: offer a reload, never reload by itself.
  useEffect(() => {
    if (resultKey === shownResultKey.current) return;
    shownResultKey.current = resultKey;
    if (phase.kind === "open") setBand("newResult", true);
  }, [resultKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // The view on the stage: where it goes, and hidden whenever anything of
  // the app covers the stage (T8, T16). The order of hiding, placing and
  // showing is `PreviewLayoutLoop`'s (PR #9 re-review B1·I1).
  useEffect(() => {
    if (!previewId || !bridge || !stage.current) return;
    let live = true;
    let checkFrame = 0;
    let recheck = 0;
    let settle = 0;
    /** App elements that appeared or changed and may lie over the stage. */
    const watched = new Set<Element>();
    /** Where the stage was last seen: when it vanishes (another tab), what
     * takes its place is still held until the view is hidden. */
    let lastRect: Rect | null = null;
    const stageRect = (): Rect | null => {
      const element = stage.current;
      if (!element) return null;
      const rect = element.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) { lastRect = { left: rect.left, top: rect.top, width: rect.width, height: rect.height }; return rect; }
      return null;
    };
    /** The stage, what is in it and what holds it: none of these covers it. */
    const ownsStage = (node: unknown) => {
      const element = stage.current;
      return node instanceof Node && element !== null && (element.contains(node) || node.contains(element));
    };
    const topAt = (x: number, y: number) => document.elementFromPoint(x, y);
    const measure = (): LoopMeasure => {
      const rect = stageRect();
      const page = { width: window.innerWidth, height: window.innerHeight };
      return { bounds: rect ? previewBounds(rect, page) : null, pixelRatio: window.devicePixelRatio, page };
    };
    const loop = new PreviewLayoutLoop({
      measure,
      viewport: () => viewportNow.current,
      obscured: () => { const now = causes.current; return now.covered || now.overlay || !now.active || now.resizing; },
      send: layout => bridge.layout({ previewId, ...layout }).then(result => ({ closed: result.state === "closed", snapshot: result.snapshot })),
      close: () => bridge.close({ previewId }),
      still: image => { if (live) setStill(image); },
      // The covers let go are in sight now: the points see them before the view may show.
      released: () => { if (live) check(); },
      holdLimitMs: PREVIEW_HOLD_LIMIT_MS,
    });
    loopRef.current = loop;
    syncNow.current = () => loop.sync();

    // What is on top of the stage, asked of the page: a grid of points, and
    // the rectangle of every app element seen appearing or changing.
    const check = () => {
      checkFrame = 0;
      if (!live) return;
      const rect = stageRect();
      let on = false;
      let moving = false;
      if (rect) {
        on = coveredAtPoints(samplePoints(rect), topAt, ownsStage);
        for (const element of watched) {
          if (!element.isConnected) { watched.delete(element); continue; }
          if (!on && coveredByRect(rect, element.getBoundingClientRect(), topAt, node => node instanceof Node && element.contains(node))) on = true;
          if (!moving && typeof element.getAnimations === "function" && element.getAnimations({ subtree: true }).length > 0) moving = true;
        }
      }
      causes.current.overlay = on;
      setOverlay(on);
      loop.sync();
      window.clearTimeout(recheck);
      // Something on its way (a transition, an animation): look every frame.
      // A cover may also leave by a transition with no change in the page to
      // notice: while covered, look again shortly.
      if (moving) scheduleCheck();
      else if (on) recheck = window.setTimeout(scheduleCheck, 250);
    };
    const scheduleCheck = () => { if (!checkFrame) checkFrame = requestAnimationFrame(check); };

    // Held: invisible (with its ::backdrop), out of reach of focus and input
    // (`inert`), until main has confirmed the view hidden (review I1). Then
    // the points see it at once, before the loop may show the view again.
    const holdElement = (element: Element) => {
      if (element.hasAttribute(PREVIEW_WAIT_ATTRIBUTE)) return;
      const active = document.activeElement;
      const focused = active instanceof HTMLElement && element.contains(active) ? active : null;
      const wasInert = element.hasAttribute("inert");
      element.setAttribute(PREVIEW_WAIT_ATTRIBUTE, "");
      if (!wasInert) element.setAttribute("inert", "");
      loop.hold(() => {
        element.removeAttribute(PREVIEW_WAIT_ATTRIBUTE);
        if (!wasInert) { element.removeAttribute("inert"); ownInert.add(element); }
        if (focused?.isConnected && (document.activeElement === null || document.activeElement === document.body)) focused.focus();
      });
    };
    const holds = (element: Element, rect: Rect) => {
      if (element instanceof HTMLDialogElement && element.matches(":modal")) return true;
      if (overlaps(element.getBoundingClientRect(), rect)) return true;
      // A wrapper of no size may still hold what is drawn (a fixed panel).
      const inner = element.querySelectorAll("*");
      for (let index = 0; index < inner.length && index < 50; index++) if (overlaps(inner[index]!.getBoundingClientRect(), rect)) return true;
      return false;
    };
    /** Drawn out of the flow, so it may move over the stage later (a panel sliding in). */
    const floats = (element: Element) => /^(?:fixed|absolute|sticky)$/.test(getComputedStyle(element).position);
    /** The stage is no longer where main last placed the view. */
    const stageMoved = () => {
      const shown = loop.accepted;
      const now = measure().bounds;
      return shown !== null && !shown.obscured && (now === null || !sameLayout(shown, { ...shown, bounds: now }));
    };
    // Runs before the page paints (review I1). A new or changed app element
    // over the stage is held, main hides the view, and only then is it
    // shown — the view never draws over it, not even for a frame. When the
    // stage itself moved (something in the tab pushed it, re-review B1),
    // what changed in the tab is held the same way and the view is placed
    // again only after it was hidden.
    /** Elements whose `inert` the tab itself just took away again: that change is not a new cover. */
    const ownInert = new WeakSet<Element>();
    const observer = new MutationObserver(records => {
      if (!live) return;
      records = records.filter(record => {
        if (record.type !== "attributes" || record.attributeName !== "inert" || !ownInert.has(record.target as Element)) return true;
        ownInert.delete(record.target as Element);
        return false;
      });
      const rect = stageRect() ?? lastRect;
      const own = section.current;
      const mayBeShown = loop.mayBeShown;
      const moved = mayBeShown && stageMoved();
      const seen = new Set<Element>();
      let movedHeld = false;
      for (const record of records) {
        const targets = record.type === "childList" ? [...record.addedNodes] : [record.target];
        for (const node of targets) {
          if (!(node instanceof Element) || seen.has(node) || node.hasAttribute(PREVIEW_WAIT_ATTRIBUTE)) continue;
          seen.add(node);
          if (stage.current?.contains(node) || node.contains(stage.current)) continue;
          if (own?.contains(node)) {
            if (moved) { holdElement(node); movedHeld = true; }
            continue;
          }
          const over = rect !== null && holds(node, rect);
          if (watched.size < MAX_WATCHED && (over || floats(node))) watched.add(node);
          if (mayBeShown && over) holdElement(node);
        }
      }
      // Moved by something outside the tab (the panel's width): hidden until it settles.
      if (moved && !movedHeld) onResize();
      scheduleCheck();
    });
    observer.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["open", "hidden", "class", "style", "aria-expanded", "inert"] });

    // Resized or zoomed (a zoom changes devicePixelRatio): main has already
    // hidden the view (it hears of the change first); hidden here too until
    // it settles, then placed by a fresh measurement (review B1).
    const onResize = () => {
      causes.current.resizing = true;
      loop.sync();
      window.clearTimeout(settle);
      settle = window.setTimeout(() => { causes.current.resizing = false; scheduleCheck(); }, SETTLE_MS);
    };
    let ratio: MediaQueryList | null = null;
    const onRatio = () => { onResize(); watchRatio(); };
    const watchRatio = () => {
      ratio?.removeEventListener("change", onRatio);
      ratio = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      ratio.addEventListener("change", onRatio);
    };
    watchRatio();
    const onScroll = () => { if (stageMoved()) onResize(); scheduleCheck(); };
    // A stage that changed size or place (after layout, before paint): hidden first.
    const resize = new ResizeObserver(() => { if (stageMoved()) onResize(); else loop.sync(); });
    resize.observe(stage.current);
    window.addEventListener("resize", onResize);
    document.addEventListener("scroll", onScroll, true);
    for (const name of ["toggle", "transitionend", "animationend", "transitionstart"]) document.addEventListener(name, scheduleCheck, true);
    check();
    return () => {
      live = false;
      observer.disconnect();
      resize.disconnect();
      ratio?.removeEventListener("change", onRatio);
      window.removeEventListener("resize", onResize);
      document.removeEventListener("scroll", onScroll, true);
      for (const name of ["toggle", "transitionend", "animationend", "transitionstart"]) document.removeEventListener(name, scheduleCheck, true);
      if (checkFrame) cancelAnimationFrame(checkFrame);
      for (const timer of [recheck, settle]) window.clearTimeout(timer);
      // Anything still held is let go only once main has detached the preview.
      loop.dispose();
      if (loopRef.current === loop) loopRef.current = null;
      causes.current.overlay = false;
      causes.current.resizing = false;
      syncNow.current = () => undefined;
      setOverlay(false);
    };
  }, [previewId, bridge]);

  // An explicit cover, another tab or width: told to main before the page paints.
  useLayoutEffect(() => { syncNow.current(); }, [covered, active, viewport]);
  // A band that moved the stage is laid out: place the view again.
  useLayoutEffect(() => { bandModel.current!.rendered({ newResult, notice }); }, [newResult, notice]);

  // Another tab: hidden now and closed after a while by main.
  useEffect(() => {
    if (!previewId || !bridge) return;
    if (!active && !hidden.current) { hidden.current = true; void bridge.hide({ previewId }).catch(() => undefined); }
    if (active) hidden.current = false;
  }, [active, previewId, bridge]);

  const reload = () => {
    setBand("newResult", false);
    if (previewId && bridge) void bridge.reload({ previewId }).catch(() => undefined);
  };
  const reopen = () => {
    if (previewId && bridge) void bridge.close({ previewId }).catch(() => undefined);
    setPhase({ kind: "idle" });
    setOpenRequest(value => value + 1);
  };
  /** Show the workspace's own files or its last build (a new preview, a new token). */
  const showSource = (next: PreviewSourceV1) => {
    setBand("newResult", false);
    sourceRef.current = next;
    setSource(next);
    reopen();
  };

  // What the tab shows as hidden (the layout loop above reads the same causes).
  const obscured = covered || overlay || !active;

  const openPhase = phase.kind === "open" || phase.kind === "opening";
  return <section ref={section} className="m5-preview" data-testid="preview-tab" data-phase={phase.kind} data-obscured={openPhase && obscured ? "true" : "false"} data-overlay={overlay ? "true" : "false"} data-active={active ? "true" : "false"} data-covered={covered ? "true" : "false"} aria-label={t("m5.preview.title")}>
    <header className="m5-preview-frame" data-testid="preview-frame">
      <div className="m5-preview-badges">
        <strong>{t("m5.preview.frameTitle")}</strong>
        <span className="m5-preview-badge">{t("m5.preview.readOnly")}</span>
        <span className="m5-preview-badge">{t("m5.preview.offline")}</span>
      </div>
      {phase.kind === "open" && <div className="m5-preview-controls">
        {source === "build" && <span className="m5-preview-badge" data-testid="preview-build-badge">{t("m5.preview.buildBadge")}</span>}
        <span className="m5-preview-entry" title={phase.entry}>{phase.entry}</span>
        <div className="m5-preview-viewport" role="group" aria-label={t("m5.preview.width")}>
          <button type="button" aria-pressed={viewport === "wide"} data-testid="preview-wide" onClick={() => setViewport("wide")}>{t("m5.preview.wide")}</button>
          <button type="button" aria-pressed={viewport === "narrow"} data-testid="preview-narrow" onClick={() => setViewport("narrow")}>{t("m5.preview.narrow")}</button>
        </div>
        <button type="button" className="m5-secondary" data-testid="preview-reload" onClick={reload}>{t("m5.preview.reload")}</button>
      </div>}
    </header>
    {enabled === true && <PreviewBuildBar projectId={projectId} chatId={chatId} active={active} teamWorking={teamWorking} showingBuild={source === "build"}
      onBuilt={() => showSource("build")} onShowBuild={() => showSource("build")} onShowSource={() => showSource("workspace")} />}
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
        <strong>{phase.reason === "noBuild" ? t("m5.preview.noBuild") : emptyView(phase.reason) === "noPage" ? t("m5.preview.noPage") : t("m5.preview.notOpenable")}</strong>
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
      {phase.kind === "closed" && phase.view === "suspended" && <div className="m5-preview-state" data-testid="preview-suspended">
        <strong>{t("m5.preview.suspended")}</strong>
        <p>{t("m5.preview.suspendedHelp")}</p>
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
