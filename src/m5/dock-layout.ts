export const PANEL_IDS = ["files", "editor", "changes", "conversation"] as const;
export type DockPanel = typeof PANEL_IDS[number];
export interface DockLayout { version: 1; order: DockPanel[]; widths: Record<DockPanel, number>; hidden?: DockPanel[]; direction?: "horizontal" | "vertical" }
export const DOCK_STORAGE_KEY = "hello-agent.m5.dock.v1";
export function defaultDockLayout(): DockLayout {
  return { version: 1, order: [...PANEL_IDS], widths: { files: .17, editor: .36, changes: .21, conversation: .26 } };
}
export function parseDockLayout(raw: string | null): DockLayout {
  try {
    const value = JSON.parse(raw ?? "null");
    if (value?.version !== 1 || !Array.isArray(value.order) || value.order.length !== 4 || new Set(value.order).size !== 4 || value.order.some((id: unknown) => !PANEL_IDS.includes(id as DockPanel))) return defaultDockLayout();
    if (!value.widths || PANEL_IDS.some(id => !Number.isFinite(value.widths[id]) || value.widths[id] < .08 || value.widths[id] > .76)) return defaultDockLayout();
    if (Math.abs(PANEL_IDS.reduce((sum, id) => sum + value.widths[id], 0) - 1) > .000001) return defaultDockLayout();
    if (value.hidden !== undefined && (!Array.isArray(value.hidden) || value.hidden.length >= PANEL_IDS.length || new Set(value.hidden).size !== value.hidden.length || value.hidden.some((id: unknown) => !PANEL_IDS.includes(id as DockPanel)))) return defaultDockLayout();
    if (value.direction !== undefined && value.direction !== "horizontal" && value.direction !== "vertical") return defaultDockLayout();
    return { version: 1, order: [...value.order], widths: Object.fromEntries(PANEL_IDS.map(id => [id, value.widths[id]])) as Record<DockPanel, number>, ...(value.hidden ? { hidden: [...value.hidden] } : {}), ...(value.direction ? { direction: value.direction } : {}) };
  } catch { return defaultDockLayout(); }
}

export function visibleDockPanels(layout: DockLayout, available: readonly DockPanel[]): DockPanel[] {
  const visible = layout.order.filter(id => available.includes(id) && !layout.hidden?.includes(id));
  // A chat can disappear while only its conversation was visible. Always retain a usable panel.
  return visible.length ? visible : available.includes("editor") ? ["editor"] : available.slice(0, 1);
}

export function toggleDockPanel(layout: DockLayout, panel: DockPanel, available: readonly DockPanel[]): DockLayout {
  if (!available.includes(panel)) return layout;
  const visible = visibleDockPanels(layout, available);
  if (visible.includes(panel) && visible.length === 1) return layout;
  const hidden = new Set(layout.hidden ?? []);
  // Materialize the fallback visible panel before updating the saved choice.
  for (const id of visible) hidden.delete(id);
  if (visible.includes(panel)) hidden.add(panel); else hidden.delete(panel);
  return { ...layout, hidden: [...hidden] };
}

export function focusDockPanel(layout: DockLayout, panel: DockPanel): DockLayout {
  return PANEL_IDS.includes(panel) ? { ...layout, hidden: PANEL_IDS.filter(id => id !== panel) } : layout;
}
export function moveDockPanel(layout: DockLayout, source: DockPanel, target: DockPanel): DockLayout {
  if (source === target || !layout.order.includes(source) || !layout.order.includes(target)) return layout;
  const order = layout.order.filter(id => id !== source);
  order.splice(layout.order.indexOf(target), 0, source);
  return { ...layout, order };
}
export function resizeDockPanels(layout: DockLayout, left: DockPanel, right: DockPanel, delta: number): DockLayout {
  if (!Number.isFinite(delta) || left === right || !PANEL_IDS.includes(left) || !PANEL_IDS.includes(right)) return layout;
  const pair = layout.widths[left] + layout.widths[right];
  const width = Math.max(.08, Math.min(pair - .08, layout.widths[left] + delta));
  return { ...layout, widths: { ...layout.widths, [left]: width, [right]: Math.max(.08, pair - width) } };
}
