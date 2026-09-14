import { useEffect, useRef, useState, type ReactNode } from "react";
import { defaultDockLayout, DOCK_STORAGE_KEY, moveDockPanel, parseDockLayout, resizeDockPanels, type DockLayout, type DockPanel } from "./dock-layout.ts";

const labels: Record<DockPanel, string> = { files: "파일", editor: "편집기", changes: "변경", conversation: "팀 대화" };
const dragType = "application/x-hello-agent-panel";

export function WorkbenchDock({ children, conversation }: { children: ReactNode[]; conversation?: ReactNode }) {
  const [layout, setLayout] = useState(() => {
    try { return parseDockLayout(localStorage.getItem(DOCK_STORAGE_KEY)); } catch { return defaultDockLayout(); }
  });
  const container = useRef<HTMLElement>(null);
  const resizing = useRef<{ x: number; width: number; weight: number; layout: DockLayout; left: DockPanel; right: DockPanel } | null>(null);
  useEffect(() => { try { localStorage.setItem(DOCK_STORAGE_KEY, JSON.stringify(layout)); } catch { /* In-memory layout remains usable when persistence is unavailable. */ } }, [layout]);
  const panels: Record<DockPanel, ReactNode> = { files: children[0], editor: children[1], changes: children[2], conversation };
  const visible = layout.order.filter(id => Boolean(panels[id]));
  const weight = visible.reduce((sum, id) => sum + layout.widths[id], 0);
  const columns = visible.map(id => `minmax(0, ${layout.widths[id]}fr)`).join(" 8px ");
  return <>
    <div className="m5-dock-help"><span>제목을 끌어 배치하고 경계선을 움직여 크기를 바꿀 수 있어요.</span><button type="button" onClick={() => setLayout(defaultDockLayout())}>기본 배치로</button></div>
    <section ref={container} className="m5-panels m5-dock" aria-label="재배치 가능한 작업 패널" style={{ gridTemplateColumns: columns }}>
      {visible.flatMap((id, index) => {
        const right = visible[index + 1];
        const panel = <div className="m5-dock-slot" key={id} data-panel={id} onDragOver={event => { if (event.dataTransfer.types.includes(dragType)) event.preventDefault(); }} onDrop={event => {
          const source = event.dataTransfer.getData(dragType) as DockPanel;
          if (!visible.includes(source)) return;
          event.preventDefault(); setLayout(current => moveDockPanel(current, source, id));
        }}>
          <div className="m5-dock-heading">
            <button type="button" className="m5-dock-handle" draggable onDragStart={event => { event.dataTransfer.setData(dragType, id); event.dataTransfer.effectAllowed = "move"; }} aria-label={`${labels[id]} 패널 끌기`}>⠿ {labels[id]}</button>
            <button type="button" aria-label={`${labels[id]} 패널 왼쪽으로`} disabled={index === 0} onClick={() => setLayout(current => moveDockPanel(current, id, visible[index - 1]))}>←</button>
            <button type="button" aria-label={`${labels[id]} 패널 오른쪽으로`} disabled={!right} onClick={() => setLayout(current => moveDockPanel(current, id, right))}>→</button>
          </div>
          {panels[id]}
        </div>;
        if (!right) return [panel];
        return [panel, <div key={`separator-${id}-${right}`} className="m5-dock-separator" role="separator" tabIndex={0} aria-label={`${labels[id]}와 ${labels[right]} 크기 조절`} aria-orientation="vertical" aria-valuenow={Math.round(layout.widths[id] / weight * 100)} onPointerDown={event => {
          if (event.button !== 0) return;
          const width = (container.current?.getBoundingClientRect().width ?? 0) - (visible.length - 1) * 8;
          if (width <= 0) return;
          resizing.current = { x: event.clientX, width, weight, layout, left: id, right };
          event.currentTarget.setPointerCapture(event.pointerId); event.preventDefault();
        }} onPointerMove={event => {
          const start = resizing.current;
          if (start) setLayout(resizeDockPanels(start.layout, start.left, start.right, (event.clientX - start.x) / start.width * start.weight));
        }} onPointerUp={() => { resizing.current = null; }} onPointerCancel={() => { resizing.current = null; }} onLostPointerCapture={() => { resizing.current = null; }} onKeyDown={event => {
          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
          event.preventDefault(); setLayout(current => resizeDockPanels(current, id, right, (event.key === "ArrowRight" ? .02 : -.02) * weight));
        }} />];
      })}
    </section>
  </>;
}
