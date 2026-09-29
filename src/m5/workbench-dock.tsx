import { useEffect, useId, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { defaultDockLayout, DOCK_STORAGE_KEY, dockGeometry, focusDockPanel, PANEL_IDS, parseDockLayout, placeDockPanel, resizeDockSplit, revealDockPanel, selectDockTab, toggleDockPanel, visibleDockPanels, type DockLayout, type DockPanel, type DockPlacement, type DockRect } from "./dock-layout.ts";
const labels: Record<DockPanel, string> = { files: "파일", editor: "편집기", changes: "변경", conversation: "팀 대화" };
const placements: Record<DockPlacement, string> = { left: "왼쪽", right: "오른쪽", top: "위", bottom: "아래", tab: "같은 탭 그룹" };
const dragType = "application/x-hello-agent-panel";
const position = (rect: DockRect): CSSProperties => ({ left: `${rect.x * 100}%`, top: `${rect.y * 100}%`, width: `${rect.width * 100}%`, height: `${rect.height * 100}%` });

export function WorkbenchDock({ children, conversation, reveal }: { children: ReactNode[]; conversation?: ReactNode; reveal?: { panel: DockPanel } | null }) {
  const [layout, setLayout] = useState(() => { try { return parseDockLayout(localStorage.getItem(DOCK_STORAGE_KEY)); } catch { return defaultDockLayout(); } });
  const [drop, setDrop] = useState<{ target: DockPanel; placement: DockPlacement } | null>(null);
  const container = useRef<HTMLElement>(null);
  const tabId = useId();
  const activateTab = (target: DockPanel) => {
    setLayout(current => selectDockTab(current, target));
    requestAnimationFrame(() => document.getElementById(`${tabId}-${target}-tab-${target}`)?.focus());
  };
  const resizing = useRef<{ coordinate: number; size: number; ratio: number; path: string; axis: string; layout: DockLayout } | null>(null);
  useEffect(() => { try { localStorage.setItem(DOCK_STORAGE_KEY, JSON.stringify(layout)); } catch { /* In-memory layout remains usable. */ } }, [layout]);
  useEffect(() => { if (reveal) { resizing.current = null; setLayout(current => revealDockPanel(current, reveal.panel)); } }, [reveal]);
  const panels: Record<DockPanel, ReactNode> = { files: children[0], editor: children[1], changes: children[2], conversation };
  const available = PANEL_IDS.filter(id => Boolean(panels[id]));
  const eligible = visibleDockPanels(layout, available);
  const { groups, separators } = dockGeometry(layout, available);
  const preset = (kind: "horizontal" | "vertical" | "tabs") => { resizing.current = null; setLayout(current => ({ ...current, tree: undefined, tabbed: kind === "tabs", direction: kind === "vertical" ? "vertical" : "horizontal" })); };
  const move = (source: DockPanel, target: DockPanel, placement: DockPlacement) => { resizing.current = null; setDrop(null); setLayout(current => placeDockPanel(current, source, target, placement)); };
  return <>
    <div className="m5-dock-help"><span>제목을 가장자리로 끌면 분할하고, 가운데로 끌면 탭으로 묶어요.</span><button type="button" onClick={() => { resizing.current = null; setLayout(defaultDockLayout()); }}>기본 배치로</button></div>
    <nav className="m5-dock-visibility" aria-label="패널 표시">
      <button type="button" aria-pressed={!layout.tree && !layout.tabbed && layout.direction !== "vertical"} onClick={() => preset("horizontal")}>가로 배치</button>
      <button type="button" aria-pressed={!layout.tree && !layout.tabbed && layout.direction === "vertical"} onClick={() => preset("vertical")}>세로 배치</button>
      <button type="button" aria-pressed={!layout.tree && layout.tabbed === true} onClick={() => preset("tabs")}>탭 배치</button>
      {available.map(id => <button key={id} type="button" aria-pressed={eligible.includes(id)} disabled={eligible.length === 1 && eligible.includes(id)} onClick={() => { resizing.current = null; setLayout(current => toggleDockPanel(current, id, available)); }}>{labels[id]}</button>)}
    </nav>
    <section ref={container} className="m5-panels m5-dock m5-dock-tree" aria-label="재배치 가능한 작업 패널" onDragEnd={() => setDrop(null)}>
      {/* Flat stable parents preserve Monaco and unsaved editor state during moves. */}
      {available.map(id => {
        const group = groups.find(group => group.panels.includes(id));
        const shown = group?.active === id;
        return <div className="m5-dock-slot" key={id} data-panel={id} hidden={!shown} style={shown && group ? position(group.rect) : { display: "none" }}
          onDragOver={event => {
            if (!event.dataTransfer.types.includes(dragType)) return;
            event.preventDefault(); event.dataTransfer.dropEffect = "move";
            const bounds = event.currentTarget.getBoundingClientRect();
            const x = (event.clientX - bounds.left) / bounds.width, y = (event.clientY - bounds.top) / bounds.height;
            const placement: DockPlacement = x < .23 ? "left" : x > .77 ? "right" : y < .23 ? "top" : y > .77 ? "bottom" : "tab";
            setDrop({ target: id, placement });
          }} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDrop(null); }} onDrop={event => {
            const source = event.dataTransfer.getData(dragType) as DockPanel; event.preventDefault();
            if (available.includes(source) && drop?.target === id) move(source, id, drop.placement);
            setDrop(null);
          }}>
          <div className="m5-dock-heading">
            <div className="m5-dock-group-tabs" role="tablist" aria-label={`${labels[id]} 그룹 패널`}>
              {group?.panels.map((tab, index) => <button key={tab} id={`${tabId}-${id}-tab-${tab}`} role="tab" type="button" aria-selected={tab === id} tabIndex={tab === id ? 0 : -1} aria-controls={`${tabId}-content-${tab}`}
                draggable onDragStart={event => { event.dataTransfer.setData(dragType, tab); event.dataTransfer.effectAllowed = "move"; }}
                onClick={() => activateTab(tab)} onKeyDown={event => {
                  const next = event.key === "Home" ? 0 : event.key === "End" ? group.panels.length - 1 : event.key === "ArrowRight" ? (index + 1) % group.panels.length : event.key === "ArrowLeft" ? (index + group.panels.length - 1) % group.panels.length : -1;
                  if (next < 0) return; event.preventDefault(); const target = group.panels[next];
                  activateTab(target);
                }} aria-label={`${labels[tab]} 패널 끌기`}>{labels[tab]}</button>)}
            </div>
            <select aria-label={`${labels[id]} 패널 배치`} value="" onChange={event => {
              const [target, placement] = event.target.value.split(":") as [DockPanel, DockPlacement];
              if (available.includes(target)) move(id, target, placement);
            }}><option value="">배치…</option>{available.filter(target => target !== id).map(target => <optgroup key={target} label={labels[target]}>{(Object.keys(placements) as DockPlacement[]).map(placement => <option key={placement} value={`${target}:${placement}`}>{labels[target]} {placements[placement]}</option>)}</optgroup>)}</select>
            <button type="button" aria-label={`${labels[id]} 패널에 집중`} disabled={eligible.length === 1} onClick={() => { resizing.current = null; setLayout(current => focusDockPanel(current, id)); }}>⛶</button>
            <button type="button" aria-label={`${labels[id]} 패널 접기`} disabled={eligible.length === 1} onClick={() => { resizing.current = null; setLayout(current => toggleDockPanel(current, id, available)); }}>−</button>
          </div>
          <div className="m5-dock-content" id={`${tabId}-content-${id}`} role="tabpanel" aria-labelledby={`${tabId}-${id}-tab-${id}`}>{panels[id]}</div>
          {drop?.target === id && <div className={`m5-dock-drop m5-dock-drop-${drop.placement}`} aria-hidden="true">{placements[drop.placement]}</div>}
        </div>;
      })}
      {separators.map(separator => <div key={separator.path} className={`m5-dock-separator m5-dock-split-${separator.axis}`} style={position(separator.rect)} role="separator" tabIndex={0} aria-label={`${separator.axis === "horizontal" ? "좌우" : "상하"} 패널 크기 조절`} aria-orientation={separator.axis === "horizontal" ? "vertical" : "horizontal"} aria-valuenow={Math.round(separator.ratio * 100)} aria-valuemin={10} aria-valuemax={90}
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
