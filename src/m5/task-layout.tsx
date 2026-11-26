import { useEffect, useId, useRef, type CSSProperties, type ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import { bandUsesDrawer, clampRightWidth, keyedRightWidth, rovingIndex, RIGHT_WIDTH_KEY, type LayoutBand, type RightTab } from "./task-layout-model.ts";

export interface RightTabSpec {
  id: RightTab;
  label: string;
  /** A count shown after the label (changes). */
  count?: number;
  /** A small dot: something new is ready here (results while the drawer is closed). */
  dot?: boolean;
  content: ReactNode;
}

/**
 * Three fixed areas (ADE 1차-나): the task list (nav), the task's story
 * (centre) and its results (right), with the wide review sheet over the
 * centre and right. Below 1100px the list is a rail and the results a
 * drawer. Every panel stays mounted and is only hidden, so Monaco and
 * unsaved drafts survive tab changes and the sheet closing.
 */
export function TaskLayout({ band, rightCollapsed = false, previewCovered = true, list, story, tabs, rightTab, onRightTab, resultBox, sheet, sheetOpen, sheetLabel, onSheetClose, drawerOpen, onDrawerOpenChange, rightWidth, onRightWidth }: {
  band: LayoutBand;
  /** ⌘\\ hides the results panel in wide windows. */
  rightCollapsed?: boolean;
  /** Something covers the preview area (task-layout-model previewObscured); read by the preview view (P3). */
  previewCovered?: boolean;
  list: ReactNode;
  story: ReactNode;
  tabs: readonly RightTabSpec[];
  rightTab: RightTab;
  onRightTab: (tab: RightTab) => void;
  resultBox?: ReactNode;
  sheet: ReactNode;
  sheetOpen: boolean;
  sheetLabel: string;
  onSheetClose: () => void;
  drawerOpen: boolean;
  onDrawerOpenChange: (open: boolean) => void;
  rightWidth: number;
  onRightWidth: (width: number) => void;
}) {
  const { t } = useI18n();
  const id = useId();
  const drawer = bandUsesDrawer(band);
  const sheetRef = useRef<HTMLElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  // Opening the sheet moves focus into it; closing gives focus back to what
  // opened it (PLAN 1.6).
  useEffect(() => {
    if (sheetOpen) {
      if (!opener.current) opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      requestAnimationFrame(() => {
        const root = sheetRef.current;
        if (!root || root.contains(document.activeElement)) return;
        root.querySelector<HTMLElement>("[data-sheet-focus], button:not(:disabled), [tabindex='0']")?.focus();
      });
    } else if (opener.current) {
      const back = opener.current;
      opener.current = null;
      if (back.isConnected) back.focus();
    }
  }, [sheetOpen]);
  // Escape closes the sheet (or the drawer) wherever focus is, unless
  // something inside took it first: Monaco's suggestions and find prevent
  // it, and an open dialog or menu closes on its own.
  const closeSheet = useRef(onSheetClose);
  closeSheet.current = onSheetClose;
  const closeDrawer = useRef(() => onDrawerOpenChange(false));
  closeDrawer.current = () => onDrawerOpenChange(false);
  useEffect(() => {
    if (!sheetOpen && !(drawer && drawerOpen)) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest("dialog[open], details[open], [popover]")) return;
      if (document.querySelector("dialog[open]")) return;
      event.preventDefault();
      if (sheetOpen) closeSheet.current(); else closeDrawer.current();
    };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [sheetOpen, drawer, drawerOpen]);
  const tabIndex = Math.max(0, tabs.findIndex(tab => tab.id === rightTab));
  const resizing = useRef<{ x: number; width: number } | null>(null);
  const saveWidth = (width: number) => { try { localStorage.setItem(RIGHT_WIDTH_KEY, String(width)); } catch { /* optional */ } };
  const style = drawer ? undefined : { "--right-w": rightCollapsed ? "0px" : `${rightWidth}px` } as CSSProperties;
  const rightHidden = drawer ? !drawerOpen : rightCollapsed;
  return <div className="m5-task-layout" data-testid="task-layout" data-band={band} data-sheet={sheetOpen ? "open" : "closed"} data-drawer={drawer ? (drawerOpen ? "open" : "closed") : undefined} data-preview-covered={previewCovered ? "true" : "false"} style={style}>
    <nav className="m5-layout-left" data-testid="layout-left" aria-label={t("m5.layout.list")}>{list}</nav>
    <section className="m5-layout-center" data-testid="layout-center" aria-label={t("m5.layout.story")} inert={sheetOpen || (drawer && drawerOpen) || undefined}>{story}</section>
    {drawer && drawerOpen && <div className="m5-layout-scrim" aria-hidden="true" onClick={() => onDrawerOpenChange(false)} />}
    <aside className="m5-layout-right" data-testid="layout-right" aria-label={t("m5.layout.results")} hidden={rightHidden} inert={sheetOpen || undefined}>
      {!drawer && <div className="m5-layout-resize" role="separator" tabIndex={0} aria-orientation="vertical" aria-label={t("m5.layout.resize")} aria-valuenow={rightWidth} aria-valuemin={320}
        onPointerDown={event => { if (event.button !== 0) return; resizing.current = { x: event.clientX, width: rightWidth }; event.currentTarget.setPointerCapture(event.pointerId); event.preventDefault(); }}
        onPointerMove={event => { const start = resizing.current; if (start) onRightWidth(clampRightWidth(start.width + start.x - event.clientX, window.innerWidth)); }}
        onPointerUp={() => { if (resizing.current) saveWidth(rightWidth); resizing.current = null; }}
        onPointerCancel={() => { resizing.current = null; }}
        onKeyDown={event => { const next = keyedRightWidth(rightWidth, event.key, event.shiftKey, window.innerWidth); if (next === null) return; event.preventDefault(); onRightWidth(next); saveWidth(next); }} />}
      <div className="m5-layout-tabs" role="tablist" aria-label={t("m5.layout.results")}>
        {tabs.map((tab, index) => <button key={tab.id} type="button" role="tab" id={`${id}-tab-${tab.id}`} aria-controls={`${id}-panel-${tab.id}`} data-testid={`right-tab-${tab.id}`}
          aria-selected={tab.id === rightTab} tabIndex={index === tabIndex ? 0 : -1}
          onClick={() => onRightTab(tab.id)}
          onKeyDown={event => {
            const next = rovingIndex(tabs.length, index, event.key);
            if (next === null) return;
            event.preventDefault();
            onRightTab(tabs[next]!.id);
            requestAnimationFrame(() => document.getElementById(`${id}-tab-${tabs[next]!.id}`)?.focus());
          }}>
          {tab.label}{tab.count !== undefined && <span className="m5-layout-count">{tab.count}</span>}{tab.dot && <span className="m5-layout-dot" aria-label={t("m5.layout.ready")} />}
        </button>)}
        {drawer && <button type="button" className="m5-layout-drawer-close" aria-label={t("m5.layout.closeDrawer")} title={t("m5.layout.closeDrawer")} onClick={() => onDrawerOpenChange(false)}>×</button>}
      </div>
      {tabs.map(tab => <div key={tab.id} className="m5-layout-tabpanel" role="tabpanel" id={`${id}-panel-${tab.id}`} aria-labelledby={`${id}-tab-${tab.id}`} data-testid={`right-panel-${tab.id}`} hidden={tab.id !== rightTab}>{tab.content}</div>)}
      {resultBox}
    </aside>
    <section ref={sheetRef} className="m5-review-sheet" data-testid="review-sheet" aria-label={sheetLabel} hidden={!sheetOpen}>{sheet}</section>
  </div>;
}
