import { useI18n } from "../i18n.tsx";
import { useEffect, useId, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { defaultDockLayout, dockGeometry, PANEL_IDS, resizeDockSplit, selectDockTab, type DockLayout, type DockPanel, type DockRect } from "./dock-layout.ts";
const position = (rect: DockRect): CSSProperties => ({ left: `${rect.x * 100}%`, top: `${rect.y * 100}%`, width: `${rect.width * 100}%`, height: `${rect.height * 100}%` });

export function WorkbenchDock({ children, conversation, chats, reveal, changeCount }: { children: ReactNode[]; conversation?: ReactNode; chats?: ReactNode; reveal?: { panel: DockPanel } | null; changeCount?: number }) {
  const { t } = useI18n();
  const labels: Record<DockPanel, string> = { files: t("m5.dock.files"), editor: t("m5.dock.editor"), changes: t("m5.dock.changes"), conversation: t("m5.dock.conversation"), chats: t("m5.dock.chats") };
  const [layout, setLayout] = useState(() => selectDockTab(defaultDockLayout(), "files"));
  const [narrow, setNarrow] = useState(() => window.matchMedia("(max-width: 820px)").matches);
  const [narrowPanel, setNarrowPanel] = useState<DockPanel>(reveal?.panel ?? "chats");
  const container = useRef<HTMLElement>(null);
  const tabId = useId();
  const activateTab = (target: DockPanel) => {
    setNarrowPanel(target);
    if (!narrow) setLayout(current => selectDockTab(current, target));
    requestAnimationFrame(() => document.getElementById(`${tabId}-${target}-tab-${target}`)?.focus());
  };
  const resizing = useRef<{ coordinate: number; size: number; ratio: number; path: string; axis: string; layout: DockLayout } | null>(null);
  useEffect(() => {
    const query = window.matchMedia("(max-width: 820px)");
    const update = () => { resizing.current = null; setNarrow(query.matches); };
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  // Resizing alone must not replay an old reveal into the saved wide layout.
  useEffect(() => { if (reveal) { resizing.current = null; setNarrowPanel(reveal.panel); if (!narrow) setLayout(current => selectDockTab(current, reveal.panel)); } }, [reveal]);
  const panels: Record<DockPanel, ReactNode> = { files: children[0], editor: children[1], changes: children[2], conversation, chats };
  const available = PANEL_IDS.filter(id => Boolean(panels[id]));
  const displayLayout: DockLayout = narrow ? { ...layout, hidden: [], hiddenBeforeFocus: undefined, tree: { type: "tabs", panels: available, active: narrowPanel } } : layout;
  const { groups, separators } = dockGeometry(displayLayout, available);
  return <>
    {narrow && <p className="m5-dock-narrow-note">{t("m5.dock.narrow")}</p>}
    <section ref={container} className="m5-panels m5-dock m5-dock-tree" aria-label={t("m5.dock.panels")}>
      {/* Stable panel parents preserve Monaco and unsaved drafts across tab changes. */}
      {available.map(id => {
        const group = groups.find(group => group.panels.includes(id));
        const shown = group?.active === id;
        return <div className="m5-dock-slot" key={id} data-panel={id} hidden={!shown} style={shown && group ? position(group.rect) : { display: "none" }}>
          <div className="m5-dock-frame">
          <div className="m5-dock-heading">
            <div className="m5-dock-group-tabs" role="tablist" aria-label={t("m5.dock.group", { panel: labels[id] })}>
              {group?.panels.map((tab, index) => <button key={tab} id={`${tabId}-${id}-tab-${tab}`} role="tab" type="button" aria-selected={tab === id} tabIndex={tab === id ? 0 : -1} aria-controls={`${tabId}-content-${tab}`}
                onClick={() => activateTab(tab)} onKeyDown={event => {
                  const next = event.key === "Home" ? 0 : event.key === "End" ? group.panels.length - 1 : event.key === "ArrowRight" ? (index + 1) % group.panels.length : event.key === "ArrowLeft" ? (index + group.panels.length - 1) % group.panels.length : -1;
                  if (next < 0) return; event.preventDefault(); const target = group.panels[next];
                  activateTab(target);
                }} aria-label={t("m5.dock.tabLabel", { panel: tab === "changes" && changeCount !== undefined ? `${labels[tab]} (${changeCount})` : labels[tab] })}>{labels[tab]}{tab === "changes" && changeCount !== undefined && <span className="m5-dock-count" aria-hidden="true">{changeCount}</span>}</button>)}
            </div>

          </div>
          <div className="m5-dock-content" id={`${tabId}-content-${id}`} role="tabpanel" aria-labelledby={`${tabId}-${id}-tab-${id}`}>{panels[id]}</div>
          </div>
        </div>;
      })}
      {separators.map(separator => <div key={separator.path} className={`m5-dock-separator m5-dock-split-${separator.axis}`} style={position(separator.rect)} role="separator" tabIndex={0} aria-label={t(separator.axis === "horizontal" ? "m5.dock.resizeHorizontal" : "m5.dock.resizeVertical")} aria-orientation={separator.axis === "horizontal" ? "vertical" : "horizontal"} aria-valuenow={Math.round(separator.ratio * 100)} aria-valuemin={10} aria-valuemax={90}
        onPointerDown={event => {
          if (event.button !== 0) return;
          const bounds = container.current?.getBoundingClientRect(); if (!bounds) return;
          const horizontal = separator.axis === "horizontal";
          const size = horizontal ? bounds.width * separator.parent.width : bounds.height * separator.parent.height;
          if (size <= 0) return;
          resizing.current = { coordinate: horizontal ? event.clientX : event.clientY, size, ratio: separator.ratio, path: separator.path, axis: separator.axis, layout };
          event.currentTarget.setPointerCapture(event.pointerId); event.preventDefault();
        }} onPointerMove={event => {
          const start = resizing.current;
          if (start) setLayout(resizeDockSplit(start.layout, start.path, start.ratio + ((start.axis === "horizontal" ? event.clientX : event.clientY) - start.coordinate) / start.size));
        }} onPointerUp={() => { resizing.current = null; }} onPointerCancel={() => { resizing.current = null; }} onLostPointerCapture={() => { resizing.current = null; }} onKeyDown={event => {
          const decrease = separator.axis === "horizontal" ? "ArrowLeft" : "ArrowUp", increase = separator.axis === "horizontal" ? "ArrowRight" : "ArrowDown";
          const ratio = event.key === "Home" ? .1 : event.key === "End" ? .9 : event.key === decrease ? separator.ratio - .02 : event.key === increase ? separator.ratio + .02 : undefined;
          if (ratio !== undefined) { event.preventDefault(); setLayout(current => resizeDockSplit(current, separator.path, ratio)); }
        }} />)}
    </section>
  </>;
}
