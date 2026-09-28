import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { activeDockTab, defaultDockLayout, DOCK_STORAGE_KEY, focusDockPanel, moveDockPanel, parseDockLayout, resizeDockPanels, toggleDockPanel, visibleDockPanels, type DockLayout, type DockPanel } from "./dock-layout.ts";

const labels: Record<DockPanel, string> = { files: "파일", editor: "편집기", changes: "변경", conversation: "팀 대화" };
const dragType = "application/x-hello-agent-panel";

export function WorkbenchDock({ children, conversation }: { children: ReactNode[]; conversation?: ReactNode }) {
  const [layout, setLayout] = useState(() => {
    try { return parseDockLayout(localStorage.getItem(DOCK_STORAGE_KEY)); } catch { return defaultDockLayout(); }
  });
  const container = useRef<HTMLElement>(null);
  const tabId = useId();
  const resizing = useRef<{ x: number; width: number; weight: number; layout: DockLayout; left: DockPanel; right: DockPanel } | null>(null);
  useEffect(() => { try { localStorage.setItem(DOCK_STORAGE_KEY, JSON.stringify(layout)); } catch { /* In-memory layout remains usable when persistence is unavailable. */ } }, [layout]);
  const panels: Record<DockPanel, ReactNode> = { files: children[0], editor: children[1], changes: children[2], conversation };
  const available = layout.order.filter(id => Boolean(panels[id]));
  const eligible = visibleDockPanels(layout, available);
  const active = activeDockTab(layout, eligible);
  const tabbed = layout.tabbed === true;
  const visible = tabbed ? eligible.filter(id => id === active) : eligible;
  const weight = visible.reduce((sum, id) => sum + layout.widths[id], 0);
  const vertical = !tabbed && layout.direction === "vertical";
  const columns = visible.map(id => `minmax(0, ${layout.widths[id]}fr)`).join(" 8px ");
  return <>
    <div className="m5-dock-help"><span>제목을 끌어 배치하고 경계선을 움직여 크기를 바꿀 수 있어요.</span><button type="button" onClick={() => setLayout(defaultDockLayout())}>기본 배치로</button></div>
    <nav className="m5-dock-visibility" aria-label="패널 표시">
      <button type="button" aria-pressed={vertical} onClick={() => { resizing.current = null; setLayout(current => ({ ...current, tabbed: false, direction: vertical ? "horizontal" : "vertical" })); }}>세로 배치</button>
      <button type="button" aria-pressed={tabbed} onClick={() => { resizing.current = null; setLayout(current => ({ ...current, tabbed: !current.tabbed })); }}>탭 배치</button>
      {available.map(id => <button key={id} type="button" aria-pressed={eligible.includes(id)} disabled={eligible.length === 1 && eligible.includes(id)} onClick={() => setLayout(current => toggleDockPanel(current, id, available))}>{labels[id]}</button>)}
    </nav>
    {tabbed && <div className="m5-dock-tabs" role="tablist" aria-label="작업 패널 탭">
      {eligible.map((id, index) => <button key={id} id={`${tabId}-tab-${id}`} type="button" role="tab" aria-controls={`${tabId}-panel-${id}`} aria-selected={id === active} tabIndex={id === active ? 0 : -1}
        onClick={() => setLayout(current => ({ ...current, activeTab: id }))}
        onKeyDown={event => {
          const next = event.key === "Home" ? 0 : event.key === "End" ? eligible.length - 1 : event.key === "ArrowRight" ? (index + 1) % eligible.length : event.key === "ArrowLeft" ? (index + eligible.length - 1) % eligible.length : -1;
          if (next < 0) return;
          event.preventDefault();
          setLayout(current => ({ ...current, activeTab: eligible[next] }));
          (event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next])?.focus();
        }} draggable onDragStart={event => { event.dataTransfer.setData(dragType, id); event.dataTransfer.effectAllowed = "move"; }}
        onDragOver={event => { if (event.dataTransfer.types.includes(dragType)) event.preventDefault(); }}
        onDrop={event => { const source = event.dataTransfer.getData(dragType) as DockPanel; if (!eligible.includes(source)) return; event.preventDefault(); setLayout(current => moveDockPanel(current, source, id)); }}
      >{labels[id]}</button>)}
    </div>}
    <section ref={container} className={`m5-panels m5-dock${vertical ? " m5-dock-vertical" : ""}`} aria-label="재배치 가능한 작업 패널" style={vertical ? { gridTemplateColumns: "minmax(0, 1fr)", gridTemplateRows: columns } : { gridTemplateColumns: columns }}>
      {available.flatMap(id => {
        const index = visible.indexOf(id);
        const shown = index >= 0;
        const right = visible[index + 1];
        const panel = <div className="m5-dock-slot" key={id} data-panel={id} id={`${tabId}-panel-${id}`} role={tabbed ? "tabpanel" : undefined} aria-labelledby={tabbed ? `${tabId}-tab-${id}` : undefined} hidden={!shown} style={!shown ? { display: "none" } : undefined} onDragOver={event => { if (event.dataTransfer.types.includes(dragType)) event.preventDefault(); }} onDrop={event => {
          const source = event.dataTransfer.getData(dragType) as DockPanel;
          if (!visible.includes(source)) return;
          event.preventDefault(); setLayout(current => moveDockPanel(current, source, id));
        }}>
          <div className="m5-dock-heading" style={tabbed ? { display: "none" } : undefined}>
            <button type="button" className="m5-dock-handle" draggable onDragStart={event => { event.dataTransfer.setData(dragType, id); event.dataTransfer.effectAllowed = "move"; }} aria-label={`${labels[id]} 패널 끌기`}>⠿ {labels[id]}</button>
            <button type="button" aria-label={`${labels[id]} 패널 ${vertical ? "위로" : "왼쪽으로"}`} disabled={index === 0} onClick={() => setLayout(current => moveDockPanel(current, id, visible[index - 1]))}>{vertical ? "↑" : "←"}</button>
            <button type="button" aria-label={`${labels[id]} 패널 ${vertical ? "아래로" : "오른쪽으로"}`} disabled={!right} onClick={() => setLayout(current => moveDockPanel(current, id, right))}>{vertical ? "↓" : "→"}</button>
            <button type="button" aria-label={`${labels[id]} 패널에 집중`} disabled={visible.length === 1} onClick={() => setLayout(current => focusDockPanel(current, id))}>⛶</button>
            <button type="button" aria-label={`${labels[id]} 패널 접기`} disabled={visible.length === 1} onClick={() => setLayout(current => toggleDockPanel(current, id, available))}>−</button>
          </div>
          {panels[id]}
        </div>;
        if (!shown || !right) return [panel];
        return [panel, <div key={`separator-${id}-${right}`} className="m5-dock-separator" role="separator" tabIndex={0} aria-label={`${labels[id]}와 ${labels[right]} 크기 조절`} aria-orientation={vertical ? "horizontal" : "vertical"} aria-valuenow={Math.round(layout.widths[id] / weight * 100)} onPointerDown={event => {
          if (event.button !== 0) return;
          const bounds = container.current?.getBoundingClientRect();
          const width = (vertical ? bounds?.height ?? 0 : bounds?.width ?? 0) - (visible.length - 1) * 8;
          if (width <= 0) return;
          resizing.current = { x: vertical ? event.clientY : event.clientX, width, weight, layout, left: id, right };
          event.currentTarget.setPointerCapture(event.pointerId); event.preventDefault();
        }} onPointerMove={event => {
          const start = resizing.current;
          if (start) setLayout(resizeDockPanels(start.layout, start.left, start.right, ((vertical ? event.clientY : event.clientX) - start.x) / start.width * start.weight));
        }} onPointerUp={() => { resizing.current = null; }} onPointerCancel={() => { resizing.current = null; }} onLostPointerCapture={() => { resizing.current = null; }} onKeyDown={event => {
          const decrease = vertical ? "ArrowUp" : "ArrowLeft";
          const increase = vertical ? "ArrowDown" : "ArrowRight";
          if (event.key !== decrease && event.key !== increase) return;
          event.preventDefault(); setLayout(current => resizeDockPanels(current, id, right, (event.key === increase ? .02 : -.02) * weight));
        }} />];
      })}
    </section>
  </>;
}
